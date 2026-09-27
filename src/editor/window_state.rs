//! Window-level editor state such as scrolling, mode switching, and menus.
//! 窗口标题显示 velora。

use super::*;

/// P4b：stride 大多未知时的单帧最大挂载行数。
const COLD_RUN_MAX_ROWS: usize = 12;

impl Editor {
    pub(super) fn scrollbar_geometry(
        viewport_height: f32,
        max_scroll_y: f32,
        current_scroll_y: f32,
    ) -> ScrollbarGeometry {
        let track_height = viewport_height.max(20.0);
        let content_height = viewport_height + max_scroll_y;
        let thumb_height = if max_scroll_y > 0.5 {
            (track_height * (viewport_height / content_height))
                .clamp(28.0_f32.min(track_height), track_height)
        } else {
            track_height
        };
        let progress = if max_scroll_y > 0.0 {
            current_scroll_y.clamp(0.0, max_scroll_y) / max_scroll_y
        } else {
            0.0
        };
        let thumb_top = (track_height - thumb_height).max(0.0) * progress;
        ScrollbarGeometry {
            track_height,
            thumb_height,
            thumb_top,
            max_scroll_y,
        }
    }

    pub(super) fn scroll_offset_for_thumb_top(
        thumb_top: f32,
        track_height: f32,
        thumb_height: f32,
        max_scroll_y: f32,
    ) -> f32 {
        if max_scroll_y <= 0.0 {
            return 0.0;
        }

        let travel = (track_height - thumb_height).max(0.0);
        if travel <= 0.0 {
            return 0.0;
        }

        let progress = (thumb_top / travel).clamp(0.0, 1.0);
        max_scroll_y * progress
    }

    /// Whether last frame's child indices still address the same children.
    /// Footprints are read back by index, so anything added to the scroll column
    /// would otherwise pair the wrong rows silently; a changed child count means
    /// the recorded indices are stale and the refresh must be skipped.
    pub(super) fn mounted_run_is_addressable(&self, run: MountedRun) -> bool {
        run.child_count > 0
            && self
                .scroll_handle
                .bounds_for_item(run.child_count - 1)
                .is_some()
            && self
                .scroll_handle
                .bounds_for_item(run.child_count)
                .is_none()
    }

