use super::*;

impl Editor {
    pub(crate) fn bump_scrollbar_visibility(&mut self, cx: &mut Context<Self>) {
        let duration = Duration::from_millis(900);
        self.scrollbar_visible_until = Instant::now() + duration;

        // 已有淡出任务时就只延长显示时间：滚轮每 tick 都起一个定时任务，
        // 长滚动会堆出一串已无意义的后台任务。
        if self.scrollbar_fade_task.is_none() {
            let weak_editor = cx.entity().downgrade();
            self.scrollbar_fade_task = Some(cx.spawn(
                async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
                    loop {
                        cx.background_executor()
                            .timer(duration + Duration::from_millis(50))
                            .await;
                        let keep_waiting = weak_editor
                            .update(cx, |this, cx| {
                                if Instant::now() < this.scrollbar_visible_until {
                                    // 等待期间又滚动过：按新的截止时间再等一轮。
                                    return true;
                                }
                                this.scrollbar_fade_task = None;
                                cx.notify();
                                false
                            })
                            .unwrap_or(false);
                        if !keep_waiting {
                            return;
                        }
                    }
                },
            ));
        }

        cx.notify();
    }

    pub(crate) fn on_editor_hover(
        &mut self,
        hovered: &bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.scrollbar_hovered = *hovered;
        if *hovered {
            self.bump_scrollbar_visibility(cx);
        } else {
            cx.notify();
        }
    }

    pub(crate) fn on_menu_bar_hover(
        &mut self,
        hovered: &bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_menu_bar_hovered(*hovered, cx);
    }

    pub(crate) fn on_menu_panel_hover(
        &mut self,
        hovered: &bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_menu_panel_hovered(*hovered, cx);
    }

    pub(crate) fn on_menu_submenu_panel_hover(
        &mut self,
        hovered: &bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_menu_submenu_panel_hovered(*hovered, cx);
    }

    pub(crate) fn on_menu_submenu_bridge_hover(
        &mut self,
        hovered: &bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_menu_submenu_bridge_hovered(*hovered, cx);
    }

    pub(crate) fn on_editor_mouse_down(
        &mut self,
        _event: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dismiss_menu_bar_from_body(cx);
        self.clear_table_axis_preview(cx);
        self.clear_table_axis_selection(cx);
    }

    pub(crate) fn on_editor_scroll_wheel(
        &mut self,
        _event: &ScrollWheelEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.typewriter_mode {
            self.pending_scroll_active_block_into_view = false;
            self.pending_scroll_recheck_after_layout = false;
        }
        self.bump_scrollbar_visibility(cx);
    }

    pub(crate) fn on_page_up(
        &mut self,
        _: &crate::components::PageUp,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let page = self.scroll_handle.bounds().size.height;
        self.scroll_viewport_by(page, cx);
    }

    pub(crate) fn on_page_down(
        &mut self,
        _: &crate::components::PageDown,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let page = self.scroll_handle.bounds().size.height;
        self.scroll_viewport_by(-page, cx);
    }

    pub(crate) fn on_jump_to_top(
        &mut self,
        _: &crate::components::JumpToTop,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_vertical_scroll_offset(px(0.0), cx);
    }

    pub(crate) fn on_jump_to_bottom(
        &mut self,
        _: &crate::components::JumpToBottom,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let max_offset_y = self.scroll_handle.max_offset().height.max(px(0.0));
        self.set_vertical_scroll_offset(-max_offset_y, cx);
    }

    /// Scrolls the viewport vertically by `delta`. A positive `delta` moves
    /// toward the start of the document; a negative one moves toward the end.
    /// One page is the current viewport height, so the step tracks window size.
    pub(crate) fn scroll_viewport_by(&mut self, delta: Pixels, cx: &mut Context<Self>) {
        let target = self.scroll_handle.offset().y + delta;
        self.set_vertical_scroll_offset(target, cx);
    }

    /// Applies an absolute vertical scroll offset, clamped to the scrollable
    /// range. Offsets run from 0 at the top to `-max_offset` at the bottom.
    pub(crate) fn set_vertical_scroll_offset(&mut self, target_y: Pixels, cx: &mut Context<Self>) {
        let max_offset_y = self.scroll_handle.max_offset().height.max(px(0.0));
        let mut offset = self.scroll_handle.offset();
        offset.y = target_y.min(px(0.0)).max(-max_offset_y);
        self.scroll_handle.set_offset(offset);
        // A direct viewport scroll should stick, so cancel any queued pass that
        // would otherwise re-center the active block on the next frame.
        self.pending_scroll_active_block_into_view = false;
        self.pending_scroll_recheck_after_layout = false;
        self.bump_scrollbar_visibility(cx);
        cx.notify();
    }

    pub(crate) fn start_scrollbar_drag(
        &mut self,
        pointer_offset_y: f32,
        track_height: f32,
        thumb_height: f32,
        max_scroll_y: f32,
        cx: &mut Context<Self>,
    ) {
        self.scrollbar_drag = Some(crate::editor::ScrollbarDragSession {
            pointer_offset_y: pointer_offset_y.clamp(0.0, thumb_height.max(0.0)),
            track_height,
            thumb_height,
            max_scroll_y,
        });
        self.pending_scroll_active_block_into_view = false;
        self.pending_scroll_recheck_after_layout = false;
        self.bump_scrollbar_visibility(cx);
        cx.notify();
    }

    pub(crate) fn update_scrollbar_drag(
        &mut self,
        pointer_y_in_track: f32,
        cx: &mut Context<Self>,
    ) {
        let Some(drag) = self.scrollbar_drag else {
            return;
        };

        let travel = (drag.track_height - drag.thumb_height).max(0.0);
        let thumb_top = (pointer_y_in_track - drag.pointer_offset_y).clamp(0.0, travel);
        let scroll_y = Self::scroll_offset_for_thumb_top(
            thumb_top,
            drag.track_height,
            drag.thumb_height,
            drag.max_scroll_y,
        );

        let mut offset = self.scroll_handle.offset();
        offset.y = -px(scroll_y);
        self.scroll_handle.set_offset(offset);
        self.bump_scrollbar_visibility(cx);
        cx.notify();
    }

    pub(crate) fn end_scrollbar_drag(&mut self, cx: &mut Context<Self>) {
        if self.scrollbar_drag.take().is_some() {
            self.bump_scrollbar_visibility(cx);
            cx.notify();
        }
    }
}
