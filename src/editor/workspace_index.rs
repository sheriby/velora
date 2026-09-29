//! 工作区链接/标签索引：反向链接面板、`[[` 自动补全、标签聚合面板共用的
//! 数据源。
//!
//! 性能约定（用户要求：热路径零新增开销）：
//! - 提取是纯函数、跑在后台执行器；全量扫描只在工作区根切换时发生，
//!   之后靠 notify watcher 的单文件增量重扫维持（自己的保存也会产生
//!   Modify 事件，因此保存后索引自动跟上）。
//! - 当前文档未保存的编辑不进索引，由查询时用编辑器内存文本做「活动
//!   文档覆盖层」补齐（见 `Editor::workspace_backlinks_for_active`），
//!   快照按 `document_revision` 缓存，文档不动时零重算。
//! - 渲染帧不做文件系统访问。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use gpui::{AppContext, Task};

use super::workspace::is_markdown_document;
use super::Editor;

/// 单个 Markdown 文件提取出的外链与标签。
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct FileLinkEntry {
    /// `[[目标]]` 的目标名（与 C3 的 `wikilink_target` 同语义：trim，不拆别名）。
    pub(crate) wikilinks: Vec<String>,
    /// `#tag`（含 `#` 前缀，与 C4 的 `tag_query` 同格式）。
    pub(crate) tags: Vec<String>,
}

/// 按工作区维护的链接/标签索引。查询一律与「文件树的存活文件列表」求
/// 交集，被删除文件的陈旧条目在查询侧自然消失，不需要监听 Remove 事件。
#[derive(Default)]
pub(crate) struct WorkspaceLinkIndex {
    /// 已建索引的工作区根：换根后第一次树扫描落地时全量重建。
    indexed_root: Option<PathBuf>,
    entries: HashMap<PathBuf, Arc<FileLinkEntry>>,
    scan_generation: u64,
    /// 全量扫描任务：新任务覆盖旧字段即取消旧扫描（树扫描同模式）。
    full_scan_task: Option<Task<()>>,
    /// 单文件重扫的挂起集合：同一路径飞行中不重复调度。
    pending_rescan: HashSet<PathBuf>,
    /// 活动文档覆盖层缓存：(提取时的 document_revision, 条目)。
    active_overlay: Option<(u64, Arc<FileLinkEntry>)>,
}

/// 从 Markdown 源码提取 wikilink 与标签。跳过围栏代码块与行内代码段，
/// 避免把示例代码里的 `[[...]]`/`#tag` 当真。
pub(crate) fn extract_links_and_tags(source: &str) -> FileLinkEntry {
    let mut wikilinks = Vec::new();
    let mut tags = Vec::new();
    let mut in_fence = false;
    let mut fence_marker = b'`';

    for line in source.lines() {
        let trimmed_start = line.trim_start();
        // 围栏开关：``` 或 ~~~ 开头（至少 3 个）。开启时记录标记，配对关闭。
        let marker = trimmed_start.as_bytes().first().copied();
        if marker == Some(b'`') || marker == Some(b'~') {
            let run = trimmed_start
                .bytes()
                .take_while(|&b| b == marker.unwrap())
                .count();
            if run >= 3 {
                if !in_fence {
                    in_fence = true;
                    fence_marker = marker.unwrap();
                } else if marker == Some(fence_marker) {
                    in_fence = false;
                }
                continue;
            }
        }
        if in_fence {
            continue;
        }

        // 行内代码段跳过：逐段扫描时把 `...` 之间的内容剪掉。
        let mut rest = line;
        let mut code_delimiter: Option<usize> = None;
        let mut scan_from = 0usize;
        loop {
            let slice = &rest[scan_from.min(rest.len())..];
            let Some(backtick) = slice.find('`') else {
                if code_delimiter.is_none() {
                    scan_line_segment(slice, &mut wikilinks, &mut tags);
                }
                break;
            };
            let absolute = scan_from + backtick;
            match code_delimiter {
                None => {
                    if absolute > scan_from {
                        scan_line_segment(
                            &rest[scan_from..absolute],
                            &mut wikilinks,
                            &mut tags,
                        );
                    }
                    code_delimiter = Some(absolute);
                }
                Some(open) => {
                    // 同一处的开闭由长度决定；简化为单反引号配对（与主流
                    // 行内代码一致），遇到即闭合。
                    let _ = open;
                    code_delimiter = None;
                }
            }
            scan_from = absolute + 1;
            if scan_from >= rest.len() {
                break;
            }
        }
    }

    FileLinkEntry { wikilinks, tags }
}

