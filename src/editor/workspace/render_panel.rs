use super::*;

impl Editor {
    pub(crate) fn render_workspace_panel(
        &mut self,
        theme: &Theme,
        strings: &I18nStrings,
        panel_width: f32,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.workspace.is_open && !self.sidebar_peek && !self.sidebar_overlay_closing {
            // 侧栏收起时不再在这里重算文档大纲：它曾经的读者是正文里的块级 `[TOC]`，
            // 而现在 `[TOC]` 在自己需要的那一帧直接要一份清单（见
            // `apply_heading_fold_filter`），于是「没人看也算一遍」的每帧整篇开销没了。
            // 文件树同步仍留给打开的抽屉。收回动画期间面板要继续渲染（浮层还在滑出），
            // 所以只在完全静止的收起状态才早退。
            return None;
        }

        self.sync_workspace_models(cx);
        let editor = cx.entity().downgrade();
        let resize_editor = editor.clone();
        let c = &theme.colors;
        let d = &theme.dimensions;

        let search_header = (self.workspace.active_tab == WorkspaceTab::Search)
            .then(|| self.render_search_header(theme, strings, window, cx));
        let tree_sort_header = (self.workspace.active_tab == WorkspaceTab::Files)
            .then(|| self.render_tree_filter_and_sort_header(theme, strings, window, cx));
        let body = match self.workspace.active_tab {
            WorkspaceTab::Files => self.render_workspace_files_tree(theme, strings, &editor),
            WorkspaceTab::Search => self.render_search_results(theme, strings, &editor),
            WorkspaceTab::Outline => self.render_workspace_outline_tree(theme, strings, &editor),
            WorkspaceTab::Backlinks => {
                self.render_workspace_backlinks_panel(theme, strings, window, cx)
            }
            WorkspaceTab::Tags => self.render_workspace_tags_panel(theme, strings, cx),
        };

        Some(
            div()
                .id("workspace-panel")
                .relative()
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(Self::on_workspace_background_right_click),
                )
                .h_full()
                .w(px(panel_width))
                .flex()
                .flex_col()
                .flex_shrink_0()
                .bg(c.dialog_secondary_button_bg)
                .border_r(px(d.dialog_border_width))
                .border_color(c.dialog_border)
                .children(search_header)
                .children(tree_sort_header)
                .child(
                    div()
                        .id("workspace-panel-scroll")
                        .flex_1()
                        .min_h(px(0.0))
                        .overflow_y_scroll()
                        .track_scroll(&self.workspace.tree_scroll_handle)
                        .px(px(4.0))
                        .py(px(6.0))
                        .child(body),
                )
                .child(
                    div()
                        .id("workspace-resize-handle")
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .right_0()
                        .w(px(6.0))
                        .cursor(CursorStyle::ResizeLeftRight)
                        .hover(|this| this.bg(c.selection))
                        .on_mouse_down(MouseButton::Left, move |event, _, cx| {
                            let _ = resize_editor.update(cx, |editor, cx| {
                                editor.start_workspace_resize(
                                    f32::from(event.position.x),
                                    panel_width,
                                    cx,
                                );
                            });
                            cx.stop_propagation();
                        }),
                )
                .into_any_element(),
        )
    }
    pub(crate) fn render_workspace_outline_tree(
        &self,
        theme: &Theme,
        strings: &I18nStrings,
        editor: &WeakEntity<Editor>,
    ) -> AnyElement {
        if self.workspace.outline_tree.is_empty() {
            return self.render_workspace_empty_state("", &strings.workspace_empty_outline, theme);
        }

        div()
            .w_full()
            .flex()
            .flex_col()
            .children(self.render_workspace_nodes(&self.workspace.outline_tree, 0, theme, editor))
            .into_any_element()
    }

    pub(crate) fn render_workspace_empty_state(
        &self,
        title: &str,
        message: &str,
        theme: &Theme,
    ) -> AnyElement {
        let c = &theme.colors;
        let t = &theme.typography;
        let title = (!title.is_empty()).then(|| {
            div()
                .text_size(px(t.text_size))
                .font_weight(FontWeight::MEDIUM)
                .text_color(c.text_default)
                .child(title.to_string())
        });

        div()
            .w_full()
            .h_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(8.0))
            .px(px(22.0))
            .text_align(TextAlign::Center)
            .children(title)
            .child(
                div()
                    .text_size(px(t.text_size * 0.9))
                    .line_height(px(t.text_size * t.text_line_height))
                    .text_color(c.dialog_muted)
                    .child(message.to_string()),
            )
            .into_any_element()
    }

    pub(crate) fn render_workspace_nodes(
        &self,
        nodes: &[WorkspaceTreeNode],
        depth: usize,
        theme: &Theme,
        editor: &WeakEntity<Editor>,
    ) -> Vec<AnyElement> {
        let mut elements = Vec::new();
        for node in nodes {
            elements.push(self.render_workspace_node(node, depth, theme, editor));
            if !node.children.is_empty() && self.workspace.expanded.contains(&node.id) {
                elements.extend(self.render_workspace_nodes(
                    &node.children,
                    depth + 1,
                    theme,
                    editor,
                ));
            }
        }
        elements
    }

    pub(crate) fn render_workspace_node(
        &self,
        node: &WorkspaceTreeNode,
        depth: usize,
        theme: &Theme,
        editor: &WeakEntity<Editor>,
    ) -> AnyElement {
        let c = &theme.colors;
        let is_expanded = self.workspace.expanded.contains(&node.id);
        let has_children = !node.children.is_empty();
        let selected = match (&self.workspace.selected, &node.kind) {
            (Some(WorkspaceSelection::Directory(selected)), WorkspaceTreeKind::Directory(path)) => {
                selected == path
            }
            (Some(WorkspaceSelection::File(selected)), WorkspaceTreeKind::MarkdownFile(path)) => {
                selected == path
            }
            (Some(WorkspaceSelection::File(selected)), WorkspaceTreeKind::CodeFile(path)) => {
                selected == path
            }
            (Some(WorkspaceSelection::Outline(selected)), _) => selected == &node.id,
            _ => false,
        };
        let node_id = node.id.clone();
        let click_editor = editor.clone();
        let click_kind = node.kind.clone();
        let context_editor = editor.clone();
        let context_kind = node.kind.clone();
        let arrow_node_id = node.id.clone();
        let arrow_editor = editor.clone();
        let arrow_icon = has_children.then_some(if is_expanded {
            CHEVRON_DOWN_ICON
        } else {
            CHEVRON_RIGHT_ICON
        });

        let icon = match &node.kind {
            WorkspaceTreeKind::Directory(_) => Some((FOLDER_ICON, Hsla::from(rgba(0x4a93d8ff)))),
            WorkspaceTreeKind::MarkdownFile(_) => Some((MARKDOWN_ICON, c.dialog_primary_button_bg)),
            WorkspaceTreeKind::CodeFile(_) => Some((CODE_ICON, c.dialog_muted)),
            WorkspaceTreeKind::OtherFile(_) => Some((GENERIC_FILE_ICON, c.dialog_muted)),
            WorkspaceTreeKind::Heading { .. } => None,
        };

        let label_color = if selected {
            c.text_default
        } else {
            c.dialog_muted
        };

        let mut arrow_el = div()
            .w(px(16.0))
            .h(px(20.0))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .children(
                arrow_icon.map(|path| svg().path(path).size(px(14.0)).text_color(c.dialog_muted)),
            );
        if has_children {
            arrow_el = arrow_el.cursor_pointer().on_mouse_down(
                MouseButton::Left,
                move |_event, _window, cx| {
                    let _ = arrow_editor.update(cx, |editor, cx| {
                        editor.toggle_workspace_node(&arrow_node_id, cx);
                    });
                    cx.stop_propagation();
                },
            );
        }

        // 文件节点悬停显示 大小 · 修改时间（roadmap D7）。
        let tooltip_text = tree_node_tooltip(&node);
        div()
            .id(("workspace-node", stable_node_hash(&node.id)))
            .h(px(WORKSPACE_NODE_HEIGHT))
            .w_full()
            .overflow_hidden()
            .flex()
            .items_center()
            .gap(px(4.0))
            .pl(px(6.0 + depth as f32 * WORKSPACE_NODE_INDENT))
            .pr(px(6.0))
            .rounded(px(4.0))
            .tooltip(move |_, cx| {
                let tooltip_text = tooltip_text.clone();
                cx.new(|_| WorkspaceTooltip { label: tooltip_text }).into()
            })
            .bg(if selected {
                c.selection
            } else {
                hsla(0.0, 0.0, 0.0, 0.0)
            })
            .hover(|this| this.bg(c.dialog_secondary_button_hover))
            .cursor_pointer()
            .child(arrow_el)
            .children(icon.map(|(path, color)| {
                svg()
                    .path(path)
                    .size(px(16.0))
                    .flex_shrink_0()
                    .text_color(color)
                    .into_any_element()
            }))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .overflow_hidden()
                    .truncate()
                    .text_size(px(12.0))
                    .line_height(px(18.0))
                    .text_color(label_color)
                    .child(node.label.clone()),
            )
            .on_mouse_down(MouseButton::Right, move |event, _, cx| {
                let selection = match &context_kind {
                    WorkspaceTreeKind::Directory(path) => {
                        Some(WorkspaceSelection::Directory(path.clone()))
                    }
                    WorkspaceTreeKind::MarkdownFile(path)
                    | WorkspaceTreeKind::CodeFile(path)
                    | WorkspaceTreeKind::OtherFile(path) => {
                        Some(WorkspaceSelection::File(path.clone()))
                    }
                    WorkspaceTreeKind::Heading { .. } => None,
                };
                let _ = context_editor.update(cx, |editor, cx| {
                    editor.open_workspace_context_menu(event.position, selection, cx);
                });
                cx.stop_propagation();
            })
            .on_click(move |event, window, cx| {
                if !event.standard_click() {
                    return;
                }
                let node_id = node_id.clone();
                let click_kind = click_kind.clone();
                let _ = click_editor.update(cx, |editor, cx| match click_kind {
                    WorkspaceTreeKind::Directory(path) => {
                        editor.workspace.selected = Some(WorkspaceSelection::Directory(path));
                        editor.toggle_workspace_node(&node_id, cx);
                    }
                    WorkspaceTreeKind::MarkdownFile(path) => {
                        let mode = Self::tree_click_open_mode(&event);
                        editor.open_workspace_file_in_mode(path, mode, window, cx);
                    }
                    WorkspaceTreeKind::CodeFile(path) => {
                        let mode = Self::tree_click_open_mode(&event);
                        editor.open_workspace_file_in_mode(path, mode, window, cx);
                    }
                    WorkspaceTreeKind::OtherFile(path) => {
                        let mode = Self::tree_click_open_mode(&event);
                        editor.open_workspace_file_in_mode(path, mode, window, cx);
                    }
                    WorkspaceTreeKind::Heading { line, .. } => {
                        if event.click_count() >= 2 {
                            editor.rename_outline_heading(line, cx);
                        } else {
                            editor.open_outline_node(node_id, line, cx);
                        }
                    }
                });
            })
            .into_any_element()
    }
}

