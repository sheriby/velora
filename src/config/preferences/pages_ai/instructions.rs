use super::*;
use crate::ai::{
    AiCommand,
    instructions::{default_instruction, effective_instruction},
};

pub(super) struct InstructionDraft {
    pub command: AiCommand,
    pub field: Entity<TextField>,
    locale: String,
}

impl InstructionDraft {
    pub fn new(settings: &AiSettings, cx: &mut Context<PreferencesWindow>) -> Self {
        let locale = cx.global::<I18nManager>().current_language_id().to_owned();
        let owner = cx.entity().downgrade();
        let field = cx.new(|cx| {
            let mut field = TextField::new("", cx).multiline(6, 10);
            field.set_value(effective_instruction(settings, AiCommand::Ask, &locale), cx);
            field.on_change(move |cx| {
                let owner = owner.clone();
                cx.defer(move |cx| {
                    if let Err(error) = owner.update(cx, |_, cx| cx.notify()) {
                        eprintln!("更新 AI 指令输入失败：{error}");
                    }
                });
            })
        });
        Self {
            command: AiCommand::Ask,
            field,
            locale,
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
                .insert(self.command.id().into(), body.into());
        }
    }

    fn load(&mut self, settings: &AiSettings, command: AiCommand, locale: String, cx: &mut App) {
        self.locale = locale;
        self.command = command;
        let body = effective_instruction(settings, command, &self.locale);
        self.field.update(cx, |field, cx| field.set_value(body, cx));
    }
}

impl PreferencesWindow {
    pub(super) fn refresh_ai_instruction_locale(&mut self, cx: &mut Context<Self>) {
        let locale = cx.global::<I18nManager>().current_language_id().to_owned();
        if let Some(state) = self.ai_preferences.as_mut()
            && state.instruction.locale != locale
        {
            state.instruction.stage(&mut state.settings, cx);
            state
                .instruction
                .load(&state.settings, state.instruction.command, locale, cx);
        }
    }

    fn select_ai_instruction(&mut self, command: AiCommand, cx: &mut Context<Self>) {
        self.close_dropdowns();
        let locale = cx.global::<I18nManager>().current_language_id().to_owned();
        if let Some(state) = self.ai_preferences.as_mut() {
            state.instruction.stage(&mut state.settings, cx);
            state.instruction.load(&state.settings, command, locale, cx);
        }
        cx.notify();
    }

    fn toggle_ai_instruction_menu(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let open = self
            .ai_preferences
            .as_ref()
            .is_some_and(|state| !state.instruction_menu_open);
        self.close_dropdowns();
        if let Some(state) = self.ai_preferences.as_mut() {
            state.instruction_menu_open = open;
        }
        cx.notify();
    }

    pub(super) fn ai_preferences_tabs(
        &self,
        theme: &Theme,
        strings: &crate::i18n::I18nStrings,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = self
            .ai_preferences
            .as_ref()
            .is_some_and(|state| state.show_instructions);
        let mut tabs = div()
            .flex()
            .items_center()
            .gap(px(4.0))
            .p(px(3.0))
            .rounded(px(7.0))
            .bg(theme.colors.dialog_secondary_button_bg);
        for (id, label, instructions) in [
            (
                "preferences-ai-connections-tab",
                strings.preferences_ai_connections.clone(),
                false,
            ),
            (
                "preferences-ai-instructions-tab",
                strings.preferences_ai_instructions.clone(),
                true,
            ),
        ] {
            tabs = tabs.child(
                self.ai_preference_button(id, label, true, theme, cx, move |preferences, cx| {
                    preferences.close_dropdowns();
                    if let Some(state) = preferences.ai_preferences.as_mut() {
                        state.show_instructions = instructions;
                    }
                    cx.notify();
                })
                .when(selected == instructions, |tab| {
                    tab.bg(theme.colors.dialog_surface)
                }),
            );
        }
        div().child(tabs).into_any_element()
    }

    pub(super) fn render_ai_instruction_editor(
        &self,
        theme: &Theme,
        strings: &crate::i18n::I18nStrings,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(state) = self.ai_preferences.as_ref() else {
            return div().into_any_element();
        };
        let label = |command: AiCommand| {
            if command == AiCommand::Ask {
                strings.preferences_ai_writing_preferences.clone()
            } else {
                command.label(strings)
            }
        };
        let mut dropdown = div().relative().child(Self::dropdown_button(
            "preferences-ai-command-menu",
            label(state.instruction.command),
            theme,
            Self::toggle_ai_instruction_menu,
            cx,
        ));
        if state.instruction_menu_open {
            let mut items = div().w_full().flex().flex_col().gap(px(4.0));
            for command in std::iter::once(AiCommand::Ask).chain(AiCommand::ALL) {
                items = items.child(Self::dropdown_item(
                    SharedString::from(format!("preferences-ai-command-{}", command.id())),
                    label(command),
                    command == state.instruction.command,
                    theme,
                    move |preferences, _, _, cx| preferences.select_ai_instruction(command, cx),
                    cx,
                ));
            }
            dropdown =
                dropdown.child(self.dropdown_menu("preferences-ai-command-menu", items, theme));
        }
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(dropdown)
                    .child(self.ai_preference_button(
                        "preferences-ai-restore-instruction",
                        strings.preferences_ai_restore_instruction.clone(),
                        true,
                        theme,
                        cx,
                        |preferences, cx| {
                            let locale =
                                cx.global::<I18nManager>().current_language_id().to_owned();
                            if let Some(state) = preferences.ai_preferences.as_mut() {
                                let command = state.instruction.command;
                                state.settings.quick_instructions.remove(command.id());
                                state.instruction.load(&state.settings, command, locale, cx);
                            }
                            cx.notify();
                        },
                    )),
            )
            .child(
                div()
                    .debug_selector(|| "preferences-ai-instruction-body".into())
                    .w_full()
                    .child(state.instruction.field.clone()),
            )
            .into_any_element()
    }
}
