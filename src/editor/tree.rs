//! Runtime ownership for the editor block tree.
//!
//! [`DocumentTree`] is the only mutable owner of block ordering and parent-child
//! relationships inside the editor. It also maintains a cached
//! [`VisibleTreeSnapshot`] so hot-path lookups do not re-run a full DFS on every
//! focus, scroll, or mutation event.

use std::collections::HashMap;
use std::sync::Arc;

use gpui::*;

use super::Editor;
use super::document::{ChunkCursor, line_is_list_marker};
use crate::components::serialize_table_markdown_lines;
use crate::components::{Block, BlockKind, CalloutVariant, parse_standalone_image};

/// Part of a document whose lines have not been turned into blocks yet.
///
/// A huge document is imported in chunks (roadmap G8): the editor holds the
/// untouched line array plus a cursor, materializes more roots as it goes, and
/// serializes the not-yet-built remainder verbatim so saves and exports stay
/// complete at any moment.
#[derive(Clone)]
pub(super) struct PendingTail {
    /// Every line of the document, not just the tail: a later chunk must see the
    /// same forward context a single full pass would (paragraph continuation and
    /// setext detection scan ahead).
    pub(super) lines: Arc<Vec<String>>,
    /// Index of the first line that has not been consumed by a chunk.
    pub(super) next_line: usize,
    /// Whether the last built root is a list item, so a blank run opening the
    /// next chunk counts preserved empty paragraphs the same way a full pass
    /// would.
    pub(super) previous_root_is_list_item: bool,
}

/// P6a：代码/纯文本文档的流式续建尾部——不再把全文切成 9.8 万个
/// String，保留原始字节，materialize 时按行扫描切块。`source` 以分隔
/// 换行符开头（与最后一块的连接符），`next_line` 是已消费的行片段数。
#[derive(Clone)]
pub(super) struct PendingSourceTail {
    pub source: String,
    pub next_line: usize,
}

/// Ordinal/blank-run carry-over for [`DocumentTree::append_roots`].
#[derive(Clone, Copy, Default)]
struct SyncSeeds {
    numbered_list_ordinal: usize,
    previous_was_list_item: bool,
}

/// A block together with its position in the current visible DFS order.
#[derive(Clone)]
pub(crate) struct VisibleBlock {
    pub entity: Entity<Block>,
}

/// A block's position inside the runtime tree.
#[derive(Clone)]
pub(super) struct BlockLocation {
    pub parent: Option<Entity<Block>>,
    pub index: usize,
}

/// Cached visible-order metadata for the current runtime tree.
#[derive(Default, Clone)]
pub(super) struct VisibleTreeSnapshot {
    visible: Vec<VisibleBlock>,
    visible_index_by_entity: HashMap<EntityId, usize>,
    location_by_entity: HashMap<EntityId, BlockLocation>,
    last_visible_descendant_by_entity: HashMap<EntityId, EntityId>,
}

impl VisibleTreeSnapshot {
    fn clear(&mut self) {
        self.visible.clear();
        self.visible_index_by_entity.clear();
        self.location_by_entity.clear();
        self.last_visible_descendant_by_entity.clear();
    }
}

/// Canonical owner of the runtime block tree.
///
/// The Markdown importer builds root blocks and nested list children, then
/// hands the structure to `DocumentTree`. From that point on, every structural
/// edit must go through this type so the runtime tree stays aligned with the
/// subset of Markdown that the importer and serializer can reconstruct.
pub(super) struct DocumentTree {
    roots: Vec<Entity<Block>>,
    snapshot: VisibleTreeSnapshot,
    pending: Option<PendingTail>,
    /// P6a：代码/纯文本文档的原始字节尾部（与 `pending` 互斥使用）。
    pending_source: Option<PendingSourceTail>,
}

impl DocumentTree {
    pub(super) fn new(roots: Vec<Entity<Block>>) -> Self {
        Self {
            roots,
            snapshot: VisibleTreeSnapshot::default(),
            pending: None,
            pending_source: None,
        }
    }

