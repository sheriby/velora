use super::*;
use gpui::prelude::FluentBuilder as _;
use gpui_kit::component::ThemeStyled as _;
use gpui_kit::component::form::{Field, Form};
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::component::resizable::{h_resizable, resizable_panel, resize_handle_appearance};

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
                        false,
                        cx,
                        |button, _cx| button.label("AI").outline(),
                        |editor, window, cx| {
                            if let Some(session) = editor.ai_session.as_mut() {
                                session.visible = true;
                                session
                                    .input
                                    .update(cx, |input, cx| input.focus(window, cx));
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
        let width = (f32::from(viewport.width) - 32.0).clamp(180.0, 620.0);
        let height = (f32::from(viewport.height) - 120.0).clamp(180.0, 720.0);
        let task_height = height.min(if session.saved.workspace.changes.proposals.is_empty() {
            520.0
        } else {
            320.0
        });
        let origin = if compact {
            session.origin
        } else {
            point((viewport.width - px(width)) * 0.5, px(82.0))
        };
        let panel = if compact {
            self.render_ai_workbench(theme, task_height, false, window, cx)
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
                        if compact { task_height } else { height }
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

    pub(in crate::editor) fn render_ai_sidebar_layout(
        &mut self,
        content: AnyElement,
        theme: &crate::theme::Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(sidebar) = self.render_ai_sidebar(theme, window, cx) else {
            return content;
        };
        let layout = self
            .ai_sidebar_layout
            .get_or_insert_with(|| AiSidebarLayout {
                state: cx.new(|_| Default::default()),
                focus: cx.focus_handle(),
            });
        let state = layout.state.clone();
        let focus = layout.focus.clone();
        let viewport = window.viewport_size().width;
        let minimum = (window.rem_size() * 20.0).min(viewport * 0.4);
        let document_minimum = (window.rem_size() * 32.0).min(viewport - minimum);
        let maximum = (window.rem_size() * 40.0)
            .min(viewport - document_minimum)
            .max(minimum);
        let preferred = (viewport * 0.3).clamp(minimum, maximum);
        let appearance = resize_handle_appearance();
        h_resizable("ai-workspace-layout")
            .with_state(&state)
            .with_handle_appearance(std::rc::Rc::new(move |handle, window, cx| {
                let line = appearance(handle, window, cx);
                let width = state.read(cx).sizes().get(1).copied().unwrap_or(preferred);
                let resize_state = state.clone();
                // Kit 0.7 的分隔条只处理指针；键盘仍通过它的同一套尺寸约束调整。
                Some(
                    div()
                        .id("ai-sidebar-resize")
                        .debug_selector(|| "ai-sidebar-resize".into())
                        .test_support()
                        .track_focus(&focus)
                        .tab_index(0)
                        .role(Role::Splitter)
                        .aria_label("Resize AI sidebar")
                        .aria_orientation(Orientation::Vertical)
                        .aria_numeric_value(f64::from(f32::from(width)))
                        .aria_min_numeric_value(f64::from(f32::from(minimum)))
                        .aria_max_numeric_value(f64::from(f32::from(maximum)))
                        // 与库分隔条共享单物理像素边界，焦点环在边界外绘制。
                        .relative()
                        .w(px(1.0))
                        .h_full()
                        .when(
                            focus.is_focused(window) && window.last_input_was_keyboard(),
                            |line| line.focus_ring_style(window, cx),
                        )
                        .on_key_down(
                            move |event: &KeyDownEvent, window: &mut Window, cx: &mut App| {
                                let modifiers = event.keystroke.modifiers;
                                if modifiers.control
                                    || modifiers.platform
                                    || modifiers.alt
                                    || modifiers.function
                                {
                                    return;
                                }
                                let step = window.rem_size()
                                    * if event.keystroke.modifiers.shift {
                                        4.0
                                    } else {
                                        1.0
                                    };
                                let current = resize_state
                                    .read(cx)
                                    .sizes()
                                    .get(1)
                                    .copied()
                                    .unwrap_or(preferred);
                                let width = match event.keystroke.key.as_str() {
                                    "left" => current + step,
                                    "right" => current - step,
                                    "home" => minimum,
                                    "end" => maximum,
                                    _ => return,
                                };
                                resize_state.update(cx, |state, cx| {
                                    state.resize_panel(1, width, window, cx)
                                });
                                cx.stop_propagation();
                            },
                        )
                        .children(line)
                        .into_any_element(),
                )
            }))
            .child(
                resizable_panel()
                    .size_range(document_minimum..Pixels::MAX)
                    .child(content),
            )
            .child(
                resizable_panel()
                    .size(preferred)
                    .size_range(minimum..maximum)
                    .flex_none()
                    .child(sidebar),
            )
            .into_any_element()
    }

    fn render_ai_sidebar(
        &self,
        theme: &crate::theme::Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let session = self
            .ai_session
            .as_ref()
            .filter(|session| session.visible && session.expanded)?;
        let height = f32::from(window.viewport_size().height) - 100.0;
        let content = if session.mode == PanelMode::Task {
            self.render_ai_workbench(theme, height, true, window, cx)
        } else {
            self.render_ai_panel(theme, height, cx)
                .w_full()
                .h_full()
                .max_h(relative(1.0))
                .border_0()
                .into_any_element()
        };
        Some(
            div()
                .bg(theme.colors.dialog_surface)
                .w_full()
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

    pub(super) fn ai_options_menu(&self, menu: PopupMenu, cx: &mut Context<Self>) -> PopupMenu {
        let Some(session) = self.ai_session.as_ref() else {
            return menu;
        };
        let strings = cx.global::<crate::i18n::I18nManager>().strings().clone();
        let mut menu = menu
            .item(
                self.ai_menu_item(strings.ai_panel_new_chat, cx, |editor, window, cx| {
                    editor.new_ai_task(window, cx);
                }),
            )
            .item(
                self.ai_menu_item(strings.ai_panel_history, cx, |editor, _, cx| {
                    editor.show_ai_history(cx);
                }),
            )
            .separator()
            .item(
                self.ai_menu_item(strings.ai_panel_skills, cx, |editor, _, cx| {
                    if let Some(session) = editor.ai_session.as_mut() {
                        session.mode = PanelMode::Skills;
                    }
                    editor.reload_ai_skills(cx);
                    cx.notify();
                }),
            )
            .item(
                self.ai_menu_item(strings.ai_panel_instructions, cx, |editor, _, cx| {
                    if let Some(session) = editor.ai_session.as_mut() {
                        session.mode = PanelMode::Instruction;
                    }
                    cx.notify();
                }),
            );
        if !session.saved.workspace.source_records.is_empty() {
            menu = menu.item(
                self.ai_menu_item(strings.ai_panel_sources, cx, |editor, _, cx| {
                    if let Some(session) = editor.ai_session.as_mut() {
                        session.mode = PanelMode::Sources;
                    }
                    cx.notify();
                }),
            );
        }
        if !session.saved.workspace.changes.proposals.is_empty() {
            menu = menu
                .separator()
                .item(self.ai_menu_item(
                    if session.review_open {
                        strings.ai_task_return
                    } else {
                        strings.ai_review_title
                    },
                    cx,
                    |editor, window, cx| {
                        if let Some(session) = editor.ai_session.as_mut() {
                            session.review_open = !session.review_open;
                            session.review_navigation = None;
                            session
                                .input
                                .update(cx, |input, cx| input.focus(window, cx));
                        }
                        cx.notify();
                    },
                ))
                .item(
                    self.ai_menu_item(strings.ai_panel_discard, cx, |editor, window, cx| {
                        if let Some(session) = editor.ai_session.as_mut() {
                            if session.saved.running {
                                return;
                            }
                            session.saved.workspace.discard_proposals();
                            session.review = None;
                            session.review_open = false;
                            session
                                .input
                                .update(cx, |input, cx| input.focus(window, cx));
                        }
                        editor.refresh_ai_preview(cx);
                        editor.persist_ai_session(cx);
                        cx.notify();
                    })
                    .disabled(session.saved.running),
                );
        }
        menu.separator().item(
            self.ai_menu_item(strings.ai_panel_settings, cx, |editor, _, cx| {
                editor.open_ai_settings(cx);
            }),
        )
    }

    fn ai_menu_item(
        &self,
        label: String,
        cx: &Context<Self>,
        handler: impl Fn(&mut Editor, &mut Window, &mut Context<Editor>) + 'static,
    ) -> PopupMenuItem {
        let session_id = self
            .ai_session
            .as_ref()
            .map(|session| session.saved.id.clone());
        PopupMenuItem::new(label).on_click(cx.listener(move |editor, _, window, cx| {
            // 菜单展开后仍可能切换标签，旧菜单不能操作刚刚切换到的会话。
            if editor.ai_session.as_ref().map(|session| &session.saved.id) == session_id.as_ref() {
                handler(editor, window, cx);
                if let Some(session) = editor.ai_session.as_ref()
                    && session.visible
                    && session.mode != PanelMode::Task
                {
                    window.focus(&session.panel_focus, cx);
                }
            }
        }))
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
            PanelMode::History => &strings.ai_panel_history,
            PanelMode::Skills => &strings.ai_panel_skills,
            PanelMode::Instruction => &strings.ai_panel_instructions,
            PanelMode::Context => &strings.ai_panel_add_context,
            PanelMode::Sources => &strings.ai_panel_sources,
            PanelMode::Task => &strings.ai_panel_more,
        };
        let header = div()
            .w_full()
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(self.ai_button(
                "ai-back",
                false,
                cx,
                |button, cx| {
                    button.icon(IconName::ArrowLeft).accessibility_label(
                        cx.global::<crate::i18n::I18nManager>()
                            .strings()
                            .ai_panel_back
                            .clone(),
                    )
                },
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
            PanelMode::Sources => {
                if !session.saved.workspace.source_records.is_empty() {
                    for (index, record) in session.saved.workspace.source_records.iter().enumerate()
                    {
                        let record = record.clone();
                        let label = format!("{}:{}", record.path, record.line);
                        content = content.child(self.ai_button(
                            ("ai-source", index),
                            false,
                            cx,
                            |button, _cx| button.label(label),
                            move |editor, window, cx| {
                                editor.open_ai_source(record.clone(), window, cx)
                            },
                        ));
                    }
                }
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
                                        false,
                                        cx,
                                        |button, _cx| button.label(title),
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
                                    false,
                                    cx,
                                    |button, _cx| {
                                        button
                                            .icon(IconName::Trash)
                                            .accessibility_label(
                                                strings.ai_panel_delete_conversation.clone(),
                                            )
                                            .tooltip(strings.ai_panel_delete_conversation.clone())
                                    },
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
                    false,
                    cx,
                    |button, _cx| button.label(strings.ai_panel_reload.clone()),
                    |editor, _, cx| editor.reload_ai_skills(cx),
                ));
                for skill in &session.skills.entries {
                    let id = skill.id.clone();
                    let disabled = session.settings.disabled_skills.contains(&skill.id);
                    let toggle_id = id.clone();
                    content = content.child(
                        Checkbox::new(SharedString::from(format!("ai-skill-toggle-{id}")))
                            .checked(!disabled)
                            .accessibility_label(skill.id.clone())
                            .disabled(!(!session.settings_saving))
                            .on_change(cx.listener(move |editor, _, _, cx| {
                                if let Some(session) = editor.ai_session.as_ref() {
                                    let mut settings = session.settings.clone();
                                    if settings.disabled_skills.contains(&toggle_id) {
                                        settings.disabled_skills.retain(|id| id != &toggle_id);
                                    } else {
                                        settings.disabled_skills.push(toggle_id.clone());
                                    }
                                    editor.update_ai_settings(settings, cx);
                                }
                            })),
                    );
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
                        false,
                        cx,
                        |button, _cx| button.label(label).disabled(!(!disabled)),
                        move |editor, window, cx| {
                            if let Some(session) = editor.ai_session.as_mut() {
                                session.input.update(cx, |input, cx| {
                                    input.set_value(
                                        &format!("/{id} {}", input.value()),
                                        window,
                                        cx,
                                    );
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
                    let instruction_label = instruction.name.clone();
                    content = content.child(
                        div()
                            .flex()
                            .gap(px(4.0))
                            .child(self.ai_button(
                                ("ai-template-edit", index),
                                false,
                                cx,
                                |button, _cx| button.label(instruction_label),
                                move |editor, window, cx| {
                                    if let Some(session) = editor.ai_session.as_mut() {
                                        session.instruction_name.update(cx, |field, cx| {
                                            field.set_value(instruction.name.clone(), window, cx)
                                        });
                                        session.instruction_body.update(cx, |field, cx| {
                                            field.set_value(instruction.prompt.clone(), window, cx)
                                        });
                                    }
                                    cx.notify();
                                },
                            ))
                            .child(self.ai_button(
                                ("ai-template-delete", index),
                                false,
                                cx,
                                |button, _cx| {
                                    button
                                        .icon(IconName::Trash)
                                        .accessibility_label(
                                            strings.ai_panel_delete_instruction.clone(),
                                        )
                                        .tooltip(strings.ai_panel_delete_instruction.clone())
                                        .disabled(!(!session.settings_saving))
                                },
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
                    .child(
                        Form::vertical()
                            .child(
                                Field::new()
                                    .label(strings.ai_panel_instruction_name.clone())
                                    .child(
                                        Input::new(&session.instruction_name)
                                            .aria_label(strings.ai_panel_instruction_name.clone()),
                                    ),
                            )
                            .child(
                                Field::new()
                                    .label(strings.ai_panel_instruction_body.clone())
                                    .child(
                                        Textarea::new(&session.instruction_body)
                                            .aria_label(strings.ai_panel_instruction_body.clone()),
                                    ),
                            ),
                    )
                    .child(div().flex().justify_end().child(self.ai_button(
                        "ai-instruction-save",
                        false,
                        cx,
                        |button, cx| {
                            button
                                .label(strings.ai_panel_save_instruction.clone())
                                .primary()
                                .disabled(
                                    session.settings_saving
                                        || session
                                            .instruction_name
                                            .read(cx)
                                            .value()
                                            .trim()
                                            .is_empty()
                                        || session
                                            .instruction_body
                                            .read(cx)
                                            .value()
                                            .trim()
                                            .is_empty(),
                                )
                        },
                        |editor, _, cx| editor.save_ai_instruction(cx),
                    )));
            }
            PanelMode::Context => {
                content = content.child(
                    Form::vertical().child(
                        Field::new()
                            .label(strings.ai_panel_sources.clone())
                            .description(strings.ai_panel_files_scope.clone())
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div().flex_1().min_w_0().child(
                                            Input::new(&session.context_input)
                                                .aria_label(strings.ai_panel_sources.clone())
                                                .disabled(session.saved.running),
                                        ),
                                    )
                                    .child(self.ai_button(
                                        "ai-context-add",
                                        false,
                                        cx,
                                        |button, cx| {
                                            button
                                                .label(strings.ai_panel_add_context.clone())
                                                .outline()
                                                .disabled(
                                                    session.saved.running
                                                        || session
                                                            .context_input
                                                            .read(cx)
                                                            .value()
                                                            .trim()
                                                            .is_empty(),
                                                )
                                        },
                                        |editor, window, cx| {
                                            editor.add_ai_context(None, window, cx)
                                        },
                                    )),
                            ),
                    ),
                );
                let paths = session
                    .saved
                    .references
                    .iter()
                    .filter(|path| !session.context_candidates.contains(path))
                    .chain(session.context_candidates.iter());
                let mut sources = div().flex().flex_col().gap_1();
                for path in paths {
                    let selected = session.saved.references.contains(path);
                    let path = path.clone();
                    sources = sources.child(
                        Checkbox::new(SharedString::from(format!("ai-context-source:{path}")))
                            .label(path.clone())
                            .checked(selected)
                            .disabled(session.saved.running)
                            .w_full()
                            .px_1()
                            .py_1()
                            .on_click(cx.listener(move |editor, _, window, cx| {
                                editor.add_ai_context(Some(path.clone()), window, cx);
                            })),
                    );
                }
                content = content.child(sources);
                content = content.child(div().flex().justify_end().child(self.ai_button(
                    "ai-context-done",
                    false,
                    cx,
                    |button, _cx| button.label(strings.ai_task_done.clone()).primary(),
                    |editor, window, cx| {
                        if let Some(session) = editor.ai_session.as_mut() {
                            session.mode = PanelMode::Task;
                            session
                                .input
                                .update(cx, |input, cx| input.focus(window, cx));
                        }
                        cx.notify();
                    },
                )));
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
        focus_panel: bool,
        cx: &mut Context<Self>,
        build: impl FnOnce(Button, &App) -> Button,
        handler: impl Fn(&mut Editor, &mut Window, &mut Context<Editor>) + 'static,
    ) -> Button {
        let id = id.into();
        let selector = id.to_string();
        build(
            Button::new(id)
                .debug_selector(move || selector.clone())
                .small()
                .ghost(),
            cx,
        )
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(move |editor, _, window, cx| {
            if focus_panel && let Some(session) = editor.ai_session.as_ref() {
                window.focus(&session.panel_focus, cx);
            }
            handler(editor, window, cx);
        }))
    }
}
