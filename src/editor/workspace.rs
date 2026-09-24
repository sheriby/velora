//! Lightweight workspace panel state, file-tree scanning, and outline parsing.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use gpui::*;

use super::{BlockKind, Editor, code_viewer};
use crate::i18n::I18nStrings;
use crate::theme::Theme;

const FOLDER_ICON: &str = "icon/workspace/folder.svg";
const MARKDOWN_ICON: &str = "icon/workspace/markdown.svg";
const CODE_ICON: &str = "icon/workspace/code.svg";
const WORKSPACE_PANEL_TARGET_RATIO: f32 = 0.15;
const WORKSPACE_PANEL_MIN_WIDTH: f32 = 240.0;
const WORKSPACE_PANEL_MAX_WIDTH: f32 = 360.0;
const WORKSPACE_NODE_HEIGHT: f32 = 28.0;
const WORKSPACE_NODE_INDENT: f32 = 18.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum WorkspaceTab {
    #[default]
    Files,
    Outline,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum WorkspaceTreeKind {
    Directory(PathBuf),
    MarkdownFile(PathBuf),
    CodeFile(PathBuf),
    Heading { line: usize, level: u8 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct WorkspaceTreeNode {
    id: String,
    label: String,
    kind: WorkspaceTreeKind,
    children: Vec<WorkspaceTreeNode>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct WorkspaceDocumentTab {
    path: PathBuf,
    markdown: String,
    dirty: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum WorkspaceSelection {
    File(PathBuf),
    Outline(String),
}

pub(super) struct WorkspaceState {
    pub(super) is_open: bool,
    active_tab: WorkspaceTab,
    root: Option<PathBuf>,
    file_tree: Option<WorkspaceTreeNode>,
    file_error: Option<String>,
    outline_tree: Vec<WorkspaceTreeNode>,
    outline_source: Option<String>,
    expanded: HashSet<String>,
    selected: Option<WorkspaceSelection>,
    open_documents: Vec<WorkspaceDocumentTab>,
    active_document: Option<PathBuf>,
}

impl Default for WorkspaceState {
    fn default() -> Self {
        Self {
            is_open: true,
            active_tab: WorkspaceTab::Files,
            root: None,
            file_tree: None,
            file_error: None,
            outline_tree: Vec::new(),
            outline_source: None,
            expanded: HashSet::new(),
            selected: None,
            open_documents: Vec::new(),
            active_document: None,
        }
    }
}

impl Editor {
    fn prompt_open_workspace_folder(&mut self, cx: &mut Context<Self>) {
        let prompt_title = cx
            .global::<crate::i18n::I18nManager>()
            .strings()
            .open_workspace_folder_prompt
            .clone();
        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(prompt_title.into()),
        });
        let editor = cx.entity().downgrade();

        cx.spawn(async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let Ok(Ok(Some(mut paths))) = prompt.await else {
                return;
            };
            let Some(root) = paths.pop() else {
                return;
            };
            let _ = editor.update(cx, |editor, cx| editor.set_workspace_root(root, cx));
        })
        .detach();
    }

    pub(crate) fn set_workspace_root(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        self.workspace.root = Some(root);
        self.workspace.file_tree = None;
        self.workspace.file_error = None;
        self.workspace.expanded.clear();
        self.sync_workspace_file_tree();
        self.sync_workspace_outline(cx);
        cx.notify();
    }

    pub(crate) fn toggle_workspace_drawer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.workspace.is_open {
            self.workspace.is_open = false;
        } else {
            self.close_menu_bar(cx);
            self.dismiss_contextual_overlays(cx);
            self.workspace.is_open = true;
            self.sync_workspace_models(cx);
            window.activate_window();
        }
        cx.notify();
    }

    pub(crate) fn on_toggle_workspace_action(
        &mut self,
        _: &crate::components::ToggleWorkspace,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_workspace_drawer(window, cx);
    }

    pub(super) fn sync_workspace_after_document_path_change(&mut self, cx: &mut Context<Self>) {
        self.workspace.file_tree = None;
        self.workspace.file_error = None;
        self.workspace.outline_source = None;
        if self.workspace.root.is_none() {
            self.workspace.root = self.workspace_root_for_current_file();
        }
        if self.workspace.is_open {
            self.sync_workspace_models(cx);
        }
    }

    fn sync_workspace_models(&mut self, cx: &mut Context<Self>) {
        self.sync_workspace_file_tree();
        self.sync_workspace_outline(cx);
        self.ensure_current_document_tab(cx);
    }

    fn ensure_current_document_tab(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.file_path.clone() else {
            return;
        };
        if self.workspace.active_document.as_ref() == Some(&path) {
            return;
        }
        if !self
            .workspace
            .open_documents
            .iter()
            .any(|tab| tab.path == path)
        {
            self.workspace.open_documents.push(WorkspaceDocumentTab {
                path: path.clone(),
                markdown: self.serialized_document_text(cx),
                dirty: self.document_dirty,
            });
        }
        self.workspace.active_document = Some(path);
    }

    fn snapshot_current_document(&mut self, cx: &App) {
        let Some(path) = self.file_path.clone() else {
            return;
        };
        let markdown = self.serialized_document_text(cx);
        if let Some(tab) = self
            .workspace
            .open_documents
            .iter_mut()
            .find(|tab| tab.path == path)
        {
            tab.markdown = markdown;
            tab.dirty = self.document_dirty;
        } else {
            self.workspace.open_documents.push(WorkspaceDocumentTab {
                path: path.clone(),
                markdown,
                dirty: self.document_dirty,
            });
        }
        self.workspace.active_document = Some(path);
    }

    fn workspace_root_for_current_file(&self) -> Option<PathBuf> {
        self.file_path.as_ref()?.parent().map(Path::to_path_buf)
    }

    fn sync_workspace_file_tree(&mut self) {
        let next_root = self
            .workspace
            .root
            .clone()
            .or_else(|| self.workspace_root_for_current_file());
        if self.workspace.root == next_root && self.workspace.file_tree.is_some() {
            self.workspace.selected = self
                .file_path
                .as_ref()
                .map(|path| WorkspaceSelection::File(path.clone()));
            return;
        }

        self.workspace.root = next_root.clone();
        self.workspace.file_tree = None;
        self.workspace.file_error = None;

        let Some(root) = next_root else {
            self.workspace.selected = None;
            return;
        };

        // Validate the root path
        if root.as_os_str().is_empty() {
            self.workspace.file_error = Some("Invalid workspace path: empty path".to_string());
            self.workspace.selected = None;
            return;
        }

        match scan_workspace_dir(&root) {
            Ok(tree) => {
                self.workspace.expanded.insert(tree.id.clone());
                self.workspace.file_tree = Some(tree);
                self.workspace.selected = self
                    .file_path
                    .as_ref()
                    .map(|path| WorkspaceSelection::File(path.clone()));
            }
            Err(err) => {
                self.workspace.file_error = Some(err.to_string());
            }
        }
    }

    fn sync_workspace_outline(&mut self, cx: &mut Context<Self>) {
        let source = self.serialized_document_text(cx);
        if self.workspace.outline_source.as_deref() == Some(source.as_str()) {
            return;
        }

        let outline = build_outline_tree(&source);
        prune_outline_state(&mut self.workspace, &outline);
        self.workspace.outline_tree = outline;
        self.workspace.outline_source = Some(source);
    }

    fn set_workspace_tab(&mut self, tab: WorkspaceTab, cx: &mut Context<Self>) {
        if self.workspace.active_tab != tab {
            self.workspace.active_tab = tab;
            self.sync_workspace_models(cx);
            cx.notify();
        }
    }

    fn toggle_workspace_node(&mut self, id: &str, cx: &mut Context<Self>) {
        if !self.workspace.expanded.remove(id) {
            self.workspace.expanded.insert(id.to_string());
        }
        cx.notify();
    }

    fn select_outline_node(&mut self, id: String, cx: &mut Context<Self>) {
        self.workspace.selected = Some(WorkspaceSelection::Outline(id));
        cx.notify();
    }

    pub(super) fn open_workspace_file(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.file_path.as_ref() == Some(&path) {
            return;
        }
        if self.file_path.is_none() && self.document_dirty {
            self.request_dropped_markdown_replace(path, window, cx);
            return;
        }

        self.snapshot_current_document(cx);
        let cached = self
            .workspace
            .open_documents
            .iter()
            .find(|tab| tab.path == path)
            .cloned();
        let (markdown, dirty) = if let Some(tab) = cached {
            (tab.markdown, tab.dirty)
        } else {
            match fs::read_to_string(&path) {
                Ok(markdown) => (markdown, false),
                Err(err) => {
                    self.workspace.file_error = Some(err.to_string());
                    cx.notify();
                    return;
                }
            }
        };
        if !self
            .workspace
            .open_documents
            .iter()
            .any(|tab| tab.path == path)
        {
            self.workspace.open_documents.push(WorkspaceDocumentTab {
                path: path.clone(),
                markdown: markdown.clone(),
                dirty,
            });
        }
        self.workspace.active_document = Some(path.clone());
        self.workspace.selected = Some(WorkspaceSelection::File(path.clone()));
        self.replace_document_from_markdown(markdown, Some(path), cx);
        self.document_dirty = dirty;
        window.set_window_edited(dirty);
        cx.notify();
    }

    pub(super) fn render_document_tabs(
        &mut self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        self.ensure_current_document_tab(cx);
        if self.workspace.open_documents.is_empty() {
            return None;
        }

        let editor = cx.entity().downgrade();
        let c = &theme.colors;
        let t = &theme.typography;
        let tabs = self
            .workspace
            .open_documents
            .iter()
            .map(|tab| {
                let path = tab.path.clone();
                let click_path = path.clone();
                let active = self.workspace.active_document.as_ref() == Some(&tab.path);
                let dirty = if active {
                    self.document_dirty
                } else {
                    tab.dirty
                };
                let title = tab
                    .path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| tab.path.to_string_lossy().into_owned());
                let tab_editor = editor.clone();
                div()
                    .id(("document-tab", stable_node_hash(&path.to_string_lossy())))
                    .h(px(36.0))
                    .px(px(14.0))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .border_b(px(if active { 2.0 } else { 1.0 }))
                    .border_color(if active {
                        c.dialog_primary_button_bg
                    } else {
                        c.dialog_border
                    })
                    .bg(if active {
                        c.dialog_surface
                    } else {
                        c.editor_background
                    })
                    .hover(|this| this.bg(c.dialog_secondary_button_hover))
                    .cursor_pointer()
                    .text_size(px(t.text_size * 0.86))
                    .text_color(if active {
                        c.text_default
                    } else {
                        c.dialog_muted
                    })
                    .child(if dirty { format!("● {title}") } else { title })
                    .on_click(move |_event, window, cx| {
                        let _ = tab_editor.update(cx, |editor, cx| {
                            editor.open_workspace_file(click_path.clone(), window, cx);
                        });
                    })
                    .into_any_element()
            })
            .collect::<Vec<_>>();

        Some(
            div()
                .id("document-tabs")
                .w_full()
                .h(px(38.0))
                .flex_shrink_0()
                .flex()
                .overflow_x_scroll()
                .bg(c.editor_background)
                .border_b(px(theme.dimensions.dialog_border_width))
                .border_color(c.dialog_border)
                .children(tabs)
                .into_any_element(),
        )
    }

    fn open_code_file(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.workspace.selected = Some(WorkspaceSelection::File(path.clone()));
        self.workspace.file_error = None;
        let error_path = path.clone();
        let editor = cx.entity().downgrade();
        let background = cx.background_executor().clone();

        cx.spawn(async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let result = background
                .spawn(async move {
                    let source = fs::read_to_string(&path)?;
                    let language = path
                        .extension()
                        .map(|ext| ext.to_string_lossy().into_owned());
                    let highlight =
                        crate::components::markdown::code_highlight::highlight_code_block(
                            language.as_deref(),
                            &source,
                        );
                    Ok::<_, std::io::Error>((source, highlight))
                })
                .await;

            match result {
                Ok((source, highlight)) => {
                    let _ = cx.update(move |app| {
                        if let Err(err) =
                            code_viewer::open_code_viewer_window(app, error_path, source, highlight)
                        {
                            eprintln!("failed to open code viewer: {err}");
                        }
                    });
                }
                Err(err) => {
                    let message = format!("无法读取代码文件：{}", err);
                    let _ = editor.update(cx, move |editor, cx| {
                        editor.workspace.file_error = Some(message);
                        cx.notify();
                    });
                }
            }
        })
        .detach();
    }

    pub(super) fn render_workspace_panel(
        &mut self,
        theme: &Theme,
        strings: &I18nStrings,
        panel_width: f32,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.workspace.is_open {
            return None;
        }

        self.sync_workspace_models(cx);
        let editor = cx.entity().downgrade();
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;

        let tab = |label: String, tab: WorkspaceTab, active: bool| {
            let tab_editor = editor.clone();
            let tab_id = match tab {
                WorkspaceTab::Files => "workspace-tab-files",
                WorkspaceTab::Outline => "workspace-tab-outline",
            };
            div()
                .id(tab_id)
                .h(px(30.0))
                .px(px(12.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(6.0))
                .bg(if active {
                    c.selection
                } else {
                    hsla(0.0, 0.0, 0.0, 0.0)
                })
                .hover(|this| this.bg(c.dialog_secondary_button_hover))
                .cursor_pointer()
                .text_size(px(t.text_size * 0.88))
                .text_color(if active {
                    c.text_default
                } else {
                    c.dialog_muted
                })
                .child(label)
                .on_click(move |_event, _window, cx| {
                    let _ = tab_editor.update(cx, |editor, cx| {
                        editor.set_workspace_tab(tab, cx);
                    });
                })
        };

        let body = match self.workspace.active_tab {
            WorkspaceTab::Files => self.render_workspace_files_tree(theme, strings, &editor),
            WorkspaceTab::Outline => self.render_workspace_outline_tree(theme, strings, &editor),
        };
        let open_folder_editor = editor.clone();
        let open_folder_button = div()
            .id("workspace-open-folder")
            .w_full()
            .h(px(32.0))
            .px(px(10.0))
            .flex()
            .items_center()
            .rounded(px(7.0))
            .bg(c.dialog_secondary_button_bg)
            .hover(|this| this.bg(c.dialog_secondary_button_hover))
            .cursor_pointer()
            .text_size(px(t.text_size * 0.9))
            .text_color(c.text_default)
            .child(strings.menu_open_workspace_folder.clone())
            .on_click(move |_event, _window, cx| {
                let _ = open_folder_editor.update(cx, |editor, cx| {
                    editor.prompt_open_workspace_folder(cx);
                });
            });

        Some(
            div()
                .id("workspace-panel")
                .h_full()
                .w(px(panel_width))
                .flex()
                .flex_col()
                .flex_shrink_0()
                .bg(c.dialog_surface)
                .border_r(px(d.dialog_border_width))
                .border_color(c.dialog_border)
                .child(
                    div()
                        .px(px(12.0))
                        .pt(px(12.0))
                        .pb(px(10.0))
                        .flex()
                        .flex_col()
                        .gap(px(8.0))
                        .border_b(px(d.dialog_border_width))
                        .border_color(c.dialog_border)
                        .child(
                            div()
                                .flex()
                                .gap(px(8.0))
                                .child(tab(
                                    strings.workspace_tab_files.clone(),
                                    WorkspaceTab::Files,
                                    self.workspace.active_tab == WorkspaceTab::Files,
                                ))
                                .child(tab(
                                    strings.workspace_tab_outline.clone(),
                                    WorkspaceTab::Outline,
                                    self.workspace.active_tab == WorkspaceTab::Outline,
                                )),
                        )
                        .child(open_folder_button),
                )
                .child(
                    div()
                        .id("workspace-panel-scroll")
                        .flex_1()
                        .min_h(px(0.0))
                        .overflow_y_scroll()
                        .px(px(8.0))
                        .py(px(10.0))
                        .child(body),
                )
                .into_any_element(),
        )
    }

    fn render_workspace_files_tree(
        &self,
        theme: &Theme,
        strings: &I18nStrings,
        editor: &WeakEntity<Editor>,
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

        let Some(root) = self.workspace.file_tree.as_ref() else {
            return self.render_workspace_empty_state("", &strings.workspace_empty_files, theme);
        };

        div()
            .w_full()
            .flex()
            .flex_col()
            .children(self.render_workspace_nodes(std::slice::from_ref(root), 0, theme, editor))
            .into_any_element()
    }

    fn render_workspace_outline_tree(
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

    fn render_workspace_empty_state(
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

    fn render_workspace_nodes(
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

    fn render_workspace_node(
        &self,
        node: &WorkspaceTreeNode,
        depth: usize,
        theme: &Theme,
        editor: &WeakEntity<Editor>,
    ) -> AnyElement {
        let c = &theme.colors;
        let t = &theme.typography;
        let is_expanded = self.workspace.expanded.contains(&node.id);
        let has_children = !node.children.is_empty();
        let selected = match (&self.workspace.selected, &node.kind) {
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
        let arrow_node_id = node.id.clone();
        let arrow_editor = editor.clone();
        let arrow = if has_children {
            if is_expanded { "v" } else { ">" }
        } else {
            ""
        };

        let icon = match &node.kind {
            WorkspaceTreeKind::Directory(_) => Some((FOLDER_ICON, Hsla::from(rgba(0xf59e0bff)))),
            WorkspaceTreeKind::MarkdownFile(_) => {
                Some((MARKDOWN_ICON, Hsla::from(rgba(0x2563ebff))))
            }
            WorkspaceTreeKind::CodeFile(_) => Some((CODE_ICON, c.dialog_muted)),
            WorkspaceTreeKind::Heading { .. } => None,
        };

        let label_color = if selected {
            c.text_default
        } else {
            c.dialog_muted
        };

        let mut arrow_el = div()
            .w(px(14.0))
            .h(px(18.0))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(12.0))
            .text_color(c.dialog_muted)
            .child(arrow);
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

        div()
            .id(("workspace-node", stable_node_hash(&node.id)))
            .h(px(WORKSPACE_NODE_HEIGHT))
            .w_full()
            .overflow_hidden()
            .flex()
            .items_center()
            .gap(px(6.0))
            .pl(px(8.0 + depth as f32 * WORKSPACE_NODE_INDENT))
            .pr(px(8.0))
            .rounded(px(6.0))
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
                    .text_size(px(t.text_size * 0.9))
                    .line_height(px(t.text_size * t.text_line_height))
                    .text_color(label_color)
                    .child(node.label.clone()),
            )
            .on_click(move |_event, window, cx| {
                let node_id = node_id.clone();
                let click_kind = click_kind.clone();
                let _ = click_editor.update(cx, |editor, cx| match click_kind {
                    WorkspaceTreeKind::Directory(_) => editor.toggle_workspace_node(&node_id, cx),
                    WorkspaceTreeKind::MarkdownFile(path) => {
                        editor.open_workspace_file(path, window, cx);
                    }
                    WorkspaceTreeKind::CodeFile(path) => {
                        editor.open_code_file(path, cx);
                    }
                    WorkspaceTreeKind::Heading { .. } => editor.select_outline_node(node_id, cx),
                });
            })
            .into_any_element()
    }
}

fn is_markdown_file(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.to_string_lossy().eq_ignore_ascii_case("md"))
}

fn is_code_file(path: &Path) -> bool {
    const CODE_EXTENSIONS: &[&str] = &[
        "c", "cc", "cpp", "cs", "css", "go", "h", "hpp", "html", "java", "js", "json", "jsx", "kt",
        "php", "py", "rb", "rs", "sh", "sql", "swift", "toml", "ts", "tsx", "xml", "yaml", "yml",
        "zsh",
    ];

    path.extension().is_some_and(|extension| {
        let extension = extension.to_string_lossy();
        CODE_EXTENSIONS
            .iter()
            .any(|candidate| extension.eq_ignore_ascii_case(candidate))
    })
}

fn scan_workspace_dir(path: &Path) -> Result<WorkspaceTreeNode> {
    let mut children = Vec::new();
    for entry in
        fs::read_dir(path).with_context(|| format!("failed to read '{}'", path.display()))?
    {
        let entry = entry?;
        let entry_path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            let name = entry.file_name();
            if matches!(
                name.to_str(),
                Some(".git" | "target" | "node_modules" | ".worktrees" | "dist")
            ) {
                continue;
            }
            children.push(scan_workspace_dir(&entry_path)?);
        } else if file_type.is_file() && is_markdown_file(&entry_path) {
            children.push(WorkspaceTreeNode {
                id: file_node_id(&entry_path),
                label: file_label(&entry_path),
                kind: WorkspaceTreeKind::MarkdownFile(entry_path),
                children: Vec::new(),
            });
        } else if file_type.is_file() && is_code_file(&entry_path) {
            children.push(WorkspaceTreeNode {
                id: file_node_id(&entry_path),
                label: file_label(&entry_path),
                kind: WorkspaceTreeKind::CodeFile(entry_path),
                children: Vec::new(),
            });
        }
    }

    children.sort_by(|left, right| {
        let left_dir = matches!(left.kind, WorkspaceTreeKind::Directory(_));
        let right_dir = matches!(right.kind, WorkspaceTreeKind::Directory(_));
        right_dir
            .cmp(&left_dir)
            .then_with(|| left.label.to_lowercase().cmp(&right.label.to_lowercase()))
    });

    Ok(WorkspaceTreeNode {
        id: file_node_id(path),
        label: file_label(path),
        kind: WorkspaceTreeKind::Directory(path.to_path_buf()),
        children,
    })
}

