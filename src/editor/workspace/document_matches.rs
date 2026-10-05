//! 当前文档的命中表：一次扫描，四个消费方共读。
//!
//! 换引擎之前，同一个查询在文档范围被算四遍，各走各的代码：结果列表
//! `search_document_source`、跳转 `find_document_match_from`、高亮
//! `tree_sync.rs::sync_document_search_highlights`、全部替换
//! `find_replace.rs::replace_all_document_matches`。四份实现靠巧合保持一致，
//! 「列表说有第 7 个命中」和「第 7 次跳转落到哪」可以来自两份独立计算——
//! 这一类不一致是搜索跳转 bug 反复出现的根因（`FIXPLAN.md` 里的 B8 replace_all
//! 虚报、`render_search.rs` 的 canonicalize 兜底都是它的症状）。
//!
//! 现在四处都读这一张表。收益有两层：跳转从「每次重扫整篇」变成一次索引取值
//! （实测 10 MiB 中文查询 61ms → 0.006ms 是引擎提前停止的部分，加上缓存之后
//! 连那 0.006ms 都不用付），以及**「四处结果不一致」这件事在类型上不再可能**。
//!
//! 失效判定只有一个入口：`TextBuffer::revision` 只在 `edit()` 里自增，
//! 所以「内容改过但版本号没变」这种情况不存在。

use super::*;
use crate::i18n::I18nManager;
use std::sync::Arc;

/// 一张命中表连同它的缓存键。键里三样东西任一变了就必须重算：缓冲区版本
/// （内容改过）、查询文本、四个搜索开关。
pub(crate) struct DocumentMatchTable {
    /// 算这份命中时缓冲区是哪一份文档。跨文档绝不复用。
    pub(crate) identity: u64,
    pub(crate) revision: u64,
    pub(crate) query: String,
    pub(crate) options: SearchOptions,
    pub(crate) hits: std::sync::Arc<Vec<SearchHit>>,
}

