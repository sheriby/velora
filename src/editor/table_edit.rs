//! Native table runtime installation and table-editing operations.

use std::ops::Range;

use super::*;

impl Editor {
    /// 这一格在缓冲区原文里占的字节区间：按「第几行第几列」从管道符之间量出来。
    ///
    /// 不能拿格子的文本去原文里找——用户刚打的字还没进文件，按新文本搜必然搜不到。
    /// 返回的是内容区间（两侧的空格填充不算），所以写回去只动这一格的内容，
    /// 同一行别的列的列宽填充一个字节都不动。
    pub(crate) fn table_cell_source_range(
        &self,
        binding: &TableCellBinding,
        cx: &App,
    ) -> Option<Range<usize>> {
        let span = binding.table_block.read(cx).record.source_span.clone()?;
        let raw = self.buffer.slice(span.clone());
        // 视觉行 0 是表头（源码第 0 行），1 起是数据行：中间那条分隔行没有格子。
        let line_index = if binding.position.row == 0 {
            0
        } else {
            binding.position.row + 1
        };
        let mut line_start = span.start;
        for (index, line) in raw.split('\n').enumerate() {
            if index == line_index {
                return cell_content_range_in_line(line, line_start, binding.position.column);
            }
            line_start += line.len() + 1;
        }
        None
    }

    /// 打字打进单元格：只把这一格的内容写回它自己的字节区间。
    ///
    /// 整张表重新序列化会把列宽填充重排（`| 甲   |` 变 `| 甲 |`），整篇重同步更会
    /// 把表外的块一起洗。这一格之外的字节——包括同一行的其它列——原样保留。
    pub(crate) fn write_back_table_cell_source(
        &mut self,
        binding: &TableCellBinding,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(old_span) = self.table_cell_source_range(binding, cx) else {
            return false;
        };
        let new_source = serialize_table_cell_markdown(&binding.cell.read(cx).record.title);
        if self.buffer.slice(old_span.clone()) == new_source {
            return true;
        }

        let old_len = old_span.end - old_span.start;
        let applied = self.buffer.edit(old_span, &new_source);
        self.record_buffer_edit(applied.clone());
        let delta = new_source.len() as i64 - old_len as i64;
        if delta == 0 {
            return true;
        }
        // 表根块的区间要跟着涨：这一格就在它里面。它后面的根块整体平移。
        if let Some(table_span) = binding.table_block.read(cx).record.source_span.clone() {
            binding.table_block.update(cx, |block, _cx| {
                block.record.source_span =
                    Some(table_span.start..(table_span.end as i64 + delta) as usize);
            });
        }
        self.shift_root_spans_after(applied.new_range.end, delta, cx);
        true
    }

    /// 这张表在缓冲区里占的源码行：行号 + 行内容，从本块区间第一行往后走，
    /// 走到不再以 `|` 开头为止。分隔行也算一行，所以第 `n` 个数据行是第 `n + 2` 行。
    ///
    /// 只在表自己的区间里走，行数对不上（单元格里有换行、表挂在容器里没有区间）时
    /// 调用方拿到的结果就不该用来落笔。
    fn table_source_lines(&self, span: &Range<usize>) -> Vec<(usize, String)> {
        let mut lines = Vec::new();
        let mut line = self.buffer.line_of(span.start);
        let final_line = self.buffer.line_of(span.end);
        while line <= final_line {
            let text = self.buffer.slice(self.buffer.line_range(line));
            if !text.trim_start().starts_with('|') {
                break;
            }
            lines.push((line, text));
            line += 1;
        }
        lines
    }

    /// 表格加一行 = 在最后一行之后插一行，别的字节一个都不动。
    ///
    /// 把整张表按模型重拼一遍会顺手洗掉用户写的列宽填充：`| 名称   | 数量 |` 变成
    /// `| 名称 | 数量 |`、`|:-------|-----:|` 变成 `| :--- | ---: |`，而用户只是
    /// 在表尾加了一行。文件里表格的一行就是文本的一行，所以按最后一行的骨架补一行
    /// （把每格内容换成等长空白），列宽与对齐写法原样留着。
    ///
    /// 返回 `false` 表示这行插不出来：表没有自己的区间（挂在容器里）、行里数出的
    /// 竖线跟列数对不上（单元格里有转义竖线），那种情况交回整块写。
    fn write_back_table_row_insertion(
        &mut self,
        table_block: &Entity<Block>,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(span) = table_block.read(cx).record.source_span.clone() else {
            return false;
        };
        let Some(columns) = table_block
            .read(cx)
            .record
            .table
            .as_ref()
            .map(|table| table.column_count())
        else {
            return false;
        };
        let Some((last_row_line, last_row_text)) = self.table_source_lines(&span).pop() else {
            return false;
        };
        // 竖线数对不上列数，说明格子里有转义竖线，骨架算不准。
        if last_row_text.chars().filter(|character| *character == '|').count() != columns + 1 {
            return false;
        }
        let new_row = last_row_text
            .split('|')
            .map(|segment| {
                if segment.chars().all(|character| character.is_whitespace()) {
                    segment.to_string()
                } else {
                    " ".repeat(segment.chars().count())
                }
            })
            .collect::<Vec<_>>()
            .join("|");

        let inserted = format!("\n{new_row}");
        let offset = self.buffer.line_range(last_row_line).end;
        let applied = self.buffer.edit(offset..offset, &inserted);
        self.record_buffer_edit(applied);
        let delta = inserted.len() as i64;
        table_block.update(cx, |block, _cx| {
            if let Some(span) = &block.record.source_span {
                block.record.source_span = Some(span.start..(span.end as i64 + delta) as usize);
            }
        });
        self.shift_root_spans_after(offset, delta, cx);
        true
    }

