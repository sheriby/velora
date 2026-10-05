//! Editor-level selection spanning multiple rendered blocks.

use std::ops::Range;

use gpui::*;

use super::{
    CrossBlockDrag, CrossBlockSelection, CrossBlockSelectionEndpoint, Editor, SourceTargetMapping,
    UndoSelectionSnapshot, ViewMode,
};
use crate::components::{Block, Copy, Cut, Delete, DeleteBack, UndoCaptureKind};
use crate::components::markdown::inline::clamp_range_to_char_boundaries;

/// Cross-block selection with endpoints ordered by visible block position.
#[derive(Clone, Copy)]
pub(super) struct NormalizedCrossBlockSelection {
    pub(super) start: CrossBlockSelectionEndpoint,
    pub(super) end: CrossBlockSelectionEndpoint,
    pub(super) start_index: usize,
    pub(super) end_index: usize,
    pub(super) reversed: bool,
}

impl Editor {
    fn clear_cross_block_selection_visuals(&mut self, cx: &mut Context<Self>) -> bool {
        let mut changed = false;
        for visible in self.document.visible_blocks().to_vec() {
            visible.entity.update(cx, |block, cx| {
                if block.editor_selection_range.take().is_some() {
                    changed = true;
                    cx.notify();
                }
            });
        }
        changed
    }

    pub(super) fn clear_cross_block_selection(&mut self, cx: &mut Context<Self>) {
        let had_selection = self.cross_block_selection.take().is_some();
        self.cross_block_drag = None;
        let changed_visuals = self.clear_cross_block_selection_visuals(cx);
        let changed = had_selection || changed_visuals;
        if changed {
            cx.notify();
        }
    }

    fn begin_cross_block_drag_at_point(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let had_selection = self.cross_block_selection.take().is_some();
        let changed_visuals = self.clear_cross_block_selection_visuals(cx);
        let changed = had_selection || changed_visuals;
        self.cross_block_drag = self
            .cross_block_endpoint_for_point(position, cx)
            .map(|anchor| CrossBlockDrag { anchor });
        if changed {
            cx.notify();
        }
    }

    pub(super) fn on_editor_capture_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // [[ 补全浮层内的按下不动它：行确认靠 bubble 阶段的同一次按下；
        // 落在浮层外的点击立即关闭。
        if self.wikilink_completion_is_open() {
            let inside_panel = self
                .wikilink_completion
                .as_ref()
                .and_then(|state| state.panel_bounds)
                .is_some_and(|bounds| bounds.contains(&event.position));
            if !inside_panel {
                self.close_wikilink_completion(cx);
            }
        }

        if event.button != MouseButton::Left {
            cx.propagate();
            return;
        }

        if self.view_mode != ViewMode::Rendered {
            cx.propagate();
            return;
        }

