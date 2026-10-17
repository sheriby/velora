use super::*;
use gpui::prelude::FluentBuilder;

/// 文件树摊平出来的一行。节点行之外，名称输入行与懒加载占位行也各占一行，
/// 三者同高才能按固定行高开窗（`Editor::workspace_list_window`）。
pub(crate) enum WorkspaceTreeRow<'a> {
    Node(&'a WorkspaceTreeNode, usize),
    NameEdit(usize),
    Loading(usize),
}

impl Editor {
    pub(crate) fn render_workspace_panel(
        &mut self,
        theme: &Theme,
        strings: &I18nStrings,
        panel_width: f32,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.workspace.is_open {
            // 面板收起时不再在这里重算文档大纲：它曾经的读者是正文里的块级 `[TOC]`，
            // 而现在 `[TOC]` 在自己需要的那一帧直接要一份清单（见
            // `apply_heading_fold_filter`），于是「没人看也算一遍」的每帧整篇开销没了。
            // 文件树同步仍留给打开的面板。
            return None;
        }

        self.sync_workspace_models(cx);
        let editor = cx.entity().downgrade();
        let tree_focus = self.workspace.tree_focus.get_or_insert_with(|| cx.focus_handle()).clone();
        let resize_editor = editor.clone();
        let c = &theme.colors;
        let d = &theme.dimensions;

        let search_header = (self.workspace.active_tab == WorkspaceTab::Search)
            .then(|| self.render_search_header(theme, strings, window, cx));
        let body = match self.workspace.active_tab {
            WorkspaceTab::Files => self.render_workspace_files_tree(theme, strings, &editor, cx),
            WorkspaceTab::Search => self.render_search_results(theme, strings, &editor, cx),
            WorkspaceTab::Outline => self.render_workspace_outline_tree(theme, strings, &editor, cx),
        };

        Some(
            div()
                .id("workspace-panel")
                .debug_selector(|| "workspace-panel".to_string())
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
                .child(
                    div()
                        .id("workspace-panel-scroll")
                        .when_some(
                            (self.workspace.active_tab == WorkspaceTab::Files)
                                .then(|| self.workspace.root.clone()).flatten(),
                            |element, root| workspace_tree_drop_target(element, root, &editor, None),
                        )
                        .track_focus(&tree_focus)
                        .on_key_down(cx.listener(Self::on_workspace_tree_key_down))
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
        &mut self,
        theme: &Theme,
        strings: &I18nStrings,
        editor: &WeakEntity<Editor>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.workspace.outline_tree.is_empty() {
            return self.render_workspace_empty_state("", &strings.workspace_empty_outline, theme);
        }
        let rows = self.workspace_outline_rows();
        let total = rows.len();
        let reveal = self.workspace.outline_reveal_pending
            && self.workspace.tree_scroll_handle.bounds().size.height > px(0.0);
        if reveal
            && let Some(WorkspaceSelection::Outline(selected)) = &self.workspace.selected
            && let Some(index) = rows.iter().position(|(node, _)| &node.id == selected)
        {
            self.reveal_workspace_row(index, total, theme);
        }
        let window = self.workspace_list_window(total, theme);
        let visible = rows[window.clone()]
            .iter()
            .map(|(node, depth)| self.render_workspace_node(node, *depth, theme, editor))
            .collect();
        if reveal {
            self.workspace.outline_reveal_pending = false;
        }
        self.workspace_windowed_body(total, window, visible, theme, cx)
    }

    pub(super) fn reveal_workspace_row(&self, index: usize, total: usize, theme: &Theme) -> bool {
        let height = f32::from(self.workspace.tree_scroll_handle.bounds().size.height);
        if height <= 0.0 {
            return false;
        }
        // 虚拟化后行可能还未挂载，定位必须用完整可见列表的行号与同一份行高。
        let row_height = Self::workspace_node_height(theme);
        let offset = self.workspace.tree_scroll_handle.offset();
        let current = f32::from(offset.y);
        let top = WORKSPACE_PANEL_PADDING_Y + index as f32 * row_height;
        let bottom = top + row_height;
        let target = if top + current < WORKSPACE_PANEL_PADDING_Y {
            WORKSPACE_PANEL_PADDING_Y - top
        } else if bottom + current > height - WORKSPACE_PANEL_PADDING_Y {
            height - WORKSPACE_PANEL_PADDING_Y - bottom
        } else {
            current
        };
        let maximum =
            (total as f32 * row_height + WORKSPACE_PANEL_PADDING_Y * 2.0 - height).max(0.0);
        self.workspace.tree_scroll_handle
            .set_offset(point(offset.x, px(target.clamp(-maximum, 0.0))));
        true
    }

    /// 视口内的行区间：行高固定，从滚动偏移直接除得出来。大纲与文件树共用。
    pub(crate) fn workspace_node_height(theme: &Theme) -> f32 {
        theme.typography.ui_text_size(WORKSPACE_NODE_HEIGHT)
            .max(WORKSPACE_NODE_HEIGHT)
    }

    pub(crate) fn workspace_list_window(&self, total: usize, theme: &Theme) -> Range<usize> {
        if total <= PANEL_WINDOW_THRESHOLD_ROWS {
            return 0..total;
        }
        let viewport_height = f32::from(self.workspace.tree_scroll_handle.bounds().size.height);
        if viewport_height <= 0.0 {
            return 0..PANEL_WINDOW_FALLBACK_ROWS.min(total);
        }
        // gpui 的内容偏移是**已滚过距离的相反数**（向下滚只会落在 `[-max, 0]`，
        // 见 `workspace/tests/sidebar.rs` 用真滚轮断言的 `offset < 0`）。拿它当正
        // 距离用，`max(0.0)` 会把窗口永远钉在第 0 行：超过 200 行的列表滚下去就
        // 只剩上下垫的空白（闸门 `outline_panel_renders_only_the_rows_in_the_viewport`
        // 用负偏移钉住这一点，注入正值是物理上到不了的状态）。
        let scrolled =
            (-f32::from(self.workspace.tree_scroll_handle.offset().y) - WORKSPACE_PANEL_PADDING_Y)
                .max(0.0);
        let row_height = Self::workspace_node_height(theme);
        let visible = (viewport_height / row_height).ceil() as usize + 1;
        // 滚轮偏移在布局前可能尚未钳制，先限制到最后一屏，避免切片越界或少铺行。
        let first = ((scrolled / row_height) as usize).min(total.saturating_sub(visible));
        let start = first.saturating_sub(PANEL_WINDOW_OVERDRAW_ROWS);
        let end = (first + visible + PANEL_WINDOW_OVERDRAW_ROWS).min(total);
        start..end
    }

    /// 面板列表的通用外壳：上下垫等高空白（滚动条长度与位置仍按整张表算）、记两个
    /// 计数器、首帧量不到视口尺寸时排一帧补齐。调用方只管取窗口并只建窗口里的行，
    /// 行高必须与 `workspace_node_height` 一致，否则窗口切不出准确边界。
    pub(crate) fn workspace_windowed_body(
        &mut self,
        total: usize,
        window: Range<usize>,
        rows: Vec<AnyElement>,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.panel_rows_rendered.set(rows.len() as u64);
        self.panel_first_row_rendered.set(window.start as u64);
        let needs_fill = total > PANEL_WINDOW_THRESHOLD_ROWS
            && f32::from(self.workspace.tree_scroll_handle.bounds().size.height) <= 0.0;
        let mut elements: Vec<AnyElement> = Vec::with_capacity(rows.len() + 2);
        // 上下各垫一段等高空白：滚动条长度与位置仍按整张表算，中间只挂窗口里的行。
        if window.start > 0 {
            elements.push(
                div()
                    .h(px(window.start as f32 * Self::workspace_node_height(theme)))
                    .flex_shrink_0()
                    .into_any_element(),
            );
        }
        elements.extend(rows);
        let below = total.saturating_sub(window.end);
        if below > 0 {
            elements.push(
                div()
                    .h(px(below as f32 * Self::workspace_node_height(theme)))
                    .flex_shrink_0()
                    .into_any_element(),
            );
        }
        let element = div()
            .w_full()
            .flex()
            .flex_col()
            .children(elements)
            .into_any_element();
        // 首帧还没量过滚动视口：先铺一小段，立刻排下一帧补齐（帧数封顶，量到尺寸即停）。
        if needs_fill && self.panel_fill_frames < PANEL_FILL_MAX_FRAMES {
            self.panel_fill_frames += 1;
            self.schedule_followup_frame(cx);
        } else if !needs_fill {
            self.panel_fill_frames = 0;
        }
        element
    }

    /// 展开状态下的标题树按行摊平：`(节点, 深度)` 的顺序就是屏幕顺序。
    fn workspace_outline_rows(&self) -> Vec<(&WorkspaceTreeNode, usize)> {
        fn walk<'a>(
            nodes: &'a [WorkspaceTreeNode],
            depth: usize,
            expanded: &HashSet<String>,
            rows: &mut Vec<(&'a WorkspaceTreeNode, usize)>,
        ) {
            for node in nodes {
                rows.push((node, depth));
                if !node.children.is_empty() && expanded.contains(&node.id) {
                    walk(&node.children, depth + 1, expanded, rows);
                }
            }
        }
        let mut rows = Vec::new();
        walk(
            &self.workspace.outline_tree,
            0,
            &self.workspace.expanded,
            &mut rows,
        );
        rows
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
                .text_size(px(t.dialog_body_size))
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
                    .text_size(px(t.dialog_body_size * 0.9))
                    .line_height(px(t.dialog_body_size * t.text_line_height))
                    .text_color(c.dialog_muted)
                    .child(message.to_string()),
            )
            .into_any_element()
    }

    /// 文件树按行摊平：顺序就是屏幕顺序。名称输入行与懒加载占位行也各占一行，
    /// 三者同高，窗口才能按固定行高切（见 `workspace_list_window`）。
    pub(crate) fn workspace_files_rows(&self) -> Vec<WorkspaceTreeRow<'_>> {
        fn walk<'a>(
            nodes: &'a [WorkspaceTreeNode],
            depth: usize,
            workspace: &WorkspaceState,
            rows: &mut Vec<WorkspaceTreeRow<'a>>,
        ) {
            for node in nodes {
                rows.push(WorkspaceTreeRow::Node(node, depth));
                let is_expanded = workspace.expanded.contains(&node.id);
                let name_edit = is_expanded
                    && workspace.name_edit.as_ref().is_some_and(|edit| {
                        !matches!(edit.kind, WorkspaceEditKind::Rename { .. })
                            && tree_node_path(node) == edit.directory.as_path()
                    });
                if is_expanded && node.kind_dir() && !node.children_loaded {
                    // 展开的这一层还没扫到（懒加载）：先挂一行占位，扫描落地后换成子项。
                    rows.push(WorkspaceTreeRow::Loading(depth + 1));
                }
                if is_expanded {
                    let insertion = if name_edit {
                        node.children.partition_point(WorkspaceTreeNode::kind_dir)
                    } else {
                        node.children.len()
                    };
                    let (before, after) = node.children.split_at(insertion);
                    walk(before, depth + 1, workspace, rows);
                    if name_edit {
                        rows.push(WorkspaceTreeRow::NameEdit(depth + 1));
                    }
                    walk(after, depth + 1, workspace, rows);
                }
            }
        }
        let mut rows = Vec::new();
        if let Some(root) = self.workspace.file_tree.as_ref() {
            walk(std::slice::from_ref(root), 0, &self.workspace, &mut rows);
        }
        rows
    }

    /// 摊平出来的一行变成元素。
    pub(crate) fn render_workspace_tree_row(
        &self,
        row: &WorkspaceTreeRow<'_>,
        theme: &Theme,
        editor: &WeakEntity<Editor>,
    ) -> AnyElement {
        match row {
            WorkspaceTreeRow::Node(node, depth) => {
                self.render_workspace_node(node, *depth, theme, editor)
            }
            WorkspaceTreeRow::NameEdit(depth) => {
                self.render_workspace_name_row(*depth, theme, editor)
            }
            WorkspaceTreeRow::Loading(depth) => div()
                .w_full()
                .h(px(Self::workspace_node_height(theme)))
                .pl(px(6.0 + *depth as f32 * WORKSPACE_NODE_INDENT))
                .text_size(px(theme.typography.ui_text_size(14.0)))
                .text_color(theme.colors.dialog_muted)
                .child("…")
                .into_any_element(),
        }
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
        // 未加载目录也需展开入口，大纲标题有下级时同样必须提供折叠箭头。
        let has_children = !node.children.is_empty() || (node.kind_dir() && !node.children_loaded);
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
            (Some(WorkspaceSelection::File(selected)), WorkspaceTreeKind::OtherFile(path)) => selected == path,
            (Some(WorkspaceSelection::Outline(selected)), _) => selected == &node.id,
            _ => false,
        };
        let node_id = node.id.clone();
        let node_selector = format!("workspace-node-{}", stable_node_hash(&node.id));
        if self.workspace.name_edit.as_ref().is_some_and(|edit| {
            matches!(&edit.kind, WorkspaceEditKind::Rename { source, .. }
                if tree_node_path(node) == source.as_path())
        }) {
            return self.render_workspace_name_row(depth, theme, editor);
        }
        let click_editor = editor.clone();
        let click_kind = node.kind.clone();
        let drag_path = match &node.kind {
            WorkspaceTreeKind::MarkdownFile(path)
            | WorkspaceTreeKind::CodeFile(path)
            | WorkspaceTreeKind::OtherFile(path) => Some(path.clone()),
            _ => None,
        };
        let directory_drag = match &node.kind {
            WorkspaceTreeKind::Directory(path) if self.workspace.root.as_ref() != Some(path) => Some(path.clone()),
            _ => None,
        };
        let drop_directory = match &node.kind {
            WorkspaceTreeKind::Directory(path) => Some(path.clone()),
            WorkspaceTreeKind::MarkdownFile(path) | WorkspaceTreeKind::CodeFile(path) | WorkspaceTreeKind::OtherFile(path) => path.parent().map(Path::to_path_buf),
            WorkspaceTreeKind::Heading { .. } => None,
        };
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
            WorkspaceTreeKind::CodeFile(path) | WorkspaceTreeKind::OtherFile(path)
                if crate::components::Block::has_supported_image_extension(path) =>
            {
                Some((IMAGE_ICON, c.dialog_muted))
            }
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

        // 文件节点悬停显示 大小 · 修改时间（roadmap D7）。这份提示里那次
        // `fs::metadata` 只有真的悬停才该付：放在行构造期就是每个可见文件节点每帧
        // 一次 stat（用户可能从不悬停），所以这里只搬两样字段、把 stat 留在闭包里。
        let tooltip_facts = TreeTooltipFacts::from_node(&node);
        div()
            .id(("workspace-node", stable_node_hash(&node.id)))
            .debug_selector(move || node_selector.clone())
            .h(px(Self::workspace_node_height(theme)))
            .w_full()
            .overflow_hidden()
            .flex()
            .items_center()
            .gap(px(4.0))
            .pl(px(6.0 + depth as f32 * WORKSPACE_NODE_INDENT))
            .pr(px(6.0))
            .rounded(px(4.0))
            .when(!node.kind_dir() && self.workspace.context_menu.is_none() && self.workspace.name_edit.is_none(), |this| this.tooltip(move |_, cx| {
                let label = tree_node_tooltip(&tooltip_facts);
                cx.new(|_| WorkspaceTooltip { label }).into()
            }))
            .bg(if selected {
                c.selection
            } else {
                hsla(0.0, 0.0, 0.0, 0.0)
            })
            .hover(|this| this.bg(c.dialog_secondary_button_hover))
            .cursor_pointer()
            .when_some(drag_path, |row, path| {
                row.on_drag(EditorFileDrag { from_path: path, source: None }, |drag, _, _, cx| {
                    let label = file_label(&drag.from_path);
                    cx.new(|_| DraggedTabPreview { label: label.into() })
                })
            })
            .when_some(directory_drag, |row, path| {
                row.on_drag(WorkspaceDirectoryDrag { path }, |drag, _, _, cx| {
                    let label = file_label(&drag.path);
                    cx.new(|_| DraggedTabPreview { label: label.into() })
                })
            })
            .when_some(drop_directory, |row, directory| {
                workspace_tree_drop_target(row, directory, editor, Some(c.selection))
            })
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
                    .text_size(px(theme.typography.ui_text_size(12.0)))
                    .line_height(px(theme.typography.ui_text_size(18.0)))
                    .text_color(label_color)
                    .child(node.label.clone()),
            )
            .on_mouse_down(MouseButton::Right, move |event, window, cx| {
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
                    editor.focus_workspace_tree(window, cx);
                });
                cx.stop_propagation();
            })
            .on_click(move |event, window, cx| {
                if !event.standard_click() {
                    return;
                }
                let node_id = node_id.clone();
                let click_kind = click_kind.clone();
                let _ = click_editor.update(cx, |editor, cx| {
                    let is_file_tree = !matches!(click_kind, WorkspaceTreeKind::Heading { .. });
                    match click_kind {
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
                    }
                    if is_file_tree && event.click_count() < 2 { editor.focus_workspace_tree(window, cx); }
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

pub(crate) use is_markdown_document as is_markdown_file;

/// 是否按 Markdown 文档打开：仅 `.md` / `.markdown`。所有打开入口（工作区树、
/// 拖拽、命令行）必须共用这一条判定，否则同一文件两条入口行为不一致。
pub(crate) use crate::file_url::is_markdown_document;

pub(crate) fn create_workspace_file(path: &Path) -> std::io::Result<()> {
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(path)?;
    let template = crate::config::EditorSettings::new_file_template();
    if is_markdown_document(path) && !template.is_empty() {
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
    // 空 suffix 就是被移动项自身：`join("")` 会补一个尾分隔符（`…/new.md/`），
    // 此后读这个路径直接 ENOTDIR——文件路径必须原样是 destination。
    if suffix.as_os_str().is_empty() {
        return Some(destination.to_path_buf());
    }
    Some(destination.join(suffix))
}

pub(crate) fn path_is_affected(path: &Path, target: &Path, target_is_directory: bool) -> bool {
    if target_is_directory {
        path.starts_with(target)
    } else {
        path == target
    }
}

fn workspace_tree_drop_target<T: InteractiveElement>(
    element: T,
    directory: PathBuf,
    editor: &WeakEntity<Editor>,
    highlight: Option<Hsla>,
) -> T {
    let file_editor = editor.clone();
    let directory_editor = editor.clone();
    let external_editor = editor.clone();
    let file_target = directory.clone();
    let directory_target = directory.clone();
    let external_target = directory.clone();
    let file_hover_target = directory.clone();
    element
        .can_drop(|drag, _, _| {
            drag.is::<EditorFileDrag>()
                || drag.is::<WorkspaceDirectoryDrag>()
                || drag.is::<ExternalPaths>()
        })
        .drag_over::<EditorFileDrag>(move |style, drag, _, _| {
            if drag.from_path.parent() == Some(file_hover_target.as_path()) {
                style
            } else if let Some(color) = highlight {
                style.bg(color)
            } else {
                style
            }
        })
        .drag_over::<WorkspaceDirectoryDrag>(move |style, drag, _, _| {
            if directory.starts_with(&drag.path) || drag.path.parent() == Some(directory.as_path())
            {
                style
            } else if let Some(color) = highlight {
                style.bg(color)
            } else {
                style
            }
        })
        .drag_over::<ExternalPaths>(move |style, _, _, _| {
            if let Some(color) = highlight {
                style.bg(color)
            } else {
                style
            }
        })
        .on_drop(move |drag: &EditorFileDrag, window, cx| {
            cx.stop_propagation();
            if let Err(error) = file_editor.update(cx, |editor, cx| {
                let move_source = editor
                    .workspace
                    .root
                    .as_ref()
                    .is_some_and(|root| drag.from_path.starts_with(root));
                editor.receive_workspace_tree_drop(
                    vec![drag.from_path.clone()],
                    file_target.clone(),
                    move_source,
                    window,
                    cx,
                );
            }) {
                eprintln!("移动拖入文件树的文件失败：{error}");
            }
        })
        .on_drop(move |drag: &WorkspaceDirectoryDrag, window, cx| {
            cx.stop_propagation();
            if let Err(error) = directory_editor.update(cx, |editor, cx| {
                let move_source = editor
                    .workspace
                    .root
                    .as_ref()
                    .is_some_and(|root| drag.path.starts_with(root));
                editor.receive_workspace_tree_drop(
                    vec![drag.path.clone()],
                    directory_target.clone(),
                    move_source,
                    window,
                    cx,
                );
            }) {
                eprintln!("移动拖入文件树的目录失败：{error}");
            }
        })
        .on_drop(move |paths: &ExternalPaths, window, cx| {
            cx.stop_propagation();
            if let Err(error) = external_editor.update(cx, |editor, cx| {
                editor.receive_workspace_tree_drop(
                    paths.paths().to_vec(),
                    external_target.clone(),
                    false,
                    window,
                    cx,
                );
            }) {
                eprintln!("导入拖入文件树的路径失败：{error}");
            }
        })
}
