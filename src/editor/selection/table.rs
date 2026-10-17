//! 表格里跨格子的文本选区。
//!
//! 表格在文件里是一格一段字节，格与格之间隔着管道符、行与行之间隔着分隔行；用户眼里
//! 的表格却是一片连着的文本：从第一格的 a 拖到最后一格的 d，就该把 a b c d 四个字都
//! 选上，复制出去是这几格的字（同行的格用制表符接，行与行之间换行）。
//!
//! 所以这里存的是**格内的可见文本偏移**（与块内选区间一套干净坐标）：高亮按格切段铺到
//! 格子的 `editor_selection_clean_range` 上（与跨块选区同一套换算与画法），复制与删除按同一份
//! 切段走。格子自己的块内选区不受影响——指针没离开按下那一格时，选文字还是格子自己的事。

use super::*;
use crate::components::{TableCellPosition, InlineTextTree, serialize_table_cell_markdown};

/// 表格里的一处文本位置：第几行第几列的那一格，加上这一格里的**干净**可见偏移。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TableTextPosition {
    pub(crate) cell: TableCellPosition,
    pub(crate) offset: usize,
    pub(crate) whole_cell: bool,
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
    pub(crate) fn table_text_selection_snapshot(&self, cx: &App) -> Option<UndoSelectionSnapshot> {
        let selection = self.table_text_selection?;
        let (start, end) = selection.normalized();
        let endpoint = |position: TableTextPosition, at_end: bool| {
            let cell = self.table_cell_entity(selection.table_block_id, position.cell, cx)?;
            let mapping = self.source_mapping_for_entity(cell.entity_id(), cx)?;
            let offset = if position.whole_cell {
                if at_end { cell.read(cx).clean_visible_len() } else { 0 }
            } else { position.offset };
            self.mapping_source_range_endpoint(&mapping, offset, at_end, cx)
        };
        Some(UndoSelectionSnapshot {
            range: endpoint(start, false)?..endpoint(end, true)?,
            reversed: selection.anchor.order_key() > selection.focus.order_key(),
        })
    }

    pub(crate) fn apply_table_text_selection_snapshot(
        &mut self,
        snapshot: &UndoSelectionSnapshot,
        cx: &mut Context<Self>,
    ) -> bool {
        if snapshot.range.is_empty() {
            return false;
        }
        let mappings = self.source_mappings_in_range(&snapshot.range, cx);
        let endpoint = |offset| {
            let mapping = mappings.iter().find(|mapping| {
                self.table_cells.contains_key(&mapping.entity.entity_id())
                    && Self::source_range_contains(&mapping.full_source_range, offset)
            })?;
            let binding = self.table_cells.get(&mapping.entity.entity_id())?;
            let local = offset.saturating_sub(mapping.full_source_range.start);
            let content = mapping.source_to_content.get(local).copied()?;
            let clean = mapping
                .entity
                .read(cx)
                .record
                .title
                .markdown_offset_map()
                .markdown_to_visible_offset(content);
            Some((
                binding.table_block.entity_id(),
                TableTextPosition {
                    cell: binding.position,
                    offset: clean,
                    whole_cell: false,
                },
            ))
        };
        let Some((start_table, start)) = endpoint(snapshot.range.start) else {
            return false;
        };
        let Some((end_table, end)) = endpoint(snapshot.range.end) else {
            return false;
        };
        if start_table != end_table || start.cell == end.cell {
            return false;
        }
        let (anchor, focus) = if snapshot.reversed {
            (end, start)
        } else {
            (start, end)
        };
        self.select_table_text_range(start_table, anchor, focus, cx);
        if let Some(cell) = self.table_cell_entity(start_table, focus.cell, cx) {
            self.focus_block(cell.entity_id());
        }
        true
    }

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
                whole_cell: !cell.has_text_layout(),
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
        mut anchor: TableTextPosition,
        mut focus: TableTextPosition,
        cx: &mut Context<Self>,
    ) {
        if anchor.cell == focus.cell && (anchor.whole_cell || focus.whole_cell) {
            anchor.whole_cell = true;
            focus.whole_cell = true;
        }
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

    /// 某一格在选区里那一段（干净坐标）。
    ///
    /// 判据只此一处：高亮 / 删除那条路（`table_cells_in_text_range`）与文本拼接都从
    /// 它取，空区间由调用方决定跳不跳——高亮与删除可以跳（没东西可画可删），复制不行
    /// （列位要留）。
    fn table_cell_range_in_text_selection(
        start: TableTextPosition,
        end: TableTextPosition,
        position: TableCellPosition,
        len: usize,
    ) -> Range<usize> {
        // 按下后焦点可能显形出文字布局；整格语义属于端点，不能随下一帧改变。
        let start_offset = if start.whole_cell { 0 } else { start.offset.min(len) };
        let end_offset = if end.whole_cell { len } else { end.offset.min(len) };
        if start.cell == end.cell {
            start_offset.min(end_offset)..start_offset.max(end_offset)
        } else if position == start.cell {
            start_offset..len
        } else if position == end.cell {
            0..end_offset
        } else {
            0..len
        }
    }

    /// 写到 `start..end` 之间的那几格（按 `TableCellPosition` 行优先序）。
    ///
    /// 空区间整格剔掉：这一份给高亮与删除用。要给用户看的文本走
    /// `table_text_selection_text`，那边按列全量补位。
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
                let block = cell.read(cx);
                let len = block.clean_visible_len();
                let range = Self::table_cell_range_in_text_selection(start, end, position, len);
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
        self.sync_document_selection_ownership(cx);
        if let Some(selection) = self.table_text_selection {
            for endpoint in [selection.anchor, selection.focus] {
                if let Some(cell) = self.table_cell_entity(selection.table_block_id, endpoint.cell, cx) {
                    let needs_binding = cell.read(cx).document_selection_active.as_ref()
                        .is_none_or(|current| !std::rc::Rc::ptr_eq(current, &self.document_selection_active));
                    if needs_binding {
                        cell.update(cx, |cell, _| cell.document_selection_active = Some(self.document_selection_active.clone()));
                    }
                }
            }
        }
        for (table_block_id, position) in std::mem::take(&mut self.table_text_selection_painted) {
            let Some(cell) = self.table_cell_entity(table_block_id, position, cx) else {
                continue;
            };
            cell.update(cx, |cell, cx| {
                if cell.editor_selection_clean_range.take().is_some() {
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
            if range.is_empty() {
                continue;
            }
            let table_block_id = self.table_cell_host_table(cell.entity_id());
            if let Some(table_block_id) = table_block_id {
                painted.push((table_block_id, position));
            }
            cell.update(cx, |cell, cx| {
                cell.document_selection_active = Some(self.document_selection_active.clone());
                if cell.editor_selection_clean_range != Some(range.clone()) {
                    cell.editor_selection_clean_range = Some(range.clone());
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
        let visible_blocks = self.document.visible_blocks().get(selection.start_index..=selection.end_index).unwrap_or(&[]);
        let mut cells = Vec::new();
        for visible in visible_blocks {
            #[cfg(test)]
            self.selection_visual_visits.set(self.selection_visual_visits.get() + 1);
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
                whole_cell: false,
            };
            let end = TableTextPosition {
                cell: TableCellPosition {
                    row: last_row,
                    column: last_column,
                },
                offset: last_cell.read(cx).clean_visible_len(),
                whole_cell: false,
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
    /// 取的格与高亮、删除完全一致（行优先线性序），并且**全量补位**：夹在中间的空格子
    /// 列位也要留出来，不然 `a | (空) | b` 复制出来成了 `a\tb`，粘回表里会串列。
    pub(crate) fn table_text_selection_text(&self, cx: &App) -> Option<String> {
        let selection = self.table_text_selection?;
        let table = self.document.block_entity_by_id(selection.table_block_id)?;
        let runtime = table.read(cx).table_runtime.clone()?;
        let (start, end) = selection.normalized();
        let start_key = (start.cell.row, start.cell.column);
        let end_key = (end.cell.row, end.cell.column);
        let mut text = String::new();
        let mut written_rows = 0usize;
        for row in start.cell.row..=end.cell.row {
            let row_cells = if row == 0 {
                Some(&runtime.header)
            } else {
                runtime.rows.get(row - 1)
            };
            let Some(row_cells) = row_cells else {
                continue;
            };
            let mut row_text = String::new();
            let mut written_cells = 0usize;
            // 取哪几格与高亮、删除用同一个判断：行优先的线性序 `start_key..=end_key`，
            // 不是行列两向的矩形——反向拖动（落点列小于锚点列）时矩形会退化成空区间，
            // 于是格子被选中、被删掉，复制却交出空串。
            for column in 0..row_cells.len() {
                if (row, column) < start_key || (row, column) > end_key {
                    continue;
                }
                let Some(cell) = row_cells.get(column) else {
                    continue;
                };
                let cell_state = cell.read(cx);
                let clean = cell_state.record.title.visible_text();
                let len = cell_state.clean_visible_len();
                let position = TableCellPosition { row, column };
                let range =
                    Self::table_cell_range_in_text_selection(start, end, position, len);
                // 端点停在格边（格首 / 格尾）时那一格整格不算在选区里：与高亮那条路
                // 同一口径（`table_cells_in_text_range` 把空区间整格剔掉），这也是
                // `dragging_between_cell_edges_keeps_the_text_granularity` 钉住的语义。
                // 夹在中间的格子不受此限：空也要占它那一列（列位不能丢）。
                if range.is_empty() && (position == start.cell || position == end.cell) {
                    continue;
                }
                let range = clamp_range_to_char_boundaries(&clean, range);
                if written_cells > 0 {
                    row_text.push('\t');
                }
                row_text.push_str(&clean[range]);
                written_cells += 1;
            }
            if written_cells == 0 {
                continue;
            }
            if written_rows > 0 {
                text.push('\n');
            }
            text.push_str(&row_text);
            written_rows += 1;
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
                eprintln!("无法修改表格选区：单元格 {:?} 缺少源码映射", cell.entity_id());
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
        selected_range_relative: Option<Range<usize>>,
        mark_inserted_text: bool,
        undo_kind: UndoCaptureKind,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.is_read_only() {
            return false;
        }

        if self.table_text_selection.is_none() {
            return false;
        }
        let Some(mut ranges) = self.table_text_selection_source_ranges(cx) else {
            return false;
        };
        ranges.sort_by_key(|range| range.start);
        // 新文字落在选区**起点**那一段的开头（与跨块替换同一条口径）。从后往前删，
        // 起点的字节位置不会被后面的删除挪动。
        let Some(insertion) = ranges.first().map(|range| range.start) else {
            return false;
        };
        let serialized_input = |input: &str| {
            let flattened = crate::components::flatten_table_cell_input(input);
            serialize_table_cell_markdown(&InlineTextTree::plain(flattened))
        };
        let inserted = serialized_input(new_text);
        let source_offset = |offset| {
            let offset = crate::components::markdown::inline::clamp_to_char_boundary(new_text, offset);
            insertion + serialized_input(new_text.get(..offset).unwrap_or_default()).len()
        };
        let selected_source = selected_range_relative.map(|relative| source_offset(relative.start)..source_offset(relative.end))
            .unwrap_or(insertion + inserted.len()..insertion + inserted.len());

        self.prepare_undo_capture(undo_kind, cx);
        self.table_text_selection = None;
        self.sync_table_text_selection_visuals(cx);
        for range in ranges.iter().rev() {
            let applied = self.buffer.edit(range.clone(), "");
            self.record_buffer_edit(applied);
        }
        if !inserted.is_empty() {
            let applied = self.buffer.edit(insertion..insertion, &inserted);
            self.record_buffer_edit(applied);
        }
        self.rebuild_document_from_buffer(cx);
        // 光标回到起点那一格新文字的末尾，不然删完/打完没有下一次敲字的地方。
        self.apply_selection_snapshot_in_current_mode(&UndoSelectionSnapshot { range: selected_source, reversed: false }, cx);
        if mark_inserted_text && !inserted.is_empty() {
            self.apply_marked_source_range(insertion..insertion + inserted.len(), cx);
        }
        self.mark_dirty_written_back(cx);
        self.finalize_pending_undo_capture(cx);
        cx.notify();
        true
    }
}
