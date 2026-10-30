//! 「插入」菜单那一档：在光标所在块之后（Front Matter 在文档最前）加一块，
//! 外加「格式 → 链接」这条写在行内的入口。
//!
//! 与换种类那一条入口（`paragraph_ops`）共用同一套规矩：一条命令一个撤销组，
//! 字节只在插入点那一处落笔，插入点之后的根块区间整体跟着挪位，其余文件的字节
//! 原样不动。

use gpui::*;

use std::path::PathBuf;

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
        if self.is_read_only() {
            return false;
        }

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
        // 可用性判定必须读 flush 之后的树：续建/物化可能改变选区到根块的映射，
        // 拿 flush 前的结论直接去取锚点会落空。
        if !self.insert_block_target_is_available(target, cx) {
            return false;
        }
        let roots = self.selection_root_blocks(cx);
        // 跨块选区插在整段选区后面：光标那头的根块。
        let Some(anchor) = roots.last().cloned() else {
            return false;
        };

        let layout: Vec<EntityId> = self
            .document
            .root_layout()
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        let Some(anchor_index) = layout.iter().position(|id| *id == anchor.entity_id()) else {
            // 根块不在布局里说明树刚被换过（物化/重载竞态）：这次不插，不 panic。
            return false;
        };
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
            self.finish_block_insertion(&block, target, false, cx);
            return true;
        };

        let (before, after) = self.insertion_seams(target, offset);
        let written = self.write_inserted_block_bytes(offset, before, after, &markdown, &block);
        self.finish_block_insertion(&block, target, written, cx);
        true
    }

    /// 插入块的收尾清单：按区间写回与整篇重投影兜底共用同一份顺序，
    /// 漏掉任何一步（聚焦、运行时注册表）都会留下「块在、打不进字」的状态。
    fn finish_block_insertion(
        &mut self,
        block: &Entity<Block>,
        target: InsertBlockTarget,
        written_back: bool,
        cx: &mut Context<Self>,
    ) {
        self.ensure_trailing_paragraph_after_structural(block, cx);
        if written_back {
            self.mark_dirty_written_back(cx);
        } else {
            self.mark_dirty(cx);
        }
        self.rebuild_image_runtimes(cx);
        self.focus_inserted_block(block, target, cx);
        self.finalize_pending_undo_capture(cx);
        cx.notify();
    }

    /// 「插入 → 图片」这一行现在能不能点：渲染态、写得动缓冲区，并且有一个编辑目标。
    /// 目标那块是不是正文不参与判断——落不进正文时由粘贴那条路改走块内替换。
    pub(crate) fn image_insert_is_available(&self, cx: &App) -> bool {
        if self.is_read_only() {
            return false;
        }

        self.view_mode == ViewMode::Rendered
            && self.writes_through_the_buffer()
            && self.current_edit_target_from_state(cx).is_some()
    }

    /// 把磁盘上的一张图片插到光标处。这一条同时服务拖放与「插入 → 图片」：
    /// 光标那一段怎么切成「前面 / 图片行 / 后面」由 `handle_paste_image_request` 定，
    /// 图片文件的落盘与相对路径写法也由它负责。返回值只说明有没有一个落点，
    /// 图片本身读不读得动由那一条路自己报告。
    pub(crate) fn insert_image_at_caret(&mut self, path: PathBuf, cx: &mut Context<Self>) -> bool {
        if self.is_read_only() {
            return false;
        }
        // 落点与拖放那条路一致：没有焦点块时退回文档第一块。
        let Some(block) = self
            .current_edit_target_from_state(cx)
            .or_else(|| self.document.first_root().cloned())
        else {
            let strings = cx.global::<crate::i18n::I18nManager>().strings().clone();
            self.show_message_modal(
                strings.insert_image.clone(),
                strings.insert_image_unsupported.clone(),
                cx,
            );
            return false;
        };
        let (leading, trailing) = block.update(cx, |block, _cx| block.paste_image_split());
        self.handle_paste_image_request(
            block,
            &leading,
            &crate::components::PastedImageSource::LocalPath(path),
            &trailing,
            cx,
        );
        true
    }

    /// 「插入 → 图片」选文件那一步：走原生文件选择器（本仓的守卫禁的是消息框，
    /// 原生选择器在放行名单里），选完交回上面那条入口。gpui 的测试壳把
    /// `prompt_for_paths` 写成 `unimplemented!()`，所以这一步用例不碰，测的是它交回的那条入口。
    pub(crate) fn open_image_picker(&mut self, cx: &mut Context<Self>) {
        if !self.image_insert_is_available(cx) {
            return;
        }
        let strings = cx.global::<crate::i18n::I18nManager>().strings().clone();
        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(strings.insert_image_prompt.into()),
            directory: self.open_dialog_start_dir(),
        });
        let weak_editor = cx.entity().downgrade();
        cx.spawn(async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let Ok(Ok(Some(paths))) = prompt.await else {
                return;
            };
            let _ = weak_editor.update(cx, |editor, cx| {
                editor.insert_picked_image(paths, cx);
            });
        })
        .detach();
    }

    /// 图片选择器交回的文件：找到第一个支持的图片插入；有文件但都不支持时
    /// 给出应用内提示（与粘贴路径同一口径），取消才保持静默。
    pub(crate) fn insert_picked_image(
        &mut self,
        paths: Vec<std::path::PathBuf>,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(path) = paths
            .into_iter()
            .find(|path| Block::is_supported_local_image_path(path))
        else {
            let strings = cx.global::<crate::i18n::I18nManager>().strings().clone();
            self.show_message_modal(
                strings.insert_image.clone(),
                strings.insert_image_unsupported.clone(),
                cx,
            );
            return false;
        };
        self.insert_image_at_caret(path, cx)
    }

    /// 「格式 → 链接」：把选中的那段包成 `[文字]()`，光标停在 `](` 之后等写地址。
    /// 链接文字不能跨块（markdown 的行内语法本来就不跨块），跨块选区按可见块逐段包，
    /// 全程只开一个撤销组——与行内格式同一条口径。
    pub(crate) fn insert_link_on_selection(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.link_insert_is_available(cx) {
            return false;
        }
        self.flush_pending_materialization(cx);
        let Some(normalized) = self.normalized_cross_block_selection(cx) else {
            let Some(target) = self.current_edit_target_from_state(cx) else {
                return false;
            };
            let range = target.read_with(cx, |block, _cx| block.selected_range.clone());
            // 单块交给块自己改：`Changed` 那一条路负责写回字节与收撤销组。
            return target.update(cx, |block, cx| block.wrap_visible_range_in_link(range, cx));
        };

        let block_count = normalized.end_index - normalized.start_index + 1;
        let mut changed = false;
        let mut caret_block: Option<Entity<Block>> = None;
        let mut caret_offset = 0usize;
        self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
        for position in 0..block_count {
            let index = normalized.start_index + position;
            let Some(entity) = self
                .document
                .visible_blocks()
                .get(index)
                .map(|visible| visible.entity.clone())
            else {
                continue;
            };
            let (start, end) = {
                // 选区存干净坐标，块那一侧认显示坐标。
                let block = entity.read(cx);
                let len = block.clean_visible_len();
                let start = if position == 0 {
                    normalized.start.offset.min(len)
                } else {
                    0
                };
                let end = if position + 1 == block_count {
                    normalized.end.offset.min(len)
                } else {
                    len
                };
                let range = block.clean_range_to_display_range(start..end);
                (range.start, range.end)
            };
            if start >= end {
                continue;
            }
            if entity.update(cx, |block, cx| {
                block.wrap_visible_range_in_link(start..end, cx)
            }) {
                changed = true;
                if caret_block.is_none() {
                    // 光标交给最靠近选区起点的那一段：地址从那一段写起。可见坐标是字节偏移，
                    // 包完是 `[文字]()`，落在 `](` 之后即 `start + 1 + 文字长度 + 2`。
                    caret_block = Some(entity.clone());
                    caret_offset = start + 1 + (end - start) + 2;
                }
            }
        }
        if let Some(block) = caret_block {
            self.focus_block(block.entity_id());
            let offset = caret_offset;
            block.update(cx, |block, _cx| {
                block.assign_collapsed_selection_offset(
                    offset,
                    CollapsedCaretAffinity::Default,
                    None,
                );
            });
        }
        self.finalize_pending_undo_capture(cx);
        if changed {
            cx.notify();
        }
        changed
    }

    /// 「格式 → 链接」这一行现在能不能点：渲染态、写得动缓冲区，并且选区真的选中了字。
    /// 只有光标时点不动——空的 `[]()` 在行内树里存不住（重读时被当成空链接丢掉），
    /// 与格式那一档其余八行同一条口径。
    pub(crate) fn link_insert_is_available(&self, cx: &App) -> bool {
        if self.is_read_only() {
            return false;
        }

        if self.view_mode != ViewMode::Rendered || !self.writes_through_the_buffer() {
            return false;
        }
        if let Some(normalized) = self.normalized_cross_block_selection(cx) {
            return normalized.end_index != normalized.start_index
                || normalized.end.offset > normalized.start.offset;
        }
        self.current_edit_target_from_state(cx)
            .is_some_and(|target| !target.read(cx).selected_range.is_empty())
    }

    /// ⌘K 走这里。链接的写法在行内，块自己看不到别的块，跨块选区只能在编辑器层收掉；
    /// 单块的情况由 `insert_link_on_selection` 内部转回块那条路径。
    pub(crate) fn on_link_selection_capture(
        &mut self,
        _: &crate::components::LinkSelection,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.insert_link_on_selection(cx) {
            cx.stop_propagation();
        }
    }

    /// ⌘⇧I 走这里：只负责打开选择器，选完由 `open_image_picker` 的回调交回插入那条入口。
    pub(crate) fn on_insert_image_capture(
        &mut self,
        _: &crate::components::InsertImage,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.image_insert_is_available(cx) {
            self.open_image_picker(cx);
            cx.stop_propagation();
        }
    }

    /// 新块那份记录。代码块与数学块的内容都存在块自己的标题里（序列化那一侧按
    /// 这一族补围栏行），所以「空代码块」就是空标题 + 那一族种类。
    fn record_for_target(&self, target: InsertBlockTarget) -> Option<BlockRecord> {
        let record = match target {
            InsertBlockTarget::CodeBlock => {
                BlockRecord::with_plain_text(BlockKind::CodeBlock { language: None }, String::new())
            }
            // 公式块与代码块同一条路：模型里只存正文，两行围栏由序列化那一侧补回来，
            // 所以插出来的是一块空正文（写进文件是 `$$\n\n$$`），光标落在正文 0 位。
            InsertBlockTarget::MathBlock => BlockRecord::math_body(String::new()),
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
            InsertBlockTarget::MathBlock => 0,
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
