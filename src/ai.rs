use std::collections::BTreeMap;
use std::ops::Range;
use std::sync::Arc;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

pub(crate) mod context;
pub(crate) mod instructions;
pub(crate) mod providers;
pub(crate) mod runtime;
pub(crate) mod session;
pub(crate) mod settings;
pub(crate) mod skills;
pub(crate) mod workspace;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum AiCommand {
    Ask,
    Polish,
    Expand,
    Translate,
    Summarize,
    Explain,
}

impl AiCommand {
    pub fn id(self) -> &'static str {
        match self {
            Self::Ask => "conversation",
            Self::Polish => "polish",
            Self::Expand => "expand",
            Self::Translate => "translate",
            Self::Summarize => "summarize",
            Self::Explain => "explain",
        }
    }

    pub fn is_reading_task(self) -> bool {
        matches!(self, Self::Summarize | Self::Explain | Self::Translate)
    }

    pub fn edits_selection(self) -> bool {
        matches!(self, Self::Polish | Self::Expand)
    }

    pub fn requires_selection(self) -> bool {
        matches!(
            self,
            Self::Polish | Self::Expand | Self::Translate | Self::Explain
        )
    }

    pub const ALL: [Self; 5] = [
        Self::Summarize,
        Self::Explain,
        Self::Translate,
        Self::Polish,
        Self::Expand,
    ];

    pub fn label(self, strings: &crate::i18n::I18nStrings) -> String {
        match self {
            Self::Ask => strings.ai_panel_ask.clone(),
            Self::Polish => strings.ai_action_polish.clone(),
            Self::Expand => strings.ai_action_expand.clone(),
            Self::Translate => strings.ai_action_translate.clone(),
            Self::Summarize => strings.ai_action_summarize.clone(),
            Self::Explain => strings.ai_action_explain.clone(),
        }
    }
}

/// 运行时错误码（后台线程拿不到 i18n）在界面层翻译成本地化文案；
/// 未知内容（模型/IO 的原始报错）原样返回。
pub(crate) fn error_text(error: &str, strings: &crate::i18n::I18nStrings) -> String {
    if let Some(detail) = error.strip_prefix("velora-ai:model-failed:") {
        if detail.contains("unusable arguments") {
            let evidence = detail
                .split_once("not executed:")
                .map_or(detail, |(_, rest)| rest.trim());
            return format!("{}{evidence}", strings.ai_error_tool_arguments);
        }
        return if detail.is_empty() {
            strings.ai_error_model_failed.clone()
        } else {
            format!("{}: {detail}", strings.ai_error_model_failed)
        };
    }
    match error {
        error_codes::TIMEOUT => strings.ai_error_timeout.clone(),
        error_codes::STOPPED => strings.ai_error_stopped.clone(),
        error_codes::LENGTH => strings.ai_error_length.clone(),
        error_codes::BUDGET => strings.ai_error_budget.clone(),
        _ => error.to_string(),
    }
}

