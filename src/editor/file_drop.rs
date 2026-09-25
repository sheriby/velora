//! External Markdown file drops for replacing the current editor window.

use std::path::{Path, PathBuf};

use anyhow::{Context as AnyhowContext, Result};
use gpui::*;

use super::{Editor, ViewMode};
use crate::components::{BlockKind, BlockRecord};
use crate::i18n::I18nManager;

impl Editor {
    pub(super) fn is_markdown_file_path(path: &Path) -> bool {
        path.is_file()
            && path.extension().is_some_and(|extension| {
                extension.to_string_lossy().eq_ignore_ascii_case("md")
                    || extension.to_string_lossy().eq_ignore_ascii_case("markdown")
            })
    }

    pub(super) fn first_dropped_markdown_path(paths: &[PathBuf]) -> Option<PathBuf> {
        paths
            .iter()
            .find(|path| Self::is_markdown_file_path(path))
            .cloned()
    }

    pub(super) fn first_dropped_image_path(paths: &[PathBuf]) -> Option<PathBuf> {
        paths
            .iter()
            .find(|path| super::Block::is_supported_local_image_path(path))
            .cloned()
    }

    pub(crate) fn on_external_paths_drop(
        &mut self,
        paths: &ExternalPaths,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(path) = Self::first_dropped_markdown_path(paths.paths()) {
            self.request_dropped_markdown_replace(path, window, cx);
            return;
        }

        if let Some(path) = Self::first_dropped_image_path(paths.paths()) {
            let block = self
                .active_entity_id
                .and_then(|id| self.focusable_entity_by_id(id))
                .or_else(|| self.document.first_root().cloned());
            if let Some(block) = block {
                let (leading, trailing) = block.update(cx, |block, _cx| block.paste_image_split());
                self.handle_paste_image_request(
                    block,
                    &leading,
                    &crate::components::PastedImageSource::LocalPath(path),
                    &trailing,
                    cx,
                );
                return;
            }
            self.show_image_paste_error(
                anyhow::anyhow!("no focused Markdown block is available for the dropped image"),
                cx,
            );
            return;
        }

        let strings = cx.global::<I18nManager>().strings().clone();
        self.show_drop_open_failed_prompt(strings.drop_no_markdown_file_message, window, cx);
    }

    pub(crate) fn request_dropped_markdown_replace(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_menu_bar(cx);
        self.hide_info_dialog(cx);
        self.dismiss_contextual_overlays(cx);

        if self.document_dirty || self.has_dirty_workspace_documents() {
            self.pending_drop_replace_path = Some(path);
            self.pending_drop_replace_after_save = false;
            if !self.show_drop_replace_dialog {
                self.drop_replace_restore_focus = self.document.focused_block_entity_id(window, cx);
                self.show_drop_replace_dialog = true;
                window.blur();
            }
            cx.notify();
            return;
        }

        match self.replace_document_from_path(&path, cx) {
            Ok(()) => window.set_window_edited(false),
            Err(err) => self.show_drop_open_failed_prompt(err.to_string(), window, cx),
        }
    }

    pub(super) fn replace_document_from_path(
        &mut self,
        path: &Path,
        cx: &mut Context<Self>,
    ) -> Result<()> {
        let markdown = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read '{}'", path.display()))?;
        self.document_revision = self.document_revision.wrapping_add(1);
        self.autosave_task = None;
        self.recovery_source_path = None;
        self.is_recovered_document = false;
        if let Err(error) = crate::config::remove_recovery_snapshot(self.recovery_id) {
            eprintln!("failed to remove replaced document recovery snapshot: {error}");
        }
        if super::workspace::is_code_file(path) {
            self.replace_document_from_code_source(markdown, path.to_path_buf(), cx);
        } else {
            self.replace_document_from_markdown(markdown, Some(path.to_path_buf()), cx);
        }
        crate::app_menu::record_recent_file_from_editor(path, cx);
        Ok(())
    }

    pub(super) fn replace_document_from_markdown(
        &mut self,
        markdown: String,
        file_path: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        self.replace_document_content(markdown, file_path, None, cx);
    }

    pub(super) fn replace_document_from_code_source(
        &mut self,
        source: String,
        file_path: PathBuf,
        cx: &mut Context<Self>,
    ) {
        let language = file_path
            .extension()
            .map(|extension| extension.to_string_lossy().into_owned().into());
        self.replace_document_content(source, Some(file_path), language, cx);
    }