fn file_label(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

fn file_node_id(path: &Path) -> String {
    format!("file:{}", path.to_string_lossy())
}

fn stable_node_hash(id: &str) -> u64 {
    use std::hash::{Hash, Hasher};

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    id.hash(&mut hasher);
    hasher.finish()
}

pub(super) fn workspace_panel_width_for_viewport(viewport_width: f32) -> f32 {
    let target = viewport_width * WORKSPACE_PANEL_TARGET_RATIO;
    target.clamp(WORKSPACE_PANEL_MIN_WIDTH, WORKSPACE_PANEL_MAX_WIDTH)
}

fn prune_outline_state(workspace: &mut WorkspaceState, outline: &[WorkspaceTreeNode]) {
    let mut current_ids = HashSet::new();
    collect_node_ids(outline, &mut current_ids);
    workspace
        .expanded
        .retain(|id| !is_outline_node_id(id) || current_ids.contains(id));

    if matches!(
        &workspace.selected,
        Some(WorkspaceSelection::Outline(id)) if !current_ids.contains(id)
    ) {
        workspace.selected = None;
    }
}

fn collect_node_ids(nodes: &[WorkspaceTreeNode], ids: &mut HashSet<String>) {
    for node in nodes {
        ids.insert(node.id.clone());
        collect_node_ids(&node.children, ids);
    }
}

fn is_outline_node_id(id: &str) -> bool {
    id.starts_with("outline:")
}

fn build_outline_tree(markdown: &str) -> Vec<WorkspaceTreeNode> {
    let mut roots = Vec::new();
    let mut stack: Vec<(u8, Vec<usize>)> = Vec::new();
    let mut fence: Option<(char, usize)> = None;

    for (line_index, line) in markdown.lines().enumerate() {
        let trimmed = line.trim_start();
        if let Some((marker, len)) = fence {
            if is_closing_fence(trimmed, marker, len) {
                fence = None;
            }
            continue;
        }

        if let Some(next_fence) = opening_fence(trimmed) {
            fence = Some(next_fence);
            continue;
        }

        let Some((level, title)) = BlockKind::parse_atx_heading_line(line) else {
            continue;
        };

        while stack
            .last()
            .is_some_and(|(parent_level, _)| *parent_level >= level)
        {
            stack.pop();
        }

        let node = WorkspaceTreeNode {
            id: format!("outline:{line_index}"),
            label: title,
            kind: WorkspaceTreeKind::Heading {
                line: line_index,
                level,
            },
            children: Vec::new(),
        };

        let siblings = if let Some((_, parent_path)) = stack.last() {
            children_at_path_mut(&mut roots, parent_path)
        } else {
            &mut roots
        };
        siblings.push(node);

        let mut node_path = stack
            .last()
            .map(|(_, path)| path.clone())
            .unwrap_or_default();
        node_path.push(siblings.len() - 1);
        stack.push((level, node_path));
    }

    roots
}

fn children_at_path_mut<'a>(
    nodes: &'a mut Vec<WorkspaceTreeNode>,
    path: &[usize],
) -> &'a mut Vec<WorkspaceTreeNode> {
    let mut current = nodes;
    for &index in path {
        current = &mut current[index].children;
    }
    current
}

