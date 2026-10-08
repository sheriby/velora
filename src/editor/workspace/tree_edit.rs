use super::*;

impl Editor {
    pub(crate) fn focus_workspace_tree(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let focus = self
            .workspace
            .tree_focus
            .get_or_insert_with(|| cx.focus_handle());
        self.pending_focus = None;
        window.focus(focus);
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
            editor.confirm_workspace_name_edit(window, cx);
        });
        self.dismiss_contextual_overlays(cx);
        self.workspace.active_tab = WorkspaceTab::Files;
        self.workspace.is_open = true;
        self.workspace.tree_filter.clear();
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
            scroll_x: px(0.0),
            caret: selected_end,
            selection_anchor: 0,
            _blur_subscription: blur_subscription,
        });
        self.pending_focus = None;
        window.focus(&focus);
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
                                    editor.refresh_workspace_tree(cx);
                                    editor.open_workspace_file(destination, window, cx);
                                }
                                WorkspaceEditKind::Folder => {
                                    editor.workspace.selected =
                                        Some(WorkspaceSelection::Directory(destination.clone()));
                                    editor.workspace.expanded.insert(file_node_id(&destination));
                                    editor.refresh_workspace_tree(cx);
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
                                window.focus(&edit.focus);
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

    fn apply_workspace_rename(
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
            if is_directory && !self.code_document {
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
        self.workspace.selected = Some(if is_directory {
            WorkspaceSelection::Directory(destination.to_path_buf())
        } else {
            WorkspaceSelection::File(destination.to_path_buf())
        });
        if is_directory {
            self.workspace.expanded.insert(file_node_id(destination));
        }
        self.refresh_workspace_tree(cx);
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
                if let Some(WorkspaceSelection::File(path)) = self.workspace.selected.clone() {
                    self.open_workspace_file_in_mode(path, WorkspaceOpenMode::Pinned, window, cx);
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
            FOLDER_ICON
        } else if is_markdown_document(Path::new(&edit.draft)) {
            MARKDOWN_ICON
        } else {
            GENERIC_FILE_ICON
        };
        let icon_color = if is_directory {
            Hsla::from(rgba(0x4a93d8ff))
        } else {
            theme.colors.dialog_primary_button_bg
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
            .h(px(WORKSPACE_NODE_HEIGHT))
            .px(px(3.0))
            .overflow_hidden()
            .border_1()
            .border_color(if edit.error.is_some() {
                colors.dialog_danger_button_bg
            } else {
                colors.dialog_primary_button_bg
            })
            .bg(colors.editor_background)
            .text_size(px(12.0))
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
                                font_size: None,
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
                        if let Err(error) = line.paint(origin, bounds.size.height, window, cx) {
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
                        window.focus(&edit.focus);
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
                    .h(px(WORKSPACE_NODE_HEIGHT))
                    .w_full()
                    .pl(px(6.0 + depth as f32 * WORKSPACE_NODE_INDENT))
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .child(div().w(px(16.0)).flex_shrink_0())
                    .child(svg().path(icon).size(px(16.0)).text_color(icon_color))
                    .child(input),
            )
            .children(edit.error.as_ref().map(|error| {
                div()
                    .pl(px(42.0 + depth as f32 * WORKSPACE_NODE_INDENT))
                    .text_size(px(10.0))
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