    pub(super) fn replace_document_content(
        &mut self,
        markdown: String,
        file_path: Option<PathBuf>,
        code_language: Option<SharedString>,
        cx: &mut Context<Self>,
    ) {
        let normalized = markdown.replace("\r\n", "\n").replace('\r', "\n");
        let is_code = code_language.is_some();
        self.code_document = is_code;
        self.code_uses_crlf = is_code && markdown.contains("\r\n");
        let source_mode_fallback_required =
            !is_code && Self::markdown_requires_source_mode_fallback(&normalized);
        let mut roots = if is_code || source_mode_fallback_required {
            let record = if is_code {
                BlockRecord::with_plain_text(
                    BlockKind::CodeBlock {
                        language: code_language,
                    },
                    normalized.clone(),
                )
            } else {
                BlockRecord::paragraph(normalized.clone())
            };
            let block = Self::new_block(cx, record);
            block.update(cx, |block, _cx| block.set_source_document_mode());
            vec![block]
        } else {
            Self::build_root_blocks_from_markdown(cx, &normalized)
        };
        if roots.is_empty() {
            roots.push(Self::new_block(cx, BlockRecord::paragraph(String::new())));
        }

        self.file_version = file_path
            .as_ref()
            .map(|_| super::persistence::file_content_version(&normalized));
        self.file_path = file_path;
        self.view_mode = if is_code || source_mode_fallback_required {
            ViewMode::Source
        } else {
            ViewMode::Rendered
        };
        self.source_mode_fallback_required = source_mode_fallback_required;
        self.document.replace_roots(roots, cx);
        self.table_cells.clear();
        self.rebuild_table_runtimes(cx);
        self.rebuild_image_runtimes(cx);

        self.document_dirty = false;
        self.pending_window_edited = false;
        self.pending_window_title_refresh = true;
        self.pending_save = false;
        self.pending_save_as = false;
        self.pending_open_link = None;
        self.pending_close_after_save = false;
        self.close_dialog_restore_focus = None;
        self.show_unsaved_changes_dialog = false;
        self.clear_pending_drop_replace_state(cx);
        self.dismiss_contextual_overlays(cx);
        self.close_menu_bar(cx);
        self.table_axis_preview = None;
        self.table_axis_selection = None;
        self.sync_table_axis_visuals(cx);
        self.clear_cross_block_selection(cx);

        self.pending_scroll_active_block_into_view = true;
        self.pending_scroll_recheck_after_layout = true;
        self.last_scroll_viewport_size = None;
        self.scroll_handle.set_offset(point(px(0.0), px(0.0)));
        self.pending_focus = self.first_focusable_entity_id(cx);
        self.active_entity_id = self.pending_focus;

        self.undo_history.clear();
        self.redo_history.clear();
        self.pending_undo_capture = None;
        self.last_selection_snapshot = Self::empty_selection_snapshot();
        self.last_stable_source_text = normalized;
        self.history_restore_in_progress = false;
        self.refresh_stable_document_snapshot(cx);
        self.sync_workspace_after_document_path_change(cx);
        cx.notify();
    }

    pub(crate) fn cancel_drop_replace_dialog(&mut self, cx: &mut Context<Self>) {
        let restore_focus = self.drop_replace_restore_focus.take();
        self.clear_pending_drop_replace_state(cx);
        if let Some(focus_id) = restore_focus {
            self.pending_focus = Some(focus_id);
            self.pending_scroll_active_block_into_view = true;
        }
        cx.notify();
    }

    pub(crate) fn discard_pending_drop_replace(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(path) = self.pending_drop_replace_path.take() else {
            self.clear_pending_drop_replace_state(cx);
            return;
        };

        self.clear_pending_drop_replace_state(cx);
        match self.replace_document_from_path(&path, cx) {
            Ok(()) => window.set_window_edited(false),
            Err(err) => self.show_drop_open_failed_prompt(err.to_string(), window, cx),
        }
    }

    pub(crate) fn save_and_replace_pending_drop(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.pending_drop_replace_path.is_none() {
            self.clear_pending_drop_replace_state(cx);
            return;
        }

        self.show_drop_replace_dialog = false;
        self.pending_drop_replace_after_save = true;
        self.close_menu_bar(cx);

        if let Some(path) = self.file_path.clone() {
            if self.save_to_existing_path(&path, window, cx) {
                self.replace_after_successful_save(window, cx);
            } else {
                self.abort_pending_drop_replace_after_save(cx);
            }
            return;
        }

        self.save_via_prompt_then_replace_drop(window, cx);
        cx.notify();
    }

    pub(crate) fn on_cancel_drop_replace_dialog(
        &mut self,
        _: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cancel_drop_replace_dialog(cx);
    }

    pub(crate) fn on_discard_and_replace_drop(
        &mut self,
        _: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.discard_pending_drop_replace(window, cx);
    }

