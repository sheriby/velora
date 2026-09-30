//! Unsaved-changes dialog and window-close interception.
//!
//! When the document is dirty, `Editor::on_window_should_close` returns
//! false and shows an overlay offering three choices: save-and-close,
//! discard-and-close, or keep editing.  Focus is restored to the
//! previously active block when the dialog is dismissed without closing.

use gpui::*;

use super::Editor;

/// 拖动/缩放窗口后隔多久落盘一次 frame。防抖的理由：拖动过程中平台会连续发
/// 尺寸变化通知，而每次落盘都是 config.toml 的读-改-写（Windows Defender 的
/// 实时扫描会放大同步小写的延迟）。
const WINDOW_FRAME_WRITE_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(500);

impl Editor {
    pub(crate) fn request_close_current_window(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_menu_bar(cx);
        self.hide_info_dialog(cx);
        self.pending_close_after_save = false;

        if self.on_window_should_close(window, cx) {
            self.close_dialog_restore_focus = None;
            self.persist_session(cx);
            Self::close_editor_window(window);
        }
    }

    /// Remembers the window frame and removes the window (roadmap A2).
    ///
    /// Every path that removes an editor window goes through here: the frame is
    /// only readable while the window is alive, and forgetting it in one path
    /// (the quit action, discarding unsaved changes, …) silently lost the user's
    /// window position and size.
    pub(crate) fn close_editor_window(window: &mut Window) {
        Self::persist_window_frame(window);
        window.remove_window();
    }

    /// Saves the current window frame so the next launch can restore it
    /// (roadmap A2). Called from [`Self::close_editor_window`] and from
    /// `on_window_should_close` before the platform closes the window.
    /// 窗口被拖动/缩放/最大化时立即记住 frame（防抖后后台落盘）。
    ///
    /// 只在退出路径上落盘不够：强杀进程、平台关闭回调缺位、或者调完窗口程序就崩，
    /// 最后一次调整都会丢掉，下一次启动只能拿旧 frame（甚至默认尺寸）。关窗/退出
    /// 路径仍然会同步写一次，兜住「刚调完就退出」——那条路径等不了防抖。
    pub(crate) fn install_window_frame_recorder(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.window_bounds_subscription.is_some() {
            return;
        }
        self.observed_window_frame = Some(Self::current_window_frame(window));
        self.window_bounds_subscription = Some(cx.observe_window_bounds(
            window,
            |editor, window, cx| {
                editor.record_changed_window_frame(window, cx);
            },
        ));
    }

