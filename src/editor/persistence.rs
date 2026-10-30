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
    // 命名判据的单点在 config::save_temp_file_name：启动清扫与监听过滤按同一
    // 口径识别这些文件。
    path.with_file_name(crate::config::save_temp_file_name(&name))
}

/// Atomic save (roadmap G1): write to a sibling temp file, fsync, then rename
/// over the destination so a crash never leaves a half-written document.
///
/// 收 `&[u8]` 而不是 `&str`：保存的内容可能就是打开时读到的原始字节（未编辑的
/// 文档），那不必是合法 UTF-8。
pub(super) fn write_atomic(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    StagedDocumentWrite::prepare(path, contents)?.commit()
}

pub(super) struct StagedDocumentWrite {
    target: PathBuf,
    temporary: PathBuf,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct DocumentSaveIdentity {
    recovery_id: uuid::Uuid,
    buffer_id: u64,
    revision: u64,
}

/// 这一笔要写到哪个文件上：软链穿透到真身，路径键与标签侧同一处
/// （`file_url::canonical_path` 会剥掉 Windows 的 verbatim 前缀）——各解各的会让
/// 同一文件在保存侧与标签侧是两个不相等的键。
fn canonical_write_target(path: &Path) -> PathBuf {
    crate::file_url::canonical_path(path)
}

impl StagedDocumentWrite {
    pub(super) fn prepare(path: &Path, contents: &[u8]) -> std::io::Result<Self> {
        Self::prepare_with_baseline(path, contents, None)
    }

