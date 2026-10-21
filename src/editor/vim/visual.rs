//! vim 可视模式：锚点管理、选区扩展与渲染驱动。
//!
//! 选区的渲染完全走应用自己的选区机制：同块写 `block.selected_range`，
//! 跨块写 `cross_block_selection` + `sync_cross_block_selection_visuals`
//! （指针选区同一条路）。块状可视（ctrl-q）的「列」取硬行内字素序号——
//! 非等宽字体下没有真正的视觉列，这是可确定性测试的口径，记为已知适配。

use gpui::{App, Context, Entity, Window};

use crate::components::Block;
use crate::editor::Editor;

use super::motion::{next_grapheme_start, VimDocStream, VimPosition};
use super::operate::VimRange;
use super::{VisualKind, VimMode};

/// 跨块选区端点（应用侧类型来自 editor 模块）。
use crate::editor::CrossBlockSelectionEndpoint as Endpoint;

impl Editor {
    /// v / V / ctrl-q：进入/切换/退出可视。
    pub(super) fn vim_toggle_visual_inner(&mut self, kind: VisualKind, cx: &mut Context<Self>) {
        match self.vim.mode {
            VimMode::Visual(current) if current == kind => {
                self.vim_exit_visual_inner(cx);
            }
            VimMode::Visual(_) => {
                // 切换种类：锚点不动，重算选区形状。
                self.vim_enter_mode(VimMode::Visual(kind), cx);
                self.vim_refresh_visual_selection(cx);
            }
            _ => {
                let entities = self.vim_navigation_entities(cx);
                if let Some(cursor) = self.vim_current_position(&entities, cx) {
                    self.vim.visual_anchor = Some(cursor);
                    self.vim.visual_focus = Some(cursor);
                    self.vim.visual_block_anchor_column =
                        Some(self.vim_grapheme_column(&entities, cursor, cx));
                }
                self.vim_enter_mode(VimMode::Visual(kind), cx);
                self.vim_refresh_visual_selection(cx);
            }
        }
    }

    /// 退出可视：收起选区，光标停在当前焦点。
    pub(super) fn vim_exit_visual_inner(&mut self, cx: &mut Context<Self>) {
        let entities = self.vim_navigation_entities(cx);
        let landed = self.vim_current_position(&entities, cx);
        if let Some(cursor) = landed
            && let Some(anchor) = self.vim.visual_anchor
        {
            self.vim.last_visual = Some((anchor, cursor, self.vim_visual_kind()));
        }
        self.vim.visual_anchor = None;
        self.vim.visual_focus = None;
        self.vim.visual_block_anchor_column = None;
        self.clear_cross_block_selection(cx);
        self.vim_enter_mode(VimMode::Normal, cx);
        // 收起选区后光标落在可视焦点上（块光标先前停在排他端点）。
        let entities = self.vim_navigation_entities(cx);
        if let Some(cursor) = landed {
            self.vim_set_cursor(&entities, cursor, cx);
        }
    }

    fn vim_visual_kind(&self) -> VisualKind {
        match self.vim.mode {
            VimMode::Visual(kind) => kind,
            _ => VisualKind::Char,
        }
    }

    /// 硬行内字素序号（块状可视的列）。
    fn vim_grapheme_column(
        &self,
        entities: &[Entity<Block>],
        position: VimPosition,
        cx: &App,
    ) -> usize {
        let stream = VimDocStream::new(entities, cx);
        let line = stream.hard_line_range(position.block_index, position.offset);
        let text = stream.text_for_line(position.block_index);
        let mut column = 0usize;
        let mut offset = line.start;
        while offset < position.offset.min(line.end) {
            let Some(next) = next_grapheme_start(text, offset) else { break };
            offset = next;
            column += 1;
        }
        column
    }