    pub(super) fn pending_tail(&self) -> Option<&PendingTail> {
        self.pending.as_ref()
    }

    pub(super) fn set_pending_tail(&mut self, pending: Option<PendingTail>) {
        self.pending = pending;
    }

    pub(super) fn pending_source(&self) -> Option<&PendingSourceTail> {
        self.pending_source.as_ref()
    }

    pub(super) fn take_pending_source(&mut self) -> Option<PendingSourceTail> {
        self.pending_source.take()
    }

    pub(super) fn set_pending_source(&mut self, pending: Option<PendingSourceTail>) {
        self.pending_source = pending;
    }

    pub(super) fn first_root(&self) -> Option<&Entity<Block>> {
        self.roots.first()
    }

    pub(super) fn root_blocks(&self) -> &[Entity<Block>] {
        &self.roots
    }

    pub(super) fn root_count(&self) -> usize {
        self.roots.len()
    }

    pub(super) fn visible_blocks(&self) -> &[VisibleBlock] {
        &self.snapshot.visible
    }

    pub(super) fn flatten_visible_blocks(&self) -> Vec<VisibleBlock> {
        self.snapshot.visible.clone()
    }

    pub(super) fn focused_block_entity_id(&self, window: &Window, cx: &App) -> Option<EntityId> {
        self.snapshot
            .visible
            .iter()
            .find(|visible| visible.entity.read(cx).focus_handle.is_focused(window))
            .map(|visible| visible.entity.entity_id())
    }

    pub(super) fn visible_index_for_entity_id(&self, entity_id: EntityId) -> Option<usize> {
        self.snapshot
            .visible_index_by_entity
            .get(&entity_id)
            .copied()
    }

    pub(super) fn block_entity_by_id(&self, entity_id: EntityId) -> Option<Entity<Block>> {
        self.visible_index_for_entity_id(entity_id)
            .and_then(|index| self.snapshot.visible.get(index))
            .map(|visible| visible.entity.clone())
    }

    /// Resolves a block entity through the full-tree location map; unlike
    /// `block_entity_by_id` this also works for blocks outside the visible
    /// window.
    pub(super) fn block_entity_at_location(
        &self,
        entity_id: EntityId,
        cx: &App,
    ) -> Option<Entity<Block>> {
        let location = self.find_block_location(entity_id)?;
        match &location.parent {
            Some(parent) => parent.read(cx).children.get(location.index).cloned(),
            None => self.roots.get(location.index).cloned(),
        }
    }

    pub(super) fn find_block_location(&self, entity_id: EntityId) -> Option<BlockLocation> {
        self.snapshot.location_by_entity.get(&entity_id).cloned()
    }

    /// Returns the sibling immediately before `entity_id` within the same
    /// parent, if any.
    pub(super) fn previous_sibling(&self, entity_id: EntityId, cx: &App) -> Option<Entity<Block>> {
        let location = self.find_block_location(entity_id)?;
        let prev_index = location.index.checked_sub(1)?;
        match &location.parent {
            Some(parent) => parent.read(cx).children.get(prev_index).cloned(),
            None => self.roots.get(prev_index).cloned(),
        }
    }

    pub(super) fn last_visible_descendant(&self, entity_id: EntityId) -> Option<Entity<Block>> {
        let descendant_id = self
            .snapshot
            .last_visible_descendant_by_entity
            .get(&entity_id)
            .copied()?;
        self.block_entity_by_id(descendant_id)
    }

    pub(super) fn replace_roots(&mut self, roots: Vec<Entity<Block>>, cx: &mut Context<Editor>) {
        self.roots = roots;
        // The replacement defines the whole document, so any not-yet-built tail
        // belongs to the previous content.
        self.pending = None;
        self.rebuild_metadata_and_snapshot(cx);
    }