    pub(crate) fn on_save_and_replace_drop(
        &mut self,
        _: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.save_and_replace_pending_drop(window, cx);
    }

    fn replace_after_successful_save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(drop_path) = self.pending_drop_replace_path.take() else {
            self.clear_pending_drop_replace_state(cx);
            return;
        };

        self.clear_pending_drop_replace_state(cx);
        match self.replace_document_from_path(&drop_path, cx) {
            Ok(()) => window.set_window_edited(false),
            Err(err) => self.show_drop_open_failed_prompt(err.to_string(), window, cx),
        }
    }

    fn save_via_prompt_then_replace_drop(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(drop_path) = self.pending_drop_replace_path.clone() else {
            self.clear_pending_drop_replace_state(cx);
            return;
        };
        let markdown = self.serialized_document_text(cx);
        let (default_dir, suggested_name) = self.save_dialog_defaults();
        let prompt = cx.prompt_for_new_path(&default_dir, suggested_name.as_deref());
        let weak_editor = cx.entity().downgrade();
        let weak_editor_for_cancel = weak_editor.clone();
        let weak_editor_for_error = weak_editor.clone();
        let weak_editor_for_write_error = weak_editor.clone();
        let window_handle = window.window_handle();

        cx.spawn(async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let mut save_path = match prompt.await {
                Ok(Ok(Some(path))) => path,
                Ok(Ok(None)) | Err(_) => {
                    let _ = weak_editor_for_cancel.update(cx, |this, cx| {
                        this.abort_pending_drop_replace_after_save(cx);
                    });
                    return;
                }
                Ok(Err(err)) => {
                    let _ = weak_editor_for_error.update(cx, |this, cx| {
                        this.abort_pending_drop_replace_after_save(cx);
                    });
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

            if save_path.extension().is_none() {
                save_path.set_extension("md");
            }

            if let Err(err) = std::fs::write(&save_path, &markdown) {
                let _ = weak_editor_for_write_error.update(cx, |this, cx| {
                    this.abort_pending_drop_replace_after_save(cx);
                });
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

            let saved_path = save_path.clone();
            let replace_result = weak_editor.update(cx, move |this, cx| {
                this.apply_successful_save(saved_path, cx);
                this.pending_drop_replace_path = Some(drop_path);
                this.replace_after_successful_save_async(cx)
            });
            let _ = cx.update_window(
                window_handle,
                move |_view: AnyView, window: &mut Window, cx: &mut App| match replace_result {
                    Ok(Ok(())) => window.set_window_edited(false),
                    Ok(Err(err)) => {
                        let strings = cx.global::<I18nManager>().strings().clone();
                        let buttons = [strings.info_dialog_ok.as_str()];
                        let _ = window.prompt(
                            PromptLevel::Critical,
                            &strings.open_failed_title,
                            Some(&err.to_string()),
                            &buttons,
                            cx,
                        );
                    }
                    Err(_) => {}
                },
            );
        })
        .detach();
    }

    fn replace_after_successful_save_async(&mut self, cx: &mut Context<Self>) -> Result<()> {
        let Some(drop_path) = self.pending_drop_replace_path.take() else {
            self.clear_pending_drop_replace_state(cx);
            return Ok(());
        };

        self.clear_pending_drop_replace_state(cx);
        self.replace_document_from_path(&drop_path, cx)
    }

    fn abort_pending_drop_replace_after_save(&mut self, cx: &mut Context<Self>) {
        self.pending_drop_replace_after_save = false;
        self.show_drop_replace_dialog = false;
        self.pending_drop_replace_path = None;
        let restore_focus = self.drop_replace_restore_focus.take();
        if let Some(focus_id) = restore_focus {
            self.pending_focus = Some(focus_id);
            self.pending_scroll_active_block_into_view = true;
        }
        cx.notify();
    }

    fn clear_pending_drop_replace_state(&mut self, cx: &mut Context<Self>) {
        let had_path = self.pending_drop_replace_path.take().is_some();
        let had_dialog = self.show_drop_replace_dialog;
        let had_after_save = self.pending_drop_replace_after_save;
        let had_restore_focus = self.drop_replace_restore_focus.take().is_some();
        let had_state = had_path || had_dialog || had_after_save || had_restore_focus;
        self.show_drop_replace_dialog = false;
        self.pending_drop_replace_after_save = false;
        if had_state {
            cx.notify();
        }
    }

    fn show_drop_open_failed_prompt(
        &self,
        detail: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let strings = cx.global::<I18nManager>().strings().clone();
        let buttons = [strings.info_dialog_ok.as_str()];
        let _ = window.prompt(
            PromptLevel::Critical,
            &strings.open_failed_title,
            Some(&detail),
            &buttons,
            cx,
        );
    }
}
