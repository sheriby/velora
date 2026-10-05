//! Undo history and selection snapshot restoration.

use super::source_mapping::clip_hit_to_span;
use super::*;

impl Editor {
    pub(super) fn empty_selection_snapshot() -> UndoSelectionSnapshot {
        UndoSelectionSnapshot {
            range: 0..0,
            reversed: false,
        }
    }

    pub(super) fn capture_source_selection_snapshot(&self, cx: &App) -> UndoSelectionSnapshot {
        if let Some(snapshot) = self.cross_block_source_selection_snapshot(cx) {
            return snapshot;
        }

        if self.view_mode == ViewMode::Source {
            // 源码视图按 512 行切成若干投影块，落点得按缓冲区偏移记账：
            // 「所在块的起点 + 块内偏移」。此前固定读第一根的块内偏移——光标
            // 落在第 2 块之后就把落点说成文件开头，撤销、切视图、外部改动重载
            // 都以这份快照为锚，一处读错就一路错。
            let target = self
                .current_edit_target_from_state(cx)
                .or_else(|| self.document.first_root().cloned());
            return target
                .map(|block| {
                    let chunk_start = self
                        .document
                        .source_span_of(block.entity_id())
                        .map(|span| span.start)
                        .unwrap_or(0);
                    let block_ref = block.read(cx);
                    let local = block_ref.selected_range.clone();
                    UndoSelectionSnapshot {
                        range: chunk_start + local.start..chunk_start + local.end,
                        reversed: block_ref.selection_reversed,
                    }
                })
                .unwrap_or_else(Self::empty_selection_snapshot);
        }

        let Some(target) = self.current_edit_target_from_state(cx) else {
            return self.last_selection_snapshot.clone();
        };
        let Some(mapping) = self.source_mapping_for_entity(target.entity_id(), cx) else {
            return self.last_selection_snapshot.clone();
        };

        let selected_range = target.read(cx).selected_range.clone();
        let content_range = target
            .read(cx)
            .current_range_to_markdown_range(selected_range);
        let max_offset = mapping.content_to_source.len().saturating_sub(1);
        let start = mapping.full_source_range.start
            + mapping.content_to_source[content_range.start.min(max_offset)];
        let end = mapping.full_source_range.start
            + mapping.content_to_source[content_range.end.min(max_offset)];

        UndoSelectionSnapshot {
            range: start..end,
            reversed: target.read(cx).selection_reversed,
        }
    }

    /// 撤销栈占用的字节数（条目负载，不含结构体本身）。
    ///
    /// 这是「撤销存增量而不是存全文」那条性质的闸门：正常打字与拆块之后，
    /// 它不该随文档大小增长。只有测试闸门读它。
    #[cfg(test)]
    pub(crate) fn undo_history_byte_len(&self) -> usize {
        self.undo_history
            .iter()
            .map(|entry| entry.byte_len())
            .sum::<usize>()
            + self
                .redo_history
                .iter()
                .map(|entry| entry.byte_len())
                .sum::<usize>()
    }

    /// 开一个撤销组：只记下「改动前的选区」，不复制任何文本。
    ///
    /// 组里的字节增量由 [`Editor::record_buffer_edit`] 在每次 `TextBuffer::edit`
    /// 落地后追加，所以准备阶段的工作量与文档大小无关。
    pub(super) fn begin_history_group(&mut self, kind: UndoCaptureKind, cx: &App) -> HistoryEntry {
        HistoryEntry {
            edits: Vec::new(),
            // 选区现场算：调用点是编辑开始前，算出来就是编辑前的选区。
            selection: self.capture_source_selection_snapshot(cx),
            timestamp: Instant::now(),
            kind,
        }
    }

    /// 把当前撤销组留到本次派发批处理结束再结算。
    ///
    /// 一次用户动作会连着发出好几个事件（回车先截断本块，再请求拆块），逐条结算
    /// 就会把一次动作切成两个撤销步——第一次撤销停在「文本截断了但第二块还没落地」
    /// 的中间态。推迟到批处理末尾，同一动作的增量自然落进同一个组。
    pub(super) fn finalize_pending_undo_capture_at_end_of_batch(&mut self, cx: &mut Context<Self>) {
        let this = cx.entity().clone();
        cx.defer(move |cx| {
            let _ = this.update(cx, |editor, cx| editor.finalize_pending_undo_capture(cx));
        });
    }