    /// 表格删一行 = 把那一行连着它前面的换行剪掉，别的字节一个都不动。
    ///
    /// 数据行在源码里就是文本的一行：删行不需要重拼这张表，重拼会把表头对齐、分隔行
    /// 的 `:` 和其余各行的填充一起洗掉。剪掉的是 `前一行行尾` 到 `本行行尾`，所以删
    /// 的永远是行本身而不是它后面的换行——删最后一行时文档末行的换行才不会跟着没了。
    ///
    /// 返回 `false` 表示删不出来：模型和源码行数已经对不上（单元格里有换行）、这一行
    /// 不是数据行、竖线数跟列数不符。那种情况交回整块写。
    fn write_back_table_row_deletion(
        &mut self,
        table_block: &Entity<Block>,
        row_index: usize,
        cx: &mut Context<Self>,
    ) -> bool {
        let (Some(span), Some((columns, rows_left))) = (
            table_block.read(cx).record.source_span.clone(),
            table_block
                .read(cx)
                .record
                .table
                .as_ref()
                .map(|table| (table.column_count(), table.rows.len())),
        ) else {
            return false;
        };
        // 模型里那一行已经删掉了，源码里该还剩「表头 + 分隔行 + 剩下的数据行」。
        let lines = self.table_source_lines(&span);
        if lines.len() != rows_left + 3 {
            return false;
        }
        let target_line = match lines.get(row_index + 2) {
            Some((line, text))
                if text.chars().filter(|character| *character == '|').count() == columns + 1 =>
            {
                *line
            }
            _ => return false,
        };
        if target_line == 0 {
            return false;
        }

        let from = self.buffer.line_range(target_line - 1).end;
        let to = self.buffer.line_range(target_line).end;
        let applied = self.buffer.edit(from..to, "");
        self.record_buffer_edit(applied);
        let delta = from as i64 - to as i64;
        table_block.update(cx, |block, _cx| {
            if let Some(span) = &block.record.source_span {
                block.record.source_span = Some(span.start..(span.end as i64 + delta) as usize);
            }
        });
        self.shift_root_spans_after(to, delta, cx);
        true
    }

    /// 调一列的对齐 = 只改写分隔行里那一格，同一行的别的格都不动。
    ///
    /// 对齐写法就写在分隔行那一格的 `:` 上，整张表按模型重拼却会顺手把用户手写的
    /// 列宽填充重排掉。所以只重写那一格：按原来的 `-` 个数排，多出来的冒号从这一格
    /// 自己的填充里腾，腾不下才让这一格变长。
    ///
    /// 返回 `false` 表示这一格改不出来：那一行不是干净的对齐写法（夹了别的字符）、
    /// 行数与模型对不上、表没有自己的区间。那种情况交回整块写。
    fn write_back_table_column_alignment(
        &mut self,
        table_block: &Entity<Block>,
        column: usize,
        alignment: TableColumnAlignment,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(span) = table_block.read(cx).record.source_span.clone() else {
            return false;
        };
        let Some((columns, rows)) = table_block
            .read(cx)
            .record
            .table
            .as_ref()
            .map(|table| (table.column_count(), table.rows.len()))
        else {
            return false;
        };
        let lines = self.table_source_lines(&span);
        if lines.len() != rows + 2 || column >= columns {
            return false;
        }
        let Some((delimiter_line, delimiter_text)) = lines.get(1).cloned() else {
            return false;
        };
        let mut cells = delimiter_text.split('|').collect::<Vec<_>>();
        if cells.len() != columns + 2 {
            return false;
        }
        let Some(new_cell) = realigned_delimiter_cell(cells[column + 1], alignment) else {
            return false;
        };
        if new_cell == cells[column + 1] {
            return true;
        }
        cells[column + 1] = &new_cell;
        let new_line = cells.join("|");

        let old_line_range = self.buffer.line_range(delimiter_line);
        let applied = self.buffer.edit(old_line_range.clone(), &new_line);
        self.record_buffer_edit(applied);
        let delta = new_line.len() as i64 - (old_line_range.end - old_line_range.start) as i64;
        if delta != 0 {
            table_block.update(cx, |block, _cx| {
                if let Some(span) = &block.record.source_span {
                    block.record.source_span =
                        Some(span.start..(span.end as i64 + delta) as usize);
                }
            });
            self.shift_root_spans_after(old_line_range.end, delta, cx);
        }
        true
    }

