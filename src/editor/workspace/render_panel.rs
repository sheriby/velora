use super::*;
use gpui::prelude::FluentBuilder;
use gpui_kit::component::{ThemeStyled as _, scroll::ScrollableElement as _};

/// 文件树的领域行；名称编辑的实际高度由视图测量，目录占位仍保留父节点身份。
pub(crate) enum WorkspaceTreeRow<'a> {
    Node(&'a WorkspaceTreeNode, usize),
    NameEdit(usize),
    Loading(&'a str, usize),
}

impl Editor {
    pub(crate) fn render_workspace_panel(
        &mut self,
        theme: &Theme,
        strings: &I18nStrings,
        window: &mut Window,
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
        let tree_focus = self
            .workspace
            .tree_focus
            .get_or_insert_with(|| cx.focus_handle())
            .clone();
        let c = &theme.colors;

        let handle = self.workspace.tree_scroll_handle.clone();
        self.workspace
            .tree_virtual_scroll
            .get_or_insert_with(|| handle.into());
        let search_header = (self.workspace.active_tab == WorkspaceTab::Search)
            .then(|| self.render_search_header(theme, strings, window, cx));
        let body = match self.workspace.active_tab {
            WorkspaceTab::Files => self.render_workspace_files_tree(theme, strings, cx),
            WorkspaceTab::Search => self.render_search_results(theme, strings, cx),
            WorkspaceTab::Outline => self.render_workspace_outline_tree(theme, strings, cx),
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
                .w_full()
                .flex()
                .flex_col()
                .flex_shrink_0()
                .bg(c.dialog_secondary_button_bg)
                .children(search_header)
                .child(
                    div()
                        .id("workspace-panel-scroll")
                        .test_support()
                        .role(if self.workspace.active_tab == WorkspaceTab::Search {
                            Role::List
                        } else {
                            Role::Tree
                        })
                        .aria_label(match self.workspace.active_tab {
                            WorkspaceTab::Files => strings.workspace_tab_files.clone(),
                            WorkspaceTab::Outline => strings.workspace_tab_outline.clone(),
                            WorkspaceTab::Search => strings.workspace_tab_search.clone(),
                        })
                        .when_some(
                            (self.workspace.active_tab == WorkspaceTab::Files)
                                .then(|| self.workspace.root.clone())
                                .flatten(),
                            |element, root| {
                                workspace_tree_drop_target(element, root, &editor, None)
                            },
                        )
                        .track_focus(&tree_focus.clone().tab_index(0).tab_stop(true))
                        .on_key_down(cx.listener(Self::on_workspace_tree_key_down))
                        .flex_1()
                        .min_h(px(0.0))
                        .when(
                            tree_focus.is_focused(window) && window.last_input_was_keyboard(),
                            |tree| tree.focus_ring_style(window, cx),
                        )
                        .child(body),
                )
                .into_any_element(),
        )
    }

    pub(crate) fn render_workspace_outline_tree(
        &mut self,
        theme: &Theme,
        strings: &I18nStrings,
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
            self.reveal_workspace_row(index);
        }
        if reveal {
            self.workspace.outline_reveal_pending = false;
        }
        self.render_workspace_virtual_list(total, None, theme, cx)
    }

    pub(super) fn render_workspace_virtual_list(
        &mut self,
        total: usize,
        name_index: Option<usize>,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if total == 0 {
            self.panel_rows_rendered.set(0);
            self.panel_first_row_rendered.set(0);
        }
        let height = px(Self::workspace_node_height(theme));
        let name_row = name_index.map(|index| {
            (
                index,
                self.workspace
                    .name_edit
                    .as_ref()
                    .and_then(|edit| edit.row_height)
                    .unwrap_or(height),
            )
        });
        let layout =
            self.workspace
                .tree_list_layout
                .get_or_insert_with(|| WorkspaceTreeListLayout {
                    count: 0,
                    height,
                    name_row: None,
                    sizes: std::rc::Rc::new(Vec::new()),
                });
        if layout.count != total || layout.height != height || layout.name_row != name_row {
            let mut sizes = vec![size(px(0.0), height); total];
            if let Some((index, row_height)) = name_row
                && let Some(size) = sizes.get_mut(index)
            {
                size.height = row_height;
            }
            *layout = WorkspaceTreeListLayout {
                count: total,
                height,
                name_row,
                sizes: std::rc::Rc::new(sizes),
            };
        }
        let sizes = layout.sizes.clone();
        let base = self.workspace.tree_scroll_handle.clone();
        let handle = self
            .workspace
            .tree_virtual_scroll
            .get_or_insert_with(|| base.into())
            .clone();
        let theme = cx.global::<ThemeManager>().derived_theme(cx);
        let search = self.workspace.active_tab == WorkspaceTab::Search;
        let list = gpui_kit::component::v_virtual_list(
            cx.entity(),
            if search {
                "workspace-search-items"
            } else {
                "workspace-tree-items"
            },
            sizes,
            move |editor, range: Range<usize>, _, cx| {
                let owner = cx.entity().downgrade();
                let items: Vec<AnyElement> = match editor.workspace.active_tab {
                    WorkspaceTab::Files => {
                        let rows = editor.workspace_files_rows();
                        range
                            .clone()
                            .filter_map(|index| rows.get(index))
                            .map(|row| editor.render_workspace_tree_row(row, &theme, &owner, cx))
                            .collect()
                    }
                    WorkspaceTab::Outline => {
                        let rows = editor.workspace_outline_rows();
                        range
                            .clone()
                            .filter_map(|index| rows.get(index))
                            .map(|(node, depth)| {
                                editor.render_workspace_node(node, *depth, &theme, &owner, cx)
                            })
                            .collect()
                    }
                    WorkspaceTab::Search => {
                        editor.render_workspace_search_rows(range.clone(), &theme, cx)
                    }
                };
                editor.panel_rows_rendered.set(items.len() as u64);
                editor.panel_first_row_rendered.set(range.start as u64);
                items
            },
        )
        .track_scroll(&handle)
        .overflow_x_hidden()
        .px_1()
        .py_1p5();
        div()
            .id(if search {
                "workspace-search-viewport"
            } else {
                "workspace-tree-viewport"
            })
            .test_support()
            .role(Role::Group)
            .size_full()
            .min_h_0()
            .child(list)
            .vertical_scrollbar(&handle)
            .into_any_element()
    }

    pub(super) fn reveal_workspace_row(&self, index: usize) -> bool {
        if let Some(handle) = self.workspace.tree_virtual_scroll.as_ref() {
            handle.scroll_to_item(index, ScrollStrategy::Nearest);
            true
        } else {
            false
        }
    }

    pub(super) fn workspace_tree_indent(depth: usize) -> Rems {
        rems(0.25 + depth as f32)
    }

    pub(crate) fn workspace_node_height(theme: &Theme) -> f32 {
        theme
            .typography
            .ui_text_size(WORKSPACE_NODE_HEIGHT)
            .max(WORKSPACE_NODE_HEIGHT)
    }

    /// 展开状态下的标题树按行摊平：`(节点, 深度)` 的顺序就是屏幕顺序。
    pub(super) fn workspace_outline_rows(&self) -> Vec<(&WorkspaceTreeNode, usize)> {
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
    /// 名称编辑的实际高度由原生虚拟列表测量。
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
                if is_expanded && node.kind_dir() && !node.children_loaded && !node.children_load_failed {
                    // 展开的这一层还没扫到（懒加载）：先挂一行占位，扫描落地后换成子项。
                    rows.push(WorkspaceTreeRow::Loading(&node.id, depth + 1));
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

pub(super) fn workspace_tree_drop_target<T: InteractiveElement>(
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