/// 在一段非代码文本里找 `[[目标]]` 与 `#tag`。
fn scan_line_segment(segment: &str, wikilinks: &mut Vec<String>, tags: &mut Vec<String>) {
    let bytes = segment.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        match bytes[index] {
            b'#' => {
                let rest = &segment[index + 1..];
                let end = rest
                    .find(|ch: char| !(ch.is_alphanumeric() || ch == '_' || ch == '-'))
                    .map(|offset| index + 1 + offset)
                    .unwrap_or(segment.len());
                if end > index + 1 {
                    tags.push(segment[index..end].to_string());
                    index = end;
                } else {
                    index += 1;
                }
            }
            b'[' if index + 1 < bytes.len() && bytes[index + 1] == b'[' => {
                match segment[index + 2..].find("]]") {
                    Some(close) => {
                        let inner = segment[index + 2..index + 2 + close].trim();
                        if !inner.is_empty() {
                            wikilinks.push(inner.to_string());
                        }
                        index += 2 + close + 2;
                    }
                    None => index += 2,
                }
            }
            _ => index += 1,
        }
    }
}

impl WorkspaceLinkIndex {
    /// 全量重建：在工作区根切换后调用。读文件与提取都在后台执行器上，
    /// 按代数丢弃过期结果。
    pub(crate) fn rebuild_all(&mut self, files: Vec<PathBuf>, cx: &mut gpui::Context<Editor>) {
        self.scan_generation = self.scan_generation.wrapping_add(1);
        let generation = self.scan_generation;
        self.entries.clear();
        let read_files = std::sync::Arc::new(files);
        let read = cx.background_spawn(async move {
            let mut out = HashMap::new();
            for path in read_files.iter() {
                if let Some(entry) = read_file_entry(path) {
                    out.insert(path.clone(), Arc::new(entry));
                }
            }
            out
        });
        self.full_scan_task = Some(cx.spawn(async move |this, cx| {
            let extracted = read.await;
            let _ = this.update(cx, |editor, cx| {
                let index = &mut editor.workspace_link_index;
                if index.scan_generation != generation {
                    return;
                }
                index.entries = extracted;
                cx.notify();
            });
        }));
    }

