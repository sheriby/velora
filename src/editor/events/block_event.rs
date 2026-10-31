use super::*;
use crate::editor::{CrossBlockSelection, CrossBlockSelectionEndpoint};

impl Editor {
    /// 本块在可见序里的位置；不可见块按旧口径当 0 号（折叠区块发不出这些事件，
    /// 0 号只是兜底）。查增量索引 O(1)，不克隆可见块表。
    fn current_visible_index_of(&self, entity_id: EntityId) -> usize {
        self.document
            .visible_index_for_entity_id(entity_id)
            .unwrap_or(0)
    }

    /// 按方向取可见序里相邻的块，跳过画不出东西的块（与
    /// `vertical_neighbor_index` 同一条规则）。只要一个实体，不为它整篇克隆。
    fn vertical_neighbor_entity(
        &self,
        entity_id: EntityId,
        delta: isize,
        cx: &mut Context<Self>,
    ) -> Option<Entity<crate::editor::Block>> {
        let from = self.current_visible_index_of(entity_id);
        let visible = self.document.visible_blocks();
        let target_index =
            self.vertical_neighbor_index(visible, from, delta, cx)?;
        visible.get(target_index).map(|visible| visible.entity.clone())
    }

    /// Shift+↑/↓ 在块边缘继续扩选：焦点落到相邻块，选区锚点保持不动。
    /// 已有跨块选区时沿用它的锚点；否则以当前块的光标为锚。
    fn extend_selection_vertically_across_blocks(
        &mut self,
        block: &Entity<crate::editor::Block>,
        direction: i32,
        preferred_x: Pixels,
        cx: &mut Context<Self>,
    ) {
        let Some(target) =
            self.vertical_neighbor_entity(block.entity_id(), direction as isize, cx)
        else {
            return;
        };
        // 表格容器没有文本元素，也建不出跨块端点：扩选停在表格前。
        if target.read(cx).kind() == BlockKind::Table {
            return;
        }
        let offset = target
            .read(cx)
            .entry_offset_for_vertical_focus(direction < 0, Some(preferred_x));
        let anchor = self
            .cross_block_selection
            .map(|selection| selection.anchor)
            .unwrap_or_else(|| {
                let block_ref = block.read(cx);
                CrossBlockSelectionEndpoint {
                    entity_id: block.entity_id(),
                    offset: block_ref.current_to_clean_offset(block_ref.cursor_offset()),
                }
            });
        let focus = {
            let target_ref = target.read(cx);
            CrossBlockSelectionEndpoint {
                entity_id: target.entity_id(),
                offset: target_ref.current_to_clean_offset(offset),
            }
        };
        self.cross_block_selection = Some(CrossBlockSelection { anchor, focus });
        self.focus_block(target.entity_id());
        target.update(cx, |target, cx| {
            target.move_to_with_preferred_x(offset, Some(preferred_x), cx);
        });
        self.sync_cross_block_selection_visuals(cx);
        cx.notify();
    }

