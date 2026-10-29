use super::*;
use std::time::Duration;

/// 一次「让光标可见」调整的结果。
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct CaretScroll {
    /// 目标块已经画过，拿到了可用几何。
    has_bounds: bool,
    /// 这一帧真的移动了视口。稳定帧（未移动）才允许清掉滚动请求。
    moved: bool,
}

impl Editor {

    /// 状态栏整篇字数
    pub(crate) fn on_titlebar_close(
        &mut self,
        event: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.standard_click() {
            self.request_close_current_window(window, cx);
        }
    }

    pub(crate) fn install_close_guard(&mut self, cx: &mut Context<Self>, window: &mut Window) {
        if self.close_guard_installed {
            return;
        }

        self.force_install_close_guard(cx, window);
    }

    pub(crate) fn force_install_close_guard(
        &mut self,
        cx: &mut Context<Self>,
        window: &mut Window,
    ) {
        let editor = cx.entity().downgrade();
        window.on_window_should_close(cx, move |window, cx| {
            editor
                .update(cx, |this, cx| this.on_window_should_close(window, cx))
                .unwrap_or(true)
        });
        self.close_guard_installed = true;
    }

    pub(crate) fn apply_pending_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending_focus.is_none()
            || self.modal_is_open()
            || gpui_kit::base::active_focus_trap(window, cx).is_some()
        {
            return;
        }
        if let Some(entity_id) = self.pending_focus.take()
            && let Some(block) = self.focusable_entity_by_id(entity_id)
        {
            block.read(cx).focus_handle.clone().focus(window, cx);
        }
    }

    pub(crate) fn ensure_focused_caret_visible(
        &mut self,
        window: &Window,
        cx: &App,
    ) -> CaretScroll {
        // 搜索跳转会把焦点交还查询框：滚动目标改用 active_entity_id，
        // 不让「滚到命中」依赖正文块持有焦点，也避免挂着的滚动请求在
        // 没有焦点块时反复排后续帧。
        // 跳转滚动进行中只认 active 锚点：命中落在表格单元格里时焦点会
        // 回落到文档首块（cell 不能持有窗口焦点），信焦点块就会滚向文档
        // 开头而不是命中。
        let anchor_id = self.active_entity_id;
        // 锚点实体 → 滚动实体：cell 不是可滚动实体（不在可见块列表、无独立
        // 布局边界），生命周期/可见列表索引都用宿主表格块。悬空 id（表格重
        // 建替换过 cell）回退 focusable 注册表，再不行按文档树为准。
        let anchor_block = anchor_id.and_then(|id| {
            self.document
                .block_entity_by_id(id)
                .or_else(|| self.focusable_entity_by_id(id))
        });
        let is_cell_anchor = anchor_block
            .as_ref()
            .is_some_and(|block| block.read_with(cx, |b, _cx| b.table_cell_position().is_some()));
        let scroll_block = anchor_block.clone().map(|block| {
            if is_cell_anchor
                && let Some(binding) = self.table_cell_binding(block.entity_id())
            {
                binding.table_block.clone()
            } else {
                block
            }
        });
        let placement = self.pending_scroll_placement;
        let focused_block = if placement != ScrollPlacement::IntoView {
            scroll_block
        } else {
            self.focused_edit_target(window, cx).or(scroll_block)
        };
        let Some(focused_block) = focused_block else {
            return CaretScroll::default();
        };
        // 居中坐标：cell 锚点优先用命中单元格自己的 bounds——长表格整体
        // 中心离命中行可能差几屏，滚「表格中心」等于没跳到那一行（用户报
        // 修）。表格未绘制时 cell 无 bounds，退回宿主表格的（先滚到表格，
        // 表格进入绘制窗口后下一帧按 cell 精确居中）。
        let active_bounds_from_anchor = if is_cell_anchor
            && let Some(cell) = anchor_block.as_ref()
        {
            self.scroll_target_bounds(cell, placement.is_aligned(), cx)
        } else {
            None
        };
        let Some(active_bounds) = active_bounds_from_anchor
            .or_else(|| self.scroll_target_bounds(&focused_block, placement.is_aligned(), cx))
        else {
            // 目标块尚未绘制：渲染窗口只画视口附近的块，窗外的块既没有
            // last_bounds 也没有文本布局，精确居中无从算起——但不滚就永远
            // 不会画，死锁。
            // 估算/爬行只在用户跳转时进行：初始加载的 pending 是 IntoView，
            // 视口在哪都该原地不动，否则用户手动滚走后活动块出窗、无边界，
            // 会被估算一路拉回（回归：reading_to_the_bottom 测试实测视口被
            // 拽回顶部）。
            if placement == ScrollPlacement::IntoView {
                return CaretScroll::default();
            }
            //
            // 危险在于估算和绘制互相干扰：比例跳一步把目标拉进窗口，目标
            // 一有边界就触发精确居中，而居中把视口滚向目标后目标可能又滑出
            // 窗口、边界被丢弃，估算再跳…… 来回震荡耗尽 settle 帧数，向上
            // 跳回开头会停在半路。所以估算只在离目标还远时
            // 大步跳；一旦接近（约一屏内）改用固定半屏步长单调爬行，保证
            // 每一帧都净逼近，绝不过冲。
            let viewport_height = f32::from(self.scroll_handle.bounds().size.height);
            let content_height =
                f32::from(self.scroll_handle.max_offset().y) + viewport_height;
            // focused_block 已是滚动实体（cell 已升级为宿主表格块），
            // 直接按它在可见列表中的位置估算。
            let index = self
                .document
                .visible_index_for_entity_id(focused_block.entity_id());

            if viewport_height > 0.0
                && let Some(index) = index
            {
                // 行计划的前缀按实测行高累加（未量行用下界）：拿它当跳转落点
                // 比 `index/total` 的平均比例精确一个量级——行高不均匀的文档
                //（首屏大表格/代码块）里平均比例能偏出几屏，先跳错再靠爬行补。
                let (estimate_top, estimate_bottom) = self
                    .estimated_row_span_for_block(focused_block.entity_id())
                    .unwrap_or_else(|| {
                        let total = self.document.visible_blocks().len().max(1);
                        let center = content_height * (index as f32 + 0.5) / total as f32;
                        (center, center)
                    });
                let mut offset = self.scroll_handle.offset();
                let content_top = -f32::from(offset.y);
                let target_y = match placement {
                    ScrollPlacement::Top => -estimate_top,
                    ScrollPlacement::Bottom => viewport_height - estimate_bottom,
                    // 跳转只求可见：估算落点在视口上方就贴顶，否则贴底。
                    ScrollPlacement::Reveal if estimate_top < content_top => -estimate_top,
                    ScrollPlacement::Reveal => viewport_height - estimate_bottom,
                    _ => viewport_height * 0.5 - (estimate_top + estimate_bottom) * 0.5,
                };
                let max_offset_y = f32::from(self.scroll_handle.max_offset().y).max(0.0);
                let target_y = target_y.min(0.0).max(-max_offset_y);
                let current = f32::from(offset.y);
                let mut moved = false;
                if (current - target_y).abs() <= viewport_height * 1.5 {
                    // 接近：每帧半屏单调爬向目标，永不出冲。
                    let step = viewport_height * 0.5;
                    let next = if target_y < current {
                        (current - step).max(target_y)
                    } else {
                        (current + step).min(target_y)
                    };
                    if (next - current).abs() > 1.0 {
                        offset.y = px(next);
                        self.scroll_handle.set_offset(offset);
                        moved = true;
                    }
                } else {
                    let clamped = target_y;
                    if (current - clamped).abs() > 1.0 {
                        offset.y = px(clamped);
                        self.scroll_handle.set_offset(offset);
                        moved = true;
                    }
                }
                return CaretScroll { has_bounds: false, moved };
            }
            return CaretScroll::default();
        };


        let viewport = self.scroll_handle.bounds();
        if self.typewriter_mode
            && self.view_mode == crate::editor::ViewMode::Rendered
            && !self.code_tab_active()
            && self.cross_block_selection.is_none()
        {
            let mut offset = self.scroll_handle.offset();
            let viewport_center = f32::from(viewport.top()) + f32::from(viewport.size.height) * 0.5;
            let caret_center =
                f32::from(active_bounds.top()) + f32::from(active_bounds.size.height) * 0.5;
            let target = typewriter_target_scroll_offset(
                f32::from(offset.y),
                viewport_center,
                caret_center,
                f32::from(self.scroll_handle.max_offset().y),
            );
            let moved = (target - f32::from(offset.y)).abs() > 0.5;
            if moved {
                offset.y = px(target);
                self.caret_scroll_applications
                    .set(self.caret_scroll_applications.get() + 1);
                self.scroll_handle.set_offset(offset);
            }
            return CaretScroll { has_bounds: true, moved };
        }
        // 跳转落点：按口径把目标贴到视口上沿、下沿或中线。滚动范围在文末
        // 多留了半屏，末尾内容也能对齐；越界时钳到滚动范围两端。
        if placement.is_aligned() {
            let delta = Self::alignment_delta(viewport, active_bounds, placement);
            let mut offset = self.scroll_handle.offset();
            offset.y += px(delta);
            let max_offset_y = self.scroll_handle.max_offset().y.max(px(0.0));
            offset.y = offset.y.min(px(0.0)).max(-max_offset_y);
            let moved = self.scroll_handle.offset().y != offset.y;
            if moved {
                self.caret_scroll_applications
                    .set(self.caret_scroll_applications.get() + 1);
            }
            self.scroll_handle.set_offset(offset);
            return CaretScroll { has_bounds: true, moved };
        }

        let padding = px(20.0);
        let top_limit = viewport.top() + padding;
        let bottom_limit = viewport.bottom() - padding;
        let mut offset = self.scroll_handle.offset();
        let mut changed = false;

        if active_bounds.top() < top_limit {
            offset.y += top_limit - active_bounds.top();
            changed = true;
        } else if active_bounds.bottom() > bottom_limit {
            offset.y -= active_bounds.bottom() - bottom_limit;
            changed = true;
        }

        let before = self.scroll_handle.offset();
        if changed {
            let max_offset_y = self.scroll_handle.max_offset().y.max(px(0.0));
            offset.y = offset.y.min(px(0.0)).max(-max_offset_y);
            if before.y != offset.y {
                self.caret_scroll_applications
                    .set(self.caret_scroll_applications.get() + 1);
            }
            self.scroll_handle.set_offset(offset);
        }
        CaretScroll {
            has_bounds: true,
            moved: self.scroll_handle.offset().y != before.y,
        }
    }

    /// 初始加载/普通可见性滚动的校验帧数（16ms 一帧，约 100ms）。
    const SCROLL_SETTLE_FRAMES: u8 = 6;
    /// 跳转对齐的帧数硬上限（~1.5s）。绝对对齐本应一帧到位；大文档跳转时
    /// 渲染窗口扩展、行高测量逐帧漂移，对齐要追着漂移校正。清除条件改为
    /// 「偏差达标即停」（见 apply），帧数只作防呆上限——此前 24 帧耗尽即弃，
    /// 命中停在视口边缘。
    const JUMP_SCROLL_SETTLE_FRAMES: u8 = 90;
    /// 跳转对齐收敛阈值：目标位置与视口边界的偏差 ≤ 此值即视为到位。
    const JUMP_ALIGN_TOLERANCE_PX: f32 = 2.0;

    /// 目标块按口径对齐到视口所需的纵向位移（像素）。
    fn alignment_delta(
        viewport: Bounds<Pixels>,
        bounds: Bounds<Pixels>,
        placement: ScrollPlacement,
    ) -> f32 {
        let (anchor, target) = match placement {
            ScrollPlacement::Top => (bounds.top(), viewport.top()),
            ScrollPlacement::Bottom => (bounds.bottom(), viewport.bottom()),
            _ => (
                bounds.top() + bounds.size.height * 0.5,
                viewport.top() + viewport.size.height * 0.5,
            ),
        };
        f32::from(target) - f32::from(anchor)
    }

    pub(crate) fn apply_pending_scroll_into_view(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.scrollbar_drag.is_some() {
            return;
        }

        if !self.pending_scroll_active_block_into_view {
            return;
        }
        // 跳转请求自己带帧预算：目标未挂载时靠估算 + 爬行接近，只给一帧
        // 会停在半路（vim 的 gg/G 不经过搜索跳转的 recheck 路径）。
        let settle_budget = if self.pending_scroll_placement.is_aligned() {
            Self::JUMP_SCROLL_SETTLE_FRAMES
        } else {
            Self::SCROLL_SETTLE_FRAMES
        };
        if self.pending_scroll_placement != ScrollPlacement::IntoView
            && self.scroll_settle_frames == 0
        {
            self.scroll_settle_frames = settle_budget;
            self.scroll_request_moved_viewport = false;
        }

        // 撤销/换模式/打开文件会替换整篇块：这一帧块边界和行高还来自旧布局或
        // 估计值，拿它算「光标离边界多远」会多滚一截，下一帧再被真实布局纠正
        // ——用户看到窗口来回滚。所以布局重算的那一帧先不滚，等下一帧拿到新
        // 布局再滚一次，之后再校验几帧直到测量落定。
        if self.pending_scroll_recheck_after_layout {
            self.pending_scroll_recheck_after_layout = false;
            self.scroll_settle_frames = if self.pending_scroll_placement == ScrollPlacement::IntoView {
                Self::SCROLL_SETTLE_FRAMES
            } else {
                Self::JUMP_SCROLL_SETTLE_FRAMES
            };
            self.schedule_followup_frame(cx);
            return;
        }

        // scroll_to_item indexed children by position, which the spacers break;
        // the focused block is always mounted, so pixel math on its bounds works.
        let caret = self.ensure_focused_caret_visible(window, cx);
        if caret.moved {
            self.scroll_request_moved_viewport = true;
        }
        let placement = self.pending_scroll_placement;
        let settled = (caret.has_bounds
            && placement.is_aligned()
            && !caret.moved
            && self.jump_alignment_converged(placement, cx))
            // 跳转一次都没挪过视口：光标本来就在窗里，不必再校验帧。
            || (caret.has_bounds
                && placement == ScrollPlacement::Reveal
                && !caret.moved
                && !self.scroll_request_moved_viewport);
        if !settled {
            // 还在移动就续满预算：跳转后行高逐帧补测，块会继续漂；停稳
            // 若干帧才算收敛，否则会停在半路。IntoView 不续：初始加载的
            // 校验帧要用完就收，否则用户手动滚走后会被一直拽回活动块。
            if caret.moved && placement != ScrollPlacement::IntoView {
                self.scroll_settle_frames = settle_budget;
            } else {
                self.scroll_settle_frames = self.scroll_settle_frames.saturating_sub(1);
            }
            if self.scroll_settle_frames > 0 {
                self.schedule_followup_frame(cx);
                return;
            }
        }
        self.pending_scroll_active_block_into_view = false;
        self.pending_scroll_placement = ScrollPlacement::IntoView;
        self.scroll_request_moved_viewport = false;
        self.scroll_settle_frames = 0;
        self.scroll_recheck_task = None;
    }

    /// 跳转是否已对齐：再走一遍对齐位移，位移量落在容差内即到位；目标已经
    /// 顶到滚动范围端点时也按到位处理（再滚也到不了）。
    fn jump_alignment_converged(&self, placement: ScrollPlacement, cx: &App) -> bool {
        let anchor_id = self.active_entity_id;
        let Some(anchor_block) = anchor_id.and_then(|id| {
            self.document
                .block_entity_by_id(id)
                .or_else(|| self.focusable_entity_by_id(id))
        }) else {
            return false;
        };
        let is_cell = anchor_block.read_with(cx, |block, _cx| {
            block.table_cell_position().is_some()
        });
        let bounds = if is_cell {
            self.scroll_target_bounds(&anchor_block, true, cx)
                .or_else(|| {
                    let binding = self.table_cell_binding(anchor_block.entity_id())?;
                    let host = binding.table_block;
                    self.scroll_target_bounds(&host, true, cx)
                })
        } else {
            self.scroll_target_bounds(&anchor_block, true, cx)
        };
        let Some(bounds) = bounds else {
            return false;
        };
        let viewport = self.scroll_handle.bounds();
        let delta = Self::alignment_delta(viewport, bounds, placement);
        let current = f32::from(self.scroll_handle.offset().y);
        let max_offset_y = f32::from(self.scroll_handle.max_offset().y).max(0.0);
        let clamped = (current + delta).min(0.0).max(-max_offset_y);
        (clamped - current).abs() <= Self::JUMP_ALIGN_TOLERANCE_PX
    }

    pub(crate) fn scroll_target_bounds(
        &self,
        block: &Entity<Block>,
        include_shell: bool,
        cx: &App,
    ) -> Option<Bounds<Pixels>> {
        let host_id = self.table_cell_binding(block.entity_id())
            .map(|binding| binding.table_block.entity_id())
            .unwrap_or(block.entity_id());
        if !self.entity_is_mounted(host_id) { return None; }
        let state = block.read(cx);
        let caret = state.active_range_or_cursor_bounds();
        if !include_shell { return caret; }
        if let Some(bounds) = caret.or(state.last_bounds) {
            return Some(bounds);
        }
        self.mounted_row_bounds(host_id)
    }

    /// 这一块所在挂载行的窗口坐标。
    ///
    /// 表格、分隔线这类没有文字元素的块只有行框可量（`last_bounds` 恒为空），
    /// 「把某块滚到视口顶」对它们只能按行对齐。
    pub(crate) fn mounted_row_bounds(&self, entity_id: EntityId) -> Option<Bounds<Pixels>> {
        let run = self.prev_mounted_run.filter(|run| self.mounted_run_is_addressable(*run))?;
        let plan = self.rendered_row_plan.as_ref()?;
        let (_, child) = run.rows().find(|(row, _)| {
            plan.rows.get(*row).is_some_and(|row| {
                row.entities().any(|entity| entity.entity_id() == entity_id)
            })
        })?;
        let mut bounds = self.scroll_handle.bounds_for_item(child)?;
        bounds.origin += self.scroll_handle.offset();
        Some(bounds)
    }

    /// Requests a repaint one frame out for work that cannot finish inside this
    /// frame: a scroll-into-view whose target block has no measured bounds yet,
    /// or a cold-start run that still has not covered the viewport. `cx.notify()`
    /// is swallowed when called from within `render`, so without this the retry
    /// would wait for the next external notify (e.g. the cursor blink, ~0.5s
    /// later).
    pub(crate) fn schedule_followup_frame(&mut self, cx: &mut Context<Self>) {
        self.scroll_recheck_task = Some(cx.spawn(async move |this: WeakEntity<Self>, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(16))
                .await;
            let _ = this.update(cx, |_this, cx| cx.notify());
        }));
    }

    pub(crate) fn sync_pending_save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending_save && !self.has_marked_document_text(cx) {
            self.pending_save = false;
            self.save_document(window, cx);
        }
    }

    /// 切换工作区后补开最近剩下的标签（`set_workspace_root` 当时没有 Window）。
    pub(crate) fn sync_pending_workspace_tab_activation(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(path) = self.pending_workspace_tab_activation.take() else {
            return;
        };
        self.show_welcome = false;
        self.open_workspace_file(path, window, cx);
    }

    pub(crate) fn sync_pending_save_as(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending_save_as && !self.has_marked_document_text(cx) {
            self.pending_save_as = false;
            self.save_document_as(window, cx);
        }
    }

    pub(crate) fn sync_window_edited_state(&mut self, window: &mut Window) {
        if self.pending_window_unedited {
            self.pending_window_unedited = false;
            window.set_window_edited(false);
        } else if self.pending_window_edited {
            self.pending_window_edited = false;
            window.set_window_edited(true);
        }
    }

    pub(crate) fn sync_scroll_viewport(&mut self, viewport_size: Size<Pixels>, cx: &mut Context<Self>) {
        match self.last_scroll_viewport_size {
            Some(previous) if Self::viewport_size_changed(previous, viewport_size) => {
                self.last_scroll_viewport_size = Some(viewport_size);
                self.request_active_block_scroll_into_view(cx);
            }
            Some(_) => {}
            None => {
                self.last_scroll_viewport_size = Some(viewport_size);
            }
        }
    }

    pub(crate) fn sync_window_title(&mut self, window: &mut Window, strings: &I18nStrings) {
        if self.pending_window_title_refresh {
            self.pending_window_title_refresh = false;
            let title = Self::window_title(
                self.file_path.as_deref(),
                self.recovery_source_path.as_deref(),
                self.is_recovered_document,
                self.document_dirty,
                strings,
            );
            window.set_window_title(&title);
        }
    }

}
