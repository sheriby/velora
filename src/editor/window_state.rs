//! Window-level editor state such as scrolling, mode switching, and menus.
//! 窗口标题显示 velora。

use super::*;
pub(crate) use crate::export::html::heading_line_for_anchor;
use crate::file_url::{percent_decode_or_raw, resolve_local_link_path, split_link_anchor};
use crate::components::markdown::link::EXTERNAL_LINK_SCHEMES;

/// P4b：stride 大多未知时的单帧最大预挂载行数。上限只削视口上下的余量，
/// 视口自身必须始终有挂载行覆盖。
const COLD_RUN_MAX_ROWS: usize = 12;

impl Editor {
    pub(super) fn on_registered_menu_action<A: Action>(
        &mut self,
        action: &A,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(handler) = crate::app_menu::editor_menu_handler(action) {
            handler(self, window, cx);
        } else {
            cx.propagate();
        }
    }

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

    #[cfg(test)]
    pub(super) fn map_scroll_offset(position: f32, from: &[f32], to: &[f32]) -> f32 {
        let prefix = |values: &[f32]| {
            let mut result = vec![0.0];
            let mut total = 0.0;
            for value in values { total += value.max(1.0); result.push(total); }
            result
        };
        Self::map_scroll_prefix(position, &prefix(from), &prefix(to), from, to)
    }

    fn map_scroll_prefix(position: f32, from_prefix: &[f32], to_prefix: &[f32], from: &[f32], to: &[f32]) -> f32 {
        let position = position.max(0.0);
        let rows = from.len().min(to.len());
        let from_ends = from_prefix.get(1..=rows).unwrap_or(&[]);
        let row = from_ends.partition_point(|end| *end < position);
        if row < rows {
            let from_start = from_prefix.get(row).copied().unwrap_or_default();
            let from_height = from.get(row).copied().unwrap_or(1.0).max(1.0);
            let to_start = to_prefix.get(row).copied().unwrap_or_default();
            let to_height = to.get(row).copied().unwrap_or(1.0).max(1.0);
            return to_start + (position - from_start) / from_height * to_height;
        }
        let from_total = from_prefix.get(rows).copied().unwrap_or_default();
        let to_total = to_prefix.get(rows).copied().unwrap_or_default();
        let scale = if from_total > 0.0 { to_total / from_total } else { 1.0 };
        to_total + (position - from_total) * scale
    }

