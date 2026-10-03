use super::*;

impl Editor {
    pub(crate) fn focus_table_cell_position(
        &mut self,
        table_block: &Entity<crate::editor::Block>,
        position: TableCellPosition,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(cell) = table_block
            .read(cx)
            .table_runtime
            .as_ref()
            .and_then(|runtime| runtime.cell(position))
        else {
            return false;
        };
        self.focus_block(cell.entity_id());
        cx.notify();
        true
    }

    /// Focus a cell when keyboard navigation enters a table from an adjacent
    /// block. Entering from above lands on the first header cell; entering from
    /// below lands on the first cell of the last body row, falling back to the
    /// header when the table has no body rows.
    pub(crate) fn focus_table_entry_cell(
        &mut self,
        table_block: &Entity<crate::editor::Block>,
        from_top: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(runtime) = table_block.read(cx).table_runtime.clone() else {
            return false;
        };
        let cell = if from_top {
            runtime.header.first().cloned()
        } else {
            runtime
                .rows
                .last()
                .and_then(|row| row.first())
                .cloned()
                .or_else(|| runtime.header.first().cloned())
        };
        let Some(cell) = cell else {
            return false;
        };
        self.focus_block(cell.entity_id());
        cx.notify();
        true
    }

    /// Move focus from a table edge to the block immediately above (delta < 0)
    /// or below (delta > 0) it, mirroring how plain blocks transfer focus when
    /// the caret leaves their first or last line. When the neighbor is itself a
    /// table, drop into one of its cells so the caret stays editable instead of
    /// landing on the table container. `to_block_start` lands the caret at the
    /// neighbor's start (Block Up/Down semantics) rather than the nearest edge
    /// (Move Up/Down semantics).
    pub(crate) fn focus_block_adjacent_to_table(
        &mut self,
        table_block: &Entity<crate::editor::Block>,
        delta: i32,
        to_block_start: bool,
        cx: &mut Context<Self>,
    ) {
        let visible = self.document.flatten_visible_blocks();
        let Some(index) = visible
            .iter()
            .position(|visible| visible.entity.entity_id() == table_block.entity_id())
        else {
            return;
        };
        let target_index = if delta < 0 {
            index.checked_sub(1)
        } else {
            Some(index + 1)
        };
        let Some(target) = target_index
            .and_then(|target_index| visible.get(target_index))
            .map(|visible| visible.entity.clone())
        else {
            return;
        };
        if target.read(cx).kind() == BlockKind::Table
            && self.focus_table_entry_cell(&target, delta > 0, cx)
        {
            return;
        }
        self.focus_block(target.entity_id());
        if to_block_start {
            target.update(cx, |target, cx| target.move_to(0, cx));
        } else {
            let prefer_last_line = delta < 0;
            let offset = target
                .read(cx)
                .entry_offset_for_vertical_focus(prefer_last_line, None);
            target.update(cx, move |target, cx| {
                target.move_to_with_preferred_x(offset, None, cx);
            });
        }
        cx.notify();
    }

    pub(crate) fn focus_table_cell_horizontal_neighbor(
        &mut self,
        table_block: &Entity<crate::editor::Block>,
        position: TableCellPosition,
        delta: i32,
        cx: &mut Context<Self>,
    ) {
        let Some(runtime) = table_block.read(cx).table_runtime.clone() else {
            return;
        };
        let columns = runtime.header.len();
        let total_rows = 1 + runtime.rows.len();
        if columns == 0 || total_rows == 0 {
            return;
        }

        let linear = position.row * columns + position.column;
        let next = if delta < 0 {
            linear.checked_sub(delta.unsigned_abs() as usize)
        } else {
            linear.checked_add(delta as usize)
        };
        let Some(next) = next else {
            return;
        };
        if next >= total_rows * columns {
            return;
        }

        let next_position = TableCellPosition {
            row: next / columns,
            column: next % columns,
        };
        let _ = self.focus_table_cell_position(table_block, next_position, cx);
    }

    pub(crate) fn focus_table_cell_vertical_neighbor(
        &mut self,
        table_block: &Entity<crate::editor::Block>,
        position: TableCellPosition,
        delta: i32,
        cx: &mut Context<Self>,
    ) {
        let Some(runtime) = table_block.read(cx).table_runtime.clone() else {
            return;
        };
        let max_row = runtime.rows.len();
        let next_row = if delta < 0 {
            position.row.checked_sub(delta.unsigned_abs() as usize)
        } else {
            position.row.checked_add(delta as usize)
        };
        // Moving past the first/last row leaves the table for the adjacent
        // block rather than stopping at the edge.
        let Some(next_row) = next_row.filter(|row| *row <= max_row) else {
            self.focus_block_adjacent_to_table(table_block, delta, false, cx);
            return;
        };

        let next_position = TableCellPosition {
            row: next_row,
            column: position.column.min(runtime.header.len().saturating_sub(1)),
        };
        let _ = self.focus_table_cell_position(table_block, next_position, cx);
    }