    fn record_changed_window_frame(&mut self, window: &Window, cx: &mut Context<Self>) {
        let frame = Self::current_window_frame(window);
        if self.observed_window_frame == Some(frame) {
            return;
        }
        self.observed_window_frame = Some(frame);
        // 重新赋值会 drop 上一个 Task，等于取消上一次防抖计时。
        self.window_frame_write_task = Some(cx.spawn(
            async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
                cx.background_executor()
                    .timer(WINDOW_FRAME_WRITE_DEBOUNCE)
                    .await;
                cx.background_executor()
                    .spawn(async move {
                        static WINDOW_FRAME_WRITE_LOCK: std::sync::Mutex<()> =
                            std::sync::Mutex::new(());
                        let _guard = WINDOW_FRAME_WRITE_LOCK.lock().ok();
                        if let Err(error) = crate::config::store_window_frame(frame) {
                            eprintln!("failed to save window frame: {error}");
                        }
                    })
                    .await;
            },
        ));
    }

    /// 当前窗口 frame（位置 + 大小）：落盘与变化检测共用同一份取值。
    fn current_window_frame(window: &Window) -> crate::config::WindowFrame {
        let frame = match window.window_bounds() {
            gpui::WindowBounds::Windowed(bounds)
            | gpui::WindowBounds::Maximized(bounds)
            | gpui::WindowBounds::Fullscreen(bounds) => bounds,
        };
        crate::config::WindowFrame {
            x: f32::from(frame.origin.x) as i32,
            y: f32::from(frame.origin.y) as i32,
            width: f32::from(frame.size.width) as i32,
            height: f32::from(frame.size.height) as i32,
        }
    }

    pub(crate) fn persist_window_frame(window: &Window) {
        let _ = crate::config::store_window_frame(Self::current_window_frame(window));
    }

    pub(crate) fn restore_focus_after_close_dialog(&mut self, cx: &mut Context<Self>) {
        if let Some(focus_id) = self.close_dialog_restore_focus.take() {
            self.pending_focus = Some(focus_id);
            self.pending_scroll_active_block_into_view = true;
            cx.notify();
        }
    }

    /// 浮层关闭后把焦点还给之前聚焦的正文块（⌘P/⇧⌘P 关闭后敲字不再丢）。
    pub(super) fn restore_focus_after_overlay(&mut self, cx: &mut Context<Self>) {
        if let Some(entity_id) = self.overlay_focus_restore_target.take() {
            self.pending_focus = Some(entity_id);
            cx.notify();
        }
    }

    pub(crate) fn hide_unsaved_changes_dialog(&mut self, cx: &mut Context<Self>) {
        if self.show_unsaved_changes_dialog {
            self.show_unsaved_changes_dialog = false;
            cx.notify();
        }
    }

    pub(crate) fn abort_pending_close_after_save(&mut self, cx: &mut Context<Self>) {
        let had_pending_close = self.pending_close_after_save;
        self.pending_close_after_save = false;
        self.close_menu_bar(cx);
        self.hide_unsaved_changes_dialog(cx);
        if had_pending_close {
            self.restore_focus_after_close_dialog(cx);
        } else {
            self.close_dialog_restore_focus = None;
        }
    }

    pub(crate) fn on_window_should_close(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.document_dirty && !self.has_dirty_workspace_documents() {
            // 平台自己发起的关闭（macOS 红灯）不经过任何应用内关闭入口，
            // 这里是最后能读到该窗口 frame 的地方（roadmap A2）。
            Self::persist_window_frame(window);
            return true;
        }

        self.close_menu_bar(cx);
        self.hide_info_dialog(cx);
        if !self.show_unsaved_changes_dialog {
            self.close_dialog_restore_focus = self.document.focused_block_entity_id(window, cx);
            self.show_unsaved_changes_dialog = true;
            window.blur();
            cx.notify();
        }

        false
    }

    pub(crate) fn on_cancel_close_dialog(
        &mut self,
        _: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.pending_close_after_save = false;
        self.close_menu_bar(cx);
        self.hide_unsaved_changes_dialog(cx);
        self.restore_focus_after_close_dialog(cx);
    }

    pub(crate) fn on_discard_and_close(
        &mut self,
        _: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.pending_close_after_save = false;
        self.close_dialog_restore_focus = None;
        self.close_menu_bar(cx);
        self.hide_unsaved_changes_dialog(cx);
        self.document_revision = self.document_revision.wrapping_add(1);
        self.autosave_task = None;
        for recovery_id in self.workspace_recovery_ids() {
            if let Err(error) = crate::config::remove_recovery_snapshot(recovery_id) {
                eprintln!("failed to remove discarded tab recovery snapshot: {error}");
            }
        }
        if let Err(error) = crate::config::remove_recovery_snapshot(self.recovery_id) {
            eprintln!("failed to remove discarded document recovery snapshot: {error}");
        }
        Self::close_editor_window(window);
    }

    pub(crate) fn on_save_and_close(
        &mut self,
        _: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.has_dirty_workspace_documents()
            && (self.file_path.is_some() || !self.document_dirty)
        {
            self.close_menu_bar(cx);
            self.hide_unsaved_changes_dialog(cx);
            self.save_dirty_workspace_documents_and_close(window, cx);
            return;
        }
        self.pending_close_after_save = true;
        self.close_menu_bar(cx);
        self.hide_unsaved_changes_dialog(cx);
        self.pending_save = true;
        cx.notify();
        // The window closes after the save completes; persist the frame now
        // while the window is still alive.
        Self::persist_window_frame(window);
    }
}