/// 永久删除（roadmap H2 的「永久删除」策略）。
pub(crate) fn permanent_delete(target: &Path, is_directory: bool) -> std::io::Result<()> {
    if is_directory {
        std::fs::remove_dir_all(target)
    } else {
        std::fs::remove_file(target)
    }
}

/// Moves a workspace item to the system trash so accidental deletions are
/// recoverable (roadmap D4). Falls back to hard delete where trash semantics
/// are unavailable.
pub(crate) fn move_to_trash(target: &Path, is_directory: bool) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var("HOME").unwrap_or_default();
        if home.is_empty() {
            return fallback_delete(target, is_directory);
        }
        let trash_dir = PathBuf::from(home).join(".Trash");
        std::fs::create_dir_all(&trash_dir)?;
        let name = target
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "deleted".into());
        let stem = target
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| name.clone());
        let extension = target
            .extension()
            .map(|extension| format!(".{}", extension.to_string_lossy()))
            .unwrap_or_default();
        let mut destination = trash_dir.join(&name);
        let mut counter = 1u32;
        while destination.exists() {
            destination = trash_dir.join(format!("{stem} {counter}{extension}"));
            counter += 1;
        }
        std::fs::rename(target, destination)
    }
    #[cfg(not(target_os = "macos"))]
    {
        fallback_delete(target, is_directory)
    }
}

