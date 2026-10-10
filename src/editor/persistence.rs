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
    path.with_file_name(format!(".{name}.velora-{}.tmp", uuid::Uuid::new_v4()))
}

/// Atomic save (roadmap G1): write to a sibling temp file, fsync, then rename
/// over the destination so a crash never leaves a half-written document.
///
/// 收 `&[u8]` 而不是 `&str`：保存的内容可能就是打开时读到的原始字节（未编辑的
/// 文档），那不必是合法 UTF-8。
pub(super) fn write_atomic(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let temp_path = autosave_temp_path(path);
    let mut file = std::fs::File::create(&temp_path)?;
    file.write_all(contents)?;
    file.sync_all()?;
    drop(file);
    match std::fs::rename(&temp_path, path) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = std::fs::remove_file(&temp_path);
            Err(error)
        }
    }
}

/// 「这一篇不能写」的那一类失败：盘上的内容变了，或者已经读不出来了。
///
/// 用类型判别，不用错误文案前缀匹配。这里以前靠 `detail.starts_with("检测到外部修改")`
/// 分流，而失败上报那一支认两个前缀、决定要不要**再**弹一个框的那一支只认一个——
/// 「读不到盘上内容」于是同时在冲突框（带「重载 / 另存为」）和「保存失败」框上冒头。
/// `show_modal` 是替换语义，后弹的那个把唯一的解除入口顶掉了，用户只看见
/// 「保存失败」（报修原文）。文案改一个字就会再漏一次，所以钉在类型上。
#[derive(Debug)]
pub(super) struct ExternalChange {
    pub(super) path: PathBuf,
    /// 盘上的内容读不出来了（文件被移走、被别的程序独占、同步盘的占位符还没落地、
    /// 半截的 UTF-16）：与「内容变了」同样不能写，但理由要分开说。
    reason: Option<String>,
}

impl std::fmt::Display for ExternalChange {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.reason.as_deref() {
            Some(reason) => write!(
                formatter,
                "无法读取文件以检查外部修改：{}：{reason}",
                self.path.display()
            ),
            None => write!(formatter, "检测到外部修改：{}", self.path.display()),
        }
    }
}

impl std::error::Error for ExternalChange {}

/// 这个失败是不是「盘上的内容变了 / 读不到」那一类——它走冲突入口，不走「保存失败」。
fn is_external_change(error: &anyhow::Error) -> bool {
    error.downcast_ref::<ExternalChange>().is_some()
}

/// 读盘比对版本，并**把盘上的原始字节一起交回去**：落盘要按这份文件自己的形状重新
/// 编码（见 [`autosave_payload`]），而校验读的正是同一个文件，不必再读第二遍。
fn verify_file_version(path: &Path, expected_version: u64) -> anyhow::Result<Vec<u8>> {
    let document = super::encoding::load_document(path).map_err(|error| ExternalChange {
        path: path.to_path_buf(),
        reason: Some(error.to_string()),
    })?;
    if file_content_version(&document.text) != expected_version {
        return Err(anyhow::Error::new(ExternalChange {
            path: path.to_path_buf(),
            reason: None,
        }));
    }
    Ok(document.raw)
}

/// 后台标签落盘的字节：形状取自磁盘上还在的那份文件。
///
/// `WorkspaceDocumentTab` 只存缓冲区文本（LF、解码后的），不存字节与 `FileShape`——
/// tabs.rs / session_watcher.rs 里那两行「tab 快照携带字节与 FileShape（独立工作项，
/// 见 FIXPLAN B2）」就是这个洞：切走的页面没有缓冲区，落盘只剩 `text.as_bytes()`，
/// 于是 UTF-16 的 BOM 与编码、GB18030 的编码、CRLF 的行尾一起被洗成 UTF-8/LF。
/// 那正是「编码修复」要堵的那一类数据丢失，只是换了个入口。
///
/// 不在每个标签上再存一份整篇字节：盘上的形状还在原文件里，写之前按它重新编码就行，
/// 编码器仍然只有 `FileShape::encode` 一个，读盘的漏斗也只有 `encoding::load_document`
/// 一个——两处都是既有事实源，不新增第三份要同步的账。
pub(super) fn tab_write_bytes(path: &Path, text: &str) -> Vec<u8> {
    match super::encoding::load_document(path) {
        Ok(document) => encoded_with_disk_shape(text, &document.raw),
        // 原文件读不出来（已被移走，或它的字节本来就不能无损写回）：退回文本字节，
        // 与修之前的行为一致，不新增失败面。
        Err(_) => text.as_bytes().to_vec(),
    }
}

