//! 选中菜单、右键菜单与快捷键共用的编辑入口：行内格式。
//!
//! 三条入口走同一段代码，行为一致——尤其是「选区横跨多个块」这一种：块的行内样式
//! 不跨块（markdown 本身就不跨），所以按块切段逐块处理，但撤销只算一次。

use gpui::*;

use super::Editor;
use crate::components::{
    BoldSelection, ClearFormatSelection, CodeSelection, HighlightSelection, InlineFormat,
    ItalicSelection, StrikethroughSelection, SubscriptSelection, SuperscriptSelection,
    UnderlineSelection, UndoCaptureKind,
};

/// 选区上的一次行内改动：开关某一种格式，或者把选区里所有样式记号剥掉。
#[derive(Clone, Copy)]
enum InlineSelectionEdit {
    Format(InlineFormat),
    ClearStyles,
}

impl Editor {
    /// 在当前选区上开关一种行内格式，返回是否改到了内容。
    ///
    /// 跨块选区按可见块顺序切成几段，每段用所在块自己的可见文本坐标；同一次操作
    /// 只开一个撤销组，一次撤销回到原样。
    pub(crate) fn toggle_inline_format_on_selection(
        &mut self,
        format: InlineFormat,
        cx: &mut Context<Self>,
    ) -> bool {
        self.apply_inline_selection_edit(InlineSelectionEdit::Format(format), cx)
    }

    /// 「清除格式」：剥掉选区里的行内样式记号。块级记号（`#`、`-`、`>`、围栏）与链接不动，
    /// 那两样不是「样式」。跨块与撤销口径与开关一种格式完全同一条。
    pub(crate) fn clear_inline_format_on_selection(&mut self, cx: &mut Context<Self>) -> bool {
        self.apply_inline_selection_edit(InlineSelectionEdit::ClearStyles, cx)
    }

    /// 「清除格式」现在点得动吗：要有选区、写得动缓冲区，并且选区里确实挂着成对的行内样式。
    /// 右键菜单那一行与选中工具栏那颗格子共用这一条判定；键盘不查它——按下去没有可剥的就
    /// 什么都不做，与「菜单里灰掉的行按同一个键没反应」是同一条口径的两面。
    pub(crate) fn clear_format_is_available(&self, cx: &App) -> bool {
        if !self.has_text_selection(cx) || !self.writes_through_the_buffer() {
            return false;
        }
        let Some(normalized) = self.normalized_cross_block_selection(cx) else {
            let Some(target) = self.current_edit_target_from_state(cx) else {
                return false;
            };
            return target.read(cx).has_inline_styles_in_selection();
        };
        let block_count = normalized.end_index - normalized.start_index + 1;
        (0..block_count).any(|position| {
            let Some(entity) = self
                .document
                .visible_blocks()
                .get(normalized.start_index + position)
                .map(|block| block.entity.clone())
            else {
                return false;
            };
            // 选区存的是干净坐标，块自己认的是显示坐标：换算完再问它有没有样式。
            let block = entity.read(cx);
            let len = block.clean_visible_len();
            let start = if position == 0 {
                normalized.start.offset.min(len)
            } else {
                0
            };
            let end = if position + 1 == block_count {
                normalized.end.offset.min(len)
            } else {
                len
            };
            let range = block.clean_range_to_display_range(start..end);
            !range.is_empty() && block.has_inline_styles_in_range(range)
        })
    }

    /// 选区上的一次行内改动，两条入口共用这一段切块与记账。
    fn apply_inline_selection_edit(
        &mut self,
        edit: InlineSelectionEdit,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(normalized) = self.normalized_cross_block_selection(cx) else {
            let Some(target) = self.current_edit_target_from_state(cx) else {
                return false;
            };
            return target.update(cx, |block, cx| match edit {
                InlineSelectionEdit::Format(format) => block.toggle_inline_format(format, cx),
                InlineSelectionEdit::ClearStyles => block.clear_inline_format(cx),
            });
        };
        let block_count = normalized.end_index - normalized.start_index + 1;
        let mut changed = false;
        self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
        for position in 0..block_count {
            let block_index = normalized.start_index + position;
            let Some(entity) = self
                .document
                .visible_blocks()
                .get(block_index)
                .map(|block| block.entity.clone())
            else {
                continue;
            };
            let (start, end) = {
                // 选区存干净坐标，块那一侧认显示坐标。
                let block = entity.read(cx);
                let len = block.clean_visible_len();
                let start = if position == 0 {
                    normalized.start.offset.min(len)
                } else {
                    0
                };
                let end = if position + 1 == block_count {
                    normalized.end.offset.min(len)
                } else {
                    len
                };
                let range = block.clean_range_to_display_range(start..end);
                (range.start, range.end)
            };
            if start >= end {
                continue;
            }
            if entity.update(cx, |block, cx| match edit {
                InlineSelectionEdit::Format(format) => block.toggle_inline_format_in_range(
                    format,
                    start..end,
                    Some(normalized.reversed),
                    cx,
                ),
                InlineSelectionEdit::ClearStyles => {
                    block.clear_inline_styles_in_range(start..end, Some(normalized.reversed), cx)
                }
            }) {
                changed = true;
            }
        }
        self.finalize_pending_undo_capture(cx);
        if changed {
            cx.notify();
        }
        changed
    }

    /// 键盘、右键菜单与命令面板都到这里为止：焦点在某一块上时交给块自己（块里那条
    /// 路径不动），跨块选区与「焦点被浮层借走」这两种由这一段收掉；后一种为什么要
    /// 在编辑器层接手，见 `Self::block_focus_is_live`。
    fn handle_inline_format_capture(
        &mut self,
        format: InlineFormat,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        if self.cross_block_selection.is_none() && self.block_focus_is_live(window, cx) {
            cx.propagate();
            return;
        }
        self.toggle_inline_format_on_selection(format, cx);
        cx.stop_propagation();
    }

    pub(crate) fn on_bold_capture(
        &mut self,
        _: &BoldSelection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.handle_inline_format_capture(InlineFormat::Bold, window, cx);
    }

    pub(crate) fn on_italic_capture(
        &mut self,
        _: &ItalicSelection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.handle_inline_format_capture(InlineFormat::Italic, window, cx);
    }

    pub(crate) fn on_underline_capture(
        &mut self,
        _: &UnderlineSelection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.handle_inline_format_capture(InlineFormat::Underline, window, cx);
    }

    pub(crate) fn on_strikethrough_capture(
        &mut self,
        _: &StrikethroughSelection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.handle_inline_format_capture(InlineFormat::Strikethrough, window, cx);
    }

    pub(crate) fn on_highlight_capture(
        &mut self,
        _: &HighlightSelection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.handle_inline_format_capture(InlineFormat::Highlight, window, cx);
    }

    pub(crate) fn on_code_capture(
        &mut self,
        _: &CodeSelection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.handle_inline_format_capture(InlineFormat::Code, window, cx);
    }

    pub(crate) fn on_superscript_capture(
        &mut self,
        _: &SuperscriptSelection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.handle_inline_format_capture(InlineFormat::Superscript, window, cx);
    }

    pub(crate) fn on_subscript_capture(
        &mut self,
        _: &SubscriptSelection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.handle_inline_format_capture(InlineFormat::Subscript, window, cx);
    }

    pub(crate) fn on_clear_format_capture(
        &mut self,
        _: &ClearFormatSelection,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.clear_inline_format_on_selection(cx) {
            cx.stop_propagation();
        }
    }
}