        self.rendered_select_all_cycle = None;
        self.begin_cross_block_drag_at_point(event.position, cx);
        cx.propagate();
    }

    pub(super) fn on_editor_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !event.dragging() {
            return;
        }
        let Some(drag) = self.cross_block_drag else {
            return;
        };
        let Some(focus) = self.cross_block_endpoint_for_point(event.position, cx) else {
            return;
        };

        if self.cross_block_selection.is_none() && drag.anchor.entity_id == focus.entity_id {
            return;
        }

        let selection = CrossBlockSelection {
            anchor: drag.anchor,
            focus,
        };
        if self.cross_block_selection_is_empty(selection) {
            self.cross_block_selection = None;
        } else {
            self.cross_block_selection = Some(selection);
        }
        self.sync_cross_block_selection_visuals(cx);
        cx.notify();
    }

    pub(super) fn on_editor_mouse_up(
        &mut self,
        _event: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cross_block_drag = None;
        self.end_block_pointer_selection_sessions(cx);
    }

    pub(super) fn on_copy_capture(
        &mut self,
        _: &Copy,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(markdown) = self.cross_block_selected_markdown(cx) else {
            cx.propagate();
            return;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(markdown));
        cx.stop_propagation();
    }

    pub(super) fn on_cut_capture(&mut self, _: &Cut, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(markdown) = self.cross_block_selected_markdown(cx) else {
            cx.propagate();
            return;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(markdown));
        self.delete_cross_block_selection(cx);
        cx.stop_propagation();
    }

    pub(super) fn on_delete_capture(
        &mut self,
        _: &Delete,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.delete_cross_block_selection(cx) {
            cx.propagate();
            return;
        }
        cx.stop_propagation();
    }

    pub(super) fn on_delete_back_capture(
        &mut self,
        _: &DeleteBack,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.delete_cross_block_selection(cx) {
            cx.propagate();
            return;
        }
        cx.stop_propagation();
    }

    fn rendered_document_is_fully_selected(&self, cx: &App) -> bool {
        let visible = self.document.visible_blocks().to_vec();
        let Some(first) = visible.first() else {
            return false;
        };
        let Some(last) = visible.last() else {
            return false;
        };
        let Some(selection) = self.cross_block_selection else {
            return false;
        };
        let last_len = last.entity.read(cx).visible_len();
        selection.anchor
            == CrossBlockSelectionEndpoint {
                entity_id: first.entity.entity_id(),
                offset: 0,
            }
            && selection.focus
                == CrossBlockSelectionEndpoint {
                    entity_id: last.entity.entity_id(),
                    offset: last_len,
                }
    }

    fn select_focused_block_text_for_rendered_select_all(
        &mut self,
        block: Entity<Block>,
        cx: &mut Context<Self>,
    ) {
        self.clear_cross_block_selection(cx);
        self.end_block_pointer_selection_sessions(cx);
        self.clear_table_axis_preview(cx);
        self.clear_table_axis_selection(cx);
        block.update(cx, |block, cx| {
            let len = block.visible_len();
            block.selected_range = 0..len;
            block.selection_reversed = false;
            block.marked_range = None;
            block.vertical_motion_x = None;
            block.cursor_blink_epoch = std::time::Instant::now();
            cx.notify();
        });
        self.active_entity_id = Some(block.entity_id());
        cx.notify();
    }

    fn select_all_rendered_document(&mut self, cx: &mut Context<Self>) {
        if self.rendered_document_is_fully_selected(cx) {
            return;
        }

        let visible = self.document.visible_blocks().to_vec();
        let Some(first) = visible.first() else {
            return;
        };
        let Some(last) = visible.last() else {
            return;
        };
        let first_id = first.entity.entity_id();
        let last_id = last.entity.entity_id();
        let last_len = last.entity.read(cx).visible_len();

        self.end_block_pointer_selection_sessions(cx);
        self.dismiss_contextual_overlays(cx);
        self.clear_table_axis_preview(cx);
        self.clear_table_axis_selection(cx);
        for visible in &visible {
            visible.entity.update(cx, |block, cx| {
                let cursor = block.cursor_offset();
                let collapsed = cursor..cursor;
                if block.selected_range != collapsed {
                    block.selected_range = collapsed;
                    cx.notify();
                }
            });
        }

        self.cross_block_drag = None;
        self.cross_block_selection = Some(CrossBlockSelection {
            anchor: CrossBlockSelectionEndpoint {
                entity_id: first_id,
                offset: 0,
            },
            focus: CrossBlockSelectionEndpoint {
                entity_id: last_id,
                offset: last_len,
            },
        });
        self.sync_cross_block_selection_visuals(cx);
        cx.notify();
    }

    pub(super) fn on_rendered_select_all_press(
        &mut self,
        block: Entity<Block>,
        cx: &mut Context<Self>,
    ) {
        if self.view_mode != ViewMode::Rendered {
            self.rendered_select_all_cycle = None;
            return;
        }

        let now = std::time::Instant::now();
        let block_id = block.entity_id();
        let count = match self.rendered_select_all_cycle {
            Some(cycle)
                if cycle.entity_id == block_id
                    && now.duration_since(cycle.last_pressed_at)
                        <= Self::RENDERED_SELECT_ALL_CYCLE_WINDOW =>
            {
                cycle.count.saturating_add(1)
            }
            _ => 1,
        }
        .min(3);

        self.rendered_select_all_cycle = Some(super::RenderedSelectAllCycle {
            entity_id: block_id,
            count,
            last_pressed_at: now,
        });

        if count == 1 {
            self.select_focused_block_text_for_rendered_select_all(block, cx);
        } else {
            self.select_all_rendered_document(cx);
        }
    }

    pub(super) fn cross_block_source_selection_snapshot(
        &self,
        cx: &App,
    ) -> Option<UndoSelectionSnapshot> {
        let normalized = self.normalized_cross_block_selection(cx)?;
        let range = self.cross_block_source_range_for_normalized(normalized, cx)?;
        Some(UndoSelectionSnapshot {
            range,
            reversed: normalized.reversed,
        })
    }

    pub(super) fn apply_cross_block_selection_snapshot_if_possible(
        &mut self,
        snapshot: &UndoSelectionSnapshot,
        cx: &mut Context<Self>,
    ) -> bool {
        if snapshot.range.is_empty() {
            return false;
        }

        let mappings = self.source_mappings_in_range(&snapshot.range, cx);
        let Some(start) = self.endpoint_for_source_offset(snapshot.range.start, &mappings, cx)
        else {
            return false;
        };
        let Some(end) = self.endpoint_for_source_offset(snapshot.range.end, &mappings, cx) else {
            return false;
        };
        let Some(start_index) = self.document.visible_index_for_entity_id(start.entity_id) else {
            return false;
        };
        let Some(end_index) = self.document.visible_index_for_entity_id(end.entity_id) else {
            return false;
        };
        if start_index == end_index {
            return false;
        }

        self.cross_block_selection = Some(if snapshot.reversed {
            CrossBlockSelection {
                anchor: end,
                focus: start,
            }
        } else {
            CrossBlockSelection {
                anchor: start,
                focus: end,
            }
        });
        self.cross_block_drag = None;
        self.sync_cross_block_selection_visuals(cx);
        let focus = if snapshot.reversed { start } else { end };
        self.focus_block(focus.entity_id);
        cx.notify();
        true
    }

    fn cross_block_endpoint_for_point(
        &self,
        position: Point<Pixels>,
        cx: &App,
    ) -> Option<CrossBlockSelectionEndpoint> {
        let mut previous: Option<(Entity<Block>, Bounds<Pixels>)> = None;
        for visible in self.document.visible_blocks() {
            let entity = visible.entity.clone();
            let bounds = entity.read(cx).last_bounds;
            let Some(bounds) = bounds else {
                continue;
            };

            if position.y < bounds.top() {
                if let Some((previous, _)) = previous {
                    let offset = previous.read(cx).visible_len();
                    return Some(CrossBlockSelectionEndpoint {
                        entity_id: previous.entity_id(),
                        offset,
                    });
                }
                return Some(CrossBlockSelectionEndpoint {
                    entity_id: entity.entity_id(),
                    offset: 0,
                });
            }

            if position.y <= bounds.bottom() {
                let offset = entity.read(cx).index_for_mouse_position(position);
                return Some(CrossBlockSelectionEndpoint {
                    entity_id: entity.entity_id(),
                    offset,
                });
            }

            previous = Some((entity, bounds));
        }

        previous.map(|(entity, _)| CrossBlockSelectionEndpoint {
            entity_id: entity.entity_id(),
            offset: entity.read(cx).visible_len(),
        })
    }

    fn cross_block_selection_is_empty(&self, selection: CrossBlockSelection) -> bool {
        let Some(anchor_index) = self
            .document
            .visible_index_for_entity_id(selection.anchor.entity_id)
        else {
            return true;
        };
        let Some(focus_index) = self
            .document
            .visible_index_for_entity_id(selection.focus.entity_id)
        else {
            return true;
        };
        anchor_index == focus_index && selection.anchor.offset == selection.focus.offset
    }

    pub(super) fn normalized_cross_block_selection(
        &self,
        cx: &App,
    ) -> Option<NormalizedCrossBlockSelection> {
        let selection = self.cross_block_selection?;
        let anchor = self.clamp_cross_block_endpoint(selection.anchor, cx)?;
        let focus = self.clamp_cross_block_endpoint(selection.focus, cx)?;
        let anchor_index = self
            .document
            .visible_index_for_entity_id(anchor.entity_id)?;
        let focus_index = self.document.visible_index_for_entity_id(focus.entity_id)?;
        let reversed = focus_index < anchor_index
            || (focus_index == anchor_index && focus.offset < anchor.offset);
        let (start, end, start_index, end_index) = if reversed {
            (focus, anchor, focus_index, anchor_index)
        } else {
            (anchor, focus, anchor_index, focus_index)
        };
        if start_index == end_index && start.offset == end.offset {
            return None;
        }
        Some(NormalizedCrossBlockSelection {
            start,
            end,
            start_index,
            end_index,
            reversed,
        })
    }

    fn clamp_cross_block_endpoint(
        &self,
        endpoint: CrossBlockSelectionEndpoint,
        cx: &App,
    ) -> Option<CrossBlockSelectionEndpoint> {
        let entity = self.document.block_entity_by_id(endpoint.entity_id)?;
        let len = entity.read(cx).visible_len();
        Some(CrossBlockSelectionEndpoint {
            entity_id: endpoint.entity_id,
            offset: endpoint.offset.min(len),
        })
    }

    fn sync_cross_block_selection_visuals(&mut self, cx: &mut Context<Self>) {
        let normalized = self.normalized_cross_block_selection(cx);
        let visible_blocks = self.document.visible_blocks().to_vec();
        for (index, visible) in visible_blocks.into_iter().enumerate() {
            let next_range = normalized.and_then(|selection| {
                if index < selection.start_index || index > selection.end_index {
                    return None;
                }
                let block = visible.entity.read(cx);
                let len = block.visible_len();
                let range = if selection.start_index == selection.end_index {
                    selection.start.offset.min(len)..selection.end.offset.min(len)
                } else if index == selection.start_index {
                    selection.start.offset.min(len)..len
                } else if index == selection.end_index {
                    0..selection.end.offset.min(len)
                } else {
                    0..len
                };
                (!range.is_empty()).then_some(range)
            });

            visible.entity.update(cx, |block, cx| {
                if block.editor_selection_range != next_range {
                    block.editor_selection_range = next_range.clone();
                    cx.notify();
                }
            });
        }
    }

    /// 光标落在缓冲区的哪个字节：块内显示偏移 → 文件字节偏移。
    ///
    /// 渲染块里「第 n 个可见字符」和「源文本第 n 个字节」不是一回事（`~2~` 三个
    /// 字节显示成 2 个字符），标题的 `# ` 前缀也不在块的显示文本里，所以插入点
    /// 必须经映射换算，不能拿块内偏移直接当缓冲区偏移用。
    ///
    /// 只取这一块自己的映射：打字每键都要走这里，整篇重建映射是 O(文档)。
    pub(crate) fn caret_source_offset(
        &self,
        entity_id: EntityId,
        display_offset: usize,
        cx: &App,
    ) -> Option<usize> {
        let mapping = self.source_mapping_for_entity(entity_id, cx)?;
        self.mapping_source_offset(&mapping, display_offset, cx)
    }

    fn mapping_source_offset(
        &self,
        mapping: &SourceTargetMapping,
        offset: usize,
        cx: &App,
    ) -> Option<usize> {
        let block = mapping.entity.read(cx);
        let visible_len = block.visible_len();
        if offset >= visible_len {
            // 「内容末尾」在文件里停在哪，映射表自己最清楚：块区间可能还压着用户写的
            // 闭合 `#`、行尾空格这些不属于内容的字节（`# 标题 #` 就是），拿区间末尾当
            // 落点会把字插到那些字节之后。
            return mapping
                .content_to_source
                .last()
                .map(|offset| mapping.full_source_range.start + *offset)
                .or(Some(mapping.full_source_range.end));
        }
        // 第 0 个可见字符也在内容里，就要按内容区间换算：`full_source_range.start` 含
        // `# `、`- `、`> ` 这些记号，段首打字直接落它就把字插到了记号前面。
        if mapping.content_to_source.is_empty() {
            return Some(mapping.full_source_range.start);
        }
        let markdown_offset = block
            .current_range_to_markdown_range(offset..offset)
            .start;
        let max_content = mapping.content_to_source.len() - 1;
        Some(
            mapping.full_source_range.start
                + mapping.content_to_source[markdown_offset.min(max_content)],
        )
    }

    fn endpoint_for_source_offset(
        &self,
        offset: usize,
        mappings: &[SourceTargetMapping],
        cx: &App,
    ) -> Option<CrossBlockSelectionEndpoint> {
        let mapping = mappings.iter().min_by_key(|mapping| {
            Self::source_offset_distance(&mapping.full_source_range, offset)
        })?;
        let local = if offset <= mapping.full_source_range.start {
            0
        } else if offset >= mapping.full_source_range.end {
            mapping.full_source_range.len()
        } else {
            offset - mapping.full_source_range.start
        };
        let content_offset =
            mapping.source_to_content[local.min(mapping.source_to_content.len().saturating_sub(1))];
        let block = mapping.entity.read(cx);
        Some(CrossBlockSelectionEndpoint {
            entity_id: mapping.entity.entity_id(),
            offset: block.markdown_offset_to_current_offset(content_offset),
        })
    }

    fn cross_block_source_range_for_normalized(
        &self,
        selection: NormalizedCrossBlockSelection,
        cx: &App,
    ) -> Option<Range<usize>> {
        let visible = self.document.visible_blocks().to_vec();

        // 端点先按块内偏移换算成缓冲区字节；算不出来的（表格这类整块原子的 cell）
        // 取这一块的边界，按它在选区的哪一头来。
        let endpoint_offset = |endpoint: CrossBlockSelectionEndpoint,
                               index: usize,
                               at_end: bool,
                               cx: &App|
         -> Option<usize> {
            if let Some(mapping) = self.source_mapping_for_entity(endpoint.entity_id, cx)
                && let Some(offset) = self.mapping_source_offset(&mapping, endpoint.offset, cx) {
                return Some(offset);
            }
            let range = self.block_source_range(visible.get(index)?.entity.entity_id(), cx)?;
            Some(if at_end { range.end } else { range.start })
        };

        let start = endpoint_offset(selection.start, selection.start_index, false, cx)?;
        let end = endpoint_offset(selection.end, selection.end_index, true, cx)?;
        let (mut lo, mut hi) = (start.min(end), start.max(end));

        // Endpoint offsets can never point *after* a zero-visible-len (atomic)
        // block, so a table at the trailing boundary of the selection would be
        // left behind. Union in the full source range of every atomic block
        // whose visible index falls inside the selection so it is removed whole.
        for index in selection.start_index..=selection.end_index {
            let entity = visible.get(index)?.entity.clone();
            if entity.read(cx).visible_len() == 0 {
                if let Some(range) = self.block_source_range(entity.entity_id(), cx) {
                    lo = lo.min(range.start);
                    hi = hi.max(range.end);
                }
            }
        }
        Some(self.clamp_source_range_to_boundaries(lo..hi))
    }

    /// 把缓冲区区间夹到字符边界上：写入端点不能落在半个多字节字符里。
    /// 只问缓冲区的边界位，不为了夹端点把整篇文本取出来。
    fn clamp_source_range_to_boundaries(&self, range: Range<usize>) -> Range<usize> {
        let clamp = |offset: usize| {
            let mut offset = offset.min(self.buffer.byte_len());
            while offset > 0 && !self.buffer.is_char_boundary(offset) {
                offset -= 1;
            }
            offset
        };
        let start = clamp(range.start);
        start..clamp(range.end).max(start)
    }

    /// 跨块改动只写选区那一段字节，其余字节一个不碰。
    ///
    /// 必须走区间：整篇写入之后重同步会从块树重新序列化全文，不相干的块就跟着
    /// 被洗（表格列宽填充重算、`__强调__` 变 `**…**`、Setext 转 ATX），撤销条目
    /// 也退化成一份全文副本。
    pub(crate) fn write_back_cross_block_source_edit(
        &mut self,
        source_range: Range<usize>,
        new_text: &str,
        cx: &mut Context<Self>,
    ) {
        let applied = self.buffer.edit(source_range, new_text);
        self.record_buffer_edit(applied);
        // 再从缓冲区重建投影：只有重建，根块才挂得上区间，之后的位置换算才有锚点。
        self.rebuild_document_from_buffer(cx);
    }

    fn apply_marked_source_range(&mut self, source_range: Range<usize>, cx: &mut Context<Self>) {
        if source_range.is_empty() {
            return;
        }
        let mappings = self.source_mappings_in_range(&source_range, cx);
        let Some(start) = self.endpoint_for_source_offset(source_range.start, &mappings, cx) else {
            return;
        };
        let Some(end) = self.endpoint_for_source_offset(source_range.end, &mappings, cx) else {
            return;
        };
        if start.entity_id != end.entity_id {
            return;
        }
        let Some(block) = self.focusable_entity_by_id(start.entity_id) else {
            return;
        };
        block.update(cx, |block, cx| {
            block.marked_range = Some(start.offset.min(end.offset)..start.offset.max(end.offset));
            cx.notify();
        });
    }

    pub(super) fn replace_cross_block_selection_with_text(
        &mut self,
        new_text: &str,
        selected_range_relative: Option<Range<usize>>,
        mark_inserted_text: bool,
        undo_kind: UndoCaptureKind,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(selection) = self.normalized_cross_block_selection(cx) else {
            return false;
        };
        let Some(source_range) = self.cross_block_source_range_for_normalized(selection, cx) else {
            return false;
        };

        self.prepare_undo_capture(undo_kind, cx);
        let start = source_range.start;
        self.cross_block_selection = None;
        self.cross_block_drag = None;

        let inserted_start = start;
        let inserted_end = inserted_start + new_text.len();
        let selected_source_range = selected_range_relative
            .map(|relative| {
                inserted_start + relative.start.min(new_text.len())
                    ..inserted_start + relative.end.min(new_text.len())
            })
            .unwrap_or(inserted_end..inserted_end);
        let marked_source_range =
            (mark_inserted_text && !new_text.is_empty()).then_some(inserted_start..inserted_end);

        self.write_back_cross_block_source_edit(source_range, new_text, cx);
        self.apply_selection_snapshot_in_current_mode(
            &UndoSelectionSnapshot {
                range: selected_source_range,
                reversed: false,
            },
            cx,
        );
        if let Some(marked_source_range) = marked_source_range {
            self.apply_marked_source_range(marked_source_range, cx);
        }
        self.mark_dirty_written_back(cx);
        self.finalize_pending_undo_capture(cx);
        self.sync_table_axis_visuals(cx);
        self.dismiss_contextual_overlays(cx);
        self.sync_cross_block_selection_visuals(cx);
        self.request_active_block_scroll_into_view(cx);
        cx.notify();
        true
    }

    /// 跨块复制交出去的就是缓冲区里的那段字节。
    ///
    /// 以前这里按块树的序列化口径逐块重拼再补空行：Setext 的下划线在这一笔里丢掉
    /// （复制—粘贴之后那一块不再是标题），紧排的列表项被撑开，写法与序列化口径不
    /// 一致的块还要整篇重拼 source mapping 才知道边界。选区的端点本来就能换算成
    /// 缓冲区位置，剪出来就是那段字节——空行与写法跟着文件走，不再需要猜测式记账。
    pub(super) fn cross_block_selected_markdown(&self, cx: &App) -> Option<String> {
        let selection = self.normalized_cross_block_selection(cx)?;
        let range = self.cross_block_source_range_for_normalized(selection, cx)?;
        Some(self.buffer.slice(range))
    }

    fn delete_cross_block_selection(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(selection) = self.normalized_cross_block_selection(cx) else {
            return false;
        };
        let Some(source_range) = self.cross_block_source_range_for_normalized(selection, cx) else {
            return false;
        };
        if source_range.is_empty() {
            return false;
        }

        self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
        let start = source_range.start;
        self.cross_block_selection = None;
        self.cross_block_drag = None;

        self.write_back_cross_block_source_edit(source_range, "", cx);

        self.apply_selection_snapshot_in_current_mode(
            &UndoSelectionSnapshot {
                range: start..start,
                reversed: false,
            },
            cx,
        );
        self.mark_dirty_written_back(cx);
        self.finalize_pending_undo_capture(cx);
        self.sync_table_axis_visuals(cx);
        self.dismiss_contextual_overlays(cx);
        self.sync_cross_block_selection_visuals(cx);
        cx.notify();
        true
    }

    /// 状态栏选词统计用的选中文本：只读可见文本，不序列化整篇文档、不重建
    /// source mapping。旧的 `selected_markdown_text` 是 O(整篇)（600 块文档
    /// 实测 38ms/次），而状态栏每帧都要算一次，长文档拖动选择直接卡死。
    pub(crate) fn selected_visible_text(&self, cx: &App) -> Option<String> {
        if let Some(selection) = self.normalized_cross_block_selection(cx) {
            let visible = self.document.visible_blocks();
            let mut text = String::new();
            let mut wrote_chunk = false;
            for index in selection.start_index..=selection.end_index {
                let block = visible.get(index)?.entity.read(cx);
                let len = block.visible_len();
                let range = if selection.start_index == selection.end_index {
                    selection.start.offset.min(len)..selection.end.offset.min(len)
                } else if index == selection.start_index {
                    selection.start.offset.min(len)..len
                } else if index == selection.end_index {
                    0..selection.end.offset.min(len)
                } else {
                    0..len
                };
                let display = block.display_text();
                let range = clamp_range_to_char_boundaries(display, range);
                if range.is_empty() {
                    continue;
                }
                if wrote_chunk {
                    text.push('\n');
                }
                text.push_str(&display[range]);
                wrote_chunk = true;
            }
            return wrote_chunk.then_some(text);
        }

        // Fall back to a single block with a non-collapsed selection range.
        for visible in self.document.visible_blocks() {
            let block = visible.entity.read_untracked(cx);
            if block.selected_range.is_empty() {
                continue;
            }
            let display = block.display_text();
            let range = clamp_range_to_char_boundaries(display, block.selected_range.clone());
            if !range.is_empty() {
                return Some(display[range].to_owned());
            }
        }

        None
    }

    /// Returns the markdown text of the current selection, whether cross-block
    /// or within a single block. Returns `None` when nothing is selected.
    pub(crate) fn selected_markdown_text(&self, cx: &App) -> Option<String> {
        // Prefer cross-block selection when present.
        if let Some(text) = self.cross_block_selected_markdown(cx) {
            if !text.is_empty() {
                return Some(text);
            }
        }

        // Fall back to a single block with a non-collapsed selection range.
        // P7：untracked 读——每帧全文档扫描时跳过 accessed 集合插入
        // （160k 块 × ~0.2µs 的纯追踪开销曾是稳态帧的大头）。
        for visible in self.document.visible_blocks() {
            let block = visible.entity.read_untracked(cx);
            if block.selected_range.is_empty() {
                continue;
            }
            let markdown_range =
                block.current_range_to_markdown_range(block.selected_range.clone());
            let full_markdown = block.record.title.serialize_markdown();
            // markdown 空间换算得到的偏移可能落在多字节字符内部，直接切片会 panic。
            let range = clamp_range_to_char_boundaries(&full_markdown, markdown_range.clone());
            if !range.is_empty() {
                return Some(full_markdown[range].to_owned());
            }
        }

        None
    }
}


#[cfg(test)]
mod tests;
