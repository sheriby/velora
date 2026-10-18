use super::*;
use gpui::prelude::FluentBuilder;

impl Editor {
    pub(in crate::editor) fn render_ai_assistant(
        &self,
        theme: &crate::theme::Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let session = self.ai_session.as_ref()?;
        if !session.visible {
            if session.document == self.ai_review_navigation_origin(cx)
                && (session.saved.running || !session.saved.workspace.changes.proposals.is_empty())
            {
                return Some(
                    self.ai_button(
                        "ai-return",
                        "AI",
                        false,
                        true,
                        theme,
                        cx,
                        |editor, _, cx| {
                            if let Some(session) = editor.ai_session.as_mut() {
                                session.visible = true;
                            }
                            cx.notify();
                        },
                    )
                    .absolute()
                    .right(px(18.0))
                    .bottom(px(36.0))
                    .into_any_element(),
                );
            }
            return None;
        }
        if session.expanded {
            return None;
        }
        let viewport = window.viewport_size();
        let compact = session.mode == PanelMode::Task;
        let menu = session.mode == PanelMode::Options;
        let width =
            (f32::from(viewport.width) - 32.0).clamp(180.0, if menu { 300.0 } else { 620.0 });
        let height = (f32::from(viewport.height) - 120.0).clamp(180.0, 720.0);
        let task_height = height.min(if session.saved.workspace.changes.proposals.is_empty() {
            520.0
        } else {
            320.0
        });
        let origin = if compact {
            session.origin
        } else if menu {
            let bounds = session.panel_bounds.get();
            point(
                if session.expanded {
                    viewport.width - px(width) - px(16.0)
                } else {
                    bounds.right() - px(width)
                },
                bounds.top() + px(36.0),
            )
        } else {
            point((viewport.width - px(width)) * 0.5, px(82.0))
        };
        let panel = if compact {
            self.render_ai_workbench(theme, task_height, false, cx)
        } else {
            self.render_ai_panel(theme, height, cx)
                .w(px(width))
                .rounded(px(10.0))
                .shadow_lg()
                .into_any_element()
        };
        let origin = point(
            origin
                .x
                .max(px(12.0))
                .min((viewport.width - px(width) - px(12.0)).max(px(12.0))),
            origin.y.max(px(65.0)).min(
                (viewport.height
                    - px(if compact && session.saved.messages.is_empty() {
                        180.0
                    } else {
                        if compact {
                            task_height
                        } else {
                            height.min(520.0)
                        }
                    })
                    - px(36.0))
                .max(px(65.0)),
            ),
        );
        let measured = session.panel_bounds.clone();
        Some(
            div()
                .id("ai-floating-workbench")
                .debug_selector(|| "ai-floating-workbench".into())
                .absolute()
                .left(origin.x)
                .top(origin.y)
                .w(px(width))
                .child(panel)
                .child(
                    canvas(
                        move |bounds, _, _| {
                            if compact {
                                measured.set(bounds);
                            }
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .inset_0(),
                )
                .into_any_element(),
        )
    }

    pub(in crate::editor) fn render_ai_sidebar(
        &self,
        theme: &crate::theme::Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let session = self
            .ai_session
            .as_ref()
            .filter(|session| session.visible && session.expanded)?;
        let width = (f32::from(window.viewport_size().width) * 0.30).clamp(320.0, 480.0);
        let height = f32::from(window.viewport_size().height) - 100.0;
        let content = if session.mode == PanelMode::Task {
            self.render_ai_workbench(theme, height, true, cx)
        } else {
            self.render_ai_panel(theme, height, cx)
                .w_full()
                .h_full()
                .max_h(relative(1.0))
                .border_0()
                .border_l_1()
                .into_any_element()
        };
        Some(
            div()
                .w(px(width))
                .h_full()
                .flex_shrink_0()
                // 块布局会让内部百分比宽度按内容收缩，侧栏要给子视图明确的横向约束。
                .flex()
                .flex_col()
                .min_w_0()
                .debug_selector(|| "ai-sidebar".into())
                .child(content)
                .into_any_element(),
        )
    }

    fn render_ai_panel(
        &self,
        theme: &crate::theme::Theme,
        height: f32,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let strings = cx.global::<crate::i18n::I18nManager>().strings().clone();
        let Some(session) = self.ai_session.as_ref() else {
            return div().id("ai-assistant-panel");
        };
        let caption = match session.mode {
            PanelMode::Settings => &strings.ai_panel_settings,
            PanelMode::History => &strings.ai_panel_history,
            PanelMode::Skills => &strings.ai_panel_skills,
            PanelMode::Instruction => &strings.ai_panel_instructions,
            PanelMode::Context => &strings.ai_panel_add_context,
            _ => &strings.ai_panel_more,
        };
        let header = div()
            .w_full()
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(self.ai_button(
                "ai-back",
                "‹",
                false,
                true,
                theme,
                cx,
                |editor, window, cx| {
                    if let Some(session) = editor.ai_session.as_mut() {
                        session.mode = PanelMode::Task;
                        session
                            .input
                            .update(cx, |input, cx| input.focus(window, cx));
                    }
                    cx.notify();
                },
            ))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(px(13.0))
                    .font_weight(FontWeight::MEDIUM)
                    .child(caption.clone()),
            )
            .child(
                self.ai_button("ai-hide", "×", false, true, theme, cx, |editor, _, cx| {
                    editor.hide_ai_panel(cx)
                }),
            );
        let mut content = div().w_full().flex().flex_col().gap(px(10.0));
        if let Some(error) = session.error.as_ref() {
            content = content.child(
                div()
                    .w_full()
                    .text_size(px(12.0))
                    .text_color(theme.colors.dialog_title)
                    .child(error.clone()),
            );
        }
        match session.mode {
            PanelMode::Options => {
                content = content
                    .child(self.ai_button(
                        "ai-settings",
                        strings.ai_panel_settings.clone(),
                        false,
                        true,
                        theme,
                        cx,
                        |editor, _, cx| {
                            let endpoint = editor
                                .ai_session
                                .as_ref()
                                .and_then(|session| session.settings.active_endpoint())
                                .cloned();
                            editor.edit_ai_endpoint(endpoint, cx);
                        },
                    ))
                    .child(self.ai_button(
                        "ai-options-history",
                        strings.ai_panel_history.clone(),
                        false,
                        true,
                        theme,
                        cx,
                        |editor, _, cx| editor.show_ai_history(cx),
                    ))
                    .child(self.ai_button(
                        "ai-options-skills",
                        strings.ai_panel_skills.clone(),
                        false,
                        true,
                        theme,
                        cx,
                        |editor, _, cx| {
                            if let Some(session) = editor.ai_session.as_mut() {
                                session.mode = PanelMode::Skills;
                            }
                            editor.reload_ai_skills(cx);
                            cx.notify();
                        },
                    ))
                    .child(self.ai_button(
                        "ai-options-instructions",
                        strings.ai_panel_instructions.clone(),
                        false,
                        true,
                        theme,
                        cx,
                        |editor, _, cx| {
                            if let Some(session) = editor.ai_session.as_mut() {
                                session.mode = PanelMode::Instruction;
                            }
                            cx.notify();
                        },
                    ))
                    .child(self.ai_button(
                        "ai-options-new",
                        strings.ai_panel_new_chat.clone(),
                        false,
                        true,
                        theme,
                        cx,
                        |editor, window, cx| editor.new_ai_task(window, cx),
                    ));
                if !session.saved.workspace.changes.proposals.is_empty() {
                    content = content.child(self.ai_button(
                        "ai-options-view",
                        if session.review_open {
                            strings.ai_task_return.clone()
                        } else {
                            strings.ai_review_title.clone()
                        },
                        false,
                        true,
                        theme,
                        cx,
                        |editor, window, cx| {
                            if let Some(session) = editor.ai_session.as_mut() {
                                session.review_open = !session.review_open;
                                session.review_navigation = None;
                                session.mode = PanelMode::Task;
                                session
                                    .input
                                    .update(cx, |input, cx| input.focus(window, cx));
                            }
                            cx.notify();
                        },
                    ));
                    content = content.child(self.ai_button(
                        "ai-options-discard",
                        strings.ai_panel_discard.clone(),
                        false,
                        !session.saved.running,
                        theme,
                        cx,
                        |editor, window, cx| {
                            if let Some(session) = editor.ai_session.as_mut() {
                                session.saved.workspace.discard_proposals();
                                session.review = None;
                                session.review_open = false;
                                session.mode = PanelMode::Task;
                                session
                                    .input
                                    .update(cx, |input, cx| input.focus(window, cx));
                            }
                            editor.refresh_ai_preview(cx);
                            editor.persist_ai_session(cx);
                            cx.notify();
                        },
                    ));
                }
                if !session.saved.workspace.source_records.is_empty() {
                    content = content.child(
                        div()
                            .pt(px(8.0))
                            .text_color(theme.colors.dialog_muted)
                            .child(strings.ai_panel_sources.clone()),
                    );
                    for (index, record) in session.saved.workspace.source_records.iter().enumerate()
                    {
                        let record = record.clone();
                        let label = format!("{}:{}", record.path, record.line);
                        content = content.child(self.ai_button(
                            ("ai-source", index),
                            label,
                            false,
                            true,
                            theme,
                            cx,
                            move |editor, window, cx| {
                                editor.open_ai_source(record.clone(), window, cx)
                            },
                        ));
                    }
                }
            }
            PanelMode::Settings => {
                let mut choices = div().w_full().flex().flex_col().gap(px(4.0));
                for endpoint in &session.settings.endpoints {
                    let endpoint = endpoint.clone();
                    let label = format!("{} · {}", endpoint.name, endpoint.model);
                    choices = choices.child(self.ai_button(
                        SharedString::from(format!("ai-endpoint-{}", endpoint.id)),
                        label,
                        session.saved.endpoint_id == endpoint.id,
                        true,
                        theme,
                        cx,
                        move |editor, _, cx| {
                            if let Some(session) = editor.ai_session.as_mut() {
                                session.saved.endpoint_id = endpoint.id.clone();
                            }
                            editor.edit_ai_endpoint(Some(endpoint.clone()), cx);
                        },
                    ));
                }
                choices = choices.child(self.ai_button(
                    "ai-endpoint-add",
                    strings.preferences_ai_add_endpoint.clone(),
                    false,
                    true,
                    theme,
                    cx,
                    |editor, _, cx| editor.edit_ai_endpoint(None, cx),
                ));
                content = content.child(choices);
                if let Some(draft) = &session.draft {
                    for (label, field) in [
                        (strings.preferences_ai_name.clone(), draft.name.clone()),
                        (
                            strings.preferences_ai_api_base_url.clone(),
                            draft.address.clone(),
                        ),
                        (strings.preferences_ai_model.clone(), draft.model.clone()),
                        (strings.preferences_ai_api_key.clone(), draft.secret.clone()),
                        (
                            strings.ai_panel_context_budget.clone(),
                            draft.context.clone(),
                        ),
                    ] {
                        content = content.child(
                            div()
                                .w_full()
                                .flex()
                                .flex_col()
                                .gap(px(3.0))
                                .child(
                                    div()
                                        .text_size(px(11.0))
                                        .text_color(theme.colors.dialog_muted)
                                        .child(label),
                                )
                                .child(field),
                        );
                    }
                    let mut protocols = div().flex().gap(px(4.0)).flex_wrap();
                    for (protocol, label) in [
                        (Protocol::ChatCompletions, "Chat Completions"),
                        (Protocol::Responses, "Responses"),
                        (Protocol::Messages, "Messages"),
                    ] {
                        protocols = protocols.child(self.ai_button(
                            SharedString::from(format!("ai-protocol-{label}")),
                            label,
                            draft.endpoint.protocol == protocol,
                            true,
                            theme,
                            cx,
                            move |editor, _, cx| {
                                editor.invalidate_ai_connection_test(cx);
                                if let Some(draft) = editor
                                    .ai_session
                                    .as_mut()
                                    .and_then(|session| session.draft.as_mut())
                                {
                                    draft.endpoint.protocol = protocol;
                                }
                                cx.notify();
                            },
                        ));
                    }
                    content = content
                        .child(protocols)
                        .child(self.ai_button(
                            "ai-endpoint-save",
                            strings.preferences_ai_save_endpoint.clone(),
                            true,
                            !session.settings_saving,
                            theme,
                            cx,
                            |editor, _, cx| editor.save_ai_endpoint(cx),
                        ))
                        .child(self.ai_button(
                            "ai-endpoint-test",
                            strings.preferences_ai_test_connection.clone(),
                            false,
                            !session.saved.running,
                            theme,
                            cx,
                            |editor, _, cx| editor.test_ai_connection(cx),
                        ));
                    if !draft.endpoint.api_key.is_empty() {
                        content = content
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .text_color(theme.colors.dialog_muted)
                                    .child(strings.ai_panel_keep_key.clone()),
                            )
                            .child(self.ai_button(
                                "ai-key-clear",
                                strings.ai_panel_clear_key.clone(),
                                false,
                                !session.settings_saving,
                                theme,
                                cx,
                                |editor, _, cx| {
                                    if let Some(draft) = editor
                                        .ai_session
                                        .as_mut()
                                        .and_then(|session| session.draft.as_mut())
                                    {
                                        draft.endpoint.api_key.clear();
                                        draft
                                            .secret
                                            .update(cx, |input, cx| input.set_value("", cx));
                                    }
                                    editor.save_ai_endpoint(cx);
                                },
                            ));
                    }
                    let id = draft.endpoint.id.clone();
                    let default_id = id.clone();
                    if session
                        .settings
                        .endpoints
                        .iter()
                        .any(|endpoint| endpoint.id == id)
                    {
                        content = content.child(
                            div()
                                .flex()
                                .gap(px(4.0))
                                .child(self.ai_button(
                                    "ai-endpoint-default",
                                    strings.ai_panel_make_default.clone(),
                                    false,
                                    !session.settings_saving,
                                    theme,
                                    cx,
                                    move |editor, _, cx| {
                                        if let Some(session) = editor.ai_session.as_ref() {
                                            let mut settings = session.settings.clone();
                                            settings.default_endpoint = default_id.clone();
                                            editor.update_ai_settings(settings, cx);
                                        }
                                    },
                                ))
                                .child(self.ai_button(
                                    "ai-endpoint-delete",
                                    strings.preferences_ai_delete.clone(),
                                    false,
                                    !session.settings_saving,
                                    theme,
                                    cx,
                                    move |editor, _, cx| editor.delete_ai_endpoint(&id, cx),
                                )),
                        );
                    }
                }
                content = content
                    .child(
                        div()
                            .text_size(px(11.0))
                            .child(strings.ai_panel_exclusions.clone()),
                    )
                    .child(session.excluded_input.clone())
                    .child(self.ai_button(
                        "ai-exclusions-save",
                        strings.ai_panel_save.clone(),
                        false,
                        !session.settings_saving,
                        theme,
                        cx,
                        |editor, _, cx| {
                            if let Some(session) = editor.ai_session.as_ref() {
                                let mut settings = session.settings.clone();
                                settings.excluded_paths = session
                                    .excluded_input
                                    .read(cx)
                                    .value()
                                    .split([',', '\n'])
                                    .map(str::trim)
                                    .filter(|path| !path.is_empty())
                                    .map(str::to_owned)
                                    .collect();
                                editor.update_ai_settings(settings, cx);
                            }
                        },
                    ));
                if !session.activity.is_empty() && !session.saved.running {
                    content =
                        content.child(div().text_size(px(12.0)).child(session.activity.clone()));
                }
                if session.input_tokens > 0 || session.output_tokens > 0 {
                    content = content.child(
                        div()
                            .text_size(px(11.0))
                            .text_color(theme.colors.dialog_muted)
                            .child(format!(
                                "{} {} · {} {}",
                                strings.ai_panel_input_tokens,
                                session.input_tokens,
                                strings.ai_panel_output_tokens,
                                session.output_tokens
                            )),
                    );
                }
                content = content
                    .child(self.ai_button(
                        "ai-history",
                        strings.ai_panel_history.clone(),
                        false,
                        true,
                        theme,
                        cx,
                        |editor, _, cx| editor.show_ai_history(cx),
                    ))
                    .child(self.ai_button(
                        "ai-skills",
                        strings.ai_panel_skills.clone(),
                        false,
                        true,
                        theme,
                        cx,
                        |editor, _, cx| {
                            if let Some(session) = editor.ai_session.as_mut() {
                                session.mode = PanelMode::Skills;
                            }
                            cx.notify();
                        },
                    ))
                    .child(self.ai_button(
                        "ai-new-chat",
                        strings.ai_panel_new_chat.clone(),
                        false,
                        true,
                        theme,
                        cx,
                        |editor, window, cx| editor.new_ai_task(window, cx),
                    ))
                    .child(self.ai_button(
                        "ai-save-instruction",
                        strings.ai_panel_save_instruction.clone(),
                        false,
                        true,
                        theme,
                        cx,
                        |editor, _, cx| {
                            if let Some(session) = editor.ai_session.as_mut() {
                                session.mode = PanelMode::Instruction;
                            }
                            cx.notify();
                        },
                    ));
            }
            PanelMode::History => {
                for kind in [SessionKind::Conversation, SessionKind::QuickCommand] {
                    if !session
                        .history_files
                        .iter()
                        .any(|(_, _, entry)| *entry == kind)
                    {
                        continue;
                    }
                    content = content.child(
                        div()
                            .text_size(px(11.0))
                            .text_color(theme.colors.dialog_muted)
                            .child(if kind == SessionKind::Conversation {
                                strings.ai_history_conversations.clone()
                            } else {
                                strings.ai_history_quick_commands.clone()
                            }),
                    );
                    for (path, title, _) in session
                        .history_files
                        .iter()
                        .filter(|(_, _, entry)| *entry == kind)
                    {
                        let path = path.clone();
                        let title = title.clone();
                        let delete_path = path.clone();
                        let identity = path
                            .file_stem()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned();
                        content = content.child(
                            div()
                                .w_full()
                                .flex()
                                .gap(px(4.0))
                                .child(
                                    self.ai_button(
                                        SharedString::from(format!("ai-history-{identity}")),
                                        title,
                                        false,
                                        true,
                                        theme,
                                        cx,
                                        move |editor, window, cx| {
                                            editor.restore_ai_history(path.clone(), window, cx)
                                        },
                                    )
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden(),
                                )
                                .child(self.ai_button(
                                    SharedString::from(format!("ai-history-delete-{identity}")),
                                    "×",
                                    false,
                                    true,
                                    theme,
                                    cx,
                                    move |editor, window, cx| {
                                        editor.delete_ai_history(delete_path.clone(), window, cx)
                                    },
                                )),
                        );
                    }
                }
            }
            PanelMode::Skills => {
                content = content.child(self.ai_button(
                    "ai-skills-reload",
                    strings.ai_panel_reload.clone(),
                    false,
                    true,
                    theme,
                    cx,
                    |editor, _, cx| editor.reload_ai_skills(cx),
                ));
                for skill in &session.skills.entries {
                    let id = skill.id.clone();
                    let disabled = session.settings.disabled_skills.contains(&skill.id);
                    let toggle_id = id.clone();
                    content = content.child(self.ai_button(
                        SharedString::from(format!("ai-skill-toggle-{id}")),
                        if disabled { "○" } else { "✓" },
                        false,
                        !session.settings_saving,
                        theme,
                        cx,
                        move |editor, _, cx| {
                            if let Some(session) = editor.ai_session.as_ref() {
                                let mut settings = session.settings.clone();
                                if settings.disabled_skills.contains(&toggle_id) {
                                    settings.disabled_skills.retain(|id| id != &toggle_id);
                                } else {
                                    settings.disabled_skills.push(toggle_id.clone());
                                }
                                editor.update_ai_settings(settings, cx);
                            }
                        },
                    ));
                    let label = format!(
                        "{} · {}{}",
                        skill.id,
                        skill.description,
                        if disabled {
                            format!(" · {}", strings.ai_panel_disabled)
                        } else {
                            String::new()
                        }
                    );
                    content = content.child(self.ai_button(
                        SharedString::from(format!("ai-skill-{id}")),
                        label,
                        false,
                        !disabled,
                        theme,
                        cx,
                        move |editor, window, cx| {
                            if let Some(session) = editor.ai_session.as_mut() {
                                session.input.update(cx, |input, cx| {
                                    input.set_value(&format!("/{id} {}", input.value()), cx);
                                    input.focus(window, cx);
                                });
                                session.mode = PanelMode::Task;
                            }
                            cx.notify();
                        },
                    ));
                }
                for warning in &session.skills.warnings {
                    content = content.child(div().text_size(px(12.0)).child(warning.clone()));
                }
            }
            PanelMode::Instruction => {
                for (index, instruction) in session.settings.instructions.iter().enumerate() {
                    let instruction = instruction.clone();
                    content = content.child(
                        div()
                            .flex()
                            .gap(px(4.0))
                            .child(self.ai_button(
                                ("ai-template-edit", index),
                                instruction.name.clone(),
                                false,
                                true,
                                theme,
                                cx,
                                move |editor, _, cx| {
                                    if let Some(session) = editor.ai_session.as_mut() {
                                        session.instruction_name.update(cx, |field, cx| {
                                            field.set_value(&instruction.name, cx)
                                        });
                                        session.instruction_body.update(cx, |field, cx| {
                                            field.set_value(&instruction.prompt, cx)
                                        });
                                    }
                                },
                            ))
                            .child(self.ai_button(
                                ("ai-template-delete", index),
                                "×",
                                false,
                                !session.settings_saving,
                                theme,
                                cx,
                                move |editor, _, cx| {
                                    if let Some(session) = editor.ai_session.as_ref() {
                                        let mut settings = session.settings.clone();
                                        if index < settings.instructions.len() {
                                            settings.instructions.remove(index);
                                        }
                                        editor.update_ai_settings(settings, cx);
                                    }
                                },
                            )),
                    );
                }
                content = content
                    .child(session.instruction_name.clone())
                    .child(session.instruction_body.clone())
                    .child(self.ai_button(
                        "ai-instruction-save",
                        strings.ai_panel_save_instruction.clone(),
                        true,
                        true,
                        theme,
                        cx,
                        |editor, _, cx| editor.save_ai_instruction(cx),
                    ));
            }
            PanelMode::Context => {
                content = content
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(theme.colors.dialog_muted)
                            .child(strings.ai_panel_files_scope.clone()),
                    )
                    .child(session.context_input.clone())
                    .child(self.ai_button(
                        "ai-context-add",
                        strings.ai_panel_add_context.clone(),
                        false,
                        !session.saved.running,
                        theme,
                        cx,
                        |editor, _, cx| editor.add_ai_context(None, cx),
                    ));
                let mut selected = div().flex().flex_wrap().gap(px(4.0));
                for (index, path) in session.saved.references.iter().enumerate() {
                    let path = path.clone();
                    selected = selected.child(self.ai_button(
                        ("ai-context-remove", index),
                        format!("{path} ×"),
                        false,
                        !session.saved.running,
                        theme,
                        cx,
                        move |editor, _, cx| {
                            if let Some(session) = editor.ai_session.as_mut() {
                                session
                                    .saved
                                    .references
                                    .retain(|selected| selected != &path);
                            }
                            cx.notify();
                        },
                    ));
                }
                content = content.child(selected);
                for (index, path) in session.context_candidates.iter().enumerate() {
                    let chosen = session.saved.references.contains(path);
                    let path = path.clone();
                    content = content.child(
                        self.ai_button(
                            ("ai-context-choice", index),
                            format!("{}  {path}", if chosen { "✓" } else { "+" }),
                            false,
                            !session.saved.running,
                            theme,
                            cx,
                            move |editor, _, cx| editor.add_ai_context(Some(path.clone()), cx),
                        )
                        .w_full()
                        .when(chosen, |button| {
                            button.bg(theme.colors.dialog_secondary_button_hover)
                        }),
                    );
                }
                content = content.child(self.ai_button(
                    "ai-context-done",
                    strings.ai_task_done.clone(),
                    true,
                    true,
                    theme,
                    cx,
                    |editor, window, cx| {
                        if let Some(session) = editor.ai_session.as_mut() {
                            session.mode = PanelMode::Task;
                            session
                                .input
                                .update(cx, |input, cx| input.focus(window, cx));
                        }
                        cx.notify();
                    },
                ));
            }
            PanelMode::Task => {}
        }