    /// Picks the contiguous run of rows to mount; the culled runs become
    /// spacers and the focused row stays mounted, on its own island when it
    /// falls outside the run. `strides[i]` is row `i`'s
    /// footprint (height plus trailing gap); being scroll-invariant, their running
    /// sum places each row against a band from the current scroll offset.
    /// Unmeasured rows use a lower-bound estimate; where that falls short of the
    /// scroll offset the trailing run is mounted instead, so the window never
    /// lands on a spacer. Pure, so it is unit-tested headlessly.
    pub(super) fn rendered_window(
        strides: &[f32],
        scroll_y: f32,
        viewport_height: f32,
        overdraw: f32,
        focus_row: Option<usize>,
        estimate: f32,
    ) -> RenderWindow {
        let n = strides.len();
        if n == 0 {
            return RenderWindow {
                run_start: 0,
                run_end: 0,
                top_h: 0.0,
                bottom_h: 0.0,
                focus_island: None,
            };
        }

        let band_top = scroll_y - overdraw;
        let band_bottom = scroll_y + viewport_height + overdraw;

        let mut run_start = n;
        let mut run_end = 0usize;
        let mut top_of_start = 0.0f32;
        let mut bottom_of_end = 0.0f32;
        let mut cursor = 0.0f32;
        for (index, &stride) in strides.iter().enumerate() {
            let top = cursor;
            let bottom = cursor + stride.max(0.0);
            if bottom >= band_top && top <= band_bottom {
                if index < run_start {
                    run_start = index;
                    top_of_start = top;
                }
                run_end = index + 1;
                bottom_of_end = bottom;
            }
            cursor = bottom;
        }
        let total = cursor;

        // P4b 冷启动保护：绝大多数 stride 还是估计值时，行高被严重低估
        // （一行真实 9000px 估计 16px），带状扫描会一口气挂载几十个巨行。
        // 限制首帧挂载数，让 stride 逐帧学习后自然放宽。
        let known = strides.iter().filter(|&&stride| stride > estimate).count();
        if known * 2 < n {
            let cap_start = run_start;
            if run_end > cap_start + COLD_RUN_MAX_ROWS {
                run_end = cap_start + COLD_RUN_MAX_ROWS;
                bottom_of_end = strides
                    .iter()
                    .take(run_end)
                    .map(|stride| stride.max(0.0))
                    .sum();
            }
        }

        // Nothing hit the band: the scroll offset is past everything the strides
        // account for, because rows the window has yet to mount are still lower
        // bounds. Fall back to the trailing run rather than a single row, so the
        // viewport stays filled while the remaining heights are learned.
        if run_start >= run_end {
            run_end = n;
            bottom_of_end = total;
            run_start = n - 1;
            top_of_start = total - strides[n - 1].max(0.0);
            let floor = (total - viewport_height - overdraw).max(0.0);
            while run_start > 0 && top_of_start > floor {
                run_start -= 1;
                top_of_start -= strides[run_start].max(0.0);
            }
        }

        // Keep the focused row mounted; GPUI blurs an unmounted caret. It goes on
        // its own island rather than widening the run, so a caret left behind
        // while reading does not drag every row between it and the viewport on
        // screen with it.
        let mut top_h = top_of_start;
        let mut bottom_h = total - bottom_of_end;
        let mut focus_island = None;
        if let Some(focus_row) = focus_row.map(|row| row.min(n - 1)) {
            let focus_top: f32 = strides[..focus_row].iter().map(|s| s.max(0.0)).sum();
            let focus_bottom = focus_top + strides[focus_row].max(0.0);
            if focus_row < run_start {
                focus_island = Some(FocusIsland {
                    row: focus_row,
                    lead_h: focus_top,
                });
                top_h = top_of_start - focus_bottom;
            } else if focus_row >= run_end {
                focus_island = Some(FocusIsland {
                    row: focus_row,
                    lead_h: focus_top - bottom_of_end,
                });
                bottom_h = total - focus_bottom;
            }
        }

        RenderWindow {
            run_start,
            run_end,
            top_h: top_h.max(0.0),
            bottom_h: bottom_h.max(0.0),
            focus_island: focus_island.map(|island| FocusIsland {
                lead_h: island.lead_h.max(0.0),
                ..island
            }),
        }
    }

    /// Linearly interpolates the editor content width ratio based on viewport
    /// width. The column stays full-width until `centered_shrink_start`, then
    /// shrinks to `centered_min_ratio` at `centered_shrink_end`.
    pub(super) fn centered_column_ratio(
        viewport_width: f32,
        dimensions: &crate::theme::ThemeDimensions,
    ) -> f32 {
        if viewport_width <= dimensions.centered_shrink_start {
            return 1.0;
        }

        let t = ((viewport_width - dimensions.centered_shrink_start)
            / (dimensions.centered_shrink_end - dimensions.centered_shrink_start))
            .clamp(0.0, 1.0);
        1.0 - t * (1.0 - dimensions.centered_min_ratio)
    }

    pub(crate) fn centered_column_width(
        viewport_width: f32,
        dimensions: &crate::theme::ThemeDimensions,
    ) -> f32 {
        let available_content_width = (viewport_width - dimensions.editor_padding * 2.0).max(1.0);
        let centered_ratio = Self::centered_column_ratio(viewport_width, dimensions);
        (available_content_width * centered_ratio)
            .max(320.0)
            .min(available_content_width)
    }

    /// Builds the OS window title, including the dirty marker when the
    /// document has unsaved changes.
    pub(super) fn window_title(
        file_path: Option<&Path>,
        recovery_source_path: Option<&Path>,
        is_recovered_document: bool,
        is_dirty: bool,
        strings: &crate::i18n::I18nStrings,
    ) -> String {
        let base_title = if is_recovered_document {
            let source_name = recovery_source_path
                .and_then(|path| path.file_name())
                .map(|name| format!(" ({})", name.to_string_lossy()))
                .unwrap_or_default();
            format!("Velora - {}{source_name}", strings.recovered_document_title)
        } else if let Some(path) = file_path {
            format!(
                "Velora - {}",
                path.file_name().map_or_else(
                    || path.to_string_lossy().to_string(),
                    |name| name.to_string_lossy().to_string()
                )
            )
        } else {
            "Velora".to_string()
        };

        if is_dirty && !strings.dirty_title_marker.is_empty() {
            format!("{} {}", strings.dirty_title_marker, base_title)
        } else {
            base_title
        }
    }