    /// 单文件增量重扫（watcher 事件驱动）。300ms 防抖合并连续事件，
    /// 读取发生在延迟之后所以拿到的总是最新内容；同一路径飞行中不重复
    /// 调度。非 Markdown 文件直接忽略。
    pub(crate) fn schedule_rescan(&mut self, path: PathBuf, cx: &mut gpui::Context<Editor>) {
        if !is_markdown_document(&path) {
            return;
        }
        if !self.pending_rescan.insert(path.clone()) {
            return;
        }
        let generation = self.scan_generation;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(300))
                .await;
            let entry = read_file_entry(&path).map(Arc::new);
            let _ = this.update(cx, |editor, cx| {
                let index = &mut editor.workspace_link_index;
                index.pending_rescan.remove(&path);
                if index.scan_generation != generation {
                    return;
                }
                match entry {
                    Some(entry) => {
                        index.entries.insert(path, entry);
                    }
                    None => {
                        index.entries.remove(&path);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 查询当前文档的反向链接：索引条目 ∩ 存活文件列表，再叠加活动
    /// 文档的内存文本覆盖层（未保存的编辑即时生效）。
    pub(crate) fn backlinks_to(
        &mut self,
        target: &Path,
        live_files: &[PathBuf],
        active_document: Option<(PathBuf, u64, FileLinkEntry)>,
    ) -> Vec<PathBuf> {
        let active_path = self.sync_active_overlay(active_document);
        let live: HashSet<&PathBuf> = live_files.iter().collect();
        let needles = target_identifiers(target);
        let mut results: Vec<PathBuf> = self
            .entries
            .iter()
            .filter(|(path, entry)| {
                path.as_path() != target
                    && live.contains(*path)
                    && entry_has_link_to(entry, &needles)
            })
            .map(|(path, _)| path.clone())
            .collect();
        if let Some((path, entry)) = &active_path
            && path.as_path() != target
            && live.contains(path)
            && entry_has_link_to(entry, &needles)
            && !results.contains(path)
        {
            results.push(path.clone());
        }
        results.sort();
        results
    }

    /// 标签聚合计数：(标签, 引用文件数)，按计数降序、同名按标签升序。
    /// 活动文档覆盖层与反链同规则。
    pub(crate) fn tag_counts(
        &mut self,
        live_files: &[PathBuf],
        active_document: Option<(PathBuf, u64, FileLinkEntry)>,
    ) -> Vec<(String, usize)> {
        let active = self.sync_active_overlay(active_document);
        let live: HashSet<&PathBuf> = live_files.iter().collect();
        let mut counts: HashMap<String, usize> = HashMap::new();
        {
            let mut bump = |entry: &FileLinkEntry| {
                for tag in entry.tags.iter().cloned().collect::<HashSet<_>>() {
                    *counts.entry(tag).or_insert(0) += 1;
                }
            };
            for (path, entry) in &self.entries {
                if live.contains(path) {
                    bump(entry);
                }
            }
            if let Some((_, entry)) = &active {
                bump(entry);
            }
        }
        let mut results: Vec<(String, usize)> = counts.into_iter().collect();
        results.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        results
    }

    /// 同步活动文档覆盖层：按 document_revision 缓存提取结果，文档不变
    /// 时零重算。返回 (活动文档路径, 覆盖层条目)。
    fn sync_active_overlay(
        &mut self,
        active_document: Option<(PathBuf, u64, FileLinkEntry)>,
    ) -> Option<(PathBuf, Arc<FileLinkEntry>)> {
        match active_document {
            Some((path, revision, entry)) => {
                if self.active_overlay.as_ref().map(|(cached, _)| *cached) != Some(revision) {
                    self.active_overlay = Some((revision, Arc::new(entry.clone())));
                }
                self.active_overlay
                    .as_ref()
                    .map(|(_, entry)| (path, entry.clone()))
            }
            None => {
                self.active_overlay = None;
                None
            }
        }
    }

    /// 含某标签的存活文件（标签面板点击 → 打开搜索前的列表容量提示用；
    /// 搜索本身走 C4 的 `open_tag_search`）。
    pub(crate) fn files_with_tag(&self, tag: &str, live_files: &[PathBuf]) -> Vec<PathBuf> {
        let live: HashSet<&PathBuf> = live_files.iter().collect();
        self.entries
            .iter()
            .filter(|(path, entry)| live.contains(path) && entry.tags.iter().any(|t| t == tag))
            .map(|(path, _)| path.clone())
            .collect()
    }

    pub(crate) fn tracked_file_count(&self) -> usize {
        self.entries.len()
    }

    /// 树扫描落地后调用：根变了（或尚未建过）就全量重建；同根则跳过，
    /// 之后由 watcher 增量维持。
    pub(crate) fn ensure_built_for_root(
        &mut self,
        root: &Path,
        files: Vec<PathBuf>,
        cx: &mut gpui::Context<Editor>,
    ) {
        if self.indexed_root.as_deref() == Some(root) {
            return;
        }
        self.indexed_root = Some(root.to_path_buf());
        self.rebuild_all(files, cx);
    }

}

/// 目标文件可被 `[[...]]` 指到的名字集合：stem 与完整文件名（小写），
/// 与 `open_wikilink` 的匹配语义一致。
fn target_identifiers(target: &Path) -> HashSet<String> {
    let mut needles = HashSet::new();
    if let Some(stem) = target.file_stem() {
        needles.insert(stem.to_string_lossy().to_lowercase());
    }
    if let Some(name) = target.file_name() {
        needles.insert(name.to_string_lossy().to_lowercase());
    }
    needles
}

fn entry_has_link_to(entry: &FileLinkEntry, needles: &HashSet<String>) -> bool {
    entry
        .wikilinks
        .iter()
        .any(|link| needles.contains(&link.to_lowercase()))
}

fn read_file_entry(path: &Path) -> Option<FileLinkEntry> {
    let source = std::fs::read_to_string(path).ok()?;
    Some(extract_links_and_tags(&source))
}

/// 侧栏反链/标签面板的快照：按 document_revision 失效，重算间隔不短于
/// `PANEL_RECOMPUTE_INTERVAL`（打字路径不做全量序列化，面板内容最多
/// 滞后半秒）。
#[derive(Default)]
pub(crate) struct LinkPanelState {
    revision: u64,
    computed_at: Option<std::time::Instant>,
    pub(crate) backlinks: Vec<PathBuf>,
    pub(crate) tags: Vec<(String, usize)>,
    pub(crate) indexed_files: usize,
}

const PANEL_RECOMPUTE_INTERVAL: Duration = Duration::from_millis(500);

impl Editor {
    /// 面板渲染前调用：revision 变了且距上次计算超过间隔才重算。
    pub(crate) fn refresh_link_panels(&mut self, cx: &mut gpui::Context<Self>) {
        let revision = self.document_revision;
        if self.link_panels.revision == revision {
            return;
        }
        if self
            .link_panels
            .computed_at
            .is_some_and(|at| at.elapsed() < PANEL_RECOMPUTE_INTERVAL)
        {
            return;
        }
        let active_document = self.file_path.clone().map(|path| {
            let entry =
                super::workspace_index::extract_links_and_tags(&self.current_document_source(cx));
            (path, revision, entry)
        });
        let live = self.workspace_text_files();
        let backlinks = match &active_document {
            Some((path, rev, entry)) => self.workspace_link_index.backlinks_to(
                path,
                &live,
                Some((path.to_path_buf(), *rev, entry.clone())),
            ),
            None => Vec::new(),
        };
        let tags = self.workspace_link_index.tag_counts(&live, active_document);
        self.link_panels = LinkPanelState {
            revision,
            computed_at: Some(std::time::Instant::now()),
            backlinks,
            tags,
            indexed_files: self.workspace_link_index.tracked_file_count(),
        };
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_wikilinks_tags_and_skips_code() {
        let source = "\
# 笔记 #top\n\n链接到 [[另一个笔记]] 和 [[带空格 的目标]]，还有 [[\n\n```rust\nlet s = \"[[不是链接]]\"; // #nottag\n```\n\n行内代码 `[[nope]] #nope` 之后的 [[真链接]] 和 #tag-1、#tag_2。\n";
        let entry = extract_links_and_tags(source);
        assert_eq!(
            entry.wikilinks,
            vec!["另一个笔记", "带空格 的目标", "真链接"],
            "围栏与行内代码里的 [[..]] 不应计入"
        );
        assert_eq!(
            entry.tags,
            vec!["#top", "#tag-1", "#tag_2"],
            "标签格式与 C4 tag_query 一致，代码里的 #nottag 不计入"
        );
    }

    #[test]
    fn fence_toggles_require_matching_marker() {
        let source = "~~~\n[[nope]]\n```\n[[still-nope]]\n~~~\n[[yes]]";
        let entry = extract_links_and_tags(source);
        assert_eq!(entry.wikilinks, vec!["yes"]);
    }

    #[test]
    fn backlinks_match_stem_or_filename_case_insensitive() {
        let mut index = WorkspaceLinkIndex::default();
        index.entries.insert(
            PathBuf::from("/ws/notes/a.md"),
            Arc::new(FileLinkEntry {
                wikilinks: vec!["B".into()],
                tags: vec![],
            }),
        );
        index.entries.insert(
            PathBuf::from("/ws/notes/b.md"),
            Arc::new(FileLinkEntry {
                wikilinks: vec!["a.md".into()],
                tags: vec![],
            }),
        );
        index.entries.insert(
            PathBuf::from("/ws/notes/c.md"),
            Arc::new(FileLinkEntry {
                wikilinks: vec!["无关于目标".into()],
                tags: vec![],
            }),
        );
        index.entries.insert(
            PathBuf::from("/ws/notes/deleted.md"),
            Arc::new(FileLinkEntry {
                wikilinks: vec!["b".into()],
                tags: vec![],
            }),
        );
        let target = PathBuf::from("/ws/notes/b.md");
        let live = vec![
            PathBuf::from("/ws/notes/a.md"),
            PathBuf::from("/ws/notes/b.md"),
            PathBuf::from("/ws/notes/c.md"),
        ];
        let backlinks = index.backlinks_to(&target, &live, None);
        // a 指到 stem「b」；b 自指被排除；c 无关；deleted 已不在文件树。
        assert_eq!(backlinks, vec![PathBuf::from("/ws/notes/a.md")]);
    }

    #[test]
    fn active_document_overlay_supplies_unsaved_links_without_duplicates() {
        let mut index = WorkspaceLinkIndex::default();
        let target = PathBuf::from("/ws/notes/b.md");
        let live = vec![
            PathBuf::from("/ws/notes/a.md"),
            PathBuf::from("/ws/notes/b.md"),
        ];
        // 磁盘上的活动文档还没有链接。
        index.entries.insert(
            PathBuf::from("/ws/notes/a.md"),
            Arc::new(FileLinkEntry::default()),
        );
        // 未保存的编辑加了 [[b]]。
        let overlay_entry = extract_links_and_tags("看 [[b]] 和 [[b]]");
        let backlinks = index.backlinks_to(
            &target,
            &live,
            Some((
                PathBuf::from("/ws/notes/a.md"),
                7,
                overlay_entry,
            )),
        );
        assert_eq!(backlinks, vec![PathBuf::from("/ws/notes/a.md")]);

        // revision 变了覆盖层才重算：再查一次走缓存路径也不重复。
        let again = index.backlinks_to(&target, &live, None);
        assert!(again.is_empty(), "无覆盖层时磁盘上没有反链");
    }

    #[test]
    fn tag_counts_dedupe_within_file_and_rank_by_count() {
        let mut index = WorkspaceLinkIndex::default();
        index.entries.insert(
            PathBuf::from("/ws/a.md"),
            Arc::new(FileLinkEntry {
                wikilinks: vec![],
                tags: vec!["#rust".into(), "#rust".into(), "#gpui".into()],
            }),
        );
        index.entries.insert(
            PathBuf::from("/ws/b.md"),
            Arc::new(FileLinkEntry {
                wikilinks: vec![],
                tags: vec!["#rust".into()],
            }),
        );
        let live = vec![PathBuf::from("/ws/a.md"), PathBuf::from("/ws/b.md")];
        let counts = index.tag_counts(&live, None);
        assert_eq!(
            counts,
            vec![("#rust".to_string(), 2), ("#gpui".to_string(), 1)],
            "同文件内重复标签只计一次，按引用文件数降序"
        );
    }
}