    /// 把一次已经落地的缓冲区写入记进当前撤销组。
    ///
    /// 撤销时按反序重放，所以这里只管追加，不需要合并区间。
    ///
    /// **不变式**：用户可见的每一次编辑都必须被某个打开的组记账——输入路径由
    /// `prepare_undo_capture` 开组，模型直改由 `BlockEvent::Changed` 臂补开组，
    /// 结构事件各自显式 prepare。无组时到达这里的写入只有两类合法来源：
    /// 撤销/重放（`history_restore_in_progress`）与 `mark_dirty` 程序化重同步
    /// （「树说了算」的声明式写入，不构成可撤销的用户动作）。新增写入路径时
    /// 必须先开组再落笔，否则撤销的增量坐标会静默错位。
    pub(crate) fn record_buffer_edit(&mut self, applied: buffer::AppliedEdit) {
        if self.history_restore_in_progress {
            return;
        }
        if let Some(pending) = self.pending_undo_capture.as_mut() {
            pending.snapshot.edits.push(applied);
        }
    }

    /// 这组逆操作重放回当前状态吗（也就是「改了等于没改」）。
    /// 撤销组里可能有多条，后面的条目会把前面的区间挪位，所以「改了等于没改」
    /// 要在一份副本上真重放一遍，不能拿当前缓冲区去套每条区间。
    fn history_group_is_noop(buffer: &buffer::TextBuffer, edits: &[buffer::AppliedEdit]) -> bool {
        let mut probe = buffer.clone();
        for edit in edits.iter().rev() {
            if edit.new_range.end > probe.byte_len()
                || !probe.is_char_boundary(edit.new_range.start)
                || !probe.is_char_boundary(edit.new_range.end)
            {
                return false;
            }
            probe.edit(edit.new_range.clone(), &edit.removed);
        }
        probe.same_content(buffer)
    }

    pub(super) fn prepare_undo_capture(&mut self, kind: UndoCaptureKind, cx: &mut Context<Self>) {
        if self.history_restore_in_progress || self.pending_undo_capture.is_some() {
            return;
        }
        self.pending_undo_capture = Some(PendingUndoCapture {
            snapshot: self.begin_history_group(kind, cx),
        });
    }

    /// 帧级选区快照刷新：只在选区（或活动块）真的变了时重算。
    ///
    /// `capture_source_selection_snapshot` 在渲染模式下要从文档第一块走到
    /// 光标块重建 source mapping，大文档里每帧都算一次就是白付 O(文档) 成本；
    /// 这里只管选区移动，文档内容变化由编辑路径自己处理。
    pub(super) fn refresh_selection_snapshot_if_changed(&mut self, cx: &App) {
        let Some(target) = self.current_edit_target_from_state(cx) else {
            return;
        };
        let current = (target.entity_id(), target.read(cx).selected_range.clone());
        if self.last_selection_snapshot_source.as_ref() == Some(&current) {
            return;
        }
        self.last_selection_snapshot_source = Some(current);
        self.last_selection_snapshot = self.capture_source_selection_snapshot(cx);
    }

