//! Command palette (⇧⌘P / roadmap E2): a static registry of editor commands
//! with fuzzy filtering, executed through the app action dispatch.

use gpui::*;

use super::Editor;
use crate::components::*;
use crate::i18n::{I18nManager, I18nStrings};
use crate::theme::Theme;

/// Overlay state. `None` while closed.
#[derive(Default)]
pub(in crate::editor) struct CommandPaletteState {
    pub(super) query: String,
    pub(super) focus: Option<FocusHandle>,
    pub(super) selected: usize,
    pub(super) selected_range: std::ops::Range<usize>,
    pub(super) marked_range: Option<std::ops::Range<usize>>,
}

/// One executable command: display label (already localized) + the action it
/// dispatches. Built from the shared command registry (roadmap H5), so the
/// palette and the app menus always list the same commands.
struct CommandEntry {
    label: String,
    action: Box<dyn Action>,
}

impl CommandEntry {
    fn from_spec(spec: &crate::commands::CommandSpec, strings: &I18nStrings) -> Self {
        Self {
            label: spec.label(strings),
            action: spec.boxed_action(),
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
        self.toggle_command_palette(window, cx);
    }

    /// 打开/关闭命令面板（roadmap H5：菜单与快捷键共用同一入口）。
    pub(crate) fn toggle_command_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.command_palette.is_some() {
            self.close_command_palette(cx);
            return;
        }
        self.dismiss_contextual_overlays(cx);
        let mut state = CommandPaletteState::default();
        self.overlay_focus_restore_target = self.focused_edit_target_entity_id(window, cx);
        let focus = cx.focus_handle();
        window.focus(&focus);
        state.focus = Some(focus);
        self.command_palette = Some(state);
        cx.notify();
    }

    pub(super) fn close_command_palette(&mut self, cx: &mut Context<Self>) {
        if self.command_palette.take().is_some() {
            self.restore_focus_after_overlay(cx);
            cx.notify();
        }
    }

    fn command_entries(&self, cx: &Context<Self>) -> Vec<CommandEntry> {
        let strings = cx.global::<I18nManager>().strings();
        crate::commands::commands()
            .iter()
            .map(|spec| CommandEntry::from_spec(spec, strings))
            .collect()
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

    /// 收起面板并派发这条命令：与菜单项、快捷键同一条动作派发链路，视图级处理者
    /// （缩放、拷贝为 HTML 等）一并生效。按回车与点一行共用这一段。
    ///
    /// 焦点还原排在下一帧的绘制里，本次派发读的是上一帧的派发路径，块那一层收不到
    /// 动作；编辑类命令因此在编辑器层收口，判定见 `Editor::block_focus_is_live`。
    fn run_palette_command(
        &mut self,
        action: Box<dyn Action>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_command_palette(cx);
        window.dispatch_action(action, cx);
    }

    /// 回车执行当前选中的那一条；列表为空（查询没命中）就什么都不做。
    fn run_selected_command(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(selected) = self.command_palette.as_ref().map(|state| state.selected) else {
            return;
        };
        let entries = self.filtered_commands(cx);
        let Some(action) = entries
            .get(selected.min(entries.len().saturating_sub(1)))
            .map(|entry| entry.action.boxed_clone())
        else {
            return;
        };
        self.run_palette_command(action, window, cx);
    }

    pub(super) fn on_command_palette_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = event.keystroke.key.clone();
        match key.as_str() {
            "escape" => {
                self.close_command_palette(cx);
                return;
            }
            "return" => {
                self.run_selected_command(window, cx);
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
            // 文本输入（含 IME、退格、粘贴）统一走编辑器的单行输入处理器，
            // 见 `render_command_palette_overlay` 里的 canvas 注册。
            _ => {}
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
                    .on_click(move |_event, window, cx| {
                        // 走标准动作派发：与菜单项、快捷键同一条链路，视图级处理者
                        // （缩放、复制为 HTML 等）也会生效；收尾与按回车是同一条。
                        let _ = dispatch_editor.update(cx, |editor, cx| {
                            editor.run_palette_command(action.boxed_clone(), window, cx);
                        });
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
                        // 输入走编辑器的单行输入处理器：与搜索框/⌘P 共用 IME 路由，
                        // 中文与非 ASCII 文本能输入（用户报修：面板里打字没反应）。
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
                        .on_key_down({
                            let editor_handle = editor_handle.clone();
                            move |event: &KeyDownEvent, window, cx| {
                                let _ = editor_handle.update(cx, |editor, cx| {
                                    editor.on_command_palette_key_down(event, window, cx);
                                });
                            }
                        }),
                )
                .children(rows),
        )
        .into_any_element()
}