    /// Appends freshly built roots and extends the cached snapshot instead of
    /// re-running the full DFS, so streaming a huge document stays linear.
    pub(super) fn append_roots(&mut self, roots: Vec<Entity<Block>>, cx: &mut Context<Editor>) {
        if roots.is_empty() {
            return;
        }

        let (numbered_list_ordinal, previous_was_list_item) = match self.roots.last() {
            Some(last) => {
                let last = last.read(cx);
                (
                    last.list_ordinal.unwrap_or(0),
                    last.kind().is_list_item(),
                )
            }
            None => (0, false),
        };
        let base_index = self.roots.len();
        Self::sync_block_list(
            &roots,
            None,
            None,
            0,
            0,
            None,
            None,
            0,
            None,
            None,
            None,
            cx,
            &mut self.snapshot,
            SyncSeeds {
                numbered_list_ordinal,
                previous_was_list_item,
            },
        );
        self.roots.extend(roots.clone());
        // 增量注册时 `sync_block_list` 只看到本批新块，`location.index`
        // 从 0 起算；顶层根块的真实下标要从既有 roots 数量起算，否则
        // 后续按 id 定位删除/插入会命中错误块。
        for (offset, block) in roots.iter().enumerate() {
            if let Some(location) = self.snapshot.location_by_entity.get_mut(&block.entity_id()) {
                if location.parent.is_none() {
                    location.index = base_index + offset;
                }
            }
        }
    }

    /// Materializes every remaining pending line.
    ///
    /// Structural edits and document-wide scans (search, outline) call this
    /// first: they must operate on the whole document, and the join of built
    /// roots plus pending lines is only guaranteed to serialize like a full pass
    /// while nothing has moved underneath it.
    pub(super) fn flush_pending_tail(&mut self, cx: &mut Context<Editor>) {
        while let Some(tail) = self.pending.clone() {
            let (roots, consumed) = Editor::build_root_block_chunk(
                cx,
                &tail.lines[tail.next_line..],
                ChunkCursor {
                    root_budget: usize::MAX,
                    is_document_start: false,
                    previous_root_is_list_item: tail.previous_root_is_list_item,
                },
            );
            let previous_root_is_list_item = roots
                .last()
                .map(|block| block.read(cx).kind().is_list_item())
                .unwrap_or(tail.previous_root_is_list_item);
            let next_line = tail.next_line + consumed;
            self.append_roots(roots, cx);
            self.pending = if next_line < tail.lines.len() {
                Some(PendingTail {
                    lines: tail.lines.clone(),
                    next_line,
                    previous_root_is_list_item,
                })
            } else {
                None
            };
        }
    }

    pub(super) fn markdown_text(&self, cx: &App) -> String {
        let mut lines = Vec::new();
        Self::collect_root_markdown_lines(&self.roots, cx, &mut lines, self.pending.as_ref());
        lines.join("\n")
    }

    pub(super) fn raw_source_text(&self, cx: &App) -> String {
        // P5：单遍追加。旧实现先把每块文本克隆成 String 再 join——超大
        // 文档一次序列化要付两倍字节量的搬运。
        let mut capacity = 0usize;
        for visible in &self.snapshot.visible {
            capacity += visible.entity.read(cx).display_text().len() + 1;
        }
        if let Some(tail) = &self.pending {
            for line in &tail.lines[tail.next_line..] {
                capacity += line.len() + 1;
            }
        }
        if let Some(tail) = &self.pending_source {
            capacity += tail.source.len();
        }
        let mut out = String::with_capacity(capacity);
        for visible in &self.snapshot.visible {
            out.push_str(visible.entity.read(cx).display_text());
            out.push('\n');
        }
        if let Some(tail) = &self.pending {
            for line in &tail.lines[tail.next_line..] {
                out.push_str(line);
                out.push('\n');
            }
        }
        if let Some(tail) = &self.pending_source {
            // source 以边界换行开头，逐字拼接即还原原文。
            out.push_str(&tail.source);
        } else if !out.is_empty() {
            out.pop();
        }
        out
    }

