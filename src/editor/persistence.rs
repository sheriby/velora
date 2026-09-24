//! Document save operations.
//!
//! Rendered mode serializes the semantic block tree back to normalized
//! Markdown. Source mode writes the raw source buffer directly so literal
//! delimiters are preserved.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};

use anyhow::Context as _;
use gpui::*;

use super::Editor;
use crate::i18n::I18nManager;

fn longest_marker_run(text: &str, marker: char) -> usize {
    let mut longest = 0usize;
    let mut current = 0usize;

    for ch in text.chars() {
        if ch == marker {
            current += 1;
            longest = longest.max(current);
        } else {
            current = 0;
        }
    }

    longest
}

pub(super) fn safe_code_fence(content: &str) -> String {
    let longest_backticks = longest_marker_run(content, '`');
    if longest_backticks < 3 {
        return "```".to_string();
    }

    let longest_tildes = longest_marker_run(content, '~');
    "~".repeat(longest_tildes.max(2) + 1)
}

pub(super) fn safe_code_fence_with_info(content: &str, info: Option<&str>) -> String {
    if info.is_some_and(|info| info.contains('`')) {
        let longest_tildes = longest_marker_run(content, '~');
        return "~".repeat(longest_tildes.max(2) + 1);
    }

    safe_code_fence(content)
}

fn autosave_temp_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_default();
    path.with_file_name(format!(".{name}.maksher-{}.tmp", uuid::Uuid::new_v4()))
}

fn verify_file_version(path: &Path, expected_version: u64) -> anyhow::Result<()> {
    let markdown = std::fs::read_to_string(path)
        .with_context(|| format!("无法读取文件以检查外部修改：{}", path.display()))?;
    if file_content_version(&markdown) != expected_version {
        anyhow::bail!("检测到外部修改：{}", path.display());
    }
    Ok(())
}

pub(super) fn file_content_version(markdown: &str) -> u64 {
    let normalized = markdown.replace("\r\n", "\n").replace('\r', "\n");
    let mut hasher = DefaultHasher::new();
    normalized.hash(&mut hasher);
    hasher.finish()
}

#[derive(Clone)]
struct PendingAutosaveDocument {
    recovery: crate::config::RecoverySnapshot,
    path: Option<PathBuf>,
    temp_path: Option<PathBuf>,
    file_version: Option<u64>,
}