    pub(super) fn finalize_pending_undo_capture(&mut self, _cx: &mut Context<Self>) {
        if self.history_restore_in_progress {
            self.pending_undo_capture = None;
            return;
        }

        let Some(pending) = self.pending_undo_capture.take() else {
            return;
        };

        // 这次改动没落下任何字节增量：撤销栈不该多出空条目。
        if pending.snapshot.edits.is_empty() {
            return;
        }

        // 组合进行中的每次更新各开一条 ImeComposition（见 input.rs）：撤销整次
        // 组合必须一次退干净，相邻的组合条目按时间序并成一条——撤销时反序重放，
        // 顺序天然正确。
        if pending.snapshot.kind == UndoCaptureKind::ImeComposition
            && self
                .undo_history
                .last()
                .is_some_and(|entry| entry.kind == UndoCaptureKind::ImeComposition)
        {
            self.redo_history.clear();
            self.undo_history
                .last_mut()
                .expect("checked above")
                .edits
                .extend(pending.snapshot.edits);
            return;
        }

        if pending.snapshot.kind == UndoCaptureKind::ImeCompositionCommit
            && self
                .undo_history
                .last()
                .is_some_and(|entry| entry.kind == UndoCaptureKind::ImeComposition)
        {
            // 组合输入收尾时文本又回到了组合开始前：那条记录没有存在意义。
            //
            // 增量式撤销组里，「组合」和「收尾」是两条各自记录了一次缓冲区写入的
            // 条目，撤销整次组合输入必须一次退干净，所以先把收尾的增量并进来，
            // 再拿合并后的整组判断「改了等于没改」。
            self.redo_history.clear();
            self.undo_history
                .last_mut()
                .expect("checked above")
                .edits
                .extend(pending.snapshot.edits);
            let noop = Self::history_group_is_noop(
                &self.buffer,
                &self.undo_history.last().expect("checked above").edits,
            );
            if noop {
                self.undo_history.pop();
            } else {
                self.undo_history.last_mut().expect("checked above").kind =
                    UndoCaptureKind::NonCoalescible;
            }
            return;
        }

        // A fresh edit invalidates any forward history available for redo.
        self.redo_history.clear();

        let should_merge = matches!(pending.snapshot.kind, UndoCaptureKind::CoalescibleText)
            && self.undo_history.last().is_some_and(|entry| {
                matches!(entry.kind, UndoCaptureKind::CoalescibleText)
                    && pending
                        .snapshot
                        .timestamp
                        .saturating_duration_since(entry.timestamp)
                        <= Self::HISTORY_COALESCE_WINDOW
            });
        if should_merge {
            // 合并 = 把两组增量接起来：撤销时整组反序重放，等价于逐步回退。
            self.undo_history
                .last_mut()
                .expect("checked above")
                .edits
                .extend(pending.snapshot.edits);
        } else {
            self.undo_history.push(pending.snapshot);
            if self.undo_history.len() > Self::HISTORY_LIMIT {
                let overflow = self.undo_history.len() - Self::HISTORY_LIMIT;
                self.undo_history.drain(0..overflow);
            }
        }
    }

