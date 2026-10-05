//! 「插入」菜单那一档：在光标所在块之后（Front Matter 在文档最前）加一块。
//!
//! 与换种类那一条入口（`paragraph_ops`）共用同一套规矩：一条命令一个撤销组，
//! 字节只在插入点那一处落笔，插入点之后的根块区间整体跟着挪位，其余文件的字节
//! 原样不动。

use gpui::*;

use super::Editor;
use crate::components::{BlockKind, BlockRecord, CollapsedCaretAffinity, UndoCaptureKind};
use crate::editor::{Block, ViewMode};

/// 「插入」菜单的目标块。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InsertBlockTarget {
    /// 空的围栏代码块，光标落进围栏里那一行。
    CodeBlock,
    /// 空的展示公式块（`$$ … $$`）。
    MathBlock,
    /// 分割线（`---`）。
    Separator,
    /// 目录（`[toc]` 那一行，条目由渲染层现算）。
    Toc,
    /// YAML Front Matter，只能在文档最前面。
    FrontMatter,
}

impl Editor {
    /// 现在能不能插这一类：得是渲染态、写得动缓冲区，而且选区落在某一块上。
    /// Front Matter 还要看这份文档有没有已经带着一份（一篇只能有一份，且必须在最前）。
    pub(crate) fn insert_block_target_is_available(
        &self,
        target: InsertBlockTarget,
        cx: &App,
    ) -> bool {
        if self.view_mode != ViewMode::Rendered || !self.writes_through_the_buffer() {
            return false;
        }
        if self.selection_root_blocks(cx).is_empty() {
            return false;
        }
        match target {
            // 一篇文档只能有一份 Front Matter，而且必须在最前面。
            InsertBlockTarget::FrontMatter => !self
                .document
                .root_blocks()
                .iter()
                .any(|root| root.read(cx).kind() == BlockKind::FrontMatter),
            _ => true,
        }
    }

    /// 在当前选区后面插入一块，返回是否改到了内容。
    pub(crate) fn insert_block_after_selection(
        &mut self,
        target: InsertBlockTarget,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.insert_block_target_is_available(target, cx) {
            return false;
        }
        self.flush_pending_materialization(cx);
        let roots = self.selection_root_blocks(cx);
        // 跨块选区插在整段选区后面：光标那头的根块。
        let anchor = roots.last().cloned().expect("可用性已经把空选区挡在外面");

        let layout: Vec<EntityId> = self
            .document
            .root_layout()
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        let anchor_index = layout
            .iter()
            .position(|id| *id == anchor.entity_id())
            .expect("根块来自根块布局");
        let anchor_span = self.document.source_span_of(anchor.entity_id());

        let Some(record) = self.record_for_target(target) else {
            return false;
        };
        let block = Self::new_block(cx, record);
        let markdown = self.document.block_markdown_source(&block, cx);
        if markdown.is_empty() {
            return false;
        }

        self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
        // Front Matter 只能在文档最前面（解析器认第一行那对 `---`），其余插在锚点块后面。
        let (insert_index, insert_offset) = match target {
            InsertBlockTarget::FrontMatter => (0, Some(0)),
            _ => (anchor_index + 1, anchor_span.clone().map(|span| span.end)),
        };
        self.document
            .insert_blocks_at(None, insert_index, vec![block.clone()], cx);
        let Some(offset) = insert_offset else {
            // 锚点块没有源码区间（后台续建到一半）：字节没法按插入点算，整篇重投影兜底。
            self.ensure_trailing_paragraph_after_structural(&block, cx);
            self.mark_dirty(cx);
            self.finalize_pending_undo_capture(cx);
            cx.notify();
            return true;
        };

        let (before, after) = self.insertion_seams(target, offset);
        let written = self.write_inserted_block_bytes(offset, before, after, &markdown, &block);
        if written {
            self.ensure_trailing_paragraph_after_structural(&block, cx);
            self.mark_dirty_written_back(cx);
        } else {
            self.ensure_trailing_paragraph_after_structural(&block, cx);
            self.mark_dirty(cx);
        }
        self.rebuild_image_runtimes(cx);
        self.focus_inserted_block(&block, target, cx);
        self.finalize_pending_undo_capture(cx);
        cx.notify();
        true
    }

