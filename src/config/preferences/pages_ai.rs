use super::*;
use crate::ai::providers::ProviderPreset;
use crate::ai::runtime::{RunControl, RunOutput};
use crate::ai::settings::{AiSettings, Endpoint, Protocol};
use crate::components::TextField;

mod instructions;

pub(super) struct AiPreferences {
    instruction: instructions::InstructionDraft,
    show_instructions: bool,
    pub(super) instruction_menu_open: bool,
    settings: AiSettings,
    saved: AiSettings,
    draft: Option<ConnectionDraft>,
    drafts: std::collections::BTreeMap<String, ConnectionDraft>,
    choosing_provider: bool,
    advanced: bool,
    pub(super) connection_menu_open: bool,
    pending_focus: Option<Entity<TextField>>,
    excluded: Entity<TextField>,
    testing: bool,
    generation: u64,
    control: RunControl,
    status: Option<String>,
}

struct ConnectionDraft {
    endpoint: Endpoint,
    name: Entity<TextField>,
    address: Entity<TextField>,
    model: Entity<TextField>,
    key: Entity<TextField>,
    context: Entity<TextField>,
}

impl ConnectionDraft {
    fn value(&self, cx: &App, strict: bool) -> anyhow::Result<Endpoint> {
        let mut endpoint = self.endpoint.clone();
        endpoint.name = self.name.read(cx).value().trim().to_owned();
        endpoint.base_url = self.address.read(cx).value().trim().to_owned();
        endpoint.model = self.model.read(cx).value().trim().to_owned();
        endpoint.api_key = self.key.read(cx).value().trim().to_owned();
        endpoint.context_tokens = match self.context.read(cx).value().parse() {
            Ok(value) => value,
            Err(_) if strict => {
                return Err(crate::ai::settings::EndpointError::InvalidContext.into());
            }
            Err(_) => endpoint.context_tokens,
        };
        Ok(endpoint)
    }
}

impl Drop for AiPreferences {
    fn drop(&mut self) {
        self.control.cancel.cancel();
    }
}

impl AiPreferences {
    fn connection_name(&self, endpoint: &Endpoint, cx: &App) -> String {
        self.draft
            .as_ref()
            .filter(|draft| draft.endpoint.id == endpoint.id)
            .or_else(|| self.drafts.get(&endpoint.id))
            .map_or_else(
                || endpoint.name.clone(),
                |draft| draft.name.read(cx).value().to_owned(),
            )
    }

    fn staged(&self, cx: &App) -> anyhow::Result<AiSettings> {
        let mut settings = self.settings.clone();
        self.instruction.stage(&mut settings, cx);
        settings.excluded_paths = self
            .excluded
            .read(cx)
            .value()
            .split([',', '\n'])
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .map(str::to_owned)
            .collect();
        for draft in self.drafts.values().chain(self.draft.iter()) {
            let staged = draft
                .value(cx, settings.enabled)
                .and_then(|endpoint| settings.set_endpoint(endpoint));
            if let Err(error) = staged {
                return Err(anyhow::anyhow!(
                    "{}: {}",
                    draft.name.read(cx).value(),
                    crate::ai::settings::error_message(
                        &error,
                        cx.global::<I18nManager>().strings()
                    )
                ));
            }
        }
        Ok(settings)
    }
}

impl PreferencesWindow {
    fn ai_field(&self, value: &str, masked: bool, cx: &mut Context<Self>) -> Entity<TextField> {
        let owner = cx.entity().downgrade();
        cx.new(|cx| {
            let mut field = TextField::new("", cx);
            field.set_value(value, cx);
            if masked {
                field = field.masked();
            }
            field.on_change(move |cx| {
                let owner = owner.clone();
                cx.defer(move |cx| {
                    if let Err(error) = owner.update(cx, |preferences, cx| {
                        preferences.stop_ai_preferences_probe();
                        cx.notify();
                    }) {
                        eprintln!("更新 AI 偏好输入失败：{error}");
                    }
                });
            })
        })
    }