/// 按磁盘上那份文件的形状把文本重新编码；认不出形状就照原文本字节走。
fn encoded_with_disk_shape(text: &str, disk_raw: &[u8]) -> Vec<u8> {
    if disk_raw.is_empty() {
        return text.as_bytes().to_vec();
    }
    let shape = super::buffer::FileShape::detect(disk_raw);
    if !shape.is_lossless() {
        return text.as_bytes().to_vec();
    }
    shape.encode(text)
}

/// 这一篇要写出去的字节。
///
/// 活动文档用缓冲区的（`bytes` 有值：未编辑就是打开时那份原字节，编辑过就按形状
/// 重编码）。后台标签只能用切换时存下的文本，按盘上的形状重新编码（见
/// [`tab_write_bytes`]）。
fn autosave_payload(document: &PendingAutosaveDocument, disk_raw: &[u8]) -> Vec<u8> {
    match document.bytes.as_deref() {
        Some(bytes) => bytes.to_vec(),
        None => encoded_with_disk_shape(&document.recovery.markdown, disk_raw),
    }
}

pub(super) fn file_content_version(markdown: &str) -> u64 {
    let normalized = markdown.replace("\r\n", "\n").replace('\r', "\n");
    file_content_version_normalized(&normalized)
}

/// P7：对已规范化文本直接哈希（调用方保证输入无 `\r`），
/// 免掉一次全文拷贝。
pub(super) fn file_content_version_normalized(markdown: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    markdown.hash(&mut hasher);
    hasher.finish()
}

#[derive(Clone)]
struct PendingAutosaveDocument {
    recovery: crate::config::RecoverySnapshot,
    path: Option<PathBuf>,
    temp_path: Option<PathBuf>,
    file_version: Option<u64>,
    /// 活动文档的落盘字节（缓冲区原始字节/按形状重编码）。`None` = 后台标签，
    /// 退写恢复快照里的文本（形状丢失是已知限制）。
    bytes: Option<Vec<u8>>,
}

impl Editor {
    pub(super) fn has_marked_document_text(&self, cx: &App) -> bool {
        self.document.flatten_visible_blocks().iter().any(|block| {
            let block = block.entity.read(cx);
            block.marked_range.is_some() || block.code_language_marked_range.is_some()
        })
    }

