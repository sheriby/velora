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
        let focused_block = self.focused_edit_target(window, cx).or_else(|| {
            self.active_entity_id
                .and_then(|entity_id| self.focusable_entity_by_id(entity_id))
        });
        let Some(focused_block) = focused_block else {
            return false;
        };
        let Some(active_bounds) =
            focused_block.read_with(cx, |block, _cx| block.active_range_or_cursor_bounds())
        else {
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

    /// 整篇替换后光标滚动的校验帧数（16ms 一帧，约 100ms）。
    const SCROLL_SETTLE_FRAMES: u8 = 6;

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
            self.scroll_settle_frames = Self::SCROLL_SETTLE_FRAMES;
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