pub(crate) fn fallback_delete(target: &Path, is_directory: bool) -> std::io::Result<()> {
    if is_directory {
        std::fs::remove_dir_all(target)
    } else {
        std::fs::remove_file(target)
    }
}

pub(crate) fn is_markdown_file(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.to_string_lossy().eq_ignore_ascii_case("md"))
}

/// 是否按 Markdown 文档打开：仅 `.md` / `.markdown`。所有打开入口（工作区树、
/// 拖拽、命令行）必须共用这一条判定，否则同一文件两条入口行为不一致。
pub(crate) fn is_markdown_document(path: &Path) -> bool {
    path.extension().is_some_and(|extension| {
        let extension = extension.to_string_lossy();
        extension.eq_ignore_ascii_case("md") || extension.eq_ignore_ascii_case("markdown")
    })
}

pub(crate) fn create_workspace_file(path: &Path) -> std::io::Result<()> {
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(path)?;
    let template = crate::config::EditorSettings::new_file_template();
    if !template.is_empty() {
        let date = crate::config::today_local_date();
        use std::io::Write;
        file.write_all(template.replace("{date}", &date).as_bytes())?;
    }
    Ok(())
}

pub(crate) fn create_workspace_folder(path: &Path) -> std::io::Result<()> {
    std::fs::create_dir(path)
}