    pub(crate) fn on_toggle_view_mode_action(
        &mut self,
        _: &crate::components::ToggleViewMode,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_view_mode_from_ui(cx);
    }

    pub(crate) fn toggle_view_mode_from_ui(&mut self, cx: &mut Context<Self>) {
        if self.code_tab_active() {
            return;
        }
        self.end_block_pointer_selection_sessions(cx);
        self.last_selection_snapshot = self.capture_source_selection_snapshot(cx);
        self.toggle_view_mode(cx);
    }

    pub(crate) fn toggle_focus_mode(&mut self, cx: &mut Context<Self>) {
        self.focus_mode = !self.focus_mode;
        cx.notify();
    }

    pub(crate) fn toggle_typewriter_mode(&mut self, cx: &mut Context<Self>) {
        self.typewriter_mode = !self.typewriter_mode;
        self.pending_scroll_active_block_into_view = true;
        self.pending_scroll_recheck_after_layout = true;
        cx.notify();
    }

    pub(crate) fn on_undo(
        &mut self,
        _: &crate::components::Undo,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.undo_document(cx);
    }

    pub(crate) fn on_redo(
        &mut self,
        _: &crate::components::Redo,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.redo_document(cx);
    }

    pub(crate) fn on_save_document(
        &mut self,
        _: &crate::components::SaveDocument,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.request_save_document(cx);
    }

    pub(crate) fn on_save_document_as(
        &mut self,
        _: &crate::components::SaveDocumentAs,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.request_save_document_as(cx);
    }

    pub(crate) fn on_export_html(
        &mut self,
        _: &crate::components::ExportHtml,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.export_document_via_prompt(crate::export::ExportFormat::Html, window, cx);
    }

    pub(crate) fn on_export_pdf(
        &mut self,
        _: &crate::components::ExportPdf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.export_document_via_prompt(crate::export::ExportFormat::Pdf, window, cx);
    }

    pub(crate) fn on_export_png(
        &mut self,
        _: &crate::components::ExportPng,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.export_document_via_prompt(crate::export::ExportFormat::Png, window, cx);
    }

    pub(crate) fn on_quit_application(
        &mut self,
        _: &crate::components::QuitApplication,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        crate::app_menu::request_quit_application(cx);
    }

    pub(crate) fn on_close_window(
        &mut self,
        _: &crate::components::CloseWindow,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.request_close_current_window(window, cx);
    }

    pub(crate) fn on_install_cli_tool(
        &mut self,
        _: &crate::components::InstallCliTool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        crate::app_menu::install_cli_tool(cx);
    }

    pub(crate) fn on_uninstall_cli_tool(
        &mut self,
        _: &crate::components::UninstallCliTool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        crate::app_menu::uninstall_cli_tool(cx);
    }