    /// Handles all block-originated editor events against the current cached
    /// visible-order snapshot.
    pub(crate) fn on_block_event(
        &mut self,
        block: Entity<crate::editor::Block>,
        event: &BlockEvent,
        cx: &mut Context<Self>,
    ) {
        if self.is_read_only()
            && !matches!(
                event,
                BlockEvent::CompositionChanged
                    | BlockEvent::RequestToggleFold
                    | BlockEvent::RequestJumpToHeadingLine { .. }
                    | BlockEvent::RequestRenderedSelectAll
                    | BlockEvent::RequestMermaidPreview { .. }
                    | BlockEvent::RequestOpenWikilink { .. }
                    | BlockEvent::RequestOpenLink { .. }
                    | BlockEvent::RequestJumpToFootnoteDefinition { .. }
                    | BlockEvent::RequestJumpToFootnoteBackref { .. }
                    | BlockEvent::RequestTableCellMoveHorizontal { .. }
                    | BlockEvent::RequestTableCellMoveVertical { .. }
                    | BlockEvent::RequestTableAxisPreview { .. }
                    | BlockEvent::RequestSelectTableAxis { .. }
                    | BlockEvent::RequestOpenTableAxisMenu { .. }
                    | BlockEvent::RequestFocusPrev { .. }
                    | BlockEvent::RequestFocusNext { .. }
                    | BlockEvent::RequestBlockUp
                    | BlockEvent::RequestBlockDown
                    | BlockEvent::RequestFocus
                    // 只读文档也要能用 Shift+↑/↓ 扩选（纯阅读导航）。
                    | BlockEvent::RequestSelectVertical { .. }
                    | BlockEvent::RequestFocusTableEntry { .. }
            )
        {
            return;
        }
        if matches!(event, BlockEvent::CompositionChanged) {
            // 组合跨度的唯一出口 `set_marked_range` 保证每次变化都发这个事件，
            // 所以这里维护的集合就是「此刻谁挂着组合文本」的唯一账本：
            // `has_marked_document_text` 每次调度只查它，不再逐块读实体。
            let id = block.entity_id();
            let marked = block.read(cx).has_marked_text();
            self.composition_blocks
                .retain(|tracked| tracked.entity_id() != id && tracked.upgrade().is_some());
            if marked {
                self.composition_blocks.push(block.downgrade());
            }
            if self.focusable_entity_by_id(block.entity_id()).is_some() {
                if !block.read(cx).has_marked_text() {
                    self.finish_ime_composition(cx);
                    self.schedule_autosave(cx);
                }
                cx.notify();
            }
            return;
        }
        if let BlockEvent::PrepareUndo { kind, selection } = event {
            self.prepare_block_undo_capture(&block, *kind, selection, cx);
            return;
        }

        if let BlockEvent::RequestReplaceCrossBlockSelection {
            text,
            selected_range_relative,
            mark_inserted_text,
            undo_kind,
        } = event
            && self.replace_cross_block_selection_with_text(
                text,
                selected_range_relative.clone(),
                *mark_inserted_text,
                *undo_kind,
                cx,
            )
        {
            return;
        }

        if matches!(event, BlockEvent::RequestRenderedSelectAll) {
            self.on_rendered_select_all_press(block, cx);
            return;
        }

        if let BlockEvent::RequestPasteImage {
            leading,
            source,
            trailing,
        } = event
        {
            self.handle_paste_image_request(block, leading, source, trailing, cx);
            return;
        }

        if let Some(binding) = self.table_cell_binding(block.entity_id()) {
            self.on_table_cell_event(binding, event, cx);
            return;
        }

        if Self::block_event_clears_cross_block_selection(event) {
            self.rendered_select_all_cycle = None;
            self.clear_cross_block_selection(cx);
        }

        // 文本改动走自己的那条：它只需要「可见序列有没有变」，而整篇克隆要付每个
        // Entity 句柄的引用计数与 LeakDetector 登记——10 MiB 档 159,683 块克隆一次
        // 实测 17ms，占满一次按键。身份序列是 `Copy` 的 `EntityId`，比它便宜两个
        // 数量级；「前一根/后一根实体」只有结构事件要。
        if matches!(event, BlockEvent::Changed) {
            self.on_block_changed(block, cx);
            return;
        }

        // 结构事件要按区间写回：先记下根块布局，变更后再算「哪几根被换成了哪几根」。
        let roots_before = matches!(
            event,
            BlockEvent::RequestNewline { .. }
                | BlockEvent::RequestMergeIntoPrev { .. }
                | BlockEvent::RequestMergeFromNext
                | BlockEvent::ToggleTaskChecked
                | BlockEvent::RequestIndent
                | BlockEvent::RequestOutdent
                | BlockEvent::RequestQuoteBreak
                | BlockEvent::RequestCalloutBreak
                | BlockEvent::RequestDelete
        )
        .then(|| self.document.root_layout());

        match event {
            // 文本改动在函数开头就转给 `on_block_changed` 了：这个空分支只为
            // 让 match 保持穷尽，好让「结构事件」那一组一眼可数。
            BlockEvent::Changed => {}
            BlockEvent::RequestToggleFold => {
                let folding_on = !block.read(cx).folded;
                block.update(cx, |block, _cx| block.folded = folding_on);
                self.fold_state_version = self.fold_state_version.wrapping_add(1);
                if folding_on {
                    self.refocus_caret_hidden_by_fold(&block, cx);
                }
                cx.notify();
            }
            BlockEvent::RequestJumpToHeadingLine { line } => {
                self.jump_to_source_line(*line, cx);
            }
            BlockEvent::RequestFormulaEditor => {
                // 块事件链没有 &mut Window：记下目标，下一帧 apply 时开弹窗
                // 并把焦点交给草稿输入。
                self.pending_formula_editor = Some(block.entity_id());
                cx.notify();
            }
            BlockEvent::RequestMermaidPreview { markdown } => {
                self.open_mermaid_preview(markdown, cx);
            }
            BlockEvent::RequestNewline {
                trailing,
                source_already_mutated,
            } => {
                // 上面那次 Changed 没有落笔（切下的那截留给这一步写），标记必须在这里
                // 收走：既不留在块上影响下一次编辑，也让下面几个提前返回有得兜底。
                let split_truncated = block.update(cx, |block, _cx| block.take_split_truncation());
                // Typing a setext underline (`=====`/`-----`) under a paragraph
                // and pressing Enter turns that paragraph into a heading, the
                // same way the importer treats the two adjacent lines.
                if self.try_form_setext_heading_on_newline(&block, cx) {
                    return;
                }
                // Typing a delimiter row under a header forms a native table,
                // and typing further pipe rows below the table absorbs them.
                if self.try_form_or_extend_table_on_newline(&block, cx) {
                    return;
                }
                let Some(location) = self.document.find_block_location(block.entity_id()) else {
                    if split_truncated {
                        self.mark_dirty(cx);
                    }
                    return;
                };
                if !source_already_mutated {
                    self.prepare_undo_capture(
                        crate::components::UndoCaptureKind::NonCoalescible,
                        cx,
                    );
                }
                let current_kind = block.read(cx).kind();
                // 源码分块文档：新块与当前块同类（CodeBlock/原始段落），
                // 才能保持整篇源码等宽直编的形态。
                let sibling_kind = if self.view_mode == crate::editor::ViewMode::Source {
                    current_kind.clone()
                } else {
                    current_kind.newline_sibling_kind()
                };
                let marker = block.read(cx).record.list_marker;
                let mut record = BlockRecord::new(sibling_kind.clone(), trailing.clone());
                // 拆出来的那半截还排在同一个列表里：记号的写法要跟着走，不然用户写的
                // `1)`/`+` 在回车那一刻只有前半截保得住。
                if sibling_kind.is_list_item() {
                    record.list_marker = marker;
                }
                let new_block = Self::new_block(cx, record);
                if self.view_mode == crate::editor::ViewMode::Source {
                    new_block.update(cx, |block, _cx| block.set_source_document_mode());
                }
                self.document.insert_blocks_at(
                    location.parent,
                    location.index + 1,
                    vec![new_block.clone()],
                    cx,
                );
                if self.view_mode == crate::editor::ViewMode::Source {
                    self.wire_source_block_after_newline(&new_block, &block, cx);
                }
                self.focus_block(new_block.entity_id());
                let quote_normalized = current_kind.is_quote_container();
                if quote_normalized {
                    // normalize 里那次重同步之前先按区间落笔，否则它会拿整篇序列化
                    // 盖掉未编辑的块（同 RequestQuoteBreak 的处理）。
                    let wrote_back =
                        self.write_back_structural_change(&block, roots_before.as_deref(), cx);
                    self.skip_next_resync = wrote_back;
                    self.normalize_rendered_quote_structure(&block, cx);
                    if wrote_back {
                        self.mark_dirty_written_back(cx);
                    } else {
                        self.mark_dirty(cx);
                    }
                } else {
                    if self.write_back_structural_change(&block, roots_before.as_deref(), cx) {
                        self.mark_dirty_written_back(cx);
                    } else {
                        self.mark_dirty(cx);
                    }
                    // 引用那一路 normalize 自己会刷；这里要在缓冲区落笔之后再刷，
                    // 定义才不是改之前那一份。
                    self.rebuild_image_runtimes(cx);
                }
                self.finalize_pending_undo_capture(cx);
                cx.notify();
            }
            BlockEvent::RequestOpenWikilink { target } => {
                if self.ai_result_preview {
                    let destination = if crate::file_url::is_markdown_document(std::path::Path::new(&target)) { target.clone() } else { format!("{target}.md") };
                    self.defer_open_link(destination, cx); return;
                }
                // `[[wikilink]]` 点击：打开工作区同名文件，缺失则在根目录创建
                // （roadmap C3）。defer 到当前更新完成后执行（打开流程需要
                // &mut Window，且不能在本次窗口更新内重入）。
                if let Some(any_handle) = self.window_handle
                    && let Some(handle) = crate::ui::window_content::<Editor>(any_handle, cx)
                {
                    let deferred_target = target.clone();
                    cx.defer(move |cx| {
                        // 窗口/实体在 defer 期间消失是正常的竞态；但失败要有痕迹，
                        // 不能连日志都没有（约定：不要静默吞错）。
                        if let Err(error) = handle.update(cx, |editor, window, cx| {
                            editor.open_wikilink(deferred_target.clone(), window, cx);
                        }) {
                            eprintln!("跳转双链失败（窗口已关闭？）：{error}");
                        }
                    });
                }
            }
            BlockEvent::RequestEnterCalloutBody => {
                let needs_body = block.read(cx).children.is_empty();
                if needs_body {
                    self.prepare_undo_capture(
                        crate::components::UndoCaptureKind::NonCoalescible,
                        cx,
                    );
                }
                let created = self.ensure_callout_body_entry(&block, cx);
                if let Some(body) = created {
                    self.focus_block(body.entity_id());
                    if needs_body {
                        // 只有真正新建正文块才算结构变更；纯焦点移动（已有正文）
                        // 不走按键路径的整篇注册表重建。
                        self.rebuild_image_runtimes(cx);
                        self.mark_dirty(cx);
                        self.finalize_pending_undo_capture(cx);
                    }
                    cx.notify();
                }
            }
            BlockEvent::RequestQuoteBreak => {
                let Some((parent, insert_index)) =
                    self.quote_break_insertion_target(block.entity_id(), cx)
                else {
                    return;
                };

                self.prepare_undo_capture(crate::components::UndoCaptureKind::NonCoalescible, cx);
                // 拆引用改的是这一根引用（它在容器里时是那一整根容器），锚点取根块。
                let region_anchor = self
                    .document
                    .root_ancestor_of(block.entity_id())
                    .unwrap_or_else(|| block.clone());

                let new_quote = Self::new_block(
                    cx,
                    BlockRecord::new(BlockKind::Quote, InlineTextTree::plain(String::new())),
                );
                let blocks = if parent.is_none() {
                    vec![new_quote.clone()]
                } else {
                    vec![
                        Self::new_block(cx, BlockRecord::paragraph(String::new())),
                        new_quote.clone(),
                    ]
                };
                self.document
                    .insert_blocks_at(parent, insert_index, blocks, cx);
                self.focus_block(new_quote.entity_id());
                // 先把这一段按区间落进缓冲区，再声明「缓冲区已经是目标状态」：
                // normalize 里那次重同步 otherwise 会拿整篇重新序列化盖掉未编辑的块。
                let wrote_back =
                    self.write_back_structural_change(&region_anchor, roots_before.as_deref(), cx);
                self.skip_next_resync = wrote_back;
                self.normalize_rendered_quote_structure(&block, cx);
                if wrote_back {
                    self.mark_dirty_written_back(cx);
                } else {
                    self.mark_dirty(cx);
                }
                self.finalize_pending_undo_capture(cx);
                cx.notify();
            }
            BlockEvent::RequestCalloutBreak => {
                let Some((parent, insert_index)) =
                    self.callout_break_insertion_target(block.entity_id(), cx)
                else {
                    return;
                };

                self.prepare_undo_capture(crate::components::UndoCaptureKind::NonCoalescible, cx);
                let region_anchor = self
                    .document
                    .root_ancestor_of(block.entity_id())
                    .unwrap_or_else(|| block.clone());
                let plain = Self::new_block(cx, BlockRecord::paragraph(String::new()));
                let blocks = if parent.is_none() {
                    vec![plain.clone()]
                } else {
                    vec![
                        Self::new_block(cx, BlockRecord::paragraph(String::new())),
                        plain.clone(),
                    ]
                };
                self.document
                    .insert_blocks_at(parent, insert_index, blocks, cx);
                self.focus_block(plain.entity_id());
                self.rebuild_image_runtimes(cx);
                if self.write_back_structural_change(&region_anchor, roots_before.as_deref(), cx) {
                    self.mark_dirty_written_back(cx);
                } else {
                    self.mark_dirty(cx);
                }
                self.finalize_pending_undo_capture(cx);
                cx.notify();
            }
            BlockEvent::RequestMergeIntoPrev { content } => {
                // 只要「前一块」这一个实体：查增量可见索引即可，整篇克隆是
                // O(文档) 的句柄账单。
                let current_visible_index = self
                    .document
                    .visible_index_for_entity_id(block.entity_id())
                    .unwrap_or(0);
                if current_visible_index == 0 {
                    return;
                }
                let Some(prev) = self
                    .document
                    .visible_blocks()
                    .get(current_visible_index - 1)
                    .map(|visible| visible.entity.clone())
                else {
                    return;
                };
                let quote_related = self.block_is_quote_structure_related(&block, cx)
                    || self.block_is_quote_structure_related(&prev, cx);
                self.prepare_undo_capture(crate::components::UndoCaptureKind::NonCoalescible, cx);

                let cursor_pos = prev.read(cx).display_text().len();
                let adopted_children = crate::editor::tree::DocumentTree::take_children(&block, cx);
                let removed_entity_id = block.entity_id();

                self.document.with_structure_mutation(cx, |document, cx| {
                    prev.update(cx, {
                        let content = content.clone();
                        let adopted_children = adopted_children.clone();
                        move |prev, cx| {
                            let mut next_title = prev.record.title.clone();
                            next_title.append_tree(content.clone());
                            prev.record.set_title(next_title);
                            prev.sync_render_cache();
                            prev.children.extend(adopted_children.clone());
                            prev.selected_range = cursor_pos..cursor_pos;
                            prev.selection_reversed = false;
                            prev.marked_range = None;
                            prev.vertical_motion_x = None;
                            prev.cursor_blink_epoch = Instant::now();
                            cx.notify();
                        }
                    });
                    let _ = document.remove_block_by_id_raw(removed_entity_id, cx);
                });

                self.focus_block(prev.entity_id());
                if quote_related {
                    self.normalize_rendered_quote_structure(&block, cx);
                } else {
                    self.rebuild_image_runtimes(cx);
                }
                // 锚点是**前一块**：它吸收了本块，被换掉的区间是「前一块 + 本块」。
                if self.write_back_structural_change(&prev, roots_before.as_deref(), cx) {
                    self.mark_dirty_written_back(cx);
                } else {
                    self.mark_dirty(cx);
                }
                self.finalize_pending_undo_capture(cx);
                cx.notify();
            }
            BlockEvent::RequestMergeFromNext => {
                // 源码分块文档：块尾前向删除，把下一块并入本块（即删除
                // 块边界换行）。光标落在接缝处。
                let current_visible_index = self.current_visible_index_of(block.entity_id());
                let Some(next) = self
                    .document
                    .visible_blocks()
                    .get(current_visible_index + 1)
                    .map(|visible| visible.entity.clone())
                else {
                    return;
                };
                if next.read(cx).kind() == BlockKind::Table {
                    // 表格的内容在 record.table 里，并进上一块会把它丢掉；
                    // 块尾前向删除对表格就是整块删除。
                    next.update(cx, |_next, cx| cx.emit(BlockEvent::RequestDelete));
                    return;
                }
                self.prepare_undo_capture(crate::components::UndoCaptureKind::NonCoalescible, cx);

                let cursor_pos = block.read(cx).display_text().len();
                let adopted_children = crate::editor::tree::DocumentTree::take_children(&next, cx);
                let removed_entity_id = next.entity_id();
                let next_title = next.read(cx).record.title.clone();

                self.document.with_structure_mutation(cx, |document, cx| {
                    block.update(cx, {
                        let next_title = next_title.clone();
                        let adopted_children = adopted_children.clone();
                        move |block, cx| {
                            let mut merged = block.record.title.clone();
                            merged.append_tree(next_title);
                            block.record.set_title(merged);
                            block.sync_render_cache();
                            block.children.extend(adopted_children);
                            block.selected_range = cursor_pos..cursor_pos;
                            block.selection_reversed = false;
                            block.marked_range = None;
                            block.vertical_motion_x = None;
                            block.cursor_blink_epoch = Instant::now();
                            cx.notify();
                        }
                    });
                    let _ = document.remove_block_by_id_raw(removed_entity_id, cx);
                });

                self.focus_block(block.entity_id());
                // 注册表在落笔之后刷新（读的是缓冲区，写回前调是旧字节）。
                if self.write_back_structural_change(&block, roots_before.as_deref(), cx) {
                    self.mark_dirty_written_back(cx);
                } else {
                    self.mark_dirty(cx);
                }
                self.rebuild_image_runtimes(cx);
                self.finalize_pending_undo_capture(cx);
                cx.notify();
            }
            BlockEvent::RequestPasteMultiline {
                leading,
                lines,
                trailing,
            } => {
                if lines.is_empty() {
                    return;
                }
                let soft_wrapped =
                    crate::components::markdown::paste::is_soft_wrapped_paste(lines);
                let quote_related = self.block_is_quote_structure_related(&block, cx);
                // 引用里的结构粘贴会往容器后面插入新的根块：这一段的写回区间从
                // 粘贴前就定下来，收尾按「引用根 + 新根」整体落笔，别处不动。
                let roots_before = self.document.root_layout();
                let quote_root_before = self.document.root_ancestor_of(block.entity_id());
                // 粘贴先按「在光标处插入一段文本」处理：能算出插入点就走缓冲区插入，
                // 块结构由导入器从缓冲区重新认。引用/标注里的粘贴要跟着容器自己那套
                // 规范化走，留在下面的整篇重投影路径上。
                if !quote_related
                    && self.paste_multiline_through_buffer(
                        &block,
                        leading,
                        lines,
                        trailing,
                        soft_wrapped,
                        cx,
                    )
                {
                    return;
                }
                self.prepare_undo_capture(crate::components::UndoCaptureKind::NonCoalescible, cx);

                let current_kind = block.read(cx).kind();
                // 连续非空行且没有块级结构：就是一段的软换行，直接并回当前
                // 段落，别拆块也别补空行。
                if soft_wrapped {
                    let mut title = leading.clone();
                    title.append_tree(InlineTextTree::from_markdown(&lines.join("\n")));
                    title.append_tree(trailing.clone());
                    let cursor = title.visible_len();
                    Self::set_block_title_and_kind(&block, current_kind, title, cursor, cx);
                    self.focus_block(block.entity_id());
                    if quote_related {
                        self.normalize_rendered_quote_structure(&block, cx);
                    } else {
                        self.rebuild_image_runtimes(cx);
                    }
                    self.mark_dirty(cx);
                    self.finalize_pending_undo_capture(cx);
                    cx.notify();
                    return;
                }

                let leading_empty = leading.visible_len() == 0;
                let mut first_title = leading.clone();
                let tail_lines = lines.clone();
                if tail_lines.is_empty() {
                    first_title.append_tree(trailing.clone());
                    let cursor = first_title.visible_len();
                    Self::set_block_title_and_kind(&block, current_kind, first_title, cursor, cx);
                    self.focus_block(block.entity_id());
                    if quote_related {
                        self.normalize_rendered_quote_structure(&block, cx);
                    } else {
                        self.rebuild_image_runtimes(cx);
                    }
                    self.mark_dirty(cx);
                    self.finalize_pending_undo_capture(cx);
                    cx.notify();
                    return;
                }

                let cursor = first_title.visible_len();
                Self::set_block_title_and_kind(&block, current_kind, first_title, cursor, cx);

                let Some(location) = self.document.find_block_location(block.entity_id()) else {
                    return;
                };

                // 结构块（表格、围栏、容器）由导入器整体解析；连续行按同一段
                // 合并，空行才断块。
                let mut inserted_roots = Self::build_blocks_from_lines(cx, &tail_lines);
                if trailing.visible_len() > 0 {
                    inserted_roots.push(Self::new_block(cx, BlockRecord::paragraph(String::new())));
                }
                self.document.insert_blocks_at(
                    location.parent,
                    location.index + 1,
                    inserted_roots.clone(),
                    cx,
                );
                self.rebuild_table_runtimes(cx);

                // A structural block pasted at the very end of the document leaves
                // no line below it; remember that so a trailing paragraph can be
                // added once the paste (and any quote normalization) settles.
                let inserted_at_doc_end = inserted_roots.last().is_some_and(|last| {
                    self.document
                        .find_block_location(last.entity_id())
                        .is_some_and(|location| {
                            location.parent.is_none()
                                && location.index + 1 >= self.document.root_count()
                        })
                });

                if let Some(last_root) = inserted_roots.last() {
                    let focus_block = if last_root.read(cx).kind() == BlockKind::Table {
                        last_root
                            .read(cx)
                            .table_runtime
                            .as_ref()
                            .and_then(|runtime| {
                                runtime
                                    .rows
                                    .last()
                                    .and_then(|row| row.last())
                                    .cloned()
                                    .or_else(|| runtime.header.last().cloned())
                            })
                    } else {
                        self.document.last_visible_descendant(last_root.entity_id())
                    };
                    let Some(focus_block) = focus_block else {
                        return;
                    };
                    focus_block.update(cx, {
                        let trailing = trailing.clone();
                        move |focus_block, cx| {
                            let mut next_title = focus_block.record.title.clone();
                            next_title.append_tree(trailing.clone());
                            focus_block.record.set_title(next_title);
                            focus_block.sync_render_cache();
                            focus_block.cursor_blink_epoch = Instant::now();
                            cx.notify();
                        }
                    });
                    let cursor = focus_block.read(cx).display_text().len();
                    Self::reset_block_cursor(&focus_block, cursor, cx);
                    self.rebuild_image_runtimes(cx);
                    if let Some(binding) = self.table_cell_binding(focus_block.entity_id()) {
                        self.sync_table_record_from_runtime(&binding.table_block, cx);
                    }
                    self.focus_block(focus_block.entity_id());
                }

                // When structural content is pasted onto an empty line there is
                // no pre-cursor text to keep, so drop the now-empty paragraph
                // rather than leaving a blank line above the pasted blocks.
                if leading_empty {
                    self.document.with_structure_mutation(cx, |document, cx| {
                        document.remove_block_by_id_raw(block.entity_id(), cx);
                    });
                }

                // 先把整段（引用根 + 粘贴插入的新根）按区间写进缓冲区，再让
                // 引用重解析把结构收一遍。只靠 normalize 写引用根的话，插在它
                // 后面的新根没有区间，随后的整篇序列化又被跳过，粘贴内容就停在
                // 树里没进缓冲区。
                let region_written = quote_related
                    && quote_root_before.as_ref().is_some_and(|anchor| {
                        self.write_back_structural_change(anchor, Some(&roots_before), cx)
                    });
                if quote_related {
                    self.normalize_rendered_quote_structure(&block, cx);
                }

                // Quote normalization rebuilds roots from Markdown, so resolve the
                // landing block from the live tree rather than the pasted handles.
                if inserted_at_doc_end {
                    if let Some(last_root) = self.document.root_blocks().last().cloned() {
                        self.ensure_trailing_paragraph_after_structural(&last_root, cx);
                    }
                }
                if region_written {
                    self.mark_dirty_written_back(cx);
                } else {
                    self.mark_dirty(cx);
                }
                self.finalize_pending_undo_capture(cx);
                cx.notify();
            }
            BlockEvent::RequestPasteImage { .. }
            | BlockEvent::RequestReplaceCrossBlockSelection { .. } => {}
            BlockEvent::RequestIndent => {
                let current_visible_index = self.current_visible_index_of(block.entity_id());
                if current_visible_index == 0 {
                    return;
                }

                let Some(location) = self.document.find_block_location(block.entity_id()) else {
                    return;
                };
                let Some(target_parent) = self
                    .document
                    .visible_blocks()
                    .get(current_visible_index - 1)
                    .map(|visible| visible.entity.clone())
                else {
                    return;
                };
                let current_kind = block.read(cx).kind();
                if !current_kind.can_nest_under(&target_parent.read(cx).kind()) {
                    return;
                }
                if location
                    .parent
                    .as_ref()
                    .is_some_and(|parent| parent.entity_id() == target_parent.entity_id())
                {
                    return;
                }
                self.prepare_undo_capture(crate::components::UndoCaptureKind::NonCoalescible, cx);
                // 区间锚点取「上一条所在的那一根」：本块被它吸收之后，被换掉的字节
                // 区间就是「那一根 + 本块」这一段，区间外的块一个字节都不动。
                let region_anchor = self
                    .document
                    .root_ancestor_of(target_parent.entity_id())
                    .unwrap_or_else(|| target_parent.clone());

                let moved = self.document.with_structure_mutation(cx, |document, cx| {
                    let moved = document.remove_block_by_id_raw(block.entity_id(), cx)?.0;
                    let child_index = target_parent.read(cx).children.len();
                    document.insert_blocks_at_raw(
                        Some(target_parent.clone()),
                        child_index,
                        vec![moved.clone()],
                        cx,
                    );
                    Some(moved)
                });

                let Some(moved) = moved else {
                    // prepare 之后早退必须把组收掉：挂着空组会让下一次动作并进
                    // 这一步的撤销组，选区快照也是上一次的。
                    self.finalize_pending_undo_capture(cx);
                    return;
                };

                self.focus_block(moved.entity_id());
                if self.write_back_structural_change(&region_anchor, roots_before.as_deref(), cx) {
                    self.mark_dirty_written_back(cx);
                } else {
                    self.mark_dirty(cx);
                }
                self.finalize_pending_undo_capture(cx);
                cx.notify();
            }
            BlockEvent::RequestOutdent => {
                let Some(location) = self.document.find_block_location(block.entity_id()) else {
                    return;
                };
                self.prepare_undo_capture(crate::components::UndoCaptureKind::NonCoalescible, cx);
                // 提级是把本块从父级挪到上一级：被换掉的区间是「父级所在的那一根」整段。
                // 本块本来就是根块时（降级成段落）块序列不变，锚点就用它自己。
                let region_anchor = location
                    .parent
                    .as_ref()
                    .and_then(|parent| self.document.root_ancestor_of(parent.entity_id()))
                    .unwrap_or_else(|| block.clone());

                if let Some(parent) = location.parent.clone() {
                    let Some(parent_location) =
                        self.document.find_block_location(parent.entity_id())
                    else {
                        // prepare 之后早退必须把组收掉：挂着空组会让下一次动作并进
                        // 这一步的撤销组，选区快照也是上一次的（与下方 RequestIndent 同规）。
                        self.finalize_pending_undo_capture(cx);
                        return;
                    };

                    let moved = self.document.with_structure_mutation(cx, |document, cx| {
                        let moved = document.remove_block_by_id_raw(block.entity_id(), cx)?.0;
                        document.insert_blocks_at_raw(
                            parent_location.parent,
                            parent_location.index + 1,
                            vec![moved.clone()],
                            cx,
                        );
                        Some(moved)
                    });

                    let Some(moved) = moved else {
                        return;
                    };
                    self.focus_block(moved.entity_id());
                } else {
                    block.update(cx, |block, cx| block.convert_to_paragraph(cx));
                    self.focus_block(block.entity_id());
                }

                if self.write_back_structural_change(&region_anchor, roots_before.as_deref(), cx) {
                    self.mark_dirty_written_back(cx);
                } else {
                    self.mark_dirty(cx);
                }
                self.finalize_pending_undo_capture(cx);
                cx.notify();
            }
            BlockEvent::RequestDowngradeNestedListItemToChildParagraph => {
                let Some(location) = self.document.find_block_location(block.entity_id()) else {
                    return;
                };
                let Some(parent) = location.parent.clone() else {
                    return;
                };
                if !block.read(cx).kind().is_list_item() || !parent.read(cx).kind().is_list_item() {
                    return;
                }

                self.prepare_undo_capture(crate::components::UndoCaptureKind::NonCoalescible, cx);
                let roots_before = self.document.root_layout();

                let downgraded = self.document.with_structure_mutation(cx, |document, cx| {
                    let (moved, removed_location) =
                        document.remove_block_by_id_raw(block.entity_id(), cx)?;
                    moved.update(cx, |block, cx| {
                        block.record.kind = BlockKind::Paragraph;
                        block.record.raw_fallback = None;
                        block.sync_edit_mode_from_kind();
                        block.sync_render_cache();
                        block.cursor_blink_epoch = Instant::now();
                        cx.notify();
                    });
                    document.insert_blocks_at_raw(
                        Some(parent.clone()),
                        removed_location.index,
                        vec![moved.clone()],
                        cx,
                    );
                    Some(moved)
                });

                let Some(downgraded) = downgraded else {
                    // prepare 之后早退必须把组收掉（与 RequestIndent 同规）。
                    self.finalize_pending_undo_capture(cx);
                    return;
                };

                self.focus_block(downgraded.entity_id());
                self.rebuild_image_runtimes(cx);
                // 降级的内容都在这一根列表里：整根块按新区间写回就够了。
                if self.write_back_structural_change(&downgraded, Some(&roots_before), cx) {
                    self.mark_dirty_written_back(cx);
                } else {
                    self.mark_dirty(cx);
                }
                self.finalize_pending_undo_capture(cx);
                cx.notify();
            }
            BlockEvent::ToggleTaskChecked => {
                self.prepare_undo_capture(crate::components::UndoCaptureKind::NonCoalescible, cx);
                block.update(cx, |block, cx| {
                    let checked = match block.kind() {
                        BlockKind::TaskListItem { checked } => checked,
                        _ => return,
                    };
                    block.record.kind = BlockKind::TaskListItem { checked: !checked };
                    block.sync_edit_mode_from_kind();
                    block.sync_render_cache();
                    block.cursor_blink_epoch = Instant::now();
                    cx.notify();
                });
                // 勾一下复选框只改这一行：块序列没变，整根块重投影回它自己的区间。
                if self.write_back_structural_change(&block, roots_before.as_deref(), cx) {
                    self.mark_dirty_written_back(cx);
                } else {
                    self.mark_dirty(cx);
                }
                self.request_active_block_scroll_into_view(cx);
                self.finalize_pending_undo_capture(cx);
                cx.notify();
            }
            BlockEvent::RequestOpenLink { open_target } => {
                self.defer_open_link(open_target.clone(), cx);
            }
            BlockEvent::RequestJumpToFootnoteDefinition { id, .. } => {
                let _ = self.jump_to_footnote_definition(id, cx);
                cx.notify();
            }
            BlockEvent::RequestJumpToFootnoteBackref { id } => {
                let _ = self.jump_to_footnote_backref(id, cx);
                cx.notify();
            }
            BlockEvent::RequestTableAxisPreview {
                kind,
                index,
                hovered,
            } => {
                if block.read(cx).kind() == BlockKind::Table {
                    self.preview_table_axis(block.entity_id(), *kind, *index, *hovered, cx);
                }
            }
            BlockEvent::RequestSelectTableAxis { kind, index } => {
                if block.read(cx).kind() == BlockKind::Table {
                    self.select_table_axis(block.entity_id(), *kind, *index, cx);
                }
            }
            BlockEvent::RequestOpenTableAxisMenu {
                kind,
                index,
                position,
            } => {
                if block.read(cx).kind() == BlockKind::Table {
                    self.open_table_axis_menu(block.entity_id(), *kind, *index, *position, cx);
                }
            }
            BlockEvent::RequestOpenTableCellMenu {
                row,
                column,
                position,
            } => {
                if block.read(cx).kind() == BlockKind::Table {
                    self.open_table_cell_context_menu(
                        block.entity_id(),
                        crate::components::TableCellPosition {
                            row: *row,
                            column: *column,
                        },
                        *position,
                        cx,
                    );
                }
            }
            BlockEvent::RequestTableCellMoveHorizontal { .. }
            | BlockEvent::RequestTableCellMoveVertical { .. } => {}
            BlockEvent::RequestFocusPrev { preferred_x } => {
                let Some(target) =
                    self.vertical_neighbor_entity(block.entity_id(), -1, cx)
                else {
                    return;
                };

                // Entering a table from below lands in a body cell instead of
                // the non-editable table container.
                if target.read(cx).kind() == BlockKind::Table
                    && self.focus_table_entry_cell(&target, false, cx)
                {
                    return;
                }
                let target_x = preferred_x.map(px);
                let offset = target
                    .read(cx)
                    .entry_offset_for_vertical_focus(true, target_x);
                self.focus_block(target.entity_id());
                target.update(cx, move |target, cx| {
                    target.move_to_with_preferred_x(offset, target_x, cx);
                });
                cx.notify();
            }
            BlockEvent::RequestFocusNext { preferred_x } => {
                let Some(target) = self.vertical_neighbor_entity(block.entity_id(), 1, cx) else {
                    // A trailing multi-line block (code, math, ...) has nowhere
                    // below to move to, so give it a paragraph to land on and
                    // focus that, matching how a trailing table behaves.
                    if block.read(cx).kind().is_multiline_text_block() {
                        // 自动补尾段落是用户可见编辑：必须落撤销组并置脏，否则关闭
                        // 不提示丢字、撤销永远删不掉它（与粘贴路径同规）。
                        self.prepare_undo_capture(
                            crate::components::UndoCaptureKind::NonCoalescible,
                            cx,
                        );
                        self.ensure_trailing_paragraph_after_structural(&block, cx);
                        // 补尾段落只往缓冲区末尾追加分隔空行、给新块挂零宽区间：
                        // 字节已经落好，别再整篇重投影把别的块写法一起洗掉。
                        self.mark_dirty_written_back(cx);
                        self.finalize_pending_undo_capture(cx);
                        // 补完段落重新按增量索引找落点：新块此刻已在可见快照里。
                        let landing = self
                            .document
                            .visible_index_for_entity_id(block.entity_id())
                            .and_then(|index| {
                                self.document.visible_blocks().get(index + 1)
                            })
                            .map(|visible| visible.entity.clone());
                        if let Some(landing) = landing {
                            self.focus_block(landing.entity_id());
                            landing.update(cx, |landing, cx| landing.move_to(0, cx));
                            cx.notify();
                        }
                    }
                    return;
                };

                // Entering a table from above lands in a header cell instead of
                // the non-editable table container.
                if target.read(cx).kind() == BlockKind::Table
                    && self.focus_table_entry_cell(&target, true, cx)
                {
                    return;
                }
                let target_x = preferred_x.map(px);
                let offset = target
                    .read(cx)
                    .entry_offset_for_vertical_focus(false, target_x);
                self.focus_block(target.entity_id());
                target.update(cx, move |target, cx| {
                    target.move_to_with_preferred_x(offset, target_x, cx);
                });
                cx.notify();
            }
            BlockEvent::RequestSelectVertical {
                direction,
                preferred_x,
                target_offset,
            } => {
                let preferred_x = px(*preferred_x);
                let Some(target_offset) = target_offset else {
                    self.extend_selection_vertically_across_blocks(
                        &block,
                        *direction,
                        preferred_x,
                        cx,
                    );
                    return;
                };
                // 焦点块里继续按 Shift+↑/↓：跨块选区的焦点端点跟着走，锚点
                // 留在原块——不能退化成块内选区，否则前面选中的几块会整段丢掉。
                let block_ref = block.read(cx);
                let clean_target = block_ref.current_to_clean_offset(*target_offset);
                let block_id = block.entity_id();
                if let Some(mut selection) = self.cross_block_selection
                    && selection.focus.entity_id == block_id
                {
                    selection.focus.offset = clean_target;
                    self.cross_block_selection = (selection.focus != selection.anchor)
                        .then_some(selection);
                    block.update(cx, |block, cx| block.move_to(*target_offset, cx));
                    self.sync_cross_block_selection_visuals(cx);
                    cx.notify();
                    return;
                }
                // 没有跨块选区：普通块内扩选，从光标往上/下延。
                self.clear_cross_block_selection(cx);
                block.update(cx, |block, cx| block.select_to(*target_offset, cx));
                cx.notify();
            }
            BlockEvent::RequestFocusTableEntry { from_top } => {
                if self.focus_table_entry_cell(&block, *from_top, cx) {
                    cx.notify();
                }
            }
            BlockEvent::RequestBlockUp => {
                let Some(target) = self.vertical_neighbor_entity(block.entity_id(), -1, cx) else {
                    return;
                };

                if target.read(cx).kind() == BlockKind::Table
                    && self.focus_table_entry_cell(&target, false, cx)
                {
                    return;
                }
                self.focus_block(target.entity_id());
                target.update(cx, |target, cx| target.move_to(0, cx));
                cx.notify();
            }
            BlockEvent::RequestBlockDown => {
                let Some(target) = self.vertical_neighbor_entity(block.entity_id(), 1, cx) else {
                    return;
                };

                if target.read(cx).kind() == BlockKind::Table
                    && self.focus_table_entry_cell(&target, true, cx)
                {
                    return;
                }
                self.focus_block(target.entity_id());
                target.update(cx, |target, cx| target.move_to(0, cx));
                cx.notify();
            }
            BlockEvent::RequestDelete => {
                if self.downgrade_empty_callout_body_to_quote(&block, cx) {
                    return;
                }
                let quote_related = self.block_is_quote_structure_related(&block, cx);
                let current_visible_index = self.current_visible_index_of(block.entity_id());
                let is_last_visible_leaf =
                    self.document.visible_blocks().len() == 1 && block.read(cx).children.is_empty();
                if is_last_visible_leaf {
                    if block.read(cx).kind() == BlockKind::Paragraph {
                        Self::reset_block_cursor(&block, 0, cx);
                    } else {
                        block.update(cx, |block, cx| block.convert_to_paragraph(cx));
                    }
                    self.focus_block(block.entity_id());
                    cx.notify();
                    return;
                }
                self.prepare_undo_capture(crate::components::UndoCaptureKind::NonCoalescible, cx);
                // 删掉这一块只该动它那一段（在容器里时是那一整根容器）。
                let region_anchor = self
                    .document
                    .root_ancestor_of(block.entity_id())
                    .unwrap_or_else(|| block.clone());

                let focus_candidate = if current_visible_index > 0 {
                    self.document
                        .visible_blocks()
                        .get(current_visible_index - 1)
                        .map(|visible| visible.entity.entity_id())
                } else {
                    self.document
                        .visible_blocks()
                        .get(current_visible_index + 1)
                        .map(|visible| visible.entity.entity_id())
                };

                let adopted_children = crate::editor::tree::DocumentTree::take_children(&block, cx);
                let removed = self.document.with_structure_mutation(cx, |document, cx| {
                    let (_, location) = document.remove_block_by_id_raw(block.entity_id(), cx)?;
                    if !adopted_children.is_empty() {
                        document.insert_blocks_at_raw(
                            location.parent.clone(),
                            location.index,
                            adopted_children.clone(),
                            cx,
                        );
                    }
                    Some(location)
                });

                if removed.is_none() {
                    // prepare 之后早退必须把组收掉（与 RequestIndent 同规）。
                    self.finalize_pending_undo_capture(cx);
                    return;
                }

                if let Some(focus_id) = focus_candidate {
                    self.focus_block(focus_id);
                } else if let Some(first_root) = self.document.first_root() {
                    self.focus_block(first_root.entity_id());
                }

                // 先把这一段按区间落进缓冲区，再声明「缓冲区已经是目标状态」：
                // normalize 里那次重同步 otherwise 会拿整篇重新序列化盖掉未编辑的块。
                let wrote_back =
                    self.write_back_structural_change(&region_anchor, roots_before.as_deref(), cx);
                self.skip_next_resync = wrote_back && quote_related;
                if quote_related {
                    self.normalize_rendered_quote_structure(&block, cx);
                }
                if wrote_back {
                    self.mark_dirty_written_back(cx);
                } else {
                    self.mark_dirty(cx);
                }
                self.finalize_pending_undo_capture(cx);
                cx.notify();
            }
            BlockEvent::RequestFocus => {
                self.close_menu_bar(cx);
                self.clear_table_axis_preview(cx);
                self.clear_table_axis_selection(cx);
                // 活动样式的切换只涉及两块：旧活动目标摘输入态（光标、占位符、
                // 行内投影），新块亮起来。旧实现逐块 notify 整篇可见块——flatten
                // 是 O(文档) 克隆，逐块 notify 是 O(文档) 实体写，每次点击全付。
                // 焦点在渲染期落地时 `window.focus` 本就会让整窗重排，前后两块
                // 之外的通知是纯浪费。旧目标可能是表格格子（从表里点出去），
                // `focusable_entity_by_id` 认得出。
                let previous_active = self
                    .active_entity_id
                    .or(self.pending_focus)
                    .filter(|entity_id| *entity_id != block.entity_id())
                    .and_then(|entity_id| self.focusable_entity_by_id(entity_id));
                self.focus_block(block.entity_id());
                let mut refreshed = 1u64;
                if let Some(previous) = previous_active {
                    previous.update(cx, |_, cx| cx.notify());
                    refreshed += 1;
                }
                block.update(cx, |_, cx| cx.notify());
                self.focus_transfer_block_updates
                    .set(self.focus_transfer_block_updates.get() + refreshed);
                cx.notify();
            }
            BlockEvent::RequestShowMessage { title, detail } => {
                self.show_message_modal(title.clone(), detail.clone(), cx);
            }
            BlockEvent::RequestRenderedSelectAll => {}
            BlockEvent::PrepareUndo { .. } => {}
            BlockEvent::CompositionChanged => {}
        }
    }