pub(crate) fn remap_moved_path(
    path: &Path,
    source: &Path,
    destination: &Path,
    source_is_directory: bool,
) -> Option<PathBuf> {
    let suffix = if source_is_directory {
        path.strip_prefix(source).ok()?
    } else if path == source {
        Path::new("")
    } else {
        return None;
    };
    Some(destination.join(suffix))
}

pub(crate) fn inline_image_destination_range(source: &str, range: Range<usize>) -> Option<Range<usize>> {
    let image = source.get(range.clone())?;
    let bytes = image.as_bytes();
    if !image.starts_with("![") {
        return None;
    }

    let mut cursor = 2;
    let mut bracket_depth = 1;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'\\' => cursor = cursor.checked_add(2)?,
            b'[' => {
                bracket_depth += 1;
                cursor += 1;
            }
            b']' => {
                bracket_depth -= 1;
                cursor += 1;
                if bracket_depth == 0 {
                    break;
                }
            }
            _ => cursor += 1,
        }
    }
    if bracket_depth != 0 {
        return None;
    }
    while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
        cursor += 1;
    }
    if bytes.get(cursor) != Some(&b'(') {
        return None;
    }
    cursor += 1;
    while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
        cursor += 1;
    }

    let (start, end) = if bytes.get(cursor) == Some(&b'<') {
        let start = cursor + 1;
        cursor = start;
        while cursor < bytes.len() {
            if bytes[cursor] == b'\\' {
                cursor = cursor.checked_add(2)?;
            } else if bytes[cursor] == b'>' {
                break;
            } else {
                cursor += 1;
            }
        }
        (start, cursor)
    } else {
        let start = cursor;
        let mut parentheses = 0usize;
        while cursor < bytes.len() {
            match bytes[cursor] {
                b'\\' => cursor = cursor.checked_add(2)?,
                b'(' => {
                    parentheses += 1;
                    cursor += 1;
                }
                b')' if parentheses == 0 => break,
                b')' => {
                    parentheses -= 1;
                    cursor += 1;
                }
                byte if byte.is_ascii_whitespace() && parentheses == 0 => break,
                _ => cursor += 1,
            }
        }
        (start, cursor)
    };
    (start < end).then_some((range.start + start)..(range.start + end))
}

pub(crate) fn rewrite_relative_image_destination(
    destination: &str,
    source_directory: &Path,
    destination_directory: &Path,
) -> Option<String> {
    if destination.starts_with("//")
        || destination.starts_with('/')
        || destination.starts_with('#')
        || url::Url::parse(destination).is_ok()
    {
        return None;
    }

    let suffix_start = destination
        .char_indices()
        .find(|(_, character)| matches!(character, '?' | '#'))
        .map_or(destination.len(), |(index, _)| index);
    let (relative_target, suffix) = destination.split_at(suffix_start);
    if relative_target.is_empty() {
        return None;
    }
    let target_path = Path::new(relative_target);
    if target_path.is_absolute() {
        return None;
    }
    let resolved_target = normalize_path(&source_directory.join(target_path));
    let relative_path = relative_path_between(destination_directory, &resolved_target)?;
    let mut relative = relative_path.to_string_lossy().replace('\\', "/");
    if !relative.starts_with("./") && !relative.starts_with("../") {
        relative = format!("./{relative}");
    }
    relative = relative
        .replace('%', "%25")
        .replace(' ', "%20")
        .replace('(', "%28")
        .replace(')', "%29")
        .replace('"', "%22");
    Some(format!("{relative}{suffix}"))
}

