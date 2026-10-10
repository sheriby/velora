//! 文件本地历史：每次成功保存落一条版本快照，浮层浏览、Enter 恢复为
//! 未保存修改（不直接写盘，可继续编辑或撤销）。

use std::path::PathBuf;

use gpui::*;

use super::{HistoryEntry, buffer};
use crate::config;
use crate::i18n::I18nStrings;
use crate::theme::Theme;

use super::Editor;

/// 浮层会话：版本列表（最新在前）+ 当前选中项。
pub(crate) struct FileHistoryOverlay {
    pub(crate) versions: Vec<PathBuf>,
    pub(crate) selected: usize,
}

impl Editor {
    pub(crate) fn file_history_is_open(&self) -> bool {
        self.file_history_overlay.is_some()
    }

    /// 打开浮层：读取当前文档对应的历史版本（同步读，≤20 个小文件）。
    pub(crate) fn open_file_history(&mut self, cx: &mut Context<Self>) {
        self.dismiss_contextual_overlays(cx);
        let Some(path) = self.file_path.clone() else {
            return;
        };
        let versions = config::list_file_history(&path);
        self.file_history_overlay = Some(super::file_history::FileHistoryOverlay {
            versions,
            selected: 0,
        });
        cx.notify();
    }

    pub(crate) fn close_file_history(&mut self, cx: &mut Context<Self>) {
        if self.file_history_overlay.take().is_some() {
            cx.notify();
        }
    }

    /// 浮层按键（intercept_keystrokes 钩子调用）：↑/↓ 选择、Enter 恢复、
    /// Esc 关闭。返回是否消费。
    pub(crate) fn file_history_key_down(
        &mut self,
        keystroke: &gpui::Keystroke,
        cx: &mut gpui::Context<Self>,
    ) -> bool {
        if !self.file_history_is_open() {
            return false;
        }
        match keystroke.key.as_str() {
            "up" | "down" => {
                if let Some(overlay) = self.file_history_overlay.as_mut()
                    && !overlay.versions.is_empty()
                {
                    let count = overlay.versions.len();
                    if keystroke.key == "up" {
                        overlay.selected = (overlay.selected + count - 1) % count;
                    } else {
                        overlay.selected = (overlay.selected + 1) % count;
                    }
                    cx.notify();
                }
                cx.stop_propagation();
                true
            }
            "enter" => {
                let selected = self
                    .file_history_overlay
                    .as_ref()
                    .map(|overlay| overlay.selected)
                    .unwrap_or(0);
                cx.stop_propagation();
                self.restore_file_history_version(selected, cx);
                true
            }
            "escape" => {
                cx.stop_propagation();
                self.close_file_history(cx);
                true
            }
            _ => false,
        }
    }

    /// 恢复版本：整篇替换为历史内容并标记未保存（用户可继续编辑/撤销）。
    pub(crate) fn restore_file_history_version(
        &mut self,
        selected: usize,
        cx: &mut gpui::Context<Self>,
    ) {
        let Some(overlay) = self.file_history_overlay.take() else {
            return;
        };
        let Some(path) = overlay.versions.get(selected) else {
            cx.notify();
            return;
        };
        let Ok(content) = std::fs::read_to_string(path) else {
            let strings = cx.global::<crate::i18n::I18nManager>().strings().clone();
            self.show_message_modal(
                strings.menu_file_history.clone(),
                strings.file_history_empty.clone(),
                cx,
            );
            return;
        };
        let file_path = self.file_path.clone();
        // 兑现「可撤销」：恢复前把当前文档压进撤销栈，⌘Z 能回到恢复前的
        // 内容（用户报修：恢复会清空整个 undo 栈，误按 Enter 就丢掉当前
        // 未保存的修改且无法撤回）。
        let selection_before = self.capture_source_selection_snapshot(cx);
        let pre_restore_text = self.buffer.text();
        self.pending_undo_capture = None;
        self.replace_document_from_markdown(content, file_path, cx);
        self.undo_history.clear();
        self.redo_history.clear();
        // 恢复整篇文档 = 一次「全文替换」的写入：撤销把它换回来。
        self.undo_history.push(HistoryEntry {
            edits: vec![buffer::AppliedEdit {
                removed: pre_restore_text,
                new_range: 0..self.buffer.byte_len(),
            }],
            selection: selection_before,
            timestamp: std::time::Instant::now(),
            kind: crate::components::UndoCaptureKind::NonCoalescible,
        });
        self.mark_dirty(cx);
        cx.notify();
    }