    pub(super) fn scrollbar_document_position(&self, scroll_y: f32, inverse: bool) -> f32 {
        let Some(plan) = self.rendered_row_plan.as_ref() else {
            return scroll_y;
        };
        let prefix = plan.stride_prefix();
        let strides = plan.strides();
        // 文末预留半屏：以视口中心定位，滚动终点就是文档末尾，
        // 不会因最后几行首次量高而改变进度分母。
        let half_viewport = f32::from(self.scroll_handle.bounds().size.height) * 0.5;
        let origin = Self::map_scroll_prefix(half_viewport, &prefix.positive, &plan.scrollbar_prefix, &strides, &plan.scrollbar_strides);
        if inverse {
            (Self::map_scroll_prefix(scroll_y + origin, &plan.scrollbar_prefix, &prefix.positive, &plan.scrollbar_strides, &strides)
                - half_viewport)
                .max(0.0)
        } else {
            (Self::map_scroll_prefix(scroll_y + half_viewport, &prefix.positive, &plan.scrollbar_prefix, &strides, &plan.scrollbar_strides)
                - origin)
                .max(0.0)
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
        run.document_identity == self.buffer.identity()
            && run.plan_version == (self.document.row_meta_version(), self.fold_state_version,
                self.toc_state_version, self.document.visible_blocks().len(), self.view_mode == ViewMode::Rendered)
            && run.child_count > 0
            && self
                .scroll_handle
                .bounds_for_item(run.child_count - 1)
                .is_some()
            && self
                .scroll_handle
                .bounds_for_item(run.child_count)
                .is_none()
    }

    pub(super) fn entity_is_mounted(&self, entity_id: EntityId) -> bool {
        let Some(run) = self.prev_mounted_run.filter(|run| self.mounted_run_is_addressable(*run)) else {
            return false;
        };
        let Some(plan) = self.rendered_row_plan.as_ref() else { return false; };
        run.rows().any(|(row, _)| plan.rows.get(row).is_some_and(|row| row.entities().any(|entity| entity.entity_id() == entity_id)))
    }

    /// Picks the contiguous run of rows to mount; the culled runs become
    /// spacers and the focused row stays mounted, on its own island when it
    /// falls outside the run. `strides[i]` is row `i`'s
    /// footprint (height plus trailing gap); being scroll-invariant, their running
    /// sum places each row against a band from the current scroll offset.
    /// Unmeasured rows use a lower-bound estimate; where that falls short of the
    /// scroll offset the trailing run is mounted instead, so the window never
    /// lands on a spacer. The cold-start cap bounds only the pre-mounted margin
    /// around the viewport: the viewport span itself is always mounted in full,
    /// so a viewport wider than the cap's row budget (source mode's 28px lines
    /// fit 37 to a screen) cannot strand its lower edge on a spacer. Pure, so it
    /// is unit-tested headlessly.
    #[cfg(test)]
    pub(super) fn rendered_window(
        strides: &[f32],
        scroll_y: f32,
        viewport_height: f32,
        overdraw: f32,
        focus_row: Option<usize>,
    ) -> RenderWindow {
        let mut prefix = vec![0.0];
        let mut total = 0.0;
        for stride in strides { total += stride.max(0.0); prefix.push(total); }
        Self::rendered_window_from_prefix(&prefix, scroll_y, viewport_height, overdraw, focus_row)
    }

    #[cfg(test)]
    pub(super) fn rendered_window_from_prefix(
        prefix: &[f32],
        scroll_y: f32,
        viewport_height: f32,
        overdraw: f32,
        focus_row: Option<usize>,
    ) -> RenderWindow {
        Self::rendered_window_with_row_floor(
            prefix, scroll_y, viewport_height, overdraw, focus_row, None,
        )
    }

    pub(super) fn rendered_window_with_row_floor(
        prefix: &[f32],
        scroll_y: f32,
        viewport_height: f32,
        overdraw: f32,
        focus_row: Option<usize>,
        row_floor: Option<usize>,
    ) -> RenderWindow {
        let n = prefix.len().saturating_sub(1);
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

        let tops = prefix.get(..n).unwrap_or(&[]);
        let bottoms = prefix.get(1..=n).unwrap_or(&[]);
        let mut run_start = bottoms.partition_point(|bottom| *bottom < band_top);
        let mut run_end = tops.partition_point(|top| *top <= band_bottom);
        let mut top_of_start = prefix.get(run_start).copied().unwrap_or_default();
        let mut bottom_of_end = prefix.get(run_end).copied().unwrap_or_default();
        let viewport_first = bottoms.partition_point(|bottom| *bottom < scroll_y);
        let viewport_last = tops.partition_point(|top| *top <= viewport_bottom);
        let total = prefix.last().copied().unwrap_or_default();

        // P4b 冷启动保护：绝大多数 stride 还是估计值时，行高被严重低估
        // （一行真实 9000px 估计 16px），带状扫描会一口气挂载几十个巨行。
        // 上限只削视口上下的预挂载余量；视口自身这一段必须整段挂上——终点
        // 一旦剪进视口内部（旧实现钉在视口首行 + 2×COLD_RUN_MAX_ROWS 行），
        // 比这宽的视口下沿就落在 spacer 上，而续帧量到的只有已挂载的那段，
        // 没挂上的行永远量不到，needs_fill 从此原地踏步（源码模式文末
        // 连打回车时，第 24 行往后曾没有行号，切换一次模式才恢复）。
        let mut needs_fill = false;
        if viewport_first < n {
            let lead_floor = viewport_first.saturating_sub(COLD_RUN_MAX_ROWS);
            if run_start < lead_floor {
                run_start = lead_floor;
                top_of_start = prefix.get(lead_floor).copied().unwrap_or_default();
            }
            let cover_end = (viewport_last + COLD_RUN_MAX_ROWS).min(run_end);
            if cover_end < run_end {
                run_end = cover_end;
                bottom_of_end = prefix.get(run_end).copied().unwrap_or_default();
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
            let floor = (total - viewport_height - overdraw).max(0.0);
            run_start = tops.partition_point(|top| *top <= floor).saturating_sub(1);
            top_of_start = prefix.get(run_start).copied().unwrap_or_default();
        }

        // 刚滚动就切走时，前文还有未量过的行。切回后扩展挂载上边界会首次量到
        // 那些行，让同一段文字向下挪；用户未操作前沿用离开时的上边界。
        if let Some(row_floor) = row_floor.filter(|floor| *floor < run_end) {
            run_start = run_start.max(row_floor);
            top_of_start = prefix.get(run_start).copied().unwrap_or_default();
        }

        // Keep the focused row mounted; GPUI blurs an unmounted caret. It goes on
        // its own island rather than widening the run, so a caret left behind
        // while reading does not drag every row between it and the viewport on
        // screen with it.
        let mut top_h = top_of_start;
        let mut bottom_h = total - bottom_of_end;
        let mut focus_island = None;
        if let Some(focus_row) = focus_row.map(|row| row.min(n - 1)) {
            let focus_top = prefix.get(focus_row).copied().unwrap_or_default();
            let focus_bottom = prefix.get(focus_row + 1).copied().unwrap_or(focus_top);
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

    /// 正文列宽：可用宽度按偏好取比例（「跟随主题」走主题那一套居中宽度）。
    /// 渲染正文列、表格量宽、图片与 mermaid 估宽必须用同一个数。
    pub(crate) fn writing_column_width(
        viewport_width: f32,
        dimensions: &crate::theme::ThemeDimensions,
        cx: &App,
    ) -> f32 {
        let available_width = (viewport_width - dimensions.editor_padding * 2.0).max(1.0);
        crate::config::EditorSettings::writing_width(cx).column_width(
            available_width,
            Self::centered_column_width(viewport_width, dimensions),
            dimensions.writing_max_width,
        )
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

    /// 「关闭标签页」（cmd/ctrl-w）：键位与文件菜单共用这一条。
    pub(crate) fn on_close_tab_action(
        &mut self,
        _: &crate::components::CloseTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_active_tab(window, cx);
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

    pub(crate) fn toggle_reading_mode(&mut self, cx: &mut Context<Self>) {
        if let Some(parent) = self.split_parent.clone() {
            cx.defer(move |cx| {
                if let Err(error) = parent.update(cx, |editor, cx| editor.toggle_reading_mode(cx)) {
                    eprintln!("切换阅读模式失败：{error}");
                }
            });
            return;
        }
        self.finish_ime_composition(cx);
        self.dismiss_contextual_overlays(cx);
        self.reading_mode.set(!self.reading_mode.get());
        for group in self.split_groups() {
            group.update(cx, |editor, cx| {
                editor.finish_ime_composition(cx);
                editor.dismiss_contextual_overlays(cx);
                cx.notify();
            });
        }
        cx.notify();
    }

    pub(crate) fn is_read_only(&self) -> bool {
        self.reading_mode.get()
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

    /// 「选择全文」（⌘⇧A / Ctrl+Shift+A）：键位与命令面板都汇到这一条。
    ///
    /// 与 ⌘A 那条「先当前段落、再按一次选整篇」的循环共用同一个选区模型，只是不看
    /// 那台计时器（`Editor::select_document`）；选全文要有这条明面上的入口，
    /// 不该让人去猜双击。
    pub(crate) fn on_select_document_action(
        &mut self,
        _: &crate::components::SelectDocument,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_document(cx);
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
    /// 只在「马上要换掉整棵块树、之后要把这份现场交还给同一篇」的入口用；换之前取，
    /// 块树一换旧实体 id 就作废了。`content` 是调用方已经序列化好的整篇文本：这里不
    /// 再取第二遍，全文哈希本来就只该在这份文本上算一次。
    pub(crate) fn capture_document_view(&self, content: &str, cx: &App) -> DocumentView {
        if let Some(view) = &self.pending_scroll_restore {
            return view.clone();
        }
        self.document_view_captures.set(self.document_view_captures.get() + 1);
        let scroll_y = f32::from(self.scroll_handle.offset().y);
        DocumentView {
            view_mode: self.view_mode,
            content_len: content.len(),
            content_version: persistence::file_content_version_normalized(content),
            scroll_y,
            row_anchor: self.capture_scroll_row_anchor(scroll_y),
            row_layout: self.capture_document_row_layout(),
            editing_state: Some(Arc::new(self.capture_document_editing_state(cx))),
            selection: self.capture_source_selection_snapshot(cx),
        }
    }

    fn capture_document_row_layout(&self) -> Option<Arc<DocumentRowLayout>> {
        let plan = self.rendered_row_plan.as_ref()?;
        if plan.row_meta_version != self.document.row_meta_version()
            || plan.rendered_mode != (self.view_mode == ViewMode::Rendered)
            || plan.fold_version != self.fold_state_version
        {
            return None;
        }
        let mounted = self
            .prev_mounted_run
            .filter(|run| self.mounted_run_is_addressable(*run));
        let strides = plan.strides();
        let mut rows = plan
            .rows
            .iter()
            .enumerate()
            .map(|(row, planned)| DocumentRowMeasurement {
                visible_start: planned.visible_start,
                visible_end: planned.visible_end,
                height: strides.get(row).copied().unwrap_or(0.0)
                    - plan.gaps.get(row + 1).copied().unwrap_or(0.0),
                measured: self.row_stride_cache.contains_key(&planned.first_id),
            })
            .collect::<Vec<_>>();
        // 最后一帧的真实行高尚未进下一帧的缓存，离开前必须一并带走。
        if let Some(mounted) = mounted {
            for (row, child) in mounted.rows() {
                if let Some(bounds) = self.scroll_handle.bounds_for_item(child)
                    && let Some(measurement) = rows.get_mut(row)
                {
                    measurement.height = f32::from(bounds.size.height);
                    measurement.measured = true;
                }
            }
        }
        Some(Arc::new(DocumentRowLayout {
            width: self.row_stride_width?,
            theme: self.row_stride_theme.clone()?,
            rendered_mode: plan.rendered_mode,
            row_start: mounted.map(|run| run.row_start),
            rows,
        }))
    }

    /// 视口顶压在哪一行。行计划还没建时（窗口第一帧）给不出锚点。
    fn capture_scroll_row_anchor(&self, scroll_y: f32) -> Option<ScrollRowAnchor> {
        let plan = self.rendered_row_plan.as_ref()?;
        let rows = plan.rows.len();
        if rows == 0 {
            return None;
        }
        let prefix = plan.stride_prefix();
        let tops = &prefix.nonnegative;
        let content_y = (-scroll_y).max(0.0);
        let row = tops
            .partition_point(|top| *top <= content_y)
            .saturating_sub(1)
            .min(rows - 1);
        Some(ScrollRowAnchor {
            rendered_mode: plan.rendered_mode,
            row,
            within: content_y - tops.get(row).copied().unwrap_or(0.0),
        })
    }

    /// 行高与偏移一起交还，首帧之后不再干预滚动。
    pub(crate) fn apply_pending_scroll_restore(&mut self) -> bool {
        let Some(view) = self.pending_scroll_restore.take() else {
            return false;
        };
        let mut restored_rows = 0;
        if let Some(layout) = &view.row_layout
            && let Some(plan) = &self.rendered_row_plan
            && self.row_stride_width == Some(layout.width)
            && self
                .row_stride_theme
                .as_ref()
                .is_some_and(|theme| Arc::ptr_eq(theme, &layout.theme))
            && plan.rendered_mode == layout.rendered_mode
        {
            for (row, (measurement, planned)) in layout.rows.iter().zip(&plan.rows).enumerate() {
                if measurement.visible_start != planned.visible_start
                    || measurement.visible_end != planned.visible_end
                {
                    break;
                }
                let stride = measurement.height + plan.gaps.get(row + 1).copied().unwrap_or(0.0);
                plan.set_stride(row, stride);
                if measurement.measured {
                    self.row_stride_cache.insert(planned.first_id, stride);
                }
                restored_rows += 1;
            }
        }
        let target = if view.row_anchor.is_some_and(|anchor| anchor.row < restored_rows) {
            self.restored_row_start = view
                .row_layout
                .as_ref()
                .and_then(|layout| layout.row_start);
            view.scroll_y
        } else {
            view.row_anchor
                .and_then(|anchor| self.anchor_scroll_y(&anchor))
                .unwrap_or(view.scroll_y)
        };
        let mut offset = self.scroll_handle.offset();
        // 句柄的上限还是上一标签的，不能在新标签布局之前用它夹取。
        offset.y = px(target.min(0.0));
        self.scroll_handle.set_offset(offset);
        true
    }

    /// 锚点行当前在内容坐标里的位置；行结构与记下来时对不上就不认这个锚点。
    fn anchor_scroll_y(&self, anchor: &ScrollRowAnchor) -> Option<f32> {
        let plan = self.rendered_row_plan.as_ref()?;
        if plan.rendered_mode != anchor.rendered_mode || plan.rows.is_empty() {
            return None;
        }
        let row = anchor.row.min(plan.rows.len() - 1);
        let prefix = plan.stride_prefix();
        let top = prefix.nonnegative.get(row).copied().unwrap_or(0.0);
        Some(-(top + anchor.within))
    }

    /// 视口顶那一行的缓冲区字节偏移。
    ///
    /// 两种视图是同一份缓冲区的两份投影，行结构不同但字节偏移都算得出来：
    /// 源码分块是缓冲区的连续切片（块内偏移就是字节偏移），渲染块要经
    /// 「显示偏移 → 内容偏移 → 源码偏移」这条与选区快照同源的换算。
    fn capture_viewport_source_anchor(&self, cx: &App) -> Option<ViewModeScrollAnchor> {
        let (entity, bounds, y) = self.block_at_viewport_top(cx)?;
        let block = entity.read(cx);
        let local = block.index_for_mouse_position(point(bounds.left() + px(1.0), y));
        let id = entity.entity_id();
        let span = self.document.source_span_of(id)?;
        let identity = span.start + local.min(span.len());
        if self.view_mode == ViewMode::Source || self.code_document {
            return Some(ViewModeScrollAnchor {
                offset: identity,
                settle_frames: Self::VIEW_MODE_SCROLL_SETTLE_FRAMES,
            });
        }
        // 渲染块的源码偏移要走与选区快照同源的换算；映射缺失时退回按块内
        // 偏移直接读（代码块等 raw 编辑的块本就是同一坐标）。
        let offset = self
            .source_mapping_for_entity(id, cx)
            .map(|mapping| {
                let content = block.current_range_to_markdown_range(local..local).start;
                let last = mapping.content_to_source.len().saturating_sub(1);
                mapping.full_source_range.start + mapping.content_to_source[content.min(last)]
            })
            .unwrap_or(identity);
        Some(ViewModeScrollAnchor {
            offset,
            settle_frames: Self::VIEW_MODE_SCROLL_SETTLE_FRAMES,
        })
    }

    /// 视口顶落在哪一块上：返回它、量到的边界，以及取落点用的窗口 y。
    ///
    /// 只认**当前挂载**的块：`last_bounds` 不会随块滚出视口清掉，曾画过的块会
    /// 一直留着上次的窗口坐标；而被钉在窗口里的光标块（焦点岛）即使远在屏幕
    /// 外也有新鲜边界。按陈旧坐标挑落点，会把早已滚过去的块认成读者看的地方。
    /// 视口顶落在块间空隙上时取下面第一段的块首。
    ///
    /// 表格、分隔线这类块没有文字元素（`last_bounds` 恒为空），它们的落点只能
    /// 按行框量；量不出文字布局时块内偏移恒为块首（`index_for_mouse_position`）。
    fn block_at_viewport_top(
        &self,
        cx: &App,
    ) -> Option<(Entity<Block>, Bounds<Pixels>, Pixels)> {
        let viewport_top = self.scroll_handle.bounds().top();
        let mut below: Option<(Entity<Block>, Bounds<Pixels>)> = None;
        for entry in self.document.visible_blocks() {
            let id = entry.entity.entity_id();
            if !self.entity_is_mounted(id) {
                continue;
            }
            let block = entry.entity.read(cx);
            let Some(bounds) = block.last_bounds.or_else(|| self.mounted_row_bounds(id)) else {
                continue;
            };
            if bounds.bottom() <= viewport_top {
                continue;
            }
            if bounds.top() >= viewport_top {
                below.get_or_insert_with(|| (entry.entity.clone(), bounds));
                continue;
            }
            return Some((entry.entity.clone(), bounds, viewport_top));
        }
        below.map(|(entity, bounds)| (entity, bounds, bounds.top()))
    }

    /// 缓冲区字节偏移 → 它所在的块与块内显示偏移（换投影后交还阅读位置用）。
    pub(crate) fn block_and_display_offset_for_source_offset(
        &self,
        offset: usize,
        cx: &App,
    ) -> Option<(Entity<Block>, usize)> {
        for entry in self.document.visible_blocks() {
            let id = entry.entity.entity_id();
            let Some(span) = self.document.source_span_of(id) else {
                continue;
            };
            if offset < span.start || offset > span.end {
                continue;
            }
            let block = entry.entity.read(cx);
            if self.view_mode == ViewMode::Source || self.code_document {
                return Some((entry.entity.clone(), (offset - span.start).min(block.visible_len())));
            }
            // 渲染块要沿与选区快照同源的反向换算回到显示偏移；映射缺失时
            // 退回按块内偏移直接读（代码块等 raw 编辑的块本就是同一坐标）。
            let Some(mapping) = self.source_mapping_for_entity(id, cx) else {
                return Some((entry.entity.clone(), (offset - span.start).min(block.visible_len())));
            };
            let last = mapping.source_to_content.len().saturating_sub(1);
            let relative = offset.saturating_sub(mapping.full_source_range.start);
            let content = mapping.source_to_content[relative.min(last)];
            let display = block.markdown_range_to_current_range(content..content).start;
            return Some((entry.entity.clone(), display));
        }
        None
    }

    /// 换投影时交还的阅读位置：目标块的块内几何来自上一帧的绘制结果，未挂载时
    /// 先按行计划估算把它拉进窗口，下一帧再按真实几何对齐。
    /// 返回是否改动了滚动偏移。
    pub(crate) fn apply_pending_view_mode_scroll(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(anchor) = self.pending_view_mode_scroll else {
            return false;
        };
        if anchor.settle_frames == 0 {
            self.pending_view_mode_scroll = None;
            return false;
        }
        let remap = |frames: u8| ViewModeScrollAnchor {
            offset: anchor.offset,
            settle_frames: frames,
        };
        let Some((block, local)) =
            self.block_and_display_offset_for_source_offset(anchor.offset, cx)
        else {
            // 目标已经不在任何块里（这几十帧里文档被改过）：交还作废。
            self.pending_view_mode_scroll = None;
            return false;
        };
        // 挂载之外的目标只有陈旧几何可用：等它被钉成焦点岛画出真边界再说。
        let anchor_top = self
            .entity_is_mounted(block.entity_id())
            .then(|| block.read(cx).visible_range_bounds(local..local).map(|bounds| bounds.top()))
            .flatten()
            .or_else(|| self.mounted_row_bounds(block.entity_id()).map(|bounds| bounds.top()));
        // 目标块还没挂载时只有估计：先按行计划把它拉进窗口，下一帧它就有真实
        // 几何了。渲染态一行可能只有几行字，源码态一屏却盖大半个 512 行分块——
        // 照着像素偏移原样换过去，目标块很可能落在窗外，永远等不到自己的几何。
        let target = match anchor_top {
            Some(anchor_top) => {
                let viewport_top = self.scroll_handle.bounds().top();
                let delta = anchor_top - viewport_top;
                if delta.abs() <= px(0.5) {
                    self.pending_view_mode_scroll = None;
                    return false;
                }
                self.scroll_handle.offset().y - delta
            }
            None => {
                let Some(estimated) = self.estimated_scroll_y_for_block(block.entity_id()) else {
                    self.pending_view_mode_scroll = Some(remap(anchor.settle_frames - 1));
                    self.schedule_followup_frame(cx);
                    return false;
                };
                estimated
            }
        };
        let max_offset_y = self.scroll_handle.max_offset().y.max(px(0.0));
        let current = self.scroll_handle.offset().y;
        let target = target.min(px(0.0)).max(-max_offset_y);
        let mut offset = self.scroll_handle.offset();
        offset.y = target;
        self.scroll_handle.set_offset(offset);
        // 行高要等测量落定：自己排下一帧，不能指望外部输入来推。
        self.pending_view_mode_scroll = Some(remap(anchor.settle_frames - 1));
        self.schedule_followup_frame(cx);
        target != current
    }

    /// 某一块在行计划里的估计滚动偏移（负值）。
    ///
    /// 没量过的行只是下界估计，所以它只能保证把目标行拉进窗口，精确对齐还得
    /// 靠下一帧的真实几何——与标签切回的交还同一套路（`anchor_scroll_y`）。
    fn estimated_scroll_y_for_block(&self, entity_id: EntityId) -> Option<Pixels> {
        let plan = self.rendered_row_plan.as_ref()?;
        let visible_index = self.document.visible_index_for_entity_id(entity_id)?;
        let row = plan
            .visible_starts
            .partition_point(|start| *start <= visible_index)
            .saturating_sub(1);
        let top = plan.stride_prefix().nonnegative.get(row).copied().unwrap_or(0.0);
        Some(px(-top))
    }

    /// 用户操作优先于尚未交还的阅读位置。
    pub(crate) fn cancel_pending_view_mode_scroll(&mut self) {
        self.pending_view_mode_scroll = None;
    }

    /// 用户操作优先于尚未交还的标签现场。
    pub(crate) fn cancel_pending_scroll_restore(&mut self) {
        self.pending_scroll_restore = None;
        self.restored_row_start = None;
    }

    /// 换投影交还阅读位置的 settle 帧数：首帧把它钉成焦点岛、次帧拿到几何，
    /// 再留几帧校正测量漂移——与滚动的 settle 同一量级。
    const VIEW_MODE_SCROLL_SETTLE_FRAMES: u8 = 6;

    pub(crate) fn toggle_view_mode(&mut self, cx: &mut Context<Self>) {
        if self.image_preview.is_some() {
            return;
        }
        self.end_block_pointer_selection_sessions(cx);
        let selection_snapshot = self.capture_source_selection_snapshot(cx);
        // 换投影之前先把视口顶记成缓冲区坐标：两边行结构不同，只有这个坐标跨
        // 模式成立。必须趁 `view_mode` 还没翻的时候取——挂载行与行计划是按
        // 「当前是哪种投影」记账的，翻完再取就认不出当前挂载了哪一块。
        let view_anchor = self.capture_viewport_source_anchor(cx);
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
        self.pending_view_mode_scroll = view_anchor;
        self.rebuild_document_from_buffer(cx);

        self.apply_selection_snapshot_in_current_mode(&selection_snapshot, cx);
        // 有阅读位置锚点时不再把光标块滚进视口：读者可能正滚在别处看，光标留在
        // 文件开头也要留在原地，视口交给锚点交还（旧行为是一律滚到光标，模式一
        // 切就被拽回文档顶部）。
        self.pending_scroll_active_block_into_view = self.pending_view_mode_scroll.is_none();
        self.pending_scroll_recheck_after_layout = true;
        self.last_scroll_viewport_size = None;
        // 换了投影（也让行锚点的行结构作废）：挂着的交还不再有效。
        self.cancel_pending_scroll_restore();
        self.pending_window_title_refresh = true;
        self.close_dialog_restore_focus = None;
        self.table_axis_preview = None;
        self.table_axis_selection = None;
        self.dismiss_contextual_overlays(cx);
        self.sync_table_axis_visuals(cx);
        // 模式切换会换掉整套块实体，文档内搜索高亮必须重算，否则正文里的
        // 命中全丢。可见性判据只住在 sync_document_search_highlights 里：这里
        // 再抄一份「面板开着且查询非空」就把 vim 的 / 高亮挡掉了。
        self.sync_document_search_highlights(cx);
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
        self.split_projection_revision = self.buffer.revision();
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
        // 块自身的重绘不会通知窗口根；共享文档每一笔都要唤醒其他编辑器组。
        if self.split_parent.is_some() { cx.notify(); }
        self.refresh_source_line_starts(cx);
        self.schedule_autosave(cx);
        self.refresh_document_find_after_edit(cx);
    }

    /// 源码分块文档的行号续号：先收集各块区间起点，再用缓冲区的批量换算
    /// `lines_and_line_starts` 一趟 O(全文) 算出所有行号。逐块调 `line_of` 是
    /// O(根块数 × chunk 数) 的平方项（代码文档按 512 行切片，10 MiB 就是百万步级）；
    /// 区间起点天然升序，正是批量接口要的形状。
    pub(super) fn refresh_source_line_starts(&mut self, cx: &mut Context<Self>) {
        // 用户手动切到源码模式（⌘Tab）也算：这一档的块同样是缓冲区切片，
        // 回车新建的块若不走这里，就沿用默认的 1，行号栏变成 1/1/1。
        if !(self.code_document
            || self.source_mode_fallback_required
            || self.view_mode == crate::editor::ViewMode::Source)
        {
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
        // 全文档一致（此前 512 上下行号对不齐）。
        let basis = self.buffer.line_count();
        for block in &blocks {
            block.update(cx, |block, _cx| {
                block.reading_mode = Some(self.reading_mode.clone());
                block.set_source_line_gutter_basis(basis);
            });
        }
    }

    pub(super) fn request_active_block_scroll_into_view(&mut self, cx: &mut Context<Self>) {
        self.cancel_pending_scroll_restore();
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
        if self.is_read_only() {
            return;
        }

        // 源码/代码视图没有「模型的写法」可言：那里的块就是文件本身。
        if self.view_mode != ViewMode::Rendered || self.image_preview.is_some() {
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
        let open_target = if self.ai_result_preview {
            match classify_link_target(&open_target) {
                LinkTarget::Anchor(anchor) => { self.jump_to_heading_anchor(&anchor, cx); return; }
                LinkTarget::External(url) => url,
                LinkTarget::Local { path, anchor } => {
                    let resolved = resolve_local_link_path(self.local_link_base_dir().as_deref(), &path);
                    let Some(root) = self.local_link_base_dir() else { return; };
                    let Ok(canonical) = resolved.canonicalize() else { return; };
                    if !canonical.starts_with(root) { return; }
                    let Ok(mut url) = url::Url::from_file_path(canonical) else { return; };
                    url.set_fragment(anchor.as_deref()); url.to_string()
                }
            }
        } else { open_target };
        let Some(any_handle) = self.window_handle else {
            return;
        };
        let Some(handle) = crate::ui::window_content::<Editor>(any_handle, cx) else {
            return;
        };
        cx.defer(move |cx| {
            if let Err(error) = handle.update(cx, |editor, window, cx| {
                editor.open_link_target(open_target.clone(), window, cx);
            }) { eprintln!("打开文档链接失败：{error}"); }
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
                let resolved =
                    resolve_local_link_path(self.local_link_base_dir().as_deref(), &path);
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

/// 把 Markdown 链接目标分成外部链接 / 文内锚点 / 本地路径。
pub(crate) fn classify_link_target(target: &str) -> LinkTarget {
    let trimmed = target.trim();
    if let Some(anchor) = trimmed.strip_prefix('#') {
        return LinkTarget::Anchor(percent_decode_or_raw(anchor).into_owned());
    }
    let lowered = trimmed.to_ascii_lowercase();
    if EXTERNAL_LINK_SCHEMES
        .iter()
        .any(|scheme| lowered.starts_with(scheme))
    {
        return LinkTarget::External(trimmed.to_string());
    }
    let file_url_target = strip_file_url_prefix(trimmed, &lowered);
    let (path, anchor) = match file_url_target {
        Some(rest) => {
            let (path, anchor) = split_link_anchor(rest);
            (
                local_path_from_file_url(path),
                anchor.map(|anchor| percent_decode_or_raw(anchor).into_owned()),
            )
        }
        None => {
            let (path, anchor) = split_link_anchor(trimmed);
            let path = percent_decode_or_raw(path).into_owned();
            (path, anchor.map(|anchor| percent_decode_or_raw(anchor).into_owned()))
        }
    };
    LinkTarget::Local {
        path,
        anchor,
    }
}

/// `file:` 目标还原成本地路径。
///
/// 绝对写法（`file:///Users/me/a.md`、`file://localhost/Users/me/a.md`）交给
/// `file_url::parse_file_url`——那是全仓唯一的文件 URL 解析，认 authority 也认
/// 百分号转义。这里以前对整串 `trim_start_matches('/')`，把
/// `file:///Users/me/a.md` 削成相对的 `Users/me/a.md`，同一个文件写成
/// `/Users/me/a.md` 却点得开：绝对 URL 与相对写法不该走同一条剥前缀的路。
/// `file:docs/a.md` 这种不带斜杠的仍是相对当前文档的写法，只剥 scheme。
fn local_path_from_file_url(rest: &str) -> String {
    let decoded = if rest.starts_with('/') || rest.starts_with("//") {
        crate::file_url::parse_file_url(&format!("file:{rest}"))
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|| percent_decode_or_raw(rest).into_owned())
    } else {
        percent_decode_or_raw(rest).into_owned()
    };
    // `file:///C:/Notes/x.md` 解出来是 `/C:/Notes/x.md`——前导斜杠 + 盘符。在 Windows
    // 上这**不是**绝对路径（缺盘符根），`is_absolute` 为假，链接随后落到「目标不存在」
    // 那条静默返回上：Windows 上 file: 链接点不开。把盘符前那道斜杠去掉。
    match decoded.strip_prefix('/') {
        Some(rest) if has_drive_prefix(rest) => rest.to_string(),
        _ => decoded,
    }
}

/// `C:` 开头的盘符写法。
fn has_drive_prefix(path: &str) -> bool {
    let mut chars = path.chars();
    matches!(chars.next(), Some(first) if first.is_ascii_alphabetic())
        && chars.next() == Some(':')
}

/// 大小写不敏感地剥掉 `file:` scheme（判据看的是 `lowered`），返回原文里剩下的目标。
/// `to_ascii_lowercase` 只改 ASCII 字母、不改字节长度，所以 `lowered` 以这段纯 ASCII
/// 前缀开头时，原文在同一个偏移上必是字符边界——旧的 `trimmed[5..]` 正是靠这件事才不
/// panic。现在改用 `get`：越界只会得到 `None`（按「没写 scheme」处理），边界写进类型，
/// 不再靠运气。
fn strip_file_url_prefix<'a>(target: &'a str, lowered: &str) -> Option<&'a str> {
    lowered
        .strip_prefix("file:")
        .and_then(|_| target.get("file:".len()..))
}



#[cfg(test)]
mod tests;