    pub(crate) fn set_nav_ai(
        &mut self,
        _: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_dropdowns();
        self.focus_handle.focus(window, cx);
        self.nav = PreferencesNav::Ai;
        self.page_scroll.set_offset(Point::default());
        if self.ai_preferences.is_none() {
            match AiSettings::load() {
                Ok(settings) => {
                    let endpoint = settings.active_endpoint().cloned();
                    let excluded = self.ai_field(&settings.excluded_paths.join(", "), false, cx);
                    let instruction = instructions::InstructionDraft::new(&settings, cx);
                    self.ai_preferences = Some(AiPreferences {
                        instruction,
                        show_instructions: false,
                        instruction_menu_open: false,
                        saved: settings.clone(),
                        settings,
                        draft: None,
                        drafts: Default::default(),
                        choosing_provider: false,
                        advanced: false,
                        connection_menu_open: false,
                        pending_focus: None,
                        excluded,
                        testing: false,
                        generation: 0,
                        control: RunControl::default(),
                        status: None,
                    });
                    if let Some(endpoint) = endpoint {
                        self.load_ai_preferences_draft(endpoint, cx);
                    }
                }
                Err(error) => self.save_error = Some(error.to_string()),
            }
        }
        cx.notify();
    }

    fn load_ai_preferences_draft(&mut self, endpoint: Endpoint, cx: &mut Context<Self>) {
        self.stop_ai_preferences_probe();
        let draft = ConnectionDraft {
            name: self.ai_field(&endpoint.name, false, cx),
            address: self.ai_field(&endpoint.base_url, false, cx),
            model: self.ai_field(&endpoint.model, false, cx),
            key: self.ai_field(&endpoint.api_key, true, cx),
            context: self.ai_field(&endpoint.context_tokens.to_string(), false, cx),
            endpoint,
        };
        if let Some(state) = self.ai_preferences.as_mut() {
            state.draft = Some(draft);
            state.status = None;
        }
        cx.notify();
    }

    fn select_ai_preferences_endpoint(&mut self, endpoint: Endpoint, cx: &mut Context<Self>) {
        self.stop_ai_preferences_probe();
        if let Some(state) = self.ai_preferences.as_mut() {
            if let Some(previous) = state.draft.take() {
                state.drafts.insert(previous.endpoint.id.clone(), previous);
            }
            state.choosing_provider = false;
            state.connection_menu_open = false;
            state.settings.default_endpoint = endpoint.id.clone();
        }
        let cached = self
            .ai_preferences
            .as_mut()
            .and_then(|state| state.drafts.remove(&endpoint.id));
        if let Some(draft) = cached {
            if let Some(state) = self.ai_preferences.as_mut() {
                state.draft = Some(draft);
            }
            cx.notify();
        } else {
            self.load_ai_preferences_draft(endpoint, cx);
        }
    }

    fn begin_ai_preferences_connection(&mut self, cx: &mut Context<Self>) {
        self.stop_ai_preferences_probe();
        if let Some(state) = self.ai_preferences.as_mut() {
            if let Some(previous) = state.draft.take() {
                state.drafts.insert(previous.endpoint.id.clone(), previous);
            }
            state.choosing_provider = true;
        }
        cx.notify();
    }

    fn add_ai_preferences_provider(
        &mut self,
        preset: &'static ProviderPreset,
        cx: &mut Context<Self>,
    ) {
        let mut endpoint = preset.endpoint();
        if preset.id == "custom" {
            endpoint.name = cx
                .global::<I18nManager>()
                .strings()
                .preferences_ai_custom_provider
                .clone();
        }
        if let Some(state) = self.ai_preferences.as_mut() {
            let name = endpoint.name.clone();
            let mut suffix = 2;
            while state
                .settings
                .endpoints
                .iter()
                .any(|saved| saved.name == endpoint.name)
            {
                endpoint.name = format!("{name} {suffix}");
                suffix += 1;
            }
            if state.settings.default_endpoint.is_empty() {
                state.settings.default_endpoint = endpoint.id.clone();
            }
            state.settings.endpoints.push(endpoint.clone());
            state.advanced = false;
        }
        self.select_ai_preferences_endpoint(endpoint, cx);
        if let Some(state) = self.ai_preferences.as_mut()
            && let Some(draft) = state.draft.as_ref()
        {
            state.pending_focus = Some(if preset.requires_key {
                draft.key.clone()
            } else {
                draft.model.clone()
            });
        }
    }

    fn toggle_ai_connection_menu(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let open = self
            .ai_preferences
            .as_ref()
            .is_some_and(|state| !state.connection_menu_open);
        self.close_dropdowns();
        if let Some(state) = self.ai_preferences.as_mut() {
            state.connection_menu_open = open;
        }
        cx.notify();
    }

