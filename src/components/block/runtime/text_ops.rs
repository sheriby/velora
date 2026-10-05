use super::*;

impl Block {
    pub(crate) fn mark_changed(&mut self, cx: &mut Context<Self>) {
        self.sync_edit_mode_from_kind();
        self.sync_render_cache();
        self.cursor_blink_epoch = Instant::now();
        self.clear_vertical_motion();
        cx.emit(BlockEvent::Changed);
        cx.notify();
    }

    pub(crate) fn convert_to_paragraph(&mut self, cx: &mut Context<Self>) {
        self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
        self.record.kind = BlockKind::Paragraph;
        self.record.raw_fallback = None;
        self.quote_reparse_requested = false;
        self.mark_changed(cx);
    }

    pub(crate) fn convert_to_separator(&mut self, cx: &mut Context<Self>) {
        self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
        self.make_separator();
        cx.emit(BlockEvent::Changed);
        cx.notify();
    }

    /// Turns this block into a separator in place without emitting events or
    /// capturing undo, so editor-level flows that already manage those can
    /// reuse the conversion.
    pub(crate) fn make_separator(&mut self) {
        // 先认下用户敲的是哪种写法（`***`/`___`/`---`），再清内容——清完就没
        // 得认了，序列化只会写默认的 `---`。
        let marker = crate::components::SeparatorMarker::detect(&self.display_text());
        self.clear_inline_projection();
        self.record.kind = BlockKind::Separator;
        self.record.separator_marker = marker;
        self.record.raw_fallback = None;
        self.record.set_title(InlineTextTree::plain(String::new()));
        self.quote_reparse_requested = false;
        self.sync_edit_mode_from_kind();
        self.sync_render_cache();
        self.assign_collapsed_selection_offset(0, CollapsedCaretAffinity::Default, None);
        self.marked_range = None;
        self.cursor_blink_epoch = Instant::now();
        self.clear_vertical_motion();
    }

    /// 就地换块种类，不动标题文本。
    ///
    /// 不发事件、不开撤销组：一条命令改多块时由 `Editor` 统一开一个撤销组、重建元数据、
    /// 把整段字节一次写回。返回种类是否真的变了。
    /// `marker` 是列表记号的写法（`-`/`*`/`+`、`.`/`)`）；`None` 表示这块原来记的是什么
    /// 就留什么。换进列表时由调用方从相邻项抄过来，用户写的 `+ `、`1)` 不该被换成规范形。
    pub(crate) fn set_kind_in_place(
        &mut self,
        next: BlockKind,
        marker: Option<crate::components::ListMarkerStyle>,
    ) -> bool {
        // 源码原文那份不动；代码块是「整块正文就是原文」，换得出去也换得进来，
        // 由 `sync_edit_mode_from_kind` 改编辑模式。
        if self.edit_mode == EditMode::SourceRaw || self.record.kind == next {
            return false;
        }
        if let Some(marker) = marker {
            self.record.list_marker = marker;
        }
        if !next.is_code_block() {
            // 换出代码块那一族：围栏行与「文件里本来是四格缩进」这两本账跟着清，
            // 留着的话下一次序列化还会按代码块的形状写（把正文包进围栏、或补回缩进）。
            self.record.code_is_indented = false;
            self.record.source_fence_lines = None;
        }
        self.record.kind = next;
        self.record.raw_fallback = None;
        self.quote_reparse_requested = false;
        self.clear_inline_projection();
        self.sync_edit_mode_from_kind();
        self.sync_render_cache();
        true
    }

    pub(crate) fn enter_code_block(
        &mut self,
        language: Option<SharedString>,
        cx: &mut Context<Self>,
    ) {
        self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
        self.clear_inline_projection();
        self.record.kind = BlockKind::CodeBlock { language };
        self.record.raw_fallback = None;
        self.record.set_title(InlineTextTree::plain(String::new()));
        self.quote_reparse_requested = false;
        self.sync_edit_mode_from_kind();
        self.sync_render_cache();
        self.assign_collapsed_selection_offset(0, CollapsedCaretAffinity::Default, None);
        self.marked_range = None;
        self.cursor_blink_epoch = Instant::now();
        self.clear_vertical_motion();
        cx.emit(BlockEvent::Changed);
        cx.notify();
    }

