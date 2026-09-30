//! Quick file switcher (⌘P / roadmap E1): fuzzy-match workspace file names
//! and open the selection in the current window.

use std::path::PathBuf;

use gpui::*;
use unicode_segmentation::UnicodeSegmentation;

use super::Editor;
use crate::i18n::I18nManager;
use crate::theme::Theme;

/// Overlay state for the quick switcher. `None` while closed.
#[derive(Default)]
pub(in crate::editor) struct QuickOpenState {
    pub(super) query: String,
    pub(super) focus: Option<FocusHandle>,
    pub(super) selected: usize,
    pub(super) results: Vec<PathBuf>,
    pub(super) selected_range: std::ops::Range<usize>,
    pub(super) marked_range: Option<std::ops::Range<usize>>,
}

impl Editor {
    pub(crate) fn on_quick_open_action(
        &mut self,
        _: &crate::components::QuickOpen,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_quick_open(window, cx);
    }

    /// Top-centered overlay panel: query line + fuzzy result rows.
    pub(super) fn render_quick_open_overlay(
        &self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let strings = cx.global::<I18nManager>().strings();
        let Some(state) = self.quick_open.as_ref() else {
            return div().into_any_element();
        };
        let Some(focus) = state.focus.clone() else {
            return div().into_any_element();
        };
        let mut rows: Vec<AnyElement> = Vec::new();
        let results = &state.results;
        if results.is_empty() {
            rows.push(
                div()
                    .px(px(10.0))
                    .h(px(28.0))
                    .flex()
                    .items_center()
                    .text_size(px(12.0))
                    .text_color(c.dialog_muted)
                    .child(strings.quick_open_no_results.clone())
                    .into_any_element(),
            );
        } else {
            let first = state.selected.saturating_sub(5);
            for (offset, path) in results.iter().skip(first).take(12).enumerate() {
                let index = first + offset;
                let selected = index == state.selected;
                let label = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.to_string_lossy().into_owned());
                let directory = path
                    .parent()
                    .map(|parent| parent.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let entry_path = path.clone();
                rows.push(
                    div()
                        .id(("quick-open-row", index))
                        .w_full()
                        .px(px(10.0))
                        .h(px(30.0))
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .rounded(px(5.0))
                        .cursor_pointer()
                        .bg(if selected {
                            c.selection
                        } else {
                            gpui::hsla(0.0, 0.0, 0.0, 0.0)
                        })
                        .hover(|this| this.bg(c.dialog_secondary_button_hover))
                        .child(
                            div()
                                .max_w(px(260.0))
                                .min_w(px(0.0))
                                .truncate()
                                .text_size(px(12.5))
                                .text_color(if selected {
                                    c.text_default
                                } else {
                                    c.dialog_body
                                })
                                .child(label),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.0))
                                .truncate()
                                .text_size(px(10.5))
                                .text_color(c.dialog_muted)
                                .child(directory),
                        )
                        .on_click(cx.listener(move |editor, _event, window, cx| {
                            editor.quick_open = None;
                            editor.open_workspace_file(entry_path.clone(), window, cx);
                        }))
                        .into_any_element(),
                );
            }
        }

        div()
            .id("quick-open-overlay")
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .bottom_0()
            .occlude()
            .bg(c.dialog_backdrop)
            .on_mouse_down(MouseButton::Left, {
                let editor = cx.entity().downgrade();
                move |_, _, cx| {
                    let _ = editor.update(cx, |editor, cx| editor.close_quick_open(cx));
                }
            })
            .child(
                div()
                    .id("quick-open-panel")
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(px(64.0))
                    .mx(px(d.editor_padding * 2.0))
                    .max_w(px(560.0))
                    .flex()
                    .flex_col()
                    .p(px(8.0))
                    .gap(px(4.0))
                    .mx_auto()
                    .bg(c.dialog_surface)
                    .border(px(d.dialog_border_width))
                    .border_color(c.dialog_border)
                    .rounded(px(d.dialog_radius))
                    .shadow_lg()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .id("quick-open-input")
                            .relative()
                            .track_focus(&focus)
                            .w_full()
                            .h(px(34.0))
                            .px(px(10.0))
                            .flex()
                            .items_center()
                            .rounded(px(6.0))
                            .border_1()
                            .border_color(c.dialog_border)
                            .bg(c.editor_background)
                            .text_size(px(13.0))
                            .text_color(if state.query.is_empty() {
                                c.dialog_muted
                            } else {
                                c.text_default
                            })
                            .child(if state.query.is_empty() {
                                strings.quick_open_placeholder.clone()
                            } else {
                                state.query.clone()
                            })
                            // 输入走编辑器的单行输入处理器：与搜索框共用 IME 路由，
                            // 中文文件名可直接用输入法拼写（roadmap E9）。
                            .child(
                                canvas(|_, _, _| (), {
                                    let focus = focus.clone();
                                    let input_editor = cx.entity();
                                    move |bounds, _, window, cx| {
                                        window.handle_input(
                                            &focus,
                                            ElementInputHandler::new(bounds, input_editor.clone()),
                                            cx,
                                        );
                                    }
                                })
                                .absolute()
                                .top_0()
                                .right_0()
                                .bottom_0()
                                .left_0(),
                            )
                            .on_key_down(cx.listener(Self::on_quick_open_key_down)),
                    )
                    .children(rows),
            )
            .into_any_element()
    }

    pub(crate) fn toggle_quick_open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.quick_open.is_some() {
            self.close_quick_open(cx);
            return;
        }
        self.dismiss_contextual_overlays(cx);
        let mut state = QuickOpenState::default();
        // 记住打开前的正文焦点，关闭时还回去。
        self.overlay_focus_restore_target = self.focused_edit_target_entity_id(window, cx);
        let focus = cx.focus_handle();
        window.focus(&focus);
        state.focus = Some(focus);
        self.quick_open = Some(state);
        self.refresh_quick_open_results(cx);
    }

    pub(super) fn close_quick_open(&mut self, cx: &mut Context<Self>) {
        if self.quick_open.take().is_some() {
            self.restore_focus_after_overlay(cx);
            cx.notify();
        }
    }

    /// Recomputes the fuzzy file list for the current query. Matching runs on
    /// the lowercase file name; empty queries list every workspace file.
    pub(super) fn refresh_quick_open_results(&mut self, cx: &mut Context<Self>) {
        let query = self
            .quick_open
            .as_ref()
            .map(|state| state.query.trim().to_lowercase())
            .unwrap_or_default();
        let mut files = self.workspace_text_files();
        if !query.is_empty() {
            files.retain(|path| {
                let name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                fuzzy_contains(&name, &query)
            });
            // Shorter names first: they tend to be the closer matches.
            files.sort_by_key(|path| {
                path.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default()
                    .len()
            });
        }
        files.truncate(12);
        if let Some(state) = self.quick_open.as_mut() {
            state.results = files;
            state.selected = state.selected.min(state.results.len().saturating_sub(1));
        }
        cx.notify();
    }

    fn quick_open_move_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(state) = self.quick_open.as_mut() else {
            return;
        };
        let len = state.results.len();
        if len == 0 {
            return;
        }
        let current = state.selected as isize;
        let next = (current + delta).rem_euclid(len as isize) as usize;
        state.selected = next;
        cx.notify();
    }

    fn quick_open_confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self
            .quick_open
            .as_ref()
            .and_then(|state| state.results.get(state.selected).cloned())
        else {
            return;
        };
        self.quick_open = None;
        self.open_workspace_file(path, window, cx);
    }

    /// Handles keystrokes on the quick-open overlay. Typing goes through the
    /// editor's `EntityInputHandler` (see `OverlayInputKind::QuickOpen`), so
    /// IME composition works for non-ASCII file names; this handler only owns
    /// navigation, confirm, delete, and clipboard shortcuts.
    pub(super) fn on_quick_open_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = event.keystroke.key.to_ascii_lowercase();
        let secondary = event.keystroke.modifiers.secondary();
        match key.as_str() {
            "escape" => {
                self.close_quick_open(cx);
            }
            "up" => {
                self.quick_open_move_selection(-1, cx);
            }
            "down" => {
                self.quick_open_move_selection(1, cx);
            }
            "enter" => {
                self.quick_open_confirm(window, cx);
            }
            "backspace" => {
                // 组合期间由输入法自己处理退格，避免双删。
                let Some(state) = self.quick_open.as_ref() else {
                    return;
                };
                if state.marked_range.is_some() {
                    return;
                }
                let text = state.query.clone();
                let selected = state.selected_range.clone();
                let end = selected.end.min(text.len());
                let range = if selected.start == selected.end {
                    let start = text[..end]
                        .grapheme_indices(true)
                        .last()
                        .map(|(start, _)| start)
                        .unwrap_or(end);
                    start..end
                } else {
                    selected.start.min(text.len())..end
                };
                self.replace_quick_open_input_text(range, "", None, cx);
            }
            "a" if secondary => {
                if let Some(state) = self.quick_open.as_mut() {
                    state.marked_range = None;
                    state.selected_range = 0..state.query.len();
                }
                cx.notify();
            }
            "v" if secondary => {
                if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                    let range = self
                        .quick_open
                        .as_ref()
                        .map(|state| state.selected_range.clone())
                        .unwrap_or_default();
                    self.replace_quick_open_input_text(range, &text, None, cx);
                }
            }
            _ => return,
        }
        cx.stop_propagation();
    }
}

/// Case-insensitive in-order subsequence containment.
fn fuzzy_contains(name: &str, query: &str) -> bool {
    let mut candidates = name.chars();
    for query_char in query.chars() {
        let mut matched = false;
        for candidate in candidates.by_ref() {
            if candidate.to_lowercase().eq(query_char.to_lowercase()) {
                matched = true;
                break;
            }
        }
        if !matched {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::fuzzy_contains;

    #[test]
    fn fuzzy_contains_matches_in_order_case_insensitively() {
        assert!(fuzzy_contains("main.rs", "mrs"));
        assert!(fuzzy_contains("main.rs", "MAIN"));
        assert!(!fuzzy_contains("main.rs", "msr"));
        assert!(!fuzzy_contains("main.rs", "xyz"));
    }
}