    pub(crate) fn on_table_cell_event(
        &mut self,
        binding: crate::editor::TableCellBinding,
        event: &BlockEvent,
        cx: &mut Context<Self>,
    ) {
        if Self::block_event_clears_cross_block_selection(event) {
            self.rendered_select_all_cycle = None;
            self.clear_cross_block_selection(cx);
        }

        match event {
            BlockEvent::Changed => {
                self.sync_table_record_from_runtime(&binding.table_block, cx);
                if self.changed_block_needs_runtime_context_refresh(&binding.cell, cx) {
                    self.rebuild_image_runtimes(cx);
                }
                // 打字打进格子就只改那一格的内容字节：同一行的列宽填充、表外的块
                // 都不该跟着重排。量不出这一格的位置（引用块里的表格没有自己的
                // 区间）才退回整张表的区间。
                if self.write_back_table_cell_source(&binding, cx) {
                    self.mark_dirty_written_back(cx);
                } else {
                    self.write_back_table_structure_edit(&binding.table_block, cx);
                }
                self.request_active_block_scroll_into_view(cx);
                self.finalize_pending_undo_capture(cx);
            }
            BlockEvent::RequestOpenLink { open_target } => {
                self.defer_open_link(open_target.clone(), cx);
            }
            BlockEvent::RequestJumpToFootnoteDefinition { id, .. } => {
                let _ = self.jump_to_footnote_definition(id, cx);
            }
            BlockEvent::RequestJumpToFootnoteBackref { id } => {
                let _ = self.jump_to_footnote_backref(id, cx);
            }
            BlockEvent::RequestTableCellMoveHorizontal { delta } => {
                self.focus_table_cell_horizontal_neighbor(
                    &binding.table_block,
                    binding.position,
                    *delta,
                    cx,
                );
            }
            BlockEvent::RequestTableCellMoveVertical { delta } => {
                self.focus_table_cell_vertical_neighbor(
                    &binding.table_block,
                    binding.position,
                    *delta,
                    cx,
                );
            }
            BlockEvent::RequestNewline { .. } => {
                let Some(location) = self
                    .document
                    .find_block_location(binding.table_block.entity_id())
                else {
                    return;
                };
                self.clear_table_axis_preview(cx);
                self.clear_table_axis_selection(cx);
                self.sync_table_record_from_runtime(&binding.table_block, cx);
                self.prepare_undo_capture(crate::components::UndoCaptureKind::NonCoalescible, cx);
                let new_block = Self::new_block(cx, BlockRecord::paragraph(String::new()));
                self.document.insert_blocks_at(
                    location.parent,
                    location.index + 1,
                    vec![new_block.clone()],
                    cx,
                );
                self.rebuild_image_runtimes(cx);
                self.focus_block(new_block.entity_id());
                self.mark_dirty(cx);
                self.request_active_block_scroll_into_view(cx);
                self.finalize_pending_undo_capture(cx);
                cx.notify();
            }
            BlockEvent::RequestFocus => {
                self.close_menu_bar(cx);
                self.clear_table_axis_preview(cx);
                self.clear_table_axis_selection(cx);
                self.focus_block(binding.cell.entity_id());
                cx.notify();
            }
            BlockEvent::RequestFocusPrev { .. } => {
                self.focus_table_cell_vertical_neighbor(
                    &binding.table_block,
                    binding.position,
                    -1,
                    cx,
                );
            }
            BlockEvent::RequestFocusNext { .. } => {
                self.focus_table_cell_vertical_neighbor(
                    &binding.table_block,
                    binding.position,
                    1,
                    cx,
                );
            }
            // Block Up/Down treat the table as a single block: leave it
            // entirely for the block above/below rather than stepping by cell.
            BlockEvent::RequestBlockUp => {
                self.focus_block_adjacent_to_table(&binding.table_block, -1, true, cx);
            }
            BlockEvent::RequestBlockDown => {
                self.focus_block_adjacent_to_table(&binding.table_block, 1, true, cx);
            }
            _ => {}
        }
    }
}
