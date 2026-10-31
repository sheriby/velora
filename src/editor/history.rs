//! Undo history and selection snapshot restoration.

use super::source_mapping::clip_hit_to_span;
use super::*;

/// [`Editor::replay_history_group`] 的产物：重做用的正向增量，加上给区域重投影
/// 用的缓冲区变更（按应用顺序，每条 `old_range` 是它落下之前那一刻的坐标）。
struct HistoryReplay {
    forward: Vec<buffer::AppliedEdit>,
    changes: Vec<buffer::BufferChange>,
}

impl Editor {
    pub(super) fn empty_selection_snapshot() -> UndoSelectionSnapshot {
        UndoSelectionSnapshot {
            range: 0..0,
            reversed: false,
        }
    }

    pub(super) fn capture_source_selection_snapshot(&self, cx: &App) -> UndoSelectionSnapshot {
        if let Some(snapshot) = self.table_text_selection_snapshot(cx) {
            return snapshot;
        }
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
    pub(super) fn begin_history_group(&mut self, kind: UndoCaptureKind, selection: UndoSelectionSnapshot) -> HistoryEntry {
        HistoryEntry {
            edits: Vec::new(),
            selection,
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
        // 单条编辑走切片比对（评审 P167）：中文 IME 每次提交都进来，整份
        // 克隆 + 反序重放在大文档上就是一次 10MB 级堆拷贝。单条编辑的逆操作
        // 只动 new_range 一段，「放回去之后与现状相同」⟺ 这一段恰好等于
        // removed，切片对比即可。
        if let [edit] = edits {
            if edit.new_range.end > buffer.byte_len()
                || !buffer.is_char_boundary(edit.new_range.start)
                || !buffer.is_char_boundary(edit.new_range.end)
            {
                return false;
            }
            return buffer.slice(edit.new_range.clone()) == edit.removed;
        }
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
        let selection = self.capture_source_selection_snapshot(cx);
        self.pending_undo_capture = Some(PendingUndoCapture {
            snapshot: self.begin_history_group(kind, selection),
        });
    }

    pub(super) fn prepare_block_undo_capture(
        &mut self,
        block: &Entity<Block>,
        kind: UndoCaptureKind,
        before: &crate::components::BlockUndoSelection,
        cx: &mut Context<Self>,
    ) {
        if self.history_restore_in_progress || self.pending_undo_capture.is_some() {
            return;
        }
        let root_span = self.document.source_span_of(block.entity_id());
        let mapping = if matches!(before.source, crate::components::BlockUndoSource::Lines(_))
            && root_span.is_some()
        {
            None
        } else {
            self.source_mapping_for_entity(block.entity_id(), cx)
        };
        let range = if let crate::components::BlockUndoSource::Lines(lines) = &before.source {
            let source_start = root_span.clone().map(|span| span.start).or_else(|| {
                mapping
                    .as_ref()
                    .map(|mapping| mapping.full_source_range.start)
            });
            let Some(source_start) = source_start else {
                return;
            };
            let first_line = self.buffer.line_of(source_start) + lines.first_body_line;
            let offset = |position: &crate::components::BlockUndoLinePosition| {
                let source_line = self.buffer.line_range(first_line + position.line);
                let prefix = lines
                    .source_line_prefixes
                    .get(position.line)
                    .copied()
                    .map(|prefix| prefix as usize)
                    .unwrap_or_else(|| {
                        let text = self.buffer.slice(source_line.clone());
                        let mut stripped = text.clone();
                        for _ in 0..lines.quote_depth {
                            let Some(next) = super::document::strip_one_quote_level(&stripped)
                            else {
                                break;
                            };
                            stripped = next;
                        }
                        let indentation = BlockKind::leading_blank_bytes(&stripped);
                        text.len() - stripped.len()
                            + indentation.saturating_sub(position.indentation)
                    });
                (source_line.start + prefix + position.column).min(source_line.end)
            };
            offset(&lines.start)..offset(&lines.end)
        } else {
            let crate::components::BlockUndoSource::Inline {
                kind: source_kind,
                source_line_prefixes,
                line_indents,
            } = &before.source
            else {
                return;
            };
            let source_start = root_span.map(|span| span.start).or_else(|| {
                mapping
                    .as_ref()
                    .map(|mapping| mapping.full_source_range.start)
            });
            let Some(source_start) = source_start else {
                return;
            };
            let needs_rebuild = mapping.as_ref().is_none_or(|mapping| {
                block.read(cx).kind() != *source_kind
                    || mapping
                        .parsed_source_markdown
                        .as_ref()
                        .is_none_or(|markdown| markdown.split('\n').count() != line_indents.len())
                    || block.read(cx).record.source_line_prefixes != *source_line_prefixes
            });
            let mapping = if needs_rebuild && !self.table_cells.contains_key(&block.entity_id()) {
                let first_line = self.buffer.line_of(source_start);
                let quote_depth = block.read(cx).quote_depth;
                let parent_quotes =
                    quote_depth.saturating_sub(usize::from(source_kind.is_quote_container()));
                let render_depth = block.read(cx).render_depth;
                let consumed = source_start - self.buffer.line_start(first_line);
                // 首行优先用解析期记在块上的那一份账（绝对前缀减去块起点在本行里的
                // 偏移）；没有那份账才按文件字节量，量的口径与块重建同一处
                // （`measured_block_lines`）。量不出就放弃这一组，不钳零——钳零会让
                // 快照落点漂进上一行的内容里。
                let first_prefix = source_line_prefixes
                    .first()
                    .copied()
                    .and_then(|prefix| (prefix as usize).checked_sub(consumed))
                    .or_else(|| {
                        self.measured_block_prefix(
                            source_start,
                            parent_quotes,
                            // 解析器算出来的实测 dedent 只存在于它的递归栈上，撤销这条
                            // 路拿不到；按模型深度估一个（每级两个空格）。这一支只在
                            // 块上没记到那份账时才走（围栏、缩进代码块、容器自己的多行
                            // 正文），记过的形状都走上面那一支。
                            render_depth * 2,
                            source_kind,
                        )
                        .map(|prefix| prefix.0)
                    });
                let Some(first_prefix) = first_prefix else {
                    return;
                };
                let Some(measured) = self.measured_block_lines(
                    source_start,
                    first_prefix,
                    line_indents,
                    Some(source_line_prefixes),
                    parent_quotes,
                    source_kind,
                ) else {
                    return;
                };
                let mut mappings = Vec::new();
                self.push_line_prefixed_inline_mapping(
                    block,
                    String::new(),
                    &measured,
                    true,
                    source_start,
                    &mut mappings,
                );
                let Some(mapping) = mappings.into_iter().next() else {
                    return;
                };
                mapping
            } else {
                let Some(mapping) = mapping else {
                    return;
                };
                mapping
            };
            let start = mapping
                .content_to_source
                .get(before.content_range.start)
                .copied()
                .unwrap_or_else(|| mapping.content_to_source.last().copied().unwrap_or(0));
            let end = mapping
                .content_to_source
                .get(before.content_range.end)
                .copied()
                .unwrap_or_else(|| mapping.content_to_source.last().copied().unwrap_or(0));
            mapping.full_source_range.start + start..mapping.full_source_range.start + end
        };
        let selection = UndoSelectionSnapshot {
            range,
            reversed: before.reversed,
        };
        self.pending_undo_capture = Some(PendingUndoCapture {
            snapshot: self.begin_history_group(kind, selection),
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

    pub(super) fn finish_ime_composition(&mut self, cx: &mut Context<Self>) {
        if self.history_restore_in_progress {
            return;
        }
        self.finalize_pending_undo_capture(cx);
        let noop = self.undo_history.last()
            .filter(|entry| entry.kind == UndoCaptureKind::ImeComposition)
            .map(|entry| Self::history_group_is_noop(&self.buffer, &entry.edits));
        match noop {
            Some(true) => { self.undo_history.pop(); }
            Some(false) => {
                if let Some(entry) = self.undo_history.last_mut() {
                    entry.kind = UndoCaptureKind::NonCoalescible;
                }
            }
            None => {}
        }
    }

    pub(super) fn finalize_pending_undo_capture(&mut self, _cx: &mut Context<Self>) {
        if self.history_restore_in_progress {
            self.pending_undo_capture = None;
            return;
        }

        let Some(mut pending) = self.pending_undo_capture.take() else {
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
            if let Some(entry) = self.undo_history.last_mut() {
                entry.edits.extend(pending.snapshot.edits);
            }
            return;
        }

        let merged_commit_noop = {
            // 组合输入收尾时文本又回到了组合开始前：那条记录没有存在意义。
            //
            // 增量式撤销组里，「组合」和「收尾」是两条各自记录了一次缓冲区写入的
            // 条目，撤销整次组合输入必须一次退干净，所以先把收尾的增量并进来，
            // 再拿合并后的整组判断「改了等于没改」。判定结果先带出借用，
            // pop/改类型要重新拿整表的访问权。
            match self
                .undo_history
                .last_mut()
                .filter(|entry| {
                    pending.snapshot.kind == UndoCaptureKind::ImeCompositionCommit
                        && entry.kind == UndoCaptureKind::ImeComposition
                }) {
                Some(entry) => {
                    entry.edits.extend(std::mem::take(&mut pending.snapshot.edits));
                    Some(Self::history_group_is_noop(&self.buffer, &entry.edits))
                }
                None => None,
            }
        };
        if let Some(noop) = merged_commit_noop {
            self.redo_history.clear();
            if noop {
                self.undo_history.pop();
            } else if let Some(entry) = self.undo_history.last_mut() {
                entry.kind = UndoCaptureKind::NonCoalescible;
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
            if let Some(entry) = self.undo_history.last_mut() {
                entry.edits.extend(pending.snapshot.edits);
            }
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
        self.clear_cross_block_selection(cx);
        match self.view_mode {
            ViewMode::Source => {
                // 源码文档按 512 行切成多根投影块（SOURCE_DOCUMENT_CHUNK_LINES）：
                // 选区必须写进**包含落点的那一根**。以前一律钳在第一根块的长度
                // 里——文件超过 512 行后，512 行之外的搜索命中、大纲与 `[TOC]`
                // 跳转全部落在第一块末尾。
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
                self.apply_cross_block_selection_snapshot_if_possible(snapshot, cx);
                self.pending_focus = Some(anchor_id);
                self.active_entity_id = Some(anchor_id);
            }
            ViewMode::Rendered => {
                if self.apply_table_text_selection_snapshot(snapshot, cx) {
                    return;
                }
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

    /// 反序重放一组逆操作，返回「重做这一步」需要的那组增量与这组缓冲变更。
    fn replay_history_group(&mut self, entry: &HistoryEntry) -> HistoryReplay {
        // 这一步之内缓冲区已经是目标状态，别让重投影拿块树盖掉它。
        self.skip_next_resync = true;
        let mut forward = Vec::with_capacity(entry.edits.len());
        let mut changes = Vec::with_capacity(entry.edits.len());
        // 反序重放：后发生的改动先退回。返回的那组也按同样的反序记录，
        // 下一次 `replay_history_group` 再反一次就正好是正向时序。
        for edit in entry.edits.iter().rev() {
            let applied = self.buffer.edit(edit.new_range.clone(), &edit.removed);
            changes.push(buffer::BufferChange {
                old_range: edit.new_range.clone(),
                new_len: edit.removed.len(),
                revision: self.buffer.revision(),
                origin: self.buffer.shared_writer_id(),
                newline_delta: edit.removed.bytes().filter(|byte| *byte == b'\n').count() as i64
                    - applied
                        .removed
                        .bytes()
                        .filter(|byte| *byte == b'\n')
                        .count() as i64,
            });
            forward.push(applied);
        }
        HistoryReplay { forward, changes }
    }

    /// 把重放落进缓冲区的这组变更只反映到受影响的根块上。
    ///
    /// 撤销/重做改动的字节是已知区间，整篇重解析让区间外的每个块都付一遍
    /// 「重新解析 + 重新挂区间 + 丢折叠与量高」。返回 `false` 表示区域重投影
    /// 不成立（懒导入未完成、结构外扩、源码模式区间不可信），调用方整篇重建。
    fn reproject_after_history_replay(
        &mut self,
        changes: &[buffer::BufferChange],
        cx: &mut Context<Self>,
    ) -> bool {
        if changes.is_empty() {
            return true;
        }
        let applied = self.reproject_buffer_changes_regionally(changes, false, cx);
        if applied && self.vim_enabled {
            // 换上的块实体要按当前 vim 模式重套输入开关；非 vim 模式下新块的
            // 默认值就是这个状态，不必整篇扫一遍。
            self.sync_vim_block_carets(cx);
        }
        applied
    }

    /// 把一组按应用顺序给出的缓冲区变更反映到渲染投影上：只重解析与改动相交的
    /// 根块，区间外的块实体原样保留。返回 `false` 表示区域重投影不成立，
    /// 调用方必须整篇重建。
    ///
    /// 这是分屏共享视图与撤销/重做共用的唯一一份「改动 → 投影」判据：两边都在
    /// 拿一组已知字节区间去同步块树，口径分开写必然在边界（零宽区间、跨根、
    /// 区间端点落在根交界）上漂移。
    pub(crate) fn reproject_buffer_changes_regionally(
        &mut self,
        changes: &[buffer::BufferChange],
        reuse_leaf: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.document.pending_tail().is_some() {
            return false;
        }
        // 源码视图的投影块是 512 行的定长分块：改行数就会移动分块边界（块首退格
        // 把两块并成一块），只按旧区间重切片给不出新分块。行数不变的编辑可以按
        // 区间重切片；变了就整篇重建（源码文档的根块数只是「行数 / 512」，便宜）。
        let source_chunks_hold = changes.iter().all(|change| change.newline_delta == 0);
        if self.view_mode == ViewMode::Source && !source_chunks_hold {
            return false;
        }
        let mut total = (self.buffer.byte_len() as i64
            - changes
                .iter()
                .map(|change| change.new_len as i64 - change.old_range.len() as i64)
                .sum::<i64>())
            .max(0) as usize;
        // 每条变更各自算出受影响的根块下标；块表在这些步骤里只换区间、不换根，
        // 所以下标在整个循环里都是同一份。相邻/重叠的区间合并成一段，避免把
        // 同一根块重解析多次。
        let mut regions: Vec<(usize, usize)> = Vec::new();
        for change in changes {
            let roots = self.document.root_blocks();
            let mut first = roots
                .partition_point(|root| {
                    self.document
                        .source_span_of(root.entity_id())
                        .is_some_and(|span| span.end <= change.old_range.start)
                })
                .min(roots.len().saturating_sub(1));
            let last_offset = change
                .old_range
                .end
                .saturating_sub(usize::from(!change.old_range.is_empty()));
            let mut last = roots
                .partition_point(|root| {
                    self.document
                        .source_span_of(root.entity_id())
                        .is_some_and(|span| span.end <= last_offset)
                })
                .min(roots.len().saturating_sub(1));
            // 改动只要**碰到**根块边界，两侧的块就可能合并/拆分成别的结构
            // （删掉两段之间的空行，两段并成一段）：必须把邻块一起重解析，
            // 否则只刷新一侧，会在投影里留下本该合并的两个块。
            if first > 0
                && self
                    .document
                    .source_span_of(roots[first - 1].entity_id())
                    .is_some_and(|span| span.end == change.old_range.start)
            {
                first -= 1;
            }
            if last + 1 < roots.len()
                && self
                    .document
                    .source_span_of(roots[last + 1].entity_id())
                    .is_some_and(|span| span.start == change.old_range.end)
            {
                last += 1;
            }
            // 没有源码区间的根（结构操作刚造出来的空段落）在字节坐标里没有位置：
            // 它可能正落在改动区里、被重解析吞掉，也可能不在。区域重投影给不出
            // 判据，只能整篇重建。
            let neighbours_without_span = |index: usize| {
                roots
                    .get(index)
                    .is_some_and(|root| self.document.source_span_of(root.entity_id()).is_none())
            };
            if (first > 0 && neighbours_without_span(first - 1))
                || neighbours_without_span(last + 1)
            {
                return false;
            }
            match regions.last_mut() {
                Some((_, open_end)) if first <= *open_end + 1 => {
                    *open_end = (*open_end).max(last);
                }
                _ => regions.push((first, last)),
            }
            // 先记下这一段里所有根的旧区间：平移只处理「整体落在改动之后」的根，
            // 与改动相交的根（包括界限恰好落在改动区间里的）要按映射逐端修正。
            let affected_spans: Vec<(EntityId, std::ops::Range<usize>)> = (first..=last)
                .filter_map(|index| {
                    roots.get(index).and_then(|root| {
                        self.document
                            .source_span_of(root.entity_id())
                            .map(|span| (root.entity_id(), span))
                    })
                })
                .collect();
            let delta = change.new_len as i64 - change.old_range.len() as i64;
            let map_offset = |offset: usize| -> usize {
                if offset <= change.old_range.start {
                    offset
                } else if offset >= change.old_range.end {
                    (offset as i64 + delta).max(0) as usize
                } else {
                    change.old_range.start + change.new_len
                }
            };
            total = (total as i64 + delta).max(0) as usize;
            self.document
                .shift_source_spans_after(change.old_range.end, delta, total);
            for (id, span) in affected_spans {
                let start = map_offset(span.start).min(total);
                let end = map_offset(span.end).min(total).max(start);
                self.document.set_source_span(id, start..end);
            }
        }

        if self.view_mode == ViewMode::Source {
            // 源码投影块是缓冲区的定长切片，不重解析：把区间内的块按新内容
            // 重新切片，再串一遍围栏状态。
            for (first, last) in regions {
                for index in first..=last {
                    let Some(root) = self.document.root_blocks().get(index).cloned() else {
                        continue;
                    };
                    let span = self
                        .document
                        .source_span_of(root.entity_id())
                        .filter(|span| self.buffer.span_is_trustworthy(span));
                    let Some(span) = span else {
                        return false;
                    };
                    let text = self.buffer.slice(span);
                    root.update(cx, |block, cx| {
                        block.record.set_title(InlineTextTree::plain(text));
                        block.sync_render_cache();
                        cx.notify();
                    });
                    self.cascade_source_fence_states_after(root.entity_id(), cx);
                }
            }
            self.refresh_source_line_starts(cx);
            return true;
        }

        let affected_roots: Vec<_> = regions
            .iter()
            .filter_map(|(first, _)| self.document.root_blocks().get(*first).cloned())
            .collect();
        let mut refresh_references = affected_roots
            .iter()
            .any(|root| self.changed_block_needs_runtime_context_refresh(root, cx));
        let mut replaced = Vec::new();
        // 从后往前替换：后面的区间先换掉，前面那些下标就还是这份表里的原值。
        for (first, last) in regions.into_iter().rev() {
            let mut previous: Vec<Entity<Block>> = self
                .document
                .root_blocks()
                .get(first..last + 1)
                .unwrap_or_default()
                .to_vec();
            let folded: Vec<bool> = previous.iter().map(|root| root.read(cx).folded).collect();
            let kinds: Vec<BlockKind> = previous.iter().map(|root| root.read(cx).kind()).collect();
            while let Some(block) = previous.pop() {
                let block_state = block.read(cx);
                previous.extend(block_state.children.iter().cloned());
                if let Some(runtime) = &block_state.table_runtime {
                    for cell in runtime.header.iter().chain(runtime.rows.iter().flatten()) {
                        self.table_cells.remove(&cell.entity_id());
                    }
                }
                self.runtime_context_sensitive_blocks
                    .remove(&block.entity_id());
            }
            let old_count = self.document.root_blocks().len();
            if self
                .reproject_root_range(first, last, reuse_leaf, cx)
                .is_none()
            {
                return false;
            }
            let count = self.document.root_blocks().len() + (last + 1 - first) - old_count;
            let installed: Vec<Entity<Block>> = self
                .document
                .root_blocks()
                .iter()
                .skip(first)
                .take(count)
                .cloned()
                .collect();
            // 同一位置、同一种类的块沿用折叠态：撤销一次标题里的改动不该把
            // 折着的标题展开——整篇重建那条路也是按签名/位置把折叠态带回去的。
            if installed.len() == folded.len()
                && installed
                    .iter()
                    .zip(&kinds)
                    .all(|(root, kind)| root.read(cx).kind() == *kind)
            {
                for (root, folded) in installed.iter().zip(folded) {
                    if folded {
                        root.update(cx, |block, cx| {
                            block.folded = true;
                            cx.notify();
                        });
                    }
                }
            }
            replaced.extend(installed);
        }
        refresh_references |= replaced
            .iter()
            .any(|root| self.changed_block_needs_runtime_context_refresh(root, cx));
        let base = self.local_link_base_dir();
        while let Some(block) = replaced.pop() {
            replaced.extend(block.read(cx).children.iter().cloned());
            let table = block.read(cx).record.table.clone();
            if let Some(table) = table {
                self.install_table_runtime_for_block(&block, &table, cx);
            }
            self.sync_runtime_context_for_block(&block, base.as_deref(), cx);
        }
        if refresh_references {
            self.rebuild_image_runtimes(cx);
        }
        true
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
                // 引用注册表的刷新由表格那一趟收尾代劳（`rebuild_table_runtimes` 的最后
                // 一行）。**别再补一次 `rebuild_image_runtimes`**：同一份缓冲区、同一棵
                // 树连着重建两遍是纯浪费——10 MiB 档一遍 766ms，撤销与切视图每次都白付
                // （闸门 `undoing_once_rebuilds_the_reference_registry_exactly_once`
                // 钉住「恰好一遍」）。
                self.rebuild_table_runtimes(cx);
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
                // 围栏/公式/frontmatter 会跨 512 行分块的接缝：整棵重建后把
                // 接缝状态从头串一遍，跨块的高亮才接得上。
                if !self.code_tab_active() {
                    self.resync_source_fence_states(cx);
                }
                self.table_cells.clear();
            }
        }
        // 撤销/切视图重建出的块同样是新实体，按当前模式重套 vim 的输入开关。
        self.sync_vim_block_carets(cx);
    }

    /// 单条缓冲区编辑之后只重解析受影响的一段根块，区间外的实体原样保留。
    ///
    /// 返回 false 表示这段区间算不准或带不了（未物化尾部、源码视图、表格、
    /// 引用/脚注定义、行内图片等需要整篇运行时收尾的形状），调用方退回
    /// [`Self::rebuild_document_from_buffer`]。
    pub(crate) fn reproject_after_buffer_edit(
        &mut self,
        edit: &buffer::AppliedEdit,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.view_mode != ViewMode::Rendered || self.document.pending_tail().is_some() {
            return false;
        }
        let old_start = edit.new_range.start;
        let old_end = old_start + edit.removed.len();
        let new_len = edit.new_range.len();
        let total = self.buffer.byte_len();
        let root_ids: Vec<_> = self
            .document
            .root_blocks()
            .iter()
            .map(|root| root.entity_id())
            .collect();
        if root_ids.is_empty() {
            return false;
        }
        let mut first: Option<usize> = None;
        let mut last: Option<usize> = None;
        for (index, id) in root_ids.iter().enumerate() {
            let Some(span) = self.document.source_span_of(*id) else {
                return false;
            };
            if span.end < old_start {
                continue;
            }
            if span.start > old_end {
                break;
            }
            first.get_or_insert(index);
            last = Some(index);
        }
        let (Some(first), Some(last)) = (first, last) else {
            return false;
        };
        // 编辑越过最后一根块的末尾：新内容落在所有旧区间之外，重投影窗口里
        // 的探针根不会被安装，新增的根会静默丢掉。交给整篇重建。
        if last + 1 == root_ids.len()
            && self
                .document
                .source_span_of(root_ids[last])
                .is_some_and(|span| old_end >= span.end)
        {
            return false;
        }
        // 边界一侧的根也可能被这次编辑并掉/拆开，扩一根一起重解析。
        let first = first.saturating_sub(1);
        let last = (last + 1).min(root_ids.len() - 1);
        let old_roots: Vec<_> = self.document.root_blocks()[first..=last].to_vec();
        let old_spans: Vec<_> = (first..=last)
            .filter_map(|index| {
                self.document
                    .root_blocks()
                    .get(index)
                    .and_then(|root| {
                        self.document
                            .source_span_of(root.entity_id())
                            .map(|span| (root.entity_id(), span))
                    })
            })
            .collect();
        let old_count = last + 1 - first;

        // 表格/引用定义/脚注等需要整篇运行时收尾的形状：宁可整篇重建，
        // 也不在半接好的区域上继续写。
        let mut needs_full = false;
        let mut walk: Vec<_> = old_roots.clone();
        while let Some(root) = walk.pop() {
            if root.read(cx).kind() == BlockKind::Table
                || root.read(cx).record.table.is_some()
                || self.changed_block_needs_runtime_context_refresh(&root, cx)
            {
                needs_full = true;
                break;
            }
            walk.extend(root.read(cx).children.iter().cloned());
        }
        if needs_full {
            return false;
        }

        let delta = new_len as i64 - edit.removed.len() as i64;
        if delta != 0 {
            let map_offset = |offset: usize| -> usize {
                if offset <= old_start {
                    offset
                } else if offset >= old_end {
                    (offset as i64 + delta).max(0) as usize
                } else {
                    old_start + new_len
                }
            };
            self.document.shift_source_spans_after(old_end, delta, total);
            for (id, span) in old_spans {
                let start = map_offset(span.start).min(total);
                let end = map_offset(span.end).min(total).max(start);
                self.document.set_source_span(id, start..end);
            }
        }

        if self.reproject_root_range(first, last, false, cx).is_none() {
            return false;
        }
        let new_count = old_count + self.document.root_blocks().len().saturating_sub(root_ids.len());
        let new_roots: Vec<_> = self
            .document
            .root_blocks()
            .iter()
            .skip(first)
            .take(new_count)
            .cloned()
            .collect();
        let base_dir = self.local_link_base_dir();
        let mut region: Vec<_> = new_roots;
        while let Some(block) = region.pop() {
            region.extend(block.read(cx).children.iter().cloned());
            self.sync_runtime_context_for_block(&block, base_dir.as_deref(), cx);
        }
        self.sync_table_axis_visuals(cx);
        self.sync_vim_block_carets(cx);
        true
    }

    /// 撤销/重做一步的收尾：把选区放回那一步之前的现场。
    fn apply_restored_selection(&mut self, entry: &HistoryEntry, cx: &mut Context<Self>) {
        self.cancel_pending_view_mode_scroll();
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
        let mut replaced_roots: Vec<Entity<Block>> = Vec::new();
        let reprojected = root
            .as_ref()
            .and_then(|root| {
                self.document
                    .root_blocks()
                    .iter()
                    .position(|block| block.entity_id() == root.entity_id())
            })
            .and_then(|index| {
                // 重投影把 1 根换成 N 根：借总根数差把换上来的根收齐，收尾只接
                // 这一段的运行时上下文（与 workspace/split/shared.rs 的区域重投影
                // 收尾同一条路）。
                let old_count = self.document.root_blocks().len();
                let region_lines = self.reproject_root_region(index, cx)?;
                let new_count = self.document.root_blocks().len();
                replaced_roots.extend(
                    self.document
                        .root_blocks()
                        .iter()
                        .skip(index)
                        .take(new_count + 1 - old_count)
                        .cloned(),
                );
                Some(region_lines)
            });
        if reprojected.is_none() {
            let roots = self.rebuild_root_blocks_from_buffer(cx);
            self.document.replace_roots(roots, cx);
            // 整篇重建：表格与注册表都由 rebuild_table_runtimes 兜底——它收尾
            // 本来就会整篇重建注册表，旧代码在这之后又直接重建了一次，等于每次
            // 调用都白付两遍整篇。
            self.rebuild_table_runtimes(cx);
        } else {
            // 区域重投影：区间外的块与表格原样活着，整篇 table_cells 清空重建与
            // 整篇注册表重建只该在「这一笔真的碰了定义行/脚注引用」时付。判据
            // 复用每键路径那把屏（`changed_block_needs_runtime_context_refresh`，
            // 底下是 `contains_reference_definition_marker`），不另立第二份口径。
            self.clear_table_axis_preview(cx);
            self.clear_table_axis_selection(cx);
            let mut refresh_references = false;
            for region_root in &replaced_roots {
                refresh_references |=
                    self.changed_block_needs_runtime_context_refresh(region_root, cx);
            }
            if refresh_references {
                self.rebuild_table_runtimes(cx);
            } else {
                // 旧区域实体换掉了：它们留下的敏感集登记与格子绑定先收走，再把
                // 换上来的块接上现有注册表（base_dir、引用表、阅读模式）。
                if let Some(old_root) = root.as_ref() {
                    let mut stale = vec![old_root.clone()];
                    while let Some(block) = stale.pop() {
                        let block_state = block.read(cx);
                        stale.extend(block_state.children.iter().cloned());
                        if let Some(runtime) = &block_state.table_runtime {
                            for cell in
                                runtime.header.iter().chain(runtime.rows.iter().flatten())
                            {
                                self.table_cells.remove(&cell.entity_id());
                            }
                        }
                        self.runtime_context_sensitive_blocks
                            .remove(&block.entity_id());
                    }
                }
                let base_dir = self.local_link_base_dir();
                let mut region_blocks = replaced_roots.clone();
                while let Some(block) = region_blocks.pop() {
                    region_blocks.extend(block.read(cx).children.iter().cloned());
                    let table = block.read(cx).record.table.clone();
                    if let Some(table) = table {
                        self.install_table_runtime_for_block(&block, &table, cx);
                    }
                    self.sync_runtime_context_for_block(&block, base_dir.as_deref(), cx);
                }
                self.sync_table_axis_visuals(cx);
            }
        }
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
        if self.is_read_only() {
            return;
        }

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
        let replay = self.replay_history_group(&entry);
        if !self.reproject_after_history_replay(&replay.changes, cx) {
            self.rebuild_document_from_buffer(cx);
        }
        self.history_restore_in_progress = false;
        self.apply_restored_selection(&entry, cx);
        // 重放留下的区间就是这一步的正向操作，交给重做用。
        self.redo_history.push(HistoryEntry {
            edits: replay.forward,
            ..current
        });
        self.mark_dirty(cx);
        self.sync_table_axis_visuals(cx);
        self.dismiss_contextual_overlays(cx);
        cx.notify();
    }

    pub(super) fn redo_document(&mut self, cx: &mut Context<Self>) {
        if self.is_read_only() {
            return;
        }

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
        let replay = self.replay_history_group(&entry);
        if !self.reproject_after_history_replay(&replay.changes, cx) {
            self.rebuild_document_from_buffer(cx);
        }
        self.history_restore_in_progress = false;
        self.apply_restored_selection(&entry, cx);
        self.undo_history.push(HistoryEntry {
            edits: replay.forward,
            ..current
        });
        self.mark_dirty(cx);
        self.sync_table_axis_visuals(cx);
        self.dismiss_contextual_overlays(cx);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::Editor;
    use crate::editor::buffer;
    use std::ops::Range;

    /// 产品路径禁 expect（AGENTS §1）：撤销合并分支依赖「上方守卫已保证」的
    /// expect 在代码被重排后就会当场 panic（评审 P168）。守卫结果必须用
    /// if let 表达，不许靠 expect 传达不变量。
    #[test]
    fn undo_merge_branches_express_guards_with_if_let_not_expect() {
        let source = include_str!("history.rs");
        assert!(
            !source.contains(concat!("checked abo", "ve")),
            "撤销合并分支不许用 expect 传达「上方已守卫」"
        );
    }

    /// 单条编辑的 noop 快速判定（评审 P167）必须与整份克隆 + 反序重放逐例
    /// 同判：IME 每次提交都走这里，10 MiB 文档上旧路径是一次 10MB 堆拷贝。
    /// 等价性守的是正确性——快速路径一旦与重放分叉，撤销栈会多出或吃掉条目。
    #[test]
    fn single_edit_noop_verdict_matches_the_full_replay() {
        let cases: [(&str, Range<usize>, &str, bool); 6] = [
            // 原样重打（多字节）：换回同一串 = noop。
            ("hello 世界 world", 6..12, "世界", true),
            // 真改动：换成了不同内容。
            ("hello 世界 world", 6..12, "世界们", false),
            // 原样重打（ASCII）。
            ("hello world", 0..5, "hello", true),
            // 纯插入：内容必然变多，非 noop。
            ("hello world", 5..5, "  ", false),
            // 整段替换回原文：noop。
            ("abcdef", 0..6, "abcdef", true),
            // 整段替换成别的：非 noop。
            ("abcdef", 0..6, "xyz", false),
        ];
        for (text, range, replaced_with, expected_noop) in cases {
            let mut buffer = buffer::TextBuffer::from_text(text);
            let applied = buffer.edit(range.clone(), replaced_with);
            let fast = Editor::history_group_is_noop(&buffer, &[applied.clone()]);
            let mut probe = buffer.clone();
            probe.edit(applied.new_range.clone(), &applied.removed);
            let slow = probe.same_content(&buffer);
            assert_eq!(
                fast, slow,
                "单条编辑 noop 快速判定与整份重放分叉（{text:?} {range:?} → {replaced_with:?}）"
            );
            assert_eq!(
                fast, expected_noop,
                "语义钉死：{text:?} {range:?} → {replaced_with:?}"
            );
        }
    }
}
