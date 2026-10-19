use super::render_panel::workspace_tree_drop_target;
use super::*;
use gpui::prelude::FluentBuilder as _;
use gpui_kit::assets::IconName;
use gpui_kit::base::ElementExt as _;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{
    ActiveTheme as _, Sizable as _,
    button::{Button, ButtonVariants},
    list::ListItem,
};

impl Editor {
    /// 单击 = 预览打开，双击 = 固定打开（用户需求）；键盘触发的点击按固定处理。
    pub(crate) fn tree_click_open_mode(event: &ClickEvent) -> WorkspaceOpenMode {
        match event {
            ClickEvent::Mouse(mouse) if mouse.up.click_count >= 2 => WorkspaceOpenMode::Pinned,
            ClickEvent::Mouse(_) => WorkspaceOpenMode::Preview,
            ClickEvent::Keyboard(_) => WorkspaceOpenMode::Pinned,
            ClickEvent::Touch(_) => WorkspaceOpenMode::Preview,
        }
    }

    pub(super) fn render_workspace_folder_icon(theme: &Theme, cx: &App) -> AnyElement {
        let icon = match crate::config::EditorSettings::folder_icon_style(cx) {
            crate::config::FolderIconStyle::Blue => gpui_kit::component::Icon::default()
                .path("icon/workspace/folder.svg")
                .text_color(theme.colors.workspace_folder_icon),
            crate::config::FolderIconStyle::Outline => {
                gpui_kit::component::Icon::new(IconName::Folder)
                    .text_color(theme.colors.dialog_muted)
            }
        };
        icon.size_4().flex_none().into_any_element()
    }

    pub(crate) fn render_workspace_files_tree(
        &mut self,
        theme: &Theme,
        strings: &I18nStrings,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.workspace.root.is_none() {
            return self.render_workspace_empty_state(
                &strings.workspace_no_file_title,
                &strings.workspace_no_file_message,
                theme,
            );
        }

        if let Some(error) = self.workspace.file_error.as_ref() {
            return self.render_workspace_empty_state(
                &strings.workspace_scan_failed_title,
                error,
                theme,
            );
        }

        if self.workspace.file_tree.is_none() {
            // 后台扫描进行中（roadmap D9）：与搜索面板一致显示处理中占位，
            // 而不是误报「空文件夹」。
            if self.workspace.tree_scan_root.is_some()
                && self.workspace.tree_scan_root.as_ref() == self.workspace.root.as_ref()
            {
                return div()
                    .id("workspace-tree-loading")
                    .role(Role::ProgressIndicator)
                    .aria_label("Loading files")
                    .p_3()
                    .text_size(px(theme.typography.ui_text_size(14.0)))
                    .text_color(theme.colors.dialog_muted)
                    .child(gpui_kit::component::spinner::Spinner::new())
                    .into_any_element();
            }
            return self.render_workspace_empty_state("", &strings.workspace_empty_files, theme);
        }

        let rows = self.workspace_files_rows();
        let total = rows.len();
        let revealed_row = self.workspace.name_edit.as_ref().and_then(|edit| {
            rows.iter()
                .position(|row| matches!(row, WorkspaceTreeRow::NameEdit(_)))
                .filter(|index| {
                    edit.revealed_row != Some(*index)
                        && self.reveal_workspace_row(*index, total, theme)
                })
        });
        if let Some(index) = revealed_row
            && let Some(edit) = self.workspace.name_edit.as_mut()
        {
            edit.revealed_row = Some(index);
        }
        let name_row = self.workspace_name_row_index();
        self.render_workspace_tree_list(total, name_row, theme, cx)
    }
    pub(super) fn workspace_name_row_index(&self) -> Option<usize> {
        let edit = self.workspace.name_edit.as_ref()?;
        self.workspace_files_rows().iter().position(|row| match row {
            WorkspaceTreeRow::NameEdit(_) => true,
            WorkspaceTreeRow::Node(node, _) => matches!(&edit.kind,
                WorkspaceEditKind::Rename { source, .. } if tree_node_path(node) == source.as_path()),
            WorkspaceTreeRow::Loading(..) => false,
        })
    }

