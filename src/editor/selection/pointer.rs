//! 正文里的指针动作：按下、拖动、抬手，以及「屏幕上的点 → 选区端点」的换算。

use super::*;

// 拖动定位时读了几次挂载行几何（测试用）。二分定位应当只碰对数级几行，
// 逐行线性扫会随挂载行数线性增长。
#[cfg(test)]
thread_local! {
    pub(crate) static DRAG_ROW_PROBES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Editor {
    pub(super) fn begin_cross_block_drag_at_point(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let had_selection = self.cross_block_selection.take().is_some();
        let had_table_selection = self.table_text_selection.take().is_some();
        let changed_visuals = self.clear_cross_block_selection_visuals(cx);
        let changed = had_selection || had_table_selection || changed_visuals;
        // 按下那一下还不知道拖动方向：端点先记块尾，随后的每次 move 由
        // whole_block_anchor 按方向改写。
        self.cross_block_drag = self
            .cross_block_endpoint_for_point(position, None, cx)
            .map(|anchor| CrossBlockDrag {
                anchor,
                anchor_table_cell: self.table_text_anchor_at_point(position, cx),
                anchor_is_whole_block: self.block_at_endpoint_selects_whole(anchor, cx),
            });
        if changed {
            cx.notify();
        }
    }

    /// 落在正文最后一块下方的按下：把光标送到文末。
    ///
    /// 正文块自己的命中测试只看块内，块底以下那片空白（最后一行往下大约
    /// 半个屏幕）没有块接得住，点击不能落空。未聚焦的空段落不绘制文本，需要借用
    /// 已挂载行的边界；其他无文本几何的末块仍由各自的交互处理。
    fn place_caret_below_document_end(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(last) = self
            .document
            .visible_blocks()
            .last()
            .map(|visible| visible.entity.clone())
        else {
            return;
        };
        if !self.entity_is_mounted(last.entity_id()) {
            return;
        }
        let bounds = last.read(cx).last_bounds.or_else(|| {
            if !last
                .read(cx)
                .collapses_to_blank_gap(false, self.view_mode == ViewMode::Source)
            {
                return None;
            }
            let plan = self.rendered_row_plan.as_ref()?;
            let row = plan.rows.len().checked_sub(1)?;
            if plan.rows.get(row)?.first_id != last.entity_id() {
                return None;
            }
            let run = self.prev_mounted_run.filter(|run| {
                self.mounted_run_is_addressable(*run)
                    && run.row_start <= row
                    && row < run.row_end
            })?;
            let mut bounds = self
                .scroll_handle
                .bounds_for_item(run.child_base + row - run.row_start)?;
            bounds.origin += self.scroll_handle.offset();
            Some(bounds)
        });
        let Some(bounds) = bounds else {
            return;
        };
        if position.y <= bounds.bottom() {
            return;
        }
        let end = last.read(cx).display_text().len();
        Self::reset_block_cursor(&last, end, cx);
        self.focus_block(last.entity_id());
        cx.notify();
    }

    /// shift+按下：从既有锚点扩选，而不是在按下处重新落锚。
    ///
    /// 拖选靠 mouse_move 拉开选区，但「点一下、按住 shift 再点另一段」没有中间的
    /// move：原先的路径在按下处重落一次锚点，抬手时选区仍是空的，跨段 shift 扩选
    /// 于是只剩同段内那一截（那段由块自己的 shift+点处理）。返回 true 表示这一次
    /// 按下已经变成扩选，调用方不要再去落光标。
    fn extend_cross_block_selection_at_point(
        &mut self,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) -> bool {
        // 先取锚点：它决定指针落在没有文字布局的块里时给哪个端点。
        let Some(anchor) = self.cross_block_shift_anchor(cx) else {
            return false;
        };
        let Some(focus) = self.cross_block_endpoint_for_point(position, Some(anchor), cx) else {
            return false;
        };
        // 落在表格格子上的按下归表格自己的按格选字，不在这里扩。
        if self.table_text_anchor_at_point(position, cx).is_some() {
            return false;
        }
        // 跨块扩选回到锚点块仍归同一条逻辑选区，不能交给失焦块重新落锚。
        if anchor.entity_id == focus.entity_id && self.cross_block_selection.is_none() {
            return false;
        }
        let anchor_is_whole_block = self.block_at_endpoint_selects_whole(anchor, cx);
        let selection = CrossBlockSelection {
            anchor: self.whole_block_anchor(anchor, focus, anchor_is_whole_block, cx),
            focus,
        };
        if self.cross_block_selection_is_empty(selection) {
            return false;
        }
        // 记住这个锚点：接着按住拖动时扩的是同一条选区，不再重新落锚。
        self.cross_block_drag = Some(CrossBlockDrag {
            anchor,
            anchor_table_cell: None,
            anchor_is_whole_block,
        });
        self.cross_block_selection = Some(selection);
        self.sync_cross_block_selection_visuals(cx);
        cx.notify();
        true
    }

    /// shift 扩选的锚点：已有跨块选区就用它的起点；否则用当前编辑目标的光标
    /// （或它在块内的选区锚）。两者都没有（表格选区在手上）就返回 None。
    fn cross_block_shift_anchor(&self, cx: &App) -> Option<CrossBlockSelectionEndpoint> {
        if let Some(selection) = self.cross_block_selection {
            return Some(selection.anchor);
        }
        if self.table_text_selection.is_some() {
            return None;
        }
        let target = self.current_edit_target_from_state(cx)?;
        let (anchor, _) = target.read(cx).clean_selection_anchor_focus();
        Some(CrossBlockSelectionEndpoint {
            entity_id: target.entity_id(),
            offset: anchor,
        })
    }

    /// 端点所在的块能不能按指针位置给出块内偏移。只画图片的块（未聚焦的公式、
    /// 带行内公式的段落、只装公式的格子）没有文字布局，指针落在它上面只能整块进选区。
    fn block_at_endpoint_selects_whole(
        &self,
        endpoint: CrossBlockSelectionEndpoint,
        cx: &App,
    ) -> bool {
        self.document
            .block_entity_by_id(endpoint.entity_id)
            .is_some_and(|block| !block.read(cx).has_text_layout())
    }

    /// 锚点落在没有文字布局的块上时，端点得整块算，不能是块内偏移。
    ///
    /// 锚点在这类块上时命中测试给不出块内偏移（按下那一下给的是块尾，方向还没定），
    /// 这里按焦点在锚点上方还是下方把它定成块首或块尾：往下拖它进起点侧（块首，
    /// 整块都进来），往上拖它进终点侧（块尾，同样整块都进来）。
    fn whole_block_anchor(
        &self,
        anchor: CrossBlockSelectionEndpoint,
        focus: CrossBlockSelectionEndpoint,
        is_whole_block: bool,
        cx: &App,
    ) -> CrossBlockSelectionEndpoint {
        if !is_whole_block {
            return anchor;
        }
        let Some(block) = self.document.block_entity_by_id(anchor.entity_id) else {
            return anchor;
        };
        let (Some(anchor_index), Some(focus_index)) = (
            self.document.visible_index_for_entity_id(anchor.entity_id),
            self.document.visible_index_for_entity_id(focus.entity_id),
        ) else {
            return anchor;
        };
        let offset = if focus_index < anchor_index {
            block.read(cx).clean_visible_len()
        } else {
            0
        };
        CrossBlockSelectionEndpoint {
            entity_id: anchor.entity_id,
            offset,
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

        if self.scroll_handle.bounds().contains(&event.position) {
            self.workspace.search_navigation_selection = None;
        }
        // 源码模式的块也要武装跨块拖拽：回车新建的是逐行块，跨块拖选是刚需。
        // 同一根块里的拖动由 mouse_move 的同块早退交还给块内选区，这里武装
        // 只记锚点，不影响块内行为。按住 shift 的按下不落光标，直接换成扩选。
        self.rendered_select_all_cycle = None;
        let extended =
            event.modifiers.shift && self.extend_cross_block_selection_at_point(event.position, cx);
        if !extended {
            self.place_caret_below_document_end(event.position, cx);
            self.begin_cross_block_drag_at_point(event.position, cx);
        }
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
        let Some(focus) =
            self.cross_block_endpoint_for_point(event.position, Some(drag.anchor), cx)
        else {
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

        let anchor = self.whole_block_anchor(drag.anchor, focus, drag.anchor_is_whole_block, cx);
        let selection = CrossBlockSelection { anchor, focus };
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
    ///
    /// 几何只问**这一帧挂载着的行**：行的屏幕边界是这一帧渲染写进滚动句柄的，块自己的
    /// `last_bounds` 只在它被画过之后才有值、划出挂载窗口也不清理（长文档往下翻过几屏，
    /// 上面那些块留着旧的屏幕边界，照它算端点会把文档开头的一块当成落点）。挂载区之外
    /// 的块，指针够不到；指针落在挂载区上方或下方时夹到边上的那一块。
    ///
    /// 行里那些没有文本布局的块（未聚焦的公式、带行内公式的段落、表格格子里的公式）量不出
    /// 块内偏移，取行边界当它的位置、整块算：跳过它们会把这一段空间算成「上一块的块尾」，
    /// 从正文往它们身上拖时端点一直停在上面那一段，与按下时的端点同块同偏移，
    /// `on_editor_mouse_move` 的同块早退把选区压着不建。
    fn cross_block_endpoint_for_point(
        &self,
        position: Point<Pixels>,
        anchor: Option<CrossBlockSelectionEndpoint>,
        cx: &App,
    ) -> Option<CrossBlockSelectionEndpoint> {
        let run = self
            .prev_mounted_run
            .filter(|run| self.mounted_run_is_addressable(*run))?;
        let plan = self.rendered_row_plan.as_ref()?;
        let visible_blocks = self.document.visible_blocks();
        if plan.visible_len != visible_blocks.len() {
            return None;
        }
        let scroll = self.scroll_handle.offset();

        // 没有焦点岛时挂载区是一段连续行，行按 y 单调排列：先二分找到包含落点
        // 的那一行，只碰它和它上面那一行。拖动是高频事件，逐行读全部成员块在
        // 长文档上一次 move 就是几十次实体读加分配。
        if run.focus_island.is_none()
            && let Some(Some(endpoint)) =
                self.drag_endpoint_by_row_bisection(run, position, anchor, cx)
        {
            return Some(endpoint);
        }

        // 有焦点岛或挂载区有空洞时退回线性扫描（行序与 y 序不再一致）。
        let mut previous: Option<CrossBlockSelectionEndpoint> = None;
        for (row, child) in run.rows() {
            #[cfg(test)]
            DRAG_ROW_PROBES.with(|probes| probes.set(probes.get() + 1));
            let Some(item) = self.scroll_handle.bounds_for_item(child) else {
                continue;
            };
            let mut row_bounds = item;
            row_bounds.origin += scroll;
            let Some(plan_row) = plan.rows.get(row) else {
                continue;
            };
            let Some(first) = plan_row.entities().next() else {
                continue;
            };
            if position.y < row_bounds.top() {
                // 落点在这一行上方：归上一行的块尾；没有上一行就归这一行的块首。
                return Some(previous.unwrap_or(CrossBlockSelectionEndpoint {
                    entity_id: first.entity_id(),
                    offset: 0,
                }));
            }
            if position.y <= row_bounds.bottom() {
                return self.pick_drag_endpoint_in_row(plan_row, row_bounds, position, anchor, cx);
            }
            if let Some(endpoint) = self.row_last_endpoint(plan_row, cx) {
                previous = Some(endpoint);
            }
        }
        // 落点在整个挂载区下方：归最后一块的块尾。
        previous
    }

    /// 二分定位落点所在行并换算端点。`None` = 这一段布局有空洞，调用方退回线性扫描。
    fn drag_endpoint_by_row_bisection(
        &self,
        run: crate::editor::MountedRun,
        position: Point<Pixels>,
        anchor: Option<CrossBlockSelectionEndpoint>,
        cx: &App,
    ) -> Option<Option<CrossBlockSelectionEndpoint>> {
        let plan = self.rendered_row_plan.as_ref()?;
        let scroll = self.scroll_handle.offset();
        let row_bounds = |row: usize| -> Option<Bounds<Pixels>> {
            if row < run.row_start || row >= run.row_end {
                return None;
            }
            let child = run.child_base + row - run.row_start;
            #[cfg(test)]
            DRAG_ROW_PROBES.with(|probes| probes.set(probes.get() + 1));
            let mut bounds = self.scroll_handle.bounds_for_item(child)?;
            bounds.origin += scroll;
            Some(bounds)
        };
        let (mut low, mut high) = (run.row_start, run.row_end);
        while low < high {
            let mid = (low + high) / 2;
            match row_bounds(mid) {
                Some(bounds) if bounds.bottom() < position.y => low = mid + 1,
                Some(_) => high = mid,
                None => return None,
            }
        }
        if low >= run.row_end {
            // 落点在整段挂载行下方：归最后一行最后一块的块尾。
            let plan_row = plan.rows.get(run.row_end.checked_sub(1)?)?;
            return Some(self.row_last_endpoint(plan_row, cx));
        }
        let row_bounds = row_bounds(low)?;
        let plan_row = plan.rows.get(low)?;
        if position.y < row_bounds.top() {
            if low == run.row_start {
                let first = plan_row.entities().next()?;
                return Some(Some(CrossBlockSelectionEndpoint {
                    entity_id: first.entity_id(),
                    offset: 0,
                }));
            }
            let previous_row = plan.rows.get(low - 1)?;
            return Some(self.row_last_endpoint(previous_row, cx));
        }
        Some(self.pick_drag_endpoint_in_row(plan_row, row_bounds, position, anchor, cx))
    }

    /// 一行的最后一块的块尾端点。
    fn row_last_endpoint(
        &self,
        plan_row: &crate::editor::render::RenderedRowPlanRow,
        cx: &App,
    ) -> Option<CrossBlockSelectionEndpoint> {
        let last = plan_row.entities().last()?;
        Some(CrossBlockSelectionEndpoint {
            entity_id: last.entity_id(),
            offset: last.read(cx).clean_visible_len(),
        })
    }

    /// 一行里挑块：单块的直接就是它；callout / 脚注组里几块共用一行，按各块
    /// 自己的边界挑（那些边界是这一帧画的），都够不着就取最后一块。
    fn pick_drag_endpoint_in_row(
        &self,
        plan_row: &crate::editor::render::RenderedRowPlanRow,
        row_bounds: Bounds<Pixels>,
        position: Point<Pixels>,
        anchor: Option<CrossBlockSelectionEndpoint>,
        cx: &App,
    ) -> Option<CrossBlockSelectionEndpoint> {
        let mut chosen: Option<Entity<Block>> = None;
        for entity in plan_row.entities() {
            let block = entity.read(cx);
            let bounds = block.last_bounds.or_else(|| {
                block.table_runtime.as_ref().and_then(|runtime| {
                    runtime
                        .header
                        .iter()
                        .chain(runtime.rows.iter().flatten())
                        .filter_map(|cell| cell.read(cx).last_bounds)
                        .reduce(|bounds, cell| bounds.union(&cell))
                })
            });
            let bounds = match &plan_row.body {
                crate::editor::render::RenderedRowBody::Ordinary { .. } => {
                    Some(bounds.unwrap_or(row_bounds))
                }
                crate::editor::render::RenderedRowBody::Group { .. } => bounds,
            };
            let Some(bounds) = bounds else { continue };
            chosen = Some(entity.clone());
            if position.y <= bounds.bottom() {
                break;
            }
        }
        let entity = chosen?;
        let block = entity.read(cx);
        // 只画图片的块量不出块内偏移（没有文字布局），整块算：它在选区起点侧
        // 就取块首、在终点侧取块尾。文本块的块内偏移由它自己裁到上下边界。
        let offset = if block.has_text_layout() {
            block.current_to_clean_offset(block.index_for_mouse_position(position))
        } else {
            self.whole_block_offset(&entity, anchor, cx)
        };
        Some(CrossBlockSelectionEndpoint {
            entity_id: entity.entity_id(),
            offset,
        })
    }

    /// 没有文字布局的块（未聚焦的公式、带行内公式的段落、只装公式的格子）量不出块内
    /// 偏移，端点只能取块首或块尾。
    ///
    /// 取哪一头由拖动方向定：焦点在锚点下方（往下拖）时这一块是选区的起点侧，取块首；
    /// 往上拖时它是终点侧，取块尾——终点侧取块首等于一个字也不选，选区中间横着的
    /// 那一根公式就永远选不中。按下那一下还不知道方向，随后的每次 move 都会按方向
    /// 改写锚点（见 [`Self::whole_block_anchor`]）。
    fn whole_block_offset(
        &self,
        entity: &Entity<Block>,
        anchor: Option<CrossBlockSelectionEndpoint>,
        cx: &App,
    ) -> usize {
        let end = entity.read(cx).clean_visible_len();
        let Some(anchor) = anchor else {
            return end;
        };
        let Some(anchor_index) = self.document.visible_index_for_entity_id(anchor.entity_id) else {
            return end;
        };
        match self.document.visible_index_for_entity_id(entity.entity_id()) {
            Some(index) if anchor_index > index => 0,
            _ => end,
        }
    }
}