pub(crate) fn rewrite_relative_image_targets(
    markdown: &str,
    source_directory: &Path,
    destination_directory: &Path,
) -> String {
    let mut replacements = Vec::new();
    let mut reference_destinations = HashMap::new();
    for (event, range) in Parser::new_ext(markdown, Options::all()).into_offset_iter() {
        let Event::Start(Tag::Image {
            link_type,
            dest_url,
            id,
            ..
        }) = event
        else {
            continue;
        };
        let Some(destination) =
            rewrite_relative_image_destination(&dest_url, source_directory, destination_directory)
        else {
            continue;
        };
        if link_type == LinkType::Inline {
            if let Some(range) = inline_image_destination_range(markdown, range) {
                replacements.push((range, destination));
            }
        } else {
            reference_destinations.insert(normalize_reference_id(&id), destination);
        }
    }

    if !reference_destinations.is_empty() {
        let mut line_offset = 0;
        for line in markdown.split_inclusive('\n') {
            if let Some((id, range)) = reference_definition_target_range(line, line_offset)
                && let Some(destination) = reference_destinations.get(&id)
            {
                replacements.push((range, destination.clone()));
            }
            line_offset += line.len();
        }
    }

    replacements.sort_by(|left, right| right.0.start.cmp(&left.0.start));
    let mut rewritten = markdown.to_string();
    for (range, destination) in replacements {
        rewritten.replace_range(range, &destination);
    }
    rewritten
}

pub(crate) fn normalize_reference_id(id: &str) -> String {
    id.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

pub(crate) fn reference_definition_target_range(
    line: &str,
    line_offset: usize,
) -> Option<(String, Range<usize>)> {
    let bytes = line.as_bytes();
    let mut cursor = 0;
    while bytes.get(cursor) == Some(&b' ') && cursor < 4 {
        cursor += 1;
    }
    if bytes.get(cursor) != Some(&b'[') {
        return None;
    }
    let id_start = cursor + 1;
    cursor = id_start;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'\\' => cursor = cursor.checked_add(2)?,
            b']' => break,
            _ => cursor += 1,
        }
    }
    if bytes.get(cursor) != Some(&b']') || bytes.get(cursor + 1) != Some(&b':') {
        return None;
    }
    let id = normalize_reference_id(line.get(id_start..cursor)?);
    cursor += 2;
    while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
        cursor += 1;
    }

    let (start, end) = if bytes.get(cursor) == Some(&b'<') {
        let start = cursor + 1;
        cursor = start;
        while cursor < bytes.len() {
            if bytes[cursor] == b'\\' {
                cursor = cursor.checked_add(2)?;
            } else if bytes[cursor] == b'>' {
                break;
            } else {
                cursor += 1;
            }
        }
        (start, cursor)
    } else {
        let start = cursor;
        while cursor < bytes.len() && !bytes[cursor].is_ascii_whitespace() && bytes[cursor] != b')'
        {
            if bytes[cursor] == b'\\' {
                cursor = cursor.checked_add(2)?;
            } else {
                cursor += 1;
            }
        }
        (start, cursor)
    };
    (start < end).then_some((id, (line_offset + start)..(line_offset + end)))
}

pub(crate) fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if !normalized.pop() {
                    normalized.push(component.as_os_str());
                }
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

pub(crate) fn relative_path_between(from: &Path, to: &Path) -> Option<PathBuf> {
    let from = normalize_path(from);
    let to = normalize_path(to);
    if !from.is_absolute() || !to.is_absolute() {
        return None;
    }
    let from_components = from.components().collect::<Vec<_>>();
    let to_components = to.components().collect::<Vec<_>>();
    let mut common = 0;
    while common < from_components.len()
        && common < to_components.len()
        && from_components[common] == to_components[common]
    {
        common += 1;
    }
    if common == 0 {
        return None;
    }

    let mut relative = PathBuf::new();
    for component in &from_components[common..] {
        if matches!(component, std::path::Component::Normal(_)) {
            relative.push("..");
        }
    }
    for component in &to_components[common..] {
        if matches!(
            component,
            std::path::Component::Normal(_) | std::path::Component::ParentDir
        ) {
            relative.push(component.as_os_str());
        }
    }
    Some(relative)
}

pub(crate) fn path_is_affected(path: &Path, target: &Path, target_is_directory: bool) -> bool {
    if target_is_directory {
        path.starts_with(target)
    } else {
        path == target
    }
}
