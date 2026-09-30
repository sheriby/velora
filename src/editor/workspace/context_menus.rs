use super::*;

impl Editor {
    pub(crate) fn selected_workspace_path(&self) -> Option<PathBuf> {
        match self.workspace.selected.as_ref()? {
            WorkspaceSelection::Directory(path)
            | WorkspaceSelection::File(path)
            | WorkspaceSelection::WorkspaceRoot(path) => Some(path.clone()),
            WorkspaceSelection::Outline(_) => None,
        }
    }

    pub(crate) fn close_workspace_context_menu(&mut self, cx: &mut Context<Self>) {
        if self.workspace.context_menu.take().is_some() {
            cx.notify();
        }
    }

    pub(crate) fn open_workspace_context_menu(
        &mut self,
        position: Point<Pixels>,
        selection: Option<WorkspaceSelection>,
        cx: &mut Context<Self>,
    ) {
        self.dismiss_contextual_overlays(cx);
        let has_target = selection.as_ref().is_some_and(|selection| match selection {
            WorkspaceSelection::File(path) | WorkspaceSelection::Directory(path) => {
                self.workspace.root.as_ref() != Some(path)
            }
            _ => false,
        });
        self.workspace.selected = selection.or_else(|| {
            self.workspace
                .root
                .clone()
                .map(WorkspaceSelection::WorkspaceRoot)
        });
        self.workspace.context_menu = Some(WorkspaceContextMenu {
            position,
            has_target,
        });
        cx.notify();
    }

    pub(crate) fn on_workspace_background_right_click(
        &mut self,
        event: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_workspace_context_menu(event.position, None, cx);
        cx.stop_propagation();
    }