    /// `baseline` 是刚为版本校验读过的那份内容（见
    /// [`capture_baseline_with`](crate::config::file_history::capture_baseline_with)）。
    pub(super) fn prepare_with_baseline(
        path: &Path,
        contents: &[u8],
        baseline: Option<&[u8]>,
    ) -> std::io::Result<Self> {
        use std::io::Write;
        crate::config::file_history::capture_baseline_with(path, baseline)?;
        let target = canonical_write_target(path);
        let permissions = match std::fs::metadata(&target) {
            Ok(metadata) => Some(metadata.permissions()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        let staged = Self { temporary: autosave_temp_path(&target), target };
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&staged.temporary)?;
        if let Some(permissions) = permissions {
            file.set_permissions(permissions)?;
        }
        file.write_all(contents)?;
        file.sync_all()?;
        drop(file);
        Ok(staged)
    }

    fn commit(self) -> std::io::Result<()> {
        std::fs::rename(&self.temporary, &self.target)?;
        crate::config::sync_parent_directory_after_rename(&self.target);
        Ok(())
    }

    /// 读盘校验、临时文件写入与目录锁都是系统调用；`verify` 与 `rename` 之间没有
    /// 可移植的原子比较交换，外部写者在这个窗口里改文件会被 rename 覆盖。窗口只
    /// 剩这些调用的间隙（校验紧挨着 rename），要更小只能上跨进程文件锁；
    /// 这里接受现状，不把它误当成「校验通过就绝无覆盖」。
    pub(super) fn commit_checked(self, path: &Path, expected_version: u64) -> anyhow::Result<()> {
        let target = std::fs::canonicalize(path)
            .map(|target| canonical_write_target(&target))
            .map_err(|error| ExternalChange {
                path: path.to_path_buf(), reason: Some(error.to_string()),
            })?;
        if target != self.target {
            return Err(ExternalChange {
                path: path.to_path_buf(), reason: Some("文件目标已变化".into()),
            }.into());
        }
        verify_file_version(path, expected_version)?;
        Ok(self.commit()?)
    }
}

impl Drop for StagedDocumentWrite {
    fn drop(&mut self) {
        // 取消任务、批量准备失败或旧版本被拒绝时，也不能留下正文临时文件。
        if let Err(error) = std::fs::remove_file(&self.temporary)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!("无法清理保存临时文件 '{}': {error}", self.temporary.display());
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
    path: PathBuf,
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
pub(super) fn is_external_change(error: &anyhow::Error) -> bool {
    error.downcast_ref::<ExternalChange>().is_some()
}

/// 读盘比对版本，并**把盘上的原始字节一起交回去**：落盘要按这份文件自己的形状重新
/// 编码（见 [`autosave_payload`]），而校验读的正是同一个文件，不必再读第二遍。
pub(super) fn verify_file_version(path: &Path, expected_version: u64) -> anyhow::Result<Vec<u8>> {
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

/// 后台标签没有未编辑的原始字节，写入前用同一次版本校验读取的文件形状编码。
pub(super) fn tab_write_bytes(path: &Path, text: &str, expected_version: u64) -> anyhow::Result<Vec<u8>> {
    let raw = verify_file_version(path, expected_version)?;
    Ok(encoded_with_disk_shape(text, &raw))
}

/// 按磁盘上那份文件的形状把文本重新编码；认不出形状就照原文本字节走。
pub(super) fn encoded_with_disk_shape(text: &str, disk_raw: &[u8]) -> Vec<u8> {
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

pub(crate) fn file_content_version(markdown: &str) -> u64 {
    // 没有 CR 就不必为规范化再拷一份全文：AI 的每次 read_document 都会算版本。
    if !markdown.contains('\r') {
        return file_content_version_normalized(markdown);
    }
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

/// 手动保存转后台之后，成功那一刻要接着做的动作（关窗、切工作区根、替换拖入
/// 的文件、继续关闭一批标签）。失败时各调用方自己的"中止"语义在完成回调里处理。
#[derive(Clone)]
pub(crate) enum SaveThen {
    CloseWindow,
    SwitchWorkspaceRoot(PathBuf),
    ReplacePendingDrop,
    CloseWorkspaceTabs(Vec<super::workspace::WorkspaceDocumentTab>),
}

#[derive(Clone)]
struct PendingAutosaveDocument {
    recovery: crate::config::RecoverySnapshot,
    path: Option<PathBuf>,
    write_file: bool,
    file_version: Option<u64>,
    /// 活动文档的落盘字节（缓冲区原始字节/按形状重编码）。`None` = 后台标签，
    /// 退写恢复快照里的文本（形状丢失是已知限制）。
    bytes: Option<Vec<u8>>,
}

impl Editor {
    pub(super) fn has_marked_document_text(&self, cx: &App) -> bool {
        if self.split_workspace.is_some() {
            return self.split_groups().iter().any(|group| group.read(cx).has_marked_document_text(cx));
        }
        let mut scanned = 0u64;
        let marked = self.composition_blocks.iter().any(|tracked| {
            scanned += 1;
            tracked
                .read_with(cx, |block, _| block.has_marked_text())
                .unwrap_or(false)
        });
        self.autosave_guard_block_reads
            .set(self.autosave_guard_block_reads.get() + scanned);
        marked
    }

    pub(super) fn schedule_autosave(&mut self, cx: &mut Context<Self>) {
        if self.split_initializing { return; }
        if let Some(parent) = self.split_parent.clone() {
            cx.defer(move |cx| {
                if let Err(error) = parent.update(cx, |editor, cx| editor.schedule_autosave(cx)) {
                    eprintln!("安排分屏自动保存失败：{error}");
                }
            });
            return;
        }
        // 外部修改冲突不在这里拦：冲突是「这一篇写不了」，不是
        // 「全窗口都停下」。冲突文档的磁盘提交由版本校验挡着（快照照常写，
        // 不覆盖磁盘、无覆盖风险），其余文档的快照与写盘必须继续，否则用户在
        // 另一篇打字，崩溃即丢全部未保存内容。
        if self.pending_close_after_save
            || self.autosave_task.is_some()
            || self.has_marked_document_text(cx)
            || (!self.document_dirty && !self.has_dirty_workspace_documents() && !self.has_dirty_split_documents(cx) && !self.has_untitled_documents(cx))
        {
            return;
        }
        self.spawn_autosave_round(cx);
    }

    /// 排一轮自动保存（防抖 + 快照 + 写盘）。`schedule_autosave` 把守卫收口后
    /// 走这里；组合态重试也直接走这里——那一次恰恰要绕开组合守卫（见
    /// `AutosaveTick::RetryAfterComposition`）。
    fn spawn_autosave_round(&mut self, cx: &mut Context<Self>) {
        let editor = cx.entity().downgrade();
        let verify_reads = self.autosave_verify_reads.clone();
        self.autosave_task = Some(cx.spawn(
            async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
                let debounce = cx
                    .update(|cx| crate::config::EditorSettings::autosave_debounce_ms(cx));
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(debounce))
                    .await;

                enum AutosaveTick {
                    Save(std::sync::Arc<Vec<PendingAutosaveDocument>>, u64),
                    /// 到点发现输入法组合还挂着：本轮取消，但要重排下一轮。
                    /// 组合可能一直挂着（用户停手离开），不重排的话恢复快照与
                    /// 文件写全部停更，直到下一次编辑事件为止。
                    RetryAfterComposition,
                    Idle,
                }
                let tick = editor
                    .update(cx, |editor, cx| {
                        if editor.has_marked_document_text(cx) {
                            let has_unsaved = editor.document_dirty
                                || editor.has_dirty_workspace_documents()
                                || editor.has_dirty_split_documents(cx)
                                || editor.has_untitled_documents(cx);
                            return if has_unsaved {
                                AutosaveTick::RetryAfterComposition
                            } else {
                                AutosaveTick::Idle
                            };
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
                                write_file: autosave_writes,
                                file_version: Some(document.file_version),
                                path: Some(document.path),
                                bytes: document.bytes,
                            })
                            .collect::<Vec<_>>();
                        if editor.document_dirty && editor.file_path.is_none() && editor.active_document_path().is_none() {
                            documents.push(PendingAutosaveDocument {
                                recovery: crate::config::RecoverySnapshot {
                                    id: editor.recovery_id,
                                    source_path: editor.recovery_source_path.clone(),
                                    markdown: editor.document_text_for_save(),
                                },
                                path: None,
                                write_file: false,
                                file_version: None,
                                bytes: None,
                            });
                        }
                        documents.extend(editor.untitled_recovery_snapshots(cx).into_iter().map(|recovery| PendingAutosaveDocument {
                            recovery, path: None, write_file: false, file_version: None, bytes: None,
                        }));
                        if documents.is_empty() {
                            return AutosaveTick::Idle;
                        }
                        AutosaveTick::Save(std::sync::Arc::new(documents), editor.document_revision)
                    })
                    .ok();
                let (documents, revision) = match tick {
                    Some(AutosaveTick::Save(documents, revision)) => (documents, revision),
                    Some(AutosaveTick::RetryAfterComposition) => {
                        let _ = editor.update(cx, |editor, cx| {
                            editor.autosave_task = None;
                            // 不走 schedule_autosave：它的组合守卫会把这次重排
                            // 立刻退回去，而等待组合结束正是重排的全部目的。
                            if !editor.pending_close_after_save {
                                editor.spawn_autosave_round(cx);
                            }
                        });
                        return;
                    }
                    Some(AutosaveTick::Idle) | None => {
                        let _ = editor.update(cx, |editor, _cx| editor.autosave_task = None);
                        return;
                    }
                };

                let documents_for_write = documents.clone();
                let write_result = cx
                    .background_executor()
                    .spawn(async move {
                        let mut prepared = Vec::new();
                        let mut conflicts = Vec::new();
                        for document in documents_for_write.iter() {
                            let failing_path = document.path.clone();
                            // 一篇失败不再拖垮整批：出问题的这一篇
                            // 跳过磁盘提交，其余文档的快照与写盘照常走。
                            if let Err(error) = crate::config::save_recovery_snapshot(&document.recovery) {
                                conflicts.push((failing_path, error));
                                prepared.push(None);
                                continue;
                            }
                            let disk_raw = match (document.path.as_deref(), document.file_version)
                            {
                                (Some(path), Some(expected_version)) => {
                                    verify_reads.fetch_add(
                                        1,
                                        std::sync::atomic::Ordering::Relaxed,
                                    );
                                    match verify_file_version(path, expected_version) {
                                        Ok(raw) => raw,
                                        Err(error) => {
                                            conflicts.push((Some(path.to_path_buf()), error));
                                            prepared.push(None);
                                            continue;
                                        }
                                    }
                                }
                                _ => Vec::new(),
                            };
                            let mut staged = None;
                            if let Some(path) = document.path.as_ref().filter(|_| document.write_file) {
                                // 优先写缓冲区字节；后台那一页按磁盘上的形状重新编码
                                // （见 `autosave_payload`），CRLF/UTF-16/GB18030 文档
                                // 经 autosave 不能被洗成 LF/UTF-8。
                                let payload = autosave_payload(document, &disk_raw);
                                match StagedDocumentWrite::prepare_with_baseline(
                                    path,
                                    &payload,
                                    Some(&disk_raw),
                                ) {
                                    Ok(staged_write) => {
                                        staged = Some((staged_write, payload));
                                    }
                                    Err(error) => {
                                        conflicts.push((failing_path, anyhow::Error::from(error)));
                                        prepared.push(None);
                                        continue;
                                    }
                                }
                            }
                            prepared.push(staged);
                        }
                        (prepared, conflicts)
                    })
                    .await;
                editor
                    .update(cx, move |editor, cx| {
                        editor.autosave_task = None;
                        let (prepared, conflicts) = write_result;
                        if editor.document_revision != revision {
                            for document in documents.iter() {
                                let is_open = if document.path.is_none() {
                                    editor.is_untitled_recovery_open(document.recovery.id, cx)
                                } else {
                                    editor.workspace_save_document_is_open(document.recovery.id, document.path.as_deref())
                                };
                                if !is_open
                                    && let Err(error) = crate::config::remove_recovery_snapshot(document.recovery.id)
                                {
                                    eprintln!("无法清理已关闭文档的恢复快照：{error}");
                                }
                            }
                            drop(prepared);
                            editor.schedule_autosave(cx);
                            return;
                        }

                        let mut saved_documents = Vec::new();
                        // 一趟里的失败先收集再上报：`show_modal` 是**替换**语义，
                        // 逐条弹框会把带「重载 / 另存为」的冲突框顶成只剩一块「好」
                        // 的「保存失败」。有冲突只出冲突框；冲突槽位已被占着就不再
                        // 重弹（自动保存每轮都会重新撞上同一个冲突），纯写盘失败
                        // 一趟只补一块「保存失败」，其余走红字。
                        let mut conflict: Option<(PathBuf, String)> = None;
                        let mut write_failures: Vec<String> = Vec::new();
                        // 准备阶段收集的失败：外部修改那一类进冲突入口，其余算纯
                        // 写盘失败；冲突的那一篇本轮没有暂存写（prepared 里是
                        // None），别的文档不受牵连。
                        for (failing_path, error) in conflicts {
                            if is_external_change(&error) {
                                // 优先用真正失败的文件路径；只有旧路径拿不到时才
                                // 退回第一个有路径的标签。
                                let conflict_path = failing_path
                                    .or_else(|| documents.iter().find_map(|document| document.path.clone()));
                                if let Some(conflict_path) = conflict_path {
                                    conflict.get_or_insert((conflict_path, error.to_string()));
                                }
                            } else {
                                write_failures.push(error.to_string());
                            }
                        }
                        #[cfg(test)]
                        if let Some(hook) = editor.save_commit_hook.take() {
                            hook(editor, cx);
                        }
                        for (document, staged) in documents.iter().zip(prepared) {
                            if document.path.is_none() && !editor.is_untitled_recovery_open(document.recovery.id, cx) {
                                if let Err(error) = crate::config::remove_recovery_snapshot(document.recovery.id) {
                                    eprintln!("清理已关闭草稿的异步快照失败：{error}");
                                }
                            }
                            let (Some(path), Some((staged, saved_bytes))) =
                                (document.path.as_ref(), staged)
                            else {
                                continue;
                            };
                            if !editor.workspace_save_snapshot_is_current(document.recovery.id, path, &document.recovery.markdown) {
                                if !editor.workspace_save_document_is_open(document.recovery.id, Some(path))
                                    && let Err(error) = crate::config::remove_recovery_snapshot(document.recovery.id)
                                {
                                    eprintln!("无法清理已关闭文档的恢复快照：{error}");
                                }
                                continue;
                            }
                            let result = match document.file_version {
                                Some(version) => {
                                    editor.autosave_verify_reads.fetch_add(
                                        1,
                                        std::sync::atomic::Ordering::Relaxed,
                                    );
                                    staged.commit_checked(path, version)
                                }
                                None => staged.commit().map_err(anyhow::Error::from),
                            };
                            if let Err(error) = result {
                                eprintln!("failed to autosave '{}': {error}", path.display());
                                if is_external_change(&error) {
                                    // `external_change_conflict` 是单槽位：这一趟取第一处
                                    // 够用，多篇都冲突时它们本来就是同一个外部动作。
                                    conflict.get_or_insert_with(|| (path.clone(), error.to_string()));
                                } else {
                                    write_failures.push(error.to_string());
                                }
                                continue;
                            }
                            editor.record_saved_history(path, &saved_bytes, crate::config::file_history::HistorySource::Automatic, cx);
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
                                bytes: Some(saved_bytes),
                            });
                        }
                        let mut write_failures = write_failures.into_iter();
                        if let Some((path, detail)) = conflict {
                            // 冲突槽位已被占着（用户还没解除上一处）就不再重弹：
                            // show_modal 是替换语义，自动保存每一轮都重弹的话，
                            // 那个解除入口永远关不掉。红字仍在，这一篇的磁盘提交
                            // 也仍被版本校验挡着。
                            if editor.has_external_autosave_conflict() {
                                eprintln!("外部修改冲突仍未解除 '{}'：{detail}", path.display());
                            } else {
                                editor.report_external_change_conflict(path, detail, cx);
                            }
                        } else if let Some(first) = write_failures.next() {
                            // 一趟只出**一个**框：有冲突出冲突框（带解除入口），
                            // 没有冲突才补一块纯写盘失败的「保存失败」。
                            editor.report_workspace_file_error(first.clone(), cx);
                            let title =
                                cx.global::<I18nManager>().strings().save_failed_title.clone();
                            editor.show_message_modal(title, first, cx);
                        }
                        for detail in write_failures {
                            editor.report_workspace_file_error(detail, cx);
                        }
                        editor.propagate_split_saves(&saved_documents, cx);
                        if editor.mark_workspace_documents_saved(&saved_documents) {
                            editor.document_dirty = false;
                            editor.pending_window_edited = false;
                            editor.pending_window_unedited = true;
                            editor.pending_window_title_refresh = true;
                            editor.sync_current_document_content();
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
        if self.save_untitled_split_before_close(window, cx) { return; }
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
        let background = cx.background_executor().clone();

        cx.spawn(async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let documents_for_write = documents;
            let write_result = background
                .spawn(async move {
                    let mut prepared = Vec::new();
                    for document in documents_for_write {
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
                        let staged = StagedDocumentWrite::prepare(&document.path, &payload).with_context(|| {
                            format!("failed to stage '{}'", document.path.display())
                        })?;
                        verify_file_version(&document.path, document.file_version)?;
                        prepared.push((document, staged, payload));
                    }
                    Ok::<_, anyhow::Error>(prepared)
                })
                .await;
            // 错误详情先取出来：实体没了（窗口已销毁）时应用内上报没机会跑，
            // 跨窗口静态入口要用。
            let failure_detail = write_result
                .as_ref()
                .err()
                .map(|error| (is_external_change(error), error.to_string()));

            let update_outcome = editor
                .update(cx, move |editor, cx| {
                    editor.autosave_task = None;
                    let prepared = match write_result {
                        Ok(prepared) => prepared,
                        Err(error) => {
                            let detail = error.to_string();
                            editor.pending_close_after_save = false;
                            editor.show_unsaved_changes_dialog = true;
                            eprintln!("failed to save workspace documents: {detail}");
                            // 「盘上内容变了 / 读不到」与自动保存撞上的同类问题走
                            // 同一个带解除入口的冲突记录，不再只给一块纯展示的
                            // 消息模态。错误详情不再往闭包外递第二份重复弹框——
                            // show_modal 是替换语义，再弹会把冲突框顶掉。
                            if let Some(external) = error.downcast_ref::<ExternalChange>() {
                                editor.report_external_change_conflict(external.path.clone(), detail, cx);
                            } else {
                                editor.report_workspace_file_error(detail.clone(), cx);
                                let title = cx.global::<I18nManager>().strings().save_failed_title.clone();
                                editor.show_message_modal(title, detail, cx);
                            }
                            cx.notify();
                            return false;
                        }
                    };
                    if editor.document_revision != revision {
                        drop(prepared);
                        editor.pending_close_after_save = false;
                        editor.show_unsaved_changes_dialog = true;
                        cx.notify();
                        return false;
                    }

                    let mut saved_documents = Vec::new();
                    let mut failed_save = false;
                    #[cfg(test)]
                    if let Some(hook) = editor.save_commit_hook.take() {
                        hook(editor, cx);
                    }
                    for (document, staged, payload) in prepared {
                        if !editor.workspace_save_snapshot_is_current(document.recovery_id, &document.path, &document.markdown) {
                            failed_save = true;
                            continue;
                        }
                        if let Err(error) = staged.commit_checked(&document.path, document.file_version) {
                            eprintln!("failed to save '{}': {error}", document.path.display());
                            if is_external_change(&error) {
                                editor.report_external_change_conflict(document.path.clone(), error.to_string(), cx);
                            } else {
                                editor.report_workspace_file_error(error.to_string(), cx);
                                let title = cx.global::<I18nManager>().strings().save_failed_title.clone();
                                editor.show_message_modal(title, error.to_string(), cx);
                            }
                            failed_save = true;
                            continue;
                        }
                        if !editor.record_saved_history(&document.path, &payload, crate::config::file_history::HistorySource::Exit, cx) {
                            failed_save = true;
                        }
                        if let Err(error) =
                            crate::config::remove_recovery_snapshot(document.recovery_id)
                        {
                            eprintln!("failed to remove saved recovery snapshot: {error}");
                        }
                        saved_documents.push(document);
                    }
                    editor.propagate_split_saves(&saved_documents, cx);
                    let active_document_saved =
                        editor.mark_workspace_documents_saved(&saved_documents);
                    if active_document_saved {
                        editor.document_dirty = false;
                        editor.pending_window_edited = false;
                        editor.pending_window_unedited = true;
                        editor.pending_window_title_refresh = true;
                        editor.sync_current_document_content();
                    }
                    let has_unsaved_documents =
                        editor.document_dirty || editor.has_dirty_workspace_documents() || editor.has_dirty_split_documents(cx);
                    if failed_save || has_unsaved_documents {
                        editor.pending_close_after_save = false;
                        editor.show_unsaved_changes_dialog = true;
                        cx.notify();
                        return false;
                    }
                    editor.pending_close_after_save = false;
                    editor.show_unsaved_changes_dialog = false;
                    true
                });
            let update_succeeded = update_outcome.is_ok();
            let close_window = update_outcome.unwrap_or(false);
            // 实体没了才走这里：窗口活着时错误已在应用内报过，重复弹只会把带
            // 解除入口的框顶掉。错误不许静默消失。
            if !update_succeeded
                && let Some((external_change, detail)) = failure_detail
            {
                if external_change {
                    Self::show_external_change_error(window_handle, detail, cx);
                } else {
                    Self::show_workspace_save_error(window_handle, detail, cx);
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
            if let Some(path) = self.active_document_path().filter(|path| super::workspace::untitled_document_id(path).is_some()) {
                if let Some(name) = path.file_name().filter(|name| crate::file_url::is_markdown_document(Path::new(name))) {
                    return (self.workspace_root_path().map(Path::to_path_buf).unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))), Some(name.to_string_lossy().into_owned()));
                }
            }
            (
                std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
                Some("untitled.md".to_string()),
            )
        }
    }

    pub(crate) fn record_saved_history(
        &mut self,
        path: &Path,
        bytes: &[u8],
        source: crate::config::file_history::HistorySource,
        cx: &mut Context<Self>,
    ) -> bool {
        match crate::config::file_history::record_snapshot_at(
            path, bytes, source, crate::config::EditorSettings::file_history(cx), std::time::SystemTime::now(),
        ) {
            Ok(()) => true,
            Err(error) => {
                eprintln!("记录文件历史失败 '{}': {error}", path.display());
                let title = cx.global::<I18nManager>().strings().menu_file_history.clone();
                self.show_message_modal(title, format!("{}: {error}", path.display()), cx);
                false
            }
        }
    }

    pub(super) fn apply_successful_save(
        &mut self,
        path: PathBuf,
        saved_text: String,
        saved_bytes: Vec<u8>,
        cx: &mut Context<Self>,
    ) -> bool {
        let source = if self.pending_close_after_save || !self.pending_tab_close_after_save.is_empty() {
            crate::config::file_history::HistorySource::Exit
        } else {
            crate::config::file_history::HistorySource::Manual
        };
        self.document_revision = self.document_revision.wrapping_add(1);
        // 版本号取实际写入的文本，不能由完成时的另一份状态推算。
        let saved_markdown = saved_text;
        self.file_version = Some(file_content_version(&saved_markdown));
        // 标签里的版本号也要跟上（自动保存按它校验磁盘，见
        // mark_workspace_document_saved）。
        if let Some(file_version) = self.file_version {
            self.mark_workspace_document_saved(&path, file_version, &saved_markdown);
        }
        self.file_path = Some(crate::file_url::strip_verbatim_prefix(&path));
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
        // 保存落下之后才知道哪些本次会话新建的图片已经没人引用：撤销过的插入
        // 到这里一并收走，重做仍可引用它们（保存前不动文件）。
        self.cleanup_unreferenced_session_images();
        cx.notify();
        self.record_saved_history(&path, &saved_bytes, source, cx)
    }

    /// 保存到已经存在的路径。返回 `true` 表示已受理（后台读盘与写临时文件正在
    /// 进行，完成后回主线程提交），`false` 表示当场拒绝（组合文本未落定）。
    ///
    /// 读盘校验、baseline 与临时文件的 fsync 都挪到后台 executor：多 MB 文档
    /// 在 UTF-16/GB18030 解码与实时扫描叠加时，这条链在主线程上能卡数百毫秒。
    /// 主线程只留修订号校对与 rename（`commit_checked` 的最后一次校验与提交）。
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
        if self.file_version.is_none()
            && let Some(opened_text) = self.pending_file_version_text.take()
        {
            self.file_version = Some(file_content_version_normalized(&opened_text));
        }
        let bytes = self.document_bytes_for_save();
        let saved_text = self.document_text_for_save();
        let identity = self.document_save_identity();
        let expected_version = self.file_version;
        let path = path.to_path_buf();
        let then = self.pending_save_then.take();
        let window_handle = window.window_handle();
        let editor = cx.entity().downgrade();
        let verify_reads = self.autosave_verify_reads.clone();

        cx.spawn(async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let staged = cx
                .background_executor()
                .spawn({
                    let path = path.clone();
                    let bytes = bytes.clone();
                    async move {
                        let baseline = match expected_version {
                            Some(version) => {
                                verify_reads
                                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                                Some(verify_file_version(&path, version)?)
                            }
                            None => None,
                        };
                        let staged = StagedDocumentWrite::prepare_with_baseline(
                            &path,
                            &bytes,
                            baseline.as_deref(),
                        )
                        .with_context(|| format!("failed to stage '{}'", path.display()))?;
                        Ok::<_, anyhow::Error>(staged)
                    }
                })
                .await;

            let outcome = editor.update(cx, {
                let path = path.clone();
                let then = then.clone();
                move |editor, cx| {
                    let staged = match staged {
                        Ok(staged) => staged,
                        Err(error) => {
                            editor.report_manual_save_failure(&path, &error, cx);
                            editor.abort_pending_save_then(then, cx);
                            return false;
                        }
                    };
                    #[cfg(test)]
                    if let Some(hook) = editor.save_commit_hook.take() {
                        hook(editor, cx);
                    }
                    if identity != editor.document_save_identity() {
                        let error = anyhow::anyhow!("文档在准备保存期间已改变");
                        editor.report_manual_save_failure(&path, &error, cx);
                        editor.abort_pending_save_then(then, cx);
                        return false;
                    }
                    let result = match expected_version {
                        Some(version) => staged.commit_checked(&path, version),
                        None => staged.commit().map_err(anyhow::Error::from),
                    };
                    match result {
                        Ok(()) => {
                            let history_saved =
                                editor.apply_successful_save(path.clone(), saved_text, bytes, cx);
                            if !history_saved {
                                // 文件已写盘，只是历史没记上：按失败处理会让调用方
                                // 以为内容没落盘（关窗流程会重新弹未保存框）。
                                eprintln!(
                                    "文件已保存，但文件历史写入失败：{}",
                                    path.display()
                                );
                            }
                            true
                        }
                        Err(error) => {
                            editor.report_manual_save_failure(&path, &error, cx);
                            editor.abort_pending_save_then(then, cx);
                            false
                        }
                    }
                }
            });

            let Ok(saved) = outcome else {
                return;
            };
            if !saved {
                return;
            }
            let _ = cx.update_window(
                window_handle,
                move |_view: AnyView, window: &mut Window, cx: &mut App| {
                    if let Err(error) = editor.update(cx, |editor, cx| {
                        editor.finish_pending_save_then(then, window, cx);
                    }) {
                        eprintln!("保存完成后无法回写编辑器状态：{error}");
                    }
                },
            );
        })
        .detach();
        true
    }

    /// 手动保存失败的统一上报：外部修改走带解除入口的冲突记录，其余走模态。
    fn report_manual_save_failure(
        &mut self,
        path: &Path,
        error: &anyhow::Error,
        cx: &mut Context<Self>,
    ) {
        let detail = error.to_string();
        if is_external_change(error) {
            self.report_workspace_file_error(detail.clone(), cx);
            self.show_external_change_conflict_modal(path.to_path_buf(), cx);
        } else {
            let title = cx.global::<I18nManager>().strings().save_failed_title.clone();
            self.show_message_modal(title, detail, cx);
        }
    }

    /// 后台保存失败时，按调用方的语境撤销"保存后继续"的挂起状态。
    fn abort_pending_save_then(&mut self, then: Option<SaveThen>, cx: &mut Context<Self>) {
        match then {
            Some(SaveThen::CloseWindow) => self.abort_pending_close_after_save(cx),
            Some(SaveThen::ReplacePendingDrop) => self.abort_pending_drop_replace_after_save(cx),
            // 切根与批量关标签失败时只需留在原地：错误已经上报，状态没有变过。
            Some(SaveThen::SwitchWorkspaceRoot(_)) | Some(SaveThen::CloseWorkspaceTabs(_)) => {}
            None => {}
        }
    }

    fn finish_pending_save_then(
        &mut self,
        then: Option<SaveThen>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.set_window_edited(false);
        match then {
            Some(SaveThen::CloseWindow) => Self::close_editor_window(window),
            Some(SaveThen::SwitchWorkspaceRoot(root)) => self.apply_workspace_root(root, cx),
            Some(SaveThen::ReplacePendingDrop) => self.replace_after_successful_save(window, cx),
            Some(SaveThen::CloseWorkspaceTabs(closing)) => {
                self.resume_close_workspace_tabs_after_save(closing, window, cx)
            }
            None => {}
        }
    }

    pub(super) fn document_save_identity(&self) -> DocumentSaveIdentity {
        DocumentSaveIdentity {
            recovery_id: self.recovery_id, buffer_id: self.buffer.identity(), revision: self.buffer.revision(),
        }
    }

    pub(super) fn complete_save_request(
        &mut self,
        path: PathBuf,
        identity: DocumentSaveIdentity,
        saved_text: String,
        saved_bytes: Vec<u8>,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(identity == self.document_save_identity(),
            "文档在保存面板期间已改变，请重新保存当前内容");
        self.ensure_save_as_target_is_free(&path)?;
        self.ensure_other_groups_save_as_target_is_free(&path, cx)?;
        write_atomic(&path, &saved_bytes)?;
        anyhow::ensure!(self.apply_successful_save(path, saved_text, saved_bytes, cx), "文件已保存，但文件历史写入失败");
        Ok(())
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
        let identity = self.document_save_identity();
        let (default_dir, suggested_name) = self.save_dialog_defaults();
        let prompt = cx.prompt_for_new_path(&default_dir, suggested_name.as_deref());
        let weak_editor = cx.entity().downgrade();
        let weak_editor_for_cancel = weak_editor.clone();
        let weak_editor_for_error = weak_editor.clone();
        let weak_editor_for_write_error = weak_editor.clone();
        let weak_editor_for_close = weak_editor.clone();
        let window_handle = window.window_handle();
        let should_close_after_save = self.pending_close_after_save;
        let split_parent = self.split_parent.clone();

        cx.spawn(async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let mut path = match prompt.await {
                Ok(Ok(Some(path))) => path,
                Ok(Ok(None)) | Err(_) => {
                    if let Err(error) = weak_editor_for_cancel.update(cx, |editor, _| editor.pending_tab_close_after_save.clear()) {
                        eprintln!("取消草稿标签关闭失败：{error}");
                    }
                    if should_close_after_save {
                        let _ = weak_editor_for_cancel
                            .update(cx, |this, cx| this.abort_pending_close_after_save(cx));
                    }
                    return;
                }
                Ok(Err(err)) => {
                    if let Err(error) = weak_editor_for_error.update(cx, |editor, _| editor.pending_tab_close_after_save.clear()) {
                        eprintln!("取消草稿标签关闭失败：{error}");
                    }
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

            let result = weak_editor.update(cx, move |this, cx| {
                this.complete_save_request(path, identity, saved_text, markdown, cx)
            }).and_then(|result| result);
                if let Err(err) = result {
                    if let Err(error) = weak_editor_for_write_error.update(cx, |editor, _| editor.pending_tab_close_after_save.clear()) {
                        eprintln!("取消草稿标签关闭失败：{error}");
                    }
                    if should_close_after_save
                        && let Err(error) = weak_editor_for_write_error
                            .update(cx, |this, cx| this.abort_pending_close_after_save(cx))
                    {
                        eprintln!("无法取消保存后的关闭请求：{error}");
                    }
                    let detail = err.to_string();
                    // 另存为撞上的「目标已被外部改动 / 读不到」与手动保存同类，
                    // 走带「重载 / 另存为」解除入口的冲突记录，不走「保存失败」
                    // 三个失败源共用一条通道时不能只看文案。
                    let external_target = err
                        .downcast_ref::<ExternalChange>()
                        .map(|external| external.path.clone());
                    if let Some(external_path) = external_target {
                        if let Err(error) = weak_editor_for_write_error.update(cx, move |editor, cx| {
                            editor.report_external_change_conflict(external_path, detail, cx);
                        }) {
                            eprintln!("无法报告另存为的外部修改冲突：{error}");
                        }
                        return;
                    }
                    if let Err(error) = weak_editor_for_write_error.update(cx, move |this, cx| {
                        let title = cx.global::<I18nManager>().strings().save_failed_title.clone();
                        this.show_message_modal(title, detail.clone(), cx);
                    }) {
                        eprintln!("无法显示保存失败：{error}");
                    }
                    return;
                }

            let _ = cx.update_window(
                window_handle,
                move |_view: AnyView, window: &mut Window, cx: &mut App| {
                    window.set_window_edited(false);
                    match weak_editor_for_close.update(cx, |editor, cx| editor.finish_pending_tab_close(window, cx)) {
                        Ok(true) => return,
                        Ok(false) => {}
                        Err(error) => { eprintln!("保存草稿后关闭标签失败：{error}"); return; }
                    }
                    if should_close_after_save {
                        if let Some(parent) = split_parent {
                            if let Err(error) = parent.update(cx, |editor, cx| editor.save_dirty_workspace_documents_and_close(window, cx)) {
                                eprintln!("保存未命名文档后关闭工作区失败：{error}");
                            }
                            return;
                        }
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
        if self.image_preview.is_some() {
            return;
        }
        if let Some(path) = self.file_path.clone() {
            let should_close_after_save = self.pending_close_after_save;
            if should_close_after_save {
                self.pending_save_then = Some(SaveThen::CloseWindow);
            }
            if !self.save_to_existing_path(&path, window, cx) {
                if should_close_after_save {
                    self.pending_save_then = None;
                    self.abort_pending_close_after_save(cx);
                }
            }
            return;
        }

        self.save_document_via_prompt(window, cx);
    }

    pub(crate) fn save_document_as(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.image_preview.is_some() {
            return;
        }
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
    use super::{canonical_write_target, safe_code_fence, safe_code_fence_with_info, write_atomic};
    use std::path::{Path, PathBuf};

    /// rename 之后的父目录 sync 判据只准有一处（评审 P171）：文档保存提交与
    /// 配置原子写都必须走 `config::sync_parent_directory_after_rename`。
    /// 判据抄出第二份后，两处的取舍（报错只记日志、Windows 不刷）必然漂移，
    /// 报修会以「某条写路径断电后丢提交」的面貌出现。
    #[test]
    fn parent_directory_sync_after_rename_is_a_single_shared_criterion() {
        let config_source = include_str!("../config/mod.rs");
        let persistence_source = include_str!("persistence.rs");
        assert!(
            config_source.contains("pub(crate) fn sync_parent_directory_after_rename("),
            "共享的目录 sync 判据应定义在 config/mod.rs"
        );
        let write_body = config_source
            .split_once("pub(crate) fn write_config_bytes_atomic")
            .and_then(|(_, rest)| rest.split_once("\npub(crate) fn "))
            .map(|(body, _)| body)
            .expect("config/mod.rs 应有 write_config_bytes_atomic");
        assert!(
            write_body.contains("sync_parent_directory_after_rename("),
            "配置原子写在 rename 成功后也要同步父目录"
        );
        assert!(
            persistence_source.contains(concat!(
                "crate::config::sync_parent_directory_after_re",
                "name("
            )),
            "文档保存提交应走共享判据"
        );
        assert!(
            !persistence_source.contains(concat!("open(par", "ent)")),
            "目录 sync 的实现不许在 persistence.rs 留第二份"
        );
    }

    /// 共享判据本身要容错：父目录存在、父目录已消失、路径没有父级三种情形
    /// 都不许 panic、不许把 sync 失败上抛（调用方已提交成功，只能记日志）。
    #[test]
    fn parent_directory_sync_tolerates_missing_parents() {
        let root = std::env::temp_dir().join(format!("velora-dirsynctest-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("note.md");
        std::fs::write(&file, b"x").unwrap();
        crate::config::sync_parent_directory_after_rename(&file);
        crate::config::sync_parent_directory_after_rename(&root.join("missing/note.md"));
        crate::config::sync_parent_directory_after_rename(Path::new("bare-name.md"));
        let _ = std::fs::remove_dir_all(root);
    }

    /// 版本按规范化后的换行计算：同一份内容的 LF / CRLF / 裸 CR 必须同版本，
    /// 否则 AI 的读取记录会在换行风格变化后误判「内容已变化」。
    #[test]
    fn content_version_ignores_line_ending_style() {
        let lf = "a\nb\nc";
        assert_eq!(
            super::file_content_version(lf),
            super::file_content_version("a\r\nb\r\nc")
        );
        assert_eq!(
            super::file_content_version(lf),
            super::file_content_version("a\rb\rc")
        );
        assert_ne!(
            super::file_content_version(lf),
            super::file_content_version("a\nb\nc ")
        );
    }

    /// 保存侧与标签侧「同一个文件的那条路径键」必须由同一个函数给出。
    ///
    /// 保存侧当时各自调 `std::fs::canonicalize`，标签那两处走 `file_url::canonical_path`
    /// （会剥掉 Windows 的 verbatim 前缀）——同一文件于是可能得到两个不相等的键，而脏标签
    /// 匹配与磁盘版本对账正是它的第一批消费者。断的是「键里不出现 verbatim 前缀、且与
    /// 标签那把键相等」的形状，不按平台写死：路径不存在时 canonicalize 原样返回，前缀
    /// 就留在键里——macOS 上也测得出。
    #[test]
    fn review_write_target_uses_the_same_path_key_as_the_tab_registry() {
        for path in [
            PathBuf::from(r"\\?\C:\definitely\missing\notes.md"),
            PathBuf::from(r"\\?\UNC\server\share\notes.md"),
        ] {
            let target = canonical_write_target(&path);
            assert!(
                !target.to_string_lossy().starts_with(r"\\?\"),
                "{path:?} 的写盘目标带着 verbatim 前缀：{target:?}"
            );
            assert_eq!(
                target,
                crate::file_url::canonical_path(&path),
                "{path:?} 的写盘目标与标签那把键不是同一条"
            );
        }
    }

    #[test]
    #[cfg(unix)]
    fn core_repair_atomic_save_preserves_private_file_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("velora-private-save-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("private.md");
        std::fs::write(&path, "old").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        write_atomic(&path, b"new").unwrap();
        let permissions = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        std::fs::remove_dir_all(root).unwrap();
        assert_eq!(permissions, 0o600);
    }

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

    /// 保存穿过符号链接写到真正目标：rename 直接顶在链接路径上会把链接
    /// 本身换成常规文件——原目标从此不再被写、同步盘的软链工作流静默断掉
    /// （评审报告维度二建议 17）。
    #[test]
    #[cfg(unix)]
    fn atomic_write_follows_symlinks_instead_of_replacing_them() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!("velora-atomic-link-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("create root");
        let real = root.join("real.md");
        std::fs::write(&real, "real content").expect("seed real");
        let link = root.join("link.md");
        symlink(&real, &link).expect("create symlink");

        write_atomic(&link, b"through the link").expect("atomic write");

        assert!(
            std::fs::symlink_metadata(&link)
                .expect("link metadata")
                .is_symlink(),
            "符号链接必须还是符号链接（rename 不得顶掉它）"
        );
        assert_eq!(
            std::fs::read_to_string(&real).expect("read real"),
            "through the link",
            "内容要落到链接指向的真实文件"
        );
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
