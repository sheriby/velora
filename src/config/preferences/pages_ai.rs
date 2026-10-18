use super::*;
use crate::ai::runtime::{RunControl, RunOutput};
use crate::ai::settings::{AiSettings, Endpoint, Protocol};
use crate::components::TextField;

pub(super) struct AiPreferences {
    settings: AiSettings,
    saved: AiSettings,
    draft: Option<ConnectionDraft>,
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

impl Drop for AiPreferences {
    fn drop(&mut self) {
        self.control.cancel.cancel();
    }
}

impl AiPreferences {
    fn staged(&self, cx: &App) -> anyhow::Result<AiSettings> {
        let mut settings = self.settings.clone();
        if !settings.enabled {
            return Ok(settings);
        }
        settings.excluded_paths = self
            .excluded
            .read(cx)
            .value()
            .split([',', '\n'])
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .map(str::to_owned)
            .collect();
        if let Some(draft) = &self.draft {
            let mut endpoint = draft.endpoint.clone();
            endpoint.name = draft.name.read(cx).value().trim().to_owned();
            endpoint.base_url = draft.address.read(cx).value().trim().to_owned();
            endpoint.model = draft.model.read(cx).value().trim().to_owned();
            endpoint.api_key = draft.key.read(cx).value().to_owned();
            endpoint.context_tokens = draft
                .context
                .read(cx)
                .value()
                .parse()
                .context("请输入有效的上下文 token 数量")?;
            endpoint.model_config()?;
            if let Some(saved) = settings
                .endpoints
                .iter_mut()
                .find(|saved| saved.id == endpoint.id)
            {
                *saved = endpoint;
            } else {
                if settings.default_endpoint.is_empty() {
                    settings.default_endpoint = endpoint.id.clone();
                }
                settings.endpoints.push(endpoint);
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

    pub(crate) fn set_nav_ai(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.close_dropdowns();
        self.nav = PreferencesNav::Ai;
        self.page_scroll.set_offset(Point::default());
        if self.ai_preferences.is_none() {
            match AiSettings::load() {
                Ok(settings) => {
                    let endpoint = settings.active_endpoint().cloned();
                    let excluded = self.ai_field(&settings.excluded_paths.join(", "), false, cx);
                    self.ai_preferences = Some(AiPreferences {
                        saved: settings.clone(),
                        settings,
                        draft: None,
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

    fn select_ai_preferences_endpoint(
        &mut self,
        endpoint: Option<Endpoint>,
        cx: &mut Context<Self>,
    ) {
        if let Some(state) = self.ai_preferences.as_ref() {
            match state.staged(cx) {
                Ok(settings) => {
                    if let Some(state) = self.ai_preferences.as_mut() {
                        state.settings = settings;
                    }
                }
                Err(error) => {
                    self.save_error = Some(error.to_string());
                    cx.notify();
                    return;
                }
            }
        }
        let endpoint = endpoint.unwrap_or_else(|| Endpoint {
            id: uuid::Uuid::new_v4().to_string(),
            name: "OpenAI Compatible".into(),
            base_url: "https://api.openai.com/v1".into(),
            ..Endpoint::default()
        });
        self.load_ai_preferences_draft(endpoint, cx);
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
        let settings = match state.staged(cx) {
            Ok(settings) => settings,
            Err(error) => {
                self.save_error = Some(error.to_string());
                cx.notify();
                return;
            }
        };
        let Some(endpoint) = state
            .draft
            .as_ref()
            .and_then(|draft| {
                settings
                    .endpoints
                    .iter()
                    .find(|endpoint| endpoint.id == draft.endpoint.id)
            })
            .cloned()
        else {
            return;
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
                            preferences.save_error = Some(error.to_string());
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
        let toggle = div().debug_selector(|| "preferences-ai-enabled".into())
            .child(crate::components::switch::Switch::new("preferences-ai-enabled-switch")
                .checked(state.settings.enabled)
                .on_click(cx.listener(|preferences, _, _, cx| {
                    preferences.stop_ai_preferences_probe();
                    if let Some(state) = preferences.ai_preferences.as_mut() {
                        state.settings.enabled = !state.settings.enabled;
                    }
                    cx.notify();
                })));
        let enabled_row = self.settings_row(theme, strings.preferences_ai_enabled.clone(), toggle);
        let mut content = div().w_full().flex().flex_col().gap(px(12.0)).child(enabled_row);
        if !state.settings.enabled {
            return self.settings_card(theme, vec![div().w_full().p(px(16.0)).child(content).into_any_element()]);
        }
        for (index, endpoint) in state.settings.endpoints.iter().enumerate() {
            let endpoint = endpoint.clone();
            content = content.child(self.ai_preference_button(
                ("preferences-ai-connection", index),
                format!(
                    "{} · {}{}",
                    endpoint.name,
                    endpoint.model,
                    if state.settings.default_endpoint == endpoint.id {
                        " ✓"
                    } else {
                        ""
                    }
                ),
                true,
                theme,
                cx,
                move |preferences, cx| {
                    preferences.select_ai_preferences_endpoint(Some(endpoint.clone()), cx)
                },
            ));
        }
        content = content.child(self.ai_preference_button(
            "preferences-ai-add",
            strings.preferences_ai_add_endpoint.clone(),
            true,
            theme,
            cx,
            |preferences, cx| preferences.select_ai_preferences_endpoint(None, cx),
        ));
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
            let id = draft.endpoint.id.clone();
            let removed_id = id.clone();
            content = content.child(protocols).child(
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
                    .child(self.ai_preference_button(
                        "preferences-ai-default",
                        strings.ai_panel_make_default.clone(),
                        true,
                        theme,
                        cx,
                        move |preferences, cx| {
                            if let Some(state) = preferences.ai_preferences.as_mut() {
                                state.settings.default_endpoint = id.clone();
                            }
                            cx.notify();
                        },
                    ))
                    .child(self.ai_preference_button(
                        "preferences-ai-delete",
                        strings.preferences_ai_delete.clone(),
                        true,
                        theme,
                        cx,
                        move |preferences, cx| {
                            preferences.stop_ai_preferences_probe();
                            let next = preferences.ai_preferences.as_mut().and_then(|state| {
                                state
                                    .settings
                                    .endpoints
                                    .retain(|endpoint| endpoint.id != removed_id);
                                if state.settings.default_endpoint == removed_id {
                                    state.settings.default_endpoint = state
                                        .settings
                                        .endpoints
                                        .first()
                                        .map(|endpoint| endpoint.id.clone())
                                        .unwrap_or_default();
                                }
                                state.draft = None;
                                state.settings.active_endpoint().cloned()
                            });
                            if let Some(next) = next {
                                preferences.load_ai_preferences_draft(next, cx);
                            }
                            cx.notify();
                        },
                    )),
            );
        }
        content = content
            .child(
                div()
                    .text_size(px(12.0))
                    .text_color(theme.colors.dialog_muted)
                    .child(strings.ai_panel_exclusions.clone()),
            )
            .child(state.excluded.clone());
        if let Some(status) = &state.status {
            content = content.child(div().text_size(px(12.0)).child(status.clone()));
        }
        if let Ok(path) = crate::ai::settings::settings_path() {
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
            .when(!enabled, |element| element.opacity(0.5))
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
