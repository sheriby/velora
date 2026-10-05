//! 「段落」菜单与快捷键的入口：把选区覆盖到的根块换成另一种块种类。
//!
//! 一条命令可能改好几块，但只算一步撤销；字节只重写被波及的那一段（连块与块之间的
//! 空行接缝一起），其余文件的字节原样不动——这是「缓冲区是事实源」换来的性质。

use gpui::*;

use super::Editor;
use crate::components::{BlockKind, UndoCaptureKind};
use crate::editor::{Block, ViewMode};

/// 「段落」菜单的目标种类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BlockKindTarget {
    /// 标题 1–6 级；对已经是这一级的块等于取消标题。
    Heading(u8),
    /// 普通段落。
    Paragraph,
}

impl Editor {
    /// 在当前选区（没有跨块选区时就是光标所在那一块）上换块种类，返回是否改到了内容。
    pub(crate) fn apply_block_kind_to_selection(
        &mut self,
        target: BlockKindTarget,
        cx: &mut Context<Self>,
    ) -> bool {
        // 源码视图里根块就是文件本身，换种类等于改写用户的字面文本（计划 §6 的边界）。
        if self.view_mode != ViewMode::Rendered || !self.writes_through_the_buffer() {
            return false;
        }
        self.flush_pending_materialization(cx);

        let roots = self.selection_root_blocks(cx);
        if roots.is_empty() {
            return false;
        }

        let ids_before: Vec<EntityId> = self
            .document
            .root_layout()
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
        let mut changed: Vec<Entity<Block>> = Vec::new();
        for root in &roots {
            let next = root.read_with(cx, |block, _cx| target.next_kind(&block.kind()));
            let Some(next) = next else { continue };
            if root.update(cx, |block, _cx| block.set_kind_in_place(next)) {
                changed.push(root.clone());
            }
        }
        if changed.is_empty() {
            self.finalize_pending_undo_capture(cx);
            return false;
        }

        self.document.rebuild_metadata_and_snapshot(cx);
        // 写回按「第一块改到的根 … 最后一块改到的根」这一段算，前面那根的区间一起
        // 带上：块与块之间的空行接缝归前一块管，列表项换成标题要多空的那一行在这里。
        let written_back = self.write_back_block_kind_region(&ids_before, &changed, cx);
        if written_back {
            self.mark_dirty_written_back(cx);
        } else {
            self.mark_dirty(cx);
        }
        self.finalize_pending_undo_capture(cx);
        cx.notify();
        true
    }

    /// 换种类之后按区段拼法写回字节；算不出可靠区段时返回 false，让调用方整篇重投影。
    ///
    /// 根块序列一旦变了（带子块的列表项换成标题会把子块提上来），接缝归属就不再由
    /// 这一段说了算，这条路直接放弃。
    fn write_back_block_kind_region(
        &mut self,
        ids_before: &[EntityId],
        changed: &[Entity<Block>],
        cx: &mut Context<Self>,
    ) -> bool {
        let layout = self.document.root_layout();
        if layout.len() != ids_before.len()
            || !layout
                .iter()
                .map(|(id, _)| *id)
                .eq(ids_before.iter().copied())
        {
            return false;
        }
        let mut first = usize::MAX;
        let mut last = 0usize;
        for root in changed {
            let Some(index) = layout.iter().position(|(id, _)| *id == root.entity_id()) else {
                return false;
            };
            first = first.min(index);
            last = last.max(index);
        }
        let start_index = if first > 0 && self.root_writing_matches_file(first - 1, &layout, cx) {
            // 接缝归前一块管，所以把它一起算进区段。
            first - 1
        } else {
            first
        };
        let Some(region_start) = layout[start_index].1.clone().map(|span| span.start) else {
            return false;
        };
        let Some(region_end) = layout[last].1.clone().map(|span| span.end) else {
            return false;
        };
        if region_end > self.buffer.byte_len() || region_start > region_end {
            return false;
        }
        let Some((text, local_spans)) = self
            .document
            .markdown_region_for_roots(start_index..last + 1, cx)
        else {
            return false;
        };
        if text.is_empty() {
            return false;
        }

        let delta = self.write_minimal_diff(region_start..region_end, &text);
        if delta != 0 {
            self.shift_root_spans_after(region_start, delta);
        }
        let roots: Vec<Entity<Block>> = self.document.root_blocks().to_vec();
        for (id, local) in local_spans {
            let span = region_start + local.start..region_start + local.end;
            let Some(block) = roots.iter().find(|block| block.entity_id() == id).cloned() else {
                continue;
            };
            self.reanchor_record_span(&block, span, cx);
        }
        true
    }