    pub(super) fn apply_selection_snapshot_in_current_mode(
        &mut self,
        snapshot: &UndoSelectionSnapshot,
        cx: &mut Context<Self>,
    ) {
        // 这里的偏移来自**上一份**内容（撤销快照、切视图前记的、标签上次离开时的、
        // 外部改动之前的）：文档插过或删过字节之后，那个字节位可能正落在多字节字符
        // 中间，而这条链路上的行号与块区间换算处处按字符边界走（`line_of` 会直接
        // 断言失败）。两端各自退回边界；取整单调，所以 `start <= end` 不会被破坏。
        let clamped = UndoSelectionSnapshot {
            range: self.buffer.floor_char_boundary(snapshot.range.start)
                ..self.buffer.floor_char_boundary(snapshot.range.end),
            reversed: snapshot.reversed,
        };
        let snapshot = &clamped;
        match self.view_mode {
            ViewMode::Source => {
                // 源码文档按 512 行切成多根投影块（SOURCE_DOCUMENT_CHUNK_LINES）：
                // 选区必须写进**包含落点的那一根**。以前一律钳在第一根块的长度
                // 里——文件超过 512 行后，512 行之外的搜索命中、大纲与 `[TOC]`
                // 跳转全部落在第一块末尾（用户报修：点击都停在 512 行）。
                let mappings = self.source_mappings_in_range(&snapshot.range, cx);
                let target = mappings
                    .iter()
                    .find(|mapping| {
                        Self::source_range_contains(&mapping.full_source_range, snapshot.range.start)
                    })
                    .or_else(|| {
                        // 落点在块与块之间的换行上时按就近取；没有投影块
                        // （刚插入、还没写回缓冲区的块）退回第一根块的老口径。
                        mappings.iter().min_by_key(|mapping| {
                            Self::source_offset_distance(
                                &mapping.full_source_range,
                                snapshot.range.start,
                            )
                        })
                    });
                let Some(mapping) = target else {
                    let Some(block) = self.document.first_root().cloned() else {
                        return;
                    };
                    let len = block.read(cx).visible_len();
                    let selected_range =
                        snapshot.range.start.min(len)..snapshot.range.end.min(len);
                    block.update(cx, move |block, cx| {
                        block.selected_range = selected_range.clone();
                        block.selection_reversed = snapshot.reversed;
                        block.marked_range = None;
                        block.vertical_motion_x = None;
                        block.cursor_blink_epoch = Instant::now();
                        cx.notify();
                    });
                    self.pending_focus = Some(block.entity_id());
                    self.active_entity_id = Some(block.entity_id());
                    return;
                };
                // 真跨块选区（D7）：源码视图的投影块就是缓冲区的连续切片，映射是
                // 恒等的，所以被这个选区盖住的每一块各拿自己那截，不再钳进起点所在
                // 的那一根。端点先裁后减：`source_mappings_in_range` 会连带返回区间
                // 外的相邻块（它按 previous/next 各补一根），旧写法在那种块上直接
                // `snapshot.range.start - chunk_start` 会下溢。
                let anchor_id = mapping.entity.entity_id();
                let mut covered_ids: Vec<EntityId> = Vec::new();
                for mapping in &mappings {
                    let chunk = &mapping.full_source_range;
                    let Some(local) = clip_hit_to_span(&snapshot.range, chunk.start, chunk.end)
                    else {
                        continue;
                    };
                    let chunk_len = chunk.len();
                    let selected = (local.start - chunk.start).min(chunk_len)
                        ..(local.end - chunk.start).min(chunk_len);
                    let is_anchor = mapping.entity.entity_id() == anchor_id;
                    let entity = mapping.entity.clone();
                    covered_ids.push(entity.entity_id());
                    entity.update(cx, move |block, cx| {
                        block.selected_range = selected;
                        block.selection_reversed = snapshot.reversed;
                        block.marked_range = None;
                        if is_anchor {
                            block.vertical_motion_x = None;
                            block.cursor_blink_epoch = Instant::now();
                        }
                        cx.notify();
                    });
                }
                // 上一次选区留在别的投影块上的残留要清掉，否则跨块选区收小之后
                // 远处那块还画着旧的高亮。
                for block in self.document.root_blocks() {
                    if covered_ids.contains(&block.entity_id()) {
                        continue;
                    }
                    block.update(cx, |block, cx| {
                        if !block.selected_range.is_empty() {
                            block.selected_range = 0..0;
                            cx.notify();
                        }
                    });
                }
                self.pending_focus = Some(anchor_id);
                self.active_entity_id = Some(anchor_id);
            }
            ViewMode::Rendered => {
                if self.apply_cross_block_selection_snapshot_if_possible(snapshot, cx) {
                    return;
                }

                let mappings = self.source_mappings_in_range(&snapshot.range, cx);
                let exact_mapping = mappings.iter().find(|mapping| {
                    let contains_start = Self::source_range_contains(
                        &mapping.full_source_range,
                        snapshot.range.start,
                    );
                    let contains_end =
                        Self::source_range_contains(&mapping.full_source_range, snapshot.range.end);
                    if !contains_start || !contains_end {
                        return false;
                    }
                    let local_start = snapshot
                        .range
                        .start
                        .saturating_sub(mapping.full_source_range.start);
                    let local_end = snapshot
                        .range
                        .end
                        .saturating_sub(mapping.full_source_range.start);
                    let content_start = mapping.source_to_content
                        [local_start.min(mapping.source_to_content.len().saturating_sub(1))];
                    let content_end = mapping.source_to_content
                        [local_end.min(mapping.source_to_content.len().saturating_sub(1))];
                    let max_content = mapping.content_to_source.len().saturating_sub(1);
                    mapping.content_to_source[content_start.min(max_content)] == local_start
                        && mapping.content_to_source[content_end.min(max_content)] == local_end
                });

                if let Some(mapping) = exact_mapping {
                    let local_start = snapshot.range.start - mapping.full_source_range.start;
                    let local_end = snapshot.range.end - mapping.full_source_range.start;
                    let content_start = mapping.source_to_content[local_start];
                    let content_end = mapping.source_to_content[local_end];
                    let selected_range = mapping
                        .entity
                        .read(cx)
                        .markdown_range_to_current_range(content_start..content_end);
                    mapping.entity.update(cx, move |block, cx| {
                        block.selected_range = selected_range.clone();
                        block.selection_reversed = snapshot.reversed;
                        block.marked_range = None;
                        block.vertical_motion_x = None;
                        block.cursor_blink_epoch = Instant::now();
                        cx.notify();
                    });
                    self.pending_focus = Some(mapping.entity.entity_id());
                    self.active_entity_id = Some(mapping.entity.entity_id());
                    return;
                }

                let caret_offset = snapshot.range.end;
                let best = mappings.iter().min_by_key(|mapping| {
                    Self::source_offset_distance(&mapping.full_source_range, caret_offset)
                });
                let Some(mapping) = best else {
                    self.pending_focus = self.first_focusable_entity_id(cx);
                    self.active_entity_id = self.pending_focus;
                    return;
                };
                let local_source = if caret_offset <= mapping.full_source_range.start {
                    0
                } else if caret_offset >= mapping.full_source_range.end {
                    mapping.full_source_range.len()
                } else {
                    caret_offset - mapping.full_source_range.start
                };
                let content_offset = mapping.source_to_content
                    [local_source.min(mapping.source_to_content.len().saturating_sub(1))];
                let current_offset = mapping
                    .entity
                    .read(cx)
                    .markdown_offset_to_current_offset(content_offset);
                mapping.entity.update(cx, move |block, cx| {
                    block.assign_collapsed_selection_offset(
                        current_offset,
                        crate::components::CollapsedCaretAffinity::Default,
                        None,
                    );
                    block.marked_range = None;
                    block.cursor_blink_epoch = Instant::now();
                    cx.notify();
                });
                self.pending_focus = Some(mapping.entity.entity_id());
                self.active_entity_id = Some(mapping.entity.entity_id());
            }
        }
    }

