//! Unsaved-changes dialog and window-close interception.
//!
//! When the document is dirty, `Editor::on_window_should_close` returns
//! false and shows an overlay offering three choices: save-and-close,
//! discard-and-close, or keep editing.  Focus is restored to the
//! previously active block when the dialog is dismissed without closing.

use gpui::*;

use super::Editor;

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
            Self::persist_window_frame(window);
            window.remove_window();
        }
    }

    /// Saves the current window frame so the next launch can restore it
    /// (roadmap A2). Called from every path that removes an editor window.
    pub(crate) fn persist_window_frame(window: &Window) {
        let frame = match window.window_bounds() {
            gpui::WindowBounds::Windowed(bounds)
            | gpui::WindowBounds::Maximized(bounds)
            | gpui::WindowBounds::Fullscreen(bounds) => bounds,
        };
        let _ = crate::config::store_window_frame(crate::config::WindowFrame {
            x: f32::from(frame.origin.x) as i32,
            y: f32::from(frame.origin.y) as i32,
            width: f32::from(frame.size.width) as i32,
            height: f32::from(frame.size.height) as i32,
        });
    }

    pub(crate) fn restore_focus_after_close_dialog(&mut self, cx: &mut Context<Self>) {
        if let Some(focus_id) = self.close_dialog_restore_focus.take() {
            self.pending_focus = Some(focus_id);
            self.pending_scroll_active_block_into_view = true;
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
        window.remove_window();
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
