//! Lightweight workspace panel state, file-tree scanning, and outline parsing.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context as _, Result};
use gpui::*;
use pulldown_cmark::{Event, LinkType, Options, Parser, Tag};
use unicode_segmentation::UnicodeSegmentation;

use super::{BlockKind, Editor, UndoSelectionSnapshot};
use crate::i18n::{I18nManager, I18nStrings};
use crate::theme::{Theme, ThemeManager};

const FOLDER_ICON: &str = "icon/workspace/folder.svg";
const ACTIVITY_FILES_ICON: &str = "icon/workspace/activity-files.svg";
const ACTIVITY_SEARCH_ICON: &str = "icon/workspace/activity-search.svg";
const ACTIVITY_OUTLINE_ICON: &str = "icon/workspace/activity-outline.svg";
const MARKDOWN_ICON: &str = "icon/workspace/markdown.svg";
const CODE_ICON: &str = "icon/workspace/code.svg";
const CHEVRON_RIGHT_ICON: &str = "icon/workspace/chevron-right.svg";
const CHEVRON_DOWN_ICON: &str = "icon/workspace/chevron-down.svg";
const TAB_CLOSE_ICON: &str = "icon/workspace/tab-close.svg";
const WORKSPACE_NODE_HEIGHT: f32 = 24.0;
const WORKSPACE_NODE_INDENT: f32 = 16.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum WorkspaceTab {
    #[default]
    Files,
    Search,
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

struct WorkspaceTooltip {
    label: String,
}

impl Render for WorkspaceTooltip {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<ThemeManager>().current_arc();
        div()
            .px(px(8.0))
            .py(px(5.0))
            .rounded(px(6.0))
            .bg(theme.colors.dialog_surface)
            .border_1()
            .border_color(theme.colors.dialog_border)
            .shadow_md()
            .text_size(px(12.0))
            .text_color(theme.colors.dialog_title)
            .child(self.label.clone())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct WorkspaceDocumentTab {
    path: PathBuf,
    recovery_id: uuid::Uuid,
    file_version: u64,
    markdown: String,
    dirty: bool,
}

#[derive(Clone, Copy)]
struct WorkspaceContextMenu {
    position: Point<Pixels>,
    has_target: bool,
}

#[derive(Clone, Copy)]
struct WorkspaceResizeDrag {
    start_x: f32,
    start_width: f32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct WorkspaceSearchHit {
    path: PathBuf,
    label: String,
    line: Option<usize>,
    /// Byte range of the match inside its line (used to select the match when
    /// jumping and to open the hit's file at the right spot).
    match_range: Option<Range<usize>>,
    /// Absolute byte range in the current document (document-scope hits only).
    source_range: Option<Range<usize>>,
    preview: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum WorkspaceSearchScope {
    #[default]
    Workspace,
    Document,
}

#[derive(Clone, Copy)]
enum WorkspaceMenuAction {
    NewFile,
    NewFolder,
    Rename,
    Delete,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TabMenuAction {
    Close,
    CloseOthers,
    CloseLeft,
    CloseRight,
    CloseAll,
}

#[derive(Clone, Copy)]
struct TabContextMenu {
    position: Point<Pixels>,
    target_index: usize,
}

/// Which search-panel input a key/IME event targets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SearchInputKind {
    Query,
    Replace,
}

pub(super) struct WorkspaceAutosaveDocument {
    pub(super) recovery_id: uuid::Uuid,
    pub(super) file_version: u64,
    pub(super) path: PathBuf,
    pub(super) markdown: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum WorkspaceSelection {
    Directory(PathBuf),
    File(PathBuf),
    WorkspaceRoot(PathBuf),
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
    search_query: String,
    search_scope: WorkspaceSearchScope,
    search_active_index: Option<usize>,
    search_selected_range: Range<usize>,
    search_marked_range: Option<Range<usize>>,
    replace_query: String,
    replace_visible: bool,
    replace_focus: Option<FocusHandle>,
    replace_selected_range: Range<usize>,
    replace_marked_range: Option<Range<usize>>,
    search_match_case: bool,
    search_whole_word: bool,
    search_use_regex: bool,
    search_fuzzy: bool,
    search_focus: Option<FocusHandle>,
    search_focus_pending: bool,
    search_results: Vec<WorkspaceSearchHit>,
    document_search_source: Option<String>,
    document_active_range: Option<Range<usize>>,
    search_pending: bool,
    search_generation: u64,
    context_menu: Option<WorkspaceContextMenu>,
    tab_context_menu: Option<TabContextMenu>,
    panel_width: Option<f32>,
    resize_drag: Option<WorkspaceResizeDrag>,
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
            search_query: String::new(),
            search_scope: WorkspaceSearchScope::Workspace,
            search_active_index: None,
            search_selected_range: 0..0,
            search_marked_range: None,
            replace_query: String::new(),
            replace_visible: false,
            replace_focus: None,
            replace_selected_range: 0..0,
            replace_marked_range: None,
            search_match_case: false,
            search_whole_word: false,
            search_use_regex: false,
            search_fuzzy: false,
            search_focus: None,
            search_focus_pending: false,
            search_results: Vec::new(),
            document_search_source: None,
            document_active_range: None,
            search_pending: false,
            search_generation: 0,
            context_menu: None,
            tab_context_menu: None,
            panel_width: None,
            resize_drag: None,
        }
    }
}

impl Editor {
    pub(crate) fn set_workspace_root(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        self.workspace.selected = Some(WorkspaceSelection::Directory(root.clone()));
        self.workspace.root = Some(root);
        self.workspace.file_tree = None;
        self.workspace.file_error = None;
        self.workspace.expanded.clear();
        self.workspace.active_tab = WorkspaceTab::Files;
        self.workspace.search_scope = WorkspaceSearchScope::Workspace;
        self.workspace.search_focus_pending = false;
        self.workspace.search_query.clear();
        self.workspace.search_selected_range = 0..0;
        self.workspace.search_marked_range = None;
        self.workspace.search_results.clear();
        self.workspace.document_search_source = None;
        self.workspace.document_active_range = None;
        self.workspace.search_pending = false;
        self.workspace.search_generation = self.workspace.search_generation.wrapping_add(1);
        self.sync_workspace_file_tree();
        self.sync_workspace_outline(cx);
        cx.notify();
    }

    fn selected_workspace_directory(&self) -> Option<PathBuf> {
        match self.workspace.selected.as_ref() {
            Some(WorkspaceSelection::Directory(path)) => Some(path.clone()),
            Some(WorkspaceSelection::File(path)) => path.parent().map(Path::to_path_buf),
            Some(WorkspaceSelection::WorkspaceRoot(path)) => Some(path.clone()),
            _ => self.workspace.root.clone(),
        }
    }

    fn refresh_workspace_tree(&mut self, cx: &mut Context<Self>) {
        self.workspace.file_tree = None;
        self.sync_workspace_file_tree();
        if self.workspace.active_tab == WorkspaceTab::Search
            && !self.workspace.search_query.is_empty()
        {
            self.schedule_workspace_search(cx);
        }
        cx.notify();
    }

    fn show_workspace_file_error(
        window_handle: AnyWindowHandle,
        detail: String,
        cx: &mut AsyncApp,
    ) {
        let _ = cx.update_window(
            window_handle,
            move |_view: AnyView, window: &mut Window, cx: &mut App| {
                let strings = cx.global::<crate::i18n::I18nManager>().strings().clone();
                let buttons = [strings.info_dialog_ok.as_str()];
                let _ = window.prompt(
                    PromptLevel::Critical,
                    &strings.open_failed_title,
                    Some(&detail),
                    &buttons,
                    cx,
                );
            },
        );
    }

    pub(super) fn show_external_change_error(
        window_handle: AnyWindowHandle,
        detail: String,
        cx: &mut AsyncApp,
    ) {
        let _ = cx.update_window(
            window_handle,
            move |_view: AnyView, window: &mut Window, cx: &mut App| {
                let strings = cx.global::<crate::i18n::I18nManager>().strings().clone();
                let message = format!("{}\n\n{}", strings.external_change_message, detail);
                let buttons = [strings.info_dialog_ok.as_str()];
                let _ = window.prompt(
                    PromptLevel::Warning,
                    &strings.external_change_title,
                    Some(&message),
                    &buttons,
                    cx,
                );
            },
        );
    }

    pub(super) fn show_workspace_save_error(
        window_handle: AnyWindowHandle,
        detail: String,
        cx: &mut AsyncApp,
    ) {
        let _ = cx.update_window(
            window_handle,
            move |_view: AnyView, window: &mut Window, cx: &mut App| {
                let strings = cx.global::<crate::i18n::I18nManager>().strings().clone();
                let buttons = [strings.info_dialog_ok.as_str()];
                let _ = window.prompt(
                    PromptLevel::Critical,
                    &strings.save_failed_title,
                    Some(&detail),
                    &buttons,
                    cx,
                );
            },
        );
    }

    pub(super) fn report_workspace_file_error(&mut self, detail: String, cx: &mut Context<Self>) {
        if self.workspace.root.is_none() {
            self.workspace.root = self.workspace_root_for_current_file();
        }
        self.workspace.file_error = Some(detail);
        cx.notify();
    }

    fn prompt_create_workspace_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(directory) = self.selected_workspace_directory() else {
            return;
        };
        let prompt = cx.prompt_for_new_path(&directory, Some("untitled.md"));
        let editor = cx.entity().downgrade();
        let window_handle = window.window_handle();
        cx.spawn(async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let Ok(Ok(Some(path))) = prompt.await else {
                return;
            };
            if let Err(err) = create_workspace_file(&path) {
                Self::show_workspace_file_error(
                    window_handle,
                    format!("无法新建文件：{}", err),
                    cx,
                );
                return;
            }
            let _ = editor.update(cx, |editor, cx| editor.refresh_workspace_tree(cx));
            let _ = cx.update_window(
                window_handle,
                move |_view: AnyView, window: &mut Window, cx: &mut App| {
                    let _ = editor.update(cx, |editor, cx| {
                        editor.open_workspace_file(path, window, cx);
                    });
                },
            );
        })
        .detach();
    }

    fn prompt_create_workspace_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(directory) = self.selected_workspace_directory() else {
            return;
        };
        let prompt = cx.prompt_for_new_path(&directory, Some("New Folder"));
        let editor = cx.entity().downgrade();
        let window_handle = window.window_handle();
        cx.spawn(async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let Ok(Ok(Some(path))) = prompt.await else {
                return;
            };
            if let Err(err) = create_workspace_folder(&path) {
                Self::show_workspace_file_error(
                    window_handle,
                    format!("无法新建文件夹：{}", err),
                    cx,
                );
                return;
            }
            let folder_path = path.clone();
            let _ = editor.update(cx, move |editor, cx| {
                editor.refresh_workspace_tree(cx);
                editor.workspace.expanded.insert(file_node_id(&folder_path));
                editor.workspace.selected = Some(WorkspaceSelection::Directory(folder_path));
                cx.notify();
            });
        })
        .detach();
    }

    fn prompt_rename_or_move_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(source) = self.selected_workspace_path() else {
            return;
        };
        let Some(parent) = source.parent().map(Path::to_path_buf) else {
            return;
        };
        let suggested_name = source
            .file_name()
            .map(|name| name.to_string_lossy().into_owned());
        let prompt = cx.prompt_for_new_path(&parent, suggested_name.as_deref());
        let editor = cx.entity().downgrade();
        let window_handle = window.window_handle();
        let source_is_directory = source.is_dir();
        let background = cx.background_executor().clone();

        cx.spawn(async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let Ok(Ok(Some(destination))) = prompt.await else {
                return;
            };
            if destination == source {
                return;
            }
            if destination.exists() {
                Self::show_workspace_file_error(
                    window_handle,
                    format!("目标路径已存在：{}", destination.display()),
                    cx,
                );
                return;
            }

            let source_directory = source.parent().map(Path::to_path_buf);
            let destination_directory = destination.parent().map(Path::to_path_buf);
            let markdown_move = !source_is_directory
                && is_markdown_file(&source)
                && source_directory != destination_directory;
            let open_markdown = if markdown_move {
                editor
                    .update(cx, |editor, cx| editor.markdown_state_for_path(&source, cx))
                    .ok()
                    .flatten()
            } else {
                None
            };
            let source_for_read = source.clone();
            let disk_markdown = if markdown_move {
                match background
                    .spawn(async move { fs::read_to_string(source_for_read) })
                    .await
                {
                    Ok(markdown) => Some(markdown),
                    Err(error) => {
                        Self::show_workspace_file_error(
                            window_handle,
                            format!("无法读取待移动的 Markdown 文件：{error}"),
                            cx,
                        );
                        return;
                    }
                }
            } else {
                None
            };
            if let (Some((_, _, expected_version)), Some(markdown)) =
                (open_markdown.as_ref(), disk_markdown.as_ref())
                && super::persistence::file_content_version(markdown) != *expected_version
            {
                Self::show_external_change_error(
                    window_handle,
                    format!("检测到外部修改：{}", source.display()),
                    cx,
                );
                return;
            }
            let rewritten_disk_markdown = match (
                markdown_move,
                disk_markdown.as_deref(),
                source_directory.as_deref(),
                destination_directory.as_deref(),
            ) {
                (true, Some(markdown), Some(source_directory), Some(destination_directory)) => {
                    Some(rewrite_relative_image_targets(
                        markdown,
                        source_directory,
                        destination_directory,
                    ))
                }
                _ => None,
            };
            let moved_disk_markdown = rewritten_disk_markdown
                .clone()
                .or_else(|| disk_markdown.clone());
            let moved_disk_version = moved_disk_markdown
                .as_deref()
                .map(super::persistence::file_content_version);

            if let Err(err) = std::fs::rename(&source, &destination) {
                Self::show_workspace_file_error(
                    window_handle,
                    format!("无法移动或重命名：{}", err),
                    cx,
                );
                return;
            }
            if let Some(rewritten) = rewritten_disk_markdown
                .as_ref()
                .zip(disk_markdown.as_ref())
                .and_then(|(rewritten, original)| (rewritten != original).then_some(rewritten))
                && let Err(error) = fs::write(&destination, rewritten)
            {
                let rollback_error = std::fs::rename(&destination, &source).err();
                let detail = if let Some(rollback_error) = rollback_error {
                    format!("无法更新图片相对路径：{error}；回滚也失败：{rollback_error}")
                } else {
                    format!("无法更新图片相对路径：{error}")
                };
                Self::show_workspace_file_error(window_handle, detail, cx);
                return;
            }

            let _ = editor.update(cx, move |editor, cx| {
                editor.document_revision = editor.document_revision.wrapping_add(1);
                editor.autosave_task = None;
                for tab in &mut editor.workspace.open_documents {
                    if markdown_move && tab.path == source {
                        if let (Some(source_directory), Some(destination_directory)) = (
                            source_directory.as_deref(),
                            destination_directory.as_deref(),
                        ) {
                            tab.markdown = rewrite_relative_image_targets(
                                &tab.markdown,
                                source_directory,
                                destination_directory,
                            );
                        }
                        if let Some(file_version) = moved_disk_version {
                            tab.file_version = file_version;
                        }
                    }
                    if let Some(path) =
                        remap_moved_path(&tab.path, &source, &destination, source_is_directory)
                    {
                        tab.path = path;
                    }
                }
                let active_markdown = if markdown_move && editor.file_path.as_ref() == Some(&source)
                {
                    Some((editor.serialized_document_text(cx), editor.document_dirty))
                } else {
                    None
                };
                if let Some(path) = editor.file_path.as_ref().and_then(|path| {
                    remap_moved_path(path, &source, &destination, source_is_directory)
                }) {
                    editor.file_path = Some(path);
                    editor.pending_window_title_refresh = true;
                }
                if let Some(path) = editor.workspace.active_document.as_ref().and_then(|path| {
                    remap_moved_path(path, &source, &destination, source_is_directory)
                }) {
                    editor.workspace.active_document = Some(path);
                }
                if let Some(root) = editor.workspace.root.as_ref().and_then(|root| {
                    remap_moved_path(root, &source, &destination, source_is_directory)
                }) {
                    editor.workspace.root = Some(root.clone());
                }
                editor.workspace.selected = match editor.workspace.selected.take() {
                    Some(WorkspaceSelection::File(path)) => {
                        remap_moved_path(&path, &source, &destination, source_is_directory)
                            .map(WorkspaceSelection::File)
                    }
                    Some(WorkspaceSelection::Directory(path)) => {
                        remap_moved_path(&path, &source, &destination, source_is_directory)
                            .map(WorkspaceSelection::Directory)
                    }
                    Some(WorkspaceSelection::WorkspaceRoot(path)) => {
                        remap_moved_path(&path, &source, &destination, source_is_directory)
                            .map(WorkspaceSelection::WorkspaceRoot)
                    }
                    other => other,
                };
                if let Some((markdown, was_dirty)) = active_markdown {
                    if let (Some(source_directory), Some(destination_directory)) = (
                        source_directory.as_deref(),
                        destination_directory.as_deref(),
                    ) {
                        let rewritten = rewrite_relative_image_targets(
                            &markdown,
                            source_directory,
                            destination_directory,
                        );
                        editor.file_version = moved_disk_version;
                        if rewritten != markdown {
                            editor.replace_document_from_markdown(
                                rewritten.clone(),
                                Some(destination.clone()),
                                cx,
                            );
                            editor.file_version = moved_disk_version;
                            editor.document_dirty = was_dirty;
                            if was_dirty {
                                editor.mark_dirty(cx);
                            }
                            if let Some(tab) =
                                editor.workspace.open_documents.iter_mut().find(|tab| {
                                    tab.path == destination && tab.recovery_id == editor.recovery_id
                                })
                            {
                                tab.markdown = rewritten;
                                tab.dirty = editor.document_dirty;
                                tab.file_version = moved_disk_version.unwrap_or_else(|| {
                                    super::persistence::file_content_version(&tab.markdown)
                                });
                            }
                        } else if was_dirty {
                            editor.document_dirty = true;
                            editor.schedule_autosave(cx);
                        }
                    }
                }
                editor.refresh_workspace_tree(cx);
                if editor.document_dirty || editor.has_dirty_workspace_documents() {
                    editor.schedule_autosave(cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn prompt_delete_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(target) = self.selected_workspace_path() else {
            return;
        };
        if self.workspace.root.as_ref() == Some(&target) {
            return;
        }
        let target_is_directory = target.is_dir();
        let strings = cx.global::<crate::i18n::I18nManager>().strings().clone();
        let has_unsaved_changes =
            self.document_dirty
                && self
                    .file_path
                    .as_ref()
                    .is_some_and(|path| path_is_affected(path, &target, target_is_directory))
                || self.workspace.open_documents.iter().any(|tab| {
                    tab.dirty && path_is_affected(&tab.path, &target, target_is_directory)
                });
        let mut detail = format!(
            "{}\n{}",
            strings.workspace_delete_confirm_message,
            target.display()
        );
        if has_unsaved_changes {
            detail.push_str("\n");
            detail.push_str(&strings.workspace_delete_unsaved_message);
        }
        let buttons = [
            strings.workspace_delete.as_str(),
            strings.open_link_cancel.as_str(),
        ];
        let prompt = window.prompt(
            PromptLevel::Warning,
            &strings.workspace_delete_confirm_title,
            Some(&detail),
            &buttons,
            cx,
        );
        let editor = cx.entity().downgrade();
        let window_handle = window.window_handle();
        let background = cx.background_executor().clone();

        cx.spawn(async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let Ok(0) = prompt.await else {
                return;
            };
            let delete_target = target.clone();
            let result = background
                .spawn(async move {
                    if target_is_directory {
                        std::fs::remove_dir_all(delete_target)
                    } else {
                        std::fs::remove_file(delete_target)
                    }
                })
                .await;
            if let Err(err) = result {
                Self::show_workspace_file_error(window_handle, format!("无法删除：{}", err), cx);
                return;
            }

            let target_for_update = target.clone();
            let _ = cx.update_window(
                window_handle,
                move |_view: AnyView, window: &mut Window, cx: &mut App| {
                    let _ = editor.update(cx, |editor, cx| {
                        let active_recovery_id = editor.recovery_id;
                        let active_deleted = editor.file_path.as_ref().is_some_and(|path| {
                            path_is_affected(path, &target_for_update, target_is_directory)
                        });
                        editor.document_revision = editor.document_revision.wrapping_add(1);
                        editor.autosave_task = None;
                        if active_deleted {
                            editor.snapshot_current_document(cx);
                        }
                        let deleted_recovery_ids = editor
                            .workspace
                            .open_documents
                            .iter()
                            .filter(|tab| {
                                path_is_affected(
                                    &tab.path,
                                    &target_for_update,
                                    target_is_directory,
                                )
                            })
                            .map(|tab| tab.recovery_id)
                            .collect::<Vec<_>>();
                        for recovery_id in deleted_recovery_ids {
                            if let Err(error) =
                                crate::config::remove_recovery_snapshot(recovery_id)
                            {
                                eprintln!("failed to remove deleted tab recovery snapshot: {error}");
                            }
                        }
                        let next_tab = editor
                            .workspace
                            .open_documents
                            .iter()
                            .find(|tab| {
                                !path_is_affected(
                                    &tab.path,
                                    &target_for_update,
                                    target_is_directory,
                                )
                            })
                            .cloned();
                        editor.workspace.open_documents.retain(|tab| {
                            !path_is_affected(&tab.path, &target_for_update, target_is_directory)
                        });
                        if editor.workspace.root.as_ref().is_some_and(|root| {
                            path_is_affected(root, &target_for_update, target_is_directory)
                        }) {
                            editor.workspace.root = None;
                            editor.workspace.file_tree = None;
                        }
                        editor.workspace.selected = None;
                        if active_deleted {
                            if let Some(tab) = next_tab {
                                editor.recovery_id = tab.recovery_id;
                                let file_version = tab.file_version;
                                editor.recovery_source_path = None;
                                editor.is_recovered_document = false;
                                editor.workspace.active_document = Some(tab.path.clone());
                                if is_code_file(&tab.path) {
                                    editor.replace_document_from_code_source(tab.markdown, tab.path, cx);
                                } else {
                                    editor.replace_document_from_markdown(tab.markdown, Some(tab.path), cx);
                                }
                                editor.document_dirty = tab.dirty;
                                editor.file_version = Some(file_version);
                                window.set_window_edited(tab.dirty);
                            } else {
                                editor.recovery_id = uuid::Uuid::new_v4();
                                editor.recovery_source_path = None;
                                editor.is_recovered_document = false;
                                if let Err(error) =
                                    crate::config::remove_recovery_snapshot(active_recovery_id)
                                {
                                    eprintln!("failed to remove deleted document recovery snapshot: {error}");
                                }
                                editor.workspace.active_document = None;
                                editor.replace_document_from_markdown(String::new(), None, cx);
                                window.set_window_edited(false);
                            }
                        }
                        editor.refresh_workspace_tree(cx);
                        if editor.document_dirty || editor.has_dirty_workspace_documents() {
                            editor.schedule_autosave(cx);
                        }
                        cx.notify();
                    });
                },
            );
        })
        .detach();
    }

    fn selected_workspace_path(&self) -> Option<PathBuf> {
        match self.workspace.selected.as_ref()? {
            WorkspaceSelection::Directory(path)
            | WorkspaceSelection::File(path)
            | WorkspaceSelection::WorkspaceRoot(path) => Some(path.clone()),
            WorkspaceSelection::Outline(_) => None,
        }
    }

    pub(super) fn close_workspace_context_menu(&mut self, cx: &mut Context<Self>) {
        if self.workspace.context_menu.take().is_some() {
            cx.notify();
        }
    }

    fn open_workspace_context_menu(
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

    fn on_workspace_background_right_click(
        &mut self,
        event: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_workspace_context_menu(event.position, None, cx);
        cx.stop_propagation();
    }

    pub(super) fn render_workspace_context_menu_overlay(
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
            actions.push((
                strings.workspace_delete.clone(),
                WorkspaceMenuAction::Delete,
            ));
        }
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
        _: &crate::components::ToggleSidebar,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_workspace_drawer(window, cx);
    }

    pub(super) fn sync_workspace_after_document_path_change(&mut self, cx: &mut Context<Self>) {
        if let Some(path) = self.file_path.clone() {
            let previous = self.workspace.active_document.clone();
            if previous.as_ref() != Some(&path) {
                let markdown = self.serialized_document_text(cx);
                let file_version = self
                    .file_version
                    .unwrap_or_else(|| super::persistence::file_content_version(&markdown));
                let previous_index = previous.as_ref().and_then(|previous| {
                    self.workspace
                        .open_documents
                        .iter()
                        .position(|tab| &tab.path == previous)
                });
                let current_index = self
                    .workspace
                    .open_documents
                    .iter()
                    .position(|tab| tab.path == path);
                if let Some(previous_index) = previous_index {
                    if let Some(current_index) = current_index {
                        self.workspace.open_documents.remove(previous_index);
                        let current_index = if previous_index < current_index {
                            current_index - 1
                        } else {
                            current_index
                        };
                        let tab = &mut self.workspace.open_documents[current_index];
                        tab.recovery_id = self.recovery_id;
                        tab.file_version = file_version;
                        tab.markdown = markdown;
                        tab.dirty = false;
                    } else {
                        let tab = &mut self.workspace.open_documents[previous_index];
                        tab.path = path.clone();
                        tab.recovery_id = self.recovery_id;
                        tab.file_version = file_version;
                        tab.markdown = markdown;
                        tab.dirty = false;
                    }
                } else if let Some(current_index) = current_index {
                    let tab = &mut self.workspace.open_documents[current_index];
                    tab.recovery_id = self.recovery_id;
                    tab.file_version = file_version;
                    tab.markdown = markdown;
                    tab.dirty = false;
                } else {
                    self.workspace.open_documents.push(WorkspaceDocumentTab {
                        path: path.clone(),
                        recovery_id: self.recovery_id,
                        file_version,
                        markdown,
                        dirty: false,
                    });
                }
                if self.workspace.selected == previous.map(WorkspaceSelection::File) {
                    self.workspace.selected = Some(WorkspaceSelection::File(path.clone()));
                }
                self.workspace.active_document = Some(path);
            }
        }
        self.workspace.file_tree = None;
        self.workspace.file_error = None;
        self.workspace.outline_source = None;
        if self.workspace.root.is_none() {
            self.workspace.root = self.workspace_root_for_current_file();
        }
        if self.workspace.is_open {
            self.sync_workspace_models(cx);
        }
        self.refresh_document_find_after_edit(cx);
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
            let markdown = self.serialized_document_text(cx);
            let file_version = self
                .file_version
                .unwrap_or_else(|| super::persistence::file_content_version(&markdown));
            self.workspace.open_documents.push(WorkspaceDocumentTab {
                path: path.clone(),
                recovery_id: self.recovery_id,
                file_version,
                markdown,
                dirty: self.document_dirty,
            });
        }
        self.workspace.active_document = Some(path);
    }

    pub(super) fn snapshot_current_document(&mut self, cx: &App) {
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
                recovery_id: self.recovery_id,
                file_version: self
                    .file_version
                    .unwrap_or_else(|| super::persistence::file_content_version(&markdown)),
                markdown,
                dirty: self.document_dirty,
            });
        }
        self.workspace.active_document = Some(path);
    }

    pub(super) fn dirty_workspace_documents(&mut self, cx: &App) -> Vec<WorkspaceAutosaveDocument> {
        self.snapshot_current_document(cx);
        self.workspace
            .open_documents
            .iter()
            .filter(|tab| tab.dirty)
            .map(|tab| WorkspaceAutosaveDocument {
                recovery_id: tab.recovery_id,
                file_version: tab.file_version,
                path: tab.path.clone(),
                markdown: tab.markdown.clone(),
            })
            .collect()
    }

    pub(super) fn mark_workspace_documents_saved(
        &mut self,
        saved: &[WorkspaceAutosaveDocument],
    ) -> bool {
        let mut active_document_saved = false;
        for document in saved {
            let Some(tab) =
                self.workspace.open_documents.iter_mut().find(|tab| {
                    tab.recovery_id == document.recovery_id && tab.path == document.path
                })
            else {
                continue;
            };
            tab.markdown = document.markdown.clone();
            tab.dirty = false;
            tab.file_version = super::persistence::file_content_version(&document.markdown);
            if self.file_path.as_ref() == Some(&tab.path) {
                self.file_version = Some(tab.file_version);
            }
            active_document_saved |= self.workspace.active_document.as_ref() == Some(&tab.path);
        }
        active_document_saved
    }

    pub(super) fn has_dirty_workspace_documents(&self) -> bool {
        self.workspace.open_documents.iter().any(|tab| tab.dirty)
    }

    pub(super) fn has_external_autosave_conflict(&self) -> bool {
        self.workspace.file_error.as_deref().is_some_and(|detail| {
            detail.starts_with("检测到外部修改") || detail.starts_with("无法读取文件以检查外部修改")
        })
    }

    pub(super) fn workspace_recovery_ids(&self) -> Vec<uuid::Uuid> {
        self.workspace
            .open_documents
            .iter()
            .map(|tab| tab.recovery_id)
            .collect()
    }

    fn workspace_root_for_current_file(&self) -> Option<PathBuf> {
        self.file_path.as_ref()?.parent().map(Path::to_path_buf)
    }

    pub(super) fn workspace_root_for_image_paste(&self) -> Option<PathBuf> {
        self.workspace.root.clone()
    }

    fn markdown_state_for_path(&self, path: &Path, cx: &App) -> Option<(String, bool, u64)> {
        if self.file_path.as_deref() == Some(path) {
            let markdown = self.serialized_document_text(cx);
            return Some((
                markdown.clone(),
                self.document_dirty,
                self.file_version
                    .unwrap_or_else(|| super::persistence::file_content_version(&markdown)),
            ));
        }
        self.workspace
            .open_documents
            .iter()
            .find(|tab| tab.path == path)
            .map(|tab| (tab.markdown.clone(), tab.dirty, tab.file_version))
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

    fn sync_workspace_outline(&mut self, _cx: &mut Context<Self>) {
        let source = &self.last_stable_source_text;
        if self.workspace.outline_source.as_deref() == Some(source.as_str()) {
            return;
        }

        let outline = build_outline_tree(source);
        prune_outline_state(&mut self.workspace, &outline);
        // Expand headings down to H3 by default so the outline is usable
        // without clicking through every level; users can still collapse.
        expand_outline_to_level(&outline, 2, &mut self.workspace.expanded);
        self.workspace.outline_tree = outline;
        self.workspace.outline_source = Some(source.clone());
    }

    fn set_workspace_tab(&mut self, tab: WorkspaceTab, cx: &mut Context<Self>) {
        let changed = self.workspace.active_tab != tab;
        self.workspace.search_focus_pending = false;
        self.workspace.search_query.clear();
        self.workspace.search_selected_range = 0..0;
        self.workspace.search_marked_range = None;
        self.workspace.search_results.clear();
        self.workspace.document_search_source = None;
        self.workspace.document_active_range = None;
        self.workspace.search_pending = false;
        self.workspace.search_generation = self.workspace.search_generation.wrapping_add(1);
        if changed {
            self.workspace.active_tab = tab;
            self.sync_workspace_models(cx);
            cx.notify();
        }
    }

    fn search_options(&self) -> SearchOptions {
        SearchOptions {
            match_case: self.workspace.search_match_case,
            whole_word: self.workspace.search_whole_word,
            use_regex: self.workspace.search_use_regex,
            fuzzy: self.workspace.search_fuzzy,
        }
    }

    fn schedule_workspace_search(&mut self, cx: &mut Context<Self>) {
        self.workspace.search_generation = self.workspace.search_generation.wrapping_add(1);
        let generation = self.workspace.search_generation;
        self.workspace.search_results.clear();
        self.workspace.search_active_index = None;
        self.workspace.document_search_source = None;
        self.workspace.document_active_range = None;
        let matcher = SearchMatcher::new(self.workspace.search_query.trim(), self.search_options());
        let scope = self.workspace.search_scope;
        let tree = self.workspace.file_tree.clone();
        if matcher.is_empty() || (scope == WorkspaceSearchScope::Workspace && tree.is_none()) {
            self.workspace.search_pending = false;
            cx.notify();
            return;
        }
        self.workspace.search_pending = true;
        let editor = cx.entity().downgrade();
        let background = cx.background_executor().clone();
        cx.spawn(async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
            background.timer(Duration::from_millis(120)).await;
            let current = editor
                .update(cx, |editor, _| {
                    editor.workspace.search_generation == generation
                })
                .unwrap_or(false);
            if !current {
                return;
            }
            let (results, document_source) = match scope {
                WorkspaceSearchScope::Workspace => {
                    let Some(tree) = tree else { return };
                    let results = background
                        .spawn(async move {
                            // A matcher bug must degrade to "no results", not
                            // take the whole process down.
                            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                search_workspace_files(&tree, &matcher, 200)
                            }))
                            .unwrap_or_default()
                        })
                        .await;
                    (results, None)
                }
                WorkspaceSearchScope::Document => {
                    let Ok((source, path, label)) = editor.update(cx, |editor, cx| {
                        let source = editor.current_document_source(cx);
                        let path = editor.file_path.clone().unwrap_or_default();
                        let label = path
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_else(|| {
                                cx.global::<I18nManager>()
                                    .strings()
                                    .workspace_current_document_label
                                    .clone()
                            });
                        (source, path, label)
                    }) else {
                        return;
                    };
                    let (results, source) = background
                        .spawn(async move {
                            let results = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                                || search_document_source(&source, &matcher, &path, &label, 200),
                            ))
                            .unwrap_or_default();
                            (results, source)
                        })
                        .await;
                    (results, Some(source))
                }
            };
            let _ = editor.update(cx, |editor, cx| {
                if editor.workspace.search_generation == generation {
                    editor.workspace.search_results = results;
                    editor.workspace.document_search_source = document_source;
                    editor.workspace.search_pending = false;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn jump_to_workspace_search_line(&mut self, line: usize, cx: &mut Context<Self>) {
        let source = self.current_document_source(cx);
        let offset = source
            .split_inclusive('\n')
            .take(line.saturating_sub(1))
            .map(str::len)
            .sum::<usize>()
            .min(source.len());
        self.apply_selection_snapshot_in_current_mode(
            &UndoSelectionSnapshot {
                range: offset..offset,
                reversed: false,
            },
            cx,
        );
        self.pending_scroll_active_block_into_view = true;
        self.pending_scroll_recheck_after_layout = true;
        cx.notify();
    }

    fn jump_to_document_search_range(&mut self, range: Range<usize>, cx: &mut Context<Self>) {
        self.apply_selection_snapshot_in_current_mode(
            &UndoSelectionSnapshot {
                range,
                reversed: false,
            },
            cx,
        );
        self.pending_scroll_active_block_into_view = true;
        self.pending_scroll_recheck_after_layout = true;
        cx.notify();
    }

    pub(crate) fn open_document_find(&mut self, cx: &mut Context<Self>) {
        self.workspace.is_open = true;
        self.workspace.active_tab = WorkspaceTab::Search;
        self.workspace.search_scope = WorkspaceSearchScope::Document;
        self.workspace.search_selected_range = 0..self.workspace.search_query.len();
        self.workspace.search_marked_range = None;
        self.workspace.search_focus_pending = true;
        self.schedule_workspace_search(cx);
        cx.notify();
    }

    pub(crate) fn on_find_in_document(
        &mut self,
        _: &crate::components::FindInDocument,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_document_find(cx);
    }

    pub(crate) fn on_find_next_match(
        &mut self,
        _: &crate::components::FindNextMatch,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.find_next_document_match(false, cx);
    }

    pub(crate) fn on_find_previous_match(
        &mut self,
        _: &crate::components::FindPreviousMatch,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.find_next_document_match(true, cx);
    }

    pub(super) fn refresh_document_find_after_edit(&mut self, cx: &mut Context<Self>) {
        if self.workspace.is_open
            && self.workspace.active_tab == WorkspaceTab::Search
            && self.workspace.search_scope == WorkspaceSearchScope::Document
            && !self.workspace.search_query.trim().is_empty()
        {
            self.schedule_workspace_search(cx);
        }
    }

    pub(super) fn apply_pending_workspace_search_focus(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.search_focus_pending {
            let focus = self
                .workspace
                .search_focus
                .get_or_insert_with(|| cx.focus_handle())
                .clone();
            window.focus(&focus);
            self.workspace.search_focus_pending = false;
        }
    }

    pub(crate) fn find_next_document_match(&mut self, reverse: bool, cx: &mut Context<Self>) {
        if self.workspace.search_scope != WorkspaceSearchScope::Document {
            return;
        }
        let Some(source) = self.workspace.document_search_source.clone() else {
            return;
        };
        let matcher = SearchMatcher::new(self.workspace.search_query.trim(), self.search_options());
        let from = self
            .workspace
            .document_active_range
            .as_ref()
            .map(|range| if reverse { range.start } else { range.end })
            .unwrap_or(if reverse { source.len() } else { 0 });
        let Some(range) =
            find_document_match_from(&source, &matcher, from, reverse).or_else(|| {
                find_document_match_from(
                    &source,
                    &matcher,
                    if reverse { source.len() } else { 0 },
                    reverse,
                )
            })
        else {
            return;
        };
        self.workspace.search_active_index = self
            .workspace
            .search_results
            .iter()
            .position(|hit| hit.source_range.as_ref() == Some(&range));
        self.workspace.document_active_range = Some(range.clone());
        self.jump_to_document_search_range(range, cx);
    }

    /// Replaces the currently active document match (selected via a jump).
    pub(super) fn replace_active_document_match(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(range) = self.workspace.document_active_range.clone() else {
            return false;
        };
        let replacement = self.workspace.replace_query.clone();
        self.apply_selection_snapshot_in_current_mode(
            &UndoSelectionSnapshot {
                range,
                reversed: false,
            },
            cx,
        );
        self.replace_selected_block_text(&replacement, window, cx)
    }

    /// Replaces every current match in the open document, last-first so the
    /// earlier byte offsets stay valid while editing.
    pub(super) fn replace_all_document_matches(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> usize {
        let Some(source) = self.workspace.document_search_source.clone() else {
            return 0;
        };
        let matcher = SearchMatcher::new(self.workspace.search_query.trim(), self.search_options());
        let replacement = self.workspace.replace_query.clone();
        let mut ranges = Vec::new();
        let mut absolute = 0usize;
        for raw_line in source.split_inclusive('\n') {
            let line = raw_line.strip_suffix('\n').unwrap_or(raw_line);
            for found in matcher.find_in_line(line) {
                ranges.push(absolute + found.start..absolute + found.end);
            }
            absolute += raw_line.len();
        }
        let mut replaced = 0usize;
        for range in ranges.iter().rev() {
            self.apply_selection_snapshot_in_current_mode(
                &UndoSelectionSnapshot {
                    range: range.clone(),
                    reversed: false,
                },
                cx,
            );
            if self.replace_selected_block_text(&replacement, window, cx) {
                replaced += 1;
            }
        }
        self.workspace.document_active_range = None;
        replaced
    }

    /// Runs `replace_text_in_range` on the block that currently holds the
    /// selection (set by `apply_selection_snapshot_in_current_mode`). The
    /// target block may be outside the visible window, so it is resolved
    /// through the full-tree location map instead of the visible snapshot.
    fn replace_selected_block_text(
        &mut self,
        replacement: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(entity_id) = self.active_entity_id else {
            return false;
        };
        let Some(block) = self.document.block_entity_at_location(entity_id, cx) else {
            return false;
        };
        block.update(cx, |block, cx| {
            block.replace_text_in_range(None, replacement, window, cx);
        });
        true
    }

    /// Replaces matches across every file in the workspace tree. Open tabs get
    /// their cached markdown rewritten (and marked dirty); the active document
    /// is replaced live through the block editor so undo still works.
    pub(super) fn replace_all_workspace_matches(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> usize {
        let Some(tree) = self.workspace.file_tree.clone() else {
            return 0;
        };
        let matcher = SearchMatcher::new(self.workspace.search_query.trim(), self.search_options());
        let replacement = self.workspace.replace_query.clone();
        let active_path = self.file_path.clone();

        let files = collect_workspace_files(&tree);
        let mut total = 0usize;
        for path in files {
            if Some(&path) == active_path.as_ref() {
                // The active document goes through the live editor path so the
                // change is undoable and stays in sync with the block model.
                let source = self.current_document_source(cx);
                let scoped = SearchMatcher::new(
                    self.workspace.search_query.trim(),
                    self.search_options(),
                );
                let count = count_matches_in_source(&source, &scoped);
                if count > 0 {
                    self.workspace.document_search_source = Some(source);
                    total += self.replace_all_document_matches(window, cx);
                }
                continue;
            }
            let open_tab_index = self
                .workspace
                .open_documents
                .iter()
                .position(|tab| tab.path == path);
            let source = match open_tab_index {
                Some(index) if self.workspace.open_documents[index].dirty => {
                    self.workspace.open_documents[index].markdown.clone()
                }
                _ => match fs::read_to_string(&path) {
                    Ok(source) => source,
                    Err(_) => continue,
                },
            };
            let updated = replace_in_source(&source, &matcher, &replacement);
            if updated == source {
                continue;
            }
            total += count_matches_in_source(&source, &matcher);
            if fs::write(&path, &updated).is_ok() {
                if let Some(index) = open_tab_index {
                    let tab = &mut self.workspace.open_documents[index];
                    tab.markdown = updated;
                    tab.dirty = true;
                    tab.file_version = super::persistence::file_content_version(&tab.markdown);
                }
            } else {
                self.workspace.file_error = Some(format!("无法写入 {}", path.display()));
            }
        }
        if total > 0 {
            self.refresh_workspace_tree(cx);
        }
        total
    }

    fn toggle_workspace_node(&mut self, id: &str, cx: &mut Context<Self>) {
        if !self.workspace.expanded.remove(id) {
            self.workspace.expanded.insert(id.to_string());
        }
        cx.notify();
    }

    /// Clicking an outline heading jumps to that heading and expands it so its
    /// children become visible.
    fn open_outline_node(&mut self, id: String, line: usize, cx: &mut Context<Self>) {
        self.workspace.selected = Some(WorkspaceSelection::Outline(id.clone()));
        self.workspace.expanded.insert(id);
        // The outline is built from `last_stable_source_text`, so compute the
        // heading's byte range against that same snapshot.
        let source = self.last_stable_source_text.clone();
        let line_start = source
            .split_inclusive('\n')
            .take(line)
            .map(str::len)
            .sum::<usize>()
            .min(source.len());
        let line_end = source[line_start..]
            .find('\n')
            .map(|offset| line_start + offset)
            .unwrap_or(source.len());
        if source.is_char_boundary(line_start) && source.is_char_boundary(line_end) {
            self.jump_to_document_search_range(line_start..line_end, cx);
        } else {
            cx.notify();
        }
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

        if !self.has_external_autosave_conflict() {
            self.workspace.file_error = None;
        }
        self.snapshot_current_document(cx);
        let cached = self
            .workspace
            .open_documents
            .iter()
            .find(|tab| tab.path == path)
            .cloned();
        let (markdown, dirty, recovery_id, file_version) = if let Some(tab) = cached {
            if tab.dirty {
                (tab.markdown, true, tab.recovery_id, tab.file_version)
            } else {
                match fs::read_to_string(&path) {
                    Ok(markdown) => {
                        let file_version = super::persistence::file_content_version(&markdown);
                        (markdown, false, tab.recovery_id, file_version)
                    }
                    Err(err) => {
                        self.workspace.file_error = Some(err.to_string());
                        cx.notify();
                        return;
                    }
                }
            }
        } else {
            match fs::read_to_string(&path) {
                Ok(markdown) => (
                    markdown.clone(),
                    false,
                    uuid::Uuid::new_v4(),
                    super::persistence::file_content_version(&markdown),
                ),
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
                recovery_id,
                file_version,
                markdown: markdown.clone(),
                dirty,
            });
        }
        if let Some(tab) = self
            .workspace
            .open_documents
            .iter_mut()
            .find(|tab| tab.path == path)
        {
            tab.markdown = markdown.clone();
            tab.dirty = dirty;
            tab.file_version = file_version;
        }
        self.recovery_id = recovery_id;
        self.file_version = Some(file_version);
        self.recovery_source_path = None;
        self.is_recovered_document = false;
        self.workspace.active_document = Some(path.clone());
        self.workspace.selected = Some(WorkspaceSelection::File(path.clone()));
        if is_code_file(&path) {
            self.replace_document_from_code_source(markdown, path, cx);
        } else {
            self.replace_document_from_markdown(markdown, Some(path), cx);
        }
        self.document_dirty = dirty;
        self.file_version = Some(file_version);
        if dirty || self.has_dirty_workspace_documents() {
            self.schedule_autosave(cx);
        }
        window.set_window_edited(dirty);
        cx.notify();
    }

    fn open_tab_context_menu(
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

    pub(super) fn close_tab_context_menu(&mut self, cx: &mut Context<Self>) {
        if self.workspace.tab_context_menu.take().is_some() {
            cx.notify();
        }
    }

    pub(super) fn render_tab_context_menu_overlay(
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

    /// Closes one tab, prompting before discarding unsaved edits.
    pub(super) fn close_workspace_document(
        &mut self,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_workspace_tabs(std::slice::from_ref(&path.to_path_buf()), window, cx);
    }

    fn close_workspace_tabs_for_action(
        &mut self,
        target: &Path,
        action: TabMenuAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let paths: Vec<PathBuf> = {
            let tabs = &self.workspace.open_documents;
            let Some(index) = tabs.iter().position(|tab| tab.path == target) else {
                return;
            };
            match action {
                TabMenuAction::Close => vec![target.to_path_buf()],
                TabMenuAction::CloseOthers => tabs
                    .iter()
                    .enumerate()
                    .filter(|(tab_index, _)| *tab_index != index)
                    .map(|(_, tab)| tab.path.clone())
                    .collect(),
                TabMenuAction::CloseLeft => tabs[..index]
                    .iter()
                    .map(|tab| tab.path.clone())
                    .collect(),
                TabMenuAction::CloseRight => tabs[index + 1..]
                    .iter()
                    .map(|tab| tab.path.clone())
                    .collect(),
                TabMenuAction::CloseAll => tabs.iter().map(|tab| tab.path.clone()).collect(),
            }
        };
        self.close_workspace_tabs(&paths, window, cx);
    }

    /// Closes the listed tabs. Unsaved edits are confirmed once for the whole
    /// batch; saving writes each tab's cached markdown back to disk.
    fn close_workspace_tabs(
        &mut self,
        paths: &[PathBuf],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if paths.is_empty() {
            return;
        }
        // Capture the live editor content into its tab before deciding what is
        // dirty, so closing the active document sees up-to-date state.
        self.snapshot_current_document(cx);
        self.dismiss_contextual_overlays(cx);

        let closing: Vec<WorkspaceDocumentTab> = self
            .workspace
            .open_documents
            .iter()
            .filter(|tab| paths.contains(&tab.path))
            .cloned()
            .collect();
        if closing.is_empty() {
            return;
        }
        let dirty: Vec<WorkspaceDocumentTab> = closing
            .iter()
            .filter(|tab| tab.dirty)
            .cloned()
            .collect();

        if dirty.is_empty() {
            // Already inside this Editor's update context (the click handler
            // wraps everything in editor.update); re-entering update here
            // would panic.
            self.finish_close_workspace_tabs(&closing, false, window, cx);
            return;
        }

        let strings = cx.global::<crate::i18n::I18nManager>().strings().clone();
        let (message, detail) = if dirty.len() == 1 {
            let name = dirty[0]
                .path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| dirty[0].path.to_string_lossy().into_owned());
            (
                strings.tab_close_dirty_message_one.replace("{name}", &name),
                String::new(),
            )
        } else {
            (
                strings
                    .tab_close_dirty_message_many
                    .replace("{count}", &dirty.len().to_string()),
                String::new(),
            )
        };
        let detail = (!detail.is_empty()).then_some(detail);
        let buttons = [
            strings.unsaved_changes_save_and_close.as_str(),
            strings.unsaved_changes_discard_and_close.as_str(),
            strings.open_link_cancel.as_str(),
        ];
        let prompt = window.prompt(
            PromptLevel::Warning,
            &message,
            detail.as_deref(),
            &buttons,
            cx,
        );
        let prompt_window = window.window_handle().downcast::<Editor>();
        cx.spawn(async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let Ok(choice) = prompt.await else {
                return;
            };
            if let Some(handle) = prompt_window {
                let _ = handle.update(cx, |editor, window, cx| match choice {
                    0 => editor.finish_close_workspace_tabs(&closing, true, window, cx),
                    1 => editor.finish_close_workspace_tabs(&closing, false, window, cx),
                    _ => {}
                });
            }
        })
        .detach();
    }

    /// Removes closed tabs from the strip, optionally saving their cached
    /// content first, and activates the nearest remaining neighbour.
    fn finish_close_workspace_tabs(
        &mut self,
        closing: &[WorkspaceDocumentTab],
        save_first: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if save_first {
            for tab in closing {
                if !tab.dirty {
                    continue;
                }
                match std::fs::write(&tab.path, tab.markdown.as_str()) {
                    Ok(()) => {
                        let _ = crate::config::remove_recovery_snapshot(tab.recovery_id);
                    }
                    Err(err) => {
                        self.workspace.file_error = Some(err.to_string());
                    }
                }
            }
        }

        let closing_paths: Vec<PathBuf> = closing.iter().map(|tab| tab.path.clone()).collect();
        let active_was_closed = closing_paths
            .iter()
            .any(|path| self.workspace.active_document.as_ref() == Some(path));
        let first_closed_index = self
            .workspace
            .open_documents
            .iter()
            .position(|tab| closing_paths.contains(&tab.path));

        self.workspace
            .open_documents
            .retain(|tab| !closing_paths.contains(&tab.path));

        if active_was_closed {
            let next_path = first_closed_index.and_then(|index| {
                self.workspace
                    .open_documents
                    .get(index)
                    .or_else(|| {
                        index
                            .checked_sub(1)
                            .and_then(|previous| self.workspace.open_documents.get(previous))
                    })
                    .map(|tab| tab.path.clone())
            });
            // Detach the closing document from the editor first so snapshot /
            // dirty-guard logic inside open_workspace_file cannot resurrect it.
            self.file_path = None;
            self.document_dirty = false;
            if let Some(next_path) = next_path {
                self.open_workspace_file(next_path, window, cx);
            } else {
                self.workspace.active_document = None;
                self.workspace.selected = None;
                self.replace_document_from_markdown(String::new(), None, cx);
                window.set_window_edited(false);
            }
        }
        if self.document_dirty || self.has_dirty_workspace_documents() {
            self.schedule_autosave(cx);
        }
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
        let tabs = self
            .workspace
            .open_documents
            .iter()
            .enumerate()
            .map(|(index, tab)| {
                let path = tab.path.clone();
                let click_path = path.clone();
                let close_path = path.clone();
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
                let context_editor = editor.clone();
                // The close button carries its own hover state: highlighting
                // the whole tab was indistinguishable from the tab's own
                // hover background, so the X lights up only under the pointer.
                let close_button = div()
                    .id(("document-tab-close", index))
                    .group("doc-tab-close")
                    .w(px(16.0))
                    .h(px(16.0))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(4.0))
                    .hover(|this| {
                        // One step darker than the tab's own hover fill —
                        // neutral, no loud accent colors.
                        this.bg({
                            let mut bg = c.dialog_secondary_button_hover;
                            bg.l = (bg.l - 0.05).max(0.0);
                            bg
                        })
                    })
                    .cursor_pointer()
                    .child({
                        // Faded body color rather than dialog_muted: inactive
                        // tabs show no other text, so the X needs to stand on
                        // its own while staying quieter than the tab title.
                        let mut idle_icon = c.text_default;
                        idle_icon.a *= 0.6;
                        svg()
                            .path(TAB_CLOSE_ICON)
                            .size(px(10.0))
                            .text_color(idle_icon)
                            .group_hover("doc-tab-close", |this| this.text_color(c.text_default))
                    })
                    .on_click({
                        let close_editor = editor.clone();
                        move |_event, window, cx| {
                            let _ = close_editor.update(cx, |editor, cx| {
                                editor.close_workspace_document(&close_path, window, cx);
                            });
                            cx.stop_propagation();
                        }
                    })
                    .into_any_element();
                div()
                    .id(("document-tab", stable_node_hash(&path.to_string_lossy())))
                    .group("doc-tab")
                    .h_full()
                    .min_w(px(120.0))
                    .max_w(px(220.0))
                    .px(px(10.0))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .relative()
                    .border_r(px(1.0))
                    .border_color(c.dialog_border)
                    .bg(if active {
                        c.editor_background
                    } else {
                        hsla(0.0, 0.0, 0.0, 0.0)
                    })
                    .hover(|this| {
                        this.bg(if active {
                            c.editor_background
                        } else {
                            c.dialog_secondary_button_hover
                        })
                    })
                    .cursor_pointer()
                    .text_size(px(12.0))
                    .text_color(if active {
                        c.text_default
                    } else {
                        c.dialog_muted
                    })
                    .children(active.then(|| {
                        div()
                            .absolute()
                            .bottom_0()
                            .left_0()
                            .right_0()
                            .h(px(2.0))
                            .bg(c.dialog_primary_button_bg)
                    }))
                    .child(
                        div()
                            .w(px(11.0))
                            .text_center()
                            .text_size(px(9.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(if is_code_file(&path) {
                                c.dialog_muted
                            } else {
                                c.dialog_primary_button_bg
                            })
                            .child(if is_code_file(&path) { "⌘" } else { "M" }),
                    )
                    .child(div().flex_1().min_w(px(0.0)).truncate().child(title))
                    .children((dirty && !active).then(|| {
                        div()
                            .w(px(7.0))
                            .h(px(7.0))
                            .flex_shrink_0()
                            .rounded(px(4.0))
                            .bg(c.dialog_primary_button_bg)
                    }))
                    .child(close_button)
                    .on_click(move |_event, window, cx| {
                        let _ = tab_editor.update(cx, |editor, cx| {
                            editor.open_workspace_file(click_path.clone(), window, cx);
                        });
                    })
                    .on_mouse_down(MouseButton::Right, move |event, _window, cx| {
                        let _ = context_editor.update(cx, |editor, cx| {
                            editor.open_tab_context_menu(event.position, index, cx);
                        });
                        cx.stop_propagation();
                    })
                    .into_any_element()
            })
            .collect::<Vec<_>>();

        Some(
            div()
                .id("document-tabs")
                .h_full()
                .min_w(px(0.0))
                .flex()
                .overflow_x_scroll()
                .children(tabs)
                .into_any_element(),
        )
    }

    pub(super) fn code_tab_active(&self) -> bool {
        self.code_document
    }

    pub(super) fn workspace_breadcrumb(&self) -> String {
        self.file_path
            .as_ref()
            .or(self.recovery_source_path.as_ref())
            .and_then(|path| path.file_name())
            .or_else(|| {
                self.workspace
                    .root
                    .as_ref()
                    .and_then(|path| path.file_name())
            })
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    pub(super) fn active_code_line_count(&self, cx: &App) -> Option<usize> {
        self.code_tab_active().then(|| {
            let source = self.document.raw_source_text(cx);
            if source.is_empty() {
                1
            } else {
                source.split('\n').count()
            }
        })
    }

    pub(super) fn current_workspace_panel_width(&self, viewport_width: f32, cx: &App) -> f32 {
        let width = self
            .workspace
            .panel_width
            .unwrap_or_else(|| crate::config::EditorSettings::workspace_sidebar_width(cx) as f32);
        clamp_workspace_panel_width(width, viewport_width)
    }

    fn start_workspace_resize(&mut self, pointer_x: f32, width: f32, cx: &mut Context<Self>) {
        self.workspace.resize_drag = Some(WorkspaceResizeDrag {
            start_x: pointer_x,
            start_width: width,
        });
        cx.notify();
    }

    pub(super) fn on_workspace_resize_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(drag) = self.workspace.resize_drag else {
            return;
        };
        let viewport = f32::from(window.viewport_size().width);
        let width = clamp_workspace_panel_width(
            drag.start_width + f32::from(event.position.x) - drag.start_x,
            viewport,
        );
        self.workspace.panel_width = Some(width);
        cx.notify();
    }

    pub(super) fn on_workspace_resize_mouse_up(
        &mut self,
        _event: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.resize_drag.take().is_some() {
            if let Some(width) = self.workspace.panel_width {
                crate::config::EditorSettings::set_workspace_sidebar_width(
                    cx,
                    width.round() as u16,
                );
            }
            cx.notify();
        }
    }

    pub(super) fn render_workspace_panel(
        &mut self,
        theme: &Theme,
        strings: &I18nStrings,
        panel_width: f32,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.workspace.is_open {
            return None;
        }

        self.sync_workspace_models(cx);
        let editor = cx.entity().downgrade();
        let resize_editor = editor.clone();
        let c = &theme.colors;
        let d = &theme.dimensions;

        let search_header = (self.workspace.active_tab == WorkspaceTab::Search)
            .then(|| self.render_search_header(theme, strings, window, cx));
        let body = match self.workspace.active_tab {
            WorkspaceTab::Files => self.render_workspace_files_tree(theme, strings, &editor),
            WorkspaceTab::Search => self.render_search_results(theme, strings, &editor),
            WorkspaceTab::Outline => self.render_workspace_outline_tree(theme, strings, &editor),
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
                .child(
                    div()
                        .id("workspace-panel-scroll")
                        .flex_1()
                        .min_h(px(0.0))
                        .overflow_y_scroll()
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

    /// VS Code-style search header: query input, collapsible replace input,
    /// option toggles, and a document/workspace scope switch. Rendered above
    /// the result list when the Search tab is active.
    fn render_search_header(
        &mut self,
        theme: &Theme,
        strings: &I18nStrings,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = &theme.colors;
        let editor = cx.entity().downgrade();

        let query_focused = self
            .workspace
            .search_focus
            .as_ref()
            .is_some_and(|focus| focus.is_focused(window));
        let search_input = self.render_search_input(
            "workspace-search-query",
            self.workspace.search_query.clone(),
            strings.workspace_search_placeholder.clone(),
            SearchInputKind::Query,
            query_focused,
            theme,
            cx,
        );
        let replace_visible = self.workspace.replace_visible;
        let replace_input = replace_visible.then(|| {
            let replace_focused = self
                .workspace
                .replace_focus
                .as_ref()
                .is_some_and(|focus| focus.is_focused(window));
            self.render_search_input(
                "workspace-search-replace",
                self.workspace.replace_query.clone(),
                strings.search_replace_placeholder.clone(),
                SearchInputKind::Replace,
                replace_focused,
                theme,
                cx,
            )
        });

        let toggle_editor = editor.clone();
        let toggle_button = div()
            .id("workspace-search-toggle-replace")
            .w(px(24.0))
            .h(px(24.0))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(5.0))
            .hover(|this| this.bg(c.dialog_secondary_button_hover))
            .cursor_pointer()
            .text_color(if replace_visible {
                c.dialog_primary_button_bg
            } else {
                c.dialog_muted
            })
            .child(
                svg()
                    .path(if replace_visible {
                        CHEVRON_DOWN_ICON
                    } else {
                        CHEVRON_RIGHT_ICON
                    })
                    .size(px(14.0))
                    .text_color(if replace_visible {
                        c.dialog_primary_button_bg
                    } else {
                        c.dialog_muted
                    }),
            )
            .tooltip(|_, cx| {
                cx.new(|_| WorkspaceTooltip {
                    label: "显示替换".into(),
                })
                .into()
            })
            .on_click(move |_, _, cx| {
                let _ = toggle_editor.update(cx, |editor, cx| {
                    editor.workspace.replace_visible = !editor.workspace.replace_visible;
                    cx.notify();
                });
            });

        let options_row = self.render_search_options_row(theme, strings, cx);

        let header = div()
            .id("workspace-search-header")
            .w_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .px(px(6.0))
            .pt(px(8.0))
            .pb(px(2.0))
            .border_b(px(1.0))
            .border_color(c.dialog_border)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .child(search_input)
                    .child(toggle_button),
            )
            .children(replace_input)
            .child(options_row);
        header.into_any_element()
    }

    /// Option toggles (case / whole word / regex / fuzzy), the scope switch,
    /// and — for the document scope — the replace action buttons.
    fn render_search_options_row(
        &mut self,
        theme: &Theme,
        strings: &I18nStrings,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = &theme.colors;
        let editor = cx.entity().downgrade();

        let toggle_chip = |editor: &WeakEntity<Self>,
                           id: &'static str,
                           label: String,
                           selected: bool,
                           tooltip: String| {
            let chip_editor = editor.clone();
            div()
                .id(id)
                .px(px(6.0))
                .h(px(22.0))
                .flex()
                .items_center()
                .rounded(px(4.0))
                .border_1()
                .border_color(if selected {
                    c.dialog_primary_button_bg
                } else {
                    c.dialog_border
                })
                .bg(if selected {
                    c.selection
                } else {
                    hsla(0.0, 0.0, 0.0, 0.0)
                })
                .text_size(px(11.0))
                .text_color(if selected {
                    c.dialog_primary_button_bg
                } else {
                    c.dialog_muted
                })
                .cursor_pointer()
                .hover(|this| this.bg(c.dialog_secondary_button_hover))
                .tooltip(move |_, cx| {
                    let tooltip = tooltip.clone();
                    cx.new(|_| WorkspaceTooltip { label: tooltip }).into()
                })
                .child(label)
                .on_click(move |_, _, cx| {
                    let _ = chip_editor.update(cx, |editor, cx| {
                        match id {
                            "workspace-search-case" => {
                                editor.workspace.search_match_case =
                                    !editor.workspace.search_match_case;
                            }
                            "workspace-search-word" => {
                                editor.workspace.search_whole_word =
                                    !editor.workspace.search_whole_word;
                            }
                            "workspace-search-regex" => {
                                editor.workspace.search_use_regex =
                                    !editor.workspace.search_use_regex;
                            }
                            "workspace-search-fuzzy" => {
                                editor.workspace.search_fuzzy = !editor.workspace.search_fuzzy;
                            }
                            _ => {}
                        }
                        editor.schedule_workspace_search(cx);
                        cx.notify();
                    });
                })
        };

        let mut options = div()
            .id("workspace-search-options")
            .w_full()
            .flex()
            .items_center()
            .flex_wrap()
            .gap(px(4.0))
            .child(toggle_chip(
                &editor,
                "workspace-search-case",
                "Aa".to_string(),
                self.workspace.search_match_case,
                strings.search_case_sensitive.clone(),
            ))
            .child(toggle_chip(
                &editor,
                "workspace-search-word",
                "ab".to_string(),
                self.workspace.search_whole_word,
                strings.search_whole_word.clone(),
            ))
            .child(toggle_chip(
                &editor,
                "workspace-search-regex",
                ".*".to_string(),
                self.workspace.search_use_regex,
                strings.search_regex.clone(),
            ))
            .child(toggle_chip(
                &editor,
                "workspace-search-fuzzy",
                strings.search_fuzzy_short.clone(),
                self.workspace.search_fuzzy,
                strings.search_fuzzy.clone(),
            ));

        // Scope switch: current document vs. whole workspace (the latter needs
        // a scanned tree).
        let scope = self.workspace.search_scope;
        let has_tree = self.workspace.file_tree.is_some();
        let scope_button =
            |editor: &WeakEntity<Self>, id: &'static str, label: String, selected: bool| {
                let scope_editor = editor.clone();
                div()
                    .id(id)
                    .px(px(6.0))
                    .h(px(22.0))
                    .flex()
                    .items_center()
                    .rounded(px(4.0))
                    .bg(if selected {
                        c.selection
                    } else {
                        hsla(0.0, 0.0, 0.0, 0.0)
                    })
                    .text_size(px(11.0))
                    .text_color(if selected {
                        c.dialog_primary_button_bg
                    } else {
                        c.dialog_muted
                    })
                    .cursor_pointer()
                    .hover(|this| this.bg(c.dialog_secondary_button_hover))
                    .child(label)
                    .on_click(move |_, _, cx| {
                        let _ = scope_editor.update(cx, |editor, cx| {
                            editor.workspace.search_scope = match id {
                                "workspace-scope-document" => WorkspaceSearchScope::Document,
                                _ => WorkspaceSearchScope::Workspace,
                            };
                            editor.workspace.search_active_index = None;
                            editor.workspace.document_active_range = None;
                            editor.schedule_workspace_search(cx);
                            cx.notify();
                        });
                    })
            };
        let match_count = self.workspace.search_results.len();
        let count_label = if match_count > 0 && scope == WorkspaceSearchScope::Document {
            Some(
                strings
                    .search_result_count
                    .replace("{n}", &match_count.to_string()),
            )
        } else {
            None
        };

        options = options.child(
            div()
                .id("workspace-search-scope-row")
                .w_full()
                .flex()
                .items_center()
                .gap(px(4.0))
                .child(scope_button(
                    &editor,
                    "workspace-scope-document",
                    strings.search_scope_document.clone(),
                    scope == WorkspaceSearchScope::Document,
                ))
                .child(scope_button(
                    &editor,
                    "workspace-scope-workspace",
                    strings.search_scope_workspace.clone(),
                    scope == WorkspaceSearchScope::Workspace,
                ))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .flex()
                        .justify_end()
                        .children(count_label.map(|label| {
                            div()
                                .text_size(px(11.0))
                                .text_color(c.dialog_muted)
                                .child(label)
                        })),
                ),
        );

        // Replace actions (document scope only for now; workspace replace
        // lives behind the same buttons when the workspace scope is active).
        if self.workspace.replace_visible && !self.workspace.replace_query.is_empty() {
            let replace_editor = editor.clone();
            let replace_all_editor = editor.clone();
            let is_document_scope = scope == WorkspaceSearchScope::Document;
            let replace_row = div()
                .w_full()
                .flex()
                .items_center()
                .justify_end()
                .gap(px(6.0))
                .child(
                    div()
                        .id("workspace-search-replace-current")
                        .px(px(8.0))
                        .h(px(24.0))
                        .flex()
                        .items_center()
                        .rounded(px(5.0))
                        .border_1()
                        .border_color(c.dialog_border)
                        .text_size(px(11.0))
                        .text_color(c.text_default)
                        .cursor_pointer()
                        .hover(|this| this.bg(c.dialog_secondary_button_hover))
                        .child(strings.search_replace_current.clone())
                        .on_click(move |_, window, cx| {
                            let _ = replace_editor.update(cx, |editor, cx| {
                                if is_document_scope {
                                    editor.replace_active_document_match(window, cx);
                                }
                                cx.notify();
                            });
                            cx.stop_propagation();
                        }),
                )
                .child(
                    div()
                        .id("workspace-search-replace-all")
                        .px(px(8.0))
                        .h(px(24.0))
                        .flex()
                        .items_center()
                        .rounded(px(5.0))
                        .border_1()
                        .border_color(c.dialog_border)
                        .text_size(px(11.0))
                        .text_color(c.text_default)
                        .cursor_pointer()
                        .hover(|this| this.bg(c.dialog_secondary_button_hover))
                        .child(strings.search_replace_all.clone())
                        .on_click(move |_, window, cx| {
                            let _ = replace_all_editor.update(cx, |editor, cx| {
                                if is_document_scope {
                                    editor.replace_all_document_matches(window, cx);
                                } else {
                                    let replaced =
                                        editor.replace_all_workspace_matches(window, cx);
                                    if replaced > 0 {
                                        editor.schedule_workspace_search(cx);
                                    }
                                }
                                cx.notify();
                            });
                            cx.stop_propagation();
                        }),
                );
            options = options.child(replace_row);
        }

        options.into_any_element()
    }

    /// Shared single-line input used by the query and replace fields. Clicks
    /// focus the field; key handling and IME route through `SearchInputKind`.
    fn render_search_input(
        &mut self,
        id: &'static str,
        value: String,
        placeholder: String,
        kind: SearchInputKind,
        focused: bool,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = &theme.colors;
        let focus = match kind {
            SearchInputKind::Query => self
                .workspace
                .search_focus
                .get_or_insert_with(|| cx.focus_handle())
                .clone(),
            SearchInputKind::Replace => self
                .workspace
                .replace_focus
                .get_or_insert_with(|| cx.focus_handle())
                .clone(),
        };
        let focus_for_click = focus.clone();
        let focus_for_input = focus.clone();
        let input_editor = cx.entity();
        let editor = cx.entity().downgrade();

        // The placeholder disappears as soon as the field is focused, not
        // just once text is typed.
        let (label, muted) = if !value.is_empty() {
            (value, false)
        } else if focused {
            (String::new(), true)
        } else {
            (placeholder, true)
        };

        div()
            .id(id)
            .relative()
            .track_focus(&focus)
            .flex_1()
            .min_w(px(0.0))
            .h(px(28.0))
            .px(px(8.0))
            .flex()
            .items_center()
            .rounded(px(6.0))
            .border_1()
            .border_color(if focused {
                c.dialog_primary_button_bg
            } else {
                c.dialog_border
            })
            .bg(c.editor_background)
            .text_size(px(12.0))
            .text_color(if muted {
                c.dialog_muted
            } else {
                c.text_default
            })
            .child(label)
            .child(
                canvas(
                    |_, _, _| (),
                    move |bounds, _, window, cx| {
                        window.handle_input(
                            &focus_for_input,
                            ElementInputHandler::new(bounds, input_editor.clone()),
                            cx,
                        );
                    },
                )
                .absolute()
                .top_0()
                .right_0()
                .bottom_0()
                .left_0(),
            )
            .on_click(move |_event, window, _cx| window.focus(&focus_for_click))
            .on_key_down(move |event: &KeyDownEvent, window, cx| {
                let key = event.keystroke.key.to_ascii_lowercase();
                let secondary = event.keystroke.modifiers.secondary();
                match key.as_str() {
                    "escape" => {
                        let _ = editor.update(cx, |editor, cx| match kind {
                            SearchInputKind::Query => {
                                editor.workspace.search_query.clear();
                                editor.workspace.search_selected_range = 0..0;
                                editor.workspace.search_marked_range = None;
                                editor.workspace.active_tab = WorkspaceTab::Files;
                                editor.workspace.search_focus_pending = false;
                                editor.schedule_workspace_search(cx);
                            }
                            SearchInputKind::Replace => {
                                editor.workspace.replace_visible = false;
                            }
                        });
                    }
                    "a" if secondary => {
                        let _ = editor.update(cx, |editor, cx| match kind {
                            SearchInputKind::Query => {
                                editor.workspace.search_selected_range =
                                    0..editor.workspace.search_query.len();
                            }
                            SearchInputKind::Replace => {
                                editor.workspace.replace_selected_range =
                                    0..editor.workspace.replace_query.len();
                            }
                        });
                    }
                    "backspace" => {
                        let handled = editor.update(cx, |editor, cx| {
                            let (text, selected, marked) = match kind {
                                SearchInputKind::Query => (
                                    editor.workspace.search_query.clone(),
                                    editor.workspace.search_selected_range.clone(),
                                    editor.workspace.search_marked_range.clone(),
                                ),
                                SearchInputKind::Replace => (
                                    editor.workspace.replace_query.clone(),
                                    editor.workspace.replace_selected_range.clone(),
                                    editor.workspace.replace_marked_range.clone(),
                                ),
                            };
                            if marked.is_some() {
                                return false;
                            }
                            let range = if selected.start == selected.end {
                                let before = &text[..selected.start];
                                let start = before
                                    .grapheme_indices(true)
                                    .last()
                                    .map(|(start, _)| start)
                                    .unwrap_or(selected.start);
                                start..selected.start
                            } else {
                                selected
                            };
                            editor.replace_search_input_text(kind, range, "", None, false, cx);
                            true
                        });
                        if !matches!(handled, Ok(true)) {
                            return;
                        }
                    }
                    "v" if secondary => {
                        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                            let _ = editor.update(cx, |editor, cx| {
                                let selected = match kind {
                                    SearchInputKind::Query => {
                                        editor.workspace.search_selected_range.clone()
                                    }
                                    SearchInputKind::Replace => {
                                        editor.workspace.replace_selected_range.clone()
                                    }
                                };
                                editor.replace_search_input_text(
                                    kind, selected, &text, None, false, cx,
                                );
                            });
                        }
                    }
                    "enter" => {
                        let reverse = event.keystroke.modifiers.shift;
                        let _ = editor.update(cx, |editor, cx| match kind {
                            SearchInputKind::Query => {
                                editor.find_next_document_match(reverse, cx);
                            }
                            SearchInputKind::Replace => {
                                editor.replace_active_document_match(window, cx);
                            }
                        });
                    }
                    _ => return,
                }
                cx.stop_propagation();
            })
            .into_any_element()
    }

    pub(super) fn render_activity_rail(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let c = &theme.colors;
        let editor = cx.entity().downgrade();
        let button = |id: &'static str,
                      icon_path: &'static str,
                      label: &'static str,
                      selected: bool,
                      tab: WorkspaceTab,
                      toggle_if_active: bool,
                      editor: WeakEntity<Self>| {
            div()
                .id(id)
                .w(px(34.0))
                .h(px(34.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(8.0))
                .bg(if selected {
                    c.selection
                } else {
                    c.dialog_secondary_button_bg
                })
                .hover(|this| this.bg(c.dialog_secondary_button_hover))
                .cursor_pointer()
                .child(
                    svg()
                        .path(icon_path)
                        .size(px(18.0))
                        .text_color(if selected {
                            c.dialog_primary_button_bg
                        } else {
                            c.dialog_muted
                        }),
                )
                .tooltip(move |_, cx| {
                    cx.new(|_| WorkspaceTooltip {
                        label: label.into(),
                    })
                    .into()
                })
                .on_click(move |_, _, cx| {
                    let _ = editor.update(cx, |editor, cx| {
                        if toggle_if_active
                            && editor.workspace.is_open
                            && editor.workspace.active_tab == tab
                        {
                            editor.workspace.is_open = false;
                            cx.notify();
                            return;
                        }
                        editor.workspace.is_open = true;
                        editor.set_workspace_tab(tab, cx);
                        cx.notify();
                    });
                })
        };
        div()
            .id("workspace-activity-rail")
            .w(px(50.0))
            .h_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(10.0))
            .pt(px(12.0))
            .bg(c.dialog_secondary_button_bg)
            .border_r(px(1.0))
            .border_color(c.dialog_border)
            .child(button(
                "activity-files",
                ACTIVITY_FILES_ICON,
                "文件",
                self.workspace.is_open && self.workspace.active_tab == WorkspaceTab::Files,
                WorkspaceTab::Files,
                true,
                editor.clone(),
            ))
            .child(button(
                "activity-search",
                ACTIVITY_SEARCH_ICON,
                "搜索",
                self.workspace.is_open && self.workspace.active_tab == WorkspaceTab::Search,
                WorkspaceTab::Search,
                true,
                editor.clone(),
            ))
            .child(button(
                "activity-outline",
                ACTIVITY_OUTLINE_ICON,
                "大纲",
                self.workspace.is_open && self.workspace.active_tab == WorkspaceTab::Outline,
                WorkspaceTab::Outline,
                false,
                editor,
            ))
            .into_any_element()
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

    fn render_search_results(
        &self,
        theme: &Theme,
        strings: &I18nStrings,
        editor: &WeakEntity<Editor>,
    ) -> AnyElement {
        if self.workspace.search_pending {
            return div()
                .p(px(12.0))
                .text_size(px(14.0))
                .text_color(theme.colors.dialog_muted)
                .child("…")
                .into_any_element();
        }
        if self.workspace.search_results.is_empty() {
            return self.render_workspace_empty_state(
                "",
                if self.workspace.search_scope == WorkspaceSearchScope::Document {
                    &strings.workspace_no_document_find_results
                } else {
                    &strings.workspace_no_search_results
                },
                theme,
            );
        }
        let c = &theme.colors;
        let is_document_scope = self.workspace.search_scope == WorkspaceSearchScope::Document;
        let mut elements: Vec<AnyElement> = Vec::new();
        let mut current_file: Option<PathBuf> = None;
        let mut file_hit_count = 0usize;
        for (index, hit) in self.workspace.search_results.iter().enumerate() {
            // Workspace scope groups hits under a file header row; document
            // scope lists matches flat with the file name on each row.
            if !is_document_scope && current_file.as_ref() != Some(&hit.path) {
                current_file = Some(hit.path.clone());
                file_hit_count = self
                    .workspace
                    .search_results
                    .iter()
                    .filter(|other| other.path == hit.path)
                    .count();
                elements.push(
                    div()
                        .w_full()
                        .px(px(6.0))
                        .pt(px(6.0))
                        .pb(px(2.0))
                        .flex()
                        .items_center()
                        .gap(px(4.0))
                        .child(
                            svg()
                                .path(if is_code_file(&hit.path) {
                                    CODE_ICON
                                } else {
                                    MARKDOWN_ICON
                                })
                                .size(px(13.0))
                                .text_color(c.dialog_primary_button_bg),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.0))
                                .truncate()
                                .text_size(px(11.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(c.text_default)
                                .child(hit.label.clone()),
                        )
                        .child(
                            div()
                                .text_size(px(10.0))
                                .text_color(c.dialog_muted)
                                .child(file_hit_count.to_string()),
                        )
                        .into_any_element(),
                );
            }
            let selected = self.workspace.search_active_index == Some(index);
            let hit_editor = editor.clone();
            let show_file_label = is_document_scope;
            let label = hit.label.clone();
            let line_number = hit.line;
            let preview = hit.preview.clone();
            elements.push(
                div()
                    .id(("workspace-search-hit", index))
                    .w_full()
                    .pl(px(if is_document_scope { 10.0 } else { 24.0 }))
                    .pr(px(6.0))
                    .py(px(4.0))
                    .flex()
                    .flex_col()
                    .gap(px(1.0))
                    .rounded(px(5.0))
                    .bg(if selected {
                        c.selection
                    } else {
                        hsla(0.0, 0.0, 0.0, 0.0)
                    })
                    .cursor_pointer()
                    .hover(|this| this.bg(c.dialog_secondary_button_hover))
                    .children(show_file_label.then(|| {
                        div()
                            .truncate()
                            .text_size(px(11.0))
                            .text_color(c.text_default)
                            .child(label)
                    }))
                    .children(line_number.map(|line| {
                        div()
                            .flex()
                            .gap(px(6.0))
                            .min_w(px(0.0))
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .text_color(c.dialog_muted)
                                    .child(format!("{line}")),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.0))
                                    .truncate()
                                    .text_size(px(11.0))
                                    .text_color(c.text_default)
                                    .child(preview),
                            )
                    }))
                    .on_click(move |event, window, cx| {
                        if !event.standard_click() {
                            return;
                        }
                        let _ = hit_editor.update(cx, |editor, cx| {
                            editor.open_search_hit(index, window, cx);
                        });
                    })
                    .into_any_element(),
            );
        }
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(1.0))
            .children(elements)
            .into_any_element()
    }

    /// Opens the hit's match: document-scope hits select the byte range in the
    /// live document; workspace-scope hits open the file first, then select
    /// the match using its line/column information.
    fn open_search_hit(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(hit) = self.workspace.search_results.get(index) else {
            return;
        };
        self.workspace.search_active_index = Some(index);
        if let Some(range) = hit.source_range.clone() {
            self.workspace.document_active_range = Some(range.clone());
            self.jump_to_document_search_range(range, cx);
            return;
        }
        let path = hit.path.clone();
        let line = hit.line;
        let match_range = hit.match_range.clone();
        self.open_workspace_file(path.clone(), window, cx);
        if self.file_path.as_ref() == Some(&path)
            && let (Some(line), Some(match_range)) = (line, match_range)
        {
            let source = self.current_document_source(cx);
            let line_start = source
                .split_inclusive('\n')
                .take(line.saturating_sub(1))
                .map(str::len)
                .sum::<usize>()
                .min(source.len());
            let start = (line_start + match_range.start).min(source.len());
            let end = (line_start + match_range.end).min(source.len());
            if source.is_char_boundary(start) && source.is_char_boundary(end) {
                self.jump_to_document_search_range(start..end, cx);
            }
        }
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
                    WorkspaceTreeKind::MarkdownFile(path) | WorkspaceTreeKind::CodeFile(path) => {
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
                        editor.open_workspace_file(path, window, cx);
                    }
                    WorkspaceTreeKind::CodeFile(path) => {
                        editor.open_workspace_file(path, window, cx);
                    }
                    WorkspaceTreeKind::Heading { line, .. } => {
                        editor.open_outline_node(node_id, line, cx)
                    }
                });
            })
            .into_any_element()
    }
}

