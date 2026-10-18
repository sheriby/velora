//! 后台工具只操作文档快照与提议，正式缓冲区的写入留给前台审阅。

use super::{ChangeSet, DocumentSnapshot};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

#[derive(Clone, Default)]
pub(crate) struct SnapshotBridge {
    pub callback:
        Option<std::sync::Arc<dyn Fn(&str) -> Result<Option<DocumentSnapshot>> + Send + Sync>>,
}

impl std::fmt::Debug for SnapshotBridge {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SnapshotBridge")
            .field("connected", &self.callback.is_some())
            .finish()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct SourceRecord {
    pub path: String,
    pub line: usize,
    pub range: std::ops::Range<usize>,
    pub version: u64,
    pub excerpt: String,
}

impl SourceRecord {
    pub fn locate(&self, current: &str) -> Result<std::ops::Range<usize>> {
        if crate::editor::persistence::file_content_version(current) == self.version
            && current.get(self.range.clone()).is_some()
        {
            return Ok(self.range.clone());
        }
        if self.excerpt.is_empty() {
            bail!("引用位置已变化");
        }
        let mut matches = current.match_indices(&self.excerpt);
        let Some((offset, _)) = matches.next() else {
            bail!("引用内容已变化，无法确定原位置");
        };
        if matches.next().is_some() {
            bail!("引用内容已变化，存在多个可能的位置");
        }
        Ok(offset..offset + self.excerpt.len())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct CursorTarget {
    pub document_id: String,
    pub offset: usize,
    pub version: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct SelectionScope {
    pub document_id: String,
    pub range: std::ops::Range<usize>,
    pub version: u64,
}

impl SelectionScope {
    pub fn allowed_in(&self, original: &str, pending: &str) -> Result<std::ops::Range<usize>> {
        if crate::editor::persistence::file_content_version(original) != self.version
            || original.get(self.range.clone()).is_none()
        {
            bail!("选区所在文档已变化，请重新选中后执行");
        }
        let prefix = original.get(..self.range.start).context("选区起点无效")?;
        let suffix = original.get(self.range.end..).context("选区终点无效")?;
        if pending.len() < prefix.len().saturating_add(suffix.len())
            || !pending.starts_with(prefix)
            || !pending.ends_with(suffix)
        {
            bail!("修改超出允许的选区");
        }
        Ok(self.range.start..pending.len().saturating_sub(suffix.len()))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct AiWorkspace {
    #[serde(default)]
    pub read_only: bool,
    #[serde(default)]
    pub selection_scope: Option<SelectionScope>,
    #[serde(default)]
    pub cursor_target: Option<CursorTarget>,
    pub root: Option<PathBuf>,
    pub documents: BTreeMap<String, DocumentSnapshot>,
    pub changes: ChangeSet,
    #[serde(default)]
    pub sources: Vec<String>,
    #[serde(default)]
    pub source_records: Vec<SourceRecord>,
    #[serde(default)]
    pending_reads: std::collections::BTreeSet<String>,
    #[serde(default)]
    pub excluded_paths: Vec<String>,
    #[serde(default)]
    pub scope: Vec<String>,
    #[serde(default)]
    pub new_documents: std::collections::BTreeSet<String>,
    #[serde(skip)]
    pub skills: super::skills::SkillCatalog,
    #[serde(skip)]
    pub refresh: SnapshotBridge,
    #[serde(default)]
    pub live_documents: std::collections::BTreeSet<String>,
    #[serde(skip)]
    files: Option<Vec<String>>,
    #[serde(skip)]
    stamps: BTreeMap<String, (Option<std::time::SystemTime>, u64)>,
    /// 运行时注入的取消令牌：超时/停止后正在执行的同步工具（全库搜索、
    /// 大文件读盘）靠它在循环边界提前退出，否则 Finished 会被拖到工具跑完。
    #[serde(skip)]
    pub cancel: Option<tokio_util::sync::CancellationToken>,
}

impl AiWorkspace {
    pub fn is_excluded(&self, id: &str) -> bool {
        self.excluded_paths
            .iter()
            .any(|excluded| path_matches(id, excluded))
    }

    pub fn tool_allowed(&self, name: &str) -> bool {
        (!self.read_only
            || !matches!(
                name,
                "propose_edit" | "propose_document" | "propose_insert_at_cursor" | "reset_proposal"
            ))
            && (self.selection_scope.is_none()
                || !matches!(name, "propose_document" | "propose_insert_at_cursor"))
    }

    pub fn discard_proposals(&mut self) {
        for id in std::mem::take(&mut self.new_documents) {
            self.documents.remove(&id);
            self.source_records.retain(|record| record.path != id);
            if let Some(files) = self.files.as_mut() {
                files.retain(|path| path != &id);
            }
        }
        self.changes = ChangeSet::default();
        self.pending_reads.clear();
    }

    pub fn set_scope(&mut self, references: &[String], primary: &str) -> Result<()> {
        let mut scope = Vec::new();
        for reference in references {
            if self.live_documents.contains(reference) || self.new_documents.contains(reference) {
                scope.push(reference.clone());
                continue;
            }
            let path = Path::new(reference);
            validate_relative(reference)?;
            let root = self
                .root
                .as_ref()
                .context("请先打开工作区，再指定其他资料")?;
            let resolved = root
                .join(path)
                .canonicalize()
                .with_context(|| format!("找不到指定资料：{reference}"))?;
            if !resolved.starts_with(root)
                || (!resolved.is_dir() && !crate::file_url::is_markdown_document(path))
            {
                bail!("指定资料不在工作区内或不是 Markdown：{reference}");
            }
            if !scope.contains(reference) {
                scope.push(reference.clone());
            }
        }
        if !scope.is_empty() && !scope.iter().any(|id| id == primary) {
            scope.push(primary.to_owned());
        }
        self.scope = scope;
        Ok(())
    }
    pub fn proposal_path(&self, id: &str) -> Result<PathBuf> {
        self.resolve(id, !self.new_documents.contains(id))
    }
    pub fn new(root: Option<PathBuf>, documents: Vec<DocumentSnapshot>) -> Result<Self> {
        let live_documents = documents
            .iter()
            .map(|document| document.id.clone())
            .collect();
        Ok(Self {
            read_only: false,
            selection_scope: None,
            cursor_target: None,
            root: root.map(std::fs::canonicalize).transpose()?,
            documents: documents
                .into_iter()
                .map(|document| (document.id.clone(), document))
                .collect(),
            changes: ChangeSet::default(),
            sources: Vec::new(),
            source_records: Vec::new(),
            pending_reads: Default::default(),
            excluded_paths: Vec::new(),
            scope: Vec::new(),
            new_documents: Default::default(),
            skills: Default::default(),
            files: None,
            stamps: BTreeMap::new(),
            cancel: None,
            refresh: SnapshotBridge::default(),
            live_documents,
        })
    }

    fn cancelled(&self) -> bool {
        self.cancel
            .as_ref()
            .is_some_and(|cancel| cancel.is_cancelled())
    }

    pub fn call(&mut self, name: &str, arguments: &Value) -> Result<Value> {
        if self.tool_allowed(name)
            && matches!(name, "search_documents" | "list_documents" | "read_document" | "get_outline")
            && self.cancelled()
        {
            bail!("任务已停止");
        }
        if !self.tool_allowed(name) {
            bail!("本任务不允许此写入方式：阅读任务不能生成修改，选区任务不能新建文档");
        }
        match name {
            "reset_proposal" => {
                let path = field(arguments, "path")?;
                if self
                    .selection_scope
                    .as_ref()
                    .is_some_and(|scope| scope.document_id != path)
                {
                    bail!("本任务只能重新生成选区所在文档的修改");
                }
                self.snapshot(path)?;
                self.changes.invalidate_dependents(path);
                self.changes.proposals.remove(path);
                self.changes.invalid.remove(path);
                self.changes.dependencies.remove(path);
                self.pending_reads.remove(path);
                Ok(json!({"path":path,"status":"ready_to_revise","written":false}))
            }
            "read_skill" => {
                let id = field(arguments, "skill")?;
                let resource = arguments
                    .get("resource")
                    .and_then(Value::as_str)
                    .unwrap_or("SKILL.md");
                Ok(
                    json!({"skill":id,"resource":resource,"content":self.skills.read(id, resource)?}),
                )
            }
            "list_documents" => {
                let files = self.files()?;
                let folder = arguments
                    .get("folder")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let offset = arguments.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize;
                let matching: Vec<_> = files
                    .into_iter()
                    .filter(|file| {
                        folder.is_empty()
                            || file.starts_with(&format!("{}/", folder.trim_end_matches('/')))
                    })
                    .collect();
                let selected: Vec<_> = matching.iter().skip(offset).take(100).collect();
                Ok(
                    json!({"documents": selected, "total":matching.len(), "offset":offset, "next_offset":offset.saturating_add(selected.len())}),
                )
            }
            "search_documents" => {
                let query = field(arguments, "query")?.to_lowercase();
                if query.is_empty() {
                    bail!("搜索词不能为空");
                }
                let mut matches = Vec::new();
                let mut errors = Vec::new();
                for path in self.files()? {
                    if self.cancelled() {
                        bail!("任务已停止");
                    }
                    match self.snapshot(&path) {
                        Ok(document) => {
                            let stale = self.changes.invalid.contains(&path)
                                || self.changes.proposals.get(&path).is_some_and(|proposal| {
                                    proposal.validate(&document.source).is_err()
                                });
                            let source = if stale {
                                document.source.as_ref()
                            } else {
                                self.changes.read(&document)
                            };
                            // 搜索读到的是待审阅内容时同样登记依赖：据此提出的
                            // 修改要随前置提议的重置一并失效。
                            if !stale && self.changes.proposals.contains_key(&path) {
                                self.pending_reads.insert(path.clone());
                            }
                            for (index, line) in source.lines().enumerate() {
                                if line.to_lowercase().contains(&query) {
                                    matches.push(json!({"path":path,"line":index + 1,"content": line.chars().take(500).collect::<String>()}));
                                    if matches.len() >= 50 {
                                        break;
                                    }
                                }
                            }
                        }
                        Err(error) => {
                            if errors.len() < 20 {
                                errors.push(format!("{path}: {error}"));
                            }
                        }
                    }
                    if matches.len() >= 50 {
                        break;
                    }
                }
                Ok(json!({"matches":matches,"errors":errors,"limited":matches.len() >= 50}))
            }
            "read_document" | "get_outline" => {
                let path = field(arguments, "path")?;
                let document = self.snapshot(path)?;
                if !self.sources.iter().any(|source| source == path) {
                    self.sources.push(path.to_owned());
                }
                let stale = self.changes.invalid.contains(path)
                    || self
                        .changes
                        .proposals
                        .get(path)
                        .is_some_and(|proposal| proposal.validate(&document.source).is_err());
                let source = if stale {
                    document.source.as_ref()
                } else {
                    self.changes.read(&document)
                };
                if !stale && self.changes.proposals.contains_key(path) {
                    self.pending_reads.insert(path.to_owned());
                }
                if name == "get_outline" {
                    // 大纲判据与侧栏同一份（outline_headings）：front matter 里的伪
                    // setext 标题、引用块内的标题都不进大纲，另起解析器会在这些
                    // 写法上与用户所见漂移。
                    let headings = crate::editor::workspace::document_outline(source)
                        .into_iter()
                        .map(|heading| {
                            json!({"title":heading.label,"line":heading.line + 1,"level":heading.level})
                        })
                        .collect::<Vec<_>>();
                    return Ok(json!({"path":path,"headings":headings}));
                }
                let start = arguments
                    .get("start_line")
                    .and_then(Value::as_u64)
                    .unwrap_or(1)
                    .max(1) as usize;
                let limit = arguments
                    .get("limit")
                    .and_then(Value::as_u64)
                    .unwrap_or(120)
                    .clamp(1, 300) as usize;
                let total = source.lines().count();
                let offset = arguments
                    .get("start_offset")
                    .and_then(Value::as_u64)
                    .map(|offset| offset as usize)
                    .unwrap_or_else(|| {
                        source
                            .split_inclusive('\n')
                            .take(start - 1)
                            .map(str::len)
                            .sum()
                    });
                let offset = source.floor_char_boundary(offset.min(source.len()));
                let tail = source.get(offset..).context("文档读取位置无效")?;
                let line_bytes: usize = tail.split_inclusive('\n').take(limit).map(str::len).sum();
                let end = source.floor_char_boundary(
                    offset
                        .saturating_add(line_bytes.min(32_768))
                        .min(source.len()),
                );
                let content = source.get(offset..end).context("文档读取区间无效")?;
                let actual_start = source
                    .get(..offset)
                    .unwrap_or_default()
                    .bytes()
                    .filter(|byte| *byte == b'\n')
                    .count()
                    + 1;
                let next_line =
                    actual_start + content.bytes().filter(|byte| *byte == b'\n').count();
                let record = SourceRecord {
                    path: path.to_owned(),
                    line: actual_start,
                    range: offset..end,
                    version: crate::editor::persistence::file_content_version(source),
                    excerpt: content.chars().take(512).collect(),
                };
                if !self.source_records.iter().any(|source| {
                    source.path == record.path
                        && source.range == record.range
                        && source.version == record.version
                }) {
                    if self.source_records.len() >= 256 {
                        self.source_records.remove(0);
                    }
                    self.source_records.push(record);
                }
                Ok(
                    json!({"path":path,"content":content,"start_line":actual_start,"total_lines":total,"start_offset":offset,
                    "pending":self.changes.proposals.contains_key(path) && !stale,"pending_stale":stale,"truncated":end < source.len(),
                    "next_line":next_line,"next_offset":end,"total_bytes":source.len()}),
                )
            }
            "propose_insert_at_cursor" => {
                let cursor = self.cursor_target.clone().context("请先在文档中放置光标")?;
                let document = self.snapshot(&cursor.document_id)?;
                if crate::editor::persistence::file_content_version(&document.source)
                    != cursor.version
                    || document.source.get(..cursor.offset).is_none()
                {
                    bail!("光标所在文档已变化，请重新指定插入位置");
                }
                let mut offset = cursor.offset;
                if let Some(proposal) = self.changes.proposals.get(&cursor.document_id) {
                    for edit in proposal.edits() {
                        if edit.range.start < cursor.offset && cursor.offset < edit.range.end {
                            bail!("光标位于其他待审阅修改中，请先审阅该修改");
                        }
                        if edit.range.end <= cursor.offset {
                            offset = offset
                                .saturating_sub(edit.range.len())
                                .saturating_add(edit.replacement.len());
                        }
                    }
                }
                self.changes.propose_within(
                    document,
                    "",
                    field(arguments, "content")?,
                    Some(offset..offset),
                )?;
                self.changes.invalidate_dependents(&cursor.document_id);
                self.attach_dependencies(&cursor.document_id);
                Ok(json!({"path":cursor.document_id,"status":"pending_review","written":false}))
            }
            "propose_edit" => {
                let path = field(arguments, "path")?;
                if self
                    .selection_scope
                    .as_ref()
                    .is_some_and(|scope| scope.document_id != path)
                {
                    bail!("润色和扩写只能修改选区所在的文档");
                }
                let document = self.snapshot(path)?;
                let allowed = self
                    .selection_scope
                    .as_ref()
                    .map(|scope| scope.allowed_in(&document.source, self.changes.read(&document)))
                    .transpose()?;
                self.changes.propose_within(
                    document,
                    field(arguments, "before")?,
                    field(arguments, "after")?,
                    allowed,
                )?;
                self.changes.invalidate_dependents(path);
                self.attach_dependencies(path);
                Ok(json!({"path":path,"status":"pending_review","written":false}))
            }
            "propose_document" => {
                let path = field(arguments, "path")?;
                if self.is_excluded(path) {
                    bail!("此路径已从 AI 资料中排除");
                }
                let destination = self.resolve(path, false)?;
                // symlink_metadata 连悬空链接也算存在：接受端 rename 不穿透符号
                // 链接，但会把用户自己的链接静默替换成普通文件。
                if self.documents.contains_key(path) || destination.symlink_metadata().is_ok() {
                    bail!("文档已存在，请读取后提出修改");
                }
                let content = field(arguments, "content")?;
                if content.len() > 2 * 1024 * 1024 {
                    bail!("新文档内容超出单次提议预算");
                }
                let document = DocumentSnapshot {
                    id: path.to_owned(),
                    source: String::new().into(),
                };
                self.documents.insert(path.to_owned(), document.clone());
                self.changes.propose(document, "", content)?;
                self.new_documents.insert(path.to_owned());
                self.attach_dependencies(path);
                Ok(json!({"path":path,"status":"pending_review","written":false}))
            }
            _ => bail!("不支持的文档工具：{name}"),
        }
    }

    fn attach_dependencies(&mut self, path: &str) {
        let dependencies = self
            .pending_reads
            .iter()
            .filter(|id| id.as_str() != path && self.changes.proposals.contains_key(*id))
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        self.changes
            .dependencies
            .entry(path.to_owned())
            .or_default()
            .extend(dependencies);
    }

    fn resolve(&self, id: &str, existing: bool) -> Result<PathBuf> {
        let path = Path::new(id);
        validate_relative(id)?;
        if !crate::file_url::is_markdown_document(path) {
            bail!("AI 文档工具仅支持 Markdown");
        }
        let root = self
            .root
            .as_ref()
            .context("请先打开工作区，再访问其他文档")?;
        let destination = root.join(path);
        let verified = if existing {
            destination.canonicalize()?
        } else {
            destination
                .ancestors()
                .find(|path| path.exists())
                .context("目标目录不存在")?
                .canonicalize()?
        };
        if !verified.starts_with(root) {
            bail!("目标不在当前工作区内");
        }
        if !existing && !destination.exists() && !verified.is_dir() {
            bail!("新文档的父路径不是目录");
        }
        Ok(destination)
    }

    fn snapshot(&mut self, id: &str) -> Result<DocumentSnapshot> {
        if self.is_excluded(id) {
            bail!("此文档已从 AI 资料中排除");
        }
        if !self.scope.is_empty()
            && !self.scope.iter().any(|scope| path_matches(id, scope))
            && !self.new_documents.contains(id)
        {
            bail!("此文档不在当前指定的资料范围内");
        }
        if self.live_documents.contains(id) {
            if let Some(refresh) = &self.refresh.callback {
                if let Some(document) = refresh(id)? {
                    self.documents.insert(id.to_owned(), document);
                    // live 覆盖之后，磁盘 stamp 只证明磁盘没变，不再证明缓存
                    // 就是磁盘内容；留着会让已关闭的草稿以 stamp 快速路径还魂。
                    self.stamps.remove(id);
                } else {
                    self.live_documents.remove(id);
                    if id.starts_with("untitled:") || self.new_documents.contains(id) {
                        bail!("未保存文档已关闭，请重新指定资料");
                    }
                }
            }
        }
        if self.live_documents.contains(id) || self.new_documents.contains(id) {
            if let Some(document) = self.documents.get(id) {
                return Ok(document.clone());
            }
        }
        let path = self.resolve(id, true)?;
        let metadata = path.metadata()?;
        let stamp = (metadata.modified().ok(), metadata.len());
        if self.stamps.get(id) == Some(&stamp) {
            if let Some(document) = self.documents.get(id) {
                return Ok(document.clone());
            }
        }
        if metadata.len() > 16 * 1024 * 1024 {
            bail!("文档超过单次载入预算，请先在编辑器中打开并指定选区");
        }
        if self.cancelled() {
            bail!("任务已停止");
        }
        let total: usize = self
            .documents
            .values()
            .map(|document| document.source.len())
            .sum();
        if total > 64 * 1024 * 1024 {
            bail!("本次任务已达到文档读取预算，请缩小资料范围");
        }
        let source = crate::editor::encoding::normalize_document_text(
            &crate::editor::encoding::read_document_string(&path)?,
        );
        let document = DocumentSnapshot {
            id: id.to_owned(),
            source: source.into(),
        };
        self.documents.insert(id.to_owned(), document.clone());
        self.stamps.insert(id.to_owned(), stamp);
        Ok(document)
    }

    fn files(&mut self) -> Result<Vec<String>> {
        if self.files.is_none() {
            let mut files = BTreeMap::new();
            if let Some(root) = &self.root {
                let (paths, skipped) =
                    crate::editor::workspace::collect_workspace_files_on_disk_reporting_skips(root);
                if skipped > 0 {
                    // 单个条目读不了只影响它自己；整单 bail 会让一个受保护子目录
                    // 废掉 list/search 对整个工作区的可用性（编辑器文件树同样容忍）。
                    eprintln!("AI 工作区有 {skipped} 个条目无法读取，已在名单中跳过");
                }
                for path in paths {
                    if crate::file_url::is_markdown_document(&path) {
                        if let Ok(relative) = path.strip_prefix(root) {
                            files.insert(relative.to_string_lossy().replace('\\', "/"), ());
                        }
                    }
                }
            }
            files.extend(self.documents.keys().cloned().map(|key| (key, ())));
            self.files = Some(files.into_keys().collect());
        }
        let mut files = self.files.clone().unwrap_or_default();
        for id in self.documents.keys() {
            if !files.contains(id) {
                files.push(id.clone());
            }
        }
        for id in &self.live_documents {
            if !files.contains(id) {
                files.push(id.clone());
            }
        }
        files.retain(|id| {
            !self.is_excluded(id)
                && (self.scope.is_empty()
                    || self.scope.iter().any(|scope| path_matches(id, scope))
                    || self.new_documents.contains(id))
        });
        Ok(files)
    }
}

fn field<'a>(arguments: &'a Value, name: &str) -> Result<&'a str> {
    arguments
        .get(name)
        .and_then(Value::as_str)
        .with_context(|| format!("缺少参数：{name}"))
}

fn path_matches(path: &str, prefix: &str) -> bool {
    let prefix = prefix.trim_end_matches('/');
    path == prefix
        || path
            .strip_prefix(prefix)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn validate_relative(id: &str) -> Result<()> {
    let path = Path::new(id);
    if id.contains(['\\', ':'])
        || path.as_os_str().is_empty()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        bail!("只能访问当前工作区内的相对路径");
    }
    Ok(())
}

pub(crate) fn tool_definitions() -> Vec<(&'static str, &'static str, Value)> {
    vec![
        (
            "reset_proposal",
            "仅丢弃此任务中该文档的旧提议，不改正文；当 pending_stale 为真时，先读取最新原文，再调用此工具重新生成。",
            json!({"type":"object","required":["path"],"properties":{"path":{"type":"string"}}}),
        ),
        (
            "read_skill",
            "按需读取已列出的 Skill 或其中的参考资料；不会执行脚本。",
            json!({"type":"object","required":["skill"],"properties":{"skill":{"type":"string"},"resource":{"type":"string"}}}),
        ),
        (
            "list_documents",
            "列举工作区 Markdown 文档；可按 folder、offset 分页。",
            json!({"type":"object","properties":{"folder":{"type":"string"},"offset":{"type":"integer","minimum":0}}}),
        ),
        (
            "search_documents",
            "搜索 Markdown 文档并返回来源路径、行号和片段。",
            json!({"type":"object","required":["query"],"properties":{"query":{"type":"string"}}}),
        ),
        (
            "read_document",
            "读取文档最新内容，包括本任务待审阅修改；pending_stale 表示正文变化，此时返回最新正文。可按 start_line、limit 读取，长行用 next_offset 作为 start_offset 继续，truncated 表示不完整。",
            json!({"type":"object","required":["path"],"properties":{"path":{"type":"string"},"start_line":{"type":"integer","minimum":1},"start_offset":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":300}}}),
        ),
        (
            "get_outline",
            "获取 Markdown 标题及对应行号。",
            json!({"type":"object","required":["path"],"properties":{"path":{"type":"string"}}}),
        ),
        (
            "propose_edit",
            "提出待审阅修改，不写正文。before 必须在已读取内容中唯一匹配；after 为替换内容；before 为空时追加到本次允许修改范围的末尾。",
            json!({"type":"object","required":["path","before","after"],"properties":{"path":{"type":"string"},"before":{"type":"string"},"after":{"type":"string"}}}),
        ),
        (
            "propose_insert_at_cursor",
            "在本次请求捕获的光标处提出插入内容；接受前不改正文，不能自行指定其他坐标。",
            json!({"type":"object","required":["content"],"properties":{"content":{"type":"string"}}}),
        ),
        (
            "propose_document",
            "提出新的 Markdown 文档，接受前不创建文件。",
            json!({"type":"object","required":["path","content"],"properties":{"path":{"type":"string"},"content":{"type":"string"}}}),
        ),
    ]
}

#[cfg(test)]
mod tests;