    /// Convert the current paragraph into a display-math block. `body` becomes
    /// the formula source between the fences (empty for a fresh `$$` block), and
    /// the caret lands at the start of that body line.
    pub(crate) fn enter_math_block(&mut self, body: &str, cx: &mut Context<Self>) {
        let source = format!("$$\n{body}\n$$");
        let cursor = "$$\n".len();

        self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
        self.clear_inline_projection();
        self.record.kind = BlockKind::MathBlock;
        self.record.set_title(InlineTextTree::plain(source));
        self.quote_reparse_requested = false;
        self.sync_edit_mode_from_kind();
        self.sync_render_cache();
        self.assign_collapsed_selection_offset(cursor, CollapsedCaretAffinity::Default, None);
        self.marked_range = None;
        self.cursor_blink_epoch = Instant::now();
        self.clear_vertical_motion();
        cx.emit(BlockEvent::Changed);
        cx.notify();
    }

    /// Toggle a style flag directly on the fragment tree without ever
    /// manipulating raw marker characters.  The selection range determines
    /// which fragments have their [`InlineStyle`] flag flipped.
    ///
    /// Serializers later translate these flags back to markers on export.
    pub(crate) fn toggle_inline_format(
        &mut self,
        format: InlineFormat,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.selected_range.is_empty() || self.uses_raw_text_editing() {
            return false;
        }

        let reversed = Some(self.selection_reversed);
        self.toggle_inline_format_in_range(format, self.selected_range.clone(), reversed, cx)
    }

    /// 在指定的**可见文本**区间上开关一种行内格式。选中菜单与右键菜单拿到的选区可能
    /// 只盖住一个块的一部分，也可能横跨多个块（那种情况由 `Editor` 逐块切段后调这里）。
    /// 区间用屏幕上的坐标，块内自己换算到树里的坐标——调用方不需要知道标记占位。
    /// 返回样式是否真的变了。
    pub(crate) fn toggle_inline_format_in_range(
        &mut self,
        format: InlineFormat,
        selection: Range<usize>,
        reversed: Option<bool>,
        cx: &mut Context<Self>,
    ) -> bool {
        if selection.is_empty() || self.uses_raw_text_editing() {
            return false;
        }

        let selection = self.current_to_clean_range(selection);
        let mut next_title = self.record.title.clone();
        let changed = match format {
            InlineFormat::Bold => next_title.toggle_bold(selection.clone()),
            InlineFormat::Italic => next_title.toggle_italic(selection.clone()),
            InlineFormat::Underline => next_title.toggle_underline(selection.clone()),
            InlineFormat::Strikethrough => next_title.toggle_strikethrough(selection.clone()),
            InlineFormat::Code => next_title.toggle_code(selection.clone()),
            InlineFormat::Superscript => next_title.toggle_superscript(selection.clone()),
            InlineFormat::Subscript => next_title.toggle_subscript(selection.clone()),
            InlineFormat::Highlight => next_title.toggle_highlight(selection.clone()),
        };
        if !changed {
            return false;
        }

        self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
        self.apply_title_edit(
            next_title,
            selection.end,
            None,
            Some(selection),
            reversed,
            false,
            cx,
        );
        true
    }

    fn current_line_layout_and_offset(&self) -> Option<(&WrappedLine, usize)> {
        let lines = self.last_layout.as_ref()?;
        let text = self.display_text();
        let ranges = crate::components::block::element::hard_line_ranges(text);
        let (line_idx, offset_in_line) =
            crate::components::block::element::line_index_for_offset(&ranges, self.cursor_offset());
        Some((lines.get(line_idx)?, offset_in_line))
    }

    pub(crate) fn vertical_anchor_x(&self) -> Pixels {
        self.vertical_motion_x
            .or_else(|| {
                self.current_line_layout_and_offset()
                    .and_then(|(layout, offset_in_line)| {
                        crate::components::block::element::position_for_offset(
                            layout,
                            offset_in_line,
                            self.last_line_height,
                            true,
                        )
                        .map(|position| position.x)
                    })
            })
            .unwrap_or(px(0.0))
    }

