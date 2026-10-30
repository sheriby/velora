use super::*;

impl Editor {
    pub(crate) fn block_is_quote_structure_related(&self, block: &Entity<crate::editor::Block>, cx: &App) -> bool {
        if self.view_mode != crate::editor::ViewMode::Rendered {
            return false;
        }

        let block_ref = block.read(cx);
        block_ref.kind().is_quote_container()
            || block_ref.quote_depth > 0
            || block_ref.quote_group_anchor.is_some()
    }

    pub(crate) fn refresh_rendered_quote_metadata_if_needed(
        &mut self,
        block: &Entity<crate::editor::Block>,
        cx: &mut Context<Self>,
    ) {
        if !self.block_is_quote_structure_related(block, cx) {
            return;
        }
        // 按键路径的代际短路：只有「引用结构指纹」变了才重排整棵快照。
        // 多行引用里普通打字不增删行、不改块种类，指纹不变；引用快捷键刚
        // 创建容器（种类变）或增删了行（行数变）才会触发。
        let signature = {
            let block_ref = block.read(cx);
            (
                block_ref.kind().is_quote_container(),
                block_ref.quote_depth,
                block_ref.display_text().lines().count(),
            )
        };

        if block.read(cx).quote_metadata_signature == Some(signature) {
            return;
        }
        block.update(cx, |block, _cx| {
            block.quote_metadata_signature = Some(signature);
        });
        self.document.rebuild_metadata_and_snapshot(cx);
    }

    pub(crate) fn rendered_quote_text_requires_reparse(block: &Entity<crate::editor::Block>, cx: &App) -> bool {
        let block_ref = block.read(cx);
        if block_ref.quote_depth == 0 && !block_ref.kind().is_quote_container() {
            return false;
        }

        let text = block_ref.display_text();
        if !text.contains('\n') {
            return false;
        }

        // 行首块语法判定只有 components/block 一处判据（含 ≤3 空格围栏缩进）。
        crate::components::Block::multiline_quote_edit_requires_reparse(text)
    }

    pub(crate) fn block_event_clears_cross_block_selection(event: &BlockEvent) -> bool {
        matches!(
            event,
            BlockEvent::Changed
                | BlockEvent::RequestNewline { .. }
                | BlockEvent::RequestEnterCalloutBody
                | BlockEvent::RequestQuoteBreak
                | BlockEvent::RequestCalloutBreak
                | BlockEvent::RequestMergeIntoPrev { .. }
                | BlockEvent::RequestMergeFromNext
                | BlockEvent::RequestPasteMultiline { .. }
                | BlockEvent::RequestPasteImage { .. }
                | BlockEvent::RequestIndent
                | BlockEvent::RequestOutdent
                | BlockEvent::RequestDowngradeNestedListItemToChildParagraph
                | BlockEvent::ToggleTaskChecked
                // 换格先收选区：焦点走进另一格之后，旧选区不再代表「当前选中什么」，
                // 留着它下一键就会按旧选区去替换**旧格子**的字节（表格分支只发这两个
                // 事件就 return，没有再经过别的清除点）。
                | BlockEvent::RequestTableCellMoveHorizontal { .. }
                | BlockEvent::RequestTableCellMoveVertical { .. }
                | BlockEvent::RequestDelete
        )
    }
    pub(crate) fn nearest_quote_ancestor(
        &self,
        entity_id: EntityId,
        cx: &App,
    ) -> Option<Entity<crate::editor::Block>> {
        let mut current = self.focusable_entity_by_id(entity_id)?;
        loop {
            if current.read(cx).kind().is_quote_container() {
                return Some(current);
            }
            let location = self.document.find_block_location(current.entity_id())?;
            current = location.parent?;
        }
    }

    pub(crate) fn topmost_quote_ancestor(
        &self,
        entity_id: EntityId,
        cx: &App,
    ) -> Option<Entity<crate::editor::Block>> {
        let mut current = self.nearest_quote_ancestor(entity_id, cx)?;
        loop {
            let Some(location) = self.document.find_block_location(current.entity_id()) else {
                break;
            };
            let Some(parent) = location.parent.clone() else {
                break;
            };
            if !parent.read(cx).kind().is_quote_container() {
                break;
            }
            current = parent;
        }
        Some(current)
    }

    pub(crate) fn quote_break_insertion_target(
        &self,
        entity_id: EntityId,
        cx: &App,
    ) -> Option<(Option<Entity<crate::editor::Block>>, usize)> {
        let quote_block = self.nearest_quote_ancestor(entity_id, cx)?;
        let location = self.document.find_block_location(quote_block.entity_id())?;
        Some((location.parent.clone(), location.index + 1))
    }

