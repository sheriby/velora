//! 选中菜单、右键菜单与快捷键共用的编辑入口：行内格式。
//!
//! 三条入口走同一段代码，行为一致——尤其是「选区横跨多个块」这一种：块的行内样式
//! 不跨块（markdown 本身就不跨），所以按块切段逐块处理，但撤销只算一次。

use gpui::*;

use super::Editor;
use crate::components::{
    BoldSelection, CodeSelection, HighlightSelection, InlineFormat, ItalicSelection,
    StrikethroughSelection, SubscriptSelection, SuperscriptSelection, UnderlineSelection,
    UndoCaptureKind,
};

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
        let Some(normalized) = self.normalized_cross_block_selection(cx) else {
            let Some(target) = self.current_edit_target_from_state(cx) else {
                return false;
            };
            return target.update(cx, |block, cx| block.toggle_inline_format(format, cx));
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
            let start = if position == 0 {
                normalized.start.offset
            } else {
                0
            };
            let end = if position + 1 == block_count {
                normalized.end.offset
            } else {
                entity.read(cx).visible_len()
            };
            if start >= end {
                continue;
            }
            if entity.update(cx, |block, cx| {
                block.toggle_inline_format_in_range(
                    format,
                    start..end,
                    Some(normalized.reversed),
                    cx,
                )
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

    /// 键盘走这里：跨块选区在编辑器层收掉（块自己看不到别的块），单块选区继续往下
    /// 传给块，沿用块里已有的那条路径。
    fn handle_inline_format_capture(&mut self, format: InlineFormat, cx: &mut Context<Self>) {
        if self.cross_block_selection.is_none() {
            cx.propagate();
            return;
        }
        self.toggle_inline_format_on_selection(format, cx);
        cx.stop_propagation();
    }

    pub(crate) fn on_bold_capture(
        &mut self,
        _: &BoldSelection,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.handle_inline_format_capture(InlineFormat::Bold, cx);
    }

    pub(crate) fn on_italic_capture(
        &mut self,
        _: &ItalicSelection,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.handle_inline_format_capture(InlineFormat::Italic, cx);
    }

    pub(crate) fn on_underline_capture(
        &mut self,
        _: &UnderlineSelection,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.handle_inline_format_capture(InlineFormat::Underline, cx);
    }

    pub(crate) fn on_strikethrough_capture(
        &mut self,
        _: &StrikethroughSelection,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.handle_inline_format_capture(InlineFormat::Strikethrough, cx);
    }

    pub(crate) fn on_highlight_capture(
        &mut self,
        _: &HighlightSelection,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.handle_inline_format_capture(InlineFormat::Highlight, cx);
    }

    pub(crate) fn on_code_capture(
        &mut self,
        _: &CodeSelection,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.handle_inline_format_capture(InlineFormat::Code, cx);
    }

    pub(crate) fn on_superscript_capture(
        &mut self,
        _: &SuperscriptSelection,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.handle_inline_format_capture(InlineFormat::Superscript, cx);
    }

    pub(crate) fn on_subscript_capture(
        &mut self,
        _: &SubscriptSelection,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.handle_inline_format_capture(InlineFormat::Subscript, cx);
    }
}
