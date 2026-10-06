//! 表格里跨格子的文本选区。
//!
//! 表格在文件里是一格一段字节，格与格之间隔着管道符、行与行之间隔着分隔行；用户眼里
//! 的表格却是一片连着的文本：从第一格的 a 拖到最后一格的 d，就该把 a b c d 四个字都
//! 选上，复制出去是这几格的字（同行的格用制表符接，行与行之间换行）。
//!
//! 所以这里存的是**格内的可见文本偏移**（与块内选区间一套干净坐标）：高亮按格切段铺到
//! 格子的 `editor_selection_range` 上（与跨块选区同一套换算与画法），复制与删除按同一份
//! 切段走。格子自己的块内选区不受影响——指针没离开按下那一格时，选文字还是格子自己的事。

use super::*;
use crate::components::TableCellPosition;

/// 表格里的一处文本位置：第几行第几列的那一格，加上这一格里的**干净**可见偏移。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TableTextPosition {
    pub(crate) cell: TableCellPosition,
    pub(crate) offset: usize,
}

/// 跨格的文本选区：从锚点选到落点。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TableTextSelection {
    pub(crate) table_block_id: EntityId,
    pub(crate) anchor: TableTextPosition,
    pub(crate) focus: TableTextPosition,
}

impl TableTextPosition {
    /// 行优先序：先比行、再比列，最后比格内偏移（与表格在文件里的顺序一致）。
    fn order_key(self) -> (usize, usize, usize) {
        (self.cell.row, self.cell.column, self.offset)
    }
}

impl TableTextSelection {
    /// 端点按行优先序归位：从下往上、从右往左拖都是同一段文字。
    pub(crate) fn normalized(self) -> (TableTextPosition, TableTextPosition) {
        if self.focus.order_key() < self.anchor.order_key() {
            (self.focus, self.anchor)
        } else {
            (self.anchor, self.focus)
        }
    }
}

impl Editor {
    /// 屏幕上这一点对应的表格文本位置（哪张表、哪一格、格内第几个字）。
    ///
    /// 格子的块内偏移存的是干净坐标，与块内选区同一条口径。
    pub(crate) fn table_text_position_at_point(
        &self,
        position: Point<Pixels>,
        cx: &App,
    ) -> Option<(EntityId, TableTextPosition)> {
        let (table_block_id, cell_position) = self.table_cell_at_point(position, cx)?;
        let binding = self
            .table_cells
            .values()
            .find(|binding| {
                binding.table_block.entity_id() == table_block_id
                    && binding.position == cell_position
            })?;
        let cell = binding.cell.read(cx);
        let offset = cell.current_to_clean_offset(cell.index_for_mouse_position(position));
        Some((
            table_block_id,
            TableTextPosition {
                cell: cell_position,
                offset,
            },
        ))
    }

    /// 按下那一刻的表格文本位置：拖动从这里起算。
    pub(crate) fn table_text_anchor_at_point(
        &self,
        position: Point<Pixels>,
        cx: &App,
    ) -> Option<(EntityId, TableTextPosition)> {
        self.table_text_position_at_point(position, cx)
    }

    /// 把锚点到落点之间的格子文字选上。
    ///
    /// 指针离开过按下那一格才由编辑器接手（没离开时选字还是格子自己的块内选区）；
    /// 接手时先把跨块选区让出来——同一时刻只该有一段选区说话。
    pub(crate) fn select_table_text_range(
        &mut self,
        table_block_id: EntityId,
        anchor: TableTextPosition,
        focus: TableTextPosition,
        cx: &mut Context<Self>,
    ) {
        let next = TableTextSelection {
            table_block_id,
            anchor,
            focus,
        };
        if self.table_text_selection.as_ref() == Some(&next) {
            return;
        }
        self.table_text_selection = Some(next);
        if self.cross_block_selection.take().is_some() {
            self.clear_cross_block_selection_visuals(cx);
        }
        self.sync_table_text_selection_visuals(cx);
        cx.notify();
    }

    /// 收起跨格选区（连同格子上的高亮）。没有选区时是空动作。
    pub(crate) fn clear_table_text_selection(&mut self, cx: &mut Context<Self>) -> bool {
        if self.table_text_selection.take().is_none() {
            return false;
        }
        self.sync_table_text_selection_visuals(cx);
        cx.notify();
        true
    }

    /// 选区覆盖的每一格：格内要选中的那一段（干净坐标）与格子本身，按表格的行优先序。
    ///
    /// 落点那一格只选到落点、起点那一格从起点选到格尾，中间的格子整格都在选区里——
    /// 表格当成一片连着的文本时，这段区间就是它落在格子上的切段。
    fn table_text_selection_cells(
        &self,
        cx: &App,
    ) -> Option<(Entity<Block>, Vec<(TableCellPosition, Entity<Block>, Range<usize>)>)> {
        let selection = self.table_text_selection?;
        let table = self.document.block_entity_by_id(selection.table_block_id)?;
        let (start, end) = selection.normalized();
        let cells = self.table_cells_in_text_range(&table, start, end, cx);
        Some((table, cells))
    }