fn is_markdown_file(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.to_string_lossy().eq_ignore_ascii_case("md"))
}

fn create_workspace_file(path: &Path) -> std::io::Result<()> {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map(|_| ())
}

fn create_workspace_folder(path: &Path) -> std::io::Result<()> {
    std::fs::create_dir(path)
}

fn remap_moved_path(
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

fn inline_image_destination_range(source: &str, range: Range<usize>) -> Option<Range<usize>> {
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

fn rewrite_relative_image_destination(
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

fn rewrite_relative_image_targets(
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

fn normalize_reference_id(id: &str) -> String {
    id.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn reference_definition_target_range(
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

fn normalize_path(path: &Path) -> PathBuf {
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

fn relative_path_between(from: &Path, to: &Path) -> Option<PathBuf> {
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

fn path_is_affected(path: &Path, target: &Path, target_is_directory: bool) -> bool {
    if target_is_directory {
        path.starts_with(target)
    } else {
        path == target
    }
}

/// Match options for workspace/document search, mirroring VS Code's toggles.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct SearchOptions {
    pub(super) match_case: bool,
    pub(super) whole_word: bool,
    pub(super) use_regex: bool,
    pub(super) fuzzy: bool,
}

/// Compiled search query. Regex compilation failures degrade to a plain
/// substring search so a bad pattern never silently kills search.
pub(super) struct SearchMatcher {
    query: String,
    options: SearchOptions,
    regex: Option<regex::Regex>,
}

impl SearchMatcher {
    pub(super) fn new(query: &str, options: SearchOptions) -> Self {
        let regex = if options.use_regex {
            let mut builder = regex::RegexBuilder::new(query);
            builder.case_insensitive(!options.match_case);
            builder.build().ok()
        } else {
            None
        };
        Self {
            query: query.to_string(),
            options,
            regex,
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.query.trim().is_empty() && self.regex.is_none()
    }

    /// Byte ranges of every match inside `line`.
    pub(super) fn find_in_line(&self, line: &str) -> Vec<Range<usize>> {
        if let Some(regex) = self.regex.as_ref() {
            return regex
                .find_iter(line)
                .map(|m| m.start()..m.end())
                .collect();
        }
        if self.options.fuzzy {
            return fuzzy_subsequence_ranges(line, &self.query);
        }
        let mut ranges = if self.options.match_case {
            line.match_indices(&self.query)
                .map(|(start, matched)| start..start + matched.len())
                .collect()
        } else {
            case_insensitive_ranges(line, &self.query)
        };
        if self.options.whole_word {
            ranges.retain(|range| is_word_boundary(line, range));
        }
        ranges
    }

    /// Whether a filename matches (fuzzy subsequence or substring).
    pub(super) fn matches_filename(&self, name: &str) -> bool {
        if self.options.fuzzy {
            return !fuzzy_subsequence_ranges(name, &self.query).is_empty();
        }
        if self.options.match_case {
            name.contains(&self.query)
        } else {
            case_insensitive_contains(name, &self.query)
        }
    }
}

fn case_insensitive_contains(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    if needle.is_ascii() {
        haystack
            .as_bytes()
            .windows(needle.len())
            .any(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
    } else {
        !case_insensitive_ranges(haystack, needle).is_empty()
    }
}

/// Case-insensitive byte ranges for one line. ASCII needles use a fast
/// sliding compare; non-ASCII needles fall back to per-char lowercase
/// comparison (haystack byte offsets stay stable because lowercase folding
/// of a char never splits the position bookkeeping below).
fn case_insensitive_ranges(line: &str, query: &str) -> Vec<Range<usize>> {
    if query.is_empty() || line.is_empty() || line.len() < query.len() {
        return Vec::new();
    }
    if query.is_ascii() {
        let mut ranges = Vec::new();
        let last = line.len() - query.len();
        let bytes = line.as_bytes();
        let mut start = 0;
        while start <= last {
            if bytes[start..start + query.len()].eq_ignore_ascii_case(query.as_bytes()) {
                ranges.push(start..start + query.len());
                start += query.len();
            } else {
                start += 1;
            }
        }
        return ranges;
    }

    let query_chars: Vec<char> = query.to_lowercase().chars().collect();
    if query_chars.is_empty() {
        return Vec::new();
    }
    let mut ranges = Vec::new();
    let char_positions: Vec<(usize, char)> = line.char_indices().collect();
    for start_index in 0..char_positions.len() {
        let mut query_index = 0usize;
        let mut cursor = start_index;
        while cursor < char_positions.len() && query_index < query_chars.len() {
            let (_, line_char) = char_positions[cursor];
            let mut folded = line_char.to_lowercase();
            let matches = match (folded.next(), folded.next()) {
                (Some(first), None) => first == query_chars[query_index],
                _ => line_char == query_chars[query_index],
            };
            if !matches {
                break;
            }
            query_index += 1;
            cursor += 1;
        }
        if query_index == query_chars.len() {
            let start = char_positions[start_index].0;
            let end = if cursor < char_positions.len() {
                char_positions[cursor].0
            } else {
                line.len()
            };
            ranges.push(start..end);
        }
    }
    ranges
}

/// fzf-style subsequence match: every query char must appear in order
/// (case-insensitive); the returned range spans the first to last consumed
/// char so the hit can be selected on jump.
fn fuzzy_subsequence_ranges(line: &str, query: &str) -> Vec<Range<usize>> {
    let query_chars: Vec<char> = query
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .flat_map(|ch| ch.to_lowercase())
        .collect();
    if query_chars.is_empty() || line.is_empty() {
        return Vec::new();
    }
    let positions: Vec<(usize, char)> = line.char_indices().collect();
    let mut ranges = Vec::new();
    for start_index in 0..positions.len() {
        let mut query_index = 0usize;
        let mut cursor = start_index;
        while cursor < positions.len() && query_index < query_chars.len() {
            let (_, line_char) = positions[cursor];
            let folded = line_char.to_lowercase().next().unwrap_or(line_char);
            if folded == query_chars[query_index] {
                query_index += 1;
            }
            cursor += 1;
        }
        if query_index == query_chars.len() {
            let start = positions[start_index].0;
            let end = positions
                .get(cursor)
                .map(|(offset, _)| *offset)
                .unwrap_or(line.len());
            ranges.push(start..end);
        }
    }
    ranges
}

fn is_word_boundary(line: &str, range: &Range<usize>) -> bool {
    let word_char = |ch: char| ch.is_alphanumeric() || ch == '_';
    let before = line[..range.start]
        .chars()
        .next_back()
        .map(word_char)
        .unwrap_or(false);
    let after = line[range.end..]
        .chars()
        .next()
        .map(word_char)
        .unwrap_or(false);
    !before && !after
}

fn search_workspace_files(
    root: &WorkspaceTreeNode,
    matcher: &SearchMatcher,
    limit: usize,
) -> Vec<WorkspaceSearchHit> {
    if matcher.is_empty() || limit == 0 {
        return Vec::new();
    }
    let root_path = match &root.kind {
        WorkspaceTreeKind::Directory(path) => path.as_path(),
        _ => return Vec::new(),
    };
    let mut hits = Vec::new();
    fn visit(
        node: &WorkspaceTreeNode,
        root: &Path,
        matcher: &SearchMatcher,
        limit: usize,
        hits: &mut Vec<WorkspaceSearchHit>,
    ) {
        if hits.len() >= limit {
            return;
        }
        match &node.kind {
            WorkspaceTreeKind::Directory(_) => {
                for child in &node.children {
                    visit(child, root, matcher, limit, hits);
                    if hits.len() >= limit {
                        break;
                    }
                }
            }
            WorkspaceTreeKind::MarkdownFile(path) | WorkspaceTreeKind::CodeFile(path) => {
                let label = path
                    .strip_prefix(root)
                    .unwrap_or(path)
                    .to_string_lossy()
                    .into_owned();
                if matcher.matches_filename(&label) {
                    hits.push(WorkspaceSearchHit {
                        path: path.clone(),
                        label: label.clone(),
                        line: None,
                        match_range: None,
                        source_range: None,
                        preview: String::new(),
                    });
                }
                if hits.len() >= limit
                    || fs::metadata(path).is_ok_and(|metadata| metadata.len() > 20_000_000)
                {
                    return;
                }
                if let Ok(source) = fs::read_to_string(path) {
                    let mut file_hits = 0;
                    for (index, raw_line) in source.split_inclusive('\n').enumerate() {
                        let line = raw_line.strip_suffix('\n').unwrap_or(raw_line);
                        let matches = matcher.find_in_line(line);
                        if let Some(first) = matches.first() {
                            hits.push(WorkspaceSearchHit {
                                path: path.clone(),
                                label: label.clone(),
                                line: Some(index + 1),
                                match_range: Some(first.clone()),
                                source_range: None,
                                preview: line.trim().chars().take(140).collect(),
                            });
                            file_hits += 1;
                            if file_hits == 3 || hits.len() >= limit {
                                break;
                            }
                        }
                    }
                }
            }
            WorkspaceTreeKind::Heading { .. } => {}
        }
    }
    visit(root, root_path, matcher, limit, &mut hits);
    hits
}

fn search_document_source(
    source: &str,
    matcher: &SearchMatcher,
    path: &Path,
    label: &str,
    limit: usize,
) -> Vec<WorkspaceSearchHit> {
    if matcher.is_empty() || limit == 0 {
        return Vec::new();
    }
    let mut hits = Vec::new();
    let mut absolute = 0usize;
    for (line_index, raw_line) in source.split_inclusive('\n').enumerate() {
        let line = raw_line.strip_suffix('\n').unwrap_or(raw_line);
        for range in matcher.find_in_line(line) {
            hits.push(WorkspaceSearchHit {
                path: path.to_path_buf(),
                label: label.to_string(),
                line: Some(line_index + 1),
                match_range: Some(range.start..range.end),
                source_range: Some(absolute + range.start..absolute + range.end),
                preview: line.trim().chars().take(140).collect(),
            });
            if hits.len() == limit {
                return hits;
            }
        }
        absolute += raw_line.len();
    }
    hits
}

/// Next match at or after `from` (or before, when reversing) across the whole
/// document source, wrapping around once.
fn find_document_match_from(
    source: &str,
    matcher: &SearchMatcher,
    from: usize,
    reverse: bool,
) -> Option<Range<usize>> {
    if matcher.is_empty() {
        return None;
    }
    let mut from = from.min(source.len());
    while !source.is_char_boundary(from) {
        from -= 1;
    }

    let mut ranges = Vec::new();
    let mut absolute = 0usize;
    for raw_line in source.split_inclusive('\n') {
        let line = raw_line.strip_suffix('\n').unwrap_or(raw_line);
        for range in matcher.find_in_line(line) {
            ranges.push(absolute + range.start..absolute + range.end);
        }
        absolute += raw_line.len();
    }
    if ranges.is_empty() {
        return None;
    }
    if reverse {
        ranges
            .iter()
            .rev()
            .find(|range| range.start < from)
            .or_else(|| ranges.last())
            .cloned()
    } else {
        ranges
            .iter()
            .find(|range| range.start >= from)
            .or_else(|| ranges.first())
            .cloned()
    }
}

pub(super) fn is_code_file(path: &Path) -> bool {
    const CODE_EXTENSIONS: &[&str] = &[
        "c", "cc", "cpp", "cs", "css", "go", "h", "hpp", "html", "java", "js", "json", "jsx", "kt",
        "php", "py", "rb", "rs", "sh", "sql", "swift", "toml", "ts", "tsx", "xml", "yaml", "yml",
        "zsh", "txt", "csv", "log", "ini", "conf", "lock",
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

fn search_utf16_to_utf8(text: &str, offset: usize) -> usize {
    let mut utf16 = 0;
    for (byte, ch) in text.char_indices() {
        if utf16 >= offset || utf16 + ch.len_utf16() > offset {
            return byte;
        }
        utf16 += ch.len_utf16();
    }
    text.len()
}

fn search_utf8_to_utf16(text: &str, offset: usize) -> usize {
    let mut byte = offset.min(text.len());
    while !text.is_char_boundary(byte) {
        byte -= 1;
    }
    text[..byte].encode_utf16().count()
}

impl Editor {
    fn active_search_input(&self, window: &Window) -> SearchInputKind {
        if self
            .workspace
            .replace_focus
            .as_ref()
            .is_some_and(|focus| focus.is_focused(window))
        {
            SearchInputKind::Replace
        } else {
            SearchInputKind::Query
        }
    }

    fn input_text(&self, kind: SearchInputKind) -> &str {
        match kind {
            SearchInputKind::Query => &self.workspace.search_query,
            SearchInputKind::Replace => &self.workspace.replace_query,
        }
    }

    fn input_selection(&self, kind: SearchInputKind) -> Range<usize> {
        match kind {
            SearchInputKind::Query => self.workspace.search_selected_range.clone(),
            SearchInputKind::Replace => self.workspace.replace_selected_range.clone(),
        }
    }

    fn input_marked(&self, kind: SearchInputKind) -> Option<Range<usize>> {
        match kind {
            SearchInputKind::Query => self.workspace.search_marked_range.clone(),
            SearchInputKind::Replace => self.workspace.replace_marked_range.clone(),
        }
    }

    fn replace_workspace_search_text(
        &mut self,
        range: Range<usize>,
        new_text: &str,
        selected_in_inserted: Option<Range<usize>>,
        marked: bool,
        cx: &mut Context<Self>,
    ) {
        self.replace_search_input_text(
            SearchInputKind::Query,
            range,
            new_text,
            selected_in_inserted,
            marked,
            cx,
        );
    }

    /// Applies an edit to the focused search-panel input (query or replace)
    /// and re-schedules the workspace search for query changes.
    fn replace_search_input_text(
        &mut self,
        kind: SearchInputKind,
        range: Range<usize>,
        new_text: &str,
        selected_in_inserted: Option<Range<usize>>,
        marked: bool,
        cx: &mut Context<Self>,
    ) {
        let (old, was_marked) = match kind {
            SearchInputKind::Query => (
                self.workspace.search_query.clone(),
                self.workspace.search_marked_range.is_some(),
            ),
            SearchInputKind::Replace => (
                self.workspace.replace_query.clone(),
                self.workspace.replace_marked_range.is_some(),
            ),
        };
        let start = range.start.min(old.len());
        let end = range.end.min(old.len()).max(start);
        if !old.is_char_boundary(start) || !old.is_char_boundary(end) {
            return;
        }
        let inserted = new_text.replace(['\r', '\n'], " ");
        let updated = {
            let mut updated = old.clone();
            updated.replace_range(start..end, &inserted);
            updated
        };
        let inserted_end = start + inserted.len();
        let selection = selected_in_inserted
            .map(|selection| {
                start + selection.start.min(inserted.len())
                    ..start + selection.end.min(inserted.len())
            })
            .unwrap_or(inserted_end..inserted_end);
        let marked_range = (marked && !inserted.is_empty()).then_some(start..inserted_end);
        match kind {
            SearchInputKind::Query => {
                self.workspace.search_query = updated;
                self.workspace.search_selected_range = selection;
                self.workspace.search_marked_range = marked_range;
                if !marked && (self.workspace.search_query != old || was_marked) {
                    self.schedule_workspace_search(cx);
                }
            }
            SearchInputKind::Replace => {
                self.workspace.replace_query = updated;
                self.workspace.replace_selected_range = selection;
                self.workspace.replace_marked_range = marked_range;
            }
        }
        cx.notify();
    }
}

// Only the focused workspace search fields register this handler; document
// blocks keep their own input handlers and IME state. The query and replace
// fields share the editor's handler, routed by which focus handle is active.
impl EntityInputHandler for Editor {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let kind = self.active_search_input(window);
        let text = self.input_text(kind);
        let start = search_utf16_to_utf8(text, range.start);
        let end = search_utf16_to_utf8(text, range.end).max(start);
        *actual_range = Some(search_utf8_to_utf16(text, start)..search_utf8_to_utf16(text, end));
        Some(text[start..end].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let kind = self.active_search_input(window);
        let text = self.input_text(kind).to_string();
        let range = self.input_selection(kind);
        Some(UTF16Selection {
            range: search_utf8_to_utf16(&text, range.start)..search_utf8_to_utf16(&text, range.end),
            reversed: false,
        })
    }

    fn marked_text_range(
        &self,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        let kind = self.active_search_input(window);
        let text = self.input_text(kind).to_string();
        self.input_marked(kind).map(|range| {
            search_utf8_to_utf16(&text, range.start)..search_utf8_to_utf16(&text, range.end)
        })
    }

    fn unmark_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let kind = self.active_search_input(window);
        let was_marked = self.input_marked(kind).is_some();
        match kind {
            SearchInputKind::Query => {
                self.workspace.search_marked_range = None;
            }
            SearchInputKind::Replace => {
                self.workspace.replace_marked_range = None;
            }
        }
        if was_marked {
            self.schedule_workspace_search(cx);
            cx.notify();
        }
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let kind = self.active_search_input(window);
        let query = self.input_text(kind).to_string();
        let range = range
            .map(|range| {
                search_utf16_to_utf8(&query, range.start)..search_utf16_to_utf8(&query, range.end)
            })
            .or_else(|| self.input_marked(kind))
            .unwrap_or_else(|| self.input_selection(kind));
        self.replace_search_input_text(kind, range, text, None, false, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        new_text: &str,
        new_selected_range: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let kind = self.active_search_input(window);
        let query = self.input_text(kind).to_string();
        let range = range
            .map(|range| {
                search_utf16_to_utf8(&query, range.start)..search_utf16_to_utf8(&query, range.end)
            })
            .or_else(|| self.input_marked(kind))
            .unwrap_or_else(|| self.input_selection(kind));
        let selected = new_selected_range.map(|range| {
            search_utf16_to_utf8(new_text, range.start)..search_utf16_to_utf8(new_text, range.end)
        });
        self.replace_search_input_text(kind, range, new_text, selected, true, cx);
    }

    fn bounds_for_range(
        &mut self,
        _range: Range<usize>,
        bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        Some(bounds)
    }

    fn character_index_for_point(
        &mut self,
        _point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        Some(self.workspace.search_query.encode_utf16().count())
    }
}

/// Flattens the markdown/code files of a workspace tree in display order.
fn collect_workspace_files(root: &WorkspaceTreeNode) -> Vec<PathBuf> {
    let mut files = Vec::new();
    fn visit(node: &WorkspaceTreeNode, files: &mut Vec<PathBuf>) {
        match &node.kind {
            WorkspaceTreeKind::Directory(_) => {
                for child in &node.children {
                    visit(child, files);
                }
            }
            WorkspaceTreeKind::MarkdownFile(path) | WorkspaceTreeKind::CodeFile(path) => {
                files.push(path.clone());
            }
            WorkspaceTreeKind::Heading { .. } => {}
        }
    }
    visit(root, &mut files);
    files
}

/// Number of matches in a whole source string.
fn count_matches_in_source(source: &str, matcher: &SearchMatcher) -> usize {
    let mut count = 0;
    for raw_line in source.split_inclusive('\n') {
        let line = raw_line.strip_suffix('\n').unwrap_or(raw_line);
        count += matcher.find_in_line(line).len();
    }
    count
}

/// Rewrites every match in `source` with `replacement`.
fn replace_in_source(source: &str, matcher: &SearchMatcher, replacement: &str) -> String {
    if matcher.is_empty() {
        return source.to_string();
    }
    let mut updated = String::with_capacity(source.len());
    for raw_line in source.split_inclusive('\n') {
        let line = raw_line.strip_suffix('\n').unwrap_or(raw_line);
        let trailing_newline = raw_line.len() - line.len();
        let mut line_cursor = 0usize;
        for range in matcher.find_in_line(line) {
            if range.start < line_cursor {
                continue;
            }
            updated.push_str(&line[line_cursor..range.start]);
            updated.push_str(replacement);
            line_cursor = range.end;
        }
        updated.push_str(&line[line_cursor..]);
        if trailing_newline > 0 {
            updated.push('\n');
        }
    }
    updated
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

fn clamp_workspace_panel_width(width: f32, viewport_width: f32) -> f32 {
    let maximum = (viewport_width - 320.0).clamp(180.0, 600.0);
    width.clamp(180.0, maximum)
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

/// Marks every heading node at or above `max_level` expanded so the outline
/// opens down to that level (level 2 = H1/H2 expanded, showing H3 leaves).
fn expand_outline_to_level(
    nodes: &[WorkspaceTreeNode],
    max_level: u8,
    expanded: &mut HashSet<String>,
) {
    for node in nodes {
        if let WorkspaceTreeKind::Heading { level, .. } = &node.kind {
            if *level <= max_level {
                expanded.insert(node.id.clone());
            }
        }
        expand_outline_to_level(&node.children, max_level, expanded);
    }
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
        Editor, SearchMatcher, SearchOptions, WorkspaceSelection, WorkspaceState,
        WorkspaceTreeKind, build_outline_tree, clamp_workspace_panel_width,
        create_workspace_file, create_workspace_folder, find_document_match_from, is_code_file,
        path_is_affected, prune_outline_state, remap_moved_path, rewrite_relative_image_targets,
        scan_workspace_dir, search_document_source, search_utf8_to_utf16, search_utf16_to_utf8,
        search_workspace_files,
    };
    use crate::components::{Block, UndoCaptureKind};
    use gpui::{AppContext, ClipboardItem, EntityInputHandler, TestAppContext, point, px};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    #[test]
    fn search_offsets_keep_cjk_and_emoji_boundaries() {
        let text = "中😀a";
        assert_eq!(search_utf16_to_utf8(text, 1), "中".len());
        assert_eq!(search_utf16_to_utf8(text, 2), "中".len());
        assert_eq!(search_utf16_to_utf8(text, 3), "中😀".len());
        assert_eq!(search_utf8_to_utf16(text, "中😀".len()), 3);
    }

        fn plain_matcher(query: &str) -> SearchMatcher {
        SearchMatcher::new(query, SearchOptions::default())
    }

#[gpui::test]
    async fn workspace_search_accepts_unicode_platform_input(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
        });
        let (editor, cx) =
            cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.workspace.active_tab = super::WorkspaceTab::Search;
                let focus = editor
                    .workspace
                    .search_focus
                    .get_or_insert_with(|| cx.focus_handle())
                    .clone();
                window.focus(&focus);
                cx.notify();
            });
            window.draw(cx).clear();
        });
        cx.simulate_input("你好");
        editor.read_with(cx, |editor, _cx| {
            assert_eq!(editor.workspace.search_query, "你好");
            assert_eq!(
                editor.workspace.search_selected_range,
                "你好".len().."你好".len()
            );
        });
        cx.simulate_keystrokes("backspace");
        editor.read_with(cx, |editor, _cx| {
            assert_eq!(editor.workspace.search_query, "你");
        });
        cx.update(|_window, cx| cx.write_to_clipboard(ClipboardItem::new_string("世界".into())));
        cx.simulate_keystrokes("cmd-v");
        editor.read_with(cx, |editor, _cx| {
            assert_eq!(editor.workspace.search_query, "你世界");
        });
    }

    #[gpui::test]
    async fn workspace_search_waits_for_ime_commit_before_searching(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
        });
        let (editor, cx) =
            cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.workspace.active_tab = super::WorkspaceTab::Search;
                let generation = editor.workspace.search_generation;
                editor.replace_and_mark_text_in_range(None, "ni", Some(2..2), window, cx);
                assert_eq!(editor.workspace.search_query, "ni");
                assert_eq!(editor.workspace.search_generation, generation);
                editor.replace_and_mark_text_in_range(None, "你", Some(1..1), window, cx);
                assert_eq!(editor.workspace.search_query, "你");
                assert_eq!(editor.workspace.search_generation, generation);
                editor.replace_text_in_range(None, "你", window, cx);
                assert!(editor.workspace.search_marked_range.is_none());
                assert_eq!(editor.workspace.search_generation, generation + 1);
            });
        });
    }

    #[test]
    fn workspace_scan_includes_markdown_and_code_files() {
        let root =
            std::env::temp_dir().join(format!("velotype-workspace-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("nested")).expect("create dirs");
        fs::write(root.join("a.md"), "a").expect("write md");
        fs::write(root.join("a.txt"), "plain text").expect("write txt");
        fs::write(root.join("main.rs"), "fn main() {}").expect("write code");
        fs::write(root.join("nested").join("b.md"), "b").expect("write nested md");

        let tree = scan_workspace_dir(&root).expect("scan tree");
        let labels = tree
            .children
            .iter()
            .map(|node| node.label.as_str())
            .collect::<Vec<_>>();
        assert_eq!(labels, vec!["nested", "a.md", "a.txt", "main.rs"]);
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
        assert!(matches!(
            tree.children[3].kind,
            WorkspaceTreeKind::CodeFile(_)
        ));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn workspace_tree_includes_plain_viewer_code_extensions() {
        let root = std::env::temp_dir().join(format!(
            "velora-workspace-plain-code-test-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).expect("create root");
        for (name, source) in [
            ("query.sql", "select 1;"),
            ("App.swift", "struct App {}"),
            ("Main.kt", "fun main() {}"),
            ("layout.xml", "<root />"),
        ] {
            fs::write(root.join(name), source).expect("write code sample");
        }

        let tree = scan_workspace_dir(&root).expect("scan code workspace");
        assert_eq!(tree.children.len(), 4);
        assert!(
            tree.children
                .iter()
                .all(|node| { matches!(node.kind, WorkspaceTreeKind::CodeFile(_)) })
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn code_file_extensions_are_case_insensitive() {
        for extension in ["rs", "sql", "swift", "kt", "xml"] {
            assert!(is_code_file(Path::new(&format!("source.{extension}"))));
            assert!(is_code_file(Path::new(&format!(
                "source.{}",
                extension.to_ascii_uppercase()
            ))));
        }
        assert!(is_code_file(Path::new("notes.txt")));
    }

    #[gpui::test]
    async fn opening_code_files_keeps_them_in_the_editor(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
        });
        let root =
            std::env::temp_dir().join(format!("velora-code-viewer-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).expect("create test workspace");
        let path = root.join("main.rs");
        fs::write(&path, "fn main() { println!(\"hello\"); }").expect("write code file");
        let plain_path = root.join("query.sql");
        fs::write(&plain_path, "select 1;").expect("write plain code file");
        let crlf_path = root.join("windows.cs");
        fs::write(&crlf_path, "class A {\r\n}\r\n").expect("write CRLF code file");
        let cleanup_root = root.clone();
        cx.on_quit(move || {
            let _ = fs::remove_dir_all(cleanup_root);
        });

        let (editor, cx) =
            cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.open_workspace_file(path.clone(), window, cx)
            });
        });
        cx.update(|window, cx| window.draw(cx).clear());
        cx.run_until_parked();
        editor.read_with(cx, |editor, cx| {
            assert!(editor.code_tab_active());
            assert_eq!(
                editor.document.raw_source_text(cx),
                "fn main() { println!(\"hello\"); }"
            );
            let block = editor.document.first_root().unwrap().read(cx);
            assert!(block.kind().is_code_block());
            assert!(block.code_highlight_result().is_some());
            assert_eq!(editor.workspace.open_documents.len(), 1);
        });
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.open_workspace_file(plain_path.clone(), window, cx)
            });
        });
        cx.update(|window, cx| window.draw(cx).clear());
        cx.run_until_parked();
        editor.read_with(cx, |editor, cx| {
            assert_eq!(editor.workspace.open_documents.len(), 2);
            assert_eq!(editor.workspace.active_document.as_ref(), Some(&plain_path));
            assert_eq!(editor.document.raw_source_text(cx), "select 1;");
        });
        let block = editor.read_with(cx, |editor, _| {
            editor.document.first_root().unwrap().clone()
        });
        cx.update(|window, cx| {
            block.update(cx, |block, cx| {
                block.selected_range = 0..block.visible_len();
                <Block as EntityInputHandler>::replace_text_in_range(
                    block,
                    None,
                    "select 2;",
                    window,
                    cx,
                );
            });
        });
        cx.run_until_parked();
        editor.read_with(cx, |editor, cx| {
            assert_eq!(editor.document.raw_source_text(cx), "select 2;");
        });
        editor.update(cx, |editor, cx| editor.undo_document(cx));
        editor.read_with(cx, |editor, cx| {
            assert!(editor.code_tab_active());
            assert_eq!(editor.document.raw_source_text(cx), "select 1;");
            assert!(
                editor
                    .document
                    .first_root()
                    .unwrap()
                    .read(cx)
                    .kind()
                    .is_code_block()
            );
        });
        editor.update(cx, |editor, cx| editor.redo_document(cx));
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| editor.save_document(window, cx));
        });
        assert_eq!(fs::read_to_string(&plain_path).unwrap(), "select 2;");
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.open_workspace_file(path.clone(), window, cx)
            });
        });
        editor.read_with(cx, |editor, cx| {
            assert_eq!(editor.workspace.open_documents.len(), 2);
            assert_eq!(
                editor.document.raw_source_text(cx),
                "fn main() { println!(\"hello\"); }"
            );
        });
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.open_workspace_file(crlf_path.clone(), window, cx)
            });
        });
        editor.read_with(cx, |editor, cx| {
            assert_eq!(editor.document.raw_source_text(cx), "class A {\n}\n");
        });
        editor.update(cx, |editor, cx| editor.jump_to_workspace_search_line(2, cx));
        editor.read_with(cx, |editor, cx| {
            assert_eq!(
                editor
                    .document
                    .first_root()
                    .unwrap()
                    .read(cx)
                    .selected_range
                    .start,
                "class A {\n".len()
            );
        });
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| editor.save_document(window, cx));
        });
        assert_eq!(
            fs::read_to_string(&crlf_path).unwrap(),
            "class A {\r\n}\r\n"
        );
    }

    #[gpui::test]
    async fn right_click_menu_renders_for_a_workspace_file(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
        });
        let root = std::env::temp_dir().join(format!("velora-menu-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("note.md");
        fs::write(&path, "hello").unwrap();
        cx.on_quit({
            let root = root.clone();
            move || {
                let _ = fs::remove_dir_all(root);
            }
        });
        let (editor, cx) =
            cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
        editor.update(cx, |editor, cx| {
            editor.set_workspace_root(root, cx);
            editor.open_workspace_context_menu(
                point(px(100.0), px(100.0)),
                Some(WorkspaceSelection::File(path)),
                cx,
            );
        });
        cx.update(|window, cx| window.draw(cx).clear());
        editor.read_with(cx, |editor, _| {
            assert!(editor.workspace.context_menu.unwrap().has_target);
        });
    }

    #[test]
    fn workspace_search_matches_file_names_and_contents() {
        let root =
            std::env::temp_dir().join(format!("velora-workspace-search-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("src")).expect("create source dir");
        fs::write(
            root.join("README.md"),
            "search term is only in file content",
        )
        .expect("write md");
        fs::write(root.join("src").join("main.rs"), "fn main() {}").expect("write code");
        let tree = scan_workspace_dir(&root).expect("scan tree");

        let matches = search_workspace_files(&tree, &SearchMatcher::new("MAIN", SearchOptions::default()), 200);
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].label, "src/main.rs");
        assert_eq!(matches[0].line, None);
        assert_eq!(matches[1].line, Some(1));

        let matches = search_workspace_files(&tree, &SearchMatcher::new("readme", SearchOptions::default()), 200);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].label, "README.md");

        let matches = search_workspace_files(&tree, &SearchMatcher::new("content", SearchOptions::default()), 200);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].label, "README.md");
        assert_eq!(matches[0].line, Some(1));
        assert!(matches[0].preview.contains("content"));
        assert!(search_workspace_files(&tree, &SearchMatcher::new("absent", SearchOptions::default()), 200).is_empty());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn document_search_finds_ascii_and_chinese_with_source_ranges() {
        let source = "# Hello\n\n你好 Hello\n";
        let hits = search_document_source(source, &SearchMatcher::new("hello", SearchOptions::default()), Path::new("note.md"), "note.md", 200);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].line, Some(1));
        assert_eq!(hits[1].line, Some(3));
        for hit in &hits {
            let range = hit.source_range.clone().unwrap();
            assert_eq!(&source[range], "Hello");
        }
        let chinese = search_document_source(source, &SearchMatcher::new("你好", SearchOptions::default()), Path::new("note.md"), "note.md", 1);
        assert_eq!(chinese.len(), 1);
        assert_eq!(&source[chinese[0].source_range.clone().unwrap()], "你好");
    }

    #[test]
    fn document_match_navigation_can_search_forward_and_backward() {
        let source = "Alpha 你好 alpha";
        assert_eq!(
            find_document_match_from(source, &plain_matcher("alpha"), 0, false),
            Some(0..5)
        );
        assert_eq!(
            find_document_match_from(source, &plain_matcher("alpha"), 5, false),
            Some(13..18)
        );
        assert_eq!(
            find_document_match_from(source, &plain_matcher("alpha"), source.len(), true),
            Some(13..18)
        );
        assert_eq!(
            find_document_match_from(source, &plain_matcher("你好"), 0, false),
            Some(6..12)
        );
    }

    #[gpui::test]
    async fn document_find_navigates_beyond_sidebar_result_limit(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
        });
        let source = "a ".repeat(250);
        let (editor, cx) = cx.add_window_view(move |_, cx| Editor::from_markdown(cx, source, None));
        editor.update(cx, |editor, cx| {
            editor.open_document_find(cx);
            editor.workspace.search_query = "a".into();
            editor.schedule_workspace_search(cx);
        });
        cx.executor().advance_clock(Duration::from_millis(150));
        cx.run_until_parked();
        editor.update(cx, |editor, cx| {
            assert_eq!(editor.workspace.search_results.len(), 200);
            for _ in 0..201 {
                editor.find_next_document_match(false, cx);
            }
            assert_eq!(editor.workspace.document_active_range, Some(400..401));
            assert_eq!(editor.workspace.search_active_index, None);
            editor.find_next_document_match(true, cx);
            assert_eq!(editor.workspace.document_active_range, Some(398..399));
            assert_eq!(editor.workspace.search_active_index, Some(199));
        });
    }

    #[gpui::test]
    async fn document_find_searches_unsaved_edits_and_selects_matches(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
        });
        let (editor, cx) =
            cx.add_window_view(|_, cx| Editor::from_markdown(cx, "# Alpha\n\nBeta".into(), None));
        editor.update(cx, |editor, cx| {
            let paragraph = editor.document.root_blocks()[1].clone();
            paragraph.update(cx, |paragraph, cx| {
                paragraph.prepare_undo_capture(UndoCaptureKind::CoalescibleText, cx);
                paragraph.replace_text_in_visible_range(4..4, " alpha", None, false, cx);
            });
        });
        cx.run_until_parked();
        editor.update(cx, |editor, cx| {
            editor.open_document_find(cx);
            editor.workspace.search_query = "alpha".into();
            editor.schedule_workspace_search(cx);
        });
        cx.executor().advance_clock(Duration::from_millis(150));
        cx.run_until_parked();
        editor.update(cx, |editor, cx| {
            assert_eq!(editor.workspace.search_results.len(), 2);
            editor.find_next_document_match(false, cx);
            assert_eq!(editor.workspace.search_active_index, Some(0));
            let heading = editor.document.root_blocks()[0].read(cx);
            assert_eq!(heading.selected_range, 0..5);
            editor.find_next_document_match(false, cx);
            assert_eq!(editor.workspace.search_active_index, Some(1));
            let paragraph = editor.document.root_blocks()[1].read(cx);
            assert_eq!(paragraph.selected_range, 5..10);
        });
        editor.update(cx, |editor, cx| {
            let paragraph = editor.document.root_blocks()[1].clone();
            paragraph.update(cx, |paragraph, cx| {
                paragraph.prepare_undo_capture(UndoCaptureKind::CoalescibleText, cx);
                paragraph.replace_text_in_visible_range(5..10, "", None, false, cx);
            });
        });
        cx.executor().advance_clock(Duration::from_millis(150));
        cx.run_until_parked();
        editor.read_with(cx, |editor, _cx| {
            assert_eq!(editor.workspace.search_results.len(), 1);
        });
    }

    #[gpui::test]
    async fn document_find_refreshes_after_switching_tabs(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
        });
        let root = std::env::temp_dir().join(format!(
            "velora-document-find-tabs-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).unwrap();
        let first = root.join("first.md");
        let second = root.join("second.md");
        fs::write(&first, "needle in first").unwrap();
        fs::write(&second, "needle in second").unwrap();
        cx.on_quit({
            let root = root.clone();
            move || {
                let _ = fs::remove_dir_all(root);
            }
        });
        let (editor, cx) =
            cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.set_workspace_root(root, cx);
                editor.open_workspace_file(first.clone(), window, cx);
                editor.open_document_find(cx);
                editor.workspace.search_query = "needle".into();
                editor.schedule_workspace_search(cx);
            });
        });
        cx.executor().advance_clock(Duration::from_millis(150));
        cx.run_until_parked();
        editor.read_with(cx, |editor, _cx| {
            assert_eq!(editor.workspace.search_results.len(), 1);
            assert_eq!(editor.workspace.search_results[0].path, first);
        });
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.open_workspace_file(second.clone(), window, cx);
            });
        });
        cx.executor().advance_clock(Duration::from_millis(150));
        cx.run_until_parked();
        editor.read_with(cx, |editor, _cx| {
            assert_eq!(editor.workspace.search_results.len(), 1);
            assert_eq!(editor.workspace.search_results[0].path, second);
        });
    }

    #[gpui::test]
    async fn cmd_f_opens_current_document_find_in_sidebar(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
            crate::app_menu::init(cx);
        });
        let (editor, cx) = cx.add_window_view(|_window, cx| {
            Editor::from_markdown(cx, "hello\n\nhello".into(), None)
        });
        cx.update(|window, cx| {
            window.activate_window();
            window.draw(cx).clear();
        });
        cx.simulate_keystrokes("cmd-f");
        cx.update(|window, cx| window.draw(cx).clear());
        editor.read_with(cx, |editor, _cx| {
            assert!(editor.workspace.is_open);
            assert!(editor.workspace.active_tab == super::WorkspaceTab::Search);
            assert_eq!(
                editor.workspace.search_scope,
                super::WorkspaceSearchScope::Document
            );
        });
        cx.simulate_input("hello");
        cx.executor().advance_clock(Duration::from_millis(150));
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear());
        editor.read_with(cx, |editor, _cx| {
            assert_eq!(editor.workspace.search_results.len(), 2);
        });
        cx.simulate_keystrokes("enter");
        cx.update(|window, cx| window.draw(cx).clear());
        editor.read_with(cx, |editor, _cx| {
            assert_eq!(editor.workspace.search_active_index, Some(0));
        });
        cx.simulate_keystrokes("cmd-g");
        editor.read_with(cx, |editor, _cx| {
            assert_eq!(editor.workspace.search_active_index, Some(1));
        });
        cx.simulate_keystrokes("cmd-shift-g");
        editor.read_with(cx, |editor, _cx| {
            assert_eq!(editor.workspace.search_active_index, Some(0));
        });
    }

    #[gpui::test]
    async fn workspace_search_returns_content_hits_after_typing(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
        });
        let root =
            std::env::temp_dir().join(format!("velora-content-search-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("notes.md"), "first line\n独特的内容在这里\n").unwrap();
        cx.on_quit({
            let root = root.clone();
            move || {
                let _ = fs::remove_dir_all(root);
            }
        });
        let (editor, cx) =
            cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
        editor.update(cx, |editor, cx| {
            editor.set_workspace_root(root, cx);
            editor.workspace.active_tab = super::WorkspaceTab::Search;
            editor.workspace.search_query = "独特".into();
            editor.schedule_workspace_search(cx);
        });
        cx.executor().advance_clock(Duration::from_millis(150));
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear());
        editor.read_with(cx, |editor, _| {
            assert_eq!(editor.workspace.search_results.len(), 1);
            assert_eq!(editor.workspace.search_results[0].line, Some(2));
            assert_eq!(editor.workspace.search_results[0].label, "notes.md");
        });
    }

    #[test]
    fn workspace_create_operations_do_not_overwrite_existing_files() {
        let root =
            std::env::temp_dir().join(format!("velora-workspace-create-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).expect("create root");
        let file = root.join("notes.md");
        fs::write(&file, "keep this").expect("write existing file");

        assert!(create_workspace_file(&file).is_err());
        assert_eq!(
            fs::read_to_string(&file).expect("read existing file"),
            "keep this"
        );

        let new_file = root.join("new.md");
        create_workspace_file(&new_file).expect("create markdown file");
        assert_eq!(fs::read_to_string(&new_file).expect("read new file"), "");

        let folder = root.join("nested");
        create_workspace_folder(&folder).expect("create folder");
        assert!(folder.is_dir());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn moving_a_folder_remaps_open_document_descendants() {
        let source = Path::new("/workspace/old");
        let destination = Path::new("/workspace/new");
        assert_eq!(
            remap_moved_path(
                Path::new("/workspace/old/docs/readme.md"),
                source,
                destination,
                true,
            ),
            Some(PathBuf::from("/workspace/new/docs/readme.md"))
        );
        assert_eq!(
            remap_moved_path(
                Path::new("/workspace/other/readme.md"),
                source,
                destination,
                true,
            ),
            None
        );
        assert_eq!(
            remap_moved_path(
                Path::new("/workspace/old.md"),
                Path::new("/workspace/old.md"),
                Path::new("/workspace/new.md"),
                false,
            ),
            Some(PathBuf::from("/workspace/new.md"))
        );
    }

    #[test]
    fn moving_a_markdown_file_rewrites_relative_inline_image_paths() {
        let markdown = "![diagram](./assets/diagram.png \"Diagram\")\n\n![online](https://example.com/image.png)";
        assert_eq!(
            rewrite_relative_image_targets(
                markdown,
                Path::new("/workspace/docs"),
                Path::new("/workspace/notes"),
            ),
            "![diagram](../docs/assets/diagram.png \"Diagram\")\n\n![online](https://example.com/image.png)"
        );
    }

    #[test]
    fn moving_a_markdown_file_rewrites_reference_image_definitions() {
        let markdown = "![cover][hero]\n\n[hero]: ./assets/cover.png \"Cover\"";
        assert_eq!(
            rewrite_relative_image_targets(
                markdown,
                Path::new("/workspace/docs"),
                Path::new("/workspace/notes"),
            ),
            "![cover][hero]\n\n[hero]: ../docs/assets/cover.png \"Cover\""
        );
    }

    #[test]
    fn deleting_a_folder_matches_only_its_descendants() {
        let folder = Path::new("/workspace/docs");
        assert!(path_is_affected(
            Path::new("/workspace/docs/readme.md"),
            folder,
            true,
        ));
        assert!(!path_is_affected(
            Path::new("/workspace/docs-old/readme.md"),
            folder,
            true,
        ));
        assert!(path_is_affected(
            Path::new("/workspace/readme.md"),
            Path::new("/workspace/readme.md"),
            false,
        ));
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

    #[gpui::test]
    async fn outline_tracks_committed_heading_edits(cx: &mut TestAppContext) {
        let editor = cx.new(|cx| Editor::from_markdown(cx, "# Old".into(), None));
        editor.update(cx, |editor, cx| {
            editor.sync_workspace_outline(cx);
            assert_eq!(editor.workspace.outline_tree[0].label, "Old");
            let heading = editor.document.first_root().unwrap().clone();
            heading.update(cx, |heading, cx| {
                heading.prepare_undo_capture(UndoCaptureKind::CoalescibleText, cx);
                heading.replace_text_in_visible_range(0..3, "New", None, false, cx);
            });
        });
        cx.run_until_parked();
        editor.update(cx, |editor, cx| {
            editor.sync_workspace_outline(cx);
            assert_eq!(editor.workspace.outline_tree[0].label, "New");
        });
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
    fn workspace_panel_width_stays_within_drag_bounds() {
        assert_eq!(clamp_workspace_panel_width(100.0, 1080.0), 180.0);
        assert_eq!(clamp_workspace_panel_width(320.0, 1080.0), 320.0);
        assert_eq!(clamp_workspace_panel_width(500.0, 720.0), 400.0);
    }
}
