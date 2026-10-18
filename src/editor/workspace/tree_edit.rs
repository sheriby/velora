use super::*;

impl Editor {
    pub(crate) fn focus_workspace_tree(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let focus = self
            .workspace
            .tree_focus
            .get_or_insert_with(|| cx.focus_handle());
        self.pending_focus = None;
        window.focus(focus, cx);
    }

    pub(crate) fn prompt_create_workspace_file(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.begin_workspace_name_edit(WorkspaceEditKind::File, "untitled.md".into(), window, cx);
    }

    pub(crate) fn create_generic_workspace_file(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.begin_workspace_name_edit(WorkspaceEditKind::File, String::new(), window, cx);
    }

    pub(crate) fn prompt_create_workspace_folder(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = cx
            .global::<I18nManager>()
            .strings()
            .workspace_new_folder
            .clone();
        self.begin_workspace_name_edit(WorkspaceEditKind::Folder, name, window, cx);
    }

    pub(crate) fn prompt_rename_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(source) = self.selected_workspace_path() else {
            return;
        };
        if self.workspace.root.as_ref() == Some(&source) {
            return;
        }
        let name = source
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let is_directory = source.is_dir();
        self.begin_workspace_name_edit(
            WorkspaceEditKind::Rename {
                source,
                is_directory,
            },
            name,
            window,
            cx,
        );
    }

    fn begin_workspace_name_edit(
        &mut self,
        kind: WorkspaceEditKind,
        draft: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .workspace
            .name_edit
            .as_ref()
            .is_some_and(|edit| edit.pending)
        {
            return;
        }
        let directory = match &kind {
            WorkspaceEditKind::Rename { source, .. } => source.parent().map(Path::to_path_buf),
            _ => self.selected_workspace_directory(),
        };
        let Some(directory) = directory else {
            return;
        };
        let select_extension = matches!(
            kind,
            WorkspaceEditKind::Folder
                | WorkspaceEditKind::Rename {
                    is_directory: true,
                    ..
                }
        );
        let selected_end = if select_extension {
            draft.len()
        } else {
            Path::new(&draft)
                .file_stem()
                .and_then(|stem| stem.to_str())
                .map(str::len)
                .unwrap_or(draft.len())
        };
        let focus = cx.focus_handle();
        let blur_subscription = cx.on_blur(&focus, window, |editor, window, cx| {
            if editor.workspace.name_edit.as_ref().is_some_and(|edit| {
                !edit.pending && edit.marked_range.is_none() && edit.draft.trim().is_empty()
            }) {
                // 失焦来自另一处点击，取消空草稿时应保留点击目标的焦点。
                editor.workspace.name_edit = None;
                cx.notify();
            } else {
                editor.confirm_workspace_name_edit(window, cx);
            }
        });
        self.dismiss_contextual_overlays(cx);
        self.workspace.active_tab = WorkspaceTab::Files;
        self.workspace.is_open = true;
        for ancestor in directory.ancestors() {
            self.workspace.expanded.insert(file_node_id(ancestor));
            if self.workspace.root.as_deref() == Some(ancestor) {
                break;
            }
        }
        self.workspace.name_edit = Some(WorkspaceNameEdit {
            kind,
            directory,
            draft,
            selected_range: 0..selected_end,
            marked_range: None,
            focus: focus.clone(),
            error: None,
            pending: false,
            last_line: None,
            last_bounds: None,
            revealed_row: None,
            row_height: None,
            scroll_x: px(0.0),
            caret: selected_end,
            selection_anchor: 0,
            _blur_subscription: blur_subscription,
        });
        self.pending_focus = None;
        window.focus(&focus, cx);
        cx.notify();
    }

    pub(crate) fn cancel_workspace_name_edit(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self
            .workspace
            .name_edit
            .as_ref()
            .is_some_and(|edit| edit.pending)
        {
            return false;
        }
        if self.workspace.name_edit.take().is_none() {
            return false;
        }
        self.focus_workspace_tree(window, cx);
        cx.notify();
        true
    }

    pub(crate) fn confirm_workspace_name_edit(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(edit) = self.workspace.name_edit.as_mut() else {
            return;
        };
        if edit.pending || edit.marked_range.is_some() {
            return;
        }
        if !valid_workspace_name(&edit.draft) {
            edit.error = Some(
                cx.global::<I18nManager>()
                    .strings()
                    .workspace_invalid_name
                    .clone(),
            );
            cx.notify();
            return;
        }
        let destination = edit.directory.join(&edit.draft);
        let kind = edit.kind.clone();
        edit.pending = true;
        edit.error = None;
        if matches!(kind, WorkspaceEditKind::Rename { .. }) {
            self.document_revision = self.document_revision.wrapping_add(1);
            self.autosave_task = None;
        }
        let operation = cx.background_spawn({
            let destination = destination.clone();
            let kind = kind.clone();
            async move { perform_workspace_name_operation(&kind, &destination) }
        });
        let window_handle = window.window_handle();
        let editor = cx.entity().downgrade();
        cx.spawn(async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let result = operation.await;
            let update = cx.update_window(window_handle, move |_, window, cx| {
                if let Err(error) = editor.update(cx, |editor, cx| {
                    match result {
                        Ok(()) => {
                            editor.workspace.name_edit = None;
                            match kind {
                                WorkspaceEditKind::File => {
                                    editor.refresh_workspace_paths(std::slice::from_ref(&destination), cx);
                                    editor.open_workspace_file(destination, window, cx);
                                }
                                WorkspaceEditKind::Folder => {
                                    editor.workspace.selected =
                                        Some(WorkspaceSelection::Directory(destination.clone()));
                                    editor.workspace.expanded.insert(file_node_id(&destination));
                                    editor.refresh_workspace_paths(std::slice::from_ref(&destination), cx);
                                    editor.focus_workspace_tree(window, cx);
                                }
                                WorkspaceEditKind::Rename {
                                    source,
                                    is_directory,
                                } => {
                                    editor.apply_workspace_rename(
                                        &source,
                                        &destination,
                                        is_directory,
                                        cx,
                                    );
                                    editor.focus_workspace_tree(window, cx);
                                }
                            }
                        }
                        Err(error) => {
                            let exists = error.kind() == std::io::ErrorKind::AlreadyExists;
                            let detail = if exists {
                                cx.global::<I18nManager>()
                                    .strings()
                                    .workspace_name_exists
                                    .clone()
                            } else {
                                error.to_string()
                            };
                            if let Some(edit) = editor.workspace.name_edit.as_mut() {
                                edit.pending = false;
                                edit.error = Some(detail.clone());
                                window.focus(&edit.focus, cx);
                            }
                            if !exists {
                                let title = cx
                                    .global::<I18nManager>()
                                    .strings()
                                    .open_failed_title
                                    .clone();
                                editor.show_message_modal(title, detail, cx);
                            }
                            if editor.document_dirty || editor.has_dirty_workspace_documents() {
                                editor.schedule_autosave(cx);
                            }
                        }
                    }
                    cx.notify();
                }) {
                    eprintln!("更新文件树名称操作结果失败：{error}");
                }
            });
            if let Err(error) = update {
                eprintln!("文件树名称操作窗口已关闭：{error}");
            }
        })
        .detach();
        cx.notify();
    }

    pub(super) fn receive_workspace_tree_drop(
        &mut self,
        sources: Vec<PathBuf>,
        directory: PathBuf,
        move_source: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self.workspace.root.clone() else {
            return;
        };
        if let Some(parent) = self.split_parent.clone() {
            let handle = window.window_handle();
            cx.defer(move |cx| {
                if let Err(error) = handle
                    .update(cx, |_, window, cx| {
                        parent.update(cx, |editor, cx| {
                            editor.start_workspace_tree_transfer(
                                sources,
                                directory,
                                root,
                                move_source,
                                window,
                                cx,
                            );
                        })
                    })
                    .and_then(|result| result)
                {
                    eprintln!("转交文件树拖放失败：{error}");
                }
            });
            return;
        }
        self.start_workspace_tree_transfer(sources, directory, root, move_source, window, cx);
    }

    fn start_workspace_tree_transfer(
        &mut self,
        sources: Vec<PathBuf>,
        directory: PathBuf,
        root: PathBuf,
        move_source: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if sources.is_empty() {
            return;
        }
        if move_source {
            self.document_revision = self.document_revision.wrapping_add(1);
            self.autosave_task = None;
            for group in self.split_groups() {
                group.update(cx, |editor, _| {
                    editor.document_revision = editor.document_revision.wrapping_add(1);
                    editor.autosave_task = None;
                });
            }
        }
        let operation = cx.background_spawn(async move {
            sources
                .into_iter()
                .map(|source| {
                    let result = transfer_workspace_path(&root, &source, &directory, move_source);
                    (source, result)
                })
                .collect::<Vec<_>>()
        });
        let handle = window.window_handle();
        let editor = cx.entity().downgrade();
        cx.spawn(async move |_, cx| {
            let results = operation.await;
            if let Err(error) = handle
                .update(cx, |_, _, cx| {
                    editor.update(cx, |editor, cx| {
                        let mut errors = Vec::new();
                        for (source, result) in results {
                            match result {
                                Ok(Some((destination, is_directory))) => {
                                    if move_source {
                                        editor.apply_workspace_rename(
                                            &source,
                                            &destination,
                                            is_directory,
                                            cx,
                                        );
                                        editor.remap_split_workspace_paths(
                                            &source,
                                            &destination,
                                            is_directory,
                                            cx,
                                        );
                                    } else {
                                        editor.reveal_workspace_transfer(&destination, cx);
                                        for group in editor.split_groups() {
                                            group.update(cx, |editor, cx| {
                                                editor.reveal_workspace_transfer(&destination, cx)
                                            });
                                        }
                                    }
                                }
                                Ok(None) => {}
                                Err(error) => {
                                    let detail =
                                        if error.kind() == std::io::ErrorKind::AlreadyExists {
                                            cx.global::<I18nManager>()
                                                .strings()
                                                .workspace_name_exists
                                                .clone()
                                        } else {
                                            error.to_string()
                                        };
                                    errors.push(format!("{}：{detail}", source.display()));
                                }
                            }
                        }
                        if !errors.is_empty() {
                            let title = cx
                                .global::<I18nManager>()
                                .strings()
                                .workspace_tab_files
                                .clone();
                            editor.show_message_modal(title, errors.join("\n"), cx);
                        }
                        if move_source
                            && (editor.document_dirty
                                || editor.has_dirty_workspace_documents()
                                || editor.has_dirty_split_documents(cx))
                        {
                            editor.schedule_autosave(cx);
                        }
                        editor.persist_session(cx);
                        cx.notify();
                    })
                })
                .and_then(|result| result)
            {
                eprintln!("更新文件树拖放结果失败：{error}");
            }
        })
        .detach();
    }

    fn reveal_workspace_transfer(&mut self, destination: &Path, cx: &mut Context<Self>) {
        if let Some(parent) = destination.parent() {
            self.workspace.expanded.insert(file_node_id(parent));
        }
        self.refresh_workspace_paths(&[destination.to_path_buf()], cx);
        cx.notify();
    }

    pub(super) fn apply_workspace_rename(
        &mut self,
        source: &Path,
        destination: &Path,
        is_directory: bool,
        cx: &mut Context<Self>,
    ) {
        for tab in &mut self.workspace.open_documents {
            if let Some(path) = remap_moved_path(&tab.path, source, destination, is_directory) {
                tab.path = path;
            }
        }
        if let Some(path) = self
            .file_path
            .as_ref()
            .and_then(|path| remap_moved_path(path, source, destination, is_directory))
        {
            self.file_path = Some(path);
            self.pending_window_title_refresh = true;
            if !self.code_document && (is_directory || source.parent() != destination.parent()) {
                self.rebuild_image_runtimes(cx);
            }
        }
        if let Some(path) = self
            .workspace
            .active_document
            .as_ref()
            .and_then(|path| remap_moved_path(path, source, destination, is_directory))
        {
            self.workspace.active_document = Some(path);
        }
        if let Some(path) = self
            .image_preview
            .as_ref()
            .and_then(|preview| remap_moved_path(&preview.path, source, destination, is_directory))
        {
            if let Some(preview) = self
                .image_preview
                .as_mut()
                .filter(|preview| preview.image.is_some())
            {
                preview.path = path;
            } else {
                self.load_image_file_preview(path, cx);
            }
        }
        if let Some(path) = self
            .unsupported_preview_path
            .as_ref()
            .and_then(|path| remap_moved_path(path, source, destination, is_directory))
        {
            self.unsupported_preview_path = Some(path);
        }
        if is_directory {
            self.workspace.expanded = self
                .workspace
                .expanded
                .iter()
                .map(|id| {
                    id.strip_prefix("file:")
                        .and_then(|path| {
                            remap_moved_path(Path::new(path), source, destination, true)
                        })
                        .map(|path| file_node_id(&path))
                        .unwrap_or_else(|| id.clone())
                })
                .collect();
        }
        self.workspace.selected = Some(if is_directory {
            WorkspaceSelection::Directory(destination.to_path_buf())
        } else {
            WorkspaceSelection::File(destination.to_path_buf())
        });
        if let Some(parent) = destination.parent() {
            self.workspace.expanded.insert(file_node_id(parent));
        }
        if is_directory {
            self.workspace.expanded.insert(file_node_id(destination));
        }
        self.refresh_workspace_paths(&[source.to_path_buf(), destination.to_path_buf()], cx);
        self.persist_session(cx);
        if self.document_dirty || self.has_dirty_workspace_documents() {
            self.schedule_autosave(cx);
        }
    }

    pub(crate) fn on_workspace_tree_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.name_edit.is_none()
            && self.workspace.context_menu.is_none()
            && !event.keystroke.modifiers.modified()
            && self
                .workspace
                .tree_focus
                .as_ref()
                .is_some_and(|focus| focus.is_focused(window))
            && self.navigate_workspace_tree(event.keystroke.key.as_str(), cx)
        {
            cx.stop_propagation();
            return;
        }
        match event.keystroke.key.as_str() {
            "f2" if self.workspace.active_tab == WorkspaceTab::Files
                && self.workspace.name_edit.is_none() =>
            {
                self.prompt_rename_selected(window, cx)
            }
            "enter"
                if self.workspace.active_tab == WorkspaceTab::Files
                    && self.workspace.name_edit.is_none() =>
            {
                match self.workspace.selected.clone() {
                    Some(WorkspaceSelection::File(path)) => self.open_workspace_file_in_mode(
                        path,
                        WorkspaceOpenMode::Pinned,
                        window,
                        cx,
                    ),
                    Some(WorkspaceSelection::Directory(path)) => {
                        self.toggle_workspace_node(&file_node_id(&path), cx)
                    }
                    _ => {}
                }
            }
            "enter" if self.workspace.active_tab == WorkspaceTab::Outline => {
                let target =
                    self.workspace
                        .selected
                        .as_ref()
                        .and_then(|selection| match selection {
                            WorkspaceSelection::Outline(id) => find_workspace_node(
                                &self.workspace.outline_tree,
                                id,
                            )
                            .and_then(|node| match node.kind {
                                WorkspaceTreeKind::Heading { line, .. } => Some((id.clone(), line)),
                                _ => None,
                            }),
                            _ => None,
                        });
                if let Some((id, line)) = target {
                    self.open_outline_node(id, line, cx);
                }
            }
            "escape" if self.workspace.context_menu.is_some() => {
                self.close_workspace_context_menu(cx)
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    fn on_workspace_name_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(edit) = self.workspace.name_edit.as_ref() else {
            return;
        };
        if edit.pending || edit.marked_range.is_some() {
            return;
        }
        let text = edit.draft.clone();
        let selected = edit.selected_range.clone();
        let caret = edit.caret;
        let anchor = edit.selection_anchor;
        let key = event.keystroke.key.as_str();
        let secondary = event.keystroke.modifiers.secondary();
        match key {
            "enter" => self.confirm_workspace_name_edit(window, cx),
            "escape" => {
                self.cancel_workspace_name_edit(window, cx);
            }
            "a" if secondary => {
                if let Some(edit) = self.workspace.name_edit.as_mut() {
                    edit.selected_range = 0..text.len();
                    edit.caret = text.len();
                    edit.selection_anchor = 0;
                }
            }
            "c" | "x" if secondary => {
                if let Some(value) = text.get(selected.clone()) {
                    cx.write_to_clipboard(ClipboardItem::new_string(value.to_string()));
                }
                if key == "x" {
                    self.replace_overlay_input_text(
                        OverlayInputKind::TreeName,
                        selected,
                        "",
                        None,
                        false,
                        cx,
                    );
                }
            }
            "v" if secondary => {
                if let Some(value) = cx.read_from_clipboard().and_then(|item| item.text()) {
                    self.replace_overlay_input_text(
                        OverlayInputKind::TreeName,
                        selected,
                        &value,
                        None,
                        false,
                        cx,
                    );
                }
            }
            "backspace" | "delete" => {
                let range = if !selected.is_empty() {
                    selected
                } else if key == "backspace" {
                    let start = text
                        .get(..selected.start)
                        .and_then(|before| before.grapheme_indices(true).last())
                        .map(|(index, _)| index)
                        .unwrap_or(selected.start);
                    start..selected.start
                } else {
                    let end = text
                        .get(selected.end..)
                        .and_then(|after| after.graphemes(true).next())
                        .map(|value| selected.end + value.len())
                        .unwrap_or(selected.end);
                    selected.end..end
                };
                self.replace_overlay_input_text(
                    OverlayInputKind::TreeName,
                    range,
                    "",
                    None,
                    false,
                    cx,
                );
            }
            "left" | "right" | "home" | "end" => {
                let shift = event.keystroke.modifiers.shift;
                let position = match key {
                    "home" => 0,
                    "end" => text.len(),
                    "left" if !shift && !selected.is_empty() => selected.start,
                    "right" if !shift && !selected.is_empty() => selected.end,
                    "left" => text
                        .get(..caret)
                        .and_then(|before| before.grapheme_indices(true).last())
                        .map(|(index, _)| index)
                        .unwrap_or(0),
                    _ => text
                        .get(caret..)
                        .and_then(|after| after.graphemes(true).next())
                        .map(|value| caret + value.len())
                        .unwrap_or(text.len()),
                };
                if let Some(edit) = self.workspace.name_edit.as_mut() {
                    edit.selection_anchor = if shift { anchor } else { position };
                    edit.caret = position;
                    edit.selected_range =
                        edit.selection_anchor.min(position)..edit.selection_anchor.max(position);
                }
            }
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    pub(crate) fn render_workspace_name_row(
        &self,
        depth: usize,
        theme: &Theme,
        editor: &WeakEntity<Editor>,
    ) -> AnyElement {
        let Some(edit) = self.workspace.name_edit.as_ref() else {
            return div().into_any_element();
        };
        let is_directory = matches!(
            edit.kind,
            WorkspaceEditKind::Folder
                | WorkspaceEditKind::Rename {
                    is_directory: true,
                    ..
                }
        );
        let icon = if is_directory {
            gpui_kit::component::Icon::new(gpui_kit::assets::IconName::Folder)
                .text_color(theme.colors.dialog_muted).into_any_element()
        } else {
            let path = if is_markdown_document(Path::new(&edit.draft)) { MARKDOWN_ICON } else { GENERIC_FILE_ICON };
            svg().path(path).size_4().text_color(theme.colors.dialog_primary_button_bg).into_any_element()
        };
        let draft: SharedString = edit.draft.clone().into();
        let selection = edit.selected_range.clone();
        let caret = edit.caret;
        let marked = edit.marked_range.clone();
        let focus = edit.focus.clone();
        let previous_scroll = edit.scroll_x;
        let input_editor = editor.clone();
        let click_editor = editor.clone();
        let key_editor = editor.clone();
        let colors = theme.colors.clone();
        let input = div()
            .id("workspace-name-input")
            .debug_selector(|| "workspace-name-input".to_string())
            .track_focus(&focus)
            .flex_1()
            .min_w(px(0.0))
            .h(px(Self::workspace_node_height(theme)))
            .px(px(3.0))
            .overflow_hidden()
            .border_1()
            .border_color(if edit.error.is_some() {
                colors.dialog_danger_button_bg
            } else {
                colors.dialog_primary_button_bg
            })
            .bg(colors.editor_background)
            .text_size(px(theme.typography.ui_text_size(12.0)))
            .text_color(colors.text_default)
            .cursor(CursorStyle::IBeam)
            .child(
                canvas(
                    move |_, window, _cx| {
                        window.text_system().shape_line(
                            draft.clone(),
                            px(12.0),
                            &[TextRun {
                                len: draft.len(),
                                font: window.text_style().font(),
                                color: colors.text_default,
                                background_color: None,
                                underline: None,
                                strikethrough: None,

                            }],
                            None,
                        )
                    },
                    move |bounds, line, window, cx| {
                        let width = (bounds.size.width - px(4.0)).max(px(1.0));
                        let caret_x = line.x_for_index(caret);
                        let scroll = if caret_x < previous_scroll {
                            caret_x
                        } else if caret_x > previous_scroll + width {
                            caret_x - width
                        } else {
                            previous_scroll
                        };
                        let scroll = scroll.min((line.width - width).max(px(0.0)));
                        let origin = point(bounds.left() - scroll, bounds.top());
                        if focus.is_focused(window) && !selection.is_empty() {
                            let start = line.x_for_index(selection.start);
                            let end = line.x_for_index(selection.end);
                            window.paint_quad(fill(
                                Bounds::new(
                                    point(origin.x + start, bounds.top()),
                                    size(end - start, bounds.size.height),
                                ),
                                colors.selection,
                            ));
                        }
                        if let Err(error) = line.paint(origin, bounds.size.height, gpui::TextAlign::Left, None, window, cx) {
                            eprintln!("绘制文件名失败：{error}");
                        }
                        if focus.is_focused(window) {
                            if selection.is_empty() {
                                window.paint_quad(fill(
                                    Bounds::new(
                                        point(origin.x + caret_x, bounds.top() + px(2.0)),
                                        size(px(1.0), (bounds.size.height - px(4.0)).max(px(1.0))),
                                    ),
                                    colors.cursor,
                                ));
                            }
                            if let Some(range) = marked.as_ref() {
                                let start = line.x_for_index(range.start);
                                let end = line.x_for_index(range.end);
                                window.paint_quad(fill(
                                    Bounds::new(
                                        point(origin.x + start, bounds.bottom() - px(2.0)),
                                        size(end - start, px(1.0)),
                                    ),
                                    colors.text_default,
                                ));
                            }
                        }
                        if let Some(entity) = input_editor.upgrade() {
                            entity.update(cx, |editor, _cx| {
                                if let Some(edit) = editor.workspace.name_edit.as_mut() {
                                    edit.last_line = Some(line.clone());
                                    edit.last_bounds = Some(bounds);
                                    edit.scroll_x = scroll;
                                }
                            });
                            window.handle_input(
                                &focus,
                                ElementInputHandler::new(bounds, entity),
                                cx,
                            );
                        }
                    },
                )
                .w_full()
                .h_full(),
            )
            .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                if let Err(error) = click_editor.update(cx, |editor, cx| {
                    if let Some(edit) = editor.workspace.name_edit.as_mut() {
                        window.focus(&edit.focus, cx);
                        if let Some((line, bounds)) = edit.last_line.as_ref().zip(edit.last_bounds)
                        {
                            let index = line.closest_index_for_x(
                                event.position.x - bounds.left() + edit.scroll_x,
                            );
                            if edit.draft.is_char_boundary(index) {
                                edit.selected_range = index..index;
                                edit.caret = index;
                                edit.selection_anchor = index;
                            }
                        }
                    }
                    cx.notify();
                }) {
                    eprintln!("聚焦文件名输入失败：{error}");
                }
                cx.stop_propagation();
            })
            .capture_key_down(move |event, window, cx| {
                if let Err(error) = key_editor.update(cx, |editor, cx| {
                    editor.on_workspace_name_key_down(event, window, cx)
                }) {
                    eprintln!("处理文件名按键失败：{error}");
                }
            });
        div()
            .w_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(Self::workspace_node_height(theme)))
                    .w_full()
                    .pl(Self::workspace_tree_indent(depth))
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .child(div().w_5().flex_shrink_0())
                    .child(icon)
                    .child(input),
            )
            .children(edit.error.as_ref().map(|error| {
                div()
                    .pl(Self::workspace_tree_indent(depth) + rems(2.75))
                    .text_size(px(theme.typography.ui_text_size(10.0)))
                    .text_color(theme.colors.dialog_danger_button_bg)
                    .child(error.clone())
            }))
            .into_any_element()
    }
}

