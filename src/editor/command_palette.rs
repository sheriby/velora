//! Command palette (⇧⌘P / roadmap E2): a static registry of editor commands
//! with fuzzy filtering, executed through the app action dispatch.

use gpui::*;

use super::Editor;
use crate::components::*;
use crate::i18n::I18nManager;
use crate::theme::Theme;

/// Overlay state. `None` while closed.
#[derive(Default)]
pub(in crate::editor) struct CommandPaletteState {
    pub(super) query: String,
    pub(super) focus: Option<FocusHandle>,
    pub(super) selected: usize,
}

/// One executable command: display label (already localized) + the action it
/// dispatches.
struct CommandEntry {
    label: String,
    action: Box<dyn Action>,
}

impl CommandEntry {
    fn new(label: String, action: impl Action + Clone) -> Self {
        Self {
            label,
            action: Box::new(action),
        }
    }
}

impl Editor {
    pub(crate) fn on_open_command_palette(
        &mut self,
        _: &OpenCommandPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.command_palette.take().is_some() {
            cx.notify();
            return;
        }
        self.dismiss_contextual_overlays(cx);
        let mut state = CommandPaletteState::default();
        let focus = cx.focus_handle();
        window.focus(&focus);
        state.focus = Some(focus);
        self.command_palette = Some(state);
        cx.notify();
    }

    pub(super) fn close_command_palette(&mut self, cx: &mut Context<Self>) {
        if self.command_palette.take().is_some() {
            cx.notify();
        }
    }

    fn command_entries(&self, cx: &Context<Self>) -> Vec<CommandEntry> {
        let strings = cx.global::<I18nManager>().strings();
        vec![
            CommandEntry::new(strings.menu_save.clone(), SaveDocument),
            CommandEntry::new(strings.menu_save_as.clone(), SaveDocumentAs),
            CommandEntry::new(strings.menu_export_html.clone(), ExportHtml),
            CommandEntry::new(strings.menu_export_pdf.clone(), ExportPdf),
            CommandEntry::new(strings.menu_open_file.clone(), OpenFile),
            CommandEntry::new(strings.menu_new_window.clone(), NewWindow),
            CommandEntry::new(strings.menu_close_window.clone(), CloseWindow),
            CommandEntry::new(
                strings.command_toggle_view_mode.clone(),
                ToggleViewMode,
            ),
            CommandEntry::new(
                strings.command_toggle_focus_mode.clone(),
                ToggleFocusMode,
            ),
            CommandEntry::new(
                strings.command_toggle_typewriter_mode.clone(),
                ToggleTypewriterMode,
            ),
            CommandEntry::new(strings.command_toggle_sidebar.clone(), ToggleSidebar),
            CommandEntry::new(strings.command_find_in_document.clone(), FindInDocument),
            CommandEntry::new(strings.command_find_next.clone(), FindNextMatch),
            CommandEntry::new(strings.command_find_previous.clone(), FindPreviousMatch),
            CommandEntry::new("⌘+ Zoom In".into(), ZoomIn),
            CommandEntry::new("⌘- Zoom Out".into(), ZoomOut),
            CommandEntry::new("⌘0 Zoom Reset".into(), ZoomReset),
            CommandEntry::new(strings.menu_preferences.clone(), OpenPreferences),
        ]
    }

    fn filtered_commands(&self, cx: &Context<Self>) -> Vec<CommandEntry> {
        let query = self
            .command_palette
            .as_ref()
            .map(|state| state.query.trim().to_lowercase())
            .unwrap_or_default();
        let entries = self.command_entries(cx);
        if query.is_empty() {
            return entries;
        }
        entries
            .into_iter()
            .filter(|entry| entry.label.to_lowercase().contains(&query))
            .collect()
    }

