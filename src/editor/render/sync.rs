use super::*;

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
        if let Some(entity_id) = self.pending_focus.take()
            && let Some(block) = self.focusable_entity_by_id(entity_id)
        {
            block.read(cx).focus_handle.focus(window);
        }
    }

    pub(crate) fn ensure_focused_caret_visible(&mut self, window: &Window, cx: &App) -> bool {
        // 搜索跳转会把焦点交还查询框：滚动目标改用 active_entity_id，
        // 不让「滚到命中」依赖正文块持有焦点，也避免挂着的滚动请求在
        // 没有焦点块时反复排后续帧。
        // 跳转滚动进行中只认 active 锚点：命中落在表格单元格里时焦点会
        // 回落到文档首块（cell 不能持有窗口焦点），信焦点块就会滚向文档
        // 开头而不是命中（用户报修：表格里的命中点了没反应）。
        let anchor_id = self.active_entity_id;
        // 锚点实体 → 滚动实体：cell 不是可滚动实体（不在可见块列表、无独立
        // 布局边界），升级为宿主表格块，把整个表格滚进视口，选区留在单元格。
        // 悬空 id（表格重建替换过 cell）回退 focusable 注册表，再不行按文档
        // 树为准。
        let scroll_block = anchor_id.and_then(|id| {
            self.document
                .block_entity_by_id(id)
                .or_else(|| self.focusable_entity_by_id(id))
        });
        let scroll_block = scroll_block.map(|block| {
            let is_cell = block.read_with(cx, |b, _cx| b.table_cell_position().is_some());
            if is_cell
                && let Some(binding) = self.table_cell_binding(block.entity_id())
            {
                binding.table_block.clone()
            } else {
                block
            }
        });
        let focused_block = if self.pending_scroll_center_into_view {
            scroll_block
        } else {
            self.focused_edit_target(window, cx).or(scroll_block)
        };
        let Some(focused_block) = focused_block else {
            return false;
        };
        let Some(active_bounds) =
            focused_block.read_with(cx, |block, _cx| block.active_range_or_cursor_bounds())
        else {
            // 目标块尚未绘制：渲染窗口只画视口附近的块，窗外的块既没有
            // last_bounds 也没有文本布局，精确居中无从算起——但不滚就永远
            // 不会画，死锁（用户报修：跨标签搜索跳转后视口停在文档顶部，
            // 要手动翻完整篇才看得到命中）。
            // 估算/爬行只在程序性跳转（center 标志）时进行：初始加载的
            // pending 不带 center，视口在哪都该原地不动，否则用户手动滚走
            // 后活动块出窗、无边界，会被估算一路拉回（回归：reading_to_
            // the_bottom 测试实测视口被拽回顶部）。
            if !self.pending_scroll_center_into_view {
                return false;
            }
            //
            // 危险在于估算和绘制互相干扰：比例跳一步把目标拉进窗口，目标
            // 一有边界就触发精确居中，而居中把视口滚向目标后目标可能又滑出
            // 窗口、边界被丢弃，估算再跳…… 来回震荡耗尽 settle 帧数（用户
            // 报修：向上跳回开头永远停在半路）。所以估算只在离目标还远时
            // 大步跳；一旦接近（约一屏内）改用固定半屏步长单调爬行，保证
            // 每一帧都净逼近，绝不过冲。
            let viewport_height = f32::from(self.scroll_handle.bounds().size.height);
            let content_height =
                f32::from(self.scroll_handle.max_offset().height) + viewport_height;
            // focused_block 已是滚动实体（cell 已升级为宿主表格块），
            // 直接按它在可见列表中的位置估算。
            let index = self
                .document
                .visible_index_for_entity_id(focused_block.entity_id());

            if viewport_height > 0.0
                && let Some(index) = index
            {
                let total = self.document.visible_blocks().len().max(1);
                let estimate_y = content_height * (index as f32 + 0.5) / total as f32;
                let mut offset = self.scroll_handle.offset();
                let target_y = -(estimate_y - viewport_height * 0.5);
                let max_offset_y = f32::from(self.scroll_handle.max_offset().height).max(0.0);
                let target_y = target_y.min(0.0).max(-max_offset_y);
                let current = f32::from(offset.y);
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
                    }
                } else {
                    let clamped = target_y;
                    if (current - clamped).abs() > 1.0 {
                        offset.y = px(clamped);
                        self.scroll_handle.set_offset(offset);
                    }
                }
            }
            return false;
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
                f32::from(self.scroll_handle.max_offset().height),
            );
            if (target - f32::from(offset.y)).abs() > 0.5 {
                offset.y = px(target);
                self.caret_scroll_applications
                    .set(self.caret_scroll_applications.get() + 1);
                self.scroll_handle.set_offset(offset);
            }
            return true;
        }
        // Outline/search jumps land the target at the viewport center. The
        // scroll range already reserves half a viewport past the end, so
        // trailing content can center too; top-of-document clamps to 0.
        if self.pending_scroll_center_into_view {
            let viewport_center = f32::from(viewport.top()) + f32::from(viewport.size.height) * 0.5;
            let target_center =
                f32::from(active_bounds.top()) + f32::from(active_bounds.size.height) * 0.5;
            let mut offset = self.scroll_handle.offset();
            offset.y += px(viewport_center - target_center);
            let max_offset_y = self.scroll_handle.max_offset().height.max(px(0.0));
            offset.y = offset.y.min(px(0.0)).max(-max_offset_y);
            if self.scroll_handle.offset().y != offset.y {
                self.caret_scroll_applications
                    .set(self.caret_scroll_applications.get() + 1);
            }
            self.scroll_handle.set_offset(offset);
            return true;
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

        if changed {
            let max_offset_y = self.scroll_handle.max_offset().height.max(px(0.0));
            offset.y = offset.y.min(px(0.0)).max(-max_offset_y);
            if self.scroll_handle.offset().y != offset.y {
                self.caret_scroll_applications
                    .set(self.caret_scroll_applications.get() + 1);
            }
            self.scroll_handle.set_offset(offset);
        }

        true
    }

    /// 初始加载/普通可见性滚动的校验帧数（16ms 一帧，约 100ms）。
    const SCROLL_SETTLE_FRAMES: u8 = 6;
    /// 跳转型滚动（center 标志）的校验帧数。目标块在未绘制区域时先估算/
    /// 爬行靠近、边界落地后精确居中——深处来回跳需要覆盖整段爬行，短窗口
    /// 会在半路停掉（用户报修：向上跳停在中间）。
    const JUMP_SCROLL_SETTLE_FRAMES: u8 = 24;

    pub(crate) fn apply_pending_scroll_into_view(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.scrollbar_drag.is_some() {
            return;
        }

        if !self.pending_scroll_active_block_into_view {
            return;
        }

        // 撤销/换模式/打开文件会替换整篇块：这一帧块边界和行高还来自旧布局或
        // 估计值，拿它算「光标离边界多远」会多滚一截，下一帧再被真实布局纠正
        // ——用户看到窗口来回滚。所以布局重算的那一帧先不滚，等下一帧拿到新
        // 布局再滚一次，之后再校验几帧直到测量落定。
        if self.pending_scroll_recheck_after_layout {
            self.pending_scroll_recheck_after_layout = false;
            self.scroll_settle_frames = if self.pending_scroll_center_into_view {
                Self::JUMP_SCROLL_SETTLE_FRAMES
            } else {
                Self::SCROLL_SETTLE_FRAMES
            };
            self.schedule_followup_frame(cx);
            return;
        }

        // scroll_to_item indexed children by position, which the spacers break;
        // the focused block is always mounted, so pixel math on its bounds works.
        let has_bounds = self.ensure_focused_caret_visible(window, cx);
        if !has_bounds {
            self.schedule_followup_frame(cx);
            return;
        }

        if self.scroll_settle_frames > 0 {
            self.scroll_settle_frames -= 1;
            self.schedule_followup_frame(cx);
            return;
        }

        self.pending_scroll_active_block_into_view = false;
        self.pending_scroll_center_into_view = false;
        self.scroll_recheck_task = None;
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