    /// 写到 `start..end` 之间的那几格（按 `TableCellPosition` 行优先序）。
    fn table_cells_in_text_range(
        &self,
        table: &Entity<Block>,
        start: TableTextPosition,
        end: TableTextPosition,
        cx: &App,
    ) -> Vec<(TableCellPosition, Entity<Block>, Range<usize>)> {
        let Some(runtime) = table.read(cx).table_runtime.clone() else {
            return Vec::new();
        };
        let mut cells = Vec::new();
        let mut rows: Vec<&Vec<Entity<Block>>> = Vec::with_capacity(runtime.rows.len() + 1);
        rows.push(&runtime.header);
        rows.extend(runtime.rows.iter());
        for (row, row_cells) in rows.into_iter().enumerate() {
            for (column, cell) in row_cells.iter().enumerate() {
                let position = TableCellPosition { row, column };
                let start_key = (start.cell.row, start.cell.column);
                let end_key = (end.cell.row, end.cell.column);
                if (row, column) < start_key || (row, column) > end_key {
                    continue;
                }
                let len = cell.read(cx).clean_visible_len();
                let range = if start.cell == end.cell {
                    start.offset.min(len)..end.offset.min(len)
                } else if position == start.cell {
                    start.offset.min(len)..len
                } else if position == end.cell {
                    0..end.offset.min(len)
                } else {
                    0..len
                };
                if range.is_empty() {
                    continue;
                }
                cells.push((position, cell.clone(), range));
            }
        }
        cells
    }

    /// 把跨格选区铺到格子上，并把上一次铺过的收回去。
    ///
    /// 表格整张落在一个跨块选区里时也走这里：整张表的文字都算选中。
    pub(crate) fn sync_table_text_selection_visuals(&mut self, cx: &mut Context<Self>) {
        for (table_block_id, position) in std::mem::take(&mut self.table_text_selection_painted) {
            let Some(cell) = self.table_cell_entity(table_block_id, position, cx) else {
                continue;
            };
            cell.update(cx, |cell, cx| {
                if cell.editor_selection_range.take().is_some() {
                    cx.notify();
                }
            });
        }

        let cells = match self.table_text_selection_cells(cx) {
            Some((_, cells)) => cells,
            None => self.cross_block_covered_table_cells(cx),
        };
        let mut painted = Vec::with_capacity(cells.len());
        for (position, cell, range) in cells {
            let display_range = cell.read(cx).clean_range_to_display_range(range);
            if display_range.is_empty() {
                continue;
            }
            let table_block_id = self.table_cell_host_table(cell.entity_id());
            if let Some(table_block_id) = table_block_id {
                painted.push((table_block_id, position));
            }
            cell.update(cx, |cell, cx| {
                if cell.editor_selection_range != Some(display_range.clone()) {
                    cell.editor_selection_range = Some(display_range.clone());
                    cx.notify();
                }
            });
        }
        self.table_text_selection_painted = painted;
    }

    /// 整张表都在一个跨块选区里时，每一格的文字整格算选中。
    fn cross_block_covered_table_cells(
        &self,
        cx: &App,
    ) -> Vec<(TableCellPosition, Entity<Block>, Range<usize>)> {
        let Some(selection) = self.normalized_cross_block_selection(cx) else {
            return Vec::new();
        };
        let visible_blocks = self.document.visible_blocks().to_vec();
        let mut cells = Vec::new();
        for (index, visible) in visible_blocks.into_iter().enumerate() {
            if index < selection.start_index || index > selection.end_index {
                continue;
            }
            let table = visible.entity.clone();
            let Some(runtime) = table.read(cx).table_runtime.clone() else {
                continue;
            };
            let columns = runtime
                .header
                .len()
                .max(runtime.rows.iter().map(Vec::len).max().unwrap_or(0));
            if columns == 0 {
                continue;
            }
            let last_row = runtime.rows.len();
            let last_column = columns - 1;
            let last_cell = if last_row == 0 {
                runtime.header.get(last_column).cloned()
            } else {
                runtime
                    .rows
                    .get(last_row - 1)
                    .and_then(|row| row.get(last_column))
                    .cloned()
            };
            let Some(last_cell) = last_cell else {
                continue;
            };
            // 整张表这一段文本：从第一格的字首到最后一格的字尾。
            let start = TableTextPosition {
                cell: TableCellPosition { row: 0, column: 0 },
                offset: 0,
            };
            let end = TableTextPosition {
                cell: TableCellPosition {
                    row: last_row,
                    column: last_column,
                },
                offset: last_cell.read(cx).clean_visible_len(),
            };
            cells.extend(self.table_cells_in_text_range(&table, start, end, cx));
        }
        cells
    }