    pub(super) fn insert_blocks_at(
        &mut self,
        parent: Option<Entity<Block>>,
        index: usize,
        blocks: Vec<Entity<Block>>,
        cx: &mut Context<Editor>,
    ) {
        self.with_structure_mutation(cx, move |tree, cx| {
            tree.insert_blocks_at_raw(parent, index, blocks, cx);
        });
    }

    /// Runs a tree mutation and then eagerly rebuilds metadata and the visible
    /// snapshot exactly once for that mutation batch.
    /// P6a：同步物化原始字节尾部（代码/纯文本文档流式续建用）。
    pub(super) fn flush_pending_source(&mut self, cx: &mut Context<Editor>) {
        let Some(tail) = self.pending_source.take() else {
            return;
        };
        let kind = self
            .first_root()
            .map(|block| block.read(cx).kind())
            .unwrap_or(crate::components::BlockKind::Paragraph);
        let chunk_lines = super::file_drop::SOURCE_DOCUMENT_CHUNK_LINES;
        let mut offset = 0usize;
        let mut line_start = tail.next_line + 1;
        let mut roots = Vec::new();
        loop {
            match super::file_drop::scan_chunk_end(tail.source.as_bytes(), offset, chunk_lines) {
                Some(end) => {
                    let is_last = end == tail.source.len();
                    let text = if is_last {
                        &tail.source[offset..]
                    } else {
                        &tail.source[offset..end - 1]
                    };
                    let block = Editor::new_block(cx, crate::components::BlockRecord::with_plain_text(kind.clone(), text));
                    let chunk_line_start = line_start;
                    block.update(cx, |block, _cx| {
                        block.set_source_document_mode();
                        block.set_source_line_start(chunk_line_start);
                    });
                    roots.push(block);
                    line_start += text.split('\n').count();
                    offset = end;
                    if is_last {
                        break;
                    }
                }
                None => {
                    let text = &tail.source[offset..];
                    if !text.is_empty() {
                        let block = Editor::new_block(
                            cx,
                            crate::components::BlockRecord::with_plain_text(kind, text),
                        );
                        let chunk_line_start = line_start;
                        block.update(cx, |block, _cx| {
                            block.set_source_document_mode();
                            block.set_source_line_start(chunk_line_start);
                        });
                        roots.push(block);
                    }
                    break;
                }
            }
        }
        self.append_roots(roots, cx);
    }

    pub(super) fn with_structure_mutation<R>(
        &mut self,
        cx: &mut Context<Editor>,
        mutate: impl FnOnce(&mut Self, &mut Context<Editor>) -> R,
    ) -> R {
        self.flush_pending_tail(cx);
        self.flush_pending_source(cx);
        let result = mutate(self, cx);
        self.rebuild_metadata_and_snapshot(cx);
        result
    }

    /// Rebuilds tree metadata and cached visible-order data from the current
    /// roots.
    ///
    /// The pass first normalizes impossible runtime-only shapes by hoisting
    /// children out of leaf blocks. It then performs one DFS to update parent
    /// UUIDs, child UUID lists, render depth, numbered-list ordinals, and the
    /// visible snapshot.
    pub(super) fn rebuild_metadata_and_snapshot(&mut self, cx: &mut Context<Editor>) {
        Self::normalize_block_list(&mut self.roots, cx);
        self.snapshot.clear();
        Self::sync_block_list(
            &self.roots.clone(),
            None,
            None,
            0,
            0,
            None,
            None,
            0,
            None,
            None,
            None,
            cx,
            &mut self.snapshot,
            SyncSeeds::default(),
        );
    }

    pub(super) fn take_children(
        block: &Entity<Block>,
        cx: &mut Context<Editor>,
    ) -> Vec<Entity<Block>> {
        let mut children = Vec::new();
        block.update(cx, |block, _cx| {
            children = std::mem::take(&mut block.children);
        });
        children
    }