    /// 一次文本改动的落笔：能按区间写回就写回，结构没变就不重投影。
    ///
    /// 它与结构事件分开走，因为这里只需要「可见序列有没有变」：整篇克隆
    /// `VisibleBlock` 要付每个 Entity 句柄的引用计数与 LeakDetector 登记，
    /// 10 MiB 档一次 17ms，占满一次按键。
    fn on_block_changed(&mut self, block: Entity<crate::editor::Block>, cx: &mut Context<Self>) {
        // 用户改了文档：换投影交还的阅读位置按旧字节记账，一律作废。
        self.cancel_pending_view_mode_scroll();
        // 块内编辑只报「变了」：变化形状由块在编辑出口记下，这里取走并平移
        // 同一块上的 vim 标记/跳转表，跨编辑后 `` `a `` 才落在原位置。
        if let Some(edit) = block.update(cx, |block, _| block.take_last_visible_edit()) {
            self.vim_adjust_tracked_offsets(block.entity_id(), edit);
        }
        let kind_changed = self.writes_through_the_buffer() && {
            let block = block.read(cx);
            block.record.kind != block.record.source_kind
        };
        // 模型直改（重命名、补全、程序化编辑）同样会走到 Changed：它们在改
        // 用户文档，撤销必须覆盖。没有开着的组就补开一个——delta 撤销栈的
        // 前提是每一次缓冲区写入都被记账，这条不变式在这里收口（组已开时
        // prepare 是空操作，打字的 Coalescible 组不受影响）。
        self.prepare_undo_capture(crate::components::UndoCaptureKind::NonCoalescible, cx);
        // 结构在这一笔里动没动，由本函数内仅有的三个结构帮手直接作证，不再拿
        // 整篇可见序列逐位比对——旧比对每键付一次 O(文档) 身份序列分配加一次
        // O(文档) 全量比较（16 万块 ≈ 每键几十万次无谓工作）。三个帮手是闭集，
        // 就写在本函数里：
        // 1. `insert_list_group_separator_before` 返回 true 当且仅当插了分隔块；
        // 2. `materialize_empty_callout_shortcut` 返回 Some 当且仅当补了正文块
        //    （早退分支在任何改动之前）；
        // 3. `normalize_rendered_quote_structure` 的区域重投影（`reuse_leaf=false`，
        //    恒换实体）与整篇兜底都会换掉区域实体。
        // 其余帮手（补全刷新、行元数据刷新、引用元数据重建）只动实体字段或
        // 快照元数据，不碰可见序列。
        let mut structure_mutated = false;
        // [[ 补全跟随编辑刷新（依据本次编辑后的文本与光标）。
        self.update_wikilink_completion_for_block(&block, cx);
        // `\` 公式补全同上：数学块/行内公式里的反斜杠命令。
        self.update_latex_completion_for_block(&block, cx);
        // 文本形状（`[TOC]`）变了就当场刷新快照里那一条元数据：整棵同步
        // 只在结构变化时重跑，这条编辑不改结构。
        self.document.refresh_row_spacing_for(block.entity_id(), cx);

        let should_restart_numbered_list = block.update(cx, |block, _cx| {
            block.take_numbered_list_restart_requested()
        });
        if should_restart_numbered_list {
            structure_mutated |=
                self.insert_list_group_separator_before(block.entity_id(), cx);
        }

        let callout_focus_target = self.materialize_empty_callout_shortcut(&block, cx);
        structure_mutated |= callout_focus_target.is_some();

        let should_normalize_quote =
            block.update(cx, |block, _cx| {
                let requested = block.take_quote_reparse_requested();
                requested && block.marked_range.is_none()
            }) || Self::rendered_quote_text_requires_reparse(&block, cx);

        self.refresh_rendered_quote_metadata_if_needed(&block, cx);
        let needs_reference_refresh = !should_normalize_quote
            && self.changed_block_needs_runtime_context_refresh(&block, cx);
        if should_normalize_quote {
            self.normalize_rendered_quote_structure(&block, cx);
            structure_mutated = true;
        }
        if let Some(focus_id) = callout_focus_target {
            self.focus_block(focus_id);
        }
        // 文本改动按区间写回缓冲区，未编辑的块因此保住原有字节。前提是这次
        // 事件没动结构：根块序列一变，区间就整体错位，只能退回整篇重投影。
        let structure_unchanged = !structure_mutated;
        // 拆块的切分不落笔：光标之后那截是下一个块的源码，写出去就是「先删一遍
        // 再写回一遍」——本块的区间塌成零宽，紧随的结构写回就没区间可用，只能
        // 整篇重投影。留着区间，那一步才只改这一段。
        let splitting = block.read(cx).split_truncation_pending();
        if splitting {
            self.mark_dirty_written_back(cx);
        } else if structure_unchanged
            && ((kind_changed && self.write_back_changed_block_kind(&block, cx))
                || (!kind_changed && self.write_back_visible_insertion(&block, cx))
                || self.write_back_block_source(&block, cx))
        {
            self.mark_dirty_written_back(cx);
        } else {
            self.mark_dirty(cx);
        }
        // markdown 源码视图：这次编辑若开/关了围栏、公式块，后续分块
        // 的接缝状态要级联重串（一致时只多一次实体读；走了 mark_dirty
        // 的整篇重投影会自己重串，这里立刻收敛为一次读）。
        self.cascade_source_fence_states_after(block.entity_id(), cx);
        // 引用定义要从缓冲区重解析，就得等这一笔落完：早一步读到的是改之前
        // 的定义行——改 `[asset]:` 的地址，图片还指着旧地址
        // （`editing_image_reference_definition_refreshes_existing_image`）。
        // 落笔**前后**两份源码都要看：上面那次问的是「这一笔之前缓冲区里是什么」，
        // 而「原来没有 `]:`、这一下才写进去」的定义行（粘贴一行定义、把定义从别处
        // 搬过来、打完整定义的最后那一键）只有落笔之后才在缓冲区里看得见。少看这
        // 一份，注册表就停在旧的定义集上：引用式图片指着旧地址或压根解不出来
        // （闸门 `introducing_a_reference_definition_refreshes_the_reference_registry`）。
        // 这一份用同一套窄口径，所以带 `[文字](链接)` 的段落依旧不会每键重建。
        if needs_reference_refresh
            || (!should_normalize_quote
                && self.changed_block_needs_runtime_context_refresh(&block, cx))
        {
            self.rebuild_image_runtimes(cx);
        }
        self.request_active_block_scroll_into_view(cx);
        self.finalize_pending_undo_capture_at_end_of_batch(cx);
    }
}