    /// 表格加一列 = 每行末尾多插这一列，已有的格一个字节都不动。
    ///
    /// 整张表按模型重拼会把用户手写的列宽填充和对齐写法一起重排，而新列自己该长
    /// 什么样模型里已经写了：数据行按模型里那一格，分隔行照抄它左边那格的写法（宽
    /// 度和风格跟着邻居，不再重排别人）。每行只在它末尾那根竖线之前插入
    /// `|` + 新格，所以插的顺序是从后往前——前面的字节不会因为后面的插入而挪位。
    ///
    /// 返回 `false` 表示插不出来：某行的竖线数跟列数对不上（格子里有转义竖线或不是
    /// 管道写法）、行数与模型对不上、表没有自己的区间。那种情况交回整块写。
    fn write_back_table_column_insertion(
        &mut self,
        table_block: &Entity<Block>,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(span) = table_block.read(cx).record.source_span.clone() else {
            return false;
        };
        let Some(table) = table_block.read(cx).record.table.clone() else {
            return false;
        };
        let lines = self.table_source_lines(&span);
        if lines.len() != table.rows.len() + 2 {
            return false;
        }
        // 模型里已经有这一列了，所以源码里每行该有「列数」根竖线（外层那两根算在内）。
        let columns = table.column_count();
        let columns = table.column_count();
        let mut inserts: Vec<(usize, usize, String)> = Vec::with_capacity(lines.len());
        for (index, (line, text)) in lines.iter().enumerate() {
            let pipes = unescaped_pipe_offsets(text);
            // 只认「外层两根竖线齐平」的管道写法：缩进的、结尾还有杂字的都算不出插点。
            let wrapped = pipes.first() == Some(&0) && pipes.last() == Some(&(text.len() - 1));
            if !wrapped || pipes.len() != columns {
                return false;
            }
            let cell = if index == 1 {
                // 分隔行：照抄左边那一格的写法。
                let start = pipes[pipes.len() - 2] + 1;
                text[start..pipes[pipes.len() - 1]].to_string()
            } else {
                let new_cell = if index == 0 {
                    table.header.last()
                } else {
                    table.rows.get(index - 2).and_then(|row| row.last())
                };
                let Some(new_cell) = new_cell else { return false };
                format!(" {} ", serialize_table_cell_markdown(new_cell))
            };
            inserts.push((*line, pipes[pipes.len() - 1], format!("|{cell}")));
        }

        // 表后面的根块整体右移：以原本最后一行的行尾为界，插点都在它前面。
        let boundary = self.buffer.line_range(inserts[inserts.len() - 1].0).end;
        let mut moved = 0usize;
        for (line, pipe, text) in inserts.into_iter().rev() {
            let offset = self.buffer.line_range(line).start + pipe;
            let applied = self.buffer.edit(offset..offset, &text);
            self.record_buffer_edit(applied);
            moved += text.len();
        }
        table_block.update(cx, |block, _cx| {
            if let Some(span) = &block.record.source_span {
                block.record.source_span =
                    Some(span.start..(span.end as i64 + moved as i64) as usize);
            }
        });
        self.shift_root_spans_after(boundary, moved as i64, cx);
        true
    }

    /// 表格结构命令只重写这张表自己的源码区间，表外的块一个字节都不动。
    ///
    /// `mark_dirty` 的整篇重同步会从块树把全文重新序列化：给一张表加一行，会把
    /// 别处的 `__强调__` 写法、Setext、CRLF 与末行换行一起洗掉。表这一级的改动
    /// 优先走按行落笔的路（[`Self::write_back_table_row_insertion`]、
    /// [`Self::write_back_table_row_deletion`]），只有算不出行形状时（转义竖线、
    /// 单元格里有换行、表挂在容器里没有自己的区间）才重拼这张表——那仍然只在它
    /// 自己的区间内。
    pub(super) fn write_back_table_structure_edit(
        &mut self,
        table_block: &Entity<Block>,
        cx: &mut Context<Self>,
    ) {
        if self.write_back_block_source(table_block, cx) {
            self.mark_dirty_written_back(cx);
        } else {
            self.mark_dirty(cx);
        }
    }

    pub(crate) fn new_table_block(cx: &mut Context<Self>, table: TableData) -> Entity<Block> {
        Self::new_block(cx, BlockRecord::table(table))
    }

