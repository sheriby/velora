use super::*;
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Icon,
    menu::{PopupMenu, PopupMenuItem},
};

impl WorkspaceMenuAction {
    pub(super) fn label(self, strings: &crate::i18n::I18nStrings) -> &str {
        match self {
            Self::NewFile => &strings.workspace_new_generic_file,
            Self::NewMarkdown => &strings.workspace_new_file,
            Self::NewFolder => &strings.workspace_new_folder,
            Self::Duplicate => &strings.workspace_duplicate,
            Self::Copy => &strings.workspace_copy,
            Self::Paste => &strings.workspace_paste,
            Self::Rename => &strings.workspace_rename,
            Self::Delete => &strings.workspace_delete,
            Self::Reveal => &strings.workspace_reveal_in_file_manager,
            Self::CopyAbsolutePath => &strings.workspace_copy_absolute_path,
            Self::CopyRelativePath => &strings.workspace_copy_relative_path,
            Self::CopyFileName => &strings.workspace_copy_file_name,
        }
    }
}

impl Editor {
    pub(crate) fn dismiss_context_menus_for_modal(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.context_menu.is_none()
            && self.workspace.tab_context_menu.is_none()
            && self.context_menu.is_none()
        {
            return;
        }
        if !self.modal_is_open()
            && self.info_dialog.is_none()
            && !self.show_drop_replace_dialog
            && !self.show_unsaved_changes_dialog
            && self.pending_folder_choice.is_none()
        {
            return;
        }
        let tree_has_focus = self
            .workspace
            .context_menu
            .as_ref()
            .and_then(|request| request.popup.as_ref())
            .is_some_and(|popup| popup.view.focus_handle(cx).is_focused(window));
        let tabs_have_focus = self
            .workspace
            .tab_context_menu
            .as_ref()
            .and_then(|request| request.popup.as_ref())
            .is_some_and(|popup| popup.view.focus_handle(cx).is_focused(window));
        if tree_has_focus {
            self.focus_workspace_tree(window, cx);
        } else if tabs_have_focus && let Some(focus) = &self.workspace.tabs_focus {
            window.focus(focus, cx);
        }
        // Kit 菜单延迟绘制在上层，模态决策出现时必须先退场。
        self.close_workspace_context_menu(cx);
        self.close_tab_context_menu(cx);
        self.close_document_menu_for_modal(window, cx);
    }

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
        // 剪贴板可能包含大图片，仅在打开时计算一次可用状态。
        let can_paste = matches!(
            self.workspace.selected,
            Some(WorkspaceSelection::Directory(_) | WorkspaceSelection::WorkspaceRoot(_))
        ) && self.workspace_paste_source(cx).is_some();
        self.workspace.context_menu = Some(WorkspaceContextMenu {
            position,
            has_target,
            target: self.workspace.selected.clone(),
            can_paste,
            popup: None,
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
        &mut self,
        _theme: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if self.workspace.context_menu.as_ref()?.popup.is_none() {
            let request = self.workspace.context_menu.as_ref()?;
            let target = request.target.clone();
            let is_file = matches!(target, Some(WorkspaceSelection::File(_)));
            let mut actions = Vec::new();
            if !is_file {
                actions.extend([
                    WorkspaceMenuAction::NewFile,
                    WorkspaceMenuAction::NewMarkdown,
                    WorkspaceMenuAction::NewFolder,
                ]);
            }
            if matches!(
                target,
                Some(WorkspaceSelection::File(_) | WorkspaceSelection::Directory(_))
            ) {
                actions.extend([
                    WorkspaceMenuAction::Reveal,
                    WorkspaceMenuAction::CopyAbsolutePath,
                    WorkspaceMenuAction::CopyRelativePath,
                    WorkspaceMenuAction::CopyFileName,
                ]);
            }
            if request.has_target && is_file {
                actions.push(WorkspaceMenuAction::Copy);
            }
            if request.can_paste {
                actions.push(WorkspaceMenuAction::Paste);
            }
            if request.has_target {
                actions.push(WorkspaceMenuAction::Rename);
                if is_file {
                    actions.push(WorkspaceMenuAction::Duplicate);
                }
                actions.push(WorkspaceMenuAction::Delete);
            }
            let focus = self
                .workspace
                .tree_focus
                .get_or_insert_with(|| cx.focus_handle())
                .clone();
            let strings = cx.global::<crate::i18n::I18nManager>().strings_arc();
            let owner = cx.entity().downgrade();
            let popup = PopupMenu::build(window, cx, move |mut menu, window, cx| {
                menu = menu
                    .action_context(focus)
                    .min_w(window.rem_size() * 13.75)
                    .scrollable(true);
                for (index, action) in actions.into_iter().enumerate() {
                    if index > 0
                        && (matches!(
                            action,
                            WorkspaceMenuAction::Reveal
                                | WorkspaceMenuAction::Copy
                                | WorkspaceMenuAction::Rename
                        ) || matches!(action, WorkspaceMenuAction::Paste) && !is_file)
                    {
                        menu = menu.separator();
                    }
                    let selection = target.clone();
                    let editor = owner.clone();
                    let mut item = PopupMenuItem::new(action.label(&strings).to_owned());
                    if matches!(action, WorkspaceMenuAction::Rename) {
                        item = item.action(Box::new(RenameWorkspaceEntry));
                    }
                    if matches!(action, WorkspaceMenuAction::Delete) {
                        item = item.icon(Icon::new(IconName::Trash).text_color(cx.theme().danger));
                    }
                    menu = menu.item(item.on_click(move |_, window, cx| {
                        if let Err(error) = editor.update(cx, |editor, cx| {
                            editor.workspace.context_menu = None;
                            editor.workspace.selected = selection.clone();
                            editor.execute_workspace_menu_action(action, window, cx);
                            cx.notify();
                        }) {
                            eprintln!("执行文件树菜单操作失败：{error}");
                        }
                    }));
                }
                menu
            });
            let dismiss = cx.subscribe(&popup, |editor, menu, _: &DismissEvent, cx| {
                // 同次右键可以打开另一菜单，旧实例的迟到关闭不能清掉新实例。
                if editor
                    .workspace
                    .context_menu
                    .as_ref()
                    .and_then(|request| request.popup.as_ref())
                    .is_some_and(|popup| popup.view == menu)
                {
                    editor.close_workspace_context_menu(cx);
                }
            });
            window.focus(&popup.focus_handle(cx), cx);
            self.workspace.context_menu.as_mut()?.popup = Some(WorkspacePopupMenu {
                view: popup,
                _dismiss: dismiss,
            });
        }
        let request = self.workspace.context_menu.as_ref()?;
        Some(crate::components::menu::popup_menu_layer(
            "workspace-context-panel",
            request.position,
            &request.popup.as_ref()?.view,
            window,
        ))
    }

    fn execute_workspace_menu_action(
        &mut self,
        action: WorkspaceMenuAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            WorkspaceMenuAction::NewFile => self.create_generic_workspace_file(window, cx),
            WorkspaceMenuAction::NewMarkdown => self.prompt_create_workspace_file(window, cx),
            WorkspaceMenuAction::NewFolder => self.prompt_create_workspace_folder(window, cx),
            WorkspaceMenuAction::Rename => self.prompt_rename_selected(window, cx),
            WorkspaceMenuAction::Duplicate => self.duplicate_selected_file(window, cx),
            WorkspaceMenuAction::Copy => self.copy_selected_workspace_file(cx),
            WorkspaceMenuAction::Paste => self.paste_into_workspace_tree(cx),
            WorkspaceMenuAction::Delete => self.prompt_delete_selected(window, cx),
            WorkspaceMenuAction::Reveal => self.reveal_selected_workspace_path(cx),
            WorkspaceMenuAction::CopyAbsolutePath
            | WorkspaceMenuAction::CopyRelativePath
            | WorkspaceMenuAction::CopyFileName => self.copy_workspace_path_text(action, cx),
        }
    }