    pub(super) fn source_range_contains(range: &std::ops::Range<usize>, offset: usize) -> bool {
        if range.start == range.end {
            offset == range.start
        } else {
            offset >= range.start && offset <= range.end
        }
    }

    pub(super) fn source_offset_distance(range: &std::ops::Range<usize>, offset: usize) -> usize {
        if Self::source_range_contains(range, offset) {
            0
        } else if offset < range.start {
            range.start - offset
        } else {
            offset.saturating_sub(range.end)
        }
    }

    /// 反序重放一组逆操作，返回「重做这一步」需要的那组增量。
    fn replay_history_group(&mut self, entry: &HistoryEntry) -> Vec<buffer::AppliedEdit> {
        // 这一步之内缓冲区已经是目标状态，别让重投影拿块树盖掉它。
        self.skip_next_resync = true;
        let mut forward = Vec::with_capacity(entry.edits.len());
        // 反序重放：后发生的改动先退回。返回的那组也按同样的反序记录，
        // 下一次 `replay_history_group` 再反一次就正好是正向时序。
        for edit in entry.edits.iter().rev() {
            forward.push(self.buffer.edit(edit.new_range.clone(), &edit.removed));
        }
        forward
    }

    /// 用缓冲区里的文本重建整棵块树，并把每个根块的区间挂回去。
    ///
    /// 视图切换、撤销/重做都走这一条：块树是缓冲区的投影，投影就只能从缓冲区来，
    /// 不能拿重新序列化的文本去建（那样根块没有区间，位置换算就没了锚点）。
    pub(crate) fn rebuild_document_from_buffer(&mut self, cx: &mut Context<Self>) {
        let source = self.buffer.text();
        match self.view_mode {
            ViewMode::Rendered => {
                let roots = self.rebuild_root_blocks_from_buffer(cx);
                self.document.replace_roots(roots, cx);
                self.rebuild_table_runtimes(cx);
                self.rebuild_image_runtimes(cx);
            }
            ViewMode::Source => {
                // markdown 源码分块是 Paragraph：高亮语言记在块上；代码文件的
                // 块是 CodeBlock，语言在 kind 里。
                let (kind, source_language) = if self.code_tab_active() {
                    let language = self
                        .file_path
                        .as_ref()
                        .or(self.recovery_source_path.as_ref())
                        .and_then(|path| path.extension())
                        .map(|extension| extension.to_string_lossy().into_owned().into());
                    (BlockKind::CodeBlock { language }, None)
                } else {
                    (BlockKind::Paragraph, Some("markdown"))
                };
                let roots = Self::build_source_document_roots(kind, &source, source_language, cx);
                self.attach_source_slice_spans(&roots, cx);
                self.document.replace_roots(roots, cx);
                self.table_cells.clear();
            }
        }
    }

    /// 撤销/重做一步的收尾：把选区放回那一步之前的现场。
    fn apply_restored_selection(&mut self, entry: &HistoryEntry, cx: &mut Context<Self>) {
        self.apply_selection_snapshot_in_current_mode(&entry.selection, cx);
        self.pending_scroll_active_block_into_view = true;
        self.pending_scroll_recheck_after_layout = true;
        self.last_scroll_viewport_size = None;
    }

