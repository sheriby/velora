use super::*;
use gpui::prelude::FluentBuilder;
use gpui_kit::component::form::{Field, Form};
use gpui_kit::component::menu::DropdownMenu as _;
use gpui_kit::component::spinner::Spinner;

impl Editor {
    pub(super) fn render_ai_workbench(
        &self,
        theme: &crate::theme::Theme,
        height: f32,
        sidebar: bool,
        window: &Window,
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
                        false,
                        cx,
                        |button, cx| {
                            button.icon(IconName::Copy).accessibility_label(
                                cx.global::<crate::i18n::I18nManager>()
                                    .strings()
                                    .ai_panel_copy
                                    .clone(),
                            )
                        },
                        |editor, _, cx| editor.copy_ai_result(cx),
                    ))
                },
            )
            .child(self.ai_button(
                "ai-expand",
                false,
                cx,
                |button, cx| {
                    let can_dock = self.ai_sidebar_fits(window, cx);
                    button
                        .icon(if sidebar {
                            IconName::ExternalLink
                        } else {
                            IconName::PanelRightOpen
                        })
                        .accessibility_label(if sidebar {
                            strings.ai_panel_float.clone()
                        } else if can_dock {
                            strings.ai_panel_dock.clone()
                        } else {
                            format!(
                                "{} · {}",
                                strings.ai_panel_dock, strings.ai_panel_dock_unavailable
                            )
                        })
                        .tooltip(if sidebar {
                            strings.ai_panel_float.clone()
                        } else if can_dock {
                            strings.ai_panel_dock.clone()
                        } else {
                            strings.ai_panel_dock_unavailable.clone()
                        })
                        .disabled(!sidebar && !can_dock)
                },
                move |editor, window, cx| {
                    if let Some(session) = editor.ai_session.as_mut() {
                        session.expanded = !sidebar;
                        session.collapsed = false;
                        session.panel_drag = None;
                        session.float_resize = None;
                        session
                            .input
                            .update(cx, |input, cx| input.focus(window, cx));
                    }
                    cx.notify();
                },
            ))
            .when(!sidebar, |header| {
                header.child(self.ai_button(
                    "ai-collapse",
                    false,
                    cx,
                    |button, _cx| {
                        button
                            .icon(IconName::Minus)
                            .accessibility_label(strings.ai_panel_collapse.clone())
                            .tooltip(strings.ai_panel_collapse.clone())
                    },
                    |editor, _, cx| {
                        if let Some(session) = editor.ai_session.as_mut() {
                            session.collapsed = true;
                            session.panel_drag = None;
                            session.float_resize = None;
                        }
                        cx.notify();
                    },
                ))
            })
            .child(self.ai_button(
                "ai-history",
                false,
                cx,
                |button, _cx| {
                    button
                        .icon(IconName::RotateCw)
                        .accessibility_label(strings.ai_panel_history.clone())
                        .tooltip(strings.ai_panel_history.clone())
                },
                |editor, _, cx| editor.show_ai_history(cx),
            ))
            .child(
                Button::new("ai-options")
                    .debug_selector(|| "ai-options".into())
                    .small()
                    .ghost()
                    .icon(IconName::Ellipsis)
                    .accessibility_label(strings.ai_panel_more.clone())
                    .tooltip(strings.ai_panel_more.clone())
                    .dropdown_menu_with_anchor(Anchor::TopRight, {
                        let owner = cx.entity().downgrade();
                        move |menu, _, cx| match owner.upgrade() {
                            Some(owner) => {
                                owner.update(cx, |editor, cx| editor.ai_options_menu(menu, cx))
                            }
                            None => menu,
                        }
                    }),
            )
            .child(self.ai_button(
                "ai-hide",
                false,
                cx,
                |button, cx| {
                    button.icon(IconName::Close).accessibility_label(
                        cx.global::<crate::i18n::I18nManager>()
                            .strings()
                            .file_history_close
                            .clone(),
                    )
                },
                |editor, _, cx| editor.hide_ai_panel(cx),
            ));
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
            .when(
                !sidebar && (has_messages || session.float_size.is_some()),
                |panel| panel.h(px(height)),
            )
            .when(sidebar, |panel| panel.h_full().p(px(16.0)).gap(px(16.0)))
            .when(!sidebar && !has_messages, |panel| panel.gap_1())
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
                false,
                cx,
                |button, _cx| button.label(strings.ai_panel_settings.clone()).outline(),
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
            if let Some(conversation) = &session.conversation {
                body = body.child(conversation.clone());
            }
            panel = panel.child(body);
        } else if sidebar {
            panel = panel.child(div().min_h_0().flex_1());
        }
        if !session.saved.tool_calls.is_empty() {
            panel = panel.child(self.render_ai_tool_trace(theme, &strings, cx));
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
            panel = panel.child(
                Form::vertical().child(
                    Field::new()
                        .label(strings.ai_panel_target_language.clone())
                        .child(
                            Input::new(&session.language)
                                .aria_label(strings.ai_panel_target_language.clone()),
                        ),
                ),
            );
        }
        if session.error.is_some() {
            panel = panel.child(self.ai_button(
                "ai-retry",
                false,
                cx,
                |button, _cx| {
                    button
                        .label(strings.ai_panel_retry.clone())
                        .disabled(!(!session.saved.running))
                },
                |editor, window, cx| editor.retry_ai_task(window, cx),
            ));
        }
        if session.saved.kind == SessionKind::Conversation || !has_messages {
            panel = panel.child(self.render_ai_composer(theme, window, cx));
        } else if session.saved.running {
            panel = panel.child(self.ai_button(
                "ai-stop",
                false,
                cx,
                |button, cx| {
                    button
                        .icon(IconName::Square)
                        .accessibility_label(
                            cx.global::<crate::i18n::I18nManager>()
                                .strings()
                                .ai_panel_stop
                                .clone(),
                        )
                        .with_size(gpui_kit::component::Size::Medium)
                        .rounded_full()
                },
                |editor, _, cx| editor.stop_ai_task(cx),
            ));
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
                                    session.float_resize = None;
                                    return;
                                }
                                if let Some((start, size)) = session.float_resize {
                                    let delta = event.position - start;
                                    session.float_size = Some(Size {
                                        width: (size.width + delta.x).max(px(240.0)),
                                        height: (size.height + delta.y).max(px(180.0)),
                                    });
                                    cx.notify();
                                    cx.stop_propagation();
                                } else if let Some(offset) = session.panel_drag {
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
                            if let Some(session) = editor.ai_session.as_mut() {
                                let was_dragging = session.panel_drag.take().is_some();
                                let was_resizing = session.float_resize.take().is_some();
                                if was_dragging || was_resizing {
                                    cx.stop_propagation();
                                }
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

    /// 工具调用轨迹：默认只显示一行标题，展开后按时间顺序列出名称、参数与结果。
    fn render_ai_tool_trace(
        &self,
        theme: &crate::theme::Theme,
        strings: &crate::i18n::I18nStrings,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(session) = self.ai_session.as_ref() else {
            return div().into_any_element();
        };
        let count = session.saved.tool_calls.len();
        let open = session.tool_trace_open;
        let header = self.ai_button(
            "ai-tool-trace",
            false,
            cx,
            |button, _cx| {
                button
                    .icon(if open {
                        IconName::ChevronDown
                    } else {
                        IconName::ChevronRight
                    })
                    .label(format!("{} · {count}", strings.ai_tool_trace))
                    .accessibility_label(strings.ai_tool_trace.clone())
            },
            |editor, _, cx| {
                if let Some(session) = editor.ai_session.as_mut() {
                    session.tool_trace_open = !session.tool_trace_open;
                }
                cx.notify();
            },
        );
        let mut section = div()
            .id("ai-tool-trace-section")
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(header);
        if open {
            let mut rows = div()
                .id("ai-tool-trace-body")
                .debug_selector(|| "ai-tool-trace-body".into())
                .flex()
                .flex_col()
                .gap(px(2.0))
                .max_h(px(200.0))
                .overflow_y_scroll();
            let first = count.saturating_sub(50);
            for (index, record) in session.saved.tool_calls.iter().enumerate().skip(first) {
                rows = rows.child(self.render_ai_tool_call(theme, strings, index, record));
            }
            section = section.child(rows);
        }
        section.into_any_element()
    }

    fn render_ai_tool_call(
        &self,
        theme: &crate::theme::Theme,
        strings: &crate::i18n::I18nStrings,
        index: usize,
        record: &crate::ai::session::ToolCallRecord,
    ) -> AnyElement {
        use crate::ai::session::ToolCallStatus;
        let (status_icon, status_label) = match record.status {
            ToolCallStatus::Running => (
                Spinner::new().small().into_any_element(),
                strings.ai_tool_running.clone(),
            ),
            ToolCallStatus::Succeeded => (
                gpui_kit::component::Icon::new(IconName::CircleCheck)
                    .text_color(theme.colors.callout_tip_border)
                    .into_any_element(),
                strings.ai_task_done.clone(),
            ),
            ToolCallStatus::Failed => (
                gpui_kit::component::Icon::new(IconName::CircleX)
                    .text_color(theme.colors.callout_warning_border)
                    .into_any_element(),
                strings.ai_tool_failed.clone(),
            ),
        };
        let arguments = tool_trace_preview(&record.arguments, 240);
        let result = tool_trace_preview(&record.result, 600);
        let mut row = div()
            .id(SharedString::from(format!("ai-tool-call-{index}")))
            .debug_selector(move || format!("ai-tool-call-{index}"))
            .flex()
            .flex_col()
            .gap(px(2.0))
            .px(px(6.0))
            .py(px(4.0))
            .rounded(px(4.0))
            .border_b_1()
            .border_color(theme.colors.dialog_border)
            .aria_label(format!("{}: {}", record.name, status_label))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .child(status_icon)
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .font_weight(FontWeight::MEDIUM)
                            .child(record.name.clone()),
                    )
                    .child(
                        div()
                            .ml_auto()
                            .text_size(px(10.0))
                            .text_color(theme.colors.dialog_muted)
                            .child(status_label),
                    ),
            );
        if !arguments.is_empty() {
            row = row.child(
                div()
                    .truncate()
                    .text_size(px(11.0))
                    .text_color(theme.colors.dialog_muted)
                    .child(arguments),
            );
        }
        if !result.is_empty() {
            row = row.child(
                div()
                    .text_size(px(11.0))
                    .text_color(if record.status == ToolCallStatus::Failed {
                        theme.colors.callout_warning_border
                    } else {
                        theme.colors.dialog_muted
                    })
                    .child(result),
            );
        }
        row.into_any_element()
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
                    .map(|proposal| scope.allowed_in(proposal.base.source.as_ref(), &source))
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
                .map(|message| message.text.to_string())
                .unwrap_or_default()
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
    }

    fn render_ai_composer(
        &self,
        theme: &crate::theme::Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(session) = self.ai_session.as_ref() else {
            return div().into_any_element();
        };
        let strings = cx.global::<crate::i18n::I18nManager>().strings_arc();
        let input_id = session.input.entity_id();
        let session_id = session.saved.id.clone();
        let has_input = !session.input.read(cx).value().trim().is_empty();
        let controls = div()
            .w_full()
            .flex()
            .items_center()
            .justify_between()
            .child(self.ai_button(
                "ai-context",
                true,
                cx,
                |button, _cx| {
                    button
                        .label("@")
                        .accessibility_label(strings.ai_panel_add_context.clone())
                        .tooltip(strings.ai_panel_add_context.clone())
                        .disabled(!(!session.saved.running))
                },
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
                            false,
                            cx,
                            |button, cx| {
                                button
                                    .icon(IconName::Square)
                                    .accessibility_label(
                                        cx.global::<crate::i18n::I18nManager>()
                                            .strings()
                                            .ai_panel_stop
                                            .clone(),
                                    )
                                    .with_size(gpui_kit::component::Size::Medium)
                                    .rounded_full()
                            },
                            |editor, _, cx| editor.stop_ai_task(cx),
                        ))
                    })
                    .when(!session.saved.running || has_input, |actions| {
                        actions.child(self.ai_button(
                            "ai-send",
                            false,
                            cx,
                            |button, cx| {
                                button
                                    .icon(IconName::ArrowUp)
                                    .accessibility_label(
                                        cx.global::<crate::i18n::I18nManager>()
                                            .strings()
                                            .ai_panel_send
                                            .clone(),
                                    )
                                    .when(has_input, |button| button.primary())
                                    .disabled(!(has_input))
                                    .with_size(gpui_kit::component::Size::Medium)
                                    .rounded_full()
                            },
                            |editor, window, cx| editor.send_ai_task(window, cx),
                        ))
                    }),
            );
        div()
            .id("ai-composer")
            .debug_selector(|| "ai-composer".into())
            .track_focus(&session.composer_focus)
            .w_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap_1()
            .p_2()
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
                    .child(
                        Textarea::new(&session.input)
                            .appearance(false)
                            .bordered(false)
                            .p_0()
                            .accessibility_id("ai-composer-input")
                            .aria_label(strings.ai_custom_placeholder.clone()),
                    ),
            )
            .child(controls)
            // 使用冒泡的回车动作，让提交与组词检查共享当前窗口。
            .on_action(cx.listener(move |editor, action: &InputEnter, window, cx| {
                let input = editor
                    .ai_session
                    .as_ref()
                    .filter(|session| {
                        session.saved.id == session_id && session.input.entity_id() == input_id
                    })
                    .map(|session| session.input.clone());
                let Some(input) = input else {
                    cx.propagate();
                    return;
                };
                if action.shift
                    || input.update(cx, |input, cx| {
                        input.marked_text_range(window, cx).is_some()
                    })
                {
                    cx.propagate();
                    return;
                }
                editor.send_ai_task(window, cx);
            }))
            .when(
                session.composer_focus.contains_focused(window, cx),
                |composer| {
                    composer.border_color(theme.colors.dialog_primary_button_bg)
                },
            )
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
                false,
                cx,
                |button, _cx| button.label(format!("{label} · {}", proposal.edit_count())),
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
                        false,
                        cx,
                        |button, cx| {
                            button
                                .icon(IconName::ArrowUp)
                                .accessibility_label(
                                    cx.global::<crate::i18n::I18nManager>()
                                        .strings()
                                        .file_history_previous_change
                                        .clone(),
                                )
                                .tooltip(
                                    cx.global::<crate::i18n::I18nManager>()
                                        .strings()
                                        .file_history_previous_change
                                        .clone(),
                                )
                        },
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
                        false,
                        cx,
                        |button, cx| {
                            button
                                .icon(IconName::ArrowDown)
                                .accessibility_label(
                                    cx.global::<crate::i18n::I18nManager>()
                                        .strings()
                                        .file_history_next_change
                                        .clone(),
                                )
                                .tooltip(
                                    cx.global::<crate::i18n::I18nManager>()
                                        .strings()
                                        .file_history_next_change
                                        .clone(),
                                )
                        },
                        |editor, _, cx| editor.move_ai_review_change(true, cx),
                    ));
            }
        }
        actions = actions
            .child(self.ai_button(
                "ai-regenerate",
                false,
                cx,
                |button, cx| {
                    button
                        .icon(IconName::RotateCw)
                        .accessibility_label(
                            cx.global::<crate::i18n::I18nManager>()
                                .strings()
                                .ai_panel_retry
                                .clone(),
                        )
                        .disabled(!(!session.saved.running))
                },
                |editor, window, cx| editor.retry_ai_task(window, cx),
            ))
            .child(div().flex_1());
        if session.saved.workspace.changes.proposals.len() > 1 {
            actions = actions.child(self.ai_button(
                "ai-accept-all",
                false,
                cx,
                |button, _cx| {
                    button
                        .label(strings.ai_panel_accept_all.clone())
                        .disabled(!(self.ai_review_can_accept(None, false)))
                },
                |editor, window, cx| editor.apply_ai_review(None, None, window, cx),
            ));
        }
        if let Some(review) = &session.review {
            let id = review.id.clone();
            let can_accept = self.ai_review_can_accept(Some(&id), false);
            actions = actions.child(
                self.ai_button(
                    "ai-review-file-accept",
                    false,
                    cx,
                    |button, _cx| {
                        button
                            .label(strings.ai_panel_accept_file.clone())
                            .primary()
                            .disabled(!can_accept)
                    },
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

/// 轨迹预览只取前 `limit` 个字符：完整参数与结果留在对话历史里。
fn tool_trace_preview(text: &str, limit: usize) -> String {
    match text.char_indices().nth(limit) {
        Some((end, _)) => format!("{}…", &text[..end]),
        None => text.to_owned(),
    }
}