pub(crate) fn valid_workspace_name(name: &str) -> bool {
    !name.trim().is_empty() && !matches!(name, "." | "..") && !name.contains(['/', '\\', '\0'])
}

fn perform_workspace_name_operation(
    kind: &WorkspaceEditKind,
    destination: &Path,
) -> std::io::Result<()> {
    match kind {
        WorkspaceEditKind::File => create_workspace_file(destination),
        WorkspaceEditKind::Folder => create_workspace_folder(destination),
        WorkspaceEditKind::Rename { source, .. } => {
            if source == destination {
                return Ok(());
            }
            match fs::symlink_metadata(destination) {
                Ok(metadata) => {
                    let case_only = !metadata.file_type().is_symlink()
                        && source.file_name().zip(destination.file_name()).is_some_and(
                            |(old, new)| {
                                old.to_string_lossy()
                                    .eq_ignore_ascii_case(&new.to_string_lossy())
                            },
                        )
                        && source.canonicalize()? == destination.canonicalize()?;
                    if !case_only {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::AlreadyExists,
                            "目标名称已存在",
                        ));
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
            fs::rename(source, destination)
        }
    }
}

fn transfer_workspace_path(
    root: &Path,
    source: &Path,
    directory: &Path,
    move_source: bool,
) -> std::io::Result<Option<(PathBuf, bool)>> {
    use std::io::{Error, ErrorKind};
    let root = crate::file_url::strip_verbatim_prefix(&fs::canonicalize(root)?);
    let directory = crate::file_url::strip_verbatim_prefix(&fs::canonicalize(directory)?);
    if !directory.is_dir() || !directory.starts_with(&root) {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "拖放目标必须是工作区内的目录",
        ));
    }
    let name = source
        .file_name()
        .ok_or_else(|| Error::new(ErrorKind::InvalidInput, "无法移动文件系统根目录"))?;
    let parent = source
        .parent()
        .ok_or_else(|| Error::new(ErrorKind::InvalidInput, "文件没有父目录"))?;
    // 保留末级符号链接本身，不能 canonicalize 后把链接指向的实体移走。
    let source = crate::file_url::strip_verbatim_prefix(&fs::canonicalize(parent)?).join(name);
    if move_source && (source == root || !source.starts_with(&root)) {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "不能移动工作区根目录或工作区外的文件",
        ));
    }
    let is_directory = fs::metadata(&source)?.is_dir();
    if is_directory
        && directory.starts_with(
            fs::canonicalize(&source).map(|path| crate::file_url::strip_verbatim_prefix(&path))?,
        )
    {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "不能将目录放入自身或它的子目录",
        ));
    }
    let destination = directory.join(name);
    if source == destination {
        return Ok(None);
    }
    if move_source {
        perform_workspace_name_operation(
            &WorkspaceEditKind::Rename {
                source,
                is_directory,
            },
            &destination,
        )?;
    } else {
        copy_workspace_drop_path(&source, &destination)?;
    }
    Ok(Some((destination, is_directory)))
}