    pub(super) fn apply_ai_preferences_focus(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.refresh_ai_instruction_locale(cx);
        if self.nav == PreferencesNav::Ai
            && let Some(field) = self
                .ai_preferences
                .as_mut()
                .and_then(|state| state.pending_focus.take())
        {
            field.update(cx, |field, cx| field.focus(window, cx));
        }
    }

    fn stop_ai_preferences_probe(&mut self) {
        if let Some(state) = self.ai_preferences.as_mut() {
            state.control.cancel.cancel();
            state.status = None;
            state.testing = false;
            state.generation = state.generation.wrapping_add(1);
        }
    }

    pub(super) fn refresh_ai_preferences_dirty(&self, cx: &App) {
        self.ai_preferences_dirty
            .set(self.ai_preferences.as_ref().is_some_and(|state| {
                state
                    .staged(cx)
                    .map(|settings| settings != state.saved)
                    .unwrap_or(true)
            }));
    }

    pub(super) fn save_ai_preferences(&mut self, cx: &mut Context<Self>) -> anyhow::Result<()> {
        let Some(state) = self.ai_preferences.as_ref() else {
            return Ok(());
        };
        let settings = state.staged(cx)?;
        if settings != state.saved {
            // 本页只在本页首次进入时读盘；持有草稿期间助手面板或别的窗口
            // 可能写过 ai.json。整文件覆盖会把那笔修改静默回滚，先比对磁盘
            // 与载入快照，不一致时报冲突，让用户重开本页拿到最新内容。
            if let Ok(disk) = AiSettings::load() {
                if disk != state.saved {
                    anyhow::bail!(
                        "{}",
                        cx.global::<crate::i18n::I18nManager>()
                            .strings()
                            .preferences_ai_conflict
                    );
                }
            }
            settings.save_at(&crate::ai::settings::settings_path()?)?;
        }
        self.stop_ai_preferences_probe();
        if let Some(state) = self.ai_preferences.as_mut() {
            state.settings = settings.clone();
            state.saved = settings.clone();
        }
        crate::ai::settings::AiFeature::set_enabled(cx, settings.enabled);
        self.ai_preferences_dirty.set(false);
        Ok(())
    }

    fn test_ai_preferences_connection(&mut self, cx: &mut Context<Self>) {
        let Some(state) = self.ai_preferences.as_ref() else {
            return;
        };
        let Some(draft) = state.draft.as_ref() else {
            return;
        };
        let endpoint = match draft.value(cx, true) {
            Ok(endpoint) => endpoint,
            Err(error) => {
                self.save_error = Some(crate::ai::settings::error_message(
                    &error,
                    cx.global::<I18nManager>().strings(),
                ));
                cx.notify();
                return;
            }
        };
        self.stop_ai_preferences_probe();
        let Some(state) = self.ai_preferences.as_mut() else {
            return;
        };
        state.control = RunControl::default();
        state.testing = true;
        state.status = Some(
            cx.global::<I18nManager>()
                .strings()
                .preferences_ai_test_running
                .clone(),
        );
        self.save_error = None;
        let generation = state.generation;
        let control = state.control.clone();
        let probe =
            cx.background_spawn(
                async move { crate::ai::runtime::test_connection(endpoint, control) },
            );
        cx.spawn(async move |owner, cx| {
            let result = probe.await;
            if let Err(error) = owner.update(cx, |preferences, cx| {
                let strings = cx.global::<I18nManager>().strings().clone();
                if let Some(state) = preferences
                    .ai_preferences
                    .as_mut()
                    .filter(|state| state.generation == generation)
                {
                    state.testing = false;
                    match result {
                        Ok(RunOutput { .. }) => state.status = Some(strings.preferences_ai_test_ok),
                        Err(error) => {
                            state.status = None;
                            preferences.save_error =
                                Some(crate::ai::settings::error_message(&error, &strings));
                        }
                    }
                    cx.notify();
                }
            }) {
                eprintln!("显示 AI 偏好连接测试失败：{error}");
            }
        })
        .detach();
        cx.notify();
    }