impl Editor {
    pub(super) fn schedule_autosave(&mut self, cx: &mut Context<Self>) {
        if self.pending_close_after_save
            || self.autosave_task.is_some()
            || self.has_external_autosave_conflict()
            || (!self.document_dirty && !self.has_dirty_workspace_documents())
        {
            return;
        }

        let editor = cx.entity().downgrade();
        let window_handle = self.window_handle;
        self.autosave_task = Some(cx.spawn(
            async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(800))
                    .await;

                let snapshot = editor
                    .update(cx, |editor, cx| {
                        let mut documents = editor
                            .dirty_workspace_documents(cx)
                            .into_iter()
                            .map(|document| PendingAutosaveDocument {
                                recovery: crate::config::RecoverySnapshot {
                                    id: document.recovery_id,
                                    source_path: Some(document.path.clone()),
                                    markdown: document.markdown,
                                },
                                temp_path: Some(autosave_temp_path(&document.path)),
                                file_version: Some(document.file_version),
                                path: Some(document.path),
                            })
                            .collect::<Vec<_>>();
                        if editor.document_dirty && editor.file_path.is_none() {
                            documents.push(PendingAutosaveDocument {
                                recovery: crate::config::RecoverySnapshot {
                                    id: editor.recovery_id,
                                    source_path: editor.recovery_source_path.clone(),
                                    markdown: editor.serialized_document_text(cx),
                                },
                                path: None,
                                temp_path: None,
                                file_version: None,
                            });
                        }
                        if documents.is_empty() {
                            return None;
                        }
                        Some((documents, editor.document_revision))
                    })
                    .ok()
                    .flatten();
                let Some((documents, revision)) = snapshot else {
                    let _ = editor.update(cx, |editor, _cx| editor.autosave_task = None);
                    return;
                };

                let documents_for_write = documents.clone();
                let write_result = cx
                    .background_executor()
                    .spawn(async move {
                        for document in documents_for_write {
                            crate::config::save_recovery_snapshot(&document.recovery)?;
                            if let (Some(path), Some(expected_version)) =
                                (document.path.as_deref(), document.file_version)
                            {
                                verify_file_version(path, expected_version)?;
                            }
                            if let Some(temp_path) = document.temp_path {
                                std::fs::write(temp_path, &document.recovery.markdown)?;
                                if let (Some(path), Some(expected_version)) =
                                    (document.path.as_deref(), document.file_version)
                                {
                                    verify_file_version(path, expected_version)?;
                                }
                            }
                        }
                        Ok::<_, anyhow::Error>(())
                    })
                    .await;
                let conflict_detail = editor
                    .update(cx, move |editor, cx| {
                        editor.autosave_task = None;
                        match write_result {
                            Ok(()) => {}
                            Err(error) => {
                                for document in &documents {
                                    if let Some(temp_path) = document.temp_path.as_ref() {
                                        let _ = std::fs::remove_file(temp_path);
                                    }
                                }
                                let detail = error.to_string();
                                eprintln!("failed to save recovery snapshot: {detail}");
                                editor.report_workspace_file_error(detail.clone(), cx);
                                return Some(detail);
                            }
                        }

                        if editor.document_revision != revision {
                            for document in &documents {
                                if let Some(temp_path) = document.temp_path.as_ref() {
                                    let _ = std::fs::remove_file(temp_path);
                                }
                            }
                            editor.schedule_autosave(cx);
                            return None;
                        }

                        let mut saved_documents = Vec::new();
                        for document in &documents {
                            let (Some(path), Some(temp_path)) =
                                (document.path.as_ref(), document.temp_path.as_ref())
                            else {
                                continue;
                            };
                            if let Err(error) = std::fs::rename(temp_path, path) {
                                let _ = std::fs::remove_file(temp_path);
                                eprintln!("failed to autosave '{}': {error}", path.display());
                                continue;
                            }
                            if let Err(error) =
                                crate::config::remove_recovery_snapshot(document.recovery.id)
                            {
                                eprintln!("failed to remove autosave snapshot: {error}");
                            }
                            saved_documents.push(super::workspace::WorkspaceAutosaveDocument {
                                recovery_id: document.recovery.id,
                                file_version: file_content_version(&document.recovery.markdown),
                                path: path.clone(),
                                markdown: document.recovery.markdown.clone(),
                            });
                        }
                        if editor.mark_workspace_documents_saved(&saved_documents) {
                            editor.document_dirty = false;
                            editor.pending_window_edited = false;
                            editor.pending_window_unedited = true;
                            editor.pending_window_title_refresh = true;
                            editor.snapshot_current_document(cx);
                        }
                        if !saved_documents.is_empty() {
                            cx.notify();
                        }
                        None
                    })
                    .ok()
                    .flatten();
                if let (Some(window_handle), Some(detail)) = (window_handle, conflict_detail) {
                    if detail.starts_with("检测到外部修改") {
                        Self::show_external_change_error(window_handle, detail, cx);
                    } else {
                        Self::show_workspace_save_error(window_handle, detail, cx);
                    }
                }
            },
        ));
    }

    pub(super) fn save_dirty_workspace_documents_and_close(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.snapshot_current_document(cx);
        let documents = self.dirty_workspace_documents(cx);
        if self.document_dirty && self.file_path.is_none() {
            self.pending_close_after_save = true;
            self.pending_save = true;
            cx.notify();
            return;
        }
        if documents.is_empty() {
            window.remove_window();
            return;
        }

        self.document_revision = self.document_revision.wrapping_add(1);
        let revision = self.document_revision;
        self.autosave_task = None;
        self.pending_close_after_save = true;
        let editor = cx.entity().downgrade();
        let window_handle = window.window_handle();
        let editor_window_for_error = Some(window_handle);
        let background = cx.background_executor().clone();

        cx.spawn(async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let documents_for_write = documents;
            let write_result = background
                .spawn(async move {
                    let mut prepared = Vec::new();
                    for document in documents_for_write {
                        let temp_path = autosave_temp_path(&document.path);
                        crate::config::save_recovery_snapshot(&crate::config::RecoverySnapshot {
                            id: document.recovery_id,
                            source_path: Some(document.path.clone()),
                            markdown: document.markdown.clone(),
                        })?;
                        verify_file_version(&document.path, document.file_version)?;
                        std::fs::write(&temp_path, &document.markdown).with_context(|| {
                            format!("failed to stage '{}'", document.path.display())
                        })?;
                        verify_file_version(&document.path, document.file_version)?;
                        prepared.push((document, temp_path));
                    }
                    Ok::<_, anyhow::Error>(prepared)
                })
                .await;

            let (close_window, error_detail) = editor
                .update(cx, move |editor, cx| {
                    editor.autosave_task = None;
                    let prepared = match write_result {
                        Ok(prepared) => prepared,
                        Err(error) => {
                            let detail = error.to_string();
                            editor.pending_close_after_save = false;
                            editor.show_unsaved_changes_dialog = true;
                            editor.report_workspace_file_error(detail.clone(), cx);
                            eprintln!("failed to save workspace documents: {detail}");
                            cx.notify();
                            return (false, Some(detail));
                        }
                    };
                    if editor.document_revision != revision {
                        for (_, temp_path) in &prepared {
                            let _ = std::fs::remove_file(temp_path);
                        }
                        editor.pending_close_after_save = false;
                        editor.show_unsaved_changes_dialog = true;
                        cx.notify();
                        return (false, None);
                    }

                    let mut saved_documents = Vec::new();
                    let mut failed_save = false;
                    for (document, temp_path) in prepared {
                        if let Err(error) = std::fs::rename(&temp_path, &document.path) {
                            let _ = std::fs::remove_file(&temp_path);
                            eprintln!("failed to save '{}': {error}", document.path.display());
                            failed_save = true;
                            continue;
                        }
                        if let Err(error) =
                            crate::config::remove_recovery_snapshot(document.recovery_id)
                        {
                            eprintln!("failed to remove saved recovery snapshot: {error}");
                        }
                        saved_documents.push(document);
                    }
                    let active_document_saved =
                        editor.mark_workspace_documents_saved(&saved_documents);
                    if active_document_saved {
                        editor.document_dirty = false;
                        editor.pending_window_edited = false;
                        editor.pending_window_unedited = true;
                        editor.pending_window_title_refresh = true;
                        editor.snapshot_current_document(cx);
                    }
                    let has_unsaved_documents =
                        editor.document_dirty || editor.has_dirty_workspace_documents();
                    if failed_save || has_unsaved_documents {
                        editor.pending_close_after_save = false;
                        editor.show_unsaved_changes_dialog = true;
                        cx.notify();
                        return (false, None);
                    }
                    editor.pending_close_after_save = false;
                    editor.show_unsaved_changes_dialog = false;
                    (true, None)
                })
                .unwrap_or((false, None));
            if let Some(detail) = error_detail {
                if let Some(error_window) = editor_window_for_error {
                    if detail.starts_with("检测到外部修改") {
                        Self::show_external_change_error(error_window, detail, cx);
                    } else {
                        Self::show_workspace_save_error(error_window, detail, cx);
                    }
                }
            }
            if close_window {
                let _ = cx.update_window(
                    window_handle,
                    |_view: AnyView, window: &mut Window, _cx: &mut App| {
                        window.remove_window();
                    },
                );
            }
        })
        .detach();
    }

    pub(super) fn serialized_document_text(&self, cx: &App) -> String {
        if self.view_mode == super::ViewMode::Source {
            self.document.raw_source_text(cx)
        } else {
            self.document.markdown_text(cx)
        }
    }

    pub(super) fn save_dialog_defaults(&self) -> (PathBuf, Option<String>) {
        if let Some(path) = self.recovery_source_path.as_ref() {
            let directory = path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
            let suggested_name = path
                .file_name()
                .map(|name| name.to_string_lossy().to_string());
            return (directory, suggested_name);
        }
        if let Some(path) = self.file_path.as_ref() {
            let directory = path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
            let suggested_name = path
                .file_name()
                .map(|name| name.to_string_lossy().to_string());
            (directory, suggested_name)
        } else {
            (
                std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
                Some("untitled.md".to_string()),
            )
        }
    }

    pub(super) fn apply_successful_save(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.document_revision = self.document_revision.wrapping_add(1);
        self.file_version = Some(file_content_version(&self.serialized_document_text(cx)));
        self.file_path = Some(path);
        self.recovery_source_path = None;
        self.is_recovered_document = false;
        self.document_dirty = false;
        self.pending_window_edited = false;
        self.pending_window_unedited = true;
        self.pending_window_title_refresh = true;
        self.pending_close_after_save = false;
        self.close_dialog_restore_focus = None;
        self.autosave_task = None;
        if let Err(error) = crate::config::remove_recovery_snapshot(self.recovery_id) {
            eprintln!("failed to remove saved document recovery snapshot: {error}");
        }
        self.sync_workspace_after_document_path_change(cx);
        cx.notify();
    }

    pub(super) fn save_to_existing_path(
        &mut self,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if let Some(expected_version) = self.file_version
            && let Err(error) = verify_file_version(path, expected_version)
        {
            let detail = error.to_string();
            let strings = cx.global::<I18nManager>().strings().clone();
            let buttons = [strings.info_dialog_ok.as_str()];
            if detail.starts_with("检测到外部修改") {
                self.report_workspace_file_error(detail.clone(), cx);
                let message = format!("{}\n\n{}", strings.external_change_message, path.display());
                let _ = window.prompt(
                    PromptLevel::Warning,
                    &strings.external_change_title,
                    Some(&message),
                    &buttons,
                    cx,
                );
            } else {
                let _ = window.prompt(
                    PromptLevel::Critical,
                    &strings.save_failed_title,
                    Some(&detail),
                    &buttons,
                    cx,
                );
            }
            return false;
        }
        let markdown = self.serialized_document_text(cx);
        match std::fs::write(path, markdown) {
            Ok(_) => {
                self.apply_successful_save(path.to_path_buf(), cx);
                window.set_window_edited(false);
                true
            }
            Err(err) => {
                let detail = err.to_string();
                let strings = cx.global::<I18nManager>().strings().clone();
                let buttons = [strings.info_dialog_ok.as_str()];
                let _ = window.prompt(
                    PromptLevel::Critical,
                    &strings.save_failed_title,
                    Some(&detail),
                    &buttons,
                    cx,
                );
                false
            }
        }
    }

    fn save_document_via_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let markdown = self.serialized_document_text(cx);
        let (default_dir, suggested_name) = self.save_dialog_defaults();
        let prompt = cx.prompt_for_new_path(&default_dir, suggested_name.as_deref());
        let weak_editor = cx.entity().downgrade();
        let weak_editor_for_cancel = weak_editor.clone();
        let weak_editor_for_error = weak_editor.clone();
        let weak_editor_for_write_error = weak_editor.clone();
        let weak_editor_for_close = weak_editor.clone();
        let window_handle = window.window_handle();
        let should_close_after_save = self.pending_close_after_save;

        cx.spawn(async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let mut path = match prompt.await {
                Ok(Ok(Some(path))) => path,
                Ok(Ok(None)) | Err(_) => {
                    if should_close_after_save {
                        let _ = weak_editor_for_cancel
                            .update(cx, |this, cx| this.abort_pending_close_after_save(cx));
                    }
                    return;
                }
                Ok(Err(err)) => {
                    if should_close_after_save {
                        let _ = weak_editor_for_error
                            .update(cx, |this, cx| this.abort_pending_close_after_save(cx));
                    }
                    let detail = err.to_string();
                    let _ = cx.update_window(
                        window_handle,
                        move |_view: AnyView, window: &mut Window, cx: &mut App| {
                            let strings = cx.global::<I18nManager>().strings().clone();
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
                    return;
                }
            };

            if path.extension().is_none() {
                path.set_extension("md");
            }

            if let Err(err) = std::fs::write(&path, &markdown) {
                if should_close_after_save {
                    let _ = weak_editor_for_write_error
                        .update(cx, |this, cx| this.abort_pending_close_after_save(cx));
                }
                let detail = err.to_string();
                let _ = cx.update_window(
                    window_handle,
                    move |_view: AnyView, window: &mut Window, cx: &mut App| {
                        let strings = cx.global::<I18nManager>().strings().clone();
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
                return;
            }

            let path_for_state = path.clone();
            let _ = weak_editor.update(cx, move |this, cx| {
                this.apply_successful_save(path_for_state, cx);
            });
            let _ = cx.update_window(
                window_handle,
                move |_view: AnyView, window: &mut Window, cx: &mut App| {
                    window.set_window_edited(false);
                    if should_close_after_save {
                        let has_dirty_tabs = weak_editor_for_close
                            .update(cx, |this, _cx| this.has_dirty_workspace_documents())
                            .unwrap_or(false);
                        if has_dirty_tabs {
                            let _ = weak_editor_for_close.update(cx, |this, cx| {
                                this.save_dirty_workspace_documents_and_close(window, cx);
                            });
                        } else {
                            window.remove_window();
                        }
                    }
                },
            );
        })
        .detach();
    }

    pub(crate) fn save_document(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(path) = self.file_path.clone() {
            let should_close_after_save = self.pending_close_after_save;
            if self.save_to_existing_path(&path, window, cx) {
                if should_close_after_save {
                    window.remove_window();
                }
            } else if should_close_after_save {
                self.abort_pending_close_after_save(cx);
            }
            return;
        }

        self.save_document_via_prompt(window, cx);
    }

    pub(crate) fn save_document_as(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.save_document_via_prompt(window, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::{safe_code_fence, safe_code_fence_with_info};

    #[test]
    fn safe_code_fence_is_longer_than_any_inner_backtick_run() {
        assert_eq!(safe_code_fence("plain code"), "```");
        assert_eq!(safe_code_fence("```\ncode"), "~~~");
        assert_eq!(safe_code_fence("value = `````"), "~~~");
        assert_eq!(safe_code_fence("```\n~~~"), "~~~~");
    }

    #[test]
    fn safe_code_fence_with_info_uses_tildes_when_info_contains_backticks() {
        assert_eq!(
            safe_code_fence_with_info("plain code", Some("we`rd")),
            "~~~"
        );
        assert_eq!(
            safe_code_fence_with_info("plain\n~~~\ncode", Some("we`rd")),
            "~~~~"
        );
        assert_eq!(safe_code_fence_with_info("plain code", Some("rust")), "```");
    }
}