    pub(super) fn insert_blocks_at_raw(
        &mut self,
        parent: Option<Entity<Block>>,
        index: usize,
        blocks: Vec<Entity<Block>>,
        cx: &mut Context<Editor>,
    ) {
        if blocks.is_empty() {
            return;
        }

        if let Some(parent) = parent {
            parent.update(cx, move |parent, _cx| {
                for (offset, block) in blocks.iter().cloned().enumerate() {
                    parent.children.insert(index + offset, block);
                }
            });
        } else {
            for (offset, block) in blocks.into_iter().enumerate() {
                self.roots.insert(index + offset, block);
            }
        }
    }

    pub(super) fn remove_block_by_id_raw(
        &mut self,
        entity_id: EntityId,
        cx: &mut Context<Editor>,
    ) -> Option<(Entity<Block>, BlockLocation)> {
        self.flush_pending_tail(cx);
        let location = self.find_block_location(entity_id)?;
        let removed = if let Some(parent) = location.parent.clone() {
            let mut removed = None;
            parent.update(cx, |parent, _cx| {
                removed = Some(parent.children.remove(location.index));
            });
            removed?
        } else {
            self.roots.remove(location.index)
        };

        Some((removed, location))
    }

    /// Normalizes a sibling list so only container-capable block kinds retain
    /// children.
    ///
    /// Children attached to leaf blocks are hoisted into the same parent list
    /// immediately after the leaf that previously owned them.
    fn normalize_block_list(blocks: &mut Vec<Entity<Block>>, cx: &mut Context<Editor>) {
        let mut index = 0;
        while index < blocks.len() {
            let block = blocks[index].clone();
            let mut children = Self::take_children(&block, cx);
            Self::normalize_block_list(&mut children, cx);

            if block.read(cx).kind().supports_children() {
                block.update(cx, {
                    let children = children.clone();
                    move |block, _cx| {
                        block.children = children.clone();
                    }
                });
            } else if !children.is_empty() {
                blocks.splice(index + 1..index + 1, children);
            }

            index += 1;
        }
    }