    /// Attempt to move the cursor up (direction < 0) or down one visual line
    /// within the current block.  Returns false if the cursor is already at
    /// the first or last line, so the editor can transfer focus instead.
    pub(crate) fn move_cursor_vertically(
        &mut self,
        direction: i32,
        preferred_x: Pixels,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(lines) = self.last_layout.as_ref() else {
            return false;
        };

        let text = self.display_text();
        let ranges = crate::components::block::element::hard_line_ranges(text);
        let (current_line_idx, offset_in_line) =
            crate::components::block::element::line_index_for_offset(&ranges, self.cursor_offset());
        let Some(current_layout) = lines.get(current_line_idx) else {
            return false;
        };
        let Some(current_position) = crate::components::block::element::position_for_offset(
            current_layout,
            offset_in_line,
            self.last_line_height,
            true,
        ) else {
            return false;
        };

        let current_y = crate::components::block::element::wrapped_line_top(
            lines,
            self.last_line_height,
            current_line_idx,
        ) + current_position.y;
        let target_y = if direction < 0 {
            current_y - self.last_line_height + self.last_line_height / 2.0
        } else {
            current_y + self.last_line_height + self.last_line_height / 2.0
        };
        if target_y < px(0.0) {
            return false;
        }

        let total_height = lines.iter().fold(px(0.0), |height, line| {
            height
                + crate::components::block::element::wrapped_line_height(
                    line,
                    self.last_line_height,
                )
        });
        if target_y >= total_height {
            return false;
        }

        let Some((target_line_idx, target_y_in_line)) =
            crate::components::block::element::wrapped_line_for_y(
                lines,
                self.last_line_height,
                target_y,
            )
        else {
            return false;
        };
        let target_layout = &lines[target_line_idx];
        let target_point = point(preferred_x, target_y_in_line);
        let target_offset_in_line =
            match target_layout.closest_index_for_position(target_point, self.last_line_height) {
                Ok(idx) | Err(idx) => idx,
            };

        let flat_offset = ranges[target_line_idx].start + target_offset_in_line;
        self.move_to_with_preferred_x(flat_offset, Some(preferred_x), cx);
        true
    }

    /// Compute the character offset where the cursor should land when focus
    /// enters this block from above or below.  Uses the stored vertical
    /// motion anchor so cursor horizontal position is preserved across
    /// different-height blocks.
    pub fn entry_offset_for_vertical_focus(
        &self,
        prefer_last_line: bool,
        preferred_x: Option<Pixels>,
    ) -> usize {
        let Some(lines) = self.last_layout.as_ref() else {
            return if prefer_last_line {
                self.visible_len()
            } else {
                0
            };
        };

        let text = self.display_text();
        let ranges = crate::components::block::element::hard_line_ranges(text);
        let target_line_idx = if prefer_last_line { lines.len() - 1 } else { 0 };
        let target_layout = &lines[target_line_idx];
        let target_x = preferred_x.unwrap_or(px(0.0));
        let target_y = if prefer_last_line {
            crate::components::block::element::wrapped_line_height(
                target_layout,
                self.last_line_height,
            ) - self.last_line_height / 2.0
        } else {
            self.last_line_height / 2.0
        };

        let offset_in_line = match target_layout
            .closest_index_for_position(point(target_x, target_y), self.last_line_height)
        {
            Ok(idx) | Err(idx) => idx,
        };
        ranges[target_line_idx].start + offset_in_line
    }

    pub fn move_to_with_preferred_x(
        &mut self,
        offset: usize,
        preferred_x: Option<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.assign_collapsed_selection_offset(
            offset,
            CollapsedCaretAffinity::Default,
            preferred_x,
        );
        self.cursor_blink_epoch = Instant::now();
        cx.notify();
    }