impl Editor {
    /// 当前查询在整篇文档里的全部命中，按出现顺序。缓存键是
    /// (缓冲区版本, 查询, 选项)，三者都没变就直接复用上一次的结果。
    /// 查询为空或正则编译失败时返回 `None`——后者由搜索面板把诊断显示出来。
    pub(crate) fn document_matches(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<Arc<Vec<SearchHit>>> {
        let query = self.workspace.search_query.trim().to_string();
        let options = self.search_options();
        if query.is_empty() {
            self.workspace.document_matches = None;
            return None;
        }
        let (identity, revision) = (self.buffer.identity(), self.buffer.revision());
        if let Some(table) = self.workspace.document_matches.as_ref() {
            if table.identity == identity
                && table.revision == revision
                && table.query == query
                && table.options == options
            {
                return Some(table.hits.clone());
            }
        }
        let compiled = super::CompiledQuery::compile(&query, options).ok()?;
        // 引擎扫的是缓冲区这一份事实源，所以行号与字节区间和用户看到的文件一致。
        // 实测把 10 MiB 拼成一个 String 只要 0.26 毫秒，不值得为它做流式读。
        let source = self.current_document_source(cx);
        let hits = Arc::new(compiled.find_all(source.as_bytes()));
        self.workspace.document_matches = Some(DocumentMatchTable {
            identity,
            revision,
            query,
            options,
            hits: hits.clone(),
        });
        Some(hits)
    }

    /// 表里第 `index` 个命中的字节区间。
    pub(crate) fn document_match_at(&self, index: usize) -> Option<Range<usize>> {
        self.workspace
            .document_matches
            .as_ref()
            .and_then(|table| table.hits.get(index).map(|hit| hit.range.clone()))
    }

    /// 按索引取下一个/上一个命中，走到两端就环绕。
    ///
    /// 用索引而不是「从某个字节位置往后找」有两个好处：一是它 O(1) 且与文档大小
    /// 无关；二是零宽命中不会再把自己卡住——按位置找的话，一个 `start == end` 的
    /// 命中在 `from == start` 时会被反复选中。
    pub(crate) fn advance_document_match_index(&mut self, reverse: bool) -> Option<usize> {
        let count = self
            .workspace
            .document_matches
            .as_ref()
            .map(|table| table.hits.len())
            .unwrap_or(0);
        if count == 0 {
            self.workspace.document_active_index = None;
            return None;
        }
        let active_range = self.workspace.document_active_range.clone();
        let current = self.workspace.document_active_index.or_else(|| {
            let active = active_range.as_ref()?;
            let table = self.workspace.document_matches.as_ref()?;
            // 选中态多半是点侧栏结果行设的，那里只知道字节区间，先按区间回查。
            if let Some(index) = table.hits.iter().position(|hit| &hit.range == active) {
                return Some(index);
            }
            // 回查不到说明这个区间不是表里的某一项（工作区命中重定位、或编辑后
            // 命中本身变了）。退回「按位置找最近的一个」，与旧实现口径一致。
            if reverse {
                (0..table.hits.len())
                    .rev()
                    .find(|index| table.hits[*index].range.start < active.start)
            } else {
                table
                    .hits
                    .iter()
                    .position(|hit| hit.range.start >= active.end)
            }
        });
        let next = match current {
            Some(index) if index < count => {
                let step = if reverse { -1isize } else { 1isize };
                (index as isize + step).rem_euclid(count as isize) as usize
            }
            _ => {
                if reverse {
                    count - 1
                } else {
                    0
                }
            }
        };
        self.workspace.document_active_index = Some(next);
        Some(next)
    }

    /// 命中表投影成侧栏的结果列表。
    pub(crate) fn project_document_hits(
        &self,
        hits: &[SearchHit],
        source: &str,
        path: &Path,
        label: &str,
        limit: usize,
    ) -> Vec<WorkspaceSearchHit> {
        project_hits_into_rows(hits, source, path, label, limit)
    }

    /// 当前文档的显示名：有文件就取文件名，没有就用 i18n 的「当前文档」。
    pub(crate) fn document_search_label(&self, cx: &App) -> (std::path::PathBuf, String) {
        let path = self.file_path.clone().unwrap_or_default();
        let label = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| {
                cx.global::<I18nManager>()
                    .strings()
                    .workspace_current_document_label
                    .clone()
            });
        (path, label)
    }
}

/// 把命中表投影成侧栏的结果列表，`limit` 是原来的 200 条上限。
///
/// 序号口径与被替换掉的手写实现逐位一致：每根「含命中的行」计一个序号，同一行的
/// 第二个命中取的是**已经自增过**的值。那是 `defect_document_scope_ordinals_are_not_unique`
/// 记下的已知缺陷，这一笔只保证不引入新的漂移；改它要连带动工作区那侧的跳转对应
/// 关系，另开一笔。
pub(crate) fn project_hits_into_rows(
    hits: &[SearchHit],
    source: &str,
    path: &Path,
    label: &str,
    limit: usize,
) -> Vec<WorkspaceSearchHit> {
    let mut out = Vec::new();
    let mut ordinal = 0usize;
    let mut previous_line: Option<u64> = None;
    for hit in hits {
        if out.len() >= limit {
            break;
        }
        let line_start = source[..hit.range.start]
            .rfind('\n')
            .map(|at| at + 1)
            .unwrap_or(0);
        let line_end = source[hit.range.end..]
            .find('\n')
            .map(|offset| hit.range.end + offset)
            .unwrap_or(source.len());
        let line_text = &source[line_start..line_end];
        let this_line = hit
            .line
            .unwrap_or_else(|| source[..line_start].matches('\n').count() as u64 + 1);
        let assigned = if Some(this_line) == previous_line {
            ordinal
        } else {
            let value = ordinal;
            ordinal += 1;
            value
        };
        previous_line = Some(this_line);
        out.push(WorkspaceSearchHit {
            path: path.to_path_buf(),
            label: label.to_string(),
            line: Some(this_line as usize),
            match_range: Some((hit.range.start - line_start)..(hit.range.end - line_start)),
            source_range: Some(hit.range.clone()),
            match_ordinal: Some(assigned),
            preview: line_text.trim().chars().take(140).collect(),
        });
    }
    out
}
