use super::*;
use gpui::prelude::FluentBuilder;

impl Editor {
    pub(super) fn render_ai_workbench(
        &self,
        theme: &crate::theme::Theme,
        height: f32,
        sidebar: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(session) = self.ai_session.as_ref() else {
            return div().into_any_element();
        };
        let strings = cx.global::<crate::i18n::I18nManager>().strings_arc();
        let caption = self
            .active_split_editor()
            .and_then(|group| group.read(cx).file_path.clone())
            .or_else(|| self.file_path.clone())
            .and_then(|path| {
                path.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .unwrap_or_else(|| strings.ai_task_scope_document.clone());
        let header = div()
            .debug_selector(|| "ai-dialog-header".into())
            .flex_shrink_0()
            .w_full()
            .flex()
            .items_center()
            .gap(px(4.0))
            .h(px(28.0))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(div().font_weight(FontWeight::SEMIBOLD).child(
                        if session.saved.kind == SessionKind::QuickCommand {
                            if session.command == AiCommand::Ask {
                                strings.ai_history_quick_commands.clone()
                            } else {
                                session.command.label(&strings)
                            }
                        } else {
                            "AI".into()
                        },
                    ))
                    .child(
                        div()
                            .truncate()
                            .text_size(px(11.0))
                            .text_color(theme.colors.dialog_muted)
                            .child(caption),
                    ),
            )
            .when(
                session.review.is_some()
                    || session
                        .saved
                        .messages
                        .iter()
                        .any(|message| message.role == "assistant" && !message.text.is_empty()),
                |header| {
                    header.child(self.ai_button(
                        "ai-copy-result",
                        "",
                        false,
                        true,
                        theme,
                        cx,
                        |editor, _, cx| editor.copy_ai_result(cx),
                    ))
                },
            )
            .child(self.ai_button(
                "ai-expand",
                if sidebar { "↙" } else { "↗" },
                false,
                true,
                theme,
                cx,
                |editor, _, cx| {
                    if let Some(session) = editor.ai_session.as_mut() {
                        session.expanded = !session.expanded;
                        session.panel_drag = None;
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
                    editor.hide_ai_panel(cx)
                }),
            );
        let titlebar = div()
            .id("ai-float-titlebar")
            .debug_selector(|| "ai-float-titlebar".into())
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .h(px(40.0))
            .occlude()
            .cursor(if session.panel_drag.is_some() {
                CursorStyle::ClosedHand
            } else {
                CursorStyle::OpenHand
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|editor, event: &MouseDownEvent, _, cx| {
                    if let Some(session) = editor.ai_session.as_mut() {
                        session.panel_drag =
                            Some(event.position - session.panel_bounds.get().origin);
                    }
                    cx.stop_propagation();
                }),
            );
        let has_messages = !session.saved.messages.is_empty();
        let mut panel = div()
            .id("ai-assistant-panel")
            .debug_selector(|| "ai-assistant-panel".into())
            .relative()
            .track_focus(&session.panel_focus)
            .w_full()
            .min_h_0()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .p(px(12.0))
            .text_size(px(13.0))
            .line_height(relative(1.4))
            .text_color(theme.colors.dialog_title)
            .bg(theme.colors.dialog_surface)
            .when(!sidebar && has_messages, |panel| panel.h(px(height)))
            .when(sidebar, |panel| panel.h_full().p(px(16.0)).gap(px(16.0)))
            .when(!sidebar, |panel| {
                panel
                    .rounded(px(10.0))
                    .border_1()
                    .border_color(theme.colors.dialog_border)
                    .shadow_lg()
            })
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .when(!sidebar, |panel| panel.child(titlebar))
            .child(header);
        if session
            .settings
            .active_endpoint()
            .is_none_or(|endpoint| endpoint.request_config().is_err())
        {
            panel = panel.child(self.ai_button(
                "ai-configure",
                strings.ai_panel_settings.clone(),
                false,
                true,
                theme,
                cx,
                |editor, _, cx| editor.open_ai_settings(cx),
            ));
        }
        if let Some(error) = &session.error {
            panel = panel.child(
                div()
                    .id("ai-command-error")
                    .debug_selector(|| "ai-command-error".into())
                    .text_size(px(12.0))
                    .text_color(theme.colors.callout_warning_border)
                    .child(error.clone()),
            );
        }
        if has_messages {
            let mut body = div()
                .id("ai-dialog-result")
                .debug_selector(|| "ai-dialog-result".into())
                .w_full()
                .min_h_0()
                .flex_1();
            if session.saved.running {
                let mut messages = div().flex().flex_col().gap(px(14.0)).p(px(8.0));
                for message in session
                    .saved
                    .messages
                    .iter()
                    .filter(|message| !message.text.is_empty())
                {
                    messages = messages.child(
                        div()
                            .w_full()
                            .when(message.role == "user", |row| {
                                row.text_color(theme.colors.dialog_muted)
                            })
                            .child(message.text.clone()),
                    );
                }
                body = body
                    .overflow_y_scroll()
                    .track_scroll(&session.scroll)
                    .child(messages);
            } else if let Some(conversation) = &session.conversation {
                body = body.child(conversation.clone());
            }
            panel = panel.child(body);
        }
        if session.saved.running {
            let activity = match session.activity.as_str() {
                "read_document" => &strings.ai_activity_read,
                "search_documents" => &strings.ai_activity_search,
                "get_outline" => &strings.ai_activity_outline,
                "list_documents" => &strings.ai_activity_list,
                "propose_edit" | "propose_insert_at_cursor" | "reset_proposal" => {
                    &strings.ai_activity_edit
                }
                "propose_document" => &strings.ai_activity_new,
                "read_skill" => &strings.ai_activity_skill,
                _ => &strings.ai_panel_running,
            };
            panel = panel.child(
                div()
                    .text_size(px(11.0))
                    .text_color(theme.colors.dialog_muted)
                    .child(activity.clone()),
            );
        }
        if !session.saved.workspace.changes.proposals.is_empty() {
            panel = panel.child(self.render_ai_pending_actions(theme, cx));
        }
        if session.command == AiCommand::Translate && !has_messages {
            panel = panel.child(session.language.clone());
        }
        if session.saved.kind == SessionKind::Conversation || !has_messages {
            panel = panel.child(self.render_ai_composer(theme, cx));
        } else if session.saved.running {
            panel = panel.child(self.ai_button(
                "ai-stop",
                "■",
                false,
                true,
                theme,
                cx,
                |editor, _, cx| editor.stop_ai_task(cx),
            ));
        }
        if session.error.is_some() {
            panel = panel.child(self.ai_button(
                "ai-retry",
                strings.ai_panel_retry.clone(),
                false,
                !session.saved.running,
                theme,
                cx,
                |editor, _, cx| editor.retry_ai_task(cx),
            ));
        }
        if sidebar && !has_messages {
            panel = panel.child(div().flex_1());
        }
        let owner = cx.entity().downgrade();
        panel = panel.child(
            canvas(
                |_, _, _| (),
                move |_, _, window, _| {
                    let moving = owner.clone();
                    window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                        if !phase.capture() {
                            return;
                        }
                        if let Err(error) = moving.update(cx, |editor, cx| {
                            if let Some(session) = editor.ai_session.as_mut() {
                                if !event.dragging() {
                                    session.panel_drag = None;
                                    return;
                                }
                                if let Some(offset) = session.panel_drag {
                                    session.origin = event.position - offset;
                                    cx.notify();
                                    cx.stop_propagation();
                                }
                            }
                        }) {
                            eprintln!("移动 AI 浮窗失败：{error}");
                        }
                    });
                    let stopped = owner.clone();
                    window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
                        if !phase.capture() {
                            return;
                        }
                        if let Err(error) = stopped.update(cx, |editor, cx| {
                            if let Some(session) = editor.ai_session.as_mut()
                                && session.panel_drag.take().is_some()
                            {
                                cx.stop_propagation();
                            }
                        }) {
                            eprintln!("结束 AI 浮窗拖动失败：{error}");
                        }
                    });
                },
            )
            .absolute()
            .inset_0(),
        );
        panel.into_any_element()
    }

    fn copy_ai_result(&self, cx: &mut Context<Self>) {
        let Some(session) = self.ai_session.as_ref() else {
            return;
        };
        let text = if let Some(review) = session
            .review
            .as_ref()
            .filter(|_| !session.saved.workspace.read_only)
        {
            let source = review.proposed.read(cx).buffer.text();
            if let Some(scope) = &session.saved.workspace.selection_scope {
                match session
                    .saved
                    .workspace
                    .changes
                    .proposals
                    .get(&review.id)
                    .map(|proposal| scope.allowed_in(&proposal.base.source, &source))
                    .transpose()
                {
                    Ok(Some(range)) => source.get(range).unwrap_or_default().to_owned(),
                    Ok(None) => source,
                    Err(error) => {
                        eprintln!("复制选区提议失败：{error}");
                        return;
                    }
                }
            } else {
                source
            }
        } else {
            session
                .saved
                .messages
                .iter()
                .rev()
                .find(|message| message.role == "assistant")
                .map(|message| message.text.clone())
                .unwrap_or_default()
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
    }

    fn render_ai_composer(
        &self,
        theme: &crate::theme::Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(session) = self.ai_session.as_ref() else {
            return div().into_any_element();
        };
        let has_input = !session.input.read(cx).value().trim().is_empty();
        let controls = div()
            .w_full()
            .flex()
            .items_center()
            .justify_between()
            .child(self.ai_button(
                "ai-context",
                "@",
                false,
                !session.saved.running,
                theme,
                cx,
                |editor, _, cx| editor.show_ai_context(cx),
            ))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .when(session.saved.running, |actions| {
                        actions.child(self.ai_button(
                            "ai-stop",
                            "■",
                            false,
                            true,
                            theme,
                            cx,
                            |editor, _, cx| editor.stop_ai_task(cx),
                        ))
                    })
                    .when(!session.saved.running || has_input, |actions| {
                        actions.child(self.ai_button(
                            "ai-send",
                            "↑",
                            has_input,
                            has_input,
                            theme,
                            cx,
                            |editor, _, cx| editor.send_ai_task(cx),
                        ))
                    }),
            );
        div()
            .id("ai-composer")
            .debug_selector(|| "ai-composer".into())
            .w_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .p(px(8.0))
            .rounded(px(10.0))
            .border_1()
            .border_color(theme.colors.dialog_border)
            .bg(theme.colors.dialog_secondary_button_bg)
            .child(
                div()
                    .id("ai-composer-input")
                    .debug_selector(|| "ai-composer-input".into())
                    .w_full()
                    .min_w_0()
                    .child(session.input.clone()),
            )
            .child(controls)
            .into_any_element()
    }

    fn render_ai_pending_actions(
        &self,
        theme: &crate::theme::Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let strings = cx.global::<crate::i18n::I18nManager>().strings_arc();
        let Some(session) = self.ai_session.as_ref() else {
            return div().into_any_element();
        };
        let mut files = div()
            .id("ai-review-files")
            .debug_selector(|| "ai-review-files".into())
            .max_h(px(72.0))
            .overflow_y_scroll()
            .flex()
            .flex_wrap()
            .gap(px(4.0));
        for (index, (id, proposal)) in session.saved.workspace.changes.proposals.iter().enumerate()
        {
            let selected = id.clone();
            let label = Path::new(id)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| id.clone());
            files = files.child(self.ai_button(
                SharedString::from(format!("ai-review-file-{index}")),
                format!("{label} · {}", proposal.edit_count()),
                false,
                true,
                theme,
                cx,
                move |editor, _, cx| {
                    if let Some(session) = editor.ai_session.as_mut() {
                        session.review_open = true;
                        session.review_navigation = None;
                    }
                    editor.select_ai_review_document(&selected, cx);
                    cx.notify();
                },
            ));
        }
        let mut actions = div()
            .id("ai-review-actions")
            .debug_selector(|| "ai-review-actions".into())
            .w_full()
            .flex()
            .items_center()
            .gap(px(6.0));
        if let Some(review) = &session.review {
            if review.edits.len() > 1 {
                actions = actions
                    .child(self.ai_button(
                        "ai-review-previous",
                        "↑",
                        false,
                        true,
                        theme,
                        cx,
                        |editor, _, cx| editor.move_ai_review_change(false, cx),
                    ))
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(theme.colors.dialog_muted)
                            .child(format!("{}/{}", review.index + 1, review.edits.len())),
                    )
                    .child(self.ai_button(
                        "ai-review-next",
                        "↓",
                        false,
                        true,
                        theme,
                        cx,
                        |editor, _, cx| editor.move_ai_review_change(true, cx),
                    ));
            }
        }
        actions = actions
            .child(self.ai_button(
                "ai-regenerate",
                "↻",
                false,
                !session.saved.running,
                theme,
                cx,
                |editor, _, cx| editor.retry_ai_task(cx),
            ))
            .child(div().flex_1());
        if session.saved.workspace.changes.proposals.len() > 1 {
            actions = actions.child(self.ai_button(
                "ai-accept-all",
                strings.ai_panel_accept_all.clone(),
                false,
                self.ai_review_can_accept(None, false),
                theme,
                cx,
                |editor, window, cx| editor.apply_ai_review(None, None, window, cx),
            ));
        }
        if let Some(review) = &session.review {
            let id = review.id.clone();
            actions = actions.child(
                self.ai_button(
                    "ai-review-file-accept",
                    strings.ai_panel_accept_file.clone(),
                    true,
                    self.ai_review_can_accept(Some(&id), false),
                    theme,
                    cx,
                    move |editor, window, cx| {
                        editor.apply_ai_review(Some(id.clone()), None, window, cx)
                    },
                )
                .min_w(px(64.0))
                .text_align(TextAlign::Center),
            );
        }
        div()
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .when(
                session.saved.workspace.changes.proposals.len() > 1
                    || session
                        .saved
                        .workspace
                        .changes
                        .proposals
                        .keys()
                        .next()
                        .is_some_and(|id| {
                            session
                                .saved
                                .target
                                .as_ref()
                                .is_none_or(|target| target.document_id != *id)
                        }),
                |panel| panel.child(files),
            )
            .child(actions)
            .into_any_element()
    }
}