    /// 表格里某一格现在的实体（位置对得上、运行时还在就一定有）。
    pub(super) fn table_cell_entity(
        &self,
        table_block_id: EntityId,
        position: TableCellPosition,
        cx: &App,
    ) -> Option<Entity<Block>> {
        let table = self.document.block_entity_by_id(table_block_id)?;
        let runtime = table.read(cx).table_runtime.clone()?;
        if position.row == 0 {
            runtime.header.get(position.column).cloned()
        } else {
            runtime
                .rows
                .get(position.row - 1)
                .and_then(|row| row.get(position.column))
                .cloned()
        }
    }

    /// 这一格属于哪张表（格子不注册在块树里，只能问绑定表）。
    pub(super) fn table_cell_host_table(&self, cell_id: EntityId) -> Option<EntityId> {
        self.table_cells
            .get(&cell_id)
            .map(|binding| binding.table_block.entity_id())
    }

    /// 跨格选区交出去的文本：同行的格用制表符接，行与行之间换行（用户口径）。
    ///
    /// 读的是屏幕上那串字（干净坐标），不带管道符、不带列宽填充、不带格子里的记号。
    pub(crate) fn table_text_selection_text(&self, cx: &App) -> Option<String> {
        let (_, cells) = self.table_text_selection_cells(cx)?;
        let mut text = String::new();
        let mut current_row: Option<usize> = None;
        for (position, cell, range) in cells {
            let cell_state = cell.read(cx);
            let clean = cell_state.record.title.visible_text();
            let range = clamp_range_to_char_boundaries(&clean, range);
            if range.is_empty() {
                continue;
            }
            match current_row {
                Some(row) if row == position.row => text.push('\t'),
                Some(_) => text.push('\n'),
                None => {}
            }
            text.push_str(&clean[range]);
            current_row = Some(position.row);
        }
        (!text.is_empty()).then_some(text)
    }

    /// 跨格选区覆盖的那几段字节（缓冲区坐标）。
    ///
    /// 每格一段：格子里选中的那一段折到原文里就是一段连续字节（格的字节区间由解析期
    /// 按结构量出来，格内偏移按这一格自己的映射换算）。算不出任何一段就返回 `None`——
    /// 宁可不删，也不在半张表上乱落笔。
    fn table_text_selection_source_ranges(&self, cx: &App) -> Option<Vec<Range<usize>>> {
        let (_, cells) = self.table_text_selection_cells(cx)?;
        let mut ranges = Vec::new();
        for (_, cell, range) in cells {
            let Some(mapping) = self.source_mapping_for_entity(cell.entity_id(), cx) else {
                return None;
            };
            let Some(start) = self.mapping_source_range_endpoint(&mapping, range.start, false, cx)
            else {
                return None;
            };
            let Some(end) = self.mapping_source_range_endpoint(&mapping, range.end, true, cx) else {
                return None;
            };
            if start < end {
                ranges.push(start..end);
            }
        }
        (!ranges.is_empty()).then_some(ranges)
    }

    /// 用 `new_text` 换掉跨格选区里的那几段字（删除就是换空串）。
    ///
    /// 落笔按字节走：格子与格子之间在文件里隔着管道符，只能一格一段地删，从后往前删
    /// （前面的字节位置才不会失效），最后把新文字插在选区末尾那一段的落点上——一次
    /// 撤销组，其余字节一个不动。光标交回给落点那一格，不然删完没有下一次敲字的地方。
    pub(crate) fn replace_table_text_selection_with_text(
        &mut self,
        new_text: &str,
        undo_kind: UndoCaptureKind,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(selection) = self.table_text_selection else {
            return false;
        };
        let Some(mut ranges) = self.table_text_selection_source_ranges(cx) else {
            return false;
        };
        let (start, _) = selection.normalized();
        ranges.sort_by_key(|range| range.start);
        // 新文字落在选区**起点**那一段的开头（与跨块替换同一条口径）。从后往前删，
        // 起点的字节位置不会被后面的删除挪动。
        let Some(insertion) = ranges.first().map(|range| range.start) else {
            return false;
        };

        self.prepare_undo_capture(undo_kind, cx);
        self.table_text_selection = None;
        self.sync_table_text_selection_visuals(cx);
        for range in ranges.iter().rev() {
            let applied = self.buffer.edit(range.clone(), "");
            self.record_buffer_edit(applied);
        }
        if !new_text.is_empty() {
            let applied = self.buffer.edit(insertion..insertion, new_text);
            self.record_buffer_edit(applied);
        }
        self.rebuild_document_from_buffer(cx);
        // 光标回到起点那一格新文字的末尾，不然删完/打完没有下一次敲字的地方。
        let caret = start.offset + new_text.len();
        if let Some(cell) = self.table_cell_entity(selection.table_block_id, start.cell, cx) {
            let cell_id = cell.entity_id();
            cell.update(cx, |cell, cx| {
                let caret = cell.clean_to_current_cursor_offset(caret);
                cell.selected_range = caret..caret;
                cell.selection_reversed = false;
                cx.notify();
            });
            self.focus_block(cell_id);
        }
        self.mark_dirty_written_back(cx);
        self.finalize_pending_undo_capture(cx);
        cx.notify();
        true
    }
}