    /// 锚点 + 当前光标 → 区间（操作符与命令行共用）。
    pub(super) fn vim_selection_as_range_inner(
        &self,
        entities: &[Entity<Block>],
        cx: &App,
    ) -> Option<VimRange> {
        let anchor = self.vim.visual_anchor?;
        let focus = self.vim_current_position(entities, cx)?;
        let kind = self.vim_visual_kind();
        match kind {
            VisualKind::Char => {
                let (start, end) = if (anchor.block_index, anchor.offset)
                    <= (focus.block_index, focus.offset)
                {
                    (anchor, focus)
                } else {
                    (focus, anchor)
                };
                let stream = VimDocStream::new(entities, cx);
                // 可视选区含焦点所在字素（独占端 +1 字素）。焦点落在块末字素时
                // next_grapheme 返回 None，独占端要取块尾，否则末字被漏掉。
                let end_inclusive = if end == anchor && end == focus {
                    end
                } else {
                    let text_len = stream.text_for_line(end.block_index).len();
                    if end.offset >= text_len {
                        end
                    } else {
                        stream
                            .next_grapheme(end)
                            .filter(|next| next.block_index == end.block_index)
                            .unwrap_or(VimPosition {
                                block_index: end.block_index,
                                offset: text_len,
                            })
                    }
                };
                Some(VimRange { start, end: end_inclusive, linewise: false })
            }
            VisualKind::Line => {
                let stream = VimDocStream::new(entities, cx);
                let (start, end) = if anchor.block_index <= focus.block_index {
                    (anchor, focus)
                } else {
                    (focus, anchor)
                };
                let start_line = stream.hard_line_range(start.block_index, start.offset);
                // 行级区间含行尾换行：末行之后还有行就落到下一行行首。
                let end_line = stream.hard_line_range(end.block_index, end.offset);
                let end_text = stream.text_for_line(end.block_index);
                let (end_block, end_offset) = if end_line.end < end_text.len() {
                    (end.block_index, end_line.end + 1)
                } else if end.block_index + 1 < entities.len() {
                    (end.block_index + 1, 0)
                } else {
                    (end.block_index, end_text.len())
                };
                Some(VimRange {
                    start: VimPosition { block_index: start.block_index, offset: start_line.start },
                    end: VimPosition { block_index: end_block, offset: end_offset },
                    linewise: true,
                })
            }
            VisualKind::Block => {
                // 行级区域：锚点行到焦点行；列：锚点列到焦点列（逐行展开在
                // vim_block_visual_ranges 里按同两列重算，这里只给首行区间）。
                let (first, last) = if anchor.block_index <= focus.block_index {
                    (anchor, focus)
                } else {
                    (focus, anchor)
                };
                let stream = VimDocStream::new(entities, cx);
                let start_line = stream.hard_line_range(first.block_index, first.offset);
                Some(VimRange {
                    start: VimPosition {
                        block_index: first.block_index,
                        offset: start_line.start,
                    },
                    end: VimPosition { block_index: last.block_index, offset: 0 },
                    linewise: false,
                })
            }
        }
    }

