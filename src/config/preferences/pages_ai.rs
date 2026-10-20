use super::*;
use crate::ai::providers::ProviderPreset;
use crate::ai::runtime::{RunControl, RunOutput};
use crate::ai::settings::{AiSettings, Endpoint, Protocol};
use gpui_kit::component::input::{
    Input, InputContentType, InputEvent, InputState, Textarea, TextareaState,
};

use gpui_kit::component::{
    Disableable, Sizable,
    button::{Button, ButtonVariants},
    form::{Field, Form},
    group_box::{GroupBox, GroupBoxVariants},
    radio::{Radio, RadioGroup},
};

mod instructions;

pub(super) struct AiPreferences {
    instruction: instructions::InstructionDraft,
    show_instructions: bool,
    connections_focus: FocusHandle,
    instructions_focus: FocusHandle,
    settings: AiSettings,
    saved: AiSettings,
    draft: Option<ConnectionDraft>,
    drafts: std::collections::BTreeMap<String, ConnectionDraft>,
    choosing_provider: bool,
    advanced: bool,
    pending_focus: Option<Entity<InputState>>,
    excluded: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
    testing: bool,
    generation: u64,
    control: RunControl,
    status: Option<String>,
}

struct ConnectionDraft {
    endpoint: Endpoint,
    name: Entity<InputState>,
    address: Entity<InputState>,
    model: Entity<InputState>,
    key: Entity<InputState>,
    context: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
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
                |draft| draft.name.read(cx).value().to_string(),
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
    fn ai_field(
        value: &str,
        masked: bool,
        subscriptions: &mut Vec<Subscription>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        let field = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(value)
                .masked(masked)
        });
        subscriptions.push(
            cx.subscribe(&field, |preferences, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    preferences.stop_ai_preferences_probe();
                    cx.notify();
                }
            }),
        );
        field
    }

    pub(crate) fn set_nav_ai(
        &mut self,
        _: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        Self::dismiss_preferences_popup(window, cx);
        self.focus_handle.focus(window, cx);
        self.nav = PreferencesNav::Ai;
        self.page_scroll.set_offset(Point::default());
        if self.ai_preferences.is_none() {
            match AiSettings::load() {
                Ok(settings) => {
                    let endpoint = settings.active_endpoint().cloned();
                    let mut subscriptions = Vec::new();
                    let excluded = Self::ai_field(
                        &settings.excluded_paths.join(", "),
                        false,
                        &mut subscriptions,
                        window,
                        cx,
                    );
                    let instruction = instructions::InstructionDraft::new(&settings, window, cx);
                    self.ai_preferences = Some(AiPreferences {
                        instruction,
                        show_instructions: false,
                        connections_focus: cx.focus_handle(),
                        instructions_focus: cx.focus_handle(),
                        saved: settings.clone(),
                        settings,
                        draft: None,
                        drafts: Default::default(),
                        choosing_provider: false,
                        advanced: false,
                        pending_focus: None,
                        excluded,
                        _subscriptions: subscriptions,
                        testing: false,
                        generation: 0,
                        control: RunControl::default(),
                        status: None,
                    });
                    if let Some(endpoint) = endpoint {
                        self.load_ai_preferences_draft(endpoint, window, cx);
                    }
                }
                Err(error) => self.save_error = Some(error.to_string()),
            }
        }
        cx.notify();
    }

    fn load_ai_preferences_draft(
        &mut self,
        endpoint: Endpoint,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.stop_ai_preferences_probe();
        let mut subscriptions = Vec::new();
        let draft = ConnectionDraft {
            name: Self::ai_field(&endpoint.name, false, &mut subscriptions, window, cx),
            address: Self::ai_field(&endpoint.base_url, false, &mut subscriptions, window, cx),
            model: Self::ai_field(&endpoint.model, false, &mut subscriptions, window, cx),
            key: Self::ai_field(&endpoint.api_key, true, &mut subscriptions, window, cx),
            context: Self::ai_field(
                &endpoint.context_tokens.to_string(),
                false,
                &mut subscriptions,
                window,
                cx,
            ),
            endpoint,
            _subscriptions: subscriptions,
        };
        if let Some(state) = self.ai_preferences.as_mut() {
            state.draft = Some(draft);
            state.status = None;
        }
        cx.notify();
    }

    fn select_ai_preferences_endpoint(
        &mut self,
        endpoint: Endpoint,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.stop_ai_preferences_probe();
        if let Some(state) = self.ai_preferences.as_mut() {
            if let Some(previous) = state.draft.take() {
                state.drafts.insert(previous.endpoint.id.clone(), previous);
            }
            state.choosing_provider = false;
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
            self.load_ai_preferences_draft(endpoint, window, cx);
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
        window: &mut Window,
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
        self.select_ai_preferences_endpoint(endpoint, window, cx);
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

    pub(super) fn apply_ai_preferences_focus(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.refresh_ai_instruction_locale(window, cx);
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
        &mut self,
        theme: &Theme,
        strings: &crate::i18n::I18nStrings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(state) = self.ai_preferences.as_ref() else {
            return div().into_any_element();
        };
        let enabled = state.settings.enabled;
        let show_instructions = state.show_instructions;
        let show_thinking = state.settings.show_thinking;
        let toggle = Self::boolean_field(
            "preferences-ai-enabled-switch",
            strings.preferences_ai_enabled.clone(),
            enabled,
            cx,
            |preferences, enabled| {
                preferences.stop_ai_preferences_probe();
                if let Some(state) = preferences.ai_preferences.as_mut() {
                    state.settings.enabled = enabled;
                }
            },
        );
        let thinking_toggle = Self::boolean_field(
            "preferences-ai-thinking-switch",
            strings.preferences_ai_show_thinking.clone(),
            show_thinking,
            cx,
            |preferences, show| {
                if let Some(state) = preferences.ai_preferences.as_mut() {
                    state.settings.show_thinking = show;
                }
            },
        );
        // 代理同时服务 AI 端点、更新检查与远程图片，所以不放在「AI 已启用」之后。
        if self.proxy_input.is_none() {
            let initial = self.proxy_url.clone();
            let field = cx.new(|cx| InputState::new(window, cx).default_value(initial));
            let subscription = cx.subscribe(&field, |preferences, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    if let Some(input) = preferences.proxy_input.as_ref() {
                        preferences.proxy_url = input.read(cx).value().to_string();
                    }
                    cx.notify();
                }
            });
            self.proxy_input = Some(field);
            self.proxy_input_subscription = Some(subscription);
        }
        let proxy_field = self.proxy_input.clone().map(|input| {
            Self::ai_input_field(
                "preferences-network-proxy",
                strings.preferences_network_proxy.clone(),
                &input,
                false,
            )
            .description(strings.preferences_network_proxy_hint.clone())
        });
        let mut page = div()
            .w_full()
            .flex()
            .flex_col()
            .gap_4()
            .when_some(proxy_field, |page, field| {
                page.child(Self::preference_group(
                    "preferences-network-group",
                    String::new(),
                    vec![field],
                    theme,
                    window,
                    PREFERENCE_CHOICE_WIDTH,
                ))
            })
            .child(Self::preference_group(
                "preferences-ai-feature",
                String::new(),
                vec![
                    Self::preference_row(
                        strings.preferences_ai_enabled.clone(),
                        div()
                            .debug_selector(|| "preferences-ai-enabled".into())
                            .child(toggle),
                    ),
                    Self::preference_row(
                        strings.preferences_ai_show_thinking.clone(),
                        div()
                            .debug_selector(|| "preferences-ai-show-thinking".into())
                            .child(thinking_toggle),
                    ),
                ],
                theme,
                window,
                rems(3.0),
            ));
        if !enabled {
            return page.into_any_element();
        }
        page = page.child(self.ai_preferences_tabs(strings, window, cx));
        if show_instructions {
            return page
                .child(self.render_ai_instruction_editor(strings, window, cx))
                .into_any_element();
        }
        let connection = self.ai_preferences.as_ref().and_then(|state| {
            state.settings.active_endpoint().map(|active| {
                let choices = state
                    .settings
                    .endpoints
                    .iter()
                    .enumerate()
                    .map(|(index, endpoint)| {
                        super::controls::PreferenceChoice::new(
                            endpoint.id.clone(),
                            state.connection_name(endpoint, cx),
                            format!("preferences-ai-connection-{index}"),
                        )
                    })
                    .collect::<Vec<_>>();
                (active.id.clone(), choices)
            })
        });
        let mut content = div().w_full().flex().flex_col().gap_4();
        if let Some((id, choices)) = connection {
            let initial_choices = choices.clone();
            let selector = self.choice_field(
                "preferences-ai-connection-menu",
                strings.preferences_ai_default_connection.clone(),
                id,
                |_| initial_choices,
                false,
                window,
                cx,
                |preferences, id, window, cx| {
                    let endpoint = preferences.ai_preferences.as_ref().and_then(|state| {
                        state
                            .settings
                            .endpoints
                            .iter()
                            .find(|endpoint| endpoint.id == id)
                            .cloned()
                    });
                    if let Some(endpoint) = endpoint {
                        preferences.select_ai_preferences_endpoint(endpoint, window, cx);
                    }
                },
            );
            self.sync_choice_options("preferences-ai-connection-menu", choices, window, cx);
            content = content.child(
                Form::horizontal().child(Self::preference_row(
                    strings.preferences_ai_default_connection.clone(),
                    div().flex().items_center().gap_2().child(selector).child(
                        Button::new("preferences-ai-add")
                            .debug_selector(|| "preferences-ai-add".into())
                            .outline()
                            .small()
                            .icon(gpui_kit::component::IconName::Plus)
                            .accessibility_label(strings.preferences_ai_add_connection.clone())
                            .on_click(cx.listener(|preferences, _, _, cx| {
                                preferences.begin_ai_preferences_connection(cx)
                            })),
                    ),
                )),
            );
        }
        let Some(state) = self.ai_preferences.as_ref() else {
            return page.into_any_element();
        };
        if state.choosing_provider || state.settings.endpoints.is_empty() {
            let mut choices = div().flex().flex_wrap().gap_2();
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
                    cx,
                    move |preferences, window, cx| {
                        preferences.add_ai_preferences_provider(preset, window, cx)
                    },
                ));
            }
            content = content.child(choices);
        }
        if let Some(draft) = &state.draft {
            let custom_address = draft.endpoint.provider == "custom"
                || ProviderPreset::for_endpoint(&draft.endpoint).is_none();
            let mut fields = vec![
                Self::ai_input_field(
                    "preferences-ai-model",
                    strings.preferences_ai_model.clone(),
                    &draft.model,
                    false,
                ),
                Self::ai_input_field(
                    "preferences-ai-key",
                    strings.preferences_ai_api_key.clone(),
                    &draft.key,
                    true,
                ),
            ];
            if custom_address {
                fields.insert(
                    0,
                    Self::ai_input_field(
                        "preferences-ai-address",
                        strings.preferences_ai_api_base_url.clone(),
                        &draft.address,
                        false,
                    ),
                );
            }
            content = content.child(Form::vertical().children(fields));
            content = content.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        self.ai_preference_button(
                            "preferences-ai-test",
                            strings.preferences_ai_test_connection.clone(),
                            !state.testing,
                            cx,
                            |preferences, _, cx| preferences.test_ai_preferences_connection(cx),
                        )
                        .loading(state.testing),
                    )
                    .child(
                        self.ai_preference_button(
                            "preferences-ai-advanced",
                            strings.preferences_ai_advanced.clone(),
                            true,
                            cx,
                            |preferences, _, cx| {
                                if let Some(state) = preferences.ai_preferences.as_mut() {
                                    state.advanced = !state.advanced;
                                }
                                cx.notify();
                            },
                        )
                        .ghost()
                        .icon(if state.advanced {
                            gpui_kit::component::IconName::ChevronDown
                        } else {
                            gpui_kit::component::IconName::ChevronRight
                        }),
                    ),
            );
            if state.advanced {
                let mut fields = vec![Self::ai_input_field(
                    "preferences-ai-name",
                    strings.preferences_ai_name.clone(),
                    &draft.name,
                    false,
                )];
                if !custom_address {
                    fields.push(Self::ai_input_field(
                        "preferences-ai-address",
                        strings.preferences_ai_api_base_url.clone(),
                        &draft.address,
                        false,
                    ));
                }
                fields.push(Self::ai_input_field(
                    "preferences-ai-context",
                    strings.ai_panel_context_budget.clone(),
                    &draft.context,
                    false,
                ));
                let selected_protocol = match draft.endpoint.protocol {
                    Protocol::ChatCompletions => 0,
                    Protocol::Responses => 1,
                    Protocol::Messages => 2,
                };
                fields.push(
                    Field::new()
                        .label(strings.preferences_ai_protocol.clone())
                        .child(
                            RadioGroup::vertical("preferences-ai-protocols")
                                .selected_index(Some(selected_protocol))
                                .children(
                                    ["Chat Completions", "Responses", "Messages"]
                                        .into_iter()
                                        .enumerate()
                                        .map(|(index, label)| {
                                            Radio::new(("preferences-ai-protocol", index))
                                                .label(label)
                                        }),
                                )
                                .on_change(cx.listener(|preferences, index: &usize, _, cx| {
                                    preferences.stop_ai_preferences_probe();
                                    if let Some(draft) = preferences
                                        .ai_preferences
                                        .as_mut()
                                        .and_then(|state| state.draft.as_mut())
                                    {
                                        draft.endpoint.protocol = match *index {
                                            1 => Protocol::Responses,
                                            2 => Protocol::Messages,
                                            _ => Protocol::ChatCompletions,
                                        };
                                    }
                                    cx.notify();
                                })),
                        ),
                );
                fields.push(Self::ai_input_field(
                    "preferences-ai-excluded",
                    strings.ai_panel_exclusions.clone(),
                    &state.excluded,
                    false,
                ));
                content = content.child(Form::vertical().children(fields));
                let removed_id = draft.endpoint.id.clone();
                content = content.child(
                    div().flex().justify_start().child(
                        self.ai_preference_button(
                            "preferences-ai-delete",
                            strings.preferences_ai_delete.clone(),
                            true,
                            cx,
                            move |preferences, window, cx| {
                                preferences.stop_ai_preferences_probe();
                                let next = preferences.ai_preferences.as_mut().and_then(|state| {
                                    state.settings.remove_endpoint(&removed_id);
                                    state.drafts.remove(&removed_id);
                                    state.draft = None;
                                    state.settings.active_endpoint().cloned()
                                });
                                if let Some(next) = next {
                                    preferences.select_ai_preferences_endpoint(next, window, cx);
                                }
                                cx.notify();
                            },
                        )
                        .danger()
                        .outline(),
                    ),
                );
                if let Ok(path) = crate::ai::settings::settings_path() {
                    content = content.child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.dialog_muted)
                            .truncate()
                            .child(path.to_string_lossy().into_owned()),
                    );
                }
            }
        }
        if let Some(status) = &state.status {
            content = content.child(
                div()
                    .text_sm()
                    .text_color(theme.colors.dialog_muted)
                    .child(status.clone()),
            );
        }
        page.child(
            GroupBox::new()
                .id("preferences-ai-connection-fields")
                .fill()
                .child(content),
        )
        .into_any_element()
    }

    fn ai_input_field(
        id: &'static str,
        label: String,
        state: &Entity<InputState>,
        secret: bool,
    ) -> Field {
        Field::new().label(label.clone()).child(
            div().w_full().debug_selector(move || id.to_string()).child(
                Input::new(state)
                    .id(id)
                    .aria_label(label)
                    .when(secret, |input| {
                        input.content_type(InputContentType::Password)
                    }),
            ),
        )
    }

    fn ai_preference_button(
        &self,
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        enabled: bool,
        cx: &mut Context<Self>,
        handler: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> Button {
        let id = id.into();
        let selector = id.to_string();
        Button::new(id)
            .debug_selector(move || selector.clone())
            .outline()
            .small()
            .label(label)
            .disabled(!enabled)
            .on_click(
                cx.listener(move |preferences, _, window, cx| handler(preferences, window, cx)),
            )
    }
}

#[cfg(test)]
mod tests;