    pub(crate) fn toggle_view_mode(&mut self, cx: &mut Context<Self>) {
        self.end_block_pointer_selection_sessions(cx);
        let selection_snapshot = self.capture_source_selection_snapshot(cx);
        self.clear_cross_block_selection(cx);
        self.rendered_select_all_cycle = None;
        match self.view_mode {
            ViewMode::Rendered => {
                let markdown = self.document.markdown_text(cx);
                let block = Self::new_block(cx, BlockRecord::paragraph(markdown));
                block.update(cx, |block, _cx| block.set_source_document_mode());
                self.document.replace_roots(vec![block], cx);
                self.view_mode = ViewMode::Source;
                self.table_cells.clear();
            }
            ViewMode::Source => {
                let source = self.document.raw_source_text(cx);
                self.source_mode_fallback_required =
                    Self::markdown_requires_source_mode_fallback(&source);
                if self.source_mode_fallback_required {
                    cx.notify();
                    return;
                }
                let mut roots = Self::build_root_blocks_from_markdown(cx, &source);
                if roots.is_empty() {
                    roots.push(Self::new_block(cx, BlockRecord::paragraph(String::new())));
                }
                self.document.replace_roots(roots, cx);
                self.view_mode = ViewMode::Rendered;
                self.source_mode_fallback_required = false;
                self.rebuild_table_runtimes(cx);
                self.rebuild_image_runtimes(cx);
            }
        }

        self.apply_selection_snapshot_in_current_mode(&selection_snapshot, cx);
        self.pending_scroll_active_block_into_view = true;
        self.pending_scroll_recheck_after_layout = true;
        self.last_scroll_viewport_size = None;
        self.pending_window_title_refresh = true;
        self.close_dialog_restore_focus = None;
        self.table_axis_preview = None;
        self.table_axis_selection = None;
        self.dismiss_contextual_overlays(cx);
        self.sync_table_axis_visuals(cx);
        self.refresh_stable_document_snapshot(cx);
        cx.notify();
    }

    /// Marks the document dirty and schedules window-title and edited-state
    /// refresh for the next render frame.
    pub(super) fn mark_dirty(&mut self, cx: &mut Context<Self>) {
        self.document_revision = self.document_revision.wrapping_add(1);
        if !self.document_dirty {
            self.document_dirty = true;
            self.pending_window_edited = true;
            self.pending_window_unedited = false;
            self.pending_window_title_refresh = true;
            cx.notify();
        }
        self.refresh_source_line_starts(cx);
        self.schedule_autosave(cx);
        self.refresh_document_find_after_edit(cx);
    }

    /// 源码分块文档的行号续号：文本/结构变化后重算每块的首行行号
    /// （分块见 `build_source_document_roots`；渲染模式文档没有行号槽）。
    fn refresh_source_line_starts(&mut self, cx: &mut Context<Self>) {
        if !(self.code_document || self.source_mode_fallback_required) {
            return;
        }
        let mut next_line = 1usize;
        for visible in self.document.flatten_visible_blocks() {
            next_line = visible.entity.update(cx, |block, _cx| {
                block.set_source_line_start(next_line);
                next_line + block.display_text().split('\n').count()
            });
        }
    }

    pub(super) fn request_active_block_scroll_into_view(&mut self, cx: &mut Context<Self>) {
        self.pending_scroll_recheck_after_layout = true;
        if !self.pending_scroll_active_block_into_view {
            self.pending_scroll_active_block_into_view = true;
            cx.notify();
        }
    }

    pub(super) fn viewport_size_changed(previous: Size<Pixels>, current: Size<Pixels>) -> bool {
        const EPSILON: f32 = 0.5;

        (f32::from(previous.width) - f32::from(current.width)).abs() > EPSILON
            || (f32::from(previous.height) - f32::from(current.height)).abs() > EPSILON
    }

    pub(crate) fn show_info_dialog(&mut self, kind: InfoDialogKind, cx: &mut Context<Self>) {
        if self.show_unsaved_changes_dialog {
            return;
        }

        self.menu_bar_open = None;
        self.menu_submenu_open = None;
        self.menu_submenu_panel_hovered = false;
        self.menu_submenu_bridge_hovered = false;
        self.info_dialog = Some(kind);
        cx.notify();
    }

    pub(crate) fn hide_info_dialog(&mut self, cx: &mut Context<Self>) {
        if self.info_dialog.take().is_some() {
            cx.notify();
        }
    }

    pub(crate) fn open_menu_bar(&mut self, index: usize, cx: &mut Context<Self>) {
        self.menu_close_task = None;
        if self.menu_bar_open != Some(index) {
            self.menu_bar_open = Some(index);
            self.menu_submenu_open = None;
            self.menu_submenu_panel_hovered = false;
            self.menu_submenu_bridge_hovered = false;
            cx.notify();
        }
    }

    pub(crate) fn open_menu_submenu(&mut self, index: usize, cx: &mut Context<Self>) {
        self.menu_close_task = None;
        if self.menu_submenu_open != Some(index) {
            self.menu_submenu_open = Some(index);
            cx.notify();
        }
    }

