//! External Markdown file drops for replacing the current editor window.

use std::path::{Path, PathBuf};

use anyhow::{Context as AnyhowContext, Result};
use gpui::*;

use super::tree::PendingSourceTail;
use super::{DocumentView, Editor, ViewMode};
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

/// 整篇导入的两种口径。区别只在「阅读现场」：`Open` 是换一篇文档，视图模式、视口、
/// 光标归零重来；`Restore` 只换内容，现场按交回来的那份接着用（同一篇从磁盘重载、
/// 切回一篇读过的标签都走它）。
pub(super) enum ImportKind {
    Open,
    Restore(DocumentView),
}

/// 有现场就交还，没有就归零——`Option<DocumentView>` 到口径的这一层转换只在这里做一次。
fn import_kind_from_view(view: Option<DocumentView>) -> ImportKind {
    match view {
        Some(view) => ImportKind::Restore(view),
        None => ImportKind::Open,
    }
}

/// 代码文档的高亮语言：按扩展名，无扩展名（`.gitignore` 之类）按 `text`。
/// 不能返回 `None`——`replace_document_content` 把 `None` 当作 Markdown。
fn code_language_for_path(path: &Path) -> SharedString {
    path.extension()
        .map(|extension| extension.to_string_lossy().into_owned().into())
        .unwrap_or_else(|| SharedString::from("text"))
}

