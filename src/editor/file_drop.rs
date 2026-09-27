//! External Markdown file drops for replacing the current editor window.

use std::path::{Path, PathBuf};

use anyhow::{Context as AnyhowContext, Result};
use gpui::*;

use super::tree::PendingSourceTail;
use super::{Editor, ViewMode};
use crate::components::{Block, BlockKind, BlockRecord};
use crate::i18n::I18nManager;

/// P6a：从 `offset` 起跳过 `lines` 个换行，返回其后的字节下标；
/// 不足 `lines` 个换行时返回 `None`（剩余即文件末块）。
pub(super) fn scan_chunk_end(source: &[u8], offset: usize, lines: usize) -> Option<usize> {
    let mut cursor = offset;
    for _ in 0..lines {
        let rest = &source[cursor..];
        let position = rest.iter().position(|&byte| byte == b'\n')?;
        cursor += position + 1;
    }
    Some(cursor)
}

/// 源码文档分块的每块行数（docs/architecture/performance.md P2）。视口
/// 窗口化以块为粒度裁剪，块太大则单块即窗口；512 行约几十 KB，足以让
/// 10 MiB 日志保持几十个块，又不至于让块数量本身成为开销。
pub(super) const SOURCE_DOCUMENT_CHUNK_LINES: usize = 512;

/// 把源码文本按行切成若干块。各块文本不含块间换行符：把分块结果用
/// `'\n'` 连接即可逐字节还原输入（空文本返回单个空块）。
pub(super) fn split_source_document_chunks(source: &str) -> Vec<String> {
    let lines: Vec<&str> = source.split('\n').collect();
    if lines.len() <= SOURCE_DOCUMENT_CHUNK_LINES {
        return vec![source.to_string()];
    }
    lines
        .chunks(SOURCE_DOCUMENT_CHUNK_LINES)
        .map(|group| group.join("\n"))
        .collect()
}

