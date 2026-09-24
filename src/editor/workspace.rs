//! Lightweight workspace panel state, file-tree scanning, and outline parsing.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use gpui::*;
use pulldown_cmark::{Event, LinkType, Options, Parser, Tag};

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
    Recent,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum WorkspaceTreeKind {
    Directory(PathBuf),
    MarkdownFile(PathBuf),
    CodeFile(PathBuf),
    RecentWorkspace(PathBuf),
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
    recovery_id: uuid::Uuid,
    file_version: u64,
    markdown: String,
    dirty: bool,
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
    recent_roots: Vec<PathBuf>,
    recent_roots_loaded: bool,
    filename_query: String,
    filename_search_focus: Option<FocusHandle>,
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
            recent_roots: Vec::new(),
            recent_roots_loaded: false,
            filename_query: String::new(),
            filename_search_focus: None,
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
        if let Ok(recent) = crate::config::record_recent_workspace(&root) {
            self.workspace.recent_roots = recent;
            self.workspace.recent_roots_loaded = true;
        }
        self.workspace.root = Some(root);
        self.workspace.file_tree = None;
        self.workspace.file_error = None;
        self.workspace.expanded.clear();
        self.sync_workspace_file_tree();
        if self.workspace.selected.is_none() {
            self.workspace.selected = self
                .workspace
                .root
                .clone()
                .map(WorkspaceSelection::Directory);
        }
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
                    if let Ok(recent) = crate::config::record_recent_workspace(&root) {
                        editor.workspace.recent_roots = recent;
                    }
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
                editor.workspace.recent_roots_loaded = true;
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
                        editor.workspace.recent_roots.retain(|root| {
                            !path_is_affected(root, &target_for_update, target_is_directory)
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
                                editor.replace_document_from_markdown(
                                    tab.markdown,
                                    Some(tab.path),
                                    cx,
                                );
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
            if let Some(root) = self.workspace.root.as_ref()
                && let Ok(recent) = crate::config::record_recent_workspace(root)
            {
                self.workspace.recent_roots = recent;
                self.workspace.recent_roots_loaded = true;
            }
        }
        if self.workspace.is_open {
            self.sync_workspace_models(cx);
        }
    }

    fn sync_workspace_models(&mut self, cx: &mut Context<Self>) {
        if !self.workspace.recent_roots_loaded {
            self.workspace.recent_roots =
                crate::config::read_recent_workspaces().unwrap_or_default();
            self.workspace.recent_roots_loaded = true;
        }
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
        self.replace_document_from_markdown(markdown, Some(path), cx);
        self.document_dirty = dirty;
        self.file_version = Some(file_version);
        if dirty || self.has_dirty_workspace_documents() {
            self.schedule_autosave(cx);
        }
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
                WorkspaceTab::Recent => "workspace-tab-recent",
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
            WorkspaceTab::Recent => self.render_recent_workspaces(theme, strings, &editor),
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
        let new_file_editor = editor.clone();
        let new_file_button = div()
            .id("workspace-new-file")
            .flex_1()
            .h(px(30.0))
            .px(px(7.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(6.0))
            .bg(c.dialog_secondary_button_bg)
            .hover(|this| this.bg(c.dialog_secondary_button_hover))
            .cursor_pointer()
            .text_size(px(t.text_size * 0.78))
            .text_color(c.text_default)
            .child(strings.workspace_new_file.clone())
            .on_click(move |_event, window, cx| {
                let _ = new_file_editor.update(cx, |editor, cx| {
                    editor.prompt_create_workspace_file(window, cx);
                });
            });
        let new_folder_editor = editor.clone();
        let new_folder_button = div()
            .id("workspace-new-folder")
            .flex_1()
            .h(px(30.0))
            .px(px(7.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(6.0))
            .bg(c.dialog_secondary_button_bg)
            .hover(|this| this.bg(c.dialog_secondary_button_hover))
            .cursor_pointer()
            .text_size(px(t.text_size * 0.78))
            .text_color(c.text_default)
            .child(strings.workspace_new_folder.clone())
            .on_click(move |_event, window, cx| {
                let _ = new_folder_editor.update(cx, |editor, cx| {
                    editor.prompt_create_workspace_folder(window, cx);
                });
            });
        let rename_editor = editor.clone();
        let rename_button = div()
            .id("workspace-rename")
            .flex_1()
            .h(px(30.0))
            .px(px(7.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(6.0))
            .bg(c.dialog_secondary_button_bg)
            .hover(|this| this.bg(c.dialog_secondary_button_hover))
            .cursor_pointer()
            .text_size(px(t.text_size * 0.78))
            .text_color(c.text_default)
            .child(strings.workspace_rename.clone())
            .on_click(move |_event, window, cx| {
                let _ = rename_editor.update(cx, |editor, cx| {
                    editor.prompt_rename_or_move_selected(window, cx);
                });
            });
        let delete_editor = editor.clone();
        let delete_button = div()
            .id("workspace-delete")
            .flex_1()
            .h(px(30.0))
            .px(px(7.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(6.0))
            .bg(c.dialog_secondary_button_bg)
            .hover(|this| this.bg(c.dialog_secondary_button_hover))
            .cursor_pointer()
            .text_size(px(t.text_size * 0.78))
            .text_color(c.text_default)
            .child(strings.workspace_delete.clone())
            .on_click(move |_event, window, cx| {
                let _ = delete_editor.update(cx, |editor, cx| {
                    editor.prompt_delete_selected(window, cx);
                });
            });
        let search_focus = self
            .workspace
            .filename_search_focus
            .get_or_insert_with(|| cx.focus_handle())
            .clone();
        let search_focus_for_click = search_focus.clone();
        let search_editor = editor.clone();
        let search_query = self.workspace.filename_query.clone();
        let search_label = if search_query.is_empty() {
            strings.workspace_search_placeholder.clone()
        } else {
            search_query.clone()
        };
        let search_field = div()
            .id("workspace-file-search")
            .track_focus(&search_focus)
            .w_full()
            .h(px(32.0))
            .px(px(10.0))
            .flex()
            .items_center()
            .rounded(px(7.0))
            .border_1()
            .border_color(c.dialog_border)
            .bg(c.editor_background)
            .text_size(px(t.text_size * 0.88))
            .text_color(if search_query.is_empty() {
                c.dialog_muted
            } else {
                c.text_default
            })
            .child(search_label)
            .on_click(move |_event, window, _cx| window.focus(&search_focus_for_click))
            .on_key_down(move |event: &KeyDownEvent, _window, cx| {
                let key = event.keystroke.key.to_ascii_lowercase();
                let key_char = event.keystroke.key_char.clone();
                let modified =
                    event.keystroke.modifiers.secondary() || event.keystroke.modifiers.alt;
                let _ = search_editor.update(cx, |editor, cx| {
                    match key.as_str() {
                        "backspace" => {
                            editor.workspace.filename_query.pop();
                        }
                        "escape" => editor.workspace.filename_query.clear(),
                        _ if !modified => {
                            if let Some(character) = key_char {
                                if !character.chars().any(char::is_control) {
                                    editor.workspace.filename_query.push_str(&character);
                                }
                            }
                        }
                        _ => {}
                    }
                    cx.notify();
                });
                cx.stop_propagation();
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
                                ))
                                .child(tab(
                                    strings.workspace_tab_recent.clone(),
                                    WorkspaceTab::Recent,
                                    self.workspace.active_tab == WorkspaceTab::Recent,
                                )),
                        )
                        .child(open_folder_button)
                        .child(search_field)
                        .child(
                            div()
                                .w_full()
                                .flex()
                                .gap(px(6.0))
                                .child(new_file_button)
                                .child(new_folder_button),
                        )
                        .child(
                            div()
                                .w_full()
                                .flex()
                                .gap(px(6.0))
                                .child(rename_button)
                                .child(delete_button),
                        ),
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

        if !self.workspace.filename_query.trim().is_empty() {
            let mut matches = Vec::new();
            collect_matching_workspace_files(
                root,
                root,
                &self.workspace.filename_query.to_lowercase(),
                &mut matches,
            );
            if matches.is_empty() {
                return self.render_workspace_empty_state(
                    "",
                    &strings.workspace_no_search_results,
                    theme,
                );
            }
            return div()
                .w_full()
                .flex()
                .flex_col()
                .children(self.render_workspace_nodes(&matches, 0, theme, editor))
                .into_any_element();
        }

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

    fn render_recent_workspaces(
        &self,
        theme: &Theme,
        strings: &I18nStrings,
        editor: &WeakEntity<Editor>,
    ) -> AnyElement {
        if self.workspace.recent_roots.is_empty() {
            return self.render_workspace_empty_state("", &strings.workspace_empty_recent, theme);
        }
        let nodes = self
            .workspace
            .recent_roots
            .iter()
            .map(|path| WorkspaceTreeNode {
                id: file_node_id(path),
                label: path.to_string_lossy().into_owned(),
                kind: WorkspaceTreeKind::RecentWorkspace(path.clone()),
                children: Vec::new(),
            })
            .collect::<Vec<_>>();
        div()
            .w_full()
            .flex()
            .flex_col()
            .children(self.render_workspace_nodes(&nodes, 0, theme, editor))
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
            (Some(WorkspaceSelection::Directory(selected)), WorkspaceTreeKind::Directory(path)) => {
                selected == path
            }
            (Some(WorkspaceSelection::File(selected)), WorkspaceTreeKind::MarkdownFile(path)) => {
                selected == path
            }
            (Some(WorkspaceSelection::File(selected)), WorkspaceTreeKind::CodeFile(path)) => {
                selected == path
            }
            (
                Some(WorkspaceSelection::WorkspaceRoot(selected)),
                WorkspaceTreeKind::RecentWorkspace(path),
            ) => selected == path,
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
            WorkspaceTreeKind::Directory(_) | WorkspaceTreeKind::RecentWorkspace(_) => {
                Some((FOLDER_ICON, Hsla::from(rgba(0xf59e0bff))))
            }
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
                    WorkspaceTreeKind::Directory(path) => {
                        editor.workspace.selected = Some(WorkspaceSelection::Directory(path));
                        editor.toggle_workspace_node(&node_id, cx);
                    }
                    WorkspaceTreeKind::MarkdownFile(path) => {
                        editor.open_workspace_file(path, window, cx);
                    }
                    WorkspaceTreeKind::CodeFile(path) => {
                        editor.open_code_file(path, cx);
                    }
                    WorkspaceTreeKind::RecentWorkspace(path) => {
                        editor.workspace.selected =
                            Some(WorkspaceSelection::WorkspaceRoot(path.clone()));
                        editor.set_workspace_root(path, cx);
                        editor.set_workspace_tab(WorkspaceTab::Files, cx);
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

fn collect_matching_workspace_files(
    node: &WorkspaceTreeNode,
    root: &WorkspaceTreeNode,
    query: &str,
    matches: &mut Vec<WorkspaceTreeNode>,
) {
    let query = query.to_lowercase();
    match &node.kind {
        WorkspaceTreeKind::Directory(_) => {
            for child in &node.children {
                collect_matching_workspace_files(child, root, &query, matches);
            }
        }
        WorkspaceTreeKind::MarkdownFile(path) | WorkspaceTreeKind::CodeFile(path)
            if path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().to_lowercase().contains(&query)) =>
        {
            let root_path = match &root.kind {
                WorkspaceTreeKind::Directory(path) => path.as_path(),
                _ => Path::new(""),
            };
            let label = path
                .strip_prefix(root_path)
                .unwrap_or(path)
                .to_string_lossy()
                .into_owned();
            matches.push(WorkspaceTreeNode {
                id: node.id.clone(),
                label,
                kind: node.kind.clone(),
                children: Vec::new(),
            });
        }
        _ => {}
    }
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
        collect_matching_workspace_files, create_workspace_file, create_workspace_folder,
        path_is_affected, prune_outline_state, remap_moved_path, rewrite_relative_image_targets,
        scan_workspace_dir, workspace_panel_width_for_viewport,
    };
    use std::fs;
    use std::path::{Path, PathBuf};

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
    fn workspace_filename_search_matches_names_case_insensitively() {
        let root =
            std::env::temp_dir().join(format!("maksher-workspace-search-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("src")).expect("create source dir");
        fs::write(
            root.join("README.md"),
            "search term is only in file content",
        )
        .expect("write md");
        fs::write(root.join("src").join("main.rs"), "fn main() {}").expect("write code");
        let tree = scan_workspace_dir(&root).expect("scan tree");

        let mut matches = Vec::new();
        collect_matching_workspace_files(&tree, &tree, "MAIN", &mut matches);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].label, "src/main.rs");

        matches.clear();
        collect_matching_workspace_files(&tree, &tree, "readme", &mut matches);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].label, "README.md");

        matches.clear();
        collect_matching_workspace_files(&tree, &tree, "content", &mut matches);
        assert!(matches.is_empty());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn workspace_create_operations_do_not_overwrite_existing_files() {
        let root =
            std::env::temp_dir().join(format!("maksher-workspace-create-{}", uuid::Uuid::new_v4()));
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