    pub(crate) fn on_workspace_rename_action(
        &mut self,
        _: &RenameWorkspaceEntry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.active_tab == WorkspaceTab::Files && self.workspace.name_edit.is_none() {
            self.prompt_rename_selected(window, cx);
        }
    }

    pub(crate) fn open_tab_context_menu(
        &mut self,
        position: Point<Pixels>,
        target_index: usize,
        cx: &mut Context<Self>,
    ) {
        let Some(target_path) = self
            .workspace
            .open_documents
            .get(target_index)
            .map(|tab| tab.path.clone())
        else {
            return;
        };
        self.dismiss_contextual_overlays(cx);
        self.workspace.tab_context_menu = Some(TabContextMenu {
            position,
            target_path,
            popup: None,
        });
        cx.notify();
    }

    pub(crate) fn close_tab_context_menu(&mut self, cx: &mut Context<Self>) {
        if self.workspace.tab_context_menu.take().is_some() {
            cx.notify();
        }
    }

    pub(crate) fn render_tab_context_menu_overlay(
        &mut self,
        _theme: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if self.workspace.tab_context_menu.as_ref()?.popup.is_none() {
            let request = self.workspace.tab_context_menu.as_ref()?;
            let target_path = request.target_path.clone();
            let index = self
                .workspace
                .open_documents
                .iter()
                .position(|tab| tab.path == target_path)?;
            let count = self.workspace.open_documents.len();
            let strings = cx.global::<crate::i18n::I18nManager>().strings_arc();
            let mut actions = vec![(strings.tab_close.clone(), TabMenuAction::Close)];
            if count > 1 {
                actions.push((strings.tab_close_others.clone(), TabMenuAction::CloseOthers));
                if index > 0 {
                    actions.push((strings.tab_close_left.clone(), TabMenuAction::CloseLeft));
                }
                if index + 1 < count {
                    actions.push((strings.tab_close_right.clone(), TabMenuAction::CloseRight));
                }
                actions.push((strings.tab_close_all.clone(), TabMenuAction::CloseAll));
            }
            for direction in [
                split::SplitDirection::Left,
                split::SplitDirection::Right,
                split::SplitDirection::Up,
                split::SplitDirection::Down,
            ] {
                actions.push((direction.label(&strings), TabMenuAction::Split(direction)));
            }
            let focus = self
                .workspace
                .tabs_focus
                .clone()
                .or_else(|| window.focused(cx))
                .unwrap_or_else(|| cx.focus_handle());
            let owner = cx.entity().downgrade();
            let popup = PopupMenu::build(window, cx, move |mut menu, window, _| {
                menu = menu
                    .action_context(focus)
                    .min_w(window.rem_size() * 13.75)
                    .scrollable(true);
                for (label, action) in actions {
                    if matches!(action, TabMenuAction::Split(split::SplitDirection::Left)) {
                        menu = menu.separator();
                    }
                    let path = target_path.clone();
                    let editor = owner.clone();
                    menu = menu.item(PopupMenuItem::new(label).on_click(move |_, window, cx| {
                        if let Err(error) = editor.update(cx, |editor, cx| {
                            editor.workspace.tab_context_menu = None;
                            editor.close_workspace_tabs_for_action(&path, action, window, cx);
                            cx.notify();
                        }) {
                            eprintln!("执行标签菜单操作失败：{error}");
                        }
                    }));
                }
                menu
            });
            let dismiss = cx.subscribe(&popup, |editor, menu, _: &DismissEvent, cx| {
                if editor
                    .workspace
                    .tab_context_menu
                    .as_ref()
                    .and_then(|request| request.popup.as_ref())
                    .is_some_and(|popup| popup.view == menu)
                {
                    editor.close_tab_context_menu(cx);
                }
            });
            window.focus(&popup.focus_handle(cx), cx);
            self.workspace.tab_context_menu.as_mut()?.popup = Some(WorkspacePopupMenu {
                view: popup,
                _dismiss: dismiss,
            });
        }
        let request = self.workspace.tab_context_menu.as_ref()?;
        Some(crate::components::menu::popup_menu_layer(
            "tab-context-panel",
            request.position,
            &request.popup.as_ref()?.view,
            window,
        ))
    }
}
