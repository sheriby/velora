//! Quick file switcher (⌘P / roadmap E1): fuzzy-match workspace file names
//! and open the selection in the current window.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::*;

use super::Editor;
use crate::i18n::I18nManager;
use crate::theme::Theme;
use gpui_kit::component::{
    ActiveTheme as _,
    input::{Enter as InputEnter, Escape as InputEscape, Input, InputEvent, InputState},
    list::ListItem,
};

/// Overlay state for the quick switcher. `None` while closed.
pub(in crate::editor) struct QuickOpenState {
    pub(super) query: String,
    pub(super) selected: usize,
    pub(super) results: Vec<PathBuf>,
    pub(super) input: Entity<InputState>,
    _subscription: Subscription,
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
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let strings = cx.global::<I18nManager>().strings();
        let placeholder = strings.quick_open_placeholder.clone();
        let no_results = strings.quick_open_no_results.clone();
        let Some(state) = self.quick_open.as_ref() else {
            return div().into_any_element();
        };
        let input = state.input.clone();
        if input.read(cx).presentation().placeholder().as_ref() != placeholder.as_str() {
            input.update(cx, |input, cx| {
                input.set_placeholder(placeholder.clone(), window, cx)
            });
        }
        let mut rows: Vec<AnyElement> = Vec::new();
        let results = &state.results;
        if results.is_empty() {
            rows.push(
                div()
                    .px(px(10.0))
                    .h(px(theme.typography.ui_text_size(28.0).max(28.0)))
                    .flex()
                    .items_center()
                    .text_size(px(theme.typography.ui_text_size(12.0)))
                    .text_color(c.dialog_muted)
                    .child(no_results)
                    .into_any_element(),
            );
        } else {
            let first = state.selected.saturating_sub(5);
            for (offset, path) in results.iter().skip(first).take(12).enumerate() {
                let selected = first + offset == state.selected;
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
                    ListItem::new(Arc::<Path>::from(entry_path.clone()))
                        .accessibility_label(format!("{label} {directory}"))
                        .selected(selected)
                        .child(
                            div()
                                .w_full()
                                .min_w_0()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(div().flex_1().min_w_0().truncate().child(label))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .text_sm()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(directory),
                                ),
                        )
                        .on_click(cx.listener(move |editor, event, window, cx| {
                            editor.quick_open = None;
                            // ⌘P 是「点开看看」：单击开预览标签，双击转固定（与文件树同一判定）。
                            let mode = Self::tree_click_open_mode(event);
                            editor.open_workspace_file_in_mode(
                                entry_path.clone(),
                                mode,
                                window,
                                cx,
                            );
                        }))
                        .into_any_element(),
                );
            }
        }

        div()
            .id("quick-open-overlay")
            .debug_selector(|| "quick-open-overlay".into())
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .bottom_0()
            .occlude()
            .flex()
            .flex_col()
            .items_center()
            .pt(px(64.0))
            .on_mouse_down(MouseButton::Left, {
                let editor = cx.entity().downgrade();
                move |_, _, cx| {
                    if let Err(error) = editor.update(cx, |editor, cx| editor.close_quick_open(cx))
                    {
                        eprintln!("关闭快速打开面板失败：{error}");
                    }
                }
            })
            .child(
                div()
                    .id("quick-open-panel")
                    .debug_selector(|| "quick-open-panel".into())
                    .w_full()
                    .max_w(px(560.0))
                    .flex()
                    .flex_col()
                    .p(px(8.0))
                    .gap(px(4.0))
                    .bg(c.dialog_surface)
                    .border(px(d.dialog_border_width))
                    .border_color(c.dialog_border)
                    .rounded(px(d.dialog_radius))
                    .shadow_lg()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .capture_key_down(cx.listener(|editor, event: &KeyDownEvent, window, cx| {
                        if event.keystroke.modifiers != Modifiers::none()
                            || editor.quick_open_is_composing(window, cx)
                        {
                            return;
                        }
                        let delta = match event.keystroke.key.as_str() {
                            "up" => -1,
                            "down" => 1,
                            _ => return,
                        };
                        editor.quick_open_move_selection(delta, cx);
                        cx.stop_propagation();
                    }))
                    .on_action(cx.listener(|editor, _: &InputEnter, window, cx| {
                        if !editor.quick_open_is_composing(window, cx) {
                            editor.quick_open_confirm(window, cx);
                        }
                    }))
                    .on_action(
                        cx.listener(|editor, _: &InputEscape, _, cx| editor.close_quick_open(cx)),
                    )
                    .child(
                        Input::new(&input)
                            .id("quick-open-input")
                            .accessibility_id("quick-open-input")
                            .aria_label(placeholder)
                            .on_paste(move |clipboard, window, cx| {
                                let Some(text) =
                                    clipboard.text().filter(|text| text.contains(['\r', '\n']))
                                else {
                                    return false;
                                };
                                input.update(cx, |input, cx| {
                                    input.replace(text.replace(['\r', '\n'], " "), window, cx)
                                });
                                true
                            }),
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
        self.overlay_focus_restore_target = self.focused_edit_target_entity_id(window, cx);
        let input = cx.new(|cx| InputState::new(window, cx));
        let focus = input.read(cx).focus_handle(cx);
        let subscription = cx.subscribe_in(
            &input,
            window,
            |editor, source, event: &InputEvent, window, cx| {
                if !matches!(event, InputEvent::Change) {
                    return;
                }
                let value = source.read(cx).value().to_string();
                let composing = source.update(cx, |input, cx| {
                    input.marked_text_range(window, cx).is_some()
                });
                let Some(state) = editor
                    .quick_open
                    .as_mut()
                    .filter(|state| state.input == *source)
                else {
                    return;
                };
                state.query = value;
                state.selected = 0;
                if !composing {
                    editor.refresh_quick_open_results(cx);
                }
                cx.notify();
            },
        );
        self.quick_open = Some(QuickOpenState {
            query: String::new(),
            selected: 0,
            results: Vec::new(),
            input,
            _subscription: subscription,
        });
        window.focus(&focus, cx);
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
        self.ensure_workspace_files_walk(cx);
        let query = self
            .quick_open
            .as_ref()
            .map(|state| state.query.trim().to_lowercase())
            .unwrap_or_default();
        let mut files = self.workspace_openable_files();
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
        // 回车打开也按预览（与鼠标单击同口径，见 VS Code 的
        // `workbench.editor.enablePreviewFromQuickOpen`）；改过它就自动转固定。
        self.open_workspace_file_in_mode(
            path,
            crate::editor::workspace::WorkspaceOpenMode::Preview,
            window,
            cx,
        );
    }

    fn quick_open_is_composing(&self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.quick_open
            .as_ref()
            .map(|state| state.input.clone())
            .is_some_and(|input| {
                input.update(cx, |input, cx| {
                    input.marked_text_range(window, cx).is_some()
                })
            })
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