    pub(crate) fn close_menu_submenu(&mut self, cx: &mut Context<Self>) {
        let had_open_submenu = self.menu_submenu_open.take().is_some();
        let had_submenu_hover = self.menu_submenu_panel_hovered || self.menu_submenu_bridge_hovered;
        self.menu_submenu_panel_hovered = false;
        self.menu_submenu_bridge_hovered = false;
        if had_open_submenu || had_submenu_hover {
            cx.notify();
        }
    }

    pub(super) fn schedule_menu_bar_close(&mut self, cx: &mut Context<Self>) {
        if self.menu_bar_open.is_none() {
            return;
        }

        let weak_editor = cx.entity().downgrade();
        self.menu_close_task = Some(cx.spawn(
            async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
                cx.background_executor()
                    .timer(Duration::from_millis(120))
                    .await;
                let _ = weak_editor.update(cx, |editor, cx| {
                    editor.menu_close_task = None;
                    if !editor.menu_bar_hovered
                        && !editor.menu_panel_hovered
                        && !editor.menu_submenu_panel_hovered
                        && !editor.menu_submenu_bridge_hovered
                    {
                        editor.close_menu_bar(cx);
                    }
                });
            },
        ));
    }

    pub(crate) fn set_menu_bar_hovered(&mut self, hovered: bool, cx: &mut Context<Self>) {
        self.menu_bar_hovered = hovered;
        if hovered {
            self.menu_close_task = None;
        } else if !self.menu_panel_hovered
            && !self.menu_submenu_panel_hovered
            && !self.menu_submenu_bridge_hovered
        {
            self.schedule_menu_bar_close(cx);
        }
    }

    pub(crate) fn set_menu_panel_hovered(&mut self, hovered: bool, cx: &mut Context<Self>) {
        self.menu_panel_hovered = hovered;
        if hovered {
            self.menu_close_task = None;
        } else if !self.menu_bar_hovered
            && !self.menu_submenu_panel_hovered
            && !self.menu_submenu_bridge_hovered
        {
            self.schedule_menu_bar_close(cx);
        }
    }

    pub(crate) fn set_menu_submenu_panel_hovered(&mut self, hovered: bool, cx: &mut Context<Self>) {
        self.menu_submenu_panel_hovered = hovered;
        if hovered {
            self.menu_close_task = None;
        } else if !self.menu_bar_hovered
            && !self.menu_panel_hovered
            && !self.menu_submenu_bridge_hovered
        {
            self.schedule_menu_bar_close(cx);
        }
    }

    /// Hover handler for the invisible gap bridge. The bridge and the submenu
    /// panel overlap, so the cursor crossing between them fires a `false` for
    /// one region and a `true` for the other in the same gesture. Keeping their
    /// hover state in separate flags lets either one hold the menu open
    /// regardless of the order those events arrive.
    pub(crate) fn set_menu_submenu_bridge_hovered(
        &mut self,
        hovered: bool,
        cx: &mut Context<Self>,
    ) {
        self.menu_submenu_bridge_hovered = hovered;
        if hovered {
            self.menu_close_task = None;
        } else if !self.menu_bar_hovered
            && !self.menu_panel_hovered
            && !self.menu_submenu_panel_hovered
        {
            self.schedule_menu_bar_close(cx);
        }
    }

    pub(crate) fn dismiss_menu_bar_from_body(&mut self, cx: &mut Context<Self>) {
        if self.menu_bar_open.is_some() {
            self.close_menu_bar(cx);
        }
    }

    pub(crate) fn request_save_document(&mut self, cx: &mut Context<Self>) {
        if !self.pending_save {
            self.pending_save = true;
            cx.notify();
        }
    }

    pub(crate) fn request_save_document_as(&mut self, cx: &mut Context<Self>) {
        if !self.pending_save_as {
            self.pending_save_as = true;
            cx.notify();
        }
    }

    /// 链接跳转要 `&mut Window`，且不能在本次窗口更新里重入，因此与 wikilink
    /// 一样延后到当前更新结束后执行。
    pub(crate) fn defer_open_link(&mut self, open_target: String, cx: &mut Context<Self>) {
        let Some(any_handle) = self.window_handle else {
            return;
        };
        let Some(handle) = any_handle.downcast::<Editor>() else {
            return;
        };
        cx.defer(move |cx| {
            let _ = handle.update(cx, |editor, window, cx| {
                editor.open_link_target(open_target.clone(), window, cx);
            });
        });
    }

    /// Cmd/Ctrl+点击链接：直接跳转，不弹确认框（用户要求：全软件不用系统原生弹窗）。
    /// 外部协议（http/https/mailto/tel/ftp）交默认浏览器；本地文档在应用内打开；
    /// `#锚点` 在当前文档内跳到对应标题。
    pub(crate) fn open_link_target(
        &mut self,
        open_target: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match classify_link_target(&open_target) {
            LinkTarget::External(url) => cx.open_url(&url),
            LinkTarget::Anchor(anchor) => self.jump_to_heading_anchor(&anchor, cx),
            LinkTarget::Local { path, anchor } => {
                let resolved = resolve_local_link_path(self.file_path.as_deref(), &path);
                if !resolved.exists() {
                    // 目标不存在：什么都不做。既不弹系统确认框，也不把当前文档
                    // 换成「无法预览」占位（那会改掉用户正在看的东西）。
                    return;
                }
                if let Some(anchor) = anchor
                    && self.file_path.as_deref() == Some(resolved.as_path())
                {
                    self.jump_to_heading_anchor(&anchor, cx);
                    return;
                }
                self.open_workspace_file(resolved, window, cx);
            }
        }
    }

    fn jump_to_heading_anchor(&mut self, anchor: &str, cx: &mut Context<Self>) {
        let source = self.last_stable_source_text.clone();
        if let Some(line) = heading_line_for_anchor(&source, anchor) {
            self.jump_to_source_line(line, cx);
        }
    }

    pub(crate) fn close_menu_bar(&mut self, cx: &mut Context<Self>) {
        let had_open_menu = self.menu_bar_open.take().is_some();
        let had_open_submenu = self.menu_submenu_open.take().is_some();
        let had_hover_state = self.menu_bar_hovered
            || self.menu_panel_hovered
            || self.menu_submenu_panel_hovered
            || self.menu_submenu_bridge_hovered;
        let had_pending_close = self.menu_close_task.take().is_some();
        self.menu_bar_hovered = false;
        self.menu_panel_hovered = false;
        self.menu_submenu_panel_hovered = false;
        self.menu_submenu_bridge_hovered = false;
        if had_open_menu || had_open_submenu || had_hover_state || had_pending_close {
            cx.notify();
        }
    }
}