    fn sync_block_list(
        blocks: &[Entity<Block>],
        parent_entity: Option<Entity<Block>>,
        parent_id: Option<uuid::Uuid>,
        list_depth: usize,
        inherited_quote_depth: usize,
        inherited_quote_group_anchor: Option<uuid::Uuid>,
        inherited_visible_quote_group_anchor: Option<uuid::Uuid>,
        inherited_callout_depth: usize,
        inherited_callout_anchor: Option<uuid::Uuid>,
        inherited_callout_variant: Option<CalloutVariant>,
        inherited_footnote_anchor: Option<uuid::Uuid>,
        cx: &mut Context<Editor>,
        snapshot: &mut VisibleTreeSnapshot,
        seeds: SyncSeeds,
    ) {
        let mut numbered_list_ordinal = seeds.numbered_list_ordinal;
        let mut previous_was_list_item = seeds.previous_was_list_item;
        for (index, block) in blocks.iter().enumerate() {
            let entity_id = block.entity_id();
            let visible_index = snapshot.visible.len();
            snapshot.visible.push(VisibleBlock {
                entity: block.clone(),
            });
            snapshot
                .visible_index_by_entity
                .insert(entity_id, visible_index);
            snapshot.location_by_entity.insert(
                entity_id,
                BlockLocation {
                    parent: parent_entity.clone(),
                    index,
                },
            );

            let (block_id, kind, children, is_empty_paragraph) = {
                let block_ref = block.read(cx);
                (
                    block_ref.record.id,
                    block_ref.kind(),
                    block_ref.children.clone(),
                    block_ref.kind() == BlockKind::Paragraph
                        && block_ref.record.title.visible_text().is_empty()
                        && block_ref.children.is_empty(),
                )
            };
            let parent_is_list_item = parent_entity
                .as_ref()
                .is_some_and(|parent| parent.read(cx).kind().is_list_item());

            let content = children
                .iter()
                .map(|child| child.read(cx).record.id)
                .collect::<Vec<_>>();
            let list_ordinal = if kind.is_numbered_list_item() {
                numbered_list_ordinal += 1;
                Some(numbered_list_ordinal)
            } else {
                numbered_list_ordinal = 0;
                None
            };
            let is_quote_container = kind.is_quote_container();
            let own_callout_variant = kind.callout_variant();
            let quote_depth = inherited_quote_depth + usize::from(is_quote_container);
            let quote_group_anchor = if is_quote_container {
                inherited_quote_group_anchor.or(Some(block_id))
            } else {
                inherited_quote_group_anchor
            };
            let callout_depth =
                inherited_callout_depth + usize::from(own_callout_variant.is_some());
            let callout_anchor = if own_callout_variant.is_some() {
                Some(block_id)
            } else {
                inherited_callout_anchor
            };
            let callout_variant = own_callout_variant.or(inherited_callout_variant);
            let visible_quote_depth = quote_depth.saturating_sub(callout_depth);
            let visible_quote_group_anchor = match kind {
                BlockKind::Quote => inherited_visible_quote_group_anchor.or(Some(block_id)),
                BlockKind::Callout(_) => None,
                _ if visible_quote_depth == 0 => None,
                _ => inherited_visible_quote_group_anchor,
            };
            let child_visible_quote_group_anchor = if own_callout_variant.is_some() {
                None
            } else {
                visible_quote_group_anchor
            };
            let footnote_anchor = if kind.is_footnote_definition() {
                Some(block_id)
            } else {
                inherited_footnote_anchor
            };
            let child_list_depth = list_depth + usize::from(kind.is_list_item());
            let list_group_separator_candidate = is_empty_paragraph && previous_was_list_item;

            block.update(cx, move |block, _cx| {
                block.record.parent = parent_id;
                block.record.content = content.clone();
                block.render_depth = list_depth;
                block.quote_depth = quote_depth;
                block.quote_group_anchor = quote_group_anchor;
                block.visible_quote_depth = visible_quote_depth;
                block.visible_quote_group_anchor = visible_quote_group_anchor;
                block.callout_depth = callout_depth;
                block.callout_anchor = callout_anchor;
                block.callout_variant = callout_variant;
                block.footnote_anchor = footnote_anchor;
                block.parent_is_list_item = parent_is_list_item;
                block.list_ordinal = list_ordinal;
                block.list_group_separator_candidate = list_group_separator_candidate;
            });

            let last_descendant_id = if children.is_empty() {
                entity_id
            } else {
                Self::sync_block_list(
                    &children,
                    Some(block.clone()),
                    Some(block_id),
                    child_list_depth,
                    quote_depth,
                    quote_group_anchor,
                    child_visible_quote_group_anchor,
                    callout_depth,
                    callout_anchor,
                    callout_variant,
                    footnote_anchor,
                    cx,
                    snapshot,
                    SyncSeeds::default(),
                );
                snapshot
                    .last_visible_descendant_by_entity
                    .get(&children.last().expect("children checked").entity_id())
                    .copied()
                    .unwrap_or_else(|| children.last().expect("children checked").entity_id())
            };

            snapshot
                .last_visible_descendant_by_entity
                .insert(entity_id, last_descendant_id);
            previous_was_list_item = kind.is_list_item();
        }
    }

    fn is_empty_root_paragraph(block: &Block) -> bool {
        block.kind() == BlockKind::Paragraph
            && block.record.title.visible_text().is_empty()
            && block.children.is_empty()
    }