    /// 历史浮层：居中列表面板，行 = UTC 时间戳，Enter/点击恢复。
    pub(crate) fn render_file_history_overlay(
        &mut self,
        theme: &Theme,
        strings: &I18nStrings,
        cx: &mut gpui::Context<Self>,
    ) -> Option<AnyElement> {
        use gpui::*;
        let overlay = self.file_history_overlay.as_ref()?;
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let selected = overlay.selected;
        let versions = overlay.versions.clone();
        if versions.is_empty() {
            return Some(
                div()
                    .id("file-history-overlay")
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .occlude()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(c.dialog_backdrop)
                    .on_mouse_down(MouseButton::Left, cx.listener(|editor, _: &MouseDownEvent, _window, cx| {
                        editor.close_file_history(cx);
                    }))
                    .child(
                        div()
                            .w(px(d.dialog_width))
                            .p(px(d.dialog_padding))
                            .rounded(px(d.dialog_radius))
                            .bg(c.dialog_surface)
                            .border(px(d.dialog_border_width))
                            .border_color(c.dialog_border)
                            .text_color(c.dialog_muted)
                            .child(strings.file_history_empty.clone()),
                    )
                    .into_any_element(),
            );
        }

        let mut rows = Vec::new();
        for (index, version) in versions.iter().enumerate() {
            let label = version
                .file_stem()
                .and_then(|stem| stem.to_str())
                .and_then(|stem| stem.parse::<u64>().ok())
                .map(|millis| {
                    let seconds = millis / 1000;
                    super::workspace::chrono_like_date_string_public(seconds)
                })
                .unwrap_or_else(|| version.display().to_string());
            let restore = cx.listener(move |editor, _: &MouseDownEvent, _window, cx| {
                editor.restore_file_history_version(index, cx);
            });
            rows.push(
                div()
                    .id(gpui::ElementId::Name(format!("file-history-entry-{index}").into()))
                    .debug_selector(move || format!("file-history-entry-{index}"))
                    .h(px(28.0))
                    .w_full()
                    .overflow_hidden()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(px(8.0))
                    .rounded(px(4.0))
                    .cursor_pointer()
                    .bg(if index == selected {
                        c.selection
                    } else {
                        hsla(0.0, 0.0, 0.0, 0.0)
                    })
                    .hover(|this| this.bg(c.dialog_secondary_button_hover))
                    .on_mouse_down(MouseButton::Left, restore)
                    .child(
                        div()
                            .text_size(px(t.ui_text_size(16.0 * 0.92)))
                            .text_color(c.text_default)
                            .child(label),
                    )
                    .child(
                        div()
                            .text_size(px(t.ui_text_size(16.0 * 0.78)))
                            .text_color(c.dialog_muted)
                            .child(format!("#{}", index + 1)),
                    ),
            );
        }

        Some(
            div()
                .id("file-history-overlay")
                .debug_selector(|| "file-history-overlay".to_string())
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .bottom_0()
                .occlude()
                .flex()
                .items_center()
                .justify_center()
                .bg(c.dialog_backdrop)
                .on_mouse_down(MouseButton::Left, cx.listener(|editor, _: &MouseDownEvent, _window, cx| {
                    editor.close_file_history(cx);
                }))
                .child(
                    div()
                        .w(px(d.dialog_width))
                        .max_w(relative(1.0))
                        .flex()
                        .flex_col()
                        .gap(px(d.dialog_gap))
                        .p(px(d.dialog_padding))
                        .rounded(px(d.dialog_radius))
                        .bg(c.dialog_surface)
                        .border(px(d.dialog_border_width))
                        .border_color(c.dialog_border)
                        .shadow_lg()
                        .child(
                            div()
                                .text_size(px(t.dialog_title_size))
                                .font_weight(t.dialog_title_weight.to_font_weight())
                                .text_color(c.dialog_title)
                                .child(strings.menu_file_history.clone()),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(2.0))
                                .children(rows),
                        ),
                )
                .into_any_element(),
        )
    }
}

use crate::components::FileHistory;

impl Editor {
    /// 菜单/命令面板入口：打开当前文档的历史浮层。
    pub(crate) fn on_file_history_action(
        &mut self,
        _: &FileHistory,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_file_history(cx);
    }
}
