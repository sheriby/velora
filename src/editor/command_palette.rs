//! 命令面板复用命令注册表，Kit 管理查询编辑、候选选择与列表滚动。

use gpui::*;
use gpui_kit::assets::IconName;
use gpui_kit::base::actions::{Cancel, Confirm};
use gpui_kit::component::{
    ActiveTheme as _, Icon,
    command::{Command, CommandItem, CommandState},
    input::{Enter as InputEnter, Input, InputEvent, InputState},
    kbd::Kbd,
};

use super::Editor;
use crate::components::OpenCommandPalette;
use crate::i18n::{I18nManager, I18nStrings};

pub(in crate::editor) struct CommandPaletteState {
    pub(super) input: Entity<InputState>,
    pub(super) list: Entity<CommandState>,
    /// 过滤结果缓存键（查询, AI 是否可用）与条目。面板每帧渲染都会要这份
    /// 列表：逐帧重新本地化文案并构造 boxed action 是纯浪费。
    entries_cache: std::cell::RefCell<
        Option<(
            String,
            bool,
            std::rc::Rc<Vec<CommandEntry>>,
        )>,
    >,
    _subscription: Subscription,
}

#[derive(Clone)]
struct CommandEntry {
    id: &'static str,
    label: SharedString,
    action: std::rc::Rc<dyn Action>,
}

impl CommandEntry {
    fn from_spec(spec: &crate::commands::CommandSpec, strings: &I18nStrings) -> Self {
        Self {
            id: spec.id,
            label: SharedString::from(spec.label(strings)),
            action: std::rc::Rc::from(spec.boxed_action()),
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

    pub(crate) fn toggle_command_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.command_palette.is_some() {
            self.close_command_palette(cx);
            return;
        }
        // 与 ⌘P 快速切换互斥（见 toggle_quick_open 的同一条约定）。
        if self.quick_open.is_some() {
            self.close_quick_open(cx);
        }
        self.dismiss_contextual_overlays(cx);
        self.overlay_focus_restore_target = self.focused_edit_target_entity_id(window, cx);
        let input = cx.new(|cx| InputState::new(window, cx));
        let list = cx.new(|cx| CommandState::new(window, cx));
        let subscription = cx.subscribe_in(
            &input,
            window,
            |editor, source, event: &InputEvent, window, cx| {
                if !matches!(event, InputEvent::Change) {
                    return;
                }
                let Some(state) = editor
                    .command_palette
                    .as_ref()
                    .filter(|state| state.input == *source)
                else {
                    return;
                };
                let query = source.read(cx).value();
                state
                    .list
                    .update(cx, |list, cx| list.set_query(query, window, cx));
                cx.notify();
            },
        );
        let focus = input.read(cx).focus_handle(cx);
        self.command_palette = Some(CommandPaletteState {
            input,
            list,
            entries_cache: std::cell::RefCell::new(None),
            _subscription: subscription,
        });
        window.focus(&focus, cx);
        cx.notify();
    }

    pub(super) fn close_command_palette(&mut self, cx: &mut Context<Self>) {
        if self.command_palette.take().is_some() {
            self.restore_focus_after_overlay(cx);
            cx.notify();
        }
    }

    fn filtered_commands(&self, cx: &Context<Self>) -> std::rc::Rc<Vec<CommandEntry>> {
        let query = self
            .command_palette
            .as_ref()
            .map(|state| state.list.read(cx).query(cx))
            .unwrap_or_default();
        let query = query.trim().to_lowercase();
        let ai_enabled = crate::ai::settings::AiFeature::enabled(cx);
        let Some(palette) = self.command_palette.as_ref() else {
            return std::rc::Rc::new(Vec::new());
        };
        if let Some((cached_query, cached_ai, entries)) = palette.entries_cache.borrow().as_ref()
            && *cached_query == query
            && *cached_ai == ai_enabled
        {
            return entries.clone();
        }
        let strings = cx.global::<I18nManager>().strings();
        let entries: Vec<CommandEntry> = crate::commands::commands()
            .iter()
            .filter(|spec| !spec.requires_ai() || ai_enabled)
            .map(|spec| CommandEntry::from_spec(spec, strings))
            .filter(|entry| query.is_empty() || entry.label.to_lowercase().contains(&query))
            .collect();
        // 探针：面板条目构造次数。静止帧不该涨。
        #[cfg(test)]
        command_palette_probe::note_entries(entries.len() as u64);
        let entries = std::rc::Rc::new(entries);
        *palette.entries_cache.borrow_mut() = Some((query, ai_enabled, entries.clone()));
        entries
    }

    fn run_palette_command(
        &mut self,
        action: Box<dyn Action>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // 必须先收起面板，再执行命令；否则新命令打开的浮层会被旧面板的焦点恢复覆盖。
        self.close_command_palette(cx);
        window.dispatch_action(action, cx);
    }

    fn palette_is_composing(&self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.command_palette
            .as_ref()
            .map(|state| state.input.clone())
            .is_some_and(|input| {
                input.update(cx, |input, cx| {
                    input.marked_text_range(window, cx).is_some()
                })
            })
    }
}

pub(super) fn render_command_palette_overlay(
    editor: &Editor,
    window: &mut Window,
    cx: &mut Context<Editor>,
) -> AnyElement {
    let Some(state) = editor.command_palette.as_ref() else {
        return div().into_any_element();
    };
    let input = state.input.clone();
    let list = state.list.clone();
    let strings = cx.global::<I18nManager>().strings();
    let placeholder = strings.command_palette_placeholder.clone();
    let empty = strings.command_palette_no_results.clone();
    if input.read(cx).presentation().placeholder().as_ref() != placeholder.as_str() {
        input.update(cx, |input, cx| {
            input.set_placeholder(placeholder.clone(), window, cx)
        });
    }
    let entries = editor.filtered_commands(cx);
    let commands: Vec<_> = entries.iter().map(|entry| entry.id).collect();
    let items = entries.iter().cloned().map(|entry| {
        CommandItem::new()
            .label(entry.label.clone())
            .child(move |window, _| {
                let binding = Kbd::binding_for_action(entry.action.as_ref(), None, window);
                div()
                    .w_full()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(entry.label.clone()),
                    )
                    .children(binding)
            })
    });
    let owner = cx.entity().downgrade();
    let confirm_owner = owner.clone();
    // 回调保存在列表模型内，只捕获身份，避免通过回调持有列表自身。
    let confirmed_list_id = list.entity_id();
    // Command 未公开查询框的组词状态；独立 Input 在确认前检查 IME，候选仍交给 Command。
    let content = Command::new(&list)
        .searchable(false)
        .filterable(false)
        .min_h_0()
        .items(items)
        .header(move |_, _, cx| {
            let paste_input = input.clone();
            div()
                .px_3()
                .flex_shrink_0()
                .border_b_1()
                .border_color(cx.theme().border)
                .child(
                    Input::new(&input)
                        .id("command-palette-input")
                        .accessibility_id("command-palette-input")
                        .aria_label(placeholder.clone())
                        .prefix(Icon::new(IconName::Search).text_color(cx.theme().muted_foreground))
                        .appearance(false)
                        .p_0()
                        .on_paste(move |clipboard, window, cx| {
                            let Some(text) =
                                clipboard.text().filter(|text| text.contains(['\r', '\n']))
                            else {
                                return false;
                            };
                            paste_input.update(cx, |input, cx| {
                                input.replace(text.replace(['\r', '\n'], " "), window, cx)
                            });
                            true
                        }),
                )
        })
        .empty(move |_, _, cx| {
            div()
                .p_3()
                .text_color(cx.theme().muted_foreground)
                .child(empty.clone())
        })
        .on_confirm(move |index, window, cx| {
            let Some(id) = commands.get(index.row) else {
                return;
            };
            let Some(spec) = crate::commands::commands()
                .iter()
                .find(|spec| spec.id == *id)
            else {
                return;
            };
            if let Err(error) = confirm_owner.update(cx, |editor, cx| {
                if editor
                    .command_palette
                    .as_ref()
                    .is_some_and(|state| state.list.entity_id() == confirmed_list_id)
                {
                    editor.run_palette_command(spec.boxed_action(), window, cx);
                }
            }) {
                eprintln!("执行命令面板操作失败：{error}");
            }
        });