    fn collect_root_markdown_lines(
        blocks: &[Entity<Block>],
        cx: &App,
        lines: &mut Vec<String>,
        tail: Option<&PendingTail>,
    ) {
        let mut pending_empty_roots = 0usize;
        let mut wrote_non_empty_root = false;
        let mut previous_was_list_item = false;

        for block in blocks {
            let block_ref = block.read(cx);
            if Self::is_empty_root_paragraph(block_ref) {
                pending_empty_roots += 1;
                continue;
            }

            let current_is_list_item = block_ref.kind().is_list_item();
            if wrote_non_empty_root {
                let separator_count = if previous_was_list_item && current_is_list_item {
                    pending_empty_roots
                } else {
                    pending_empty_roots + 1
                };
                lines.extend(std::iter::repeat_n(String::new(), separator_count));
            } else if pending_empty_roots > 0 {
                lines.extend(std::iter::repeat_n(String::new(), pending_empty_roots));
            }

            Self::collect_single_block_markdown_lines(block_ref, 0, cx, lines);
            wrote_non_empty_root = true;
            pending_empty_roots = 0;
            previous_was_list_item = current_is_list_item;
        }

        if let Some(tail) = tail {
            // The not-yet-built remainder takes the place of the next non-empty
            // root, so it gets the same separator a parsed root would: a blank
            // run kept 1:1 after a list item continues that list group with no
            // extra blank line, every other junction gets one.
            let next_is_list_item = tail
                .lines
                .get(tail.next_line)
                .is_some_and(|line| line_is_list_marker(line));
            let separator_count = if !wrote_non_empty_root {
                pending_empty_roots
            } else if previous_was_list_item && next_is_list_item {
                pending_empty_roots
            } else {
                pending_empty_roots + 1
            };
            lines.extend(std::iter::repeat_n(String::new(), separator_count));
            lines.extend(tail.lines[tail.next_line..].iter().cloned());
            return;
        }

        if wrote_non_empty_root {
            if pending_empty_roots > 0 {
                lines.extend(std::iter::repeat_n(String::new(), pending_empty_roots + 1));
            }
        } else if pending_empty_roots > 1 {
            lines.extend(std::iter::repeat_n(String::new(), pending_empty_roots));
        }
    }