    /// 引用容器里的改动要重新解析才看得出结构（引用行的换行可能把行首变成 `- 项`，
    /// 容器因此不再是容器）。
    ///
    /// 这一步按**区间**做：先把本块自己的字节写进缓冲区，再只重解析它占的那几行。
    /// 两条路里任何一条不通才退回整篇重投影——那条路会把每个未编辑块的原始字节连
    /// 折叠状态、光标现场一起洗掉。调用方已经按区间落笔时（拆引用、引用里拆块），
    /// 它留下的 `skip_next_resync` 说话，这里不再另写一遍。
    pub(super) fn normalize_rendered_quote_structure(
        &mut self,
        anchor: &Entity<Block>,
        cx: &mut Context<Self>,
    ) {
        if self.view_mode != ViewMode::Rendered {
            return;
        }

        let root = self
            .document
            .root_ancestor_of(anchor.entity_id())
            .or_else(|| Some(anchor.clone()));
        // 先落缓冲区再算选区：这一步之前插入的新引用块还没有源码区间，而位置换算
        // 只认区间——拿旧状态算出来的快照会把焦点放回上一个块。
        let wrote_region = root
            .as_ref()
            .is_some_and(|root| self.write_back_block_source(root, cx));
        self.skip_next_resync |= wrote_region;
        self.resync_buffer_from_projection(cx);
        let selection_snapshot = self.capture_source_selection_snapshot(cx);
        let reprojected = root
            .as_ref()
            .and_then(|root| {
                self.document
                    .root_blocks()
                    .iter()
                    .position(|block| block.entity_id() == root.entity_id())
            })
            .and_then(|index| self.reproject_root_region(index, cx));
        if reprojected.is_none() {
            let roots = self.rebuild_root_blocks_from_buffer(cx);
            self.document.replace_roots(roots, cx);
        }
        self.rebuild_table_runtimes(cx);
        self.rebuild_image_runtimes(cx);
        self.apply_selection_snapshot_in_current_mode(&selection_snapshot, cx);
        self.pending_scroll_active_block_into_view = true;
        self.pending_scroll_recheck_after_layout = true;
        self.last_scroll_viewport_size = None;
        // 走到这里缓冲区已经是目标状态（两条路都以它为准重解析过），调用方随后的
        // 落笔不该再来一遍整篇重投影。只把「区间写回成功」这个事实传出去：写回
        // 失败时上面的 resync 已经把投影刷进缓冲区，再设标志就会吃掉调用方
        // 下一次的落笔（skip_next_resync 是一次性标志，见 resync 的 mem::take）。
        self.skip_next_resync = wrote_region;
    }

    pub(super) fn undo_document(&mut self, cx: &mut Context<Self>) {
        let Some(entry) = self.undo_history.pop() else {
            return;
        };

        // 记下此刻的选区，重做时能回到同一现场。
        let selection_before = self.capture_source_selection_snapshot(cx);
        let current = HistoryEntry {
            edits: Vec::new(),
            selection: selection_before,
            timestamp: Instant::now(),
            kind: UndoCaptureKind::NonCoalescible,
        };
        self.pending_undo_capture = None;
        self.history_restore_in_progress = true;
        self.clear_cross_block_selection(cx);
        let forward = self.replay_history_group(&entry);
        self.rebuild_document_from_buffer(cx);
        self.history_restore_in_progress = false;
        self.apply_restored_selection(&entry, cx);
        // 重放留下的区间就是这一步的正向操作，交给重做用。
        self.redo_history.push(HistoryEntry {
            edits: forward,
            ..current
        });
        self.mark_dirty(cx);
        self.sync_table_axis_visuals(cx);
        self.dismiss_contextual_overlays(cx);
        cx.notify();
    }

    pub(super) fn redo_document(&mut self, cx: &mut Context<Self>) {
        let Some(entry) = self.redo_history.pop() else {
            return;
        };

        let selection_before = self.capture_source_selection_snapshot(cx);
        let current = HistoryEntry {
            edits: Vec::new(),
            selection: selection_before,
            timestamp: Instant::now(),
            kind: UndoCaptureKind::NonCoalescible,
        };
        self.pending_undo_capture = None;
        self.history_restore_in_progress = true;
        self.clear_cross_block_selection(cx);
        let forward = self.replay_history_group(&entry);
        self.rebuild_document_from_buffer(cx);
        self.history_restore_in_progress = false;
        self.apply_restored_selection(&entry, cx);
        self.undo_history.push(HistoryEntry {
            edits: forward,
            ..current
        });
        self.mark_dirty(cx);
        self.sync_table_axis_visuals(cx);
        self.dismiss_contextual_overlays(cx);
        cx.notify();
    }
}