    pub(super) fn render_ai_page(
        &self,
        theme: &Theme,
        strings: &crate::i18n::I18nStrings,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(state) = self.ai_preferences.as_ref() else {
            return div().into_any_element();
        };
        let toggle = div()
            .debug_selector(|| "preferences-ai-enabled".into())
            .child(
                Switch::new("preferences-ai-enabled-switch")
                    .checked(state.settings.enabled)
                    .on_click(cx.listener(|preferences, _, _, cx| {
                        preferences.stop_ai_preferences_probe();
                        if let Some(state) = preferences.ai_preferences.as_mut() {
                            state.settings.enabled = !state.settings.enabled;
                        }
                        cx.notify();
                    })),
            );
        let enabled_row = self.settings_row(theme, strings.preferences_ai_enabled.clone(), toggle);
        let mut content = div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(enabled_row);
        if !state.settings.enabled {
            return self.settings_card(
                theme,
                vec![div().w_full().p(px(16.0)).child(content).into_any_element()],
            );
        }
        content = content.child(self.ai_preferences_tabs(theme, strings, cx));
        if state.show_instructions {
            content = content.child(self.render_ai_instruction_editor(theme, strings, cx));
            return self.settings_card(
                theme,
                vec![div().w_full().p(px(16.0)).child(content).into_any_element()],
            );
        }
        if let Some(active) = state.settings.active_endpoint() {
            let mut dropdown = div().relative().child(Self::dropdown_button(
                "preferences-ai-connection-menu",
                state.connection_name(active, cx),
                theme,
                Self::toggle_ai_connection_menu,
                cx,
            ));
            if state.connection_menu_open {
                let mut list = div().w_full().flex().flex_col().gap(px(4.0));
                for (index, endpoint) in state.settings.endpoints.iter().enumerate() {
                    let label = state.connection_name(endpoint, cx);
                    let endpoint = endpoint.clone();
                    list = list.child(Self::dropdown_item(
                        ("preferences-ai-connection", index),
                        label,
                        state.settings.default_endpoint == endpoint.id,
                        theme,
                        move |preferences, _, _, cx| {
                            preferences.select_ai_preferences_endpoint(endpoint.clone(), cx)
                        },
                        cx,
                    ));
                }
                dropdown = dropdown.child(self.dropdown_menu(
                    "preferences-ai-connection-menu",
                    list,
                    theme,
                ));
            }
            content = content.child(
                self.settings_row(
                    theme,
                    strings.preferences_ai_default_connection.clone(),
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .child(dropdown)
                        .child(self.ai_preference_button(
                            "preferences-ai-add",
                            "+",
                            true,
                            theme,
                            cx,
                            |preferences, cx| preferences.begin_ai_preferences_connection(cx),
                        )),
                ),
            );
        }
        if state.choosing_provider || state.settings.endpoints.is_empty() {
            let mut choices = div().w_full().flex().flex_wrap().gap(px(8.0));
            for preset in ProviderPreset::all() {
                let label = if preset.id == "custom" {
                    strings.preferences_ai_custom_provider.clone()
                } else {
                    preset.name.into()
                };
                choices = choices.child(self.ai_preference_button(
                    SharedString::from(format!("preferences-ai-provider-{}", preset.id)),
                    label,
                    true,
                    theme,
                    cx,
                    move |preferences, cx| preferences.add_ai_preferences_provider(preset, cx),
                ));
            }
            content = content.child(choices);
        }
        if let Some(draft) = &state.draft {
            for (id, label, field) in [
                (
                    "preferences-ai-name",
                    strings.preferences_ai_name.clone(),
                    draft.name.clone(),
                ),
                (
                    "preferences-ai-address",
                    strings.preferences_ai_api_base_url.clone(),
                    draft.address.clone(),
                ),
                (
                    "preferences-ai-model",
                    strings.preferences_ai_model.clone(),
                    draft.model.clone(),
                ),
                (
                    "preferences-ai-key",
                    strings.preferences_ai_api_key.clone(),
                    draft.key.clone(),
                ),
                (
                    "preferences-ai-context",
                    strings.ai_panel_context_budget.clone(),
                    draft.context.clone(),
                ),
            ] {
                let custom_address = id == "preferences-ai-address"
                    && (draft.endpoint.provider == "custom"
                        || ProviderPreset::for_endpoint(&draft.endpoint).is_none());
                if !state.advanced
                    && !custom_address
                    && !matches!(id, "preferences-ai-model" | "preferences-ai-key")
                {
                    continue;
                }
                content = content.child(
                    div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .gap(px(4.0))
                        .child(
                            div()
                                .text_size(px(12.0))
                                .text_color(theme.colors.dialog_muted)
                                .child(label),
                        )
                        .child(
                            div()
                                .debug_selector(move || id.into())
                                .w_full()
                                .child(field),
                        ),
                );
            }
            let mut protocols = div().flex().flex_wrap().gap(px(6.0));
            for (index, (protocol, label)) in [
                (Protocol::ChatCompletions, "Chat Completions"),
                (Protocol::Responses, "Responses"),
                (Protocol::Messages, "Messages"),
            ]
            .into_iter()
            .enumerate()
            {
                protocols = protocols.child(self.ai_preference_button(
                    ("preferences-ai-protocol", index),
                    format!(
                        "{}{}",
                        label,
                        if draft.endpoint.protocol == protocol {
                            " ✓"
                        } else {
                            ""
                        }
                    ),
                    true,
                    theme,
                    cx,
                    move |preferences, cx| {
                        preferences.stop_ai_preferences_probe();
                        if let Some(draft) = preferences
                            .ai_preferences
                            .as_mut()
                            .and_then(|state| state.draft.as_mut())
                        {
                            draft.endpoint.protocol = protocol;
                        }
                        cx.notify();
                    },
                ));
            }
            let removed_id = draft.endpoint.id.clone();
            if state.advanced {
                content = content.child(protocols);
            }
            content = content.child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(6.0))
                    .child(self.ai_preference_button(
                        "preferences-ai-test",
                        strings.preferences_ai_test_connection.clone(),
                        !state.testing,
                        theme,
                        cx,
                        |preferences, cx| preferences.test_ai_preferences_connection(cx),
                    ))
                    .when(state.advanced, |actions| {
                        actions.child(self.ai_preference_button(
                            "preferences-ai-delete",
                            strings.preferences_ai_delete.clone(),
                            true,
                            theme,
                            cx,
                            move |preferences, cx| {
                                preferences.stop_ai_preferences_probe();
                                let next = preferences.ai_preferences.as_mut().and_then(|state| {
                                    state.settings.remove_endpoint(&removed_id);
                                    state.drafts.remove(&removed_id);
                                    state.draft = None;
                                    state.settings.active_endpoint().cloned()
                                });
                                if let Some(next) = next {
                                    preferences.select_ai_preferences_endpoint(next, cx);
                                }
                                cx.notify();
                            },
                        ))
                    }),
            );
        }
        content = content.child(self.ai_preference_button(
            "preferences-ai-advanced",
            strings.preferences_ai_advanced.clone(),
            true,
            theme,
            cx,
            |preferences, cx| {
                if let Some(state) = preferences.ai_preferences.as_mut() {
                    state.advanced = !state.advanced;
                }
                cx.notify();
            },
        ));
        if state.advanced {
            content = content
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(theme.colors.dialog_muted)
                        .child(strings.ai_panel_exclusions.clone()),
                )
                .child(state.excluded.clone());
        }
        if let Some(status) = &state.status {
            content = content.child(div().text_size(px(12.0)).child(status.clone()));
        }
        if let Ok(path) = crate::ai::settings::settings_path()
            && state.advanced
        {
            content = content.child(
                div()
                    .text_size(px(11.0))
                    .text_color(theme.colors.dialog_muted)
                    .child(path.to_string_lossy().into_owned()),
            );
        }
        self.settings_card(
            theme,
            vec![div().w_full().p(px(16.0)).child(content).into_any_element()],
        )
    }

    fn ai_preference_button(
        &self,
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        enabled: bool,
        theme: &Theme,
        cx: &mut Context<Self>,
        handler: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> Stateful<Div> {
        let id = id.into();
        let selector = id.to_string();
        div()
            .id(id)
            .debug_selector(move || selector.clone())
            .px(px(10.0))
            .py(px(6.0))
            .rounded(px(5.0))
            .cursor_pointer()
            .bg(theme.colors.dialog_secondary_button_bg)
            .text_color(theme.colors.dialog_secondary_button_text)
            .when(!enabled, |element| element.opacity(0.5).cursor_default())
            .child(label.into())
            .on_click(cx.listener(move |preferences, _, _, cx| {
                if enabled {
                    handler(preferences, cx);
                }
            }))
    }
}

#[cfg(test)]
mod tests;
