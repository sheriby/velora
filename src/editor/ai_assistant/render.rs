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
            if session.saved.running || !session.saved.workspace.changes.proposals.is_empty() {
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
        let width = (f32::from(viewport.width) - 32.0).clamp(180.0, 420.0);
        let height = (f32::from(viewport.height) - 120.0).clamp(180.0, 560.0);
        Some(
            anchored()
                .position(session.origin + point(px(0.0), px(12.0)))
                .snap_to_window_with_margin(Edges {
                    top: px(70.0),
                    bottom: px(36.0),
                    left: px(16.0),
                    right: px(16.0),
                })
                .child(
                    self.render_ai_panel(theme, height, cx)
                        .w(px(width))
                        .rounded(px(10.0))
                        .shadow_lg(),
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
        self.ai_session
            .as_ref()
            .filter(|session| session.visible && session.expanded)?;
        let width = (f32::from(window.viewport_size().width) * 0.34).clamp(300.0, 480.0);
        Some(
            self.render_ai_panel(
                theme,
                (f32::from(window.viewport_size().height) - 100.0).max(180.0),
                cx,
            )
            .w(px(width))
            .h_full()
            .flex_shrink_0()
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
        let caption = session
            .saved
            .target
            .as_ref()
            .map(|target| {
                target
                    .path
                    .as_ref()
                    .and_then(|path| path.file_name())
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "Untitled".into())
            })
            .unwrap_or_default();
        let header = div()
            .w_full()
            .flex()
            .items_center()
            .gap(px(6.0))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .text_size(px(13.0))
                    .text_color(theme.colors.dialog_muted)
                    .child(format!("AI · {caption}")),
            )
            .child(self.ai_button(
                "ai-expand",
                if session.expanded { "↙" } else { "↗" },
                false,
                true,
                theme,
                cx,
                |editor, _, cx| {
                    if let Some(session) = editor.ai_session.as_mut() {
                        session.expanded = !session.expanded;
                    }
                    cx.notify();
                },
            ))
            .child(self.ai_button(
                "ai-options",
                "⋯",
                false,
                true,
                theme,
                cx,
                |editor, _, cx| {
                    if let Some(session) = editor.ai_session.as_mut() {
                        session.mode = PanelMode::Options;
                    }
                    cx.notify();
                },
            ))
            .child(
                self.ai_button("ai-hide", "×", false, true, theme, cx, |editor, _, cx| {
                    editor.hide_ai_panel(cx);
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
        if session.mode != PanelMode::Chat {
            content = content.child(self.ai_button(
                "ai-back",
                "‹",
                false,
                true,
                theme,
                cx,
                |editor, _, cx| {
                    if let Some(session) = editor.ai_session.as_mut() {
                        session.mode = PanelMode::Chat;
                    }
                    cx.notify();
                },
            ));
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
                        |editor, window, cx| editor.new_ai_chat(window, cx),
                    ));
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
                        |editor, window, cx| editor.new_ai_chat(window, cx),
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
                for (path, title) in &session.history_files {
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
                                    move |editor, _, cx| {
                                        editor.restore_ai_history(path.clone(), cx)
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
                                session.mode = PanelMode::Chat;
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
                        true,
                        true,
                        theme,
                        cx,
                        |editor, _, cx| editor.add_ai_context(None, cx),
                    ));
                for (index, path) in session.context_candidates.iter().enumerate() {
                    let path = path.clone();
                    content = content.child(self.ai_button(
                        ("ai-context-choice", index),
                        path.clone(),
                        false,
                        true,
                        theme,
                        cx,
                        move |editor, _, cx| editor.add_ai_context(Some(path.clone()), cx),
                    ));
                }
            }
            PanelMode::Chat => {
                if session.saved.messages.is_empty() {
                    let mut actions = div().w_full().flex().flex_wrap().gap(px(4.0));
                    for command in AiCommand::ALL {
                        let label = command.label(&strings);
                        actions = actions.child(
                            self.ai_button(
                                ("ai-command", command as usize),
                                label.clone(),
                                false,
                                !command.requires_selection()
                                    || session
                                        .saved
                                        .target
                                        .as_ref()
                                        .is_some_and(|target| !target.range.is_empty()),
                                theme,
                                cx,
                                move |editor, window, cx| {
                                    if let Some(session) = editor.ai_session.as_mut() {
                                        session.command = command;
                                        session.input.update(cx, |input, cx| {
                                            input.set_value(&label, cx);
                                            input.focus(window, cx);
                                        });
                                    }
                                    if command != AiCommand::Translate {
                                        editor.send_ai_task(cx);
                                    }
                                    cx.notify();
                                },
                            ),
                        );
                    }
                    for instruction in &session.settings.instructions {
                        let prompt = instruction.prompt.clone();
                        actions = actions.child(self.ai_button(
                            SharedString::from(format!("ai-instruction-{}", instruction.name)),
                            instruction.name.clone(),
                            false,
                            true,
                            theme,
                            cx,
                            move |editor, _, cx| {
                                if let Some(session) = editor.ai_session.as_mut() {
                                    session
                                        .input
                                        .update(cx, |input, cx| input.set_value(&prompt, cx));
                                }
                                editor.send_ai_task(cx);
                            },
                        ));
                    }
                    content = content.child(actions);
                }
                if !session.saved.references.is_empty() {
                    let mut references = div().w_full().flex().flex_wrap().gap(px(4.0));
                    for (index, path) in session.saved.references.iter().enumerate() {
                        references = references.child(self.ai_button(
                            ("ai-context-remove", index),
                            format!("{path} ×"),
                            false,
                            !session.saved.running,
                            theme,
                            cx,
                            move |editor, _, cx| {
                                if let Some(session) = editor.ai_session.as_mut() {
                                    if index < session.saved.references.len() {
                                        session.saved.references.remove(index);
                                    }
                                }
                                cx.notify();
                            },
                        ));
                    }
                    content = content.child(references);
                }
                let last_assistant = session
                    .saved
                    .messages
                    .iter()
                    .rposition(|message| message.role == "assistant");
                for (index, message) in session
                    .saved
                    .messages
                    .iter()
                    .enumerate()
                    .filter(|(_, message)| !message.text.is_empty())
                {
                    let mut row = div().w_full().flex().flex_col().gap(px(6.0)).py(px(6.0));
                    if message.role == "assistant"
                        && (!session.saved.running || Some(index) != last_assistant)
                    {
                        if let Some(preview) = session.previews.get(&index) {
                            row = row.child(preview.clone());
                        } else {
                            row = row.child(message.text.clone());
                        }
                    } else {
                        row = row.child(message.text.clone());
                    }
                    if message.role == "assistant" {
                        let copied = message.text.clone();
                        let inserted = message.text.clone();
                        row = row.child(
                            div()
                                .flex()
                                .gap(px(4.0))
                                .child(self.ai_button(
                                    ("ai-copy", index),
                                    strings.ai_panel_copy.clone(),
                                    false,
                                    true,
                                    theme,
                                    cx,
                                    move |_, _, cx| {
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            copied.clone(),
                                        ))
                                    },
                                ))
                                .child(self.ai_button(
                                    ("ai-insert", index),
                                    strings.ai_panel_insert.clone(),
                                    false,
                                    !session.saved.running,
                                    theme,
                                    cx,
                                    move |editor, _, cx| editor.propose_ai_reply(&inserted, cx),
                                )),
                        );
                    }
                    content = content.child(row);
                }
                if session.saved.running {
                    let activity = match session.activity.as_str() {
                        "read_document" => &strings.ai_activity_read,
                        "search_documents" => &strings.ai_activity_search,
                        "get_outline" => &strings.ai_activity_outline,
                        "list_documents" => &strings.ai_activity_list,
                        "propose_edit" | "reset_proposal" => &strings.ai_activity_edit,
                        "propose_document" => &strings.ai_activity_new,
                        "read_skill" => &strings.ai_activity_skill,
                        _ => &strings.ai_panel_running,
                    };
                    content = content.child(
                        div()
                            .text_size(px(12.0))
                            .text_color(theme.colors.dialog_muted)
                            .child(activity.clone()),
                    );
                }
                for (file_index, (id, proposal)) in
                    session.saved.workspace.changes.proposals.iter().enumerate()
                {
                    let mut review = div()
                        .w_full()
                        .p(px(8.0))
                        .border_1()
                        .border_color(theme.colors.dialog_border)
                        .rounded(px(6.0))
                        .flex()
                        .flex_col()
                        .gap(px(6.0))
                        .child(
                            div()
                                .text_size(px(12.0))
                                .child(format!("{} · {id}", strings.ai_panel_pending)),
                        );
                    for (index, edit) in proposal.edits().iter().enumerate() {
                        let before = proposal
                            .base
                            .source
                            .get(edit.range.clone())
                            .unwrap_or_default();
                        let accepted_id = id.clone();
                        let rejected_id = id.clone();
                        review = review
                            .child(div().w_full().text_size(px(12.0)).child(format!(
                                "− {}\n+ {}",
                                before.chars().take(1200).collect::<String>(),
                                edit.replacement.chars().take(1200).collect::<String>()
                            )))
                            .child(
                                div()
                                    .flex()
                                    .gap(px(4.0))
                                    .child(self.ai_button(
                                        ("ai-edit-accept", file_index * 1000 + index),
                                        strings.ai_panel_accept_edit.clone(),
                                        false,
                                        !session.saved.running && !self.is_read_only(),
                                        theme,
                                        cx,
                                        move |editor, window, cx| {
                                            editor.apply_ai_review(
                                                Some(accepted_id.clone()),
                                                Some(index),
                                                window,
                                                cx,
                                            )
                                        },
                                    ))
                                    .child(self.ai_button(
                                        ("ai-edit-reject", file_index * 1000 + index),
                                        strings.ai_panel_reject.clone(),
                                        false,
                                        !session.saved.running,
                                        theme,
                                        cx,
                                        move |editor, _, cx| {
                                            editor.reject_ai_review(&rejected_id, Some(index), cx)
                                        },
                                    )),
                            );
                    }
                    if session.saved.workspace.changes.invalid.contains(id) {
                        review = review.child(
                            div()
                                .text_size(px(12.0))
                                .child(strings.ai_panel_stale.clone()),
                        );
                    } else if session
                        .saved
                        .workspace
                        .changes
                        .dependencies
                        .get(id)
                        .is_some_and(|dependencies| !dependencies.is_empty())
                    {
                        review = review.child(
                            div()
                                .text_size(px(12.0))
                                .child(strings.ai_panel_related.clone()),
                        );
                    }
                    let rejected_id = id.clone();
                    review = review.child(self.ai_button(
                        ("ai-file-reject", file_index),
                        strings.ai_panel_reject.clone(),
                        false,
                        !session.saved.running,
                        theme,
                        cx,
                        move |editor, _, cx| editor.reject_ai_review(&rejected_id, None, cx),
                    ));
                    let id = id.clone();
                    review = review.child(self.ai_button(
                        ("ai-file-accept", file_index),
                        strings.ai_panel_accept_file.clone(),
                        false,
                        !session.saved.running && !self.is_read_only(),
                        theme,
                        cx,
                        move |editor, window, cx| {
                            editor.apply_ai_review(Some(id.clone()), None, window, cx)
                        },
                    ));
                    content = content.child(review);
                }
                if !session.saved.workspace.changes.proposals.is_empty() {
                    content = content.child(
                        div()
                            .flex()
                            .gap(px(6.0))
                            .child(self.ai_button(
                                "ai-accept-all",
                                strings.ai_panel_accept_all.clone(),
                                true,
                                !session.saved.running && !self.is_read_only(),
                                theme,
                                cx,
                                |editor, window, cx| editor.apply_ai_review(None, None, window, cx),
                            ))
                            .child(self.ai_button(
                                "ai-discard-all",
                                strings.ai_panel_discard.clone(),
                                false,
                                !session.saved.running,
                                theme,
                                cx,
                                |editor, _, cx| {
                                    if let Some(session) = editor.ai_session.as_mut() {
                                        session.saved.workspace.discard_proposals();
                                    }
                                    editor.persist_ai_session(cx);
                                    cx.notify();
                                },
                            )),
                    );
                }
                if !session.saved.workspace.source_records.is_empty() {
                    content = content.child(
                        div()
                            .text_size(px(11.0))
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
        }
        let body = div()
            .id("ai-content")
            .w_full()
            .min_h_0()
            .max_h(px((height - 150.0).max(90.0)))
            .overflow_y_scroll()
            .track_scroll(&session.scroll)
            .child(content);
        let mut panel = div()
            .id("ai-assistant-panel")
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
        if session.mode == PanelMode::Chat {
            if session.command == AiCommand::Translate {
                panel = panel.child(session.language.clone());
            }
            panel = panel.child(session.input.clone()).child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .child(self.ai_button(
                        "ai-context",
                        "@",
                        false,
                        !session.saved.running,
                        theme,
                        cx,
                        |editor, _, cx| editor.show_ai_context(cx),
                    ))
                    .child(self.ai_button(
                        "ai-send",
                        strings.ai_panel_send.clone(),
                        true,
                        true,
                        theme,
                        cx,
                        |editor, _, cx| editor.send_ai_task(cx),
                    ))
                    .when(session.saved.running, |row| {
                        row.child(self.ai_button(
                            "ai-stop",
                            strings.ai_panel_stop.clone(),
                            false,
                            true,
                            theme,
                            cx,
                            |editor, _, cx| editor.stop_ai_task(cx),
                        ))
                    })
                    .when(session.error.is_some(), |row| {
                        row.child(self.ai_button(
                            "ai-retry",
                            strings.ai_panel_retry.clone(),
                            false,
                            !session.saved.running,
                            theme,
                            cx,
                            |editor, _, cx| editor.retry_ai_task(cx),
                        ))
                    }),
            );
        }
        panel
    }

    fn ai_button(
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
                theme.colors.dialog_secondary_button_bg
            })
            .text_color(if primary {
                theme.colors.dialog_primary_button_text
            } else {
                theme.colors.dialog_secondary_button_text
            })
            .when(!enabled, |button| button.opacity(0.45))
            .child(label.into())
            .on_click(cx.listener(move |editor, _, window, cx| {
                if enabled {
                    handler(editor, window, cx);
                }
            }))
    }
}
