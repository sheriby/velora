use crate::ai::ProposedEdit;
use anyhow::{Context, Result, bail};
use std::ops::Range;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::editor) enum MarkKind {
    Added,
    Removed,
}

#[derive(Clone, Debug)]
pub(in crate::editor) struct ReviewMark {
    pub range: Range<usize>,
    pub kind: MarkKind,
    pub edits: Range<usize>,
}

fn chunks<'a>(source: &'a str, blocks: &[Range<usize>]) -> Result<Vec<(Range<usize>, &'a str)>> {
    if source.is_empty() {
        return Ok(Vec::new());
    }
    let mut starts = vec![0];
    for block in blocks {
        if source.get(block.clone()).is_none() {
            bail!("审阅块的源码范围无效");
        }
        if let Some(previous) = starts.last().copied() {
            if block.start < previous {
                bail!("审阅块的顺序无效");
            }
            if block.start > previous {
                starts.push(block.start);
            }
        }
    }
    if starts.last().copied() != Some(source.len()) {
        starts.push(source.len());
    }
    starts
        .windows(2)
        .map(|bounds| {
            let Some((&start, rest)) = bounds.split_first() else {
                bail!("缺少审阅块起点");
            };
            let end = rest.first().copied().context("缺少审阅块终点")?;
            let range = start..end;
            Ok((
                range.clone(),
                source.get(range).context("审阅块不在字符边界")?,
            ))
        })
        .collect()
}

pub(super) fn covered_edits(edits: &[ProposedEdit], range: Range<usize>) -> Range<usize> {
    let first = edits.partition_point(|edit| edit.range.end < range.start);
    let last = edits.partition_point(|edit| edit.range.start <= range.end);
    if first < last {
        first..last
    } else {
        // 无重叠时宁可不归属：行级与块级两次差分对重复段落的对齐可能不同，
        // 强行挂到邻近编辑会让行内按钮作用于错位的修改。
        0..0
    }
}

pub(super) fn tracked_document(
    before: &str,
    before_blocks: &[Range<usize>],
    after: &str,
    after_blocks: &[Range<usize>],
    edits: &[ProposedEdit],
) -> Result<(String, Vec<ReviewMark>)> {
    if edits.is_empty() {
        return Ok((after.to_owned(), Vec::new()));
    }
    let before_chunks = chunks(before, before_blocks)?;
    let after_chunks = chunks(after, after_blocks)?;
    // 根块边界来自编辑器，忽略末尾空行的键避免新增段落时重复显示未改的正文。
    let before_keys: Vec<_> = before_chunks
        .iter()
        .map(|(_, text)| text.trim_end_matches('\n'))
        .collect();
    let after_keys: Vec<_> = after_chunks
        .iter()
        .map(|(_, text)| text.trim_end_matches('\n'))
        .collect();
    let operations = similar::capture_diff_slices_deadline(
        similar::Algorithm::Myers,
        &before_keys,
        &after_keys,
        Some(Instant::now() + Duration::from_millis(100)),
    );
    let mut text = String::new();
    let mut marks = Vec::new();
    for operation in operations {
        let (tag, old, new) = operation.as_tag_tuple();
        let old_start = before_chunks
            .get(old.start)
            .map_or(before.len(), |(span, _)| span.start);
        let old_end = old
            .clone()
            .last()
            .and_then(|index| before_chunks.get(index))
            .map_or(old_start, |(span, _)| span.end);
        let indices = covered_edits(edits, old_start..old_end);
        if matches!(tag, similar::DiffTag::Delete | similar::DiffTag::Replace) {
            let start = text.len();
            for (_, chunk) in before_chunks.get(old.clone()).context("删除块范围无效")? {
                text.push_str(chunk);
            }
            marks.push(ReviewMark {
                range: start..text.len(),
                kind: MarkKind::Removed,
                edits: indices.clone(),
            });
        }
        if matches!(tag, similar::DiffTag::Insert | similar::DiffTag::Replace) {
            if tag == similar::DiffTag::Replace && !text.ends_with("\n\n") {
                if !text.ends_with('\n') {
                    text.push('\n');
                }
                text.push('\n');
            }
            let start = text.len();
            for (_, chunk) in after_chunks.get(new.clone()).context("新增块范围无效")? {
                text.push_str(chunk);
            }
            marks.push(ReviewMark {
                range: start..text.len(),
                kind: MarkKind::Added,
                edits: indices,
            });
        } else if tag == similar::DiffTag::Equal {
            // 块键只比 trim 掉收尾换行后的正文：仅块间空行的差异在这里归一成
            // Equal。这类编辑也必须有可见标记，否则会以正文原样混入、被盲接受。
            for old_index in old.clone() {
                let new_index = new.start + (old_index - old.start);
                let Some((before_span, before_chunk)) = before_chunks.get(old_index) else {
                    bail!("未修改块范围无效");
                };
                let Some((_, after_chunk)) = after_chunks.get(new_index) else {
                    bail!("未修改块范围无效");
                };
                if before_chunk == after_chunk {
                    text.push_str(after_chunk);
                    continue;
                }
                let indices = covered_edits(edits, before_span.clone());
                let start = text.len();
                text.push_str(before_chunk);
                marks.push(ReviewMark {
                    range: start..text.len(),
                    kind: MarkKind::Removed,
                    edits: indices.clone(),
                });
                let start = text.len();
                text.push_str(after_chunk);
                marks.push(ReviewMark {
                    range: start..text.len(),
                    kind: MarkKind::Added,
                    edits: indices,
                });
            }
        }
    }
    Ok((text, marks))
}

#[derive(Clone)]
pub(in crate::editor) struct ReviewBinding {
    pub session_id: String,
    pub owner: gpui::WeakEntity<super::Editor>,
    pub document_id: String,
    pub versions: (u64, u64),
    pub index: usize,
    pub enabled: bool,
    pub can_accept: bool,
}
