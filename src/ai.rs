use std::collections::BTreeMap;
use std::ops::Range;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

pub(crate) mod context;
pub(crate) mod runtime;
pub(crate) mod providers;
pub(crate) mod session;
pub(crate) mod settings;
pub(crate) mod skills;
pub(crate) mod workspace;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AiCommand {
    Ask,
    Polish,
    Expand,
    Translate,
    Summarize,
    Explain,
}

impl AiCommand {
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

    pub fn instruction(self) -> &'static str {
        match self {
            Self::Ask => "回答用户关于文档的问题。",
            Self::Summarize => concat!(
                "对选区生成摘要；无选区时读取并概括当前文档。先识别内容类型和主要问题，再提炼核心结论、关键事实、决策与未决事项；不要逐段复述。",
                "通常用一段短文或 3—5 个要点，短选区相应缩短，不为凑条目重复内容。保留重要的数字、日期、条件和不确定性，不把计划说成已完成，不把新增功能说成修复。",
                "只依据已读内容；需要其他文档时按需读取并附可跳转的来源，不编造引用。直接给出摘要，不写开场白、原文对照或执行过程。只供阅读，不生成正文修改。"
            ),
            Self::Explain => concat!(
                "解释指定选区，结合附近上下文识别它在本文中的作用。先用简明语言说明核心含义，再仅展开理解所必需的细节。",
                "代码说明目的、输入输出与关键逻辑；公式说明符号、关系和适用条件；术语结合本文语境说明，必要时给一个贴切的小例子。不要机械逐句翻译或复制选区充当解释。",
                "选区本身信息不足时按需读取当前文档或相关笔记，引用外部于选区的资料时附来源；区分原文事实与解释性推断，不确定时明确说明。只回答，不插入或修改正文。"
            ),
            Self::Translate => concat!(
                "把指定选区翻译为单独给出的目标语言。译文应自然、准确，保留原意、文体、语气、逻辑关系和限定条件；同一术语前后一致，不擅自删减或扩充。",
                "保留 Markdown 层级、列表、链接目标、图片路径、代码标识符、代码内容、公式、占位符与引用标记；只翻译其中需要翻译的自然语言。",
                "直接输出译文，不附原文、对照表、开场白或解释，除非用户明确要求。内容已是目标语言时保持原意，不做额外润色。仅供阅读，不插入或替换正文。"
            ),
            Self::Polish => concat!(
                "润色指定选区。先理解文体、读者与上下文，在保留作者原意和语气的前提下，修正错别字、语法、标点和不自然表达，改善冗余、衔接及句子结构。",
                "保留事实、数字、专名、论证强度和不确定性，不加入新观点，不为显得正式而堆砌术语，也不要将自然表达改成模板化文风。已经清晰的内容不必强行改写。",
                "保留 Markdown 结构、链接与路径、代码、公式及引用标记；只提出完成任务所需的局部修改，不格式化全文，不改变选区外内容。"
            ),
            Self::Expand => concat!(
                "扩写指定选区，在保留原意、文体和已有框架的基础上补充有用内容。如果选区是提纲，就沿既有层级填充适当的论点、解释、步骤或例子；如果是段落，就补足必要背景与论证。",
                "新增内容必须服务原主题，有具体信息，避免重复、套话和无关延伸。需要依据时按需查阅相关文档并给出来源；不编造数据、引文、经历或已经完成的事实。关键资料不足时指出缺口或提出必要的问题。",
                "保留已有标题层级、Markdown 写法、链接、代码、公式与引用；只能修改指定选区，不改其他段落或文档。"
            ),
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
            if matches.next().is_some() {
                bail!("原文有多处匹配，请提供更完整的上下文");
            }
            allowed.start + start..allowed.start + start + before.len()
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