    pub(crate) fn callout_break_insertion_target(
        &self,
        entity_id: EntityId,
        cx: &App,
    ) -> Option<(Option<Entity<crate::editor::Block>>, usize)> {
        let callout_root = self.topmost_quote_ancestor(entity_id, cx)?;
        let location = self
            .document
            .find_block_location(callout_root.entity_id())?;
        Some((location.parent.clone(), location.index + 1))
    }

    pub(crate) fn ensure_callout_body_entry(
        &mut self,
        callout: &Entity<crate::editor::Block>,
        cx: &mut Context<Self>,
    ) -> Option<Entity<crate::editor::Block>> {
        if !matches!(callout.read(cx).kind(), BlockKind::Callout(_)) {
            return None;
        }

        if let Some(first_child) = callout.read(cx).children.first().cloned() {
            return Some(first_child);
        }

        let body = Self::new_block(cx, BlockRecord::paragraph(String::new()));
        self.document
            .insert_blocks_at(Some(callout.clone()), 0, vec![body.clone()], cx);
        Some(body)
    }

    pub(crate) fn materialize_empty_callout_shortcut(
        &mut self,
        block: &Entity<crate::editor::Block>,
        cx: &mut Context<Self>,
    ) -> Option<EntityId> {
        if self.view_mode != crate::editor::ViewMode::Rendered {
            return None;
        }

        let (kind, title_markdown, has_children) = block.read_with(cx, |block, _cx| {
            (
                block.kind(),
                block.record.title.serialize_markdown(),
                !block.children.is_empty(),
            )
        });
        if kind != BlockKind::Quote || has_children {
            return None;
        }

        let Some((variant, title)) =
            crate::components::CalloutVariant::parse_header_line(&title_markdown)
        else {
            return None;
        };

        block.update(cx, |block, cx| {
            block.record.kind = BlockKind::Callout(variant);
            block
                .record
                .set_title(InlineTextTree::from_markdown(&title));
            block.sync_edit_mode_from_kind();
            block.sync_render_cache();
            block.cursor_blink_epoch = Instant::now();
            cx.notify();
        });
        let body = self.ensure_callout_body_entry(block, cx)?;
        Some(body.entity_id())
    }

    pub(crate) fn downgrade_empty_callout_body_to_quote(
        &mut self,
        block: &Entity<crate::editor::Block>,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(location) = self.document.find_block_location(block.entity_id()) else {
            return false;
        };
        let Some(parent) = location.parent.clone() else {
            return false;
        };

        let (header_markdown, only_child, block_is_empty_leaf) = {
            let parent_ref = parent.read(cx);
            let Some(variant) = parent_ref.kind().callout_variant() else {
                return false;
            };
            let block_ref = block.read(cx);
            (
                variant.header_markdown(&parent_ref.record.title.serialize_markdown()),
                parent_ref.children.len() == 1,
                block_ref.kind() == BlockKind::Paragraph
                    && block_ref.display_text().is_empty()
                    && block_ref.children.is_empty(),
            )
        };
        if !only_child || !block_is_empty_leaf {
            return false;
        }

        self.prepare_undo_capture(crate::components::UndoCaptureKind::NonCoalescible, cx);
        let roots_before = self.document.root_layout();
        self.document.with_structure_mutation(cx, |document, cx| {
            let _ = document.remove_block_by_id_raw(block.entity_id(), cx);
            parent.update(cx, |parent, cx| {
                parent.record.kind = BlockKind::Quote;
                parent
                    .record
                    .set_title(InlineTextTree::from_markdown(&header_markdown));
                parent.sync_edit_mode_from_kind();
                parent.sync_render_cache();
                parent.assign_collapsed_selection_offset(0, CollapsedCaretAffinity::Default, None);
                parent.marked_range = None;
                parent.cursor_blink_epoch = Instant::now();
                cx.notify();
            });
        });
        self.focus_block(parent.entity_id());
        // 注册表在落笔之后刷新（读的是缓冲区，写回前调是旧字节）。
        if self.write_back_structural_change(&parent, Some(&roots_before), cx) {
            self.mark_dirty_written_back(cx);
        } else {
            self.mark_dirty(cx);
        }
        self.rebuild_image_runtimes(cx);
        self.finalize_pending_undo_capture(cx);
        cx.notify();
        true
    }
}
