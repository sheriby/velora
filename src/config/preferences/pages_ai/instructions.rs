use super::*;
use crate::ai::{
    AiCommand,
    instructions::{default_instruction, effective_instruction},
};
use gpui_kit::component::ThemeStyled as _;
use gpui_kit::component::tab::{Tab, TabBar};

pub(super) struct InstructionDraft {
    pub command: AiCommand,
    pub field: Entity<TextareaState>,
    locale: String,
    _subscription: Subscription,
}

impl InstructionDraft {
    pub fn new(
        settings: &AiSettings,
        window: &mut Window,
        cx: &mut Context<PreferencesWindow>,
    ) -> Self {
        let locale = cx.global::<I18nManager>().current_language_id().to_owned();
        let field = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(6, 10)
                .default_value(effective_instruction(settings, AiCommand::Ask, &locale))
        });
        let subscription = cx.subscribe(&field, |_, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        });
        Self {
            command: AiCommand::Ask,
            field,
            locale,
            _subscription: subscription,
        }
    }

    pub fn stage(&self, settings: &mut AiSettings, cx: &App) {
        let body = self.field.read(cx).value();
        if body.trim().is_empty() || body.trim() == default_instruction(self.command, &self.locale)
        {
            settings.quick_instructions.remove(self.command.id());
        } else {
            settings
                .quick_instructions
                .insert(self.command.id().into(), body.to_string());
        }
    }

    fn load(
        &mut self,
        settings: &AiSettings,
        command: AiCommand,
        locale: String,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.locale = locale;
        self.command = command;
        let body = effective_instruction(settings, command, &self.locale);
        self.field
            .update(cx, |field, cx| field.set_value(body, window, cx));
    }
}

impl PreferencesWindow {
    pub(super) fn refresh_ai_instruction_locale(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let locale = cx.global::<I18nManager>().current_language_id().to_owned();
        if let Some(state) = self.ai_preferences.as_mut()
            && state.instruction.locale != locale
        {
            state.instruction.stage(&mut state.settings, cx);
            state.instruction.load(
                &state.settings,
                state.instruction.command,
                locale,
                window,
                cx,
            );
        }
    }

    fn select_ai_instruction(
        &mut self,
        command: AiCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        Self::dismiss_preferences_popup(window, cx);
        let locale = cx.global::<I18nManager>().current_language_id().to_owned();
        if let Some(state) = self.ai_preferences.as_mut() {
            state.instruction.stage(&mut state.settings, cx);
            state
                .instruction
                .load(&state.settings, command, locale, window, cx);
        }
        cx.notify();
    }

    pub(super) fn ai_preferences_tabs(
        &self,
        strings: &crate::i18n::I18nStrings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(state) = self.ai_preferences.as_ref() else {
            return div().into_any_element();
        };
        let instructions = state.show_instructions;
        let connections_focus = state.connections_focus.clone().tab_stop(!instructions);
        let instructions_focus = state.instructions_focus.clone().tab_stop(instructions);
        let tabs = TabBar::new("preferences-ai-tabs")
            .segmented()
            .w_auto()
            .selected_index(usize::from(instructions))
            .child(
                Tab::new()
                    .label(strings.preferences_ai_connections.clone())
                    .track_focus(&connections_focus)
                    .debug_selector(|| "preferences-ai-connections-tab".into()),
            )
            .child(
                Tab::new()
                    .label(strings.preferences_ai_instructions.clone())
                    .track_focus(&instructions_focus)
                    .debug_selector(|| "preferences-ai-instructions-tab".into()),
            )
            .on_click(cx.listener(|preferences, index: &usize, window, cx| {
                Self::dismiss_preferences_popup(window, cx);
                if let Some(state) = preferences.ai_preferences.as_mut() {
                    state.show_instructions = *index == 1;
                    let focus = if state.show_instructions {
                        &state.instructions_focus
                    } else {
                        &state.connections_focus
                    };
                    focus.focus(window, cx);
                }
                cx.notify();
            }));
        // Kit 0.7.3 的 Tab 不提供键盘导航；焦点与页面切换由本页共同维护。
        let focused = connections_focus.is_focused(window) || instructions_focus.is_focused(window);
        let group = div()
            .relative()
            .rounded(gpui_kit::component::Theme::global(cx).radius)
            .child(tabs)
            .when(focused, |group| group.focus_ring_style(window, cx))
            .on_key_down(
                cx.listener(|preferences, event: &KeyDownEvent, window, cx| {
                    if event.keystroke.modifiers != Modifiers::none() {
                        return;
                    }
                    let Some(state) = preferences.ai_preferences.as_mut() else {
                        return;
                    };
                    let instructions = match event.keystroke.key.as_str() {
                        "left" | "home" => false,
                        "right" | "end" => true,
                        "enter" | "space" => state.show_instructions,
                        _ => return,
                    };
                    state.show_instructions = instructions;
                    let focus = if instructions {
                        &state.instructions_focus
                    } else {
                        &state.connections_focus
                    };
                    focus.focus(window, cx);
                    cx.stop_propagation();
                    cx.notify();
                }),
            );
        div().flex().items_center().child(group).into_any_element()
    }

    pub(super) fn render_ai_instruction_editor(
        &mut self,
        strings: &crate::i18n::I18nStrings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(command) = self
            .ai_preferences
            .as_ref()
            .map(|state| state.instruction.command)
        else {
            return div().into_any_element();
        };
        let label = |command: AiCommand| {
            if command == AiCommand::Ask {
                strings.preferences_ai_writing_preferences.clone()
            } else {
                command.label(strings)
            }
        };
        let choices = std::iter::once(AiCommand::Ask)
            .chain(AiCommand::ALL)
            .map(|command| {
                super::super::controls::PreferenceChoice::new(
                    command.id(),
                    label(command),
                    format!("preferences-ai-command-{}", command.id()),
                )
            })
            .collect::<Vec<_>>();
        let selector = self.choice_field(
            "preferences-ai-command-menu",
            strings.preferences_ai_instructions.clone(),
            command.id(),
            |_| choices,
            false,
            window,
            cx,
            |preferences, id, window, cx| {
                if let Some(command) = std::iter::once(AiCommand::Ask)
                    .chain(AiCommand::ALL)
                    .find(|command| command.id() == id)
                {
                    preferences.select_ai_instruction(command, window, cx);
                }
            },
        );
        let Some(state) = self.ai_preferences.as_ref() else {
            return div().into_any_element();
        };
        GroupBox::new()
            .id("preferences-ai-instruction-fields")
            .fill()
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .child(selector)
                            .child(
                                self.ai_preference_button(
                                    "preferences-ai-restore-instruction",
                                    strings.preferences_ai_restore_instruction.clone(),
                                    true,
                                    cx,
                                    |preferences, window, cx| {
                                        let locale = cx
                                            .global::<I18nManager>()
                                            .current_language_id()
                                            .to_owned();
                                        if let Some(state) = preferences.ai_preferences.as_mut() {
                                            let command = state.instruction.command;
                                            state.settings.quick_instructions.remove(command.id());
                                            state.instruction.load(
                                                &state.settings,
                                                command,
                                                locale,
                                                window,
                                                cx,
                                            );
                                        }
                                        cx.notify();
                                    },
                                )
                                .ghost(),
                            ),
                    )
                    .child(
                        div()
                            .debug_selector(|| "preferences-ai-instruction-body".into())
                            .w_full()
                            .child(
                                Textarea::new(&state.instruction.field)
                                    .accessibility_id("preferences-ai-instruction-input")
                                    .aria_label(label(command)),
                            ),
                    ),
            )
            .into_any_element()
    }
}
