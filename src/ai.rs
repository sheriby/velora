use std::collections::BTreeMap;
use std::ops::Range;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

pub(crate) mod context;
pub(crate) mod runtime;
pub(crate) mod session;
pub(crate) mod settings;
pub(crate) mod skills;
pub(crate) mod workspace;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AiCommand {
    Ask,
    Polish,
    FixGrammar,
    Shorten,
    Expand,
    Translate,
    Summarize,
    Explain,
    Continue,
}

impl AiCommand {
    pub fn requires_selection(self) -> bool {
        matches!(
            self,
            Self::Polish
                | Self::FixGrammar
                | Self::Shorten
                | Self::Expand
                | Self::Translate
                | Self::Explain
        )
    }

    pub const ALL: [Self; 8] = [
        Self::Polish,
        Self::FixGrammar,
        Self::Shorten,
        Self::Expand,
        Self::Translate,
        Self::Summarize,
        Self::Explain,
        Self::Continue,
    ];

    pub fn label(self, strings: &crate::i18n::I18nStrings) -> String {
        match self {
            Self::Ask => strings.ai_panel_ask.clone(),
            Self::Polish => strings.ai_action_polish.clone(),
            Self::FixGrammar => strings.ai_action_fix_grammar.clone(),
            Self::Shorten => strings.ai_action_shorten.clone(),
            Self::Expand => strings.ai_action_expand.clone(),
            Self::Translate => strings.ai_action_translate.clone(),
            Self::Summarize => strings.ai_action_summarize.clone(),
            Self::Explain => strings.ai_action_explain.clone(),
            Self::Continue => strings.ai_action_continue.clone(),
        }
    }

    pub fn instruction(self) -> &'static str {
        match self {
            Self::Ask => "回答用户关于文档的问题。",
            Self::Polish => {
                "润色指定选区，使表达清晰自然，保留原意与作者语气，只提出必要的局部修改。"
            }
            Self::FixGrammar => "检查指定选区的错别字、语法和标点，只提出最小修改。",
            Self::Shorten => "缩短指定选区，保留关键事实、例子和引用。",
            Self::Expand => "扩写指定选区，结合已有资料补足解释，不编造事实。",
            Self::Translate => {
                "把指定选区翻译成用户指定的目标语言，保留 Markdown 结构、链接、代码、公式与术语。"
            }
            Self::Summarize => "总结指定选区；没有选区时总结当前文档，给出有来源的简明摘要。",
            Self::Explain => "解释指定选区，必要时查阅相关笔记并标明来源。",
            Self::Continue => "从指定光标或选区末尾续写，只提出新增内容，衔接前文并保持写作风格。",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct DocumentSnapshot {
    pub id: String,
    pub source: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ProposedEdit {
    pub range: Range<usize>,
    pub replacement: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Proposal {
    pub base: DocumentSnapshot,
    pub proposed: String,
    #[serde(skip)]
    edits_cache: std::sync::OnceLock<Vec<ProposedEdit>>,
}

impl Proposal {
    pub fn new(base: DocumentSnapshot, proposed: String) -> Self {
        Self {
            base,
            proposed,
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
            .diff_lines(&self.base.source, &self.proposed);
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
        if current != self.base.source {
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
        proposal
            .base
            .source
            .replace_range(edit.range.clone(), &edit.replacement);
        proposal.edits_cache.take();
        if proposal.base.source == proposal.proposed {
            self.proposals.remove(id);
        }
        Ok(edit)
    }
    pub fn read<'a>(&'a self, document: &'a DocumentSnapshot) -> &'a str {
        self.proposals
            .get(&document.id)
            .map(|proposal| proposal.proposed.as_str())
            .unwrap_or(&document.source)
    }

    pub fn propose(&mut self, document: DocumentSnapshot, before: &str, after: &str) -> Result<()> {
        if self.invalid.contains(&document.id) {
            bail!("提议的依据已变化，请先重置并读取最新文档");
        }
        if let Some(proposal) = self.proposals.get(&document.id) {
            proposal.validate(&document.source)?;
        }
        let source = self.read(&document);
        let range = if before.is_empty() {
            source.len()..source.len()
        } else {
            let mut matches = source.match_indices(before);
            let Some((start, _)) = matches.next() else {
                bail!("原文未找到，请先读取最新内容");
            };
            if matches.next().is_some() {
                bail!("原文有多处匹配，请提供更完整的上下文");
            }
            start..start + before.len()
        };
        let mut proposed = source.to_owned();
        proposed.replace_range(range, after);
        if proposed == document.source {
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
        proposal.proposed.replace_range(range, original);
        proposal.edits_cache.take();
        if proposal.proposed == proposal.base.source {
            self.proposals.remove(id);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