/// 链接目标的分类结果（`Cmd/Ctrl+点击` 直接跳转，不弹确认框）。
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum LinkTarget {
    /// 交默认浏览器/系统处理器打开的外部协议。
    External(String),
    /// 当前文档内的 `#锚点`。
    Anchor(String),
    /// 本地文档路径（绝对或相对当前文档/工作区），可带 `#锚点`。
    Local { path: String, anchor: Option<String> },
}

const EXTERNAL_LINK_SCHEMES: [&str; 5] = ["http:", "https:", "mailto:", "tel:", "ftp:"];

/// 把 Markdown 链接目标分成外部链接 / 文内锚点 / 本地路径。
pub(crate) fn classify_link_target(target: &str) -> LinkTarget {
    let trimmed = target.trim();
    if let Some(anchor) = trimmed.strip_prefix('#') {
        return LinkTarget::Anchor(percent_decode(anchor));
    }
    let lowered = trimmed.to_ascii_lowercase();
    if EXTERNAL_LINK_SCHEMES
        .iter()
        .any(|scheme| lowered.starts_with(scheme))
    {
        return LinkTarget::External(trimmed.to_string());
    }
    let raw = lowered
        .strip_prefix("file:")
        .map(|_| trimmed[5..].trim_start_matches('/').to_string())
        .unwrap_or_else(|| trimmed.to_string());
    let (path_part, anchor) = split_link_anchor(&raw);
    // Windows 的 `C:/x.md` 被 strip 掉斜杠后会丢掉盘符冒号后的分隔，这里补回。
    let path = percent_decode(path_part);
    let path = match lowered.strip_prefix("file:") {
        Some(_) if path.len() > 2 && path.as_bytes()[1] == b':' => format!("/{path}"),
        _ => path,
    };
    LinkTarget::Local {
        path,
        anchor: anchor.map(percent_decode),
    }
}

