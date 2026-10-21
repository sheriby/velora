use super::*;
use gpui::prelude::FluentBuilder as _;
use gpui_kit::component::form::{Field, Form};
use gpui_kit::component::alert::Alert;
use gpui_kit::base::ElementExt as _;
use gpui_kit::component::{ActiveTheme as _, command::{Command, CommandGroup, CommandItem}, spinner::Spinner};
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::component::resizable::{h_resizable, resizable_panel};
use gpui_kit::component::scroll::ScrollableElement as _;

impl Editor {
    pub(super) fn ai_sidebar_fits(&self, window: &Window, cx: &App) -> bool {
        let viewport = window.viewport_size().width;
        let navigation = self.workspace_navigation_width(window, cx);
        let minimum_sidebar = (window.rem_size() * 20.0).min(viewport * 0.4);
        viewport >= navigation + window.rem_size() * 20.0 + minimum_sidebar
    }

    pub(in crate::editor) fn render_ai_assistant(
        &self,
        theme: &crate::theme::Theme,
        window: &mut Window,
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
                        |button, cx| {
                            button
                                .label("AI")
                                .secondary()
                                .rounded(cx.theme().radius_lg)
                                .shadow_md()
                        },
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
        if session.collapsed && !session.expanded {
            let strings = cx.global::<crate::i18n::I18nManager>().strings().clone();
            return Some(
                self.ai_button(
                    "ai-collapsed",
                    false,
                    cx,
                    |button, cx| {
                        button
                            .icon(IconName::Bot)
                            .accessibility_label(strings.ai_panel_open.clone())
                            .tooltip(strings.ai_panel_open.clone())
                            // Ghost 变体把边框解成 transparent，outline() 在这类按钮上
                            // 画不出任何东西；细边框要换 Secondary 变体才有。
                            .secondary()
                            .with_size(gpui_kit::component::Size::Medium)
                            .rounded(cx.theme().radius_lg)
                            .shadow_md()
                    },
                    |editor, window, cx| {
                        if let Some(session) = editor.ai_session.as_mut() {
                            session.collapsed = false;
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
        if session.expanded && self.ai_sidebar_fits(window, cx) {
            return None;
        }
        let viewport = window.viewport_size();
        // 浮窗模式不区分形态：历史、技能等集合视图同样用浮窗尺寸与位置，
        // 不能因为点开历史就换成贴边的宽面板。
        let compact = true;
        let inset = window.rem_size();
        let auxiliary_top = px(crate::window_chrome::custom_titlebar_height(
            window,
            &theme.dimensions,
        )) + inset;
        let status_height = if crate::config::EditorSettings::status_bar_preferences(cx).enabled {
            px(theme.dimensions.status_bar_height)
        } else {
            px(0.0)
        };
        let has_messages = !session.saved.messages.is_empty();
        let default_width = if compact {
            (f32::from(viewport.width) - 32.0).clamp(180.0, 620.0)
        } else {
            f32::from(
                (viewport.width - inset * 2.0)
                    .max(px(0.0))
                    .min(inset * 38.75),
            )
        };
        let default_height = if compact {
            (f32::from(viewport.height) - 120.0).clamp(180.0, 720.0)
        } else {
            f32::from(
                (viewport.height - auxiliary_top - status_height - inset)
                    .max(px(0.0))
                    .min(inset * 45.0),
            )
        };
        // 用户拖过右下角就按他的尺寸渲染；否则保持默认高度，避免空对话被拉长。
        let (width, height) = match (compact, session.float_size) {
            (true, Some(size)) => (
                f32::from(size.width).clamp(240.0, (f32::from(viewport.width) - 24.0).max(240.0)),
                f32::from(size.height)
                    .clamp(180.0, (f32::from(viewport.height) - 80.0).max(180.0)),
            ),
            (true, None) => {
                // 任务浮窗保持「无提议时收紧」的默认高度；集合视图给足高度，
                // 但以浮窗位置为顶、不再向下顶开状态栏。
                let height = if session.mode == PanelMode::Task {
                    default_height.min(if session.saved.workspace.changes.proposals.is_empty() {
                        520.0
                    } else {
                        320.0
                    })
                } else {
                    let max_height = f32::from(
                        viewport.height - session.origin.y - status_height - px(36.0),
                    )
                    .max(180.0);
                    default_height.min(max_height)
                };
                (default_width, height)
            }
            (false, _) => (default_width, default_height),
        };
        let origin = if compact {
            session.origin
        } else {
            point((viewport.width - px(width)) * 0.5, auxiliary_top)
        };
        let panel = if session.mode == PanelMode::Task {
            self.render_ai_workbench(theme, height, false, window, cx)
        } else {
            self.render_ai_panel(theme, height, window, cx)
                .w(px(width))
                .rounded(cx.theme().radius_lg)
                .shadow_lg()
                .into_any_element()
        };
        let origin = if compact {
            // 集合视图（历史/技能等）面板较高，按实际高度避让状态栏；
            // 任务浮窗保持原有规则（空对话只预留一小条）。
            let reserve = if session.mode == PanelMode::Task {
                if has_messages || session.float_size.is_some() {
                    height
                } else {
                    180.0
                }
            } else {
                f32::from(px(height) + status_height)
            };
            point(
                origin
                    .x
                    .max(px(12.0))
                    .min((viewport.width - px(width) - px(12.0)).max(px(12.0))),
                origin.y.max(px(65.0)).min(
                    (viewport.height - px(reserve) - px(36.0)).max(px(65.0)),
                ),
            )
        } else {
            origin
        };
        let measured = session.panel_bounds.clone();
        let mut container = div()
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
            );
        if compact {
            container = container.child(
                div()
                    .id("ai-float-resize")
                    .debug_selector(|| "ai-float-resize".into())
                    .absolute()
                    .right(px(2.0))
                    .bottom(px(2.0))
                    .w(px(16.0))
                    .h(px(16.0))
                    .cursor(CursorStyle::ResizeUpRightDownLeft)
                    .occlude()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|editor, event: &MouseDownEvent, _, cx| {
                            if let Some(session) = editor.ai_session.as_mut() {
                                let size = session.panel_bounds.get().size;
                                if size.width > px(0.0) && size.height > px(0.0) {
                                    session.float_resize = Some((event.position, size));
                                }
                            }
                            cx.stop_propagation();
                        }),
                    )
                    .child(
                        div()
                            .absolute()
                            .right(px(3.0))
                            .bottom(px(3.0))
                            .w(px(8.0))
                            .h(px(8.0))
                            .border_r(px(1.0))
                            .border_b(px(1.0))
                            .border_color(theme.colors.dialog_muted),
                    ),
            );
        }
        Some(container.into_any_element())
    }

    pub(in crate::editor) fn render_ai_sidebar_layout(
        &mut self,
        content: AnyElement,
        theme: &crate::theme::Theme,
        window: &mut Window,
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
        let document_minimum = (self.workspace_navigation_width(window, cx)
            + window.rem_size() * 20.0)
            .max(window.rem_size() * 32.0)
            .min(viewport - minimum);
        let maximum = (window.rem_size() * 40.0)
            .min(viewport - document_minimum)
            .max(minimum);
        let preferred = (viewport * 0.3).clamp(minimum, maximum);
        h_resizable("ai-workspace-layout")
            .with_state(&state)
            .with_handle_appearance(
                crate::ui::PanelResizeHandle {
                    id: "ai-sidebar-resize",
                    label: "Resize AI sidebar",
                    state: state.clone(),
                    focus,
                    panel_index: 1,
                    edge: gpui_kit::base::HandleEdge::Leading,
                    minimum,
                    maximum,
                    initial: preferred,
                }
                .appearance(),
            )
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
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let session = self
            .ai_session
            .as_ref()
            .filter(|session| session.visible && session.expanded)?;
        if !self.ai_sidebar_fits(window, cx) {
            return None;
        }
        let height = f32::from(window.viewport_size().height) - 100.0;
        let content = if session.mode == PanelMode::Task {
            self.render_ai_workbench(theme, height, true, window, cx)
        } else {
            self.render_ai_panel(theme, height, window, cx)
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
                        session.source_focus_pending = true;
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
        window: &mut Window,
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
        let mut header = div()
            .w_full()
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap_2()
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
                    .truncate()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child(caption.clone()),
            );
        if session.mode == PanelMode::History && !session.history_loading {
            if let Some(index) = session.history_list.read(cx).selected_index() {
                let kind = if index.section == 0 {
                    SessionKind::Conversation
                } else {
                    SessionKind::QuickCommand
                };
                if let Some((path, title, _)) = session
                    .history_files
                    .iter()
                    .filter(|(_, _, entry)| *entry == kind)
                    .nth(index.row)
                {
                    let path = path.clone();
                    let list_id = session.history_list.entity_id();
                    header = header.child(self.ai_button(
                        "ai-history-delete",
                        false,
                        cx,
                        |button, _| {
                            button
                                .icon(IconName::Trash)
                                .accessibility_label(format!(
                                    "{} · {title}",
                                    strings.ai_panel_delete_conversation
                                ))
                                .tooltip(strings.ai_panel_delete_conversation.clone())
                        },
                        move |editor, window, cx| {
                            if editor.ai_session.as_ref().is_some_and(|session| {
                                session.mode == PanelMode::History
                                    && session.history_list.entity_id() == list_id
                            }) {
                                editor.delete_ai_history(path.clone(), window, cx);
                            }
                        },
                    ));
                }
            }
        }

        if session.mode == PanelMode::Skills {
            header = header.child(self.ai_button(
                "ai-skills-reload",
                false,
                cx,
                |button, _| {
                    button
                        .icon(IconName::RotateCw)
                        .accessibility_label(strings.ai_panel_reload.clone())
                        .tooltip(strings.ai_panel_reload.clone())
                },
                |editor, _, cx| editor.reload_ai_skills(cx),
            ));
        }

        header = header.child(self.ai_button(
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
        let native_collection = matches!(session.mode, PanelMode::History | PanelMode::Sources);
        let feedback = session.error.as_ref().map(|error| {
            let scroll = window.use_keyed_state("ai-panel-feedback-scroll", cx, |_, _| ScrollHandle::new()).read(cx).clone();
            div().id("ai-panel-feedback").debug_selector(|| "ai-panel-feedback".into())
                .w_full().h_auto().max_h(px(height) * 0.25).flex_shrink_0().relative()
                .overflow_y_scroll().track_scroll(&scroll)
                .child(Alert::error("ai-panel-error", error.clone()).small())
                .vertical_scrollbar(&scroll)
        });
        let mut content = div().w_full().flex().flex_col().gap(px(10.0))
            .when(native_collection, |content| content.flex_1().min_h_0());
        match session.mode {
            PanelMode::Sources => {
                content = content.min_h_0().child(super::source_list::render_sources(self, height, window, cx));
            }
            PanelMode::History => {
                content = content.child(self.render_ai_history(height, cx));
            }
            PanelMode::Skills => {
                if session.skills.entries.is_empty() {
                    content = content.child(
                        div()
                            .p_4()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(strings.ai_skills_empty.clone()),
                    );
                }
                for skill in &session.skills.entries {
                    let id = skill.id.clone();
                    let disabled = session.settings.disabled_skills.contains(&id);
                    let toggle_id = id.clone();
                    let toggle_session = session.saved.id.clone();
                    let use_session = session.saved.id.clone();
                    let scope = match skill.id.split_once(':').map(|(scope, _)| scope) {
                        Some("workspace") => Some(strings.ai_skills_scope_workspace.clone()),
                        Some("user") => Some(strings.ai_skills_scope_user.clone()),
                        _ => None,
                    };
                    let skill_label = scope.as_ref().map_or_else(
                        || skill.name.clone(),
                        |scope| format!("{} · {scope}", skill.name),
                    );
                    let toggle = Checkbox::new(SharedString::from(format!("ai-skill-toggle-{id}")))
                        .checked(!disabled)
                        .accessibility_label(format!(
                            "{} · {}",
                            strings.ai_panel_enable_skill, skill_label
                        ))
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .w_full()
                                .min_w_0()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .child(skill.name.clone()),
                                )
                                .when_some(scope, |row, scope| {
                                    row.child(
                                        div()
                                            .flex_none()
                                            .max_w(relative(0.35))
                                            .truncate()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(scope),
                                    )
                                }),
                        )
                        .disabled(session.settings_saving || session.settings_readonly())
                        .on_change(cx.listener(move |editor, checked: &bool, _, cx| {
                            if let Some(session) = editor
                                .ai_session
                                .as_ref()
                                .filter(|session| session.saved.id == toggle_session)
                            {
                                let mut settings = session.settings.clone();
                                settings.disabled_skills.retain(|id| id != &toggle_id);
                                if !*checked {
                                    settings.disabled_skills.push(toggle_id.clone());
                                }
                                editor.update_ai_settings(settings, cx);
                            }
                        }));
                    let apply = self.ai_button(
                        SharedString::from(format!("ai-skill-{id}")),
                        false,
                        cx,
                        |button, _| {
                            button
                                .label(strings.ai_panel_use_skill.clone())
                                .outline()
                                .accessibility_label(format!(
                                    "{} · {}",
                                    strings.ai_panel_use_skill, skill_label
                                ))
                                .disabled(disabled || session.settings_saving)
                        },
                        move |editor, window, cx| {
                            if let Some(session) = editor
                                .ai_session
                                .as_mut()
                                .filter(|session| session.saved.id == use_session)
                            {
                                session.input.update(cx, |input, cx| {
                                    input.set_value(&format!("/{id} {}", input.value()), window, cx);
                                    input.focus(window, cx);
                                });
                                session.mode = PanelMode::Task;
                                cx.notify();
                            }
                        },
                    );
                    content = content.child(
                        div()
                            .w_full()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_0()
                            .px_1()
                            .pt_1()
                            .pb_2()
                            .child(
                                crate::ui::scroll_focus_region(
                                    SharedString::from(format!("ai-skill-actions:{}", skill.id)).into(),
                                    &session.scroll,
                                    window,
                                    cx,
                                )
                                // 留出焦点边框的空间，避免滚动入视后仍被裁切。
                                .p_1()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(toggle)
                                .child(apply),
                            )
                            .child(
                                div()
                                    .pl_7()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(skill.description.clone()),
                            ),
                    );
                }
                for warning in &session.skills.warnings {
                    content = content.child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(warning.clone()),
                    );
                }
            }
            PanelMode::Instruction => {
                let mut names = std::collections::HashSet::new();
                for (index, instruction) in session.settings.instructions.iter().enumerate() {
                    let instruction = instruction.clone();
                    let instruction_label = instruction.name.clone();
                    // 手写配置可能同名，重复项使用独立身份空间，删除仍按内容快照定位。
                    let unique_name = names.insert(instruction.name.clone());
                    let edit_id: ElementId = if unique_name {
                        SharedString::from(format!("ai-template-edit:{}", instruction.name)).into()
                    } else {
                        ("ai-template-edit-duplicate", index).into()
                    };
                    let delete_id: ElementId = if unique_name {
                        SharedString::from(format!("ai-template-delete:{}", instruction.name)).into()
                    } else {
                        ("ai-template-delete-duplicate", index).into()
                    };
                    let region_id: ElementId = if unique_name {
                        SharedString::from(format!("ai-template-actions:{}", instruction.name)).into()
                    } else {
                        ("ai-template-actions-duplicate", index).into()
                    };
                    let deleted = instruction.clone();
                    content = content.child(
                        crate::ui::scroll_focus_region(region_id, &session.scroll, window, cx)
                            .p_1()
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(self.ai_button(
                                edit_id,
                                false,
                                cx,
                                |button, _| {
                                    button
                                        .flex_1()
                                        .min_w_0()
                                        .h_auto()
                                        .py_2()
                                        .justify_start()
                                        .accessibility_label(instruction_label.clone())
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w_0()
                                                .text_left()
                                                .truncate()
                                                .child(instruction_label),
                                        )
                                },
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
                                delete_id,
                                false,
                                cx,
                                |button, _cx| {
                                    button
                                        .icon(IconName::Trash)
                                        .accessibility_label(
                                            strings.ai_panel_delete_instruction.clone(),
                                        )
                                        .tooltip(strings.ai_panel_delete_instruction.clone())
                                        .disabled(session.settings_saving || session.settings_readonly())
                                },
                                move |editor, _, cx| {
                                    if let Some(session) = editor.ai_session.as_ref() {
                                        let mut settings = session.settings.clone();
                                        if let Some(index) = settings
                                            .instructions
                                            .iter()
                                            .position(|instruction| instruction == &deleted)
                                        {
                                            settings.instructions.remove(index);
                                            editor.update_ai_settings(settings, cx);
                                        }
                                    }
                                },
                            )),
                    );
                }
                content = content
                    .child(
                        Form::vertical()
                            .p_1()
                            .child(
                                Field::new()
                                    .label(strings.ai_panel_instruction_name.clone())
                                    .child(
                                        Input::new(&session.instruction_name)
                                            .readonly(session.settings_readonly())
                                            .aria_label(strings.ai_panel_instruction_name.clone()),
                                    ),
                            )
                            .child(
                                Field::new()
                                    .label(strings.ai_panel_instruction_body.clone())
                                    .child(
                                        Textarea::new(&session.instruction_body)
                                            .readonly(session.settings_readonly())
                                            .aria_label(strings.ai_panel_instruction_body.clone()),
                                    ),
                            ),
                    )
                    .child(
                        crate::ui::scroll_focus_region(
                            "ai-instruction-save-actions".into(),
                            &session.scroll,
                            window,
                            cx,
                        )
                        .p_1()
                        .flex()
                        .justify_end()
                        .child(self.ai_button(
                            "ai-instruction-save",
                            false,
                            cx,
                            |button, cx| {
                                button
                                    .label(strings.ai_panel_save_instruction.clone())
                                    .primary()
                                    .disabled(
                                        session.settings_saving
                                            || session.settings_readonly()
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
                        )),
                    );
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
            .debug_selector(|| "ai-content".into())
            .w_full()
            .min_h_0()
            .relative()
            .when(native_collection, |body| body.flex().flex_col().flex_1())
            .child(content)
            .when(!native_collection, |body| {
                body.max_h(px((height - 150.0).max(90.0)))
                    .overflow_y_scroll()
                    .track_scroll(&session.scroll)
                    .vertical_scrollbar(&session.scroll)
            });
        let panel = div()
            .id("ai-assistant-panel")
            .track_focus(&session.panel_focus)
            .debug_selector(|| "ai-assistant-panel".into())
            .p_3()
            .flex()
            .flex_col()
            .gap_2()
            .max_h(px(height))
            .min_w_0()
            .bg(theme.colors.dialog_surface)
            .border_1()
            .border_color(theme.colors.dialog_border)
            .text_color(theme.colors.dialog_title)
            .text_sm()
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(header)
            .children(feedback)
            .child(body);

        panel
    }

    fn render_ai_history(&self, height: f32, cx: &mut Context<Self>) -> AnyElement {
        let Some(session) = &self.ai_session else {
            return div().into_any_element();
        };
        let strings = cx.global::<crate::i18n::I18nManager>().strings();
        let groups = [SessionKind::Conversation, SessionKind::QuickCommand].map(|kind| {
            session
                .history_files
                .iter()
                .filter(|(_, _, entry)| *entry == kind)
                .map(|(path, title, _)| (path.clone(), title.clone()))
                .collect::<Vec<_>>()
        });
        let list_id = session.history_list.entity_id();
        let owner = cx.entity().downgrade();
        let confirm_owner = owner.clone();
        let select_owner = owner.clone();
        let empty = strings.ai_history_empty.clone();
        let loading = session.history_loading;
        let mut list = Command::new(&session.history_list)
            .searchable(false)
            .filterable(false)
            .bordered(false)
            .max_h((px(height) - cx.theme().font_size * 4.0).max(px(0.0)));
        for (entries, label) in groups.iter().zip([
            &strings.ai_history_conversations,
            &strings.ai_history_quick_commands,
        ]) {
            list = list.group(
                CommandGroup::new().label(label.clone()).items(
                    entries
                        .iter()
                        .map(|(_, title)| {
                            let title = title.clone();
                            CommandItem::new().label(title.clone()).child(move |_, _| {
                                div().w_full().min_w_0().truncate().child(title.clone())
                            })
                        }),
                ),
            );
        }
        let list = list
            .empty(move |_, _, cx| {
                div()
                    .id("ai-history-empty")
                    .debug_selector(|| "ai-history-empty".into())
                    .p_4()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .when(loading, |body| body.child(Spinner::new().small()))
                    .when(!loading, |body| body.child(empty.clone()))
            })
            .on_select(move |_, _, cx| {
                if let Err(error) = select_owner.update(cx, |editor, cx| {
                    if editor.ai_session.as_ref().is_some_and(|session| {
                        session.mode == PanelMode::History
                            && session.history_list.entity_id() == list_id
                    }) {
                        cx.notify();
                    }
                }) {
                    eprintln!("更新 AI 历史选择失败：{error}");
                }
            })
            .on_confirm(move |index, window, cx| {
                let Some((path, _)) = groups
                    .get(index.section)
                    .and_then(|group| group.get(index.row))
                else {
                    return;
                };
                if let Err(error) = confirm_owner.update(cx, |editor, cx| {
                    if editor.ai_session.as_ref().is_some_and(|session| {
                        session.visible
                            && session.mode == PanelMode::History
                            && session.history_list.entity_id() == list_id
                    }) {
                        editor.restore_ai_history(path.clone(), window, cx);
                    }
                }) {
                    eprintln!("打开 AI 历史失败：{error}");
                }
            });
        div()
            .id("ai-history-list")
            .debug_selector(|| "ai-history-list".into())
            .w_full()
            .min_h_0()
            .child(list)
            .on_prepaint(move |_, window, cx| {
                if let Err(error) = owner.update(cx, |editor, cx| {
                    if let Some(session) = editor.ai_session.as_mut().filter(|session| {
                        session.history_focus_pending
                            && session.visible
                            && session.mode == PanelMode::History
                            && session.history_list.entity_id() == list_id
                    }) {
                        session.history_focus_pending = false;
                        session
                            .history_list
                            .update(cx, |list, cx| list.focus(window, cx));
                        cx.notify();
                    }
                }) {
                    eprintln!("聚焦 AI 历史失败：{error}");
                }
            })
            .into_any_element()
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