impl Editor {
    /// 构建源码模式（整文件直编）的根块列表：按行分块 + 连续源码行号。
    /// 导入（`replace_document_content`）与 undo 恢复共用，保证 undo 后
    /// 不会退化回整文件单块。
    pub(super) fn build_source_document_roots(
        kind: BlockKind,
        source: &str,
        cx: &mut Context<Self>,
    ) -> Vec<Entity<Block>> {
        let mut blocks = Vec::new();
        for chunk in split_source_document_chunks(source) {
            let record = BlockRecord::with_plain_text(kind.clone(), chunk);
            let block = Self::new_block(cx, record);
            block.update(cx, |block, _cx| block.set_source_document_mode());
            blocks.push(block);
        }
        let mut next_line = 1usize;
        for block in &blocks {
            let line_count = block.update(cx, |block, _cx| {
                block.set_source_line_start(next_line);
                block.display_text().split('\n').count()
            });
            next_line += line_count;
        }
        blocks
    }
}

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
        self.show_welcome = false;
        // Dropping a folder makes it the working set of this window.
        if let Some(folder) = paths
            .paths()
            .iter()
            .find(|path| path.is_dir())
            .cloned()
        {
            self.set_workspace_root(folder, cx);
            return;
        }

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
        // Extension-less text files (dotfiles like .gitignore) must still load
        // as code: a `None` language means "markdown" to
        // `replace_document_content`, so give them an explicit `text` one.
        let language = file_path
            .extension()
            .map(|extension| extension.to_string_lossy().into_owned().into())
            .or_else(|| Some(SharedString::from("text")));
        self.replace_document_content(source, Some(file_path), language, cx);
    }

    pub(super) fn replace_document_content(
        &mut self,
        markdown: String,
        file_path: Option<PathBuf>,
        code_language: Option<SharedString>,
        cx: &mut Context<Self>,
    ) {
        // P6 顺手项：LF 文档（常见日志/代码）零拷贝直通，避免两次全文分配。
        let had_cr = markdown.contains('\r');
        let had_crlf = markdown.contains("\r\n");
        let normalized = if had_cr {
            markdown.replace("\r\n", "\n").replace('\r', "\n")
        } else {
            markdown
        };
        let is_code = code_language.is_some();
        self.code_document = is_code;
        self.code_uses_crlf = is_code && had_crlf;
        let source_mode_fallback_required =
            !is_code && Self::markdown_requires_source_mode_fallback(&normalized);
        let mut pending_code_tail = None;
        let mut roots = if is_code || source_mode_fallback_required {
            // 大纯文本文件不再整文件压进单块：按行分块让视口窗口化能裁剪
            // 屏外内容（docs/architecture/performance.md P2）。各块文本不含
            // 块间换行符，`raw_source_text` 用 '\n' 连接即逐字节还原。
            // 首块同步建、其余经 PendingTail 后台续建（P2c）。
            let chunk_kind = if is_code {
                BlockKind::CodeBlock {
                    language: code_language,
                }
            } else {
                BlockKind::Paragraph
            };
            // P6a：不再把全文切成近十万个 String——只同步建首块，剩余
            // 原始字节进 PendingSourceTail 后台按行切块续建。
            let built = match scan_chunk_end(
                normalized.as_bytes(),
                0,
                SOURCE_DOCUMENT_CHUNK_LINES,
            ) {
                Some(end) if is_code && end < normalized.len() => {
                    pending_code_tail = Some(PendingSourceTail {
                        source: normalized[end..].to_string(),
                        next_line: SOURCE_DOCUMENT_CHUNK_LINES,
                    });
                    Self::build_source_document_roots(chunk_kind, &normalized[..end - 1], cx)
                }
                _ => Self::build_source_document_roots(chunk_kind, &normalized, cx),
            };
            built
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
        // 导入视为一次修订：按 revision 缓存的统计（字数/行数等）全部失效。
        self.document_revision = self.document_revision.wrapping_add(1);
        self.document.replace_roots(roots, cx);
        if let Some(tail) = pending_code_tail {
            self.document.set_pending_source(Some(tail));
            self.start_pending_materialization_task(cx);
        }
        self.table_cells.clear();
        self.rebuild_table_runtimes(cx);
        self.rebuild_image_runtimes(cx);

        self.document_dirty = false;
        self.pending_window_edited = false;
        self.pending_window_title_refresh = true;
        self.pending_save = false;
        self.pending_save_as = false;
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
        // 导入路径无需再全文重序列化一遍：last_stable_source_text 就是
        // 刚刚建块的源文本（无损导入不变量），刷新只会白付一次 O(n)。
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
                    let _ = weak_editor_for_error.update(cx, move |this, cx| {
                        let title = cx.global::<I18nManager>().strings().save_failed_title.clone();
                        this.show_message_modal(title, detail.clone(), cx);
                    });
                    let _ = window_handle;
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
                let _ = weak_editor_for_write_error.update(cx, move |this, cx| {
                    let title = cx.global::<I18nManager>().strings().save_failed_title.clone();
                    this.show_message_modal(title, detail.clone(), cx);
                });
                let _ = window_handle;
                return;
            }

            let saved_path = save_path.clone();
            let replace_result = weak_editor.update(cx, move |this, cx| {
                this.apply_successful_save(saved_path, cx);
                this.pending_drop_replace_path = Some(drop_path);
                this.replace_after_successful_save_async(cx)
            });
            let replaced_ok = matches!(replace_result, Ok(Ok(())));
            let failure_detail = match &replace_result {
                Ok(Err(err)) => Some(err.to_string()),
                _ => None,
            };
            if replaced_ok {
                let _ = cx.update_window(window_handle, |_view: AnyView, window, _cx| {
                    window.set_window_edited(false);
                });
            }
            if let Some(detail) = failure_detail {
                let _ = weak_editor.update(cx, move |this, cx| {
                    let title = cx.global::<I18nManager>().strings().open_failed_title.clone();
                    this.show_message_modal(title, detail.clone(), cx);
                });
            }
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
        &mut self,
        detail: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // 应用内模态，不用系统原生弹窗（用户要求）。
        let strings = cx.global::<I18nManager>().strings().clone();
        self.show_message_modal(strings.open_failed_title.clone(), detail, cx);
        let _ = window;
    }
}

#[cfg(test)]
mod tests {
    use super::split_source_document_chunks;

    #[test]
    fn small_sources_stay_one_chunk() {
        assert_eq!(split_source_document_chunks(""), vec![""]);
        assert_eq!(split_source_document_chunks("a"), vec!["a"]);
        assert_eq!(split_source_document_chunks("a\nb\n"), vec!["a\nb\n"]);
    }

    #[test]
    fn chunks_join_back_to_the_original_bytes() {
        for line_count in [0usize, 1, 511, 512, 513, 1_024, 1_025, 2_000] {
            let mut source = String::new();
            for index in 0..line_count {
                source.push_str(&format!("line-{index}\n"));
            }
            let chunks = split_source_document_chunks(&source);
            let total_lines: usize = chunks
                .iter()
                .map(|chunk| chunk.split('\n').count())
                .sum();
            if line_count == 0 {
                assert_eq!(chunks, vec![""]);
            } else {
                assert!(
                    chunks.len() > 1 || line_count <= 512,
                    "line_count={line_count} chunks={}",
                    chunks.len()
                );
            }
            assert_eq!(
                chunks.join("\n"),
                source,
                "line_count={line_count}: 分块用换行连接必须还原原文"
            );
            assert!(total_lines >= line_count);
            for window in chunks.windows(2) {
                assert!(
                    !window[0].ends_with('\n'),
                    "非末块不得以换行结尾（块间换行由序列化连接补上）"
                );
            }
        }
    }

    #[test]
    fn empty_interior_lines_preserved_across_chunks() {
        let source = "a\n\n\nb\n";
        let chunks = split_source_document_chunks(source);
        assert_eq!(chunks.join("\n"), source);
    }
}