    pub(super) fn install_table_runtime_for_block(
        &mut self,
        table_block: &Entity<Block>,
        table: &TableData,
        cx: &mut Context<Self>,
    ) {
        let header = table
            .header
            .iter()
            .cloned()
            .enumerate()
            .map(|(column, title)| {
                let alignment = table
                    .alignments
                    .get(column)
                    .copied()
                    .unwrap_or(TableColumnAlignment::Default);
                let position = TableCellPosition { row: 0, column };
                let cell = Self::new_table_cell_block(cx, title, position, alignment);
                self.table_cells.insert(
                    cell.entity_id(),
                    TableCellBinding {
                        table_block: table_block.clone(),
                        cell: cell.clone(),
                        position,
                    },
                );
                cell
            })
            .collect::<Vec<_>>();

        let rows = table
            .rows
            .iter()
            .cloned()
            .enumerate()
            .map(|(body_row_index, row)| {
                row.into_iter()
                    .enumerate()
                    .map(|(column, title)| {
                        let alignment = table
                            .alignments
                            .get(column)
                            .copied()
                            .unwrap_or(TableColumnAlignment::Default);
                        let position = TableCellPosition {
                            row: body_row_index + 1,
                            column,
                        };
                        let cell = Self::new_table_cell_block(cx, title, position, alignment);
                        self.table_cells.insert(
                            cell.entity_id(),
                            TableCellBinding {
                                table_block: table_block.clone(),
                                cell: cell.clone(),
                                position,
                            },
                        );
                        cell
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();

        table_block.update(cx, {
            let runtime = TableRuntime { header, rows };
            move |block, _cx| block.set_table_runtime(runtime.clone())
        });
    }

    pub(super) fn rebuild_table_runtimes(&mut self, cx: &mut Context<Self>) {
        self.table_cells.clear();
        self.table_axis_preview = None;
        let visible = self.document.visible_blocks().to_vec();
        // 表格重建会替换全部单元格实体：滚动/焦点锚点若指向旧 cell，重建后
        // 就成了文档树里查不到的悬空 id，跳转滚动永无坐标（用户报修：表格里
        // 的搜索命中点了没反应）。在旧 runtime 销毁前确认悬空锚点确是某表的
        // 单元格并记下宿主，重建后把锚点迁到宿主表格块。
        let mut stale_cell_host: Option<Entity<Block>> = None;
        if let Some(anchor) = self.active_entity_id.or(self.pending_focus)
            && self.document.block_entity_by_id(anchor).is_none()
            && let Some(host) = visible.iter().find_map(|visible| {
                let is_host = visible.entity.read(cx).table_runtime.as_ref().is_some_and(
                    |runtime| {
                        runtime
                            .rows
                            .iter()
                            .flatten()
                            .any(|cell| cell.entity_id() == anchor)
                    },
                );
                is_host.then_some(visible.entity.clone())
            })
        {
            stale_cell_host = Some(host);
        }
        for block in &visible {
            let has_table_state = block.entity.read_with(cx, |block, _cx| {
                block.kind() == BlockKind::Table || block.table_runtime.is_some()
            });
            if !has_table_state {
                continue;
            }
            block
                .entity
                .update(cx, |block, _cx| block.clear_table_runtime());
        }
        for visible in visible {
            let Some(table) = visible.entity.read(cx).record.table.clone() else {
                continue;
            };
            if visible.entity.read(cx).kind() == BlockKind::Table {
                self.install_table_runtime_for_block(&visible.entity, &table, cx);
            }
        }
        if let Some(host) = stale_cell_host {
            if self.active_entity_id.is_some_and(|id| {
                self.document.block_entity_by_id(id).is_none()
            }) {
                self.active_entity_id = Some(host.entity_id());
            }
            if self.pending_focus.is_some_and(|id| {
                self.document.block_entity_by_id(id).is_none()
            }) {
                self.pending_focus = Some(host.entity_id());
            }
        }
        self.rebuild_image_runtimes(cx);
        self.sync_table_axis_visuals(cx);
    }

    pub(super) fn sync_table_record_from_runtime(
        &mut self,
        table_block: &Entity<Block>,
        cx: &mut Context<Self>,
    ) {
        let Some(runtime) = table_block.read(cx).table_runtime.clone() else {
            return;
        };
        let alignments = table_block
            .read(cx)
            .record
            .table
            .as_ref()
            .map(|table| table.alignments.clone())
            .unwrap_or_default();
        let header = runtime
            .header
            .iter()
            .map(|cell| cell.read(cx).record.title.clone())
            .collect::<Vec<_>>();
        let rows = runtime
            .rows
            .iter()
            .map(|row| {
                row.iter()
                    .map(|cell| cell.read(cx).record.title.clone())
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        table_block.update(cx, move |block, _cx| {
            block.record.table = Some(TableData {
                header,
                rows,
                alignments,
            });
        });
    }

    pub(super) fn append_table_column(
        &mut self,
        table_block: &Entity<Block>,
        cx: &mut Context<Self>,
    ) {
        self.sync_table_record_from_runtime(table_block, cx);

        let Some(mut table) = table_block.read(cx).record.table.clone() else {
            return;
        };
        let started_local_capture = if self.pending_undo_capture.is_none() {
            self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
            true
        } else {
            false
        };
        let alignment = table
            .alignments
            .last()
            .copied()
            .unwrap_or(TableColumnAlignment::Default);
        table.append_column(alignment);

        table_block.update(cx, move |block, _cx| {
            block.record.table = Some(table.clone());
        });
        self.rebuild_table_runtimes(cx);
        if let Some(cell) = table_block
            .read(cx)
            .table_runtime
            .as_ref()
            .and_then(|runtime| runtime.header.last())
        {
            self.focus_block(cell.entity_id());
        }
        if self.write_back_table_column_insertion(table_block, cx) {
            self.mark_dirty_written_back(cx);
        } else {
            self.write_back_table_structure_edit(table_block, cx);
        }
        self.request_active_block_scroll_into_view(cx);
        if started_local_capture {
            self.finalize_pending_undo_capture(cx);
        }
        cx.notify();
    }

    pub(super) fn append_table_row(&mut self, table_block: &Entity<Block>, cx: &mut Context<Self>) {
        self.sync_table_record_from_runtime(table_block, cx);

        let Some(mut table) = table_block.read(cx).record.table.clone() else {
            return;
        };
        let started_local_capture = if self.pending_undo_capture.is_none() {
            self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
            true
        } else {
            false
        };
        table.append_row();

        table_block.update(cx, move |block, _cx| {
            block.record.table = Some(table.clone());
        });
        self.rebuild_table_runtimes(cx);
        if let Some(cell) = table_block
            .read(cx)
            .table_runtime
            .as_ref()
            .and_then(|runtime| runtime.rows.last())
            .and_then(|row| row.first())
        {
            self.focus_block(cell.entity_id());
        }
        if self.write_back_table_row_insertion(table_block, cx) {
            self.mark_dirty_written_back(cx);
        } else {
            self.write_back_table_structure_edit(table_block, cx);
        }
        self.request_active_block_scroll_into_view(cx);
        if started_local_capture {
            self.finalize_pending_undo_capture(cx);
        }
        cx.notify();
    }

    pub(super) fn preview_table_axis(
        &mut self,
        table_block_id: EntityId,
        kind: TableAxisKind,
        index: usize,
        hovered: bool,
        cx: &mut Context<Self>,
    ) {
        let marker = TableAxisSelection {
            table_block_id,
            kind,
            index,
        };
        if hovered {
            self.set_table_axis_preview(Some(marker), cx);
        } else if self.table_axis_preview == Some(marker) {
            // Only clear on a leave that still owns the preview. Adjacent
            // handles share one preview slot, and a leave can arrive after
            // the next handle's enter; clearing unconditionally would erase
            // the highlight the pointer just moved onto.
            self.set_table_axis_preview(None, cx);
        }
    }

    pub(super) fn select_table_axis(
        &mut self,
        table_block_id: EntityId,
        kind: TableAxisKind,
        index: usize,
        cx: &mut Context<Self>,
    ) {
        let selection = TableAxisSelection {
            table_block_id,
            kind,
            index,
        };
        self.set_table_axis_preview(Some(selection), cx);
        self.set_table_axis_selection(Some(selection), cx);
    }

    pub(super) fn open_table_axis_menu(
        &mut self,
        table_block_id: EntityId,
        kind: TableAxisKind,
        index: usize,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.select_table_axis(table_block_id, kind, index, cx);
        if let Some(selection) = self.table_axis_selection {
            self.open_table_axis_context_menu(position, selection, cx);
        }
    }

    pub(super) fn set_table_column_alignment(
        &mut self,
        table_block: &Entity<Block>,
        column: usize,
        alignment: TableColumnAlignment,
        cx: &mut Context<Self>,
    ) {
        self.sync_table_record_from_runtime(table_block, cx);
        let Some(mut table) = table_block.read(cx).record.table.clone() else {
            return;
        };
        let started_local_capture = if self.pending_undo_capture.is_none() {
            self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
            true
        } else {
            false
        };
        table.set_column_alignment(column, alignment);
        table_block.update(cx, move |block, _cx| {
            block.record.table = Some(table.clone());
        });
        self.rebuild_table_runtimes(cx);
        let selection = TableAxisSelection {
            table_block_id: table_block.entity_id(),
            kind: TableAxisKind::Column,
            index: column,
        };
        self.set_table_axis_selection(Some(selection), cx);
        self.focus_table_cell_position(table_block, TableCellPosition { row: 0, column }, cx);
        if self.write_back_table_column_alignment(table_block, column, alignment, cx) {
            self.mark_dirty_written_back(cx);
        } else {
            self.write_back_table_structure_edit(table_block, cx);
        }
        self.request_active_block_scroll_into_view(cx);
        if started_local_capture {
            self.finalize_pending_undo_capture(cx);
        }
        cx.notify();
    }

    pub(super) fn move_table_row(
        &mut self,
        table_block: &Entity<Block>,
        visual_row: usize,
        delta: i32,
        cx: &mut Context<Self>,
    ) {
        self.sync_table_record_from_runtime(table_block, cx);
        let Some(mut table) = table_block.read(cx).record.table.clone() else {
            return;
        };
        let next_row = if delta < 0 {
            visual_row.checked_sub(delta.unsigned_abs() as usize)
        } else {
            visual_row.checked_add(delta as usize)
        };
        let Some(next_row) = next_row else {
            return;
        };
        // Visual rows are the header (0) plus every body row, so the last valid
        // index is `rows.len()`.
        if next_row > table.rows.len() {
            return;
        }
        let started_local_capture = if self.pending_undo_capture.is_none() {
            self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
            true
        } else {
            false
        };
        table.swap_visual_rows(visual_row, next_row);
        table_block.update(cx, move |block, _cx| {
            block.record.table = Some(table.clone());
        });
        self.rebuild_table_runtimes(cx);
        let selection = TableAxisSelection {
            table_block_id: table_block.entity_id(),
            kind: TableAxisKind::Row,
            index: next_row,
        };
        self.set_table_axis_selection(Some(selection), cx);
        self.focus_table_cell_position(
            table_block,
            TableCellPosition {
                row: next_row,
                column: 0,
            },
            cx,
        );
        self.write_back_table_structure_edit(table_block, cx);
        self.request_active_block_scroll_into_view(cx);
        if started_local_capture {
            self.finalize_pending_undo_capture(cx);
        }
        cx.notify();
    }

    pub(super) fn move_table_column(
        &mut self,
        table_block: &Entity<Block>,
        column: usize,
        delta: i32,
        cx: &mut Context<Self>,
    ) {
        self.sync_table_record_from_runtime(table_block, cx);
        let Some(mut table) = table_block.read(cx).record.table.clone() else {
            return;
        };
        let next_column = if delta < 0 {
            column.checked_sub(delta.unsigned_abs() as usize)
        } else {
            column.checked_add(delta as usize)
        };
        let Some(next_column) = next_column else {
            return;
        };
        if next_column >= table.column_count() {
            return;
        }
        let started_local_capture = if self.pending_undo_capture.is_none() {
            self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
            true
        } else {
            false
        };
        table.swap_columns(column, next_column);
        table_block.update(cx, move |block, _cx| {
            block.record.table = Some(table.clone());
        });
        self.rebuild_table_runtimes(cx);
        let selection = TableAxisSelection {
            table_block_id: table_block.entity_id(),
            kind: TableAxisKind::Column,
            index: next_column,
        };
        self.set_table_axis_selection(Some(selection), cx);
        self.focus_table_cell_position(
            table_block,
            TableCellPosition {
                row: 0,
                column: next_column,
            },
            cx,
        );
        self.write_back_table_structure_edit(table_block, cx);
        self.request_active_block_scroll_into_view(cx);
        if started_local_capture {
            self.finalize_pending_undo_capture(cx);
        }
        cx.notify();
    }

    pub(super) fn delete_table_row(
        &mut self,
        table_block: &Entity<Block>,
        row_index: usize,
        cx: &mut Context<Self>,
    ) {
        self.sync_table_record_from_runtime(table_block, cx);
        let Some(mut table) = table_block.read(cx).record.table.clone() else {
            return;
        };
        if row_index >= table.rows.len() {
            return;
        }
        let started_local_capture = if self.pending_undo_capture.is_none() {
            self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
            true
        } else {
            false
        };
        table.remove_body_row(row_index);
        let remaining_body_rows = table.rows.len();
        table_block.update(cx, move |block, _cx| {
            block.record.table = Some(table.clone());
        });
        self.rebuild_table_runtimes(cx);
        // Row selections are addressed by visual index, where the first body row
        // is `1` (the header is `0`). With no body rows left, fall back to the
        // header so focus lands on a cell that still exists.
        let focus_visual_row = if remaining_body_rows == 0 {
            0
        } else {
            row_index.min(remaining_body_rows - 1) + 1
        };
        if remaining_body_rows == 0 {
            self.clear_table_axis_selection(cx);
        } else {
            self.set_table_axis_selection(
                Some(TableAxisSelection {
                    table_block_id: table_block.entity_id(),
                    kind: TableAxisKind::Row,
                    index: focus_visual_row,
                }),
                cx,
            );
        }
        self.focus_table_cell_position(
            table_block,
            TableCellPosition {
                row: focus_visual_row,
                column: 0,
            },
            cx,
        );
        if self.write_back_table_row_deletion(table_block, row_index, cx) {
            self.mark_dirty_written_back(cx);
        } else {
            self.write_back_table_structure_edit(table_block, cx);
        }
        self.request_active_block_scroll_into_view(cx);
        if started_local_capture {
            self.finalize_pending_undo_capture(cx);
        }
        cx.notify();
    }

    pub(super) fn delete_table_header_row(
        &mut self,
        table_block: &Entity<Block>,
        cx: &mut Context<Self>,
    ) {
        self.sync_table_record_from_runtime(table_block, cx);
        let Some(mut table) = table_block.read(cx).record.table.clone() else {
            return;
        };
        // The first body row is promoted into the header, so there must be at
        // least one body row to delete the header.
        if table.rows.is_empty() {
            return;
        }
        let started_local_capture = if self.pending_undo_capture.is_none() {
            self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
            true
        } else {
            false
        };
        table.remove_header_row();
        table_block.update(cx, move |block, _cx| {
            block.record.table = Some(table.clone());
        });
        self.rebuild_table_runtimes(cx);
        self.clear_table_axis_selection(cx);
        self.focus_table_cell_position(table_block, TableCellPosition { row: 0, column: 0 }, cx);
        self.write_back_table_structure_edit(table_block, cx);
        self.request_active_block_scroll_into_view(cx);
        if started_local_capture {
            self.finalize_pending_undo_capture(cx);
        }
        cx.notify();
    }

    pub(super) fn delete_table_column(
        &mut self,
        table_block: &Entity<Block>,
        column: usize,
        cx: &mut Context<Self>,
    ) {
        self.sync_table_record_from_runtime(table_block, cx);
        let Some(mut table) = table_block.read(cx).record.table.clone() else {
            return;
        };
        if table.column_count() <= 1 || column >= table.column_count() {
            return;
        }
        let started_local_capture = if self.pending_undo_capture.is_none() {
            self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
            true
        } else {
            false
        };
        table.remove_column(column);
        let focus_column = column.min(table.column_count().saturating_sub(1));
        table_block.update(cx, move |block, _cx| {
            block.record.table = Some(table.clone());
        });
        self.rebuild_table_runtimes(cx);
        let selection = TableAxisSelection {
            table_block_id: table_block.entity_id(),
            kind: TableAxisKind::Column,
            index: focus_column,
        };
        self.set_table_axis_selection(Some(selection), cx);
        self.focus_table_cell_position(
            table_block,
            TableCellPosition {
                row: 0,
                column: focus_column,
            },
            cx,
        );
        self.write_back_table_structure_edit(table_block, cx);
        self.request_active_block_scroll_into_view(cx);
        if started_local_capture {
            self.finalize_pending_undo_capture(cx);
        }
        cx.notify();
    }

    /// Removes the table block entirely, leaving an empty paragraph in its place
    /// so the caret has somewhere to land. Used when deleting the last remaining
    /// row or column, which empties the table.
    pub(super) fn remove_table_block(
        &mut self,
        table_block: &Entity<Block>,
        cx: &mut Context<Self>,
    ) {
        let Some(location) = self.document.find_block_location(table_block.entity_id()) else {
            return;
        };
        let started_local_capture = if self.pending_undo_capture.is_none() {
            self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
            true
        } else {
            false
        };
        // Insert the replacement paragraph after the table first, then remove the
        // table, so the document is never momentarily empty.
        let roots_before = self.document.root_layout(cx);
        let paragraph = Self::new_block(cx, BlockRecord::paragraph(String::new()));
        self.document.insert_blocks_at(
            location.parent.clone(),
            location.index + 1,
            vec![paragraph.clone()],
            cx,
        );
        let table_id = table_block.entity_id();
        self.document.with_structure_mutation(cx, |document, cx| {
            let _ = document.remove_block_by_id_raw(table_id, cx);
        });
        self.rebuild_table_runtimes(cx);
        self.clear_table_axis_selection(cx);
        self.focus_block(paragraph.entity_id());
        // 整根块没了要改的是「一段连续根块」的区间：把表那几行连它们自己的换行
        // 一起收掉，剩下的那一行空行就是原位那个空段落。算不出来才退回整篇重投影。
        if self.write_back_root_region(table_block, &roots_before, cx) {
            self.mark_dirty_written_back(cx);
        } else {
            self.mark_dirty(cx);
        }
        self.request_active_block_scroll_into_view(cx);
        if started_local_capture {
            self.finalize_pending_undo_capture(cx);
        }
        cx.notify();
    }

    pub(super) fn table_axis_marker(selection: TableAxisSelection) -> TableAxisMarker {
        TableAxisMarker {
            kind: selection.kind,
            index: selection.index,
        }
    }

    pub(super) fn clear_table_axis_preview(&mut self, cx: &mut Context<Self>) {
        if self.table_axis_preview.take().is_some() {
            self.sync_table_axis_visuals(cx);
        }
    }

    pub(super) fn clear_table_axis_selection(&mut self, cx: &mut Context<Self>) {
        if self.table_axis_selection.take().is_some() {
            self.sync_table_axis_visuals(cx);
        }
    }

    pub(super) fn set_table_axis_preview(
        &mut self,
        preview: Option<TableAxisSelection>,
        cx: &mut Context<Self>,
    ) {
        if self.table_axis_preview != preview {
            self.table_axis_preview = preview;
            self.sync_table_axis_visuals(cx);
        }
    }

    pub(super) fn set_table_axis_selection(
        &mut self,
        selection: Option<TableAxisSelection>,
        cx: &mut Context<Self>,
    ) {
        if self.table_axis_selection != selection {
            self.table_axis_selection = selection;
            self.sync_table_axis_visuals(cx);
        }
    }

    pub(super) fn table_axis_selection_valid(
        &self,
        selection: TableAxisSelection,
        cx: &App,
    ) -> bool {
        let Some(table_block) = self.table_block_by_id(selection.table_block_id, cx) else {
            return false;
        };
        let Some(runtime) = table_block.read(cx).table_runtime.as_ref() else {
            return false;
        };
        match selection.kind {
            TableAxisKind::Column => selection.index < runtime.header.len(),
            // Visual row index: `0` is the header, `1..=rows.len()` the body.
            TableAxisKind::Row => selection.index <= runtime.rows.len(),
        }
    }

    pub(super) fn normalize_table_axis_state(&mut self, cx: &mut Context<Self>) {
        if let Some(selection) = self.table_axis_selection
            && !self.table_axis_selection_valid(selection, cx)
        {
            self.table_axis_selection = None;
        }
        if let Some(preview) = self.table_axis_preview
            && !self.table_axis_selection_valid(preview, cx)
        {
            self.table_axis_preview = None;
        }
    }

    pub(super) fn sync_table_axis_visuals(&mut self, cx: &mut Context<Self>) {
        self.normalize_table_axis_state(cx);

        let visible_tables = self
            .document
            .flatten_visible_blocks()
            .into_iter()
            .filter(|visible| visible.entity.read(cx).kind() == BlockKind::Table)
            .map(|visible| visible.entity)
            .collect::<Vec<_>>();

        for table_block in &visible_tables {
            let block_id = table_block.entity_id();
            let preview_marker = self
                .table_axis_preview
                .filter(|selection| selection.table_block_id == block_id)
                .map(Self::table_axis_marker);
            let selected_marker = self
                .table_axis_selection
                .filter(|selection| selection.table_block_id == block_id)
                .map(Self::table_axis_marker);

            table_block.update(cx, move |block, cx| {
                block.set_table_axis_visual_state(preview_marker, selected_marker);
                cx.notify();
            });

            let Some(runtime) = table_block.read(cx).table_runtime.clone() else {
                continue;
            };

            let selected = self
                .table_axis_selection
                .filter(|selection| selection.table_block_id == block_id);
            let preview = self
                .table_axis_preview
                .filter(|selection| selection.table_block_id == block_id);

            // `row` is the visual row index: `0` is the header and body rows
            // follow at `1..`, matching how row selections are addressed.
            let mut apply_highlight = |cell: &Entity<Block>, row: usize, column: usize| {
                let highlight = if selected.is_some_and(|selection| match selection.kind {
                    TableAxisKind::Column => selection.index == column,
                    TableAxisKind::Row => selection.index == row,
                }) {
                    TableAxisHighlight::Selected
                } else if preview.is_some_and(|selection| match selection.kind {
                    TableAxisKind::Column => selection.index == column,
                    TableAxisKind::Row => selection.index == row,
                }) {
                    TableAxisHighlight::Preview
                } else {
                    TableAxisHighlight::None
                };

                cell.update(cx, move |block, cx| {
                    block.set_table_axis_highlight(highlight);
                    cx.notify();
                });
            };

            for (column, cell) in runtime.header.iter().enumerate() {
                apply_highlight(cell, 0, column);
            }
            for (body_row_index, row) in runtime.rows.iter().enumerate() {
                for (column, cell) in row.iter().enumerate() {
                    apply_highlight(cell, body_row_index + 1, column);
                }
            }
        }
    }
}

/// 一行表格里「真正的列分隔符」的字节位置。反斜杠转义的 `|` 不算列分隔符。
fn unescaped_pipe_offsets(line: &str) -> Vec<usize> {
    let mut offsets = Vec::new();
    let mut escaped = false;
    for (offset, byte) in line.bytes().enumerate() {
        if byte == b'\\' {
            escaped = !escaped;
        } else if byte == b'|' && !escaped {
            offsets.push(offset);
            escaped = false;
        } else {
            escaped = false;
        }
    }
    offsets
}

/// 分隔行里一格的新写法：换成 `alignment`，但这一格的宽度尽量不动。
///
/// GFM 在这一格里只认 `-` 和两端的 `:`，所以照原来的 `-` 个数重排；要多加的冒号
/// 从这一格自己的填充里腾（`-----:` 居中变 `:----:`），腾不出位置才让它变长
/// （`---` 居中变 `:-:`）。不是干净的对齐写法（夹了别的字符、一个 `-` 都没有）
/// 就返回 `None`，调用方交回整块写。
fn realigned_delimiter_cell(cell: &str, alignment: TableColumnAlignment) -> Option<String> {
    let core = cell.trim();
    let old_dashes = core.matches('-').count();
    if old_dashes == 0 || !core.chars().all(|character| character == '-' || character == ':') {
        return None;
    }
    let (left, right) = match alignment {
        TableColumnAlignment::Default => (false, false),
        TableColumnAlignment::Left => (true, false),
        TableColumnAlignment::Center => (true, true),
        TableColumnAlignment::Right => (false, true),
    };
    let colons = usize::from(left) + usize::from(right);
    let target_width = cell.chars().count();
    let dashes = old_dashes.min(target_width.saturating_sub(colons).max(1));

    let mut cell = String::with_capacity(target_width + 2);
    if left {
        cell.push(':');
    }
    for _ in 0..dashes {
        cell.push('-');
    }
    if right {
        cell.push(':');
    }
    for _ in cell.chars().count()..target_width {
        cell.push(' ');
    }
    Some(cell)
}

/// 一行表格里第 `column` 个格子的**内容**字节区间（两侧的填充空格不算在内）。
///
/// 反斜杠转义的 `|` 不算列分隔符；首尾没有外层管道符的写法也能量出来。空格全占
/// 的空格，内容区间取零宽、插在第一个空格后面，写进去就是「往这格里加字」。
fn cell_content_range_in_line(
    line: &str,
    line_start: usize,
    column: usize,
) -> Option<Range<usize>> {
    let bytes = line.as_bytes();
    let mut slots: Vec<Range<usize>> = Vec::new();
    let mut start = 0usize;
    let mut escaped = false;
    for (offset, byte) in bytes.iter().enumerate() {
        if *byte == b'\\' {
            escaped = !escaped;
        } else if *byte == b'|' && !escaped {
            slots.push(start..offset);
            start = offset + 1;
            escaped = false;
        } else {
            escaped = false;
        }
    }
    slots.push(start..line.len());
    // 外层管道符两侧什么都没有：那一格不算数据。
    if slots.first().is_some_and(|slot| slot.is_empty()) {
        slots.remove(0);
    }
    if slots.last().is_some_and(|slot| slot.is_empty()) {
        slots.pop();
    }

    let slot = slots.get(column)?.clone();
    let mut left = slot.start;
    while left < slot.end && matches!(bytes[left], b' ' | b'\t') {
        left += 1;
    }
    let mut right = slot.end;
    while right > left && matches!(bytes[right - 1], b' ' | b'\t') {
        right -= 1;
    }
    if left == right {
        let at = slot.start + usize::from(slot.end - slot.start > 1);
        return Some(line_start + at..line_start + at);
    }
    Some(line_start + left..line_start + right)
}