impl Editor {
    /// 构建源码模式（整文件直编）的根块列表：按行分块 + 连续源码行号。
    /// 导入（`replace_document_content`）与 undo 恢复共用，保证 undo 后
    /// 不会退化回整文件单块。`source_language`：markdown 源码分块是
    /// Paragraph，高亮语言记在块上（代码文件的块自带 kind 里的语言）。
    pub(super) fn build_source_document_roots(
        kind: BlockKind,
        source: &str,
        source_language: Option<&str>,
        cx: &mut Context<Self>,
    ) -> Vec<Entity<Block>> {
        let mut blocks = Vec::new();
        for chunk in split_source_document_chunks(source) {
            let record = BlockRecord::with_plain_text(kind.clone(), chunk);
            let block = Self::new_block(cx, record);
            block.update(cx, |block, _cx| {
                block.set_source_document_mode();
                if let Some(language) = source_language {
                    block.set_source_language(language);
                }
            });
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
        let document = super::encoding::load_document(path)
            .with_context(|| format!("failed to read '{}'", path.display()))?;
        let super::encoding::LoadedDocument { raw, text: markdown } = document;
        self.document_revision = self.document_revision.wrapping_add(1);
        self.autosave_task = None;
        self.recovery_source_path = None;
        self.is_recovered_document = false;
        if let Err(error) = crate::config::remove_recovery_snapshot(self.recovery_id) {
            eprintln!("failed to remove replaced document recovery snapshot: {error}");
        }
        // 与工作区树打开共用同一判定：只有 .md/.markdown 按 Markdown 解析，
        // 其余（.jsonl/.log/无扩展名等）一律代码文档。此前拖拽用 is_code_file
        // 白名单，.jsonl 被误当 Markdown 解析，两条入口行为不一致。
        if super::workspace::is_markdown_document(path) {
            self.replace_document_from_markdown(markdown, Some(path.to_path_buf()), cx);
        } else {
            self.replace_document_from_code_source(markdown, path.to_path_buf(), cx);
        }
        self.attach_file_origin(raw);
        crate::app_menu::record_recent_file_from_editor(path, cx);
        Ok(())
    }

    pub(super) fn replace_document_from_markdown(
        &mut self,
        markdown: String,
        file_path: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        self.replace_document_content(markdown, file_path, None, ImportKind::Open, cx);
    }

    pub(super) fn replace_document_from_code_source(
        &mut self,
        source: String,
        file_path: PathBuf,
        cx: &mut Context<Self>,
    ) {
        let language = code_language_for_path(&file_path);
        self.replace_document_content(
            source,
            Some(file_path),
            Some(language),
            ImportKind::Open,
            cx,
        );
    }

    /// 换内容，并尽量把阅读现场交还给用户：`view` 给得出来就按它摆（外部改动重载、
    /// 切回一个读过的标签），给不出来（本次会话第一次读这篇）按打开新文档的口径归零。
    pub(super) fn restore_document_from_markdown(
        &mut self,
        markdown: String,
        file_path: PathBuf,
        view: Option<DocumentView>,
        cx: &mut Context<Self>,
    ) {
        self.replace_document_content(
            markdown,
            Some(file_path),
            None,
            import_kind_from_view(view),
            cx,
        );
    }

    pub(super) fn restore_document_from_code_source(
        &mut self,
        source: String,
        file_path: PathBuf,
        view: Option<DocumentView>,
        cx: &mut Context<Self>,
    ) {
        let language = code_language_for_path(&file_path);
        self.replace_document_content(
            source,
            Some(file_path),
            Some(language),
            import_kind_from_view(view),
            cx,
        );
    }

    pub(super) fn replace_document_content(
        &mut self,
        markdown: String,
        file_path: Option<PathBuf>,
        code_language: Option<SharedString>,
        kind: ImportKind,
        cx: &mut Context<Self>,
    ) {
        // `Restore` 那份现场由调用方在换内容之前记好，这里只负责在块树重建之后交还。
        let restored_view = match kind {
            ImportKind::Open => None,
            ImportKind::Restore(view) => Some(view),
        };
        // P6 顺手项：LF 文档（常见日志/代码）零拷贝直通，避免两次全文分配。
        let had_cr = markdown.contains('\r');
        let normalized = if had_cr {
            markdown.replace("\r\n", "\n").replace('\r', "\n")
        } else {
            markdown
        };
        // 内容整体换掉了，事实源必须跟着换：缓冲区留着上一个文档的内容，
        // 保存就会写出别的文件的字节。
        self.reset_buffer_for_text(&normalized);
        let is_code = code_language.is_some();
        self.code_document = is_code;
        let source_mode_fallback_required =
            !is_code && Self::markdown_requires_source_mode_fallback(&normalized);
        // 视图口径要在建块树之前定下来：源码视图的块是缓冲区的连续切片，渲染视图的块
        // 是 markdown 解析的结果。先按渲染建、再把模式换成源码，两套位置换算就对不上
        // ——块起点不再落在行首，问行号那一步直接断言失败。
        let view_mode = if is_code || source_mode_fallback_required {
            ViewMode::Source
        } else if let Some(view) = &restored_view {
            // 交还现场：用户在读哪一面就从哪一面接着读。
            view.view_mode
        } else {
            ViewMode::Rendered
        };
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
                    // 代码文件的块语言在 kind 里，无需额外记。
                    Self::build_source_document_roots(chunk_kind, &normalized[..end - 1], None, cx)
                }
                _ => Self::build_source_document_roots(chunk_kind, &normalized, None, cx),
            };
            self.attach_source_slice_spans(&built, cx);
            built
        } else if view_mode == ViewMode::Source {
            // 交还的现场是源码视图，而这是一篇 markdown：整篇按源码切片直编，
            // 高亮语言记 markdown——与「渲染↔源码」切换用的是同一个入口。
            let built = Self::build_source_document_roots(
                BlockKind::Paragraph,
                &normalized,
                Some("markdown"),
                cx,
            );
            self.attach_source_slice_spans(&built, cx);
            built
        } else {
            self.rebuild_root_blocks_from_buffer(cx)
        };
        if roots.is_empty() {
            roots.push(Self::new_block(cx, BlockRecord::paragraph(String::new())));
        }

        // P7：内容哈希移出同步打开路径（10MiB 全文 SipHash ~15-25ms），
        // 后台计算后写回；保存/自动保存在此期间按未登记（None）处理。
        self.file_version = None;
        let open_generation = self.open_generation.wrapping_add(1);
        self.open_generation = open_generation;
        if file_path.is_some() {
            let hash_input = normalized.clone();
            let hash_generation = open_generation;
            cx.spawn(async move |this, cx| {
                let version = cx
                    .background_spawn(async move {
                        super::persistence::file_content_version_normalized(&hash_input)
                    })
                    .await;
                let _ = this.update(cx, |editor, _cx| {
                    if editor.open_generation == hash_generation && editor.file_version.is_none() {
                        editor.file_version = Some(version);
                    }
                });
            })
            .detach();
        }
        self.file_path = file_path;
        self.view_mode = view_mode;
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

        match restored_view {
            None => {
                self.pending_scroll_active_block_into_view = true;
                self.pending_scroll_recheck_after_layout = true;
                self.last_scroll_viewport_size = None;
                self.scroll_handle.set_offset(point(px(0.0), px(0.0)));
                self.pending_focus = self.first_focusable_entity_id(cx);
                self.active_entity_id = self.pending_focus;
            }
            Some(view) => {
                // 视口按交回来的那份摆好，并且不挂「滚到活动块」——挂了就会把视口
                // 拽向活动块（通常是文档首块），用户报修的正是这个。内容变矮时
                // gpui 在布局那一趟把偏移夹回可滚范围并写回，这里不必介入。
                self.pending_scroll_active_block_into_view = false;
                self.pending_scroll_center_into_view = false;
                self.pending_scroll_recheck_after_layout = false;
                self.scroll_handle
                    .set_offset(point(px(0.0), px(view.scroll_y)));
                // 块树整棵换掉，旧实体 id 一律作废：先清空，再由选区快照把光标落到
                // 新树上覆盖它的那一根（跨块选区只恢复选区本身、不写活动块，那种
                // 落点按文档首块，与打开新文档同口径）。
                self.pending_focus = None;
                self.active_entity_id = None;
                // 刚打开的大文件只同步建了首块（512 行），其余还在后台续建：落点在
                // 未物化的部分时选区找不到归属块，会被钳进首块末尾（与搜索跳转同一处
                // 报修）。先把续建落地，已物化的文档这里是空操作。
                self.flush_pending_materialization(cx);
                self.apply_selection_snapshot_in_current_mode(&view.selection, cx);
                if self.active_entity_id.is_none() {
                    self.pending_focus = self.first_focusable_entity_id(cx);
                    self.active_entity_id = self.pending_focus;
                }
            }
        }

        self.undo_history.clear();
        self.redo_history.clear();
        self.pending_undo_capture = None;
        self.last_selection_snapshot = Self::empty_selection_snapshot();
        self.history_restore_in_progress = false;
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
        // 先取字节再弹面板，和「另存为」同一条规则：保存的字节来自缓冲区，未编辑过的
        // 部分落盘就是打开时那份原文。取块树序列化会先把下划线强调变成星号、把表格
        // 列宽重新对齐。
        let markdown = self.document_bytes_for_save();
        let saved_text = self.document_text_for_save();
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

            if let Err(err) = super::persistence::write_atomic(&save_path, &markdown) {
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
                this.apply_successful_save(saved_path, saved_text, cx);
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