    pub(crate) fn render_workspace_context_menu_overlay(
        &self,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let menu = self.workspace.context_menu?;
        let strings = cx.global::<crate::i18n::I18nManager>().strings();
        let mut actions = vec![
            (
                strings.workspace_new_file.clone(),
                WorkspaceMenuAction::NewFile,
            ),
            (
                strings.workspace_new_folder.clone(),
                WorkspaceMenuAction::NewFolder,
            ),
        ];
        if menu.has_target {
            actions.push((
                strings.workspace_rename.clone(),
                WorkspaceMenuAction::Rename,
            ));
            if let Some(WorkspaceSelection::File(path)) = self.workspace.selected.as_ref() {
                if !path.is_dir() {
                    actions.push((
                        strings.workspace_duplicate.clone(),
                        WorkspaceMenuAction::Duplicate,
                    ));
                    actions.push((
                        strings.workspace_copy.clone(),
                        WorkspaceMenuAction::Copy,
                    ));
                }
            }
            actions.push((
                strings.workspace_delete.clone(),
                WorkspaceMenuAction::Delete,
            ));
        }
        actions.push((
            strings.workspace_paste.clone(),
            WorkspaceMenuAction::Paste,
        ));
        let width = 180.0;
        let height = actions.len() as f32 * 32.0 + 8.0;
        let viewport = window.viewport_size();
        let left = f32::from(menu.position.x)
            .min((f32::from(viewport.width) - width - 8.0).max(8.0))
            .max(8.0);
        let top = f32::from(menu.position.y)
            .min((f32::from(viewport.height) - height - 8.0).max(8.0))
            .max(8.0);
        let editor = cx.entity().downgrade();
        let rows = actions
            .into_iter()
            .enumerate()
            .map(|(index, (label, action))| {
                let editor = editor.clone();
                div()
                    .id(("workspace-context-action", index))
                    .h(px(32.0))
                    .px(px(10.0))
                    .flex()
                    .items_center()
                    .rounded(px(5.0))
                    .cursor_pointer()
                    .text_size(px(12.0))
                    .text_color(if matches!(action, WorkspaceMenuAction::Delete) {
                        theme.colors.dialog_danger_button_bg
                    } else {
                        theme.colors.dialog_body
                    })
                    .hover(|this| this.bg(theme.colors.dialog_secondary_button_hover))
                    .child(label)
                    .on_click(move |_, window, cx| {
                        let _ = editor.update(cx, |editor, cx| {
                            editor.workspace.context_menu = None;
                            match action {
                                WorkspaceMenuAction::NewFile => {
                                    editor.prompt_create_workspace_file(window, cx)
                                }
                                WorkspaceMenuAction::NewFolder => {
                                    editor.prompt_create_workspace_folder(window, cx)
                                }
                                WorkspaceMenuAction::Rename => {
                                    editor.prompt_rename_or_move_selected(window, cx)
                                }
                                WorkspaceMenuAction::Duplicate => {
                                    editor.duplicate_selected_file(window, cx)
                                }
                                WorkspaceMenuAction::Copy => {
                                    editor.copy_selected_workspace_file(cx)
                                }
                                WorkspaceMenuAction::Paste => {
                                    editor.paste_into_workspace_tree(cx)
                                }
                                WorkspaceMenuAction::Delete => {
                                    editor.prompt_delete_selected(window, cx)
                                }
                            }
                            cx.notify();
                        });
                        cx.stop_propagation();
                    })
            })
            .collect::<Vec<_>>();
        let close_editor = editor;
        Some(
            div()
                .id("workspace-context-overlay")
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .bottom_0()
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = close_editor
                        .update(cx, |editor, cx| editor.close_workspace_context_menu(cx));
                })
                .child(
                    div()
                        .id("workspace-context-panel")
                        .absolute()
                        .left(px(left))
                        .top(px(top))
                        .w(px(width))
                        .p(px(4.0))
                        .rounded(px(8.0))
                        .border_1()
                        .border_color(theme.colors.dialog_border)
                        .bg(theme.colors.dialog_surface)
                        .shadow_md()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .children(rows),
                )
                .into_any_element(),
        )
    }
    pub(crate) fn open_tab_context_menu(
        &mut self,
        position: Point<Pixels>,
        target_index: usize,
        cx: &mut Context<Self>,
    ) {
        self.dismiss_contextual_overlays(cx);
        self.workspace.tab_context_menu = Some(TabContextMenu {
            position,
            target_index,
        });
        cx.notify();
    }

    pub(crate) fn close_tab_context_menu(&mut self, cx: &mut Context<Self>) {
        if self.workspace.tab_context_menu.take().is_some() {
            cx.notify();
        }
    }

    pub(crate) fn render_tab_context_menu_overlay(
        &self,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let menu = self.workspace.tab_context_menu?;
        let target = self.workspace.open_documents.get(menu.target_index)?;
        let target_path = target.path.clone();
        let count = self.workspace.open_documents.len();
        let strings = cx.global::<crate::i18n::I18nManager>().strings();
        let mut actions = vec![(strings.tab_close.clone(), TabMenuAction::Close)];
        if count > 1 {
            actions.push((strings.tab_close_others.clone(), TabMenuAction::CloseOthers));
            if menu.target_index > 0 {
                actions.push((strings.tab_close_left.clone(), TabMenuAction::CloseLeft));
            }
            if menu.target_index + 1 < count {
                actions.push((strings.tab_close_right.clone(), TabMenuAction::CloseRight));
            }
            actions.push((strings.tab_close_all.clone(), TabMenuAction::CloseAll));
        }

        let width = 200.0;
        let height = actions.len() as f32 * 32.0 + 8.0;
        let viewport = window.viewport_size();
        let left = f32::from(menu.position.x)
            .min((f32::from(viewport.width) - width - 8.0).max(8.0))
            .max(8.0);
        let top = f32::from(menu.position.y)
            .min((f32::from(viewport.height) - height - 8.0).max(8.0))
            .max(8.0);
        let editor = cx.entity().downgrade();
        let rows = actions
            .into_iter()
            .enumerate()
            .map(|(index, (label, action))| {
                let editor = editor.clone();
                let target_path = target_path.clone();
                div()
                    .id(("tab-context-action", index))
                    .h(px(32.0))
                    .px(px(10.0))
                    .flex()
                    .items_center()
                    .rounded(px(5.0))
                    .cursor_pointer()
                    .text_size(px(12.0))
                    .text_color(theme.colors.dialog_body)
                    .hover(|this| this.bg(theme.colors.dialog_secondary_button_hover))
                    .child(label)
                    .on_click(move |_, window, cx| {
                        let _ = editor.update(cx, |editor, cx| {
                            editor.workspace.tab_context_menu = None;
                            editor.close_workspace_tabs_for_action(&target_path, action, window, cx);
                            cx.notify();
                        });
                        cx.stop_propagation();
                    })
            })
            .collect::<Vec<_>>();
        let close_editor_left = editor.clone();
        let close_editor_right = editor.clone();
        Some(
            div()
                .id("tab-context-overlay")
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .bottom_0()
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = close_editor_left
                        .update(cx, |editor, cx| editor.close_tab_context_menu(cx));
                })
                .on_mouse_down(MouseButton::Right, move |_, _, cx| {
                    let _ = close_editor_right
                        .update(cx, |editor, cx| editor.close_tab_context_menu(cx));
                })
                .child(
                    div()
                        .id("tab-context-panel")
                        .absolute()
                        .left(px(left))
                        .top(px(top))
                        .w(px(width))
                        .p(px(4.0))
                        .rounded(px(8.0))
                        .border_1()
                        .border_color(theme.colors.dialog_border)
                        .bg(theme.colors.dialog_surface)
                        .shadow_md()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .children(rows),
                )
                .into_any_element(),
        )
    }

}