    /// 块状可视的逐硬行区间（行号 → 行内显示范围）。
    pub(super) fn vim_block_visual_ranges(
        &self,
        entities: &[Entity<Block>],
        cx: &App,
    ) -> Option<Vec<(usize, std::ops::Range<usize>)>> {
        let anchor = self.vim.visual_anchor?;
        let focus = self.vim_current_position(entities, cx)?;
        let anchor_column = self.vim.visual_block_anchor_column.unwrap_or(0);
        let focus_column = self.vim_grapheme_column(entities, focus, cx);
        let column_start = anchor_column.min(focus_column);
        // 块选含焦点所在的那一列，区间右端是排他的。
        let column_end = anchor_column.max(focus_column) + 1;
        let stream = VimDocStream::new(entities, cx);
        let (start_position, end_position) =
            if (anchor.block_index, anchor.offset) <= (focus.block_index, focus.offset) {
                (anchor, focus)
            } else {
                (focus, anchor)
            };
        let first = start_position.block_index;
        let last = end_position.block_index;
        let mut ranges = Vec::new();
        for block_index in first..=last {
            let text = stream.text_for_line(block_index);
            let mut line_starts = vec![0usize];
            for (index, ch) in text.char_indices() {
                if ch == '\n' {
                    line_starts.push(index + 1);
                }
            }
            let line_of = |offset: usize| {
                text.get(..offset.min(text.len()))
                    .map_or(0, |before| before.matches('\n').count())
            };
            let start_line = if block_index == first {
                line_of(start_position.offset)
            } else {
                0
            };
            let end_line = if block_index == last {
                line_of(end_position.offset)
            } else {
                line_starts.len().saturating_sub(1)
            };
            for line_index in start_line..=end_line.min(line_starts.len().saturating_sub(1)) {
                let line_start = line_starts[line_index];
                let line_end = line_starts
                    .get(line_index + 1)
                    .map(|next| next.saturating_sub(1))
                    .unwrap_or(text.len());
                // 列 → 字节偏移。
                let mut col_offset = line_start;
                let mut column = 0usize;
                while column < column_start && col_offset < line_end {
                    let Some(next) = next_grapheme_start(text, col_offset) else { break };
                    col_offset = next;
                    column += 1;
                }
                let mut col_end_offset = col_offset;
                while column < column_end && col_end_offset < line_end {
                    let Some(next) = next_grapheme_start(text, col_end_offset) else { break };
                    col_end_offset = next;
                    column += 1;
                }
                if col_end_offset > col_offset {
                    ranges.push((block_index, col_offset..col_end_offset));
                }
            }
        }
        if ranges.is_empty() { None } else { Some(ranges) }
    }

    /// 动作扩展选区（可视模式下的移动）。
    pub(super) fn vim_extend_with_motion_inner(
        &mut self,
        motion: super::motion::Motion,
        repeat: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // 用与普通模式完全相同的动作解析算目标，但只落焦点、随后刷新选区。
        self.vim_move_by(motion, repeat, window, cx);
        self.vim_refresh_visual_selection(cx);
    }

    /// 把锚点→焦点区间写进应用选区（同块 selected_range / 跨块 cross_block）。
    pub(super) fn vim_refresh_visual_selection(&mut self, cx: &mut Context<Self>) {
        let kind = self.vim_visual_kind();
        let entities = self.vim_navigation_entities(cx);
        let Some(range) = self.vim_selection_as_range_inner(&entities, cx) else {
            return;
        };
        if kind == VisualKind::Block {
            // 块状：逐块逐行写选区（同块多行时块内只有一段能显示，取并集）。
            if let Some(ranges) = self.vim_block_visual_ranges(&entities, cx) {
                self.clear_cross_block_selection(cx);
                for (block_index, range) in ranges {
                    if let Some(entity) = entities.get(block_index) {
                        entity.update(cx, |block, cx| {
                            block.set_selection_from_anchor_focus(range.start, range.end);
                            cx.notify();
                        });
                    }
                }
            }
            return;
        }
        if range.start.block_index == range.end.block_index {
            self.clear_cross_block_selection(cx);
            if let Some(entity) = entities.get(range.start.block_index) {
                entity.update(cx, |block, cx| {
                    block.set_selection_from_anchor_focus(range.start.offset, range.end.offset);
                    cx.notify();
                });
            }
            return;
        }
        // 跨块：写 editor 级选区（指针选区同一条路），焦点块保持光标。
        let anchor_entity = entities.get(range.start.block_index).cloned();
        let focus_entity = entities.get(range.end.block_index).cloned();
        if let (Some(anchor_entity), Some(focus_entity)) = (anchor_entity, focus_entity) {
            self.cross_block_selection = Some(crate::editor::CrossBlockSelection {
                anchor: Endpoint {
                    entity_id: anchor_entity.entity_id(),
                    offset: range.start.offset,
                },
                focus: Endpoint {
                    entity_id: focus_entity.entity_id(),
                    offset: range.end.offset,
                },
            });
            self.cross_block_drag = None;
            self.sync_cross_block_selection_visuals(cx);
            cx.notify();
        }
    }

