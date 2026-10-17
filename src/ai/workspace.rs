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
pub(crate) struct AiWorkspace {
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
}

impl AiWorkspace {
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
            refresh: SnapshotBridge::default(),
            live_documents,
        })
    }

    pub fn call(&mut self, name: &str, arguments: &Value) -> Result<Value> {
        match name {
            "reset_proposal" => {
                let path = field(arguments, "path")?;
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
                    match self.snapshot(&path) {
                        Ok(document) => {
                            let source = if self.changes.invalid.contains(&path)
                                || self.changes.proposals.get(&path).is_some_and(|proposal| {
                                    proposal.validate(&document.source).is_err()
                                }) {
                                document.source.as_str()
                            } else {
                                self.changes.read(&document)
                            };
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
                    document.source.as_str()
                } else {
                    self.changes.read(&document)
                };
                if !stale && self.changes.proposals.contains_key(path) {
                    self.pending_reads.insert(path.to_owned());
                }
                if name == "get_outline" {
                    let mut headings = Vec::new();
                    let mut heading: Option<(String, usize)> = None;
                    for (event, range) in pulldown_cmark::Parser::new(source).into_offset_iter() {
                        match event {
                            pulldown_cmark::Event::Start(pulldown_cmark::Tag::Heading {
                                ..
                            }) => {
                                heading = Some((
                                    String::new(),
                                    source
                                        .get(..range.start)
                                        .unwrap_or_default()
                                        .bytes()
                                        .filter(|byte| *byte == b'\n')
                                        .count()
                                        + 1,
                                ));
                            }
                            pulldown_cmark::Event::Text(text)
                            | pulldown_cmark::Event::Code(text) => {
                                if let Some((title, _)) = heading.as_mut() {
                                    title.push_str(&text);
                                }
                            }
                            pulldown_cmark::Event::End(pulldown_cmark::TagEnd::Heading(_)) => {
                                if let Some((title, line)) = heading.take() {
                                    headings.push(json!({"title":title,"line":line}));
                                }
                            }
                            _ => {}
                        }
                    }
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
            "propose_edit" => {
                let path = field(arguments, "path")?;
                let document = self.snapshot(path)?;
                self.changes.propose(
                    document,
                    field(arguments, "before")?,
                    field(arguments, "after")?,
                )?;
                self.changes.invalidate_dependents(path);
                self.attach_dependencies(path);
                Ok(json!({"path":path,"status":"pending_review","written":false}))
            }
            "propose_document" => {
                let path = field(arguments, "path")?;
                if self
                    .excluded_paths
                    .iter()
                    .any(|excluded| path_matches(path, excluded))
                {
                    bail!("此路径已从 AI 资料中排除");
                }
                let destination = self.resolve(path, false)?;
                if self.documents.contains_key(path) || destination.exists() {
                    bail!("文档已存在，请读取后提出修改");
                }
                let content = field(arguments, "content")?;
                if content.len() > 2 * 1024 * 1024 {
                    bail!("新文档内容超出单次提议预算");
                }
                let document = DocumentSnapshot {
                    id: path.to_owned(),
                    source: String::new(),
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
        if self
            .excluded_paths
            .iter()
            .any(|excluded| path_matches(id, excluded))
        {
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
            source,
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
                    bail!("有 {skipped} 个工作区条目无法读取，请检查目录权限");
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
            !self
                .excluded_paths
                .iter()
                .any(|excluded| path_matches(id, excluded))
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
            "提出待审阅修改，不写正文。before 必须在已读取内容中唯一匹配；after 为替换内容；before 为空时追加。",
            json!({"type":"object","required":["path","before","after"],"properties":{"path":{"type":"string"},"before":{"type":"string"},"after":{"type":"string"}}}),
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
