use super::*;
use crate::components::{
    CloseTab, CloseWindow, FileHistory, FindInDocument, FindNextMatch, FindPreviousMatch,
    FormatDocument, OpenCommandPalette, QuickOpen, Redo, SaveDocument, SaveDocumentAs,
    SelectTabIndex, SplitEditorDown, SplitEditorLeft, SplitEditorRight, SplitEditorUp,
    ToggleFocusMode, ToggleFullscreen, ToggleSidebar, ToggleTypewriterMode, ToggleViewMode, Undo,
};
use gpui::prelude::FluentBuilder;
use gpui_kit::component::resizable::{h_resizable, v_resizable, resizable_panel};

pub(super) fn tab_group_accent(theme: &Theme, index: usize) -> Hsla {
    // 黄金角错开色相，避免相邻分组落在近似颜色上。
    let hue = (0.58 + index as f32 * 0.618_034).fract();
    let lightness = if theme.colors.dialog_surface.l < 0.5 {
        0.68
    } else {
        0.46
    };
    hsla(hue, 0.60, lightness, 1.0)
}

impl Editor {
    pub(crate) fn render_split_workspace(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.sync_pending_save(window, cx);
        let Some(active) = self.active_split_editor() else {
            return div().into_any_element();
        };
        let theme = cx.global::<ThemeManager>().derived_theme(cx);
        let strings = cx.global::<I18nManager>().strings_arc();
        let height = crate::window_chrome::custom_titlebar_height(window, &theme.dimensions);
        let (sidebar, status, title, workspace_width) = active.update(cx, |editor, cx| {
            let width =
                editor.current_workspace_panel_width(window, cx);
            let panel = editor.render_workspace_panel(&theme, &strings, window, cx);
            let panel = if editor.workspace.is_open {
                panel
            } else {
                None
            };
            let status = editor.render_status_bar(&theme, &strings, window, cx);
            editor.sync_window_title(window, &strings);
            (panel, status, editor.workspace_breadcrumb(), width)
        });
        let groups = self.split_groups();
        let mut group_tabs = self.render_group_tabs(&theme, window, cx);
        let header_height = if height > 0.0 {
            height
        } else if group_tabs.is_some() {
            36.0
        } else {
            0.0
        };
        let titlebar_tabs = if height > 0.0 {
            group_tabs.take()
        } else {
            None
        };
        window.set_window_edited(groups.iter().any(|group| {
            group.read(cx).document_dirty || group.read(cx).has_dirty_workspace_documents()
        }));
        let document = if let Some(layout) = self.split_workspace.as_ref().map(|workspace| workspace.layout.clone()) {
            let mut index = 0;
            let groups = Self::render_group_node(&layout, &mut index, cx);
            self.render_ai_workspace(groups, &theme, window, cx)
        } else {
            div().into_any_element()
        };
        let main = active.update(cx, |editor, cx| {
            editor.render_workspace_layout(document, sidebar, workspace_width, window, cx)
        });
        let main = self.render_ai_sidebar_layout(main, &theme, window, cx);
        let menus = self.window_menus(cx).filter(|menus| !menus.is_empty());
        let leading = (crate::editor::render::supports_hamburger_menu() && menus.is_some())
            .then(|| self.render_hamburger_menu_button(&theme, cx));
        let mut root = div()
            .key_context("VeloraEditor")
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .bg(theme.colors.editor_background)
            .child(div().flex_1().min_h_0().flex().flex_col()
                .pt(px(header_height)).child(main));
        if let Some(status) = status {
            root = root.child(status);
        }
        if let Some(titlebar) = crate::window_chrome::render_custom_titlebar(
            "editor-titlebar",
            format!("Velora - {title}").into(),
            leading,
            titlebar_tabs,
            &theme,
            window,
            cx,
            Self::on_titlebar_close,
        ) {
            root = root.child(titlebar);
        }
        if let Some(tabs) = group_tabs {
            root = root.child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .h(px(header_height))
                    .flex()
                    .bg(theme.colors.dialog_surface)
                    .child(tabs),
            );
        }
        if let Some(panel) = self.render_ai_assistant(&theme, window, cx) { root = root.child(deferred(panel).with_priority(10)); }
        if let Some(menus) = menus {
            if let Some(menu) = self.render_hamburger_menu_panel(&theme, cx, &menus, height) {
                root = root.child(deferred(menu).with_priority(20));
            }
            if let Some(index) = self.menu_bar_open {
                let origin = crate::editor::render::hamburger_menu_item_panel_origin(
                    index,
                    height,
                    &crate::editor::render::hamburger_menu_labels(&menus, &strings),
                    &theme.dimensions,
                );
                if let Some(panel) = self.render_in_window_menu_panel(
                    &theme,
                    cx,
                    Some(&menus),
                    origin,
                    f32::from(window.viewport_size().height),
                ) {
                    root = root.child(deferred(panel).with_priority(20));
                }
            }
        }
        if let Some(notification) = self.render_update_notification(&theme, &strings, cx) {
            root = root.child(notification);
        }
        if let Some(kind) = self.info_dialog {
            root = root.child(
                deferred(self.render_info_dialog_overlay(&theme, kind, cx)).with_priority(20),
            );
        } else if self.modal_is_open() {
            if let Some(modal) = self.render_modal_overlay(&theme, cx) {
                root = root.child(deferred(modal).with_priority(20));
            }
        } else if self.show_unsaved_changes_dialog {
            root = root
                .child(deferred(self.render_unsaved_changes_overlay(&theme, cx)).with_priority(20));
        }
        root = root
            .on_action(cx.listener(Self::on_open_ai_assistant))
            .on_action(cx.listener(Self::on_toggle_ai_sidebar))
            .on_action(cx.listener(Self::on_registered_menu_action::<crate::components::AiPolish>))
            .on_action(cx.listener(Self::on_registered_menu_action::<crate::components::AiTranslate>))
            .on_action(cx.listener(Self::on_registered_menu_action::<crate::components::AiSummarize>))
            .on_action(cx.listener(Self::on_registered_menu_action::<crate::components::AiExplain>))
            .on_action(cx.listener(Self::on_registered_menu_action::<crate::components::AiExpand>))
            .on_action(cx.listener(Self::on_registered_menu_action::<crate::components::ToggleReadingMode>))
            .on_action(cx.listener(Self::on_dismiss_transient_ui))
            .on_action(cx.listener(Self::forward_split_action::<SplitEditorLeft>))
            .on_action(cx.listener(Self::forward_split_action::<SplitEditorRight>))
            .on_action(cx.listener(Self::forward_split_action::<SplitEditorUp>))
            .on_action(cx.listener(Self::forward_split_action::<SplitEditorDown>))
            .on_action(cx.listener(Self::on_new_document))
            .on_action(cx.listener(Self::forward_split_action::<SaveDocument>))
            .on_action(cx.listener(Self::forward_split_action::<SaveDocumentAs>))
            .on_action(cx.listener(Self::forward_split_action::<CloseTab>))
            .on_action(cx.listener(Self::forward_split_action::<CloseWindow>))
            .on_action(cx.listener(Self::forward_split_action::<ToggleViewMode>))
            .on_action(cx.listener(Self::forward_split_action::<ToggleFocusMode>))
            .on_action(cx.listener(Self::forward_split_action::<ToggleTypewriterMode>))
            .on_action(cx.listener(Self::forward_split_action::<ToggleSidebar>))
            .on_action(cx.listener(Self::forward_split_action::<ToggleFullscreen>))
            .on_action(cx.listener(Self::forward_split_action::<FindInDocument>))
            .on_action(cx.listener(Self::forward_split_action::<FindNextMatch>))
            .on_action(cx.listener(Self::forward_split_action::<FindPreviousMatch>))
            .on_action(cx.listener(Self::forward_split_action::<QuickOpen>))
            .on_action(cx.listener(Self::forward_split_action::<OpenCommandPalette>))
            .on_action(cx.listener(Self::forward_split_action::<SelectTabIndex>))
            .on_action(cx.listener(Self::forward_split_action::<FileHistory>))
            .on_action(cx.listener(Self::forward_split_action::<FormatDocument>))
            .on_action(cx.listener(Self::forward_split_action::<Undo>))
            .on_action(cx.listener(Self::forward_split_action::<Redo>));
        root.into_any_element()
    }

    fn render_group_tabs(
        &mut self,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let groups = self.split_groups();
        let show_group_accents = groups.len() > 1;
        let active_group = self
            .split_workspace
            .as_ref()
            .map(|workspace| workspace.active);
        let mut tabs = Vec::new();
        let mut active_tab = None;
        for (group_index, group) in groups.into_iter().enumerate() {
            let (items, selected) = group.update(cx, |editor, cx| {
                let accent = show_group_accents.then(|| tab_group_accent(theme, group_index));
                let items = editor.render_document_tab_items(theme, accent, cx);
                let selected = editor
                    .workspace
                    .open_documents
                    .iter()
                    .enumerate()
                    .find(|(_, tab)| Some(&tab.path) == editor.workspace.active_document.as_ref())
                    .map(|(index, tab)| (index, tab.path.clone()));
                (items, selected)
            });
            if items.is_empty() {
                continue;
            }
            if !tabs.is_empty() {
                tabs.push(
                    crate::window_chrome::render_titlebar_drag_area(
                        ("tab-group-gap", group_index),
                        window,
                    )
                    .debug_selector(move || format!("tab-group-gap-{group_index}"))
                    .flex_shrink_0()
                    .w(px(12.0))
                    .h_full()
                    .into_any_element(),
                );
            }
            let id = group.entity_id();
            for (index, (path_hash, tab)) in items.into_iter().enumerate() {
                if Some(id) == active_group {
                    if let Some((selected_index, path)) = &selected {
                        if *selected_index == index {
                            active_tab = Some((
                                id,
                                path.clone(),
                                tabs.len(),
                            ));
                        }
                    }
                }
                let focus_group = group.clone();
                tabs.push(
                    div()
                        .id(SharedString::from(format!("titlebar-tab-{id:?}-{path_hash}")))
                        .debug_selector(move || format!("group-tab-{id:?}-{index}"))
                        .h_full()
                        .flex()
                        .flex_shrink_0()
                        .capture_key_down(cx.listener(move |editor, _, _, cx| {
                            if editor.split_workspace.as_ref()
                                .is_some_and(|workspace| workspace.active != id)
                            {
                                editor.activate_split_group(id, cx);
                            }
                        }))
                        .capture_any_mouse_down(cx.listener(move |editor, _, _, cx| {
                            editor.activate_split_group(id, cx);
                            focus_group.update(cx, |editor, cx| {
                                editor.pending_focus = editor
                                    .active_entity_id
                                    .or_else(|| editor.first_focusable_entity_id(cx));
                                cx.notify();
                            });
                        }))
                        .child(tab)
                        .into_any_element(),
                );
            }
        }
        if tabs.is_empty() {
            return None;
        }
        if let Some((group, path, index)) = active_tab {
            self.reveal_document_tab(group, path, index, window, cx);
        }
        Some(
            gpui_kit::base::Tabs::new("window-editor-tabs")
                .debug_selector(|| "window-editor-tabs".into())
                .h_full()
                .min_w(px(0.0))
                .flex()
                .track_scroll(&self.workspace.tabs_scroll_handle)
                .overflow_x_scroll()
                .scrollbar_width(px(0.0))
                .children(tabs)
                .into_any_element(),
        )
    }

    fn render_group_node(
        node: &GroupNode,
        index: &mut usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // 嵌套容器必须保持 flex 与可收缩尺寸，正文才能按分配的视口排版。
        match node {
            GroupNode::Editor(group) => {
                let position = *index;
                *index += 1;
                let id = group.entity_id();
                let measured = group.clone();
                div()
                    .id(SharedString::from(format!("editor-group-{id:?}")))
                    .debug_selector(move || format!("editor-group-{position}"))
                    .size_full()
                    .flex()
                    .min_w(px(0.0))
                    .min_h(px(0.0))
                    .relative()
                    .capture_any_mouse_down(
                        cx.listener(move |editor, _, _, cx| editor.activate_split_group(id, cx)),
                    )
                    .child(
                        canvas(
                            move |bounds, _, cx| {
                                measured.update(cx, |editor, _| editor.split_bounds = bounds);
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .size_full(),
                    )
                    .child(group.clone())
                    .into_any_element()
            }
            GroupNode::Split {
                id,
                vertical,
                ratio,
                bounds,
                view,
                first,
                second,
            } => {
                let (first_width, first_height) = first.minimum_size();
                let (second_width, second_height) = second.minimum_size();
                let (first_minimum, second_minimum, measured_size) = if *vertical {
                    (
                        px(first_height),
                        px(second_height),
                        bounds.get().size.height,
                    )
                } else {
                    (px(first_width), px(second_width), bounds.get().size.width)
                };
                let available = view
                    .borrow()
                    .as_ref()
                    .map(|layout| layout.state.read(cx).container_size())
                    .filter(|size| *size > px(0.0))
                    .unwrap_or_else(|| {
                        if measured_size > px(0.0) {
                            measured_size
                        } else {
                            first_minimum + second_minimum
                        }
                    });
                let preference =
                    SplitPanelPreference::new(*ratio, available, first_minimum, second_minimum);
                let (state, focus) = split_panel_layout(view, *id, preference, cx);
                let measured = bounds.clone();

                let first = Self::render_group_node(first, index, cx);
                let second = Self::render_group_node(second, index, cx);
                let group = if *vertical {
                    v_resizable(SharedString::from(format!("editor-split-{id}")))
                } else {
                    h_resizable(SharedString::from(format!("editor-split-{id}")))
                };
                let group = group
                    .with_state(&state)
                    .with_handle_appearance(
                        crate::ui::PanelResizeHandle {
                            id: "split-divider",
                            label: "Resize editor panes",
                            state: state.clone(),
                            focus,
                            panel_index: 0,
                            edge: gpui_kit::base::HandleEdge::Trailing,
                            minimum: preference.first_minimum,
                            maximum: preference.maximum(),
                            initial: preference.first_size(),
                        }
                        .appearance(),
                    )
                    .child(
                        resizable_panel()
                            .size(preference.first_size())
                            .size_range(preference.first_minimum..preference.maximum())
                            .min_w_0()
                            .min_h_0()
                            .child(first),
                    )
                    .child(
                        resizable_panel()
                            .size(preference.available - preference.first_size())
                            .size_range(
                                preference.second_minimum
                                    ..(preference.available - preference.first_minimum)
                                        .max(preference.second_minimum),
                            )
                            .min_w_0()
                            .min_h_0()
                            .child(second),
                    );
                div()
                    .size_full()
                    .flex()
                    .min_w_0()
                    .min_h_0()
                    .relative()
                    .child(
                        canvas(move |bounds, _, _| measured.set(bounds), |_, _, _, _| {})
                            .absolute()
                            .size_full(),
                    )
                    .child(group)
                    .into_any_element()
            }
        }
    }
}

fn split_panel_layout(
    view: &Rc<RefCell<Option<SplitPanelLayout>>>,
    id: uuid::Uuid,
    preference: SplitPanelPreference,
    cx: &mut Context<Editor>,
) -> (Entity<ResizableState>, FocusHandle) {
    let previous = view.borrow_mut().take();
    let layout = match previous {
        Some(layout) if layout.preference == preference => layout,
        previous => {
            let focus = previous
                .as_ref()
                .map(|layout| layout.focus.clone())
                .unwrap_or_else(|| cx.focus_handle());
            let state = cx.new(|_| ResizableState::default());
            state.update(cx, |state, cx| {
                state.insert_panel(Some(preference.first_size()), None, cx);
                state.insert_panel(
                    Some(preference.available - preference.first_size()),
                    None,
                    cx,
                );
            });
            let observation = cx.observe(&state, |_, _, cx| cx.notify());
            let weak_view = Rc::downgrade(view);
            let subscription = cx.subscribe(&state, move |editor, state, _, cx| {
                let (first, total) = {
                    let state = state.read(cx);
                    (
                        state.sizes().first().copied(),
                        state.sizes().iter().copied().sum::<Pixels>(),
                    )
                };
                if let Some(first) = first
                    && total > px(0.0)
                {
                    let ratio = first / total;
                    if let Some(view) = weak_view.upgrade()
                        && let Some(layout) = view.borrow_mut().as_mut()
                    {
                        layout.preference.ratio = ratio;
                    }
                    if editor
                        .split_workspace
                        .as_mut()
                        .is_some_and(|workspace| workspace.layout.set_ratio(id, ratio))
                    {
                        editor.persist_session(cx);
                        cx.notify();
                    }
                }
            });
            SplitPanelLayout {
                state,
                focus,
                preference,
                _observation: observation,
                _subscription: subscription,
            }
        }
    };
    let result = (layout.state.clone(), layout.focus.clone());
    *view.borrow_mut() = Some(layout);
    result
}

impl Editor {
    fn forward_split_action<A: Action>(
        &mut self,
        action: &A,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.forward_split_action_value(action, window, cx);
    }

    fn forward_split_action_value(
        &mut self,
        action: &dyn Action,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        if self.modal_is_open() || self.show_unsaved_changes_dialog || self.info_dialog.is_some() {
            return;
        }
        let Some(group) = self.active_split_editor().map(|group| group.downgrade()) else {
            return;
        };
        let action = action.boxed_clone();
        let handle = window.window_handle();
        // 侧栏位于分组视图之外；离开根实体的更新栈再路由，避免子组读取根时重入。
        cx.defer(move |cx| {
            if let Err(error) = handle.update(cx, |_, window, cx| {
                let handled = group.update(cx, |editor, cx| {
                    if action.as_any().is::<QuickOpen>() {
                        editor.on_quick_open_action(&QuickOpen, window, cx);
                    } else if action.as_any().is::<OpenCommandPalette>() {
                        editor.on_open_command_palette(&OpenCommandPalette, window, cx);
                    } else if action.as_any().is::<Undo>() {
                        editor.on_undo(&Undo, window, cx);
                    } else if action.as_any().is::<Redo>() {
                        editor.on_redo(&Redo, window, cx);
                    } else if let Some(action) = action.as_any().downcast_ref::<SelectTabIndex>() {
                        editor.on_select_tab_index(action, window, cx);
                    } else {
                        return false;
                    }
                    true
                });
                match handled {
                    Ok(false) => crate::app_menu::dispatch_menu_action_for_editor(
                        action.as_ref(),
                        &group,
                        window,
                        cx,
                    ),
                    Ok(true) => {}
                    Err(error) => eprintln!("向活动分屏派发动作失败：{error}"),
                }
            }) {
                eprintln!("访问分屏动作窗口失败：{error}");
            }
        });
    }

    pub(crate) fn pane_overlay(&self, element: AnyElement, window: &Window) -> AnyElement {
        if self.split_parent.is_none() {
            return element;
        }
        let origin = self.split_bounds.origin;
        deferred(
            div()
                .absolute()
                .left(-origin.x)
                .top(-origin.y)
                .w(window.viewport_size().width)
                .h(window.viewport_size().height)
                .child(element),
        )
        .with_priority(10)
        .into_any_element()
    }

    pub(crate) fn render_split_drop_targets(
        &mut self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if !cx.has_active_drag() {
            self.split_drop_preview = None;
        }
        let mut targets = div()
            .id("split-drop-targets")
            .absolute()
            .inset_0()
            .on_drag_move::<EditorFileDrag>(cx.listener(
                |editor, event: &DragMoveEvent<EditorFileDrag>, _, cx| {
                    let bounds = event.bounds;
                    let point = event.event.position;
                    let next = bounds.contains(&point).then(|| {
                        let horizontal = f32::from(point.x - bounds.left())
                            / f32::from(bounds.size.width.max(px(1.0)));
                        let vertical = f32::from(point.y - bounds.top())
                            / f32::from(bounds.size.height.max(px(1.0)));
                        if horizontal < 0.25 {
                            Some(SplitDirection::Left)
                        } else if horizontal > 0.75 {
                            Some(SplitDirection::Right)
                        } else if vertical < 0.25 {
                            Some(SplitDirection::Up)
                        } else if vertical > 0.75 {
                            Some(SplitDirection::Down)
                        } else {
                            None
                        }
                    });
                    let next = next.map(|direction| (direction, event.drag(cx).source.is_none()));
                    if next != editor.split_drop_preview {
                        editor.split_drop_preview = next;
                        cx.notify();
                    }
                },
            ));
        for (index, direction) in [
            Some(SplitDirection::Left),
            Some(SplitDirection::Right),
            Some(SplitDirection::Up),
            Some(SplitDirection::Down),
            None,
        ]
        .into_iter()
        .enumerate()
        {
            targets = targets.child(
                div()
                    .id(("split-drop", index))
                    .debug_selector(move || format!("split-drop-{index}"))
                    .absolute()
                    .when(direction == Some(SplitDirection::Left), |this| {
                        this.left_0().top_0().bottom_0().w(relative(0.25))
                    })
                    .when(direction == Some(SplitDirection::Right), |this| {
                        this.right_0().top_0().bottom_0().w(relative(0.25))
                    })
                    .when(direction == Some(SplitDirection::Up), |this| {
                        this.left(relative(0.25))
                            .top_0()
                            .w(relative(0.5))
                            .h(relative(0.25))
                    })
                    .when(direction == Some(SplitDirection::Down), |this| {
                        this.left(relative(0.25))
                            .bottom_0()
                            .w(relative(0.5))
                            .h(relative(0.25))
                    })
                    .when(direction.is_none(), |this| {
                        this.left(relative(0.25))
                            .top(relative(0.25))
                            .w(relative(0.5))
                            .h(relative(0.5))
                    })
                    .on_drop(
                        cx.listener(move |editor, drag: &EditorFileDrag, window, cx| {
                            editor.split_drop_preview = None;
                            editor.receive_editor_file_drop(drag, None, direction, window, cx);
                            cx.notify();
                        }),
                    ),
            );
        }
        if let Some((direction, from_tree)) = self.split_drop_preview {
            let strings = cx.global::<I18nManager>().strings();
            let allowed =
                direction.is_none_or(|direction| direction.fits(self.scroll_handle.bounds().size));
            let label = if allowed {
                direction
                    .map(|direction| direction.label(strings))
                    .unwrap_or_else(|| {
                        if from_tree {
                            strings.split_open_here.clone()
                        } else {
                            strings.split_move_here.clone()
                        }
                    })
            } else {
                strings.split_insufficient_space.clone()
            };
            let name = match direction {
                Some(SplitDirection::Left) => "left",
                Some(SplitDirection::Right) => "right",
                Some(SplitDirection::Up) => "up",
                Some(SplitDirection::Down) => "down",
                None => "center",
            };
            let mut color = theme.colors.dialog_primary_button_bg;
            color.a = 0.16;
            targets = targets.child(
                div()
                    .absolute()
                    .debug_selector(move || format!("split-preview-{name}"))
                    .top_0()
                    .left_0()
                    .size_full()
                    .when(direction == Some(SplitDirection::Left), |this| {
                        this.w(relative(0.5))
                    })
                    .when(direction == Some(SplitDirection::Right), |this| {
                        this.left(relative(0.5)).w(relative(0.5))
                    })
                    .when(direction == Some(SplitDirection::Up), |this| {
                        this.h(relative(0.5))
                    })
                    .when(direction == Some(SplitDirection::Down), |this| {
                        this.top(relative(0.5)).h(relative(0.5))
                    })
                    .bg(color)
                    .border(px(2.0))
                    .border_color(theme.colors.dialog_primary_button_bg)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .debug_selector(|| "split-preview-label".into())
                            .px(px(12.0))
                            .py(px(8.0))
                            .rounded(px(6.0))
                            .bg(theme.colors.dialog_surface)
                            .text_color(theme.colors.text_default)
                            .text_size(px(13.0))
                            .child(label),
                    ),
            );
        }
        targets.into_any_element()
    }
}