fn split_link_anchor(target: &str) -> (&str, Option<&str>) {
    match target.find('#') {
        Some(index) => (&target[..index], Some(&target[index + 1..])),
        None => (target, None),
    }
}

/// 只做链接里常见的百分号转义（`My%20File.md` → `My File.md`）。按**字节**还原，
/// 再整体按 UTF-8 解码，否则中文锚点（`%E6%A0%87%E9%A2%98`）会被拆成半个字符。
fn percent_decode(text: &str) -> String {
    if !text.contains('%') {
        return text.to_string();
    }
    let bytes = text.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (
                (bytes[index + 1] as char).to_digit(16),
                (bytes[index + 2] as char).to_digit(16),
            )
        {
            out.push((hi * 16 + lo) as u8);
            index += 3;
            continue;
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8(out).unwrap_or_else(|err| String::from_utf8_lossy(err.as_bytes()).into_owned())
}

/// 本地链接的落点：绝对路径直接用，相对路径按当前文档目录（无文档时按工作区根）解析。
pub(crate) fn resolve_local_link_path(document_path: Option<&Path>, path: &str) -> PathBuf {
    let candidate = PathBuf::from(path);
    if candidate.is_absolute() {
        return candidate;
    }
    let base = document_path
        .and_then(|path| path.parent())
        .map(|dir| dir.to_path_buf());
    base.unwrap_or_else(|| PathBuf::from("."))
        .join(candidate)
}

/// 按标题文本找它在源文本里的行号（GitHub 风格锚点的宽松匹配：忽略大小写、
/// 空白与标点，保留 `-`/`_` 与 CJK）。
pub(crate) fn heading_line_for_anchor(source: &str, anchor: &str) -> Option<usize> {
    let needle = heading_slug(anchor)?;
    source.lines().enumerate().find_map(|(index, line)| {
        let text = line.trim_start();
        let hashes = text.len() - text.trim_start_matches('#').len();
        if hashes == 0 || hashes > 6 {
            return None;
        }
        (heading_slug(text[hashes..].trim())? == needle).then_some(index)
    })
}

fn heading_slug(text: &str) -> Option<String> {
    // GitHub 风格：小写、空白转 `-`、丢掉其它标点（CJK 直接保留）。
    let slug: String = text
        .trim()
        .to_lowercase()
        .chars()
        .filter_map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                Some(c)
            } else if c.is_whitespace() {
                Some('-')
            } else {
                None
            }
        })
        .collect();
    (!slug.is_empty()).then_some(slug)
}

#[cfg(test)]
mod tests {
    use super::{LinkTarget, classify_link_target, heading_line_for_anchor, resolve_local_link_path};
    use crate::editor::Editor;
    use gpui::TestAppContext;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn link_targets_are_classified_without_asking_the_user() {
        assert_eq!(
            classify_link_target("https://example.com/a?b=1"),
            LinkTarget::External("https://example.com/a?b=1".to_string())
        );
        assert_eq!(
            classify_link_target("mailto:someone@example.com"),
            LinkTarget::External("mailto:someone@example.com".to_string())
        );
        assert_eq!(
            classify_link_target("#设计与来源"),
            LinkTarget::Anchor("设计与来源".to_string())
        );
        assert_eq!(
            classify_link_target("docs/plans/2026-09-24-design.md"),
            LinkTarget::Local {
                path: "docs/plans/2026-09-24-design.md".to_string(),
                anchor: None,
            }
        );
        // 本地路径可以带锚点，百分号转义要还原。
        assert_eq!(
            classify_link_target("./My%20Notes.md#%E6%A0%87%E9%A2%98"),
            LinkTarget::Local {
                path: "./My Notes.md".to_string(),
                anchor: Some("标题".to_string()),
            }
        );
        assert_eq!(
            classify_link_target("/abs/path/other.md"),
            LinkTarget::Local {
                path: "/abs/path/other.md".to_string(),
                anchor: None,
            }
        );
    }