    /// 选区盖住的可见块各自往上找到根块，按文档顺序去重。
    fn selection_root_blocks(&self, cx: &App) -> Vec<Entity<Block>> {
        let mut covered: Vec<EntityId> = Vec::new();
        if let Some(normalized) = self.normalized_cross_block_selection(cx) {
            for index in normalized.start_index..=normalized.end_index {
                if let Some(visible) = self.document.visible_blocks().get(index) {
                    covered.push(visible.entity.entity_id());
                }
            }
        } else if let Some(target) = self.current_edit_target_from_state(cx) {
            covered.push(target.entity_id());
        }

        let mut roots: Vec<Entity<Block>> = Vec::new();
        for entity_id in covered {
            let Some(root) = self.document.root_ancestor_of(entity_id) else {
                continue;
            };
            if !roots
                .iter()
                .any(|existing| existing.entity_id() == root.entity_id())
            {
                roots.push(root);
            }
        }
        roots
    }

    /// 第 `index` 根块在文件里的字节，是否就是模型序列化出来的那份。
    ///
    /// Setext 标题、`1)` 序号、`#  标题` 这类写法模型只会写规范形：拿它当区段起点
    /// 会把用户没碰过的字节改掉，所以这种邻居不并进来（宁可少补那一行空行）。
    fn root_writing_matches_file(
        &self,
        index: usize,
        layout: &[(EntityId, Option<std::ops::Range<usize>>)],
        cx: &App,
    ) -> bool {
        let Some((entity_id, Some(span))) = layout.get(index).map(|(id, span)| (*id, span.clone()))
        else {
            return false;
        };
        let Some(entity) = self.document.block_entity_by_id(entity_id) else {
            return false;
        };
        span.end <= self.buffer.byte_len()
            && self.document.block_markdown_source(&entity, cx) == self.buffer.slice(span)
    }

    /// 键盘走这里：动作只带目标级别，落到 `apply_block_kind_to_selection`。
    pub(crate) fn apply_heading_level_to_selection(&mut self, level: u8, cx: &mut Context<Self>) {
        self.apply_block_kind_to_selection(BlockKindTarget::Heading(level), cx);
    }
}

macro_rules! heading_capture {
    ($handler:ident, $action:ident, $level:expr) => {
        impl Editor {
            pub(crate) fn $handler(
                &mut self,
                _: &crate::components::$action,
                _window: &mut Window,
                cx: &mut Context<Self>,
            ) {
                self.apply_heading_level_to_selection($level, cx);
            }
        }
    };
}

heading_capture!(on_heading1_capture, Heading1, 1);
heading_capture!(on_heading2_capture, Heading2, 2);
heading_capture!(on_heading3_capture, Heading3, 3);
heading_capture!(on_heading4_capture, Heading4, 4);
heading_capture!(on_heading5_capture, Heading5, 5);
heading_capture!(on_heading6_capture, Heading6, 6);

impl Editor {
    pub(crate) fn on_paragraph_text_capture(
        &mut self,
        _: &crate::components::ParagraphText,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_block_kind_to_selection(BlockKindTarget::Paragraph, cx);
    }
}

impl BlockKindTarget {
    /// 这一块换过去应该变成什么种类；不该动（已经一样、或本笔还不敢碰的种类）时 None。
    ///
    /// 引用与标注是容器：换进去要把每一行的前缀补上、换出去要把子块安置好，
    /// 那是 FP4 的第二段；表、代码块、公式块这类原子的结构块本身就不是「一段文字」。
    fn next_kind(self, current: &BlockKind) -> Option<BlockKind> {
        if current.is_atomic_structural() || current.is_quote_container() {
            return None;
        }
        let next = match self {
            Self::Heading(level) if *current == BlockKind::Heading { level } => {
                BlockKind::Paragraph
            }
            Self::Heading(level) => BlockKind::Heading { level },
            Self::Paragraph => BlockKind::Paragraph,
        };
        (next != *current).then_some(next)
    }
}
