//! Document save operations.
//!
//! Rendered mode serializes the semantic block tree back to normalized
//! Markdown. Source mode writes the raw source buffer directly so literal
//! delimiters are preserved.

use std::path::{Path, PathBuf};

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

impl Editor {
    pub(super) fn schedule_autosave(&mut self, cx: &mut Context<Self>) {
        if self.autosave_task.is_some() || !self.document_dirty || self.file_path.is_none() {
            return;
        }

        let editor = cx.entity().downgrade();
        self.autosave_task = Some(cx.spawn(
            async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(800))
                    .await;

                let snapshot = editor
                    .update(cx, |editor, cx| {
                        let path = editor.file_path.clone()?;
                        let markdown = editor.serialized_document_text(cx);
                        let revision = editor.document_revision;
                        let temp_path = autosave_temp_path(&path);
                        Some((path, temp_path, markdown, revision))
                    })
                    .ok()
                    .flatten();
                let Some((path, temp_path, markdown, revision)) = snapshot else {
                    let _ = editor.update(cx, |editor, _cx| editor.autosave_task = None);
                    return;
                };

                let write_path = temp_path.clone();
                let write_result = cx
                    .background_executor()
                    .spawn(async move { std::fs::write(write_path, markdown) })
                    .await;
                let path_for_update = path;
                let _ = editor.update(cx, move |editor, cx| {
                    editor.autosave_task = None;
                    if let Err(error) = write_result {
                        let _ = std::fs::remove_file(&temp_path);
                        eprintln!(
                            "failed to autosave '{}': {error}",
                            path_for_update.display()
                        );
                        return;
                    }
                    if editor.file_path.as_ref() == Some(&path_for_update)
                        && editor.document_revision == revision
                        && editor.document_dirty
                    {
                        if let Err(error) = std::fs::rename(&temp_path, &path_for_update) {
                            let _ = std::fs::remove_file(&temp_path);
                            eprintln!(
                                "failed to autosave '{}': {error}",
                                path_for_update.display()
                            );
                            return;
                        }
                        editor.document_dirty = false;
                        editor.pending_window_edited = false;
                        editor.pending_window_unedited = true;
                        editor.pending_window_title_refresh = true;
                        editor.snapshot_current_document(cx);
                        cx.notify();
                    } else if editor.document_dirty {
                        let _ = std::fs::remove_file(&temp_path);
                        editor.schedule_autosave(cx);
                    } else {
                        let _ = std::fs::remove_file(&temp_path);
                    }
                });
            },
        ));
    }

    pub(super) fn serialized_document_text(&self, cx: &App) -> String {
        if self.view_mode == super::ViewMode::Source {
            self.document.raw_source_text(cx)
        } else {
            self.document.markdown_text(cx)
        }
    }

    pub(super) fn save_dialog_defaults(&self) -> (PathBuf, Option<String>) {
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
        self.file_path = Some(path);
        self.document_dirty = false;
        self.pending_window_edited = false;
        self.pending_window_unedited = true;
        self.pending_window_title_refresh = true;
        self.pending_close_after_save = false;
        self.close_dialog_restore_focus = None;
        self.autosave_task = None;
        self.sync_workspace_after_document_path_change(cx);
        cx.notify();
    }

    pub(super) fn save_to_existing_path(
        &mut self,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
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
                move |_view: AnyView, window: &mut Window, _cx: &mut App| {
                    window.set_window_edited(false);
                    if should_close_after_save {
                        window.remove_window();
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
