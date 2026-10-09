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
            target: self.workspace.selected.clone(),
        });
        cx.notify();
    }

    pub(crate) fn on_workspace_background_right_click(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_workspace_context_menu(event.position, None, cx);
        self.focus_workspace_tree(window, cx);
        cx.stop_propagation();
    }

    pub(crate) fn render_workspace_context_menu_overlay(
        &self,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let menu = self.workspace.context_menu.as_ref()?;
        let strings = cx.global::<crate::i18n::I18nManager>().strings();
        let mut actions = vec![
            (strings.workspace_new_generic_file.clone(), WorkspaceMenuAction::NewFile),
            (strings.workspace_new_file.clone(), WorkspaceMenuAction::NewMarkdown),
            (strings.workspace_new_folder.clone(), WorkspaceMenuAction::NewFolder),
            (strings.workspace_reveal_in_file_manager.clone(), WorkspaceMenuAction::Reveal),
            (strings.workspace_copy_absolute_path.clone(), WorkspaceMenuAction::CopyAbsolutePath),
            (strings.workspace_copy_relative_path.clone(), WorkspaceMenuAction::CopyRelativePath),
            (strings.workspace_copy_file_name.clone(), WorkspaceMenuAction::CopyFileName),
        ];
        let is_file = matches!(menu.target, Some(WorkspaceSelection::File(_)));
        if menu.has_target && is_file {
            actions.push((strings.workspace_copy.clone(), WorkspaceMenuAction::Copy));
        }
        actions.push((strings.workspace_paste.clone(), WorkspaceMenuAction::Paste));
        if menu.has_target {
            actions.push((strings.workspace_rename.clone(), WorkspaceMenuAction::Rename));
            if is_file {
                actions.push((strings.workspace_duplicate.clone(), WorkspaceMenuAction::Duplicate));
            }
            actions.push((strings.workspace_delete.clone(), WorkspaceMenuAction::Delete));
        }
        let width = theme.dimensions.menu_panel_width.max(220.0);
        let dimensions = &theme.dimensions;
        let height = actions.len() as f32
            * (dimensions.menu_item_height + dimensions.menu_panel_gap)
            + dimensions.menu_panel_padding * 2.0
            + 3.0 * (dimensions.menu_separator_height + dimensions.menu_separator_margin_y * 2.0);
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
                let selection = menu.target.clone();
                let separator = matches!(action, WorkspaceMenuAction::Reveal | WorkspaceMenuAction::Copy | WorkspaceMenuAction::Rename)
                    || matches!(action, WorkspaceMenuAction::Paste) && !is_file;
                let shortcut = matches!(action, WorkspaceMenuAction::Rename).then(|| SharedString::from("F2"));
                let row = crate::components::menu::menu_item(
                    theme,
                    format!("workspace-context-action-{index}"),
                    label,
                    shortcut,
                    true,
                    matches!(action, WorkspaceMenuAction::Delete),
                    false,
                    false,
                    None,
                )
                .on_click(move |_, window, cx| {
                        if let Err(error) = editor.update(cx, |editor, cx| {
                            editor.workspace.context_menu = None;
                            editor.workspace.selected = selection.clone();
                            match action {
                                WorkspaceMenuAction::NewFile => editor.create_generic_workspace_file(window, cx),
                                WorkspaceMenuAction::NewMarkdown => editor.prompt_create_workspace_file(window, cx),
                                WorkspaceMenuAction::NewFolder => {
                                    editor.prompt_create_workspace_folder(window, cx)
                                }
                                WorkspaceMenuAction::Rename => {
                                    editor.prompt_rename_selected(window, cx)
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
                                WorkspaceMenuAction::Reveal => editor.reveal_selected_workspace_path(cx),
                                WorkspaceMenuAction::CopyAbsolutePath | WorkspaceMenuAction::CopyRelativePath | WorkspaceMenuAction::CopyFileName => editor.copy_workspace_path_text(action, cx),
                            }
                            cx.notify();
                        }) { eprintln!("执行文件树菜单操作失败：{error}"); }
                        cx.stop_propagation();
                    });
                div().w_full().children(separator.then(|| crate::components::menu::menu_separator(theme)))
                    .child(row).into_any_element()
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
                // 遮罩要挡住底下的命中：没有 occlude，菜单底下的树行在鼠标压在
                // 菜单上时仍然算悬停（亮起来），点击也会穿到底下的行。
                .occlude()
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = close_editor
                        .update(cx, |editor, cx| editor.close_workspace_context_menu(cx));
                })
                .child(
                    div()
                        .id("workspace-context-panel")
                        .debug_selector(|| "workspace-context-panel".to_string())
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
                        .occlude()
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
        let dimensions = &theme.dimensions;
        let height = actions.len() as f32
            * (dimensions.menu_item_height + dimensions.menu_panel_gap)
            + dimensions.menu_panel_padding * 2.0;
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
                crate::components::menu::menu_item(
                    theme,
                    format!("tab-context-action-{index}"),
                    label,
                    None,
                    true,
                    false,
                    false,
                    false,
                    None,
                )
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
                // 同工作区菜单：遮罩不 occlude，菜单底下那一行的悬停与点击都会漏过去。
                .occlude()
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
                        .debug_selector(|| "tab-context-panel".to_string())
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
                        .occlude()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .children(rows),
                )
                .into_any_element(),
        )
    }

}