    pub(super) fn render_workspace_tree_list(
        &mut self,
        total: usize,
        name_index: Option<usize>,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
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
        let list = gpui_kit::component::v_virtual_list(
            cx.entity(),
            "workspace-tree-items",
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
                    WorkspaceTab::Search => Vec::new(),
                };
                editor.panel_rows_rendered.set(items.len() as u64);
                editor.panel_first_row_rendered.set(range.start as u64);
                items
            },
        )
        .track_scroll(&handle)
        .overflow_x_hidden()
        .px(px(4.0))
        .py(px(WORKSPACE_PANEL_PADDING_Y));
        div()
            .id("workspace-tree-viewport")
            .test_support()
            .role(Role::Group)
            .size_full()
            .min_h_0()
            .child(list)
            .vertical_scrollbar(&handle)
            .into_any_element()
    }

    fn render_workspace_measured_name_row(
        &self,
        depth: usize,
        theme: &Theme,
        editor: &WeakEntity<Editor>,
        cx: &App,
    ) -> AnyElement {
        let row = self.render_workspace_name_row(depth, theme, editor, cx);
        let owner = editor.clone();
        let focus = self
            .workspace
            .name_edit
            .as_ref()
            .map(|edit| edit.focus.clone());
        div()
            .id("workspace-name-row")
            .w_full()
            .child(row)
            .on_prepaint(move |bounds, _, cx| {
                if let Err(error) = owner.update(cx, |editor, cx| {
                    if let Some(edit) = editor.workspace.name_edit.as_mut()
                        && focus.as_ref() == Some(&edit.focus)
                        && bounds.size.height > px(0.0)
                        && edit.row_height != Some(bounds.size.height)
                    {
                        edit.row_height = Some(bounds.size.height);
                        if edit.error.is_some() {
                            edit.revealed_row = None;
                        }
                        cx.notify();
                    }
                }) {
                    eprintln!("测量文件名输入行失败：{error}");
                }
            })
            .into_any_element()
    }

    pub(super) fn navigate_workspace_tree(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        if !matches!(key, "up" | "down" | "left" | "right" | "home" | "end") {
            return false;
        }
        enum Movement {
            Toggle(String),
            Select(WorkspaceSelection, usize),
        }
        let movement = {
            let nodes: Vec<(usize, &WorkspaceTreeNode, usize)> = match self.workspace.active_tab {
                WorkspaceTab::Files => self
                    .workspace_files_rows()
                    .into_iter()
                    .enumerate()
                    .filter_map(|(index, row)| match row {
                        WorkspaceTreeRow::Node(node, depth) => Some((index, node, depth)),
                        _ => None,
                    })
                    .collect(),
                WorkspaceTab::Outline => self
                    .workspace_outline_rows()
                    .into_iter()
                    .enumerate()
                    .map(|(index, (node, depth))| (index, node, depth))
                    .collect(),
                WorkspaceTab::Search => return false,
            };
            let selected = nodes.iter().position(|(_, node, _)| {
                node.matches_workspace_selection(self.workspace.selected.as_ref())
            });
            let position = selected.unwrap_or(0);
            let Some((_, current, depth)) = nodes.get(position) else {
                return true;
            };
            let expandable = current.kind_dir() || !current.children.is_empty();
            let expanded = self.workspace.expanded.contains(&current.id);
            let target = match key {
                "home" => Some(0),
                "end" => Some(nodes.len().saturating_sub(1)),
                "up" => Some(position.saturating_sub(1)),
                "down" => Some(if selected.is_none() {
                    0
                } else {
                    (position + 1).min(nodes.len().saturating_sub(1))
                }),
                "left" if expanded && expandable => None,
                "left" => nodes.get(..position).and_then(|before| {
                    before
                        .iter()
                        .rposition(|(_, _, parent_depth)| parent_depth < depth)
                }),
                "right" if !expanded && expandable => None,
                "right" => nodes
                    .get(position + 1)
                    .filter(|(_, _, child_depth)| child_depth > depth)
                    .map(|_| position + 1),
                _ => None,
            };
            if (key == "left" && expanded && expandable)
                || (key == "right" && !expanded && expandable)
            {
                Some(Movement::Toggle(current.id.clone()))
            } else {
                target
                    .and_then(|index| nodes.get(index))
                    .map(|(index, node, _)| Movement::Select(node.workspace_selection(), *index))
            }
        };
        match movement {
            Some(Movement::Toggle(id)) => self.toggle_workspace_node(&id, cx),
            Some(Movement::Select(selection, index)) => {
                self.workspace.selected = Some(selection);
                if let Some(handle) = self.workspace.tree_virtual_scroll.as_ref() {
                    handle.scroll_to_item(index, ScrollStrategy::Nearest);
                }
                cx.notify();
            }
            None => {}
        }
        true
    }

    /// 摊平出来的一行变成元素。
    pub(crate) fn render_workspace_tree_row(
        &self,
        row: &WorkspaceTreeRow<'_>,
        theme: &Theme,
        editor: &WeakEntity<Editor>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match row {
            WorkspaceTreeRow::Node(node, depth) => {
                self.render_workspace_node(node, *depth, theme, editor, cx)
            }
            WorkspaceTreeRow::NameEdit(depth) => {
                self.render_workspace_measured_name_row(*depth, theme, editor, cx)
            }
            WorkspaceTreeRow::Loading(id, depth) => div()
                .id(("workspace-tree-loading", stable_node_hash(id)))
                .role(Role::ProgressIndicator)
                .aria_label("Loading folder")
                .flex()
                .items_center()
                .w_full()
                .h(px(Self::workspace_node_height(theme)))
                .pl(Self::workspace_tree_indent(*depth))
                .text_size(px(theme.typography.ui_text_size(14.0)))
                .text_color(theme.colors.dialog_muted)
                .child(gpui_kit::component::spinner::Spinner::new().small())
                .into_any_element(),
        }
    }

    pub(crate) fn render_workspace_node(
        &self,
        node: &WorkspaceTreeNode,
        depth: usize,
        theme: &Theme,
        editor: &WeakEntity<Editor>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.workspace.name_edit.as_ref().is_some_and(|edit| {
            matches!(&edit.kind, WorkspaceEditKind::Rename { source, .. }
                if tree_node_path(node) == source.as_path())
        }) {
            return self.render_workspace_measured_name_row(depth, theme, editor, cx);
        }
        let expanded = self.workspace.expanded.contains(&node.id);
        let expandable = node.kind_dir() || !node.children.is_empty();
        let selected = node.matches_workspace_selection(self.workspace.selected.as_ref());
        let hash = stable_node_hash(&node.id);
        let disclosure = if expandable {
            let id = node.id.clone();
            Button::new(("workspace-node-toggle", hash))
                .xsmall()
                .ghost()
                .tab_stop(false)
                .icon(if expanded {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                })
                .accessibility_label(format!(
                    "{} {}",
                    if expanded { "Collapse" } else { "Expand" },
                    node.label
                ))
                .on_click(cx.listener(move |editor, _, window, cx| {
                    editor.toggle_workspace_node(&id, cx);
                    editor.focus_workspace_tree(window, cx);
                    cx.stop_propagation();
                }))
                .into_any_element()
        } else {
            div().size_5().flex_none().into_any_element()
        };
        let icon = match &node.kind {
            WorkspaceTreeKind::Directory(_) => Some(Self::render_workspace_folder_icon(theme, cx)),
            WorkspaceTreeKind::Heading { .. } => None,
            kind => {
                let path = match kind {
                    WorkspaceTreeKind::MarkdownFile(_) => MARKDOWN_ICON,
                    WorkspaceTreeKind::CodeFile(path) | WorkspaceTreeKind::OtherFile(path)
                        if crate::components::Block::has_supported_image_extension(path) =>
                    {
                        IMAGE_ICON
                    }
                    WorkspaceTreeKind::CodeFile(_) => CODE_ICON,
                    _ => GENERIC_FILE_ICON,
                };
                Some(
                    svg()
                        .path(path)
                        .size_4()
                        .flex_none()
                        .text_color(if matches!(kind, WorkspaceTreeKind::MarkdownFile(_)) {
                            theme.colors.dialog_primary_button_bg
                        } else {
                            theme.colors.dialog_muted
                        })
                        .into_any_element(),
                )
            }
        };
        let content = div()
            .flex()
            .items_center()
            .w_full()
            .min_w_0()
            .gap_1()
            .pl(Self::workspace_tree_indent(depth))
            .pr_2()
            .child(disclosure)
            .children(icon)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .when(
                        matches!(node.kind, WorkspaceTreeKind::Heading { .. }),
                        |label| {
                            label.text_xs().text_color(if selected {
                                theme.colors.text_default
                            } else {
                                theme.colors.dialog_muted
                            })
                        },
                    )
                    .when(
                        !matches!(node.kind, WorkspaceTreeKind::Heading { .. }),
                        |label| label.text_sm(),
                    )
                    .child(node.label.clone()),
            );
        let click_kind = node.kind.clone();
        let click_id = node.id.clone();
        let context_kind = node.kind.clone();
        let facts = TreeTooltipFacts::from_node(node);
        let mut row = ListItem::new(("workspace-node", hash))
            .debug_selector(move || format!("workspace-node-{hash}"))
            .role(Role::TreeItem)
            .aria_level(depth + 1)
            .aria_selected(selected)
            .accessibility_label(node.label.clone())
            .when(expandable, |row| row.aria_expanded(expanded))
            .selected(selected)
            .h(px(Self::workspace_node_height(theme)))
            .w_full()
            .py_0()
            .px_0()
            .rounded(cx.theme().radius)
            .when(
                !node.kind_dir()
                    && self.workspace.context_menu.is_none()
                    && self.workspace.name_edit.is_none(),
                |row| {
                    row.tooltip(move |_, cx| {
                        cx.new(|_| WorkspaceTooltip {
                            label: tree_node_tooltip(&facts),
                        })
                        .into()
                    })
                },
            )
            .child(content)
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |editor, event: &MouseDownEvent, window, cx| {
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
                    editor.open_workspace_context_menu(event.position, selection, cx);
                    editor.focus_workspace_tree(window, cx);
                    cx.stop_propagation();
                }),
            )
            .on_click(cx.listener(move |editor, event: &ClickEvent, window, cx| {
                if !event.standard_click() {
                    return;
                }
                let file_tree = !matches!(click_kind, WorkspaceTreeKind::Heading { .. });
                match &click_kind {
                    WorkspaceTreeKind::Directory(path) => {
                        editor.workspace.selected =
                            Some(WorkspaceSelection::Directory(path.clone()));
                        editor.toggle_workspace_node(&click_id, cx);
                    }
                    WorkspaceTreeKind::MarkdownFile(path)
                    | WorkspaceTreeKind::CodeFile(path)
                    | WorkspaceTreeKind::OtherFile(path) => {
                        editor.open_workspace_file_in_mode(
                            path.clone(),
                            Self::tree_click_open_mode(event),
                            window,
                            cx,
                        );
                    }
                    WorkspaceTreeKind::Heading { line, .. } => {
                        if event.click_count() >= 2 {
                            editor.rename_outline_heading(*line, cx);
                        } else {
                            editor.open_outline_node(click_id.clone(), *line, cx);
                        }
                    }
                }
                if file_tree && event.click_count() < 2 {
                    editor.focus_workspace_tree(window, cx);
                }
            }));
        match &node.kind {
            WorkspaceTreeKind::MarkdownFile(path)
            | WorkspaceTreeKind::CodeFile(path)
            | WorkspaceTreeKind::OtherFile(path) => {
                let path = path.clone();
                row = row.on_drag(
                    EditorFileDrag {
                        from_path: path,
                        source: None,
                    },
                    |drag, _, _, cx| {
                        cx.new(|_| DraggedTabPreview {
                            label: file_label(&drag.from_path).into(),
                        })
                    },
                );
            }
            WorkspaceTreeKind::Directory(path) if self.workspace.root.as_ref() != Some(path) => {
                row = row.on_drag(
                    WorkspaceDirectoryDrag { path: path.clone() },
                    |drag, _, _, cx| {
                        cx.new(|_| DraggedTabPreview {
                            label: file_label(&drag.path).into(),
                        })
                    },
                );
            }
            _ => {}
        }
        let drop_directory = match &node.kind {
            WorkspaceTreeKind::Directory(path) => Some(path.clone()),
            WorkspaceTreeKind::Heading { .. } => None,
            _ => tree_node_path(node).parent().map(Path::to_path_buf),
        };
        if let Some(directory) = drop_directory {
            row = workspace_tree_drop_target(row, directory, editor, Some(theme.colors.selection));
        }
        row.into_any_element()
    }
}