fn opening_fence(trimmed: &str) -> Option<(char, usize)> {
    let marker = trimmed.chars().next()?;
    if marker != '`' && marker != '~' {
        return None;
    }
    let len = trimmed.chars().take_while(|ch| *ch == marker).count();
    (len >= 3).then_some((marker, len))
}

fn is_closing_fence(trimmed: &str, marker: char, len: usize) -> bool {
    let count = trimmed.chars().take_while(|ch| *ch == marker).count();
    count >= len && trimmed[count..].trim().is_empty()
}

#[cfg(test)]
mod tests {
    use super::{
        WorkspaceSelection, WorkspaceState, WorkspaceTreeKind, build_outline_tree,
        prune_outline_state, scan_workspace_dir, workspace_panel_width_for_viewport,
    };
    use std::fs;

    #[test]
    fn workspace_scan_includes_markdown_and_code_files() {
        let root =
            std::env::temp_dir().join(format!("velotype-workspace-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("nested")).expect("create dirs");
        fs::write(root.join("a.md"), "a").expect("write md");
        fs::write(root.join("a.txt"), "ignored").expect("write txt");
        fs::write(root.join("main.rs"), "fn main() {}").expect("write code");
        fs::write(root.join("nested").join("b.md"), "b").expect("write nested md");

        let tree = scan_workspace_dir(&root).expect("scan tree");
        let labels = tree
            .children
            .iter()
            .map(|node| node.label.as_str())
            .collect::<Vec<_>>();
        assert_eq!(labels, vec!["nested", "a.md", "main.rs"]);
        assert!(matches!(
            tree.children[0].kind,
            WorkspaceTreeKind::Directory(_)
        ));
        assert!(matches!(
            tree.children[1].kind,
            WorkspaceTreeKind::MarkdownFile(_)
        ));
        assert!(matches!(
            tree.children[2].kind,
            WorkspaceTreeKind::CodeFile(_)
        ));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn outline_tree_skips_headings_inside_fenced_code() {
        let outline = build_outline_tree(
            "# Root\n\n```md\n# ignored\n```\n\n## Child\n\n### Grandchild\n\n# Next",
        );

        assert_eq!(outline.len(), 2);
        assert_eq!(outline[0].label, "Root");
        assert_eq!(outline[0].children[0].label, "Child");
        assert_eq!(outline[0].children[0].children[0].label, "Grandchild");
        assert_eq!(outline[1].label, "Next");
    }

    #[test]
    fn outline_expansion_state_is_not_auto_populated_and_prunes_stale_ids() {
        let outline = build_outline_tree("# Root\n\n## Child\n\n# Next");
        let mut fresh = WorkspaceState::default();
        prune_outline_state(&mut fresh, &outline);
        assert!(fresh.expanded.is_empty());

        let mut existing = WorkspaceState::default();
        existing.expanded.insert("outline:0".to_string());
        existing.expanded.insert("outline:999".to_string());
        existing
            .expanded
            .insert("workspace-dir:C:/docs".to_string());
        existing.selected = Some(WorkspaceSelection::Outline("outline:999".to_string()));

        prune_outline_state(&mut existing, &outline);

        assert!(existing.expanded.contains("outline:0"));
        assert!(existing.expanded.contains("workspace-dir:C:/docs"));
        assert!(!existing.expanded.contains("outline:999"));
        assert_eq!(existing.selected, None);
    }

    #[test]
    fn workspace_panel_width_uses_ratio_with_bounds() {
        assert_eq!(workspace_panel_width_for_viewport(1000.0), 240.0);
        assert_eq!(workspace_panel_width_for_viewport(2000.0), 300.0);
        assert_eq!(workspace_panel_width_for_viewport(4000.0), 360.0);
    }
}