    fn collect_single_block_markdown_lines(
        block_ref: &Block,
        list_depth: usize,
        cx: &App,
        lines: &mut Vec<String>,
    ) {
        match block_ref.kind() {
            BlockKind::Table => {
                if let Some(table) = block_ref.record.table.as_ref() {
                    lines.extend(serialize_table_markdown_lines(table));
                }
            }
            BlockKind::CodeBlock { language } => {
                let indentation = "  ".repeat(list_depth);
                let lang_str = language.as_ref().map(|s| s.as_ref()).unwrap_or("");
                let fence = super::persistence::safe_code_fence_with_info(
                    &block_ref.record.title.visible_text(),
                    language.as_ref().map(|language| language.as_ref()),
                );
                lines.push(format!("{indentation}{fence}{lang_str}"));
                let content = block_ref.record.title.visible_text();
                for code_line in content.split('\n') {
                    lines.push(format!("{indentation}{code_line}"));
                }
                lines.push(format!("{indentation}{fence}"));
            }
            BlockKind::Quote => {
                let title_markdown =
                    CalloutVariant::escape_plain_quote_header(&block_ref.record.title_markdown());
                let indentation = "  ".repeat(list_depth);
                if !title_markdown.is_empty() || block_ref.children.is_empty() {
                    for line in title_markdown.split('\n') {
                        lines.push(format!("{indentation}> {line}"));
                    }
                }

                if !block_ref.children.is_empty() {
                    let mut child_lines = Vec::new();
                    Self::collect_markdown_lines(
                        &block_ref.children,
                        list_depth,
                        cx,
                        &mut child_lines,
                        false,
                    );
                    lines.extend(
                        child_lines
                            .into_iter()
                            .map(|line| format!("{indentation}> {line}")),
                    );
                }
            }
            BlockKind::Callout(variant) => {
                let indentation = "  ".repeat(list_depth);
                lines.push(format!(
                    "{indentation}> {}",
                    variant.header_markdown(&block_ref.record.title_markdown())
                ));
                if !block_ref.children.is_empty() {
                    let mut child_lines = Vec::new();
                    Self::collect_markdown_lines(
                        &block_ref.children,
                        list_depth,
                        cx,
                        &mut child_lines,
                        false,
                    );
                    lines.extend(
                        child_lines
                            .into_iter()
                            .map(|line| format!("{indentation}> {line}")),
                    );
                }
            }
            BlockKind::FootnoteDefinition => {
                let indentation = "  ".repeat(list_depth);
                let id = block_ref.record.title.visible_text();
                if block_ref.children.is_empty() {
                    lines.push(format!("{indentation}[^{}]:", id));
                    return;
                }

                let first_child = block_ref.children.first().cloned().expect("checked");
                let first_is_paragraph = first_child.read(cx).kind() == BlockKind::Paragraph;
                if first_is_paragraph {
                    let first_title = first_child.read(cx).record.title_markdown();
                    let mut first_lines = first_title.split('\n');
                    let first_line = first_lines.next().unwrap_or_default();
                    lines.push(format!("{indentation}[^{}]: {}", id, first_line));
                    for line in first_lines {
                        if line.is_empty() {
                            lines.push(String::new());
                        } else {
                            lines.push(format!("{indentation}    {line}"));
                        }
                    }

                    if block_ref.children.len() > 1 {
                        lines.push(String::new());
                        Self::collect_markdown_lines(&block_ref.children[1..], 2, cx, lines, true);
                    }
                } else {
                    lines.push(format!("{indentation}[^{}]:", id));
                    Self::collect_markdown_lines(&block_ref.children, 2, cx, lines, true);
                }
            }
            BlockKind::RawMarkdown
            | BlockKind::FrontMatter
            | BlockKind::Comment
            | BlockKind::HtmlBlock
            | BlockKind::MathBlock
            | BlockKind::MermaidBlock => {
                let indentation = "  ".repeat(list_depth);
                let raw_markdown = block_ref
                    .record
                    .raw_fallback
                    .clone()
                    .unwrap_or_else(|| block_ref.record.title_markdown());
                for line in raw_markdown.split('\n') {
                    if indentation.is_empty() {
                        lines.push(line.to_string());
                    } else {
                        lines.push(format!("{indentation}{line}"));
                    }
                }
            }
            BlockKind::BulletedListItem
            | BlockKind::TaskListItem { .. }
            | BlockKind::NumberedListItem => {
                lines.push(
                    block_ref
                        .record
                        .markdown_line(list_depth, block_ref.list_ordinal),
                );
                let child_list_depth = list_depth + 1;
                for child in &block_ref.children {
                    let child_ref = child.read(cx);
                    if Self::list_child_requires_leading_blank_line(child_ref) {
                        lines.push(String::new());
                    }
                    Self::collect_single_block_markdown_lines(
                        child_ref,
                        child_list_depth,
                        cx,
                        lines,
                    );
                }
            }
            _ => {
                lines.push(
                    block_ref
                        .record
                        .markdown_line(list_depth, block_ref.list_ordinal),
                );
                let child_list_depth = list_depth + usize::from(block_ref.kind().is_list_item());
                Self::collect_markdown_lines(
                    &block_ref.children,
                    child_list_depth,
                    cx,
                    lines,
                    false,
                );
            }
        }
    }

    fn list_child_requires_leading_blank_line(block_ref: &Block) -> bool {
        if block_ref.kind() != BlockKind::Paragraph || !block_ref.children.is_empty() {
            return false;
        }

        let markdown = block_ref.record.title_markdown();
        !markdown.is_empty() && parse_standalone_image(&markdown).is_none()
    }

    fn collect_markdown_lines(
        blocks: &[Entity<Block>],
        depth: usize,
        cx: &App,
        lines: &mut Vec<String>,
        blank_line_between_siblings: bool,
    ) {
        let mut first = true;
        let mut previous_was_list_item = false;
        for block in blocks {
            let current_is_list_item = block.read(cx).kind().is_list_item();
            if !first
                && blank_line_between_siblings
                && !(previous_was_list_item && current_is_list_item)
            {
                lines.push(String::new());
            }
            first = false;

            let block_ref = block.read(cx);
            Self::collect_single_block_markdown_lines(block_ref, depth, cx, lines);
            previous_was_list_item = current_is_list_item;
        }
    }
}


#[cfg(test)]
mod tests;
