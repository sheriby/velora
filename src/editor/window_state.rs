//! Window-level editor state such as scrolling, mode switching, and menus.
//! 窗口标题显示 velora。

use super::*;

/// P4b：stride 大多未知时的单帧最大预挂载行数。上限只削视口上下的余量，
/// 视口自身必须始终有挂载行覆盖。
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
    /// lands on a spacer. When heights are still estimates the cold-start cap
    /// trims only the margin around the viewport: the viewport keeps its mounted
    /// rows, and when covering it needs more rows than the cap allows,
    /// `needs_fill` asks the caller for another frame instead of leaving spacer
    /// on screen. Pure, so it is unit-tested headlessly.
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
                needs_fill: false,
            };
        }

        let band_top = scroll_y - overdraw;
        let band_bottom = scroll_y + viewport_height + overdraw;
        let viewport_bottom = scroll_y + viewport_height;

        let mut run_start = n;
        let mut run_end = 0usize;
        let mut top_of_start = 0.0f32;
        let mut bottom_of_end = 0.0f32;
        // 视口自身落到的行区间。冷启动上限只许削掉这之外的预挂载行。
        let mut viewport_first = n;
        let mut viewport_last = 0usize;
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
            if viewport_first == n && bottom >= scroll_y {
                viewport_first = index;
            }
            if top <= viewport_bottom {
                viewport_last = index + 1;
            }
            cursor = bottom;
        }
        let total = cursor;

        // P4b 冷启动保护：绝大多数 stride 还是估计值时，行高被严重低估
        // （一行真实 9000px 估计 16px），带状扫描会一口气挂载几十个巨行。
        // 上限只约束预挂载：run 起点最多高出视口首行 COLD_RUN_MAX_ROWS 行，
        // 终点先按估计值铺到视口底部。估计值是行高的下界，铺满估计值即铺满
        // 视口；行高被低估、预算内铺不满时置 needs_fill，由调用方续帧补齐，
        // 绝不把视口留在 spacer 上。
        let known = strides.iter().filter(|&&stride| stride > estimate).count();
        let mut needs_fill = false;
        if known * 2 < n && viewport_first < n {
            let lead_floor = viewport_first.saturating_sub(COLD_RUN_MAX_ROWS);
            if run_start < lead_floor {
                run_start = lead_floor;
                top_of_start = strides[..lead_floor]
                    .iter()
                    .map(|stride| stride.max(0.0))
                    .sum();
            }
            let cover_end = viewport_first
                .saturating_add(COLD_RUN_MAX_ROWS * 2)
                .min(viewport_last);
            let capped_end = cover_end.max(viewport_first + 1).min(run_end);
            if capped_end < run_end {
                run_end = capped_end;
                bottom_of_end = strides
                    .iter()
                    .take(run_end)
                    .map(|stride| stride.max(0.0))
                    .sum();
            }
            needs_fill = run_end < viewport_last;
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
            needs_fill,
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

    /// 记下这一篇文档的阅读现场：模式、视口偏移、光标区间。
    ///
    /// 只在「马上要换掉整棵块树、但文档还是这篇或要切回另一篇」的入口用；
    /// 现场要在换之前取，块树一换旧实体 id 就作废了。
    pub(crate) fn capture_document_view(&self, cx: &App) -> DocumentView {
        DocumentView {
            view_mode: self.view_mode,
            scroll_y: f32::from(self.scroll_handle.offset().y),
            selection: self.capture_source_selection_snapshot(cx),
        }
    }

    pub(crate) fn toggle_view_mode(&mut self, cx: &mut Context<Self>) {
        self.end_block_pointer_selection_sessions(cx);
        let selection_snapshot = self.capture_source_selection_snapshot(cx);
        self.clear_cross_block_selection(cx);
        self.rendered_select_all_cycle = None;
        match self.view_mode {
            ViewMode::Rendered => {
                self.view_mode = ViewMode::Source;
                self.table_cells.clear();
            }
            ViewMode::Source => {
                self.source_mode_fallback_required =
                    Self::markdown_requires_source_mode_fallback(&self.buffer.text());
                if self.source_mode_fallback_required {
                    cx.notify();
                    return;
                }
                self.view_mode = ViewMode::Rendered;
                self.source_mode_fallback_required = false;
            }
        }
        // 两种视图都是缓冲区的一份投影：换视图 = 换投影，文本与区间都从缓冲区来。
        self.rebuild_document_from_buffer(cx);

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
        // 模式切换会换掉整套块实体，文档内搜索高亮必须重算，否则正文里的
        // 命中全丢（用户报修）。非查找场景下该函数自行早退/清理。
        if self.workspace.is_open
            && self.workspace.active_tab == super::workspace::WorkspaceTab::Search
            && !self.workspace.search_query.trim().is_empty()
        {
            self.sync_document_search_highlights(cx);
        }
        cx.notify();
    }

    /// Marks the document dirty and schedules window-title and edited-state
    /// refresh for the next render frame.
    ///
    /// 这条是给**没声明区间**的改动用的（表格、跨块选区、源码模式……）：块树变了，
    /// 缓冲区只能整篇重投影才能跟上，未编辑块的原始字节就此丢失。想保住原文的改动
    /// 路径请走 [`Self::mark_dirty_written_back`]。
    pub(super) fn mark_dirty(&mut self, cx: &mut Context<Self>) {
        self.finish_dirty(cx);
    }

    /// 改动已经按区间写回缓冲区：不重投影，所以别的块一个字节都不会被改写。
    pub(crate) fn mark_dirty_written_back(&mut self, cx: &mut Context<Self>) {
        self.skip_next_resync = true;
        self.finish_dirty(cx);
    }

    fn finish_dirty(&mut self, cx: &mut Context<Self>) {
        // 改动没自己声明区间时，这里是唯一的落笔处：把块树的序列化刷进缓冲区，
        // 搜索高亮、大纲、跨块选区恢复都按缓冲区坐标算位置，少刷一次就会拿旧文本
        // 去映射新块树。
        self.resync_buffer_from_projection(cx);
        self.document_revision = self.document_revision.wrapping_add(1);
        if !self.document_dirty {
            self.document_dirty = true;
            // 临时（预览）标签的转正点：用户既然动过它，这篇就不再是「点开看看」。
            // 也是在这里转固定，切走时它才不会作为可替换的预览被销毁。
            self.pin_active_preview_tab();
            self.pending_window_edited = true;
            self.pending_window_unedited = false;
            self.pending_window_title_refresh = true;
            cx.notify();
        }
        self.refresh_source_line_starts(cx);
        self.schedule_autosave(cx);
        self.refresh_document_find_after_edit(cx);
    }

    /// 源码分块文档的行号续号：先收集各块区间起点，再用缓冲区的批量换算
    /// `lines_and_line_starts` 一趟 O(全文) 算出所有行号。逐块调 `line_of` 是
    /// O(根块数 × chunk 数) 的平方项（代码文档按 512 行切片，10 MiB 就是百万步级）；
    /// 区间起点天然升序，正是批量接口要的形状。
    fn refresh_source_line_starts(&mut self, cx: &mut Context<Self>) {
        if !(self.code_document || self.source_mode_fallback_required) {
            return;
        }
        let total = self.buffer.byte_len();
        let blocks = self.document.root_blocks().to_vec();
        let starts: Vec<Option<usize>> = blocks
            .iter()
            .map(|block| self.document.source_span_of(block.entity_id()).map(|span| span.start))
            .collect();
        // 没挂区间的块沿用前一块的续号（旧行为），批量接口只吃有区间的。
        let mut known: Vec<(usize, usize)> = Vec::new(); // (序号, 字节偏移)
        let mut cursor = 0usize;
        for (index, start) in starts.iter().enumerate() {
            let start = (*start).unwrap_or(cursor).min(total);
            known.push((index, start));
            let length = blocks[index].read(cx).display_text().len();
            cursor = (start + length + 1).min(total);
        }
        let lines = self.buffer.lines_and_line_starts(
            &known.iter().map(|(_, offset)| *offset).collect::<Vec<_>>(),
        );
        for ((index, _), (line_index, _)) in known.iter().zip(&lines) {
            blocks[*index].update(cx, |block, _cx| block.set_source_line_start(line_index + 1));
        }
        // 行号栏宽度基准跟着总行数走：编辑增删行后各块同步更新，栏宽保持
        // 全文档一致（用户报修：512 上下行号对不齐）。
        let basis = self.buffer.line_count();
        for block in &blocks {
            block.update(cx, |block, _cx| block.set_source_line_gutter_basis(basis));
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

    /// 点标题栏左侧的汉堡按钮：开/关一级菜单列表（Windows）。开、关都把子面板状态
    /// 清干净，免得下次打开时旧的面板先闪一下。
    pub(crate) fn toggle_hamburger_menu(&mut self, cx: &mut Context<Self>) {
        self.menu_close_task = None;
        let opening = !self.hamburger_menu_open;
        self.hamburger_menu_open = opening;
        self.menu_bar_open = None;
        self.menu_submenu_open = None;
        self.menu_submenu_panel_hovered = false;
        self.menu_submenu_bridge_hovered = false;
        cx.notify();
    }

    /// 鼠标划过汉堡列表里的某一项：列表保持打开，同时把它的条目面板打开。
    pub(crate) fn open_hamburger_menu_item(&mut self, index: usize, cx: &mut Context<Self>) {
        self.hamburger_menu_open = true;
        self.open_menu_bar(index, cx);
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
        if self.menu_bar_open.is_some() || self.hamburger_menu_open {
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

    /// 显式「格式化文档」：把整篇按**模型的规范化写法**重新落一遍字节。
    ///
    /// 规范化（Setext→ATX、`__粗__`→`**粗**`、`1)`→`1.`、表格列宽重排）只允许出现在
    /// 这一条命令里。打开、打字、保存、撤销走的都是「按区间落笔」，用户没碰过的写法一个
    /// 字节都不动——那是缓冲区当事实源换来的性质，混进隐式路径就全废了。
    ///
    /// 一次格式化是一条**可撤销**的编辑组：落笔仍走最小差异，撤销把原字节逐段放回去
    /// （而不是「再规范化一次」）。已经规范化到位的那一次什么都不做：不动字节、不标脏、
    /// 不留空撤销组。
    pub(crate) fn format_document(&mut self, cx: &mut Context<Self>) {
        // 源码/代码视图没有「模型的写法」可言：那里的块就是文件本身。
        if self.view_mode != ViewMode::Rendered {
            return;
        }
        self.flush_pending_materialization(cx);
        // 先把「用户自己选的记号」这份数据清成默认（`__`→`**`、`1)`→`1.`），再序列化。
        // 不清的话拿不到规范化结果——那些记号正是保真那批提交特意存进模型的。
        self.document.canonicalize_writing_style(cx);
        let started = std::time::Instant::now();
        let (serialized, _) = self.document.markdown_text_with_block_spans(cx);
        // 这一次整篇落笔是命令自己付的账，不在按键路径上（闸门量的是后者），
        // 但计数器必须数得到——漏一档就等于给隐形成本开门。
        self.source_serializations
            .set(self.source_serializations.get() + 1);
        self.source_serialization_nanos.set(
            self.source_serialization_nanos.get()
                + started.elapsed().as_nanos().min(u64::MAX as u128) as u64,
        );
        let text = self.resynced_text(&serialized);
        if self.buffer.matches_text(text.as_ref()) {
            return;
        }
        let selection = self.capture_source_selection_snapshot(cx);
        self.prepare_undo_capture(
            crate::components::UndoCaptureKind::NonCoalescible,
            cx,
        );
        // `apply_resynced_text` 会把文件原来那个末行换行补回来，并且只写真正的差异。
        self.apply_resynced_text(&text);
        // 缓冲区已经是目标状态：别让紧随其后的重同步再把整篇序列化一遍。
        self.skip_next_resync = true;
        self.finish_dirty(cx);
        // 从缓冲区重建投影：区间、写法数据（现在按规范文本重新量）、表格与图片运行时
        // 都跟着这份新文本走，模型与文件才是同一份文档。
        self.rebuild_document_from_buffer(cx);
        self.apply_selection_snapshot_in_current_mode(&selection, cx);
        self.finalize_pending_undo_capture(cx);
        cx.notify();
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
                // 正文里点链接是浏览行为：开预览标签，连着点几个不堆标签栏
                // （与搜索结果、⌘P 同口径）。
                self.open_workspace_file_in_mode(
                    resolved,
                    crate::editor::workspace::WorkspaceOpenMode::Preview,
                    window,
                    cx,
                );
            }
        }
    }

    fn jump_to_heading_anchor(&mut self, anchor: &str, cx: &mut Context<Self>) {
        let source = self.buffer.text();
        if let Some(line) = heading_line_for_anchor(&source, anchor) {
            self.jump_to_source_line(line, cx);
        }
    }

    pub(crate) fn close_menu_bar(&mut self, cx: &mut Context<Self>) {
        let had_open_menu = self.menu_bar_open.take().is_some();
        let had_open_hamburger_list = std::mem::take(&mut self.hamburger_menu_open);
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
        if had_open_menu
            || had_open_hamburger_list
            || had_open_submenu
            || had_hover_state
            || had_pending_close
        {
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
mod tests;