    /// Starts the cursor blink loop: a repeating background timer every 33ms
    /// that calls `cx.notify()` to repaint the cursor — but only while the
    /// cursor opacity is actually animating. During the first 0.5 s after
    /// each `cursor_blink_epoch` reset (which arrow keys / typing trigger),
    /// opacity is pinned to 1.0, so a repaint would just re-do the full
    /// projection rebuild for no visible change.
    ///
    /// The blink task is automatically cancelled when the block loses focus
    /// (the task handle is dropped in [`Block::render`]).
    pub(crate) fn start_cursor_blink(&mut self, cx: &mut Context<Self>) {
        self.cursor_blink_epoch = Instant::now();
        self.cursor_blink_task = Some(cx.spawn(
            async |this: WeakEntity<Block>, cx: &mut AsyncApp| loop {
                cx.background_executor()
                    .timer(Duration::from_millis(33))
                    .await;
                if this
                    .update(cx, |this: &mut Block, cx: &mut Context<Block>| {
                        if this.cursor_blink_epoch.elapsed().as_secs_f32() >= 0.5 {
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            },
        ));
    }

    /// Cosine-based smooth blink: fully opaque for 0.5s, then oscillates
    /// with a period of ~1s (33ms x 30 ticks ~= 1s).
    pub fn cursor_opacity(&self) -> f32 {
        let elapsed = self.cursor_blink_epoch.elapsed().as_secs_f32();
        if elapsed < 0.5 {
            return 1.0;
        }
        let t = elapsed - 0.5;
        (f32::cos(t * std::f32::consts::TAU) + 1.0) / 2.0
    }

    pub fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    pub(crate) fn end_pointer_selection_session(&mut self) -> bool {
        let changed = self.is_selecting || self.code_language_is_selecting;
        self.is_selecting = false;
        self.code_language_is_selecting = false;
        changed
    }

    pub(crate) fn selection_anchor_focus(&self) -> (usize, usize) {
        if self.selection_reversed {
            (self.selected_range.end, self.selected_range.start)
        } else {
            (self.selected_range.start, self.selected_range.end)
        }
    }

    pub(crate) fn clean_selection_anchor_focus(&self) -> (usize, usize) {
        let (anchor, focus) = self.selection_anchor_focus();
        (
            self.current_to_clean_offset(anchor),
            self.current_to_clean_offset(focus),
        )
    }

    pub(crate) fn set_selection_from_anchor_focus(&mut self, anchor: usize, focus: usize) {
        let clamped_anchor = anchor.min(self.visible_len());
        let clamped_focus = focus.min(self.visible_len());
        self.selected_range = clamped_anchor.min(clamped_focus)..clamped_anchor.max(clamped_focus);
        self.selection_reversed = !self.selected_range.is_empty() && clamped_focus < clamped_anchor;
    }

    pub(crate) fn set_selection_from_clean_anchor_focus(
        &mut self,
        anchor: usize,
        focus: usize,
        anchor_affinity: CollapsedCaretAffinity,
        focus_affinity: CollapsedCaretAffinity,
    ) {
        // Map each endpoint back through its own affinity. Several display
        // positions can share one clean offset (a trailing link's `](url)`
        // delimiters all collapse onto the anchor-text end), so the plain
        // clean->display cursor map would snap an endpoint that sat after the
        // closing delimiter back to just inside it. Honoring the captured
        // affinity keeps such endpoints in place across a projection rebuild.
        self.set_selection_from_anchor_focus(
            self.clean_to_current_cursor_offset_with_affinity(anchor, anchor_affinity),
            self.clean_to_current_cursor_offset_with_affinity(focus, focus_affinity),
        );
    }

    pub fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.move_to_with_preferred_x(offset, None, cx);
    }

    pub fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let clamped_offset = offset.min(self.visible_len());
        if self.selection_reversed {
            self.selected_range.start = clamped_offset;
        } else {
            self.selected_range.end = clamped_offset;
        }
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        self.cursor_blink_epoch = Instant::now();
        self.clear_vertical_motion();
        self.sync_collapsed_caret_affinity();
        cx.notify();
    }

    pub(crate) fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        Self::utf8_range_to_utf16_in(self.display_text(), range)
    }

    pub(crate) fn range_from_utf16(&self, range_utf16: &Range<usize>) -> Range<usize> {
        Self::utf16_range_to_utf8_in(self.display_text(), range_utf16)
    }

    pub fn previous_boundary(&self, offset: usize) -> usize {
        let text = self.display_text();
        let mut cursor = GraphemeCursor::new(offset.min(text.len()), text.len(), true);
        cursor.prev_boundary(text, 0).ok().flatten().unwrap_or(0)
    }