    /// o：交换锚点与焦点。
    pub(super) fn vim_exchange_anchor_inner(&mut self, cx: &mut Context<Self>) {
        let entities = self.vim_navigation_entities(cx);
        let Some(focus) = self.vim_current_position(&entities, cx) else { return };
        let Some(anchor) = self.vim.visual_anchor else { return };
        self.vim.visual_anchor = Some(focus);
        self.vim.visual_block_anchor_column = Some(self.vim_grapheme_column(&entities, anchor, cx));
        self.vim_set_cursor(&entities, anchor, cx);
        self.vim_refresh_visual_selection(cx);
    }

    /// 块状可视 O：交换锚点列与焦点列。
    pub(super) fn vim_swap_block_column_inner(&mut self, cx: &mut Context<Self>) {
        let entities = self.vim_navigation_entities(cx);
        let Some(focus) = self.vim_current_position(&entities, cx) else { return };
        let focus_column = self.vim_grapheme_column(&entities, focus, cx);
        let anchor_column = self.vim.visual_block_anchor_column;
        self.vim.visual_block_anchor_column = Some(focus_column);
        let _ = anchor_column;
        self.vim_refresh_visual_selection(cx);
    }

    /// 可视模式里 i"/a"/iw…：把选区扩到对象。
    pub(super) fn vim_extend_to_object_inner(
        &mut self,
        object: super::operate::TextObject,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let entities = self.vim_navigation_entities(cx);
        let Some(cursor) = self.vim_current_position(&entities, cx) else { return };
        let Some(range) = self.vim_text_object_range(&entities, cursor, object, cx)
        else {
            return;
        };
        self.vim.visual_anchor = Some(range.start);
        self.vim_set_cursor(&entities, range.end, cx);
        // 对象选区含端点：焦点右移一字素。
        let stream = VimDocStream::new(&entities, cx);
        if let Some(next) = stream.next_grapheme(range.end) {
            self.vim_set_cursor(&entities, next, cx);
        }
        self.vim_refresh_visual_selection(cx);
        let _ = window;
    }

    /// gv：重选上次可视选区。
    pub(super) fn vim_reselect_inner(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((start, end, kind)) = self.vim.last_visual else { return };
        let entities = self.vim_navigation_entities(cx);
        if entities.get(start.block_index).is_none() || entities.get(end.block_index).is_none() {
            return;
        }
        self.vim.visual_anchor = Some(start);
        self.vim_enter_mode(VimMode::Visual(kind), cx);
        self.vim_set_cursor(&entities, end, cx);
        self.vim_refresh_visual_selection(cx);
        let _ = window;
    }
}

// ---- 模块门面（vim.rs 执行器调用）----

pub(super) fn vim_toggle_visual(
    editor: &mut Editor,
    kind: VisualKind,
    cx: &mut Context<Editor>,
) {
    editor.vim_toggle_visual_inner(kind, cx);
}

pub(super) fn vim_exit_visual(editor: &mut Editor, cx: &mut Context<Editor>) {
    editor.vim_exit_visual_inner(cx);
}

pub(super) fn vim_extend_with_motion(
    editor: &mut Editor,
    motion: super::motion::Motion,
    repeat: u32,
    window: &mut Window,
    cx: &mut Context<Editor>,
) {
    editor.vim_extend_with_motion_inner(motion, repeat, window, cx);
}

pub(super) fn vim_extend_to_object(
    editor: &mut Editor,
    object: super::operate::TextObject,
    window: &mut Window,
    cx: &mut Context<Editor>,
) {
    editor.vim_extend_to_object_inner(object, window, cx);
}

pub(super) fn vim_exchange_anchor(editor: &mut Editor, cx: &mut Context<Editor>) {
    editor.vim_exchange_anchor_inner(cx);
}

pub(super) fn vim_swap_block_column(editor: &mut Editor, cx: &mut Context<Editor>) {
    editor.vim_swap_block_column_inner(cx);
}

pub(super) fn vim_reselect(editor: &mut Editor, window: &mut Window, cx: &mut Context<Editor>) {
    editor.vim_reselect_inner(window, cx);
}

pub(super) fn vim_selection_as_range(
    editor: &Editor,
    entities: &[Entity<Block>],
    cx: &App,
) -> Option<VimRange> {
    editor.vim_selection_as_range_inner(entities, cx)
}