        let body = div()
            .id("ai-content")
            .w_full()
            .min_h_0()
            .max_h(px((height - 150.0).max(90.0)))
            .overflow_y_scroll()
            .track_scroll(&session.scroll)
            .child(content);
        let panel = div()
            .id("ai-assistant-panel")
            .track_focus(&session.panel_focus)
            .debug_selector(|| "ai-assistant-panel".into())
            .p(px(10.0))
            .flex()
            .flex_col()
            .gap(px(8.0))
            .max_h(px(height))
            .min_w_0()
            .bg(theme.colors.dialog_surface)
            .border_1()
            .border_color(theme.colors.dialog_border)
            .text_color(theme.colors.dialog_title)
            .text_size(px(13.0))
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(header)
            .child(body);

        panel
    }

    pub(super) fn ai_button(
        &self,
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        primary: bool,
        enabled: bool,
        theme: &crate::theme::Theme,
        cx: &mut Context<Self>,
        handler: impl Fn(&mut Editor, &mut Window, &mut Context<Editor>) + 'static,
    ) -> Stateful<Div> {
        let id = id.into();
        let selector = id.to_string();
        let strings = cx.global::<crate::i18n::I18nManager>().strings();
        let tooltip = match selector.as_str() {
            "ai-hide" | "ai-task-hide" | "ai-details-hide" => {
                Some(strings.file_history_close.clone())
            }
            "ai-review-previous" => Some(strings.file_history_previous_change.clone()),
            "ai-review-next" => Some(strings.file_history_next_change.clone()),
            "ai-stop" => Some(strings.ai_panel_stop.clone()),
            "ai-send" => Some(strings.ai_panel_send.clone()),
            "ai-copy-result" => Some(strings.ai_panel_copy.clone()),
            "ai-context" => Some(strings.ai_panel_add_context.clone()),
            "ai-regenerate" => Some(strings.ai_panel_retry.clone()),
            "ai-expand" => Some(
                if self
                    .ai_session
                    .as_ref()
                    .is_some_and(|session| session.expanded)
                {
                    strings.ai_panel_float.clone()
                } else {
                    strings.ai_panel_dock.clone()
                },
            ),
            "ai-options" => Some(strings.ai_panel_more.clone()),
            "ai-task-details" => Some(strings.ai_task_details.clone()),
            "ai-review-accept" => Some(format!(
                "{} · {}",
                strings.ai_panel_accept_edit,
                if cfg!(target_os = "macos") {
                    "⌘Enter"
                } else {
                    "Ctrl+Enter"
                }
            )),
            _ => None,
        };
        let icon = match selector.as_str() {
            "ai-hide" => Some("icon/workspace/tab-close.svg"),
            "ai-copy-result" => Some("icon/workspace/copy.svg"),
            "ai-options" => Some("icon/editor/ai-more.svg"),
            "ai-send" => Some("icon/editor/ai-send.svg"),
            "ai-stop" => Some("icon/editor/ai-stop.svg"),
            "ai-regenerate" => Some("icon/editor/ai-retry.svg"),
            "ai-back" => Some("icon/editor/ai-back.svg"),
            "ai-expand" => Some(
                if self
                    .ai_session
                    .as_ref()
                    .is_some_and(|session| session.expanded)
                {
                    "icon/editor/ai-float.svg"
                } else {
                    "icon/editor/ai-dock.svg"
                },
            ),
            _ => None,
        };
        let send_button = selector == "ai-send";
        let opens_menu = matches!(
            selector.as_str(),
            "ai-options"
                | "ai-context"
                | "ai-settings"
                | "ai-options-history"
                | "ai-options-skills"
                | "ai-options-instructions"
        );
        let color = if primary {
            theme.colors.dialog_primary_button_text
        } else {
            theme.colors.dialog_secondary_button_text
        };
        div()
            .id(id)
            .debug_selector(move || selector.clone())
            .px(px(7.0))
            .py(px(4.0))
            .text_size(px(12.0))
            .rounded(px(4.0))
            .cursor_pointer()
            .bg(if primary {
                theme.colors.dialog_primary_button_bg
            } else {
                hsla(0.0, 0.0, 0.0, 0.0)
            })
            .text_color(if primary {
                theme.colors.dialog_primary_button_text
            } else {
                theme.colors.dialog_secondary_button_text
            })
            .when(enabled, |button| {
                button.hover(|style| {
                    style.bg(if primary {
                        theme.colors.dialog_primary_button_hover
                    } else {
                        theme.colors.dialog_secondary_button_hover
                    })
                })
            })
            .when(!enabled, |button| button.opacity(0.4).cursor_default())
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .when(icon.is_none(), |button| button.child(label.into()))
            .when_some(icon, |button, path| {
                button
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(28.0))
                    .p_0()
                    .rounded(px(6.0))
                    .child(svg().path(path).size(px(15.0)).text_color(color))
            })
            .when(send_button, |button| button.size(px(30.0)).rounded_full())
            .when_some(tooltip, |button, label| {
                button.tooltip(move |_, cx| {
                    cx.new(|_| crate::components::HoverPreviewTooltip {
                        label: label.clone().into(),
                    })
                    .into()
                })
            })
            .on_click(cx.listener(move |editor, _, window, cx| {
                if enabled {
                    if opens_menu && let Some(session) = editor.ai_session.as_ref() {
                        window.focus(&session.panel_focus);
                    }
                    handler(editor, window, cx);
                }
            }))
    }
}
