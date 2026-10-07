//! 正文里的指针动作：按下、拖动、抬手，以及「屏幕上的点 → 选区端点」的换算。

use super::*;

impl Editor {
    pub(super) fn begin_cross_block_drag_at_point(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let had_selection = self.cross_block_selection.take().is_some();
        let had_table_selection = self.table_text_selection.take().is_some();
        let changed_visuals = self.clear_cross_block_selection_visuals(cx);
        let changed = had_selection || had_table_selection || changed_visuals;
        self.cross_block_drag = self
            .cross_block_endpoint_for_point(position, cx)
            .map(|anchor| CrossBlockDrag {
                anchor,
                anchor_table_cell: self.table_text_anchor_at_point(position, cx),
            });
        if changed {
            cx.notify();
        }
    }

    pub(crate) fn on_editor_capture_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // [[ 补全浮层内的按下不动它：行确认靠 bubble 阶段的同一次按下；
        // 落在浮层外的点击立即关闭。
        if self.wikilink_completion_is_open() {
            let inside_panel = self
                .wikilink_completion
                .as_ref()
                .and_then(|state| state.panel_bounds)
                .is_some_and(|bounds| bounds.contains(&event.position));
            if !inside_panel {
                self.close_wikilink_completion(cx);
            }
        }

        // `\\` 公式补全同一条口径。
        if self.latex_completion_is_open() {
            let inside_panel = self
                .latex_completion
                .as_ref()
                .and_then(|state| state.panel_bounds)
                .is_some_and(|bounds| bounds.contains(&event.position));
            if !inside_panel {
                self.close_latex_completion(cx);
            }
        }
        // 公式编辑器弹窗是全屏遮罩（occlude），正文收不到按下，无需豁免。

        // 选中工具栏上的按下不当成正文落点：不然这一次按下先把选区收成光标，
        // 工具栏自己就先消失了（与 [[ 补全浮层同一条口径）。
        if self.selection_toolbar_contains_point(event.position) {
            cx.propagate();
            return;
        }

        if event.button != MouseButton::Left {
            cx.propagate();
            return;
        }

        // 源码模式的块也要武装跨块拖拽：回车新建的是逐行块，跨块拖选是刚需。
        // 同一根块里的拖动由 mouse_move 的同块早退交还给块内选区，这里武装
        // 只记锚点，不影响块内行为。
        self.rendered_select_all_cycle = None;
        self.begin_cross_block_drag_at_point(event.position, cx);
        cx.propagate();
    }

    pub(crate) fn on_editor_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !event.dragging() {
            return;
        }
        let Some(drag) = self.cross_block_drag else {
            return;
        };
        let Some(focus) = self.cross_block_endpoint_for_point(event.position, cx) else {
            return;
        };

        // 在表格格子上按下的拖动：指针离开按下那一格之后由编辑器接手，把两格之间的
        // 文字按格切段选上（同行的格连着，行与行之间换行）。没离开那一格时选字还是
        // 格子自己的块内选区，编辑器一个字都不碰。
        if let Some((table_block_id, anchor)) = drag.anchor_table_cell {
            let focus_position = self
                .table_text_position_at_point(event.position, cx)
                .filter(|(table, _)| *table == table_block_id);
            match focus_position {
                Some((_, focus_position)) => {
                    if focus_position.cell != anchor.cell
                        || self.table_text_selection.is_some()
                    {
                        self.select_table_text_range(table_block_id, anchor, focus_position, cx);
                    }
                    return;
                }
                None => {
                    if focus.entity_id == table_block_id {
                        // 还在同一张表里（行间留白、表格内边距）：保持现在的选区不动。
                        return;
                    }
                    self.clear_table_text_selection(cx);
                }
            }
        }

        // 同一根块里的拖动交给块自己的块内选区，编辑器不掺和。
        if self.cross_block_selection.is_none() && drag.anchor.entity_id == focus.entity_id {
            return;
        }

        let selection = CrossBlockSelection {
            anchor: drag.anchor,
            focus,
        };
        if self.cross_block_selection_is_empty(selection) {
            self.cross_block_selection = None;
        } else {
            self.cross_block_selection = Some(selection);
        }
        self.sync_cross_block_selection_visuals(cx);
        cx.notify();
    }

    pub(crate) fn on_editor_mouse_up(
        &mut self,
        _event: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cross_block_drag = None;
        self.end_block_pointer_selection_sessions(cx);
    }

    /// 屏幕上这一点对应的选区端点。
    ///
    /// 存的是块内**干净**偏移（可见文本的坐标，不含为了编辑显形出来的 `**` 这类记号）：
    /// 拖动过程中起点块会因聚焦而显形、别的块又收回去，显示长度随时在变，端点要是记的
    /// 是显示偏移，选区就跟着漂——画出来的高亮比选区的字节短一截，按删除留在文件里的
    /// 又是另一段字节。命中测试给出的显示偏移在这里换算一次，之后就都按干净坐标算。
    fn cross_block_endpoint_for_point(
        &self,
        position: Point<Pixels>,
        cx: &App,
    ) -> Option<CrossBlockSelectionEndpoint> {
        let mut previous: Option<(Entity<Block>, Bounds<Pixels>)> = None;
        // 没有文本布局的块（表格、分隔线、未聚焦的公式与图表）在屏幕上照样占位置，
        // 只是量不出块内偏移。跳过它们会把这一段空间算成「上一块的块尾」，于是从正文
        // 往表格里拖时端点一直停在上面那一段，与按下时的端点同块同偏移，
        // `on_editor_mouse_move` 的同块早退把选区压着不建——表格怎么拖都进不了选区。
        // 落点落在它们这一段里就归它们自己。
        let mut layout_less: Option<Entity<Block>> = None;
        for visible in self.document.visible_blocks() {
            let entity = visible.entity.clone();
            let bounds = entity.read(cx).last_bounds;
            let Some(bounds) = bounds else {
                layout_less.get_or_insert(entity);
                continue;
            };

            if position.y < bounds.top() {
                // 上一块底与这一块顶之间的空间属于中间那些块：归最上面那一块。
                if let Some(layout_less) = layout_less {
                    return Some(CrossBlockSelectionEndpoint {
                        entity_id: layout_less.entity_id(),
                        offset: 0,
                    });
                }
                // 落点在这一块上方：归上一块的块尾；没有上一块就归它的块首。
                if let Some((previous, _)) = previous {
                    let offset = previous.read(cx).clean_visible_len();
                    return Some(CrossBlockSelectionEndpoint {
                        entity_id: previous.entity_id(),
                        offset,
                    });
                }
                return Some(CrossBlockSelectionEndpoint {
                    entity_id: entity.entity_id(),
                    offset: 0,
                });
            }

            if position.y <= bounds.bottom() {
                let block = entity.read(cx);
                let offset = block.current_to_clean_offset(block.index_for_mouse_position(position));
                return Some(CrossBlockSelectionEndpoint {
                    entity_id: entity.entity_id(),
                    offset,
                });
            }

            previous = Some((entity, bounds));
            layout_less = None;
        }

        if let Some(layout_less) = layout_less {
            return Some(CrossBlockSelectionEndpoint {
                entity_id: layout_less.entity_id(),
                offset: 0,
            });
        }
        previous.map(|(entity, _)| CrossBlockSelectionEndpoint {
            entity_id: entity.entity_id(),
            offset: entity.read(cx).clean_visible_len(),
        })
    }
}