fn copy_workspace_drop_path(source: &Path, destination: &Path) -> std::io::Result<()> {
    let metadata = fs::symlink_metadata(source)?;
    if metadata.file_type().is_symlink() {
        let target = fs::read_link(source)?;
        #[cfg(unix)]
        return std::os::unix::fs::symlink(target, destination);
        #[cfg(windows)]
        return if source.is_dir() {
            std::os::windows::fs::symlink_dir(target, destination)
        } else {
            std::os::windows::fs::symlink_file(target, destination)
        };
    }
    if metadata.is_dir() {
        fs::create_dir(destination)?;
        let result = (|| {
            for entry in fs::read_dir(source)? {
                let entry = entry?;
                copy_workspace_drop_path(&entry.path(), &destination.join(entry.file_name()))?;
            }
            Ok(())
        })();
        if result.is_err()
            && let Err(error) = fs::remove_dir_all(destination)
        {
            eprintln!(
                "清理未完成的拖入目录失败 {}：{error}",
                destination.display()
            );
        }
        result
    } else if metadata.is_file() {
        let mut input = fs::File::open(source)?;
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination)?;
        let result = std::io::copy(&mut input, &mut output)
            .and_then(|_| fs::set_permissions(destination, metadata.permissions()));
        drop(output);
        if result.is_err()
            && let Err(error) = fs::remove_file(destination)
        {
            eprintln!(
                "清理未完成的拖入文件失败 {}：{error}",
                destination.display()
            );
        }
        result
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "只能拖入普通文件或目录",
        ))
    }
}