    pub(super) fn schedule_autosave(&mut self, cx: &mut Context<Self>) {
        if self.pending_close_after_save
            || self.autosave_task.is_some()
            || self.has_external_autosave_conflict()
            || self.has_marked_document_text(cx)
            || (!self.document_dirty && !self.has_dirty_workspace_documents())
        {
            return;
        }

        let editor = cx.entity().downgrade();
        let window_handle = self.window_handle;
        self.autosave_task = Some(cx.spawn(
            async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
                let debounce = cx
                    .update(|cx| crate::config::EditorSettings::autosave_debounce_ms(cx))
                    .unwrap_or(800);
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(debounce))
                    .await;

                let snapshot = editor
                    .update(cx, |editor, cx| {
                        if editor.has_marked_document_text(cx) {
                            return None;
                        }
                        let autosave_writes = crate::config::EditorSettings::autosave(cx);
                        let mut documents = editor
                            .dirty_workspace_documents(cx)
                            .into_iter()
                            .map(|document| PendingAutosaveDocument {
                                recovery: crate::config::RecoverySnapshot {
                                    id: document.recovery_id,
                                    source_path: Some(document.path.clone()),
                                    markdown: document.markdown,
                                },
                                // 自动保存关掉时不给临时路径：后台那一趟只写恢复快照
                                // 并校验文件版本，末尾的 rename 循环按 `temp_path` 跳过
                                // 这一篇，真文件不动。
                                temp_path: autosave_writes
                                    .then(|| autosave_temp_path(&document.path)),
                                file_version: Some(document.file_version),
                                path: Some(document.path),
                                bytes: document.bytes,
                            })
                            .collect::<Vec<_>>();
                        if editor.document_dirty && editor.file_path.is_none() {
                            documents.push(PendingAutosaveDocument {
                                recovery: crate::config::RecoverySnapshot {
                                    id: editor.recovery_id,
                                    source_path: editor.recovery_source_path.clone(),
                                    markdown: editor.document_text_for_save(),
                                },
                                path: None,
                                temp_path: None,
                                file_version: None,
                                bytes: None,
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
                            // 失败时把真正出问题的文件路径一起带回去：冲突要记在
                            // 那个文件上，而不是第一个有路径的标签（用户报修）。
                            let failing_path = document.path.clone();
                            crate::config::save_recovery_snapshot(&document.recovery)
                                .map_err(|error| (failing_path.clone(), error))?;
                            if let (Some(path), Some(expected_version)) =
                                (document.path.as_deref(), document.file_version)
                            {
                                verify_file_version(path, expected_version)
                                    .map_err(|error| (failing_path.clone(), error))?;
                            }
                            if let Some(temp_path) = document.temp_path {
                                // 优先写缓冲区字节：CRLF/GB18030 文档经 autosave
                                // 不能被洗成 LF/UTF-8（那是「打开没动的字节被改写」
                                // 的旁门版本）。
                                let payload = document
                                    .bytes
                                    .as_deref()
                                    .unwrap_or(document.recovery.markdown.as_bytes());
                                std::fs::write(temp_path, payload)
                                    .map_err(|error| {
                                        (failing_path.clone(), anyhow::Error::from(error))
                                    })?;
                                if let (Some(path), Some(expected_version)) =
                                    (document.path.as_deref(), document.file_version)
                                {
                                    verify_file_version(path, expected_version)
                                        .map_err(|error| (failing_path.clone(), error))?;
                                }
                            }
                        }
                        Ok::<_, (Option<PathBuf>, anyhow::Error)>(())
                    })
                    .await;
                let conflict_detail = editor
                    .update(cx, move |editor, cx| {
                        editor.autosave_task = None;
                        match write_result {
                            Ok(()) => {}
                            Err((failing_path, error)) => {
                                for document in &documents {
                                    if let Some(temp_path) = document.temp_path.as_ref() {
                                        let _ = std::fs::remove_file(temp_path);
                                    }
                                }
                                if editor.document_revision != revision {
                                    editor.schedule_autosave(cx);
                                    return;
                                }
                                let external_change = is_external_change(&error);
                                let detail = error.to_string();
                                eprintln!("failed to save recovery snapshot: {detail}");
                                if external_change {
                                    // 优先用真正失败的文件路径；只有旧路径拿不到时才
                                    // 退回第一个有路径的标签。
                                    let conflict_path = failing_path.or_else(|| {
                                        documents.iter().find_map(|document| document.path.clone())
                                    });
                                    if let Some(conflict_path) = conflict_path {
                                        editor.report_external_change_conflict(
                                            conflict_path,
                                            detail.clone(),
                                            cx,
                                        );
                                    }
                                } else {
                                    editor.report_workspace_file_error(detail.clone(), cx);
                                }
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
                                bytes: document.bytes.clone(),
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
                    })
                    .ok();
            },
        ));
    }

    pub(super) fn save_dirty_workspace_documents_and_close(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.has_marked_document_text(cx) {
            self.pending_close_after_save = true;
            self.pending_save = true;
            cx.notify();
            return;
        }
        self.snapshot_current_document(cx);
        let documents = self.dirty_workspace_documents(cx);
        if self.document_dirty && self.file_path.is_none() {
            self.pending_close_after_save = true;
            self.pending_save = true;
            cx.notify();
            return;
        }
        if documents.is_empty() {
            Self::close_editor_window(window);
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
                        let disk_raw = verify_file_version(&document.path, document.file_version)?;
                        // 同 autosave：活动文档优先写缓冲区字节，后台那一页按磁盘上的
                        // 形状重新编码（形状只在盘上那份文件里，标签快照没有它）。
                        let payload = match document.bytes.as_deref() {
                            Some(bytes) => bytes.to_vec(),
                            None => encoded_with_disk_shape(&document.markdown, &disk_raw),
                        };
                        std::fs::write(&temp_path, payload).with_context(|| {
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
                            let external_change = is_external_change(&error);
                            let detail = error.to_string();
                            editor.pending_close_after_save = false;
                            editor.show_unsaved_changes_dialog = true;
                            editor.report_workspace_file_error(detail.clone(), cx);
                            eprintln!("failed to save workspace documents: {detail}");
                            cx.notify();
                            return (false, Some((external_change, detail)));
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
            if let Some((external_change, detail)) = error_detail {
                if let Some(error_window) = editor_window_for_error {
                    if external_change {
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
                        Editor::close_editor_window(window);
                    },
                );
            }
        })
        .detach();
    }

    /// 保存实际落盘的**文本**（LF）：就是缓冲区。
    ///
    /// 版本号、工作区标签文本、本地历史都取这份文本，不能取重新序列化的结果，
    /// 否则下一次校验磁盘时会发现自己刚写的文件「被外部改了」。源码/代码文档也走
    /// 这一条——每次改动都由 `resync_buffer_from_projection` 落进缓冲区，再序列化
    /// 一遍等于给保存路径留着「重拼整篇」这份可写字节。
    pub(super) fn document_text_for_save(&self) -> String {
        self.buffer.text()
    }

    /// 保存落盘的**字节**：写的就是缓冲区。未编辑过时 `file_bytes` 直接返回打开
    /// 读到的那串字节；编辑过的部分由区间写回落进缓冲区，未编辑的块保持原文，
    /// 行尾与编码按文件原来的形状重新编码。
    pub(super) fn document_bytes_for_save(&self) -> Vec<u8> {
        self.buffer.file_bytes()
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

    pub(super) fn apply_successful_save(
        &mut self,
        path: PathBuf,
        saved_text: String,
        cx: &mut Context<Self>,
    ) {
        self.document_revision = self.document_revision.wrapping_add(1);
        // 版本号必须来自**实际写盘的那份字节**对应的文本：另存流程在弹面板前就
        // 取了字节，面板期间缓冲区再变的话，完成时现取的文本与磁盘就对不上了，
        // 下一次校验会把自己写的文件误判成外部修改。
        let saved_markdown = saved_text;
        self.file_version = Some(file_content_version(&saved_markdown));
        // 标签里的版本号也要跟上（自动保存按它校验磁盘，见
        // mark_workspace_document_saved）。
        if let Some(file_version) = self.file_version {
            self.mark_workspace_document_saved(&path, file_version, &saved_markdown);
        }
        // 本地历史：每次成功保存后台落一条版本快照（同内容去重、每文件
        // 保留最近 20 条）。写盘不在保存关键路径上。
        let history_path = path.clone();
        let history_content = saved_markdown.clone();
        cx.spawn(async move |_this, cx| {
            let _ = cx.update(|_cx| {
                let _ = crate::config::record_file_history(&history_path, &history_content);
            });
        })
        .detach();
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
        if self.has_marked_document_text(cx) {
            self.pending_save = true;
            cx.notify();
            return false;
        }
        if let Some(expected_version) = self.file_version
            && let Err(error) = verify_file_version(path, expected_version)
        {
            let detail = error.to_string();
            let strings = cx.global::<I18nManager>().strings().clone();
            // 应用内模态，不用系统原生弹窗（用户要求）。
            if detail.starts_with("检测到外部修改") {
                self.report_workspace_file_error(detail.clone(), cx);
                // 手动保存撞上的冲突与自动保存撞上的同一个问题，给同一个解除入口
                // （重载 / 另存为），不再只有一块「好」按钮。
                self.show_external_change_conflict_modal(path.to_path_buf(), cx);
            } else {
                self.show_message_modal(strings.save_failed_title.clone(), detail.clone(), cx);
            }
            let _ = window;
            return false;
        }
        let bytes = self.document_bytes_for_save();
        let saved_text = self.document_text_for_save();
        match write_atomic(path, &bytes) {
            Ok(_) => {
                self.apply_successful_save(path.to_path_buf(), saved_text, cx);
                window.set_window_edited(false);
                true
            }
            Err(err) => {
                let detail = err.to_string();
                let strings = cx.global::<I18nManager>().strings().clone();
                self.show_message_modal(strings.save_failed_title.clone(), detail, cx);
                let _ = window;
                false
            }
        }
    }

    fn save_document_via_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.has_marked_document_text(cx) {
            self.pending_save = true;
            cx.notify();
            return;
        }
        // 先取字节再弹面板：未编辑的文档保存的就是打开时那份原始字节。
        let markdown = self.document_bytes_for_save();
        let saved_text = self.document_text_for_save();
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
                    let _ = weak_editor_for_error.update(cx, move |this, cx| {
                        let title = cx.global::<I18nManager>().strings().save_failed_title.clone();
                        this.show_message_modal(title, detail.clone(), cx);
                    });
                    let _ = window_handle;
                    return;
                }
            };

            if path.extension().is_none() {
                path.set_extension("md");
            }

            if let Err(err) = write_atomic(&path, &markdown) {
                if should_close_after_save {
                    let _ = weak_editor_for_write_error
                        .update(cx, |this, cx| this.abort_pending_close_after_save(cx));
                }
                let detail = err.to_string();
                let _ = weak_editor_for_write_error.update(cx, move |this, cx| {
                    let title = cx.global::<I18nManager>().strings().save_failed_title.clone();
                    this.show_message_modal(title, detail.clone(), cx);
                });
                let _ = window_handle;
                return;
            }

            let path_for_state = path.clone();
            let _ = weak_editor.update(cx, move |this, cx| {
                this.apply_successful_save(path_for_state, saved_text, cx);
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
                            Editor::close_editor_window(window);
                        }
                    }
                },
            );
        })
        .detach();
    }

    pub(crate) fn save_document(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending_close_after_save
            && self.has_dirty_workspace_documents()
            && (self.file_path.is_some() || !self.document_dirty)
        {
            self.save_dirty_workspace_documents_and_close(window, cx);
            return;
        }
        if let Some(path) = self.file_path.clone() {
            let should_close_after_save = self.pending_close_after_save;
            if self.save_to_existing_path(&path, window, cx) {
                if should_close_after_save {
                    Editor::close_editor_window(window);
                }
            } else if should_close_after_save {
                self.abort_pending_close_after_save(cx);
            }
            return;
        }

        self.save_document_via_prompt(window, cx);
    }

    pub(crate) fn save_document_as(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.has_marked_document_text(cx) {
            self.pending_save_as = true;
            cx.notify();
            return;
        }
        self.save_document_via_prompt(window, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::{safe_code_fence, safe_code_fence_with_info, write_atomic};

    #[test]
    fn atomic_write_replaces_content_and_leaves_no_temp() {
        let root = std::env::temp_dir().join(format!("velora-atomic-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("create root");
        let path = root.join("doc.md");
        std::fs::write(&path, "old").expect("seed");

        write_atomic(&path, b"new content").expect("atomic write");

        assert_eq!(std::fs::read_to_string(&path).expect("read"), "new content");
        let leftovers: Vec<_> = std::fs::read_dir(&root)
            .expect("read dir")
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "temp files left behind: {leftovers:?}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn atomic_write_failure_keeps_original_content() {
        let root = std::env::temp_dir().join(format!("velora-atomic-fail-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("create root");
        let path = root.join("doc.md");
        std::fs::write(&path, "original").expect("seed");

        // Target inside a missing subdirectory fails at temp-file creation.
        let missing = root.join("missing").join("doc.md");
        assert!(write_atomic(&missing, b"nope").is_err());
        assert_eq!(std::fs::read_to_string(&path).expect("read"), "original");
        let _ = std::fs::remove_dir_all(root);
    }

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
