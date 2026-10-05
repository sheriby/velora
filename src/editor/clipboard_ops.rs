//! 命令面板与快速打开借走窗口焦点时，「粘贴为纯文本」在编辑器层的收口。
//!
//! 实现只在块里那一处：这一段在焦点不在任何一块上时代为认出当前编辑目标，
//! 调块里那同一条路径（跨块选区、剪贴板图片那些分支都跟着一起走）。
//! 焦点还在块上时原样往下传，⌘⇧V 那一条不动。

use gpui::*;

use super::Editor;
use crate::components::PasteAsPlainText;

impl Editor {
    pub(crate) fn on_paste_as_plain_text_capture(
        &mut self,
        action: &PasteAsPlainText,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.block_focus_is_live(window, cx) {
            cx.propagate();
            return;
        }
        let Some(target) = self.current_edit_target_from_state(cx) else {
            return;
        };
        target.update(cx, |block, block_cx| {
            block.on_paste_as_plain_text(action, window, block_cx);
        });
        cx.stop_propagation();
    }
}