    #[test]
    fn relative_links_resolve_against_the_current_document() {
        let document = PathBuf::from("/work/notes/index.md");
        assert_eq!(
            resolve_local_link_path(Some(&document), "docs/plans/x.md"),
            PathBuf::from("/work/notes/docs/plans/x.md")
        );
        assert_eq!(
            resolve_local_link_path(Some(&document), "/abs/x.md"),
            PathBuf::from("/abs/x.md")
        );
    }

    #[test]
    fn anchors_match_headings_loosely() {
        let source = "# 设计与来源\n\n- 正文\n\n## Math style (extension)\n";
        assert_eq!(heading_line_for_anchor(source, "设计与来源"), Some(0));
        assert_eq!(
            heading_line_for_anchor(source, "math-style-extension"),
            Some(4)
        );
        assert_eq!(heading_line_for_anchor(source, "不存在的标题"), None);
    }

    #[gpui::test]
    async fn external_links_go_to_the_default_browser(cx: &mut TestAppContext) {
        init_app(cx);
        let (editor, cx) = cx.add_window_view(|_, cx| {
            Editor::from_markdown(cx, "看 [官网](https://example.com) 吧\n".into(), None)
        });
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.open_link_target("https://example.com".to_string(), window, cx);
            });
        });
        assert_eq!(
            cx.opened_url().as_deref(),
            Some("https://example.com"),
            "网页链接应直接交给默认浏览器"
        );
    }

    #[gpui::test]
    async fn local_document_links_open_inside_the_app(cx: &mut TestAppContext) {
        init_app(cx);
        let root = std::env::temp_dir().join(format!("velora-link-open-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("docs")).unwrap();
        let current = root.join("index.md");
        let target = root.join("docs").join("target.md");
        fs::write(&current, "# 首页\n\nsee [target](docs/target.md)\n").unwrap();
        fs::write(&target, "# 目标文档\n").unwrap();
        cx.on_quit({
            let root = root.clone();
            move || {
                let _ = fs::remove_dir_all(root);
            }
        });

        let (editor, cx) = cx.add_window_view(|_, cx| {
            Editor::from_markdown(cx, format!("# 首页\n\nsee [target](docs/target.md)\n"), Some(current))
        });
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.open_link_target("docs/target.md".to_string(), window, cx);
            });
        });
        cx.run_until_parked();

        editor.read_with(cx, |editor, _| {
            assert_eq!(
                editor.file_path.as_deref(),
                Some(target.as_path()),
                "本地文档链接应在应用内打开"
            );
            assert!(editor.unsupported_preview_path.is_none());
        });
        assert!(
            cx.opened_url().is_none(),
            "本地文档不该交给浏览器，实测 {:?}",
            cx.opened_url()
        );
    }

    #[gpui::test]
    async fn missing_local_link_changes_nothing(cx: &mut TestAppContext) {
        init_app(cx);
        let root = std::env::temp_dir().join(format!("velora-link-miss-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let current = root.join("index.md");
        fs::write(&current, "# 首页\n").unwrap();
        cx.on_quit({
            let root = root.clone();
            move || {
                let _ = fs::remove_dir_all(root);
            }
        });

        let (editor, cx) =
            cx.add_window_view(|_, cx| Editor::from_markdown(cx, "# 首页\n".into(), Some(current)));
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.open_link_target("gone.md".to_string(), window, cx);
            });
        });
        editor.read_with(cx, |editor, _| {
            assert_eq!(
                editor.file_path.as_deref(),
                Some(root.join("index.md").as_path()),
                "点开到不存在的目标不该改变当前文档"
            );
            assert!(editor.unsupported_preview_path.is_none());
        });
        assert!(cx.opened_url().is_none(), "缺失的本地路径不该丢给浏览器");
    }

    fn init_app(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
        });
    }
}