/// 后台线程发出的固定错误码：不携带语言，界面层按码取词。
pub(crate) mod error_codes {
    pub const TIMEOUT: &str = "velora-ai:timeout";
    pub const STOPPED: &str = "velora-ai:stopped";
    pub const BUDGET: &str = "velora-ai:budget";
    pub const LENGTH: &str = "velora-ai:length";
    pub fn model_failed(detail: Option<String>) -> String {
        format!("velora-ai:model-failed:{}", detail.unwrap_or_default())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct DocumentSnapshot {
    pub id: String,
    /// 全文用 `Arc<str>`：会话每轮检查点与持久化都要整体克隆，逐字节拷贝
    /// 会把大工作区的成本放大到 UI 线程。
    pub source: Arc<str>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ProposedEdit {
    pub range: Range<usize>,
    pub replacement: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Proposal {
    pub base: DocumentSnapshot,
    /// 待审阅全文同样用 `Arc<str>`，理由同 `DocumentSnapshot::source`。
    pub proposed: Arc<str>,
    #[serde(skip)]
    edits_cache: std::sync::OnceLock<Vec<ProposedEdit>>,
}

impl Proposal {
    pub fn new(base: DocumentSnapshot, proposed: impl Into<Arc<str>>) -> Self {
        Self {
            base,
            proposed: proposed.into(),
            edits_cache: Default::default(),
        }
    }

    fn cached_edits(&self) -> &Vec<ProposedEdit> {
        self.edits_cache.get_or_init(|| self.compute_edits())
    }

    pub fn edits(&self) -> Vec<ProposedEdit> {
        self.cached_edits().clone()
    }

    pub fn edit_count(&self) -> usize {
        self.cached_edits().len()
    }

    fn compute_edits(&self) -> Vec<ProposedEdit> {
        let mut edits = Vec::new();
        let mut offset = 0;
        let mut pending: Option<ProposedEdit> = None;
        let diff = similar::TextDiff::configure()
            .timeout(std::time::Duration::from_millis(100))
            .diff_lines(self.base.source.as_ref(), self.proposed.as_ref());
        for change in diff.iter_all_changes() {
            match change.tag() {
                similar::ChangeTag::Equal => {
                    if let Some(edit) = pending.take() {
                        edits.push(edit);
                    }
                    offset += change.value().len();
                }
                similar::ChangeTag::Delete => {
                    let edit = pending.get_or_insert_with(|| ProposedEdit {
                        range: offset..offset,
                        replacement: String::new(),
                    });
                    offset += change.value().len();
                    edit.range.end = offset;
                }
                similar::ChangeTag::Insert => {
                    pending
                        .get_or_insert_with(|| ProposedEdit {
                            range: offset..offset,
                            replacement: String::new(),
                        })
                        .replacement
                        .push_str(change.value());
                }
            }
        }
        if let Some(edit) = pending {
            edits.push(edit);
        }
        edits
            .into_iter()
            .map(|mut edit| {
                let Some(before) = self.base.source.get(edit.range.clone()) else {
                    return edit;
                };
                let prefix = before
                    .bytes()
                    .zip(edit.replacement.bytes())
                    .take_while(|(left, right)| left == right)
                    .count();
                let prefix = before.floor_char_boundary(prefix);
                let old_tail = before.get(prefix..).unwrap_or_default();
                let new_tail = edit.replacement.get(prefix..).unwrap_or_default();
                let mut suffix = old_tail
                    .bytes()
                    .rev()
                    .zip(new_tail.bytes().rev())
                    .take_while(|(left, right)| left == right)
                    .count();
                while suffix > 0
                    && (!before.is_char_boundary(before.len() - suffix)
                        || !edit
                            .replacement
                            .is_char_boundary(edit.replacement.len() - suffix))
                {
                    suffix -= 1;
                }
                edit.range.start += prefix;
                edit.range.end -= suffix;
                edit.replacement = edit
                    .replacement
                    .get(prefix..edit.replacement.len() - suffix)
                    .unwrap_or_default()
                    .to_owned();
                edit
            })
            .collect()
    }

    pub fn validate(&self, current: &str) -> Result<()> {
        if current != self.base.source.as_ref() {
            bail!("文档已经变化，请重新审阅修改");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct ChangeSet {
    pub proposals: BTreeMap<String, Proposal>,
    #[serde(default)]
    pub dependencies: BTreeMap<String, std::collections::BTreeSet<String>>,
    #[serde(default)]
    pub invalid: std::collections::BTreeSet<String>,
}

impl ChangeSet {
    pub fn validate_review(&self, ids: &[String], partial: bool) -> Result<()> {
        for id in ids {
            if self.invalid.contains(id) {
                bail!("提议的依据已变化，请重新生成：{id}");
            }
            if let Some(dependencies) = self.dependencies.get(id) {
                for dependency in dependencies
                    .iter()
                    .filter(|dependency| self.proposals.contains_key(*dependency))
                {
                    if partial || !ids.contains(dependency) {
                        bail!("相关修改需一起接受：{id}、{dependency}");
                    }
                }
            }
            if partial
                && self.dependencies.iter().any(|(other, dependencies)| {
                    self.proposals.contains_key(other) && dependencies.contains(id)
                })
            {
                bail!("此文档包含关联修改，请一起接受相关文档");
            }
        }
        Ok(())
    }

    pub fn invalidate_dependents(&mut self, id: &str) {
        let mut changed = std::collections::BTreeSet::from([id.to_owned()]);
        loop {
            let previous = changed.len();
            for (other, dependencies) in &self.dependencies {
                if dependencies
                    .iter()
                    .any(|dependency| changed.contains(dependency))
                {
                    changed.insert(other.clone());
                }
            }
            if previous == changed.len() {
                break;
            }
        }
        changed.remove(id);
        self.invalid.extend(changed);
    }

    pub fn accept_edit(&mut self, id: &str, index: usize, current: &str) -> Result<ProposedEdit> {
        let proposal = self
            .proposals
            .get_mut(id)
            .ok_or_else(|| anyhow::anyhow!("修改已不存在"))?;
        proposal.validate(current)?;
        let edit = proposal
            .edits()
            .into_iter()
            .nth(index)
            .ok_or_else(|| anyhow::anyhow!("修改已变化，请重新审阅"))?;
        if proposal.base.source.get(edit.range.clone()).is_none() {
            bail!("修改区间无效");
        }
        let mut source = proposal.base.source.to_string();
        source.replace_range(edit.range.clone(), &edit.replacement);
        proposal.base.source = source.into();
        proposal.edits_cache.take();
        if proposal.base.source == proposal.proposed {
            self.proposals.remove(id);
        }
        Ok(edit)
    }
    pub fn read<'a>(&'a self, document: &'a DocumentSnapshot) -> &'a str {
        self.proposals
            .get(&document.id)
            .map(|proposal| proposal.proposed.as_ref())
            .unwrap_or(&document.source)
    }

    pub fn propose(&mut self, document: DocumentSnapshot, before: &str, after: &str) -> Result<()> {
        self.propose_within(document, before, after, None)
    }

    pub fn propose_within(
        &mut self,
        document: DocumentSnapshot,
        before: &str,
        after: &str,
        allowed: Option<Range<usize>>,
    ) -> Result<()> {
        if self.invalid.contains(&document.id) {
            bail!("提议的依据已变化，请先重置并读取最新文档");
        }
        if let Some(proposal) = self.proposals.get(&document.id) {
            proposal.validate(&document.source)?;
        }
        let source = self.read(&document);
        let allowed = allowed.unwrap_or(0..source.len());
        let searchable = source
            .get(allowed.clone())
            .ok_or_else(|| anyhow::anyhow!("允许修改的选区已失效"))?;
        let range = if before.is_empty() {
            allowed.end..allowed.end
        } else {
            let mut matches = searchable.match_indices(before);
            let Some((start, _)) = matches.next() else {
                bail!("原文未找到，请先读取最新内容");
            };
            // match_indices 只给不重叠的匹配："aa" 在 "aaa" 里语义上有两处候选。
            // 把起点推进首字符再查一次，覆盖重叠出现。
            let next = start + before.chars().next().map_or(0, char::len_utf8);
            if searchable.get(next..).is_some_and(|rest| rest.contains(before)) {
                bail!("原文有多处匹配，请提供更完整的上下文");
            }
            allowed.start + start..allowed.start + start + before.len()
        };
        let mut proposed = source.to_owned();
        proposed.replace_range(range, after);
        if proposed == document.source.as_ref() {
            self.proposals.remove(&document.id);
        } else {
            let proposal = Proposal::new(document.clone(), proposed);
            proposal
                .edits_cache
                .get_or_init(|| proposal.compute_edits());
            self.proposals.insert(document.id, proposal);
        }
        Ok(())
    }

    pub fn reject_edit(&mut self, id: &str, index: usize) -> Result<()> {
        self.invalidate_dependents(id);
        let proposal = self
            .proposals
            .get_mut(id)
            .ok_or_else(|| anyhow::anyhow!("修改已不存在"))?;
        let edits = proposal.edits();
        let edit = edits
            .get(index)
            .ok_or_else(|| anyhow::anyhow!("修改已变化，请重新审阅"))?;
        let original = proposal
            .base
            .source
            .get(edit.range.clone())
            .ok_or_else(|| anyhow::anyhow!("修改区间无效"))?;
        let start = edits
            .iter()
            .take(index)
            .try_fold(edit.range.start, |offset, previous| {
                offset
                    .checked_add(previous.replacement.len())?
                    .checked_sub(previous.range.len())
            })
            .ok_or_else(|| anyhow::anyhow!("修改区间无效"))?;
        let range = start..start + edit.replacement.len();
        if proposal.proposed.get(range.clone()).is_none() {
            bail!("修改区间无效");
        }
        let mut text = proposal.proposed.to_string();
        text.replace_range(range, original);
        proposal.proposed = text.into();
        proposal.edits_cache.take();
        if proposal.proposed == proposal.base.source {
            self.proposals.remove(id);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