    /// 新块那份记录。代码块与数学块的内容都存在块自己的标题里（序列化那一侧按
    /// 这一族补围栏行），所以「空代码块」就是空标题 + 那一族种类。
    fn record_for_target(&self, target: InsertBlockTarget) -> Option<BlockRecord> {
        let record = match target {
            InsertBlockTarget::CodeBlock => {
                BlockRecord::with_plain_text(BlockKind::CodeBlock { language: None }, String::new())
            }
            // 两条记号紧挨着：`$$\n\n$$` 这种中间空一行的写法解析器不认（会被切成两块
            // 原始 markdown），空公式只能写成一前一后两行。
            InsertBlockTarget::MathBlock => BlockRecord::math("$$\n$$".to_string()),
            InsertBlockTarget::Separator => {
                BlockRecord::with_plain_text(BlockKind::Separator, String::new())
            }
            InsertBlockTarget::Toc => BlockRecord::paragraph("[toc]".to_string()),
            // 两条 `---` 之间留一个空行：光标落在这一行打字才写进信息区，不会顶到闭合那行上。
            InsertBlockTarget::FrontMatter => BlockRecord::front_matter("---\n\n---".to_string()),
        };
        Some(record)
    }

    /// 新块两边那段接缝，返回（前面、后面）。根块之间空一行；这一档插进来的五类都不是
    /// 列表项，用不着「同组相邻两项不空行」那条规则。Front Matter 反过来：前面没有接缝，
    /// 空行补在它后面——解析器只认第一行那对 `---`，它必须顶到 0 位。
    fn insertion_seams(
        &self,
        target: InsertBlockTarget,
        offset: usize,
    ) -> (&'static str, &'static str) {
        if target == InsertBlockTarget::FrontMatter {
            return ("", "\n\n");
        }
        if offset == 0 {
            // 文档本来就是空的：新块前面不需要接缝。
            return ("", "");
        }
        ("\n\n", "")
    }

    /// 落笔：插入点写入 `接缝 + 新块的 markdown + 接缝`，之后的根块区间挪位，
    /// 新块挂上自己那一段（不含两边接缝）。
    fn write_inserted_block_bytes(
        &mut self,
        offset: usize,
        before: &str,
        after: &str,
        markdown: &str,
        block: &Entity<Block>,
    ) -> bool {
        if offset > self.buffer.byte_len() {
            return false;
        }
        let text = format!("{before}{markdown}{after}");
        let applied = self.buffer.edit(offset..offset, &text);
        self.record_buffer_edit(applied);
        let delta = text.len() as i64;
        self.shift_root_spans_after(offset, delta);
        let start = offset + before.len();
        self.document
            .set_source_span(block.entity_id(), start..start + markdown.len());
        true
    }

    /// 插完把光标交给新块：代码块落在围栏里那一行，公式块与 Front Matter 落在
    /// 两条记号中间那一行，目录落在 `[toc]` 末尾，分割线停在自己那行。
    fn focus_inserted_block(
        &mut self,
        block: &Entity<Block>,
        target: InsertBlockTarget,
        cx: &mut Context<Self>,
    ) {
        let caret = match target {
            InsertBlockTarget::CodeBlock => 0,
            InsertBlockTarget::MathBlock => "$$\n".len(),
            InsertBlockTarget::FrontMatter => "---\n".len(),
            InsertBlockTarget::Toc => "[toc]".len(),
            InsertBlockTarget::Separator => 0,
        };
        self.focus_block(block.entity_id());
        block.update(cx, |block, _cx| {
            block.assign_collapsed_selection_offset(caret, CollapsedCaretAffinity::Default, None);
        });
    }
}