    div()
        .id("command-palette-overlay")
        .debug_selector(|| "command-palette-overlay".into())
        .absolute()
        .inset_0()
        .occlude()
        .flex()
        .flex_col()
        .items_center()
        .pt(px(64.0))
        .px_4()
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            if let Err(error) = owner.update(cx, |editor, cx| editor.close_command_palette(cx)) {
                eprintln!("关闭命令面板失败：{error}");
            }
        })
        .child(
            div()
                .id("command-palette-panel")
                .debug_selector(|| "command-palette-panel".into())
                .w_full()
                .max_w(px(560.0))
                .max_h(relative(1.0))
                .min_h_0()
                .flex()
                .flex_col()
                .shadow_lg()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .capture_action(cx.listener(|editor, _: &Confirm, window, cx| {
                    if !editor.palette_is_composing(window, cx) {
                        cx.propagate();
                    }
                }))
                .capture_action(
                    cx.listener(|editor, _: &Cancel, _, cx| editor.close_command_palette(cx)),
                )
                .on_action(cx.listener(|editor, action: &InputEnter, window, cx| {
                    if !editor.palette_is_composing(window, cx) {
                        window.dispatch_action(
                            Box::new(Confirm {
                                secondary: action.secondary,
                            }),
                            cx,
                        );
                    }
                }))
                .child(content),
        )
        .into_any_element()
}

/// thread-local 探针：命令面板条目构造次数。并行用例各数各的。
#[cfg(test)]
pub(crate) mod command_palette_probe {
    use std::cell::Cell;
    thread_local! {
        static PALETTE_ENTRIES_BUILT: Cell<u64> = const { Cell::new(0) };
    }
    pub(crate) fn note_entries(count: u64) {
        PALETTE_ENTRIES_BUILT.with(|counter| counter.set(counter.get() + count));
    }
    pub(crate) fn entries_built() -> u64 {
        PALETTE_ENTRIES_BUILT.with(|counter| counter.get())
    }
}
