//! Undo history and selection snapshot restoration.

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
            return self
                .document
                .first_root()
                .map(|block| {
                    let block_ref = block.read(cx);
                    UndoSelectionSnapshot {
                        range: block_ref.selected_range.clone(),
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
    /// 它不该随文档大小增长。
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

    /// 与 `prepare_undo_capture` 同一条路：撤销组不再需要任何文本快照，
    /// 「稳定快照」那种每键全文对比的说法就此作废。
    pub(super) fn prepare_undo_capture_from_stable_snapshot(
        &mut self,
        kind: UndoCaptureKind,
        cx: &App,
    ) {
        if self.history_restore_in_progress || self.pending_undo_capture.is_some() {
            return;
        }
        self.pending_undo_capture = Some(PendingUndoCapture {
            snapshot: self.begin_history_group(kind, cx),
        });
    }

    pub(super) fn refresh_stable_document_snapshot(&mut self, cx: &App) {
        let source = self.current_document_source(cx);
        self.set_stable_document_snapshot(source, cx);
    }

    fn set_stable_document_snapshot(&mut self, source: String, _cx: &App) {
        self.last_stable_source_text = source;
    }

    /// 帧级选区快照刷新：只在选区（或活动块）真的变了时重算。
    ///
    /// `capture_source_selection_snapshot` 在渲染模式下要从文档第一块走到
    /// 光标块重建 source mapping，大文档里每帧都算一次就是白付 O(文档) 成本；
    /// 文档内容变化会经编辑路径刷新稳定快照，这里只管选区移动。
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

    pub(super) fn finalize_pending_undo_capture(&mut self, cx: &mut Context<Self>) {
        if self.history_restore_in_progress {
            self.pending_undo_capture = None;
            return;
        }

        let Some(pending) = self.pending_undo_capture.take() else {
            self.refresh_stable_document_snapshot(cx);
            return;
        };

        // 这次改动没落下任何字节增量：撤销栈不该多出空条目。
        if pending.snapshot.edits.is_empty() {
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
        match self.view_mode {
            ViewMode::Source => {
                let Some(block) = self.document.first_root().cloned() else {
                    return;
                };
                let len = block.read(cx).visible_len();
                let selected_range = snapshot.range.start.min(len)..snapshot.range.end.min(len);
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
            }
            ViewMode::Rendered => {
                if self.apply_cross_block_selection_snapshot_if_possible(snapshot, cx) {
                    return;
                }

                let mappings = self.build_source_target_mappings(cx);
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
                let kind = if self.code_tab_active() {
                    let language = self
                        .file_path
                        .as_ref()
                        .or(self.recovery_source_path.as_ref())
                        .and_then(|path| path.extension())
                        .map(|extension| extension.to_string_lossy().into_owned().into());
                    BlockKind::CodeBlock { language }
                } else {
                    BlockKind::Paragraph
                };
                let roots = Self::build_source_document_roots(kind, &source, cx);
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

    pub(super) fn normalize_rendered_quote_structure(&mut self, cx: &mut Context<Self>) {
        if self.view_mode != ViewMode::Rendered {
            return;
        }

        // 先落缓冲区再算选区：这一步之前插入的新引用块还没有源码区间，
        // 而位置换算只认区间——拿旧状态算出来的快照会把焦点放回上一个块。
        // 引用块的重排会换掉整棵树的实体：先把当前块树落进缓冲区并挂好区间，
        // 否则刚插入的块算不出位置，快照会退回上一个块。
        self.resync_buffer_and_stable_snapshot(cx);
        let selection_snapshot = self.capture_source_selection_snapshot(cx);
        let roots = self.rebuild_root_blocks_from_buffer(cx);
        self.document.replace_roots(roots, cx);
        self.rebuild_table_runtimes(cx);
        self.rebuild_image_runtimes(cx);
        self.apply_selection_snapshot_in_current_mode(&selection_snapshot, cx);
        self.pending_scroll_active_block_into_view = true;
        self.pending_scroll_recheck_after_layout = true;
        self.last_scroll_viewport_size = None;
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