    pub fn next_boundary(&self, offset: usize) -> usize {
        let text = self.display_text();
        let mut cursor = GraphemeCursor::new(offset.min(text.len()), text.len(), true);
        cursor
            .next_boundary(text, 0)
            .ok()
            .flatten()
            .unwrap_or(text.len())
    }

    /// Offset of the start of the word before `offset`, or 0 if there is none.
    pub fn previous_word_start(&self, offset: usize) -> usize {
        let text = self.display_text();
        let offset = offset.min(text.len());
        text.unicode_word_indices()
            .map(|(start, _)| start)
            .take_while(|start| *start < offset)
            .last()
            .unwrap_or(0)
    }

    /// Offset of the start of the word after `offset`, or the text length if
    /// there is none.
    pub fn next_word_start(&self, offset: usize) -> usize {
        let text = self.display_text();
        let offset = offset.min(text.len());
        text.unicode_word_indices()
            .map(|(start, _)| start)
            .find(|start| *start > offset)
            .unwrap_or(text.len())
    }

    /// Reverse of `display_offset`: maps an expanded display offset
    /// back to the clean tree offset.
    pub(crate) fn unexpand_offset(&self, expanded: usize) -> usize {
        let Some(projection) = &self.projection else {
            return expanded;
        };
        projection
            .display_to_clean
            .get(expanded.min(projection.display_to_clean.len().saturating_sub(1)))
            .copied()
            .unwrap_or(expanded)
    }

    pub fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        if self.display_text().is_empty() {
            return 0;
        }

        let (Some(bounds), Some(lines)) = (self.last_bounds.as_ref(), self.last_layout.as_ref())
        else {
            return 0;
        };

        if position.y < bounds.top() {
            return 0;
        }
        if position.y > bounds.bottom() {
            return self.visible_len();
        }

        let text = self.display_text();
        let ranges = crate::components::block::element::hard_line_ranges(text);
        let relative_y = position.y - bounds.top();
        let Some((line_idx, y_in_line)) = crate::components::block::element::wrapped_line_for_y(
            lines,
            self.last_line_height,
            relative_y,
        ) else {
            return 0;
        };
        let layout = &lines[line_idx];
        let origin_x = crate::components::block::element::aligned_line_left(
            layout,
            *bounds,
            self.text_align(),
        );

        let offset_in_line = match layout.closest_index_for_position(
            point(position.x - origin_x, y_in_line),
            self.last_line_height,
        ) {
            Ok(idx) | Err(idx) => idx,
        };
        // 字形几何反推的字节偏移可能落在多字节字符内部（中文文本里很常见）。
        // 这个偏移会进选择范围，之后按它切片（状态栏选词统计、复制）就会
        // panic；release 下 panic = abort，即用户报的 coredump。就地收敛。
        clamp_to_char_boundary(text, ranges[line_idx].start + offset_in_line)
    }

    pub(crate) fn active_range_or_cursor_bounds(&self) -> Option<Bounds<Pixels>> {
        let active_range = self
            .marked_range
            .clone()
            .unwrap_or_else(|| self.selected_range.clone());
        if active_range.is_empty() {
            let bounds = self.last_bounds?;
            let lines = self.last_layout.as_ref()?;
            return crate::components::block::element::cursor_bounds_for_offset(
                lines,
                bounds,
                self.last_line_height,
                self.display_text(),
                self.cursor_offset(),
                self.text_align(),
                px(1.0),
            );
        }
        self.visible_range_bounds(active_range)
    }

    /// 量一段**可见文本**坐标里的区间在屏幕上的外接框（窗口绝对像素坐标）。
    /// 跨块选区要逐块量再并起来，选中工具栏的锚点就是这么来的。
    pub(crate) fn visible_range_bounds(
        &self,
        range: std::ops::Range<usize>,
    ) -> Option<Bounds<Pixels>> {
        let bounds = self.last_bounds?;
        let lines = self.last_layout.as_ref()?;
        crate::components::block::element::range_bounds(
            lines,
            bounds,
            self.last_line_height,
            self.display_text(),
            range,
            self.text_align(),
        )
    }
}