    pub(super) fn on_command_palette_key_down(
        &mut self,
        event: &KeyDownEvent,
        cx: &mut Context<Self>,
    ) {
        let key = event.keystroke.key.clone();
        match key.as_str() {
            "escape" => {
                self.close_command_palette(cx);
                return;
            }
            "down" => {
                if let Some(state) = self.command_palette.as_mut() {
                    state.selected = state.selected.saturating_add(1);
                    cx.notify();
                }
                return;
            }
            "up" => {
                if let Some(state) = self.command_palette.as_mut() {
                    state.selected = state.selected.saturating_sub(1);
                    cx.notify();
                }
                return;
            }
            "backspace" => {
                if let Some(state) = self.command_palette.as_mut() {
                    state.query.pop();
                    state.selected = 0;
                }
                cx.notify();
                return;
            }
            "shift" | "control" | "alt" | "meta" | "capslock" | "tab" | "enter" => return,
            _ => {}
        }
        if key.len() == 1 && key.chars().all(|ch| ch.is_ascii_graphic()) {
            if let Some(state) = self.command_palette.as_mut() {
                state.query.push_str(&key);
                state.selected = 0;
            }
            cx.notify();
        }
    }
}

/// Renders the palette overlay from the editor's command registry.
pub(super) fn render_command_palette_overlay(
    editor: &Editor,
    theme: &Theme,
    cx: &mut Context<Editor>,
) -> AnyElement {
    let c = &theme.colors;
    let d = &theme.dimensions;
    let strings = cx.global::<I18nManager>().strings();
    let Some(state) = editor.command_palette.as_ref() else {
        return div().into_any_element();
    };
    let Some(focus) = state.focus.clone() else {
        return div().into_any_element();
    };
    let entries = editor.filtered_commands(cx);
    let selected = state.selected.min(entries.len().saturating_sub(1));
    let editor_handle = cx.entity().downgrade();

    let mut rows: Vec<AnyElement> = Vec::new();
    if entries.is_empty() {
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
        let first = selected.saturating_sub(6);
        for (offset, entry) in entries.iter().skip(first).take(12).enumerate() {
            let index = first + offset;
            let is_selected = index == selected;
            let dispatch_editor = editor_handle.clone();
            let action = entry.action.boxed_clone();
            rows.push(
                div()
                    .id(("command-palette-row", index))
                    .w_full()
                    .px(px(10.0))
                    .h(px(30.0))
                    .flex()
                    .items_center()
                    .rounded(px(5.0))
                    .cursor_pointer()
                    .bg(if is_selected {
                        c.selection
                    } else {
                        gpui::hsla(0.0, 0.0, 0.0, 0.0)
                    })
                    .hover(|this| this.bg(c.dialog_secondary_button_hover))
                    .text_size(px(12.5))
                    .text_color(if is_selected {
                        c.text_default
                    } else {
                        c.dialog_body
                    })
                    .child(entry.label.clone())
                    .on_click(move |_event, _window, cx| {
                        let _ = dispatch_editor.update(cx, |editor, _cx| {
                            editor.command_palette = None;
                        });
                        crate::app_menu::dispatch_menu_action(action.as_ref(), cx);
                    })
                    .into_any_element(),
            );
        }
    }

    div()
        .id("command-palette-overlay")
        .absolute()
        .top_0()
        .left_0()
        .right_0()
        .bottom_0()
        .occlude()
        .bg(c.dialog_backdrop)
        .on_mouse_down(MouseButton::Left, {
            let editor_handle = editor_handle.clone();
            move |_, _, cx| {
                let _ = editor_handle.update(cx, |editor, cx| editor.close_command_palette(cx));
            }
        })
        .child(
            div()
                .id("command-palette-panel")
                .absolute()
                .left_0()
                .right_0()
                .top(px(64.0))
                .max_w(px(560.0))
                .mx_auto()
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
                .child(
                    div()
                        .id("command-palette-input")
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
                            strings.command_palette_placeholder.clone()
                        } else {
                            state.query.clone()
                        })
                        .on_key_down({
                            let editor_handle = editor_handle.clone();
                            move |event: &KeyDownEvent, _window, cx| {
                                let _ = editor_handle.update(cx, |editor, cx| {
                                    editor.on_command_palette_key_down(event, cx);
                                });
                            }
                        }),
                )
                .children(rows),
        )
        .into_any_element()
}
