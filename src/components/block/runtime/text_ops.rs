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

    /// 改 IME 组合跨度（marked range）的唯一出口：值变了就发 `CompositionChanged`。
    ///
    /// 这条信号全仓只有一个消费者（`editor/events/block_event.rs` 的
    /// `CompositionChanged` 臂 → `Editor::finish_ime_composition` + 排自动保存），
    /// 它按「块上还有没有 marked 文本」判断该不该收尾，所以凡是改动跨度的编辑路径都
    /// 从这里走。少发一处的后果不是「少一次重绘」：编辑器那侧的撤销组会一直开着，
    /// 下一次编辑的字节并进这个 `ImeCompositionCommit` 组，一次撤销退两步。
    pub(crate) fn set_marked_range(&mut self, next: Option<Range<usize>>, cx: &mut Context<Self>) {
        if self.marked_range == next {
            return;
        }
        self.marked_range = next;
        cx.emit(BlockEvent::CompositionChanged);
        cx.notify();
    }

    pub(crate) fn convert_to_paragraph(&mut self, cx: &mut Context<Self>) {
        self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
        if self.record.kind.is_code_block() {
            self.record.clear_code_family_books();
        }
        self.record.kind = BlockKind::Paragraph;
        self.record.math_body_only = false;
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
        cx: &mut Context<Self>,
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
            self.record.clear_code_family_books();
        }
        self.record.kind = next;
        self.record.math_body_only = false;
        self.record.raw_fallback = None;
        self.record.raw_fallback = None;
        self.quote_reparse_requested = false;
        self.clear_inline_projection();
        self.sync_edit_mode_from_kind();
        self.sync_render_cache();
        // 编辑模式切换意味着显示坐标系整体变了：选区两端按新长度收敛、
        // 光标亲和重置，IME 标记作废；与 `enter_code_block` / 转分隔线同收尾。
        let len = self.visible_len();
        self.selected_range =
            self.selected_range.start.min(len)..self.selected_range.end.min(len);
        self.collapsed_caret_affinity = CollapsedCaretAffinity::Default;
        self.set_marked_range(None, cx);
        true
    }

    /// 这一块在文档里顶格：不在引用、标注、列表项里。
    ///
    /// 「模型只留正文」的公式块只在这一层成立：容器里的块文本要与文件行一一对齐，
    /// 位置换算靠的正是这份对齐（见 [`BlockRecord::math_body_only`]）。
    pub(crate) fn sits_at_root_level(&self) -> bool {
        self.quote_depth == 0 && self.callout_depth == 0 && !self.is_direct_list_child()
    }

    pub(crate) fn enter_code_block(
        &mut self,
        language: Option<SharedString>,
        cx: &mut Context<Self>,
    ) {
        self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
        self.clear_inline_projection();
        self.record.clear_code_family_books();
        self.record.kind = BlockKind::CodeBlock { language };
        self.record.math_body_only = false;
        self.record.raw_fallback = None;
        self.record.set_title(InlineTextTree::plain(String::new()));
        self.quote_reparse_requested = false;
        self.sync_edit_mode_from_kind();
        self.sync_render_cache();
        self.assign_collapsed_selection_offset(0, CollapsedCaretAffinity::Default, None);
        self.set_marked_range(None, cx);
        self.cursor_blink_epoch = Instant::now();
        self.clear_vertical_motion();
        cx.emit(BlockEvent::Changed);
        cx.notify();
    }

    /// Convert the current paragraph into a display-math block. `body` becomes
    /// the formula source between the fences (empty for a fresh `$$` block), and
    /// the caret lands at the start of that body line.
    ///
    /// 顶格的块模型里只留正文（两行围栏归结构，见 [`BlockRecord::math_body`]）：
    /// 光标就是正文里的 0 位，用户也改不到定界符。
    pub(crate) fn enter_math_block(&mut self, body: &str, cx: &mut Context<Self>) {
        // 容器里的块只能整段存原文：那边的模型文本要与文件行一一对齐（见
        // `math_body_or_raw_block`），光标落在两行围栏之间。
        let body_only = self.sits_at_root_level();
        let source = if body_only {
            body.to_string()
        } else if body.trim().is_empty() {
            "$$\n\n$$".to_string()
        } else {
            format!("$$\n{body}\n$$")
        };
        let cursor = if body_only { 0 } else { "$$\n".len() };

        self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
        self.clear_inline_projection();
        self.record.kind = BlockKind::MathBlock;
        self.record.math_body_only = body_only;
        self.record.set_title(InlineTextTree::plain(source));
        self.quote_reparse_requested = false;
        self.sync_edit_mode_from_kind();
        self.sync_render_cache();
        self.assign_collapsed_selection_offset(cursor, CollapsedCaretAffinity::Default, None);
        self.set_marked_range(None, cx);
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

    /// 把这一段**可见文本**包成 `[文字]()`：包完光标停在括号中间，等用户写地址。
    /// 没有选中文字时不做任何事，链接命令只包装已有的选区。
    /// 区间用屏幕上的坐标，与 [`Self::toggle_inline_format_in_range`] 同一个口径。
    pub(crate) fn wrap_visible_range_in_link(
        &mut self,
        range: Range<usize>,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.is_read_only() {
            return false;
        }

        if range.is_empty() || self.uses_raw_text_editing() {
            return false;
        }
        let text = self.display_text().to_string();
        let range =
            crate::components::markdown::inline::clamp_range_to_char_boundaries(&text, range);
        if range.is_empty() {
            return false;
        }
        let selected = text[range.clone()].to_string();
        // 开组与记账沿用 `Changed` 那一条路：块只管改自己，缓冲区写回与撤销收口在编辑器。
        self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
        // 落点在 `[文字](` 之后，紧挨着待写的地址。
        let caret = range.start + 1 + selected.len() + 2;
        self.replace_text_in_visible_range(range, &format!("[{selected}]()"), None, false, cx);
        self.assign_collapsed_selection_offset(caret, CollapsedCaretAffinity::Default, None);
        cx.notify();
        true
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
        if self.is_read_only() {
            return false;
        }

        if selection.is_empty() || self.uses_raw_text_editing() {
            return false;
        }

        let selection = self.current_to_clean_range(selection);
        let mut next_title = self.record.title.clone();
        // 强调写法跟着块记：选区内已有写法取多数派，否则沿用这一块上次用过的；
        // 两者都没有才退回规范星号。
        let block_preference = self
            .preferred_emphasis_marker
            .or_else(|| self.record.title.emphasis_marker_preference());
        if let Some(marker) = block_preference {
            self.preferred_emphasis_marker = Some(marker);
        }
        let emphasis_marker = next_title
            .emphasis_marker_majority(selection.clone())
            .or(block_preference);
        let changed = match format {
            InlineFormat::Bold => next_title.toggle_emphasis_with_marker(
                selection.clone(),
                crate::components::markdown::inline::StyleFlag::Bold,
                emphasis_marker,
            ),
            InlineFormat::Italic => next_title.toggle_emphasis_with_marker(
                selection.clone(),
                crate::components::markdown::inline::StyleFlag::Italic,
                emphasis_marker,
            ),
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
        self.apply_title_edit_preserving_kind(
            next_title,
            selection.end,
            None,
            Some(selection),
            reversed,
            None,
            cx,
        );
        true
    }

    /// 在当前选区上剥掉行内样式记号：菜单与快捷键的「清除格式」走这里。
    pub(crate) fn clear_inline_format(&mut self, cx: &mut Context<Self>) -> bool {
        if self.selected_range.is_empty() || self.uses_raw_text_editing() {
            return false;
        }

        let reversed = Some(self.selection_reversed);
        self.clear_inline_styles_in_range(self.selected_range.clone(), reversed, cx)
    }

    /// 在指定的**可见文本**区间上剥掉行内样式记号。坐标口径与
    /// [`Self::toggle_inline_format_in_range`] 一致：调用方给屏幕上的坐标，块自己换算到树里。
    /// 返回是否真剥掉了什么。
    pub(crate) fn clear_inline_styles_in_range(
        &mut self,
        selection: Range<usize>,
        reversed: Option<bool>,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.is_read_only() {
            return false;
        }

        if selection.is_empty() || self.uses_raw_text_editing() {
            return false;
        }

        let selection = self.current_to_clean_range(selection);
        let mut next_title = self.record.title.clone();
        if !next_title.clear_styles_in_range(selection.clone()) {
            return false;
        }

        self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
        self.apply_title_edit_preserving_kind(
            next_title,
            selection.end,
            None,
            Some(selection),
            reversed,
            None,
            cx,
        );
        true
    }

    /// 这一块在当前选区上有没有可剥的行内样式。菜单那一行与工具栏那颗格子的置灰判定读这一条，
    /// 坐标换算与 [`Self::clear_inline_format`] 走同一个（屏幕坐标 → `current_to_clean_range` →
    /// 树内坐标），所以不会出现「灰着却点得动」或「亮着却点不动」。
    pub(crate) fn has_inline_styles_in_selection(&self) -> bool {
        if self.selected_range.is_empty() || self.uses_raw_text_editing() {
            return false;
        }
        self.has_inline_styles_in_range(self.selected_range.clone())
    }

    /// 指定的一段**可见文本**上有没有可剥的行内样式；跨块选区由 `Editor` 逐块切段后调这里。
    pub(crate) fn has_inline_styles_in_range(&self, selection: Range<usize>) -> bool {
        if self.uses_raw_text_editing() {
            return false;
        }
        let selection = self.current_to_clean_range(selection);
        self.record.title.has_styles_in_range(selection)
    }

    fn current_line_layout_and_offset(&self) -> Option<(&WrappedLine, usize)> {
        let lines = self.last_layout.as_deref()?;
        let ranges = self.shared_hard_line_ranges();
        let (line_idx, offset_in_line) =
            crate::components::block::element::line_index_for_offset(&ranges, self.cursor_offset());
        Some((lines.get(line_idx)?, offset_in_line))
    }

    fn row_alignment_offset(&self, line: &WrappedLine, relative_y: Pixels) -> Pixels {
        let Some(bounds) = self.last_bounds else {
            return px(0.0);
        };
        crate::components::block::element::aligned_row_left(
            line, bounds, self.text_align(), self.last_line_height, relative_y,
        ) - bounds.left()
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
                        .map(|position| position.x + self.row_alignment_offset(layout, position.y))
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
        let Some(target) = self.vertical_target_offset(direction, preferred_x) else {
            return false;
        };
        self.move_to_with_preferred_x(target, Some(preferred_x), cx);
        true
    }

    /// 纵向移动的落点（块内）。光标已在首/末视觉行时返回 `None`，由调用方交给
    /// 编辑器换块；Shift+↑/↓ 与普通移动共用这一份判据。
    pub(crate) fn vertical_target_offset(
        &self,
        direction: i32,
        preferred_x: Pixels,
    ) -> Option<usize> {
        let lines = self.last_layout.as_deref()?;

        let text = self.display_text();
        let ranges = self.shared_hard_line_ranges();
        let (current_line_idx, offset_in_line) =
            crate::components::block::element::line_index_for_offset(&ranges, self.cursor_offset());
        let current_layout = lines.get(current_line_idx)?;
        let current_position = crate::components::block::element::position_for_offset(
            current_layout,
            offset_in_line,
            self.last_line_height,
            true,
        )?;

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
            return None;
        }

        let total_height = lines.iter().fold(px(0.0), |height, line| {
            height
                + crate::components::block::element::wrapped_line_height(
                    line,
                    self.last_line_height,
                )
        });
        if target_y >= total_height {
            return None;
        }

        let (target_line_idx, target_y_in_line) =
            crate::components::block::element::wrapped_line_for_y(
                lines,
                self.last_line_height,
                target_y,
            )?;
        let target_layout = &lines[target_line_idx];
        let target_point = point(
            preferred_x - self.row_alignment_offset(target_layout, target_y_in_line),
            target_y_in_line,
        );
        let target_offset_in_line =
            match target_layout.closest_index_for_position(target_point, self.last_line_height) {
                Ok(idx) | Err(idx) => idx,
            };

        // 行表（last_layout，上一帧）与字节行表（当前文本）可能错配：被跳过
        // 绘制的帧里改过文本时行数对不上，旧行号裸下标会越界，钳回有效行。
        let line_range = ranges
            .get(target_line_idx)
            .or_else(|| ranges.last())
            .cloned()
            .unwrap_or(0..0);
        Some(crate::components::markdown::inline::clamp_to_char_boundary(
            text, line_range.start + target_offset_in_line,
        ))
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
        let fallback = if prefer_last_line {
            self.visible_len()
        } else {
            0
        };
        let Some(lines) = self.last_layout.as_deref() else {
            return fallback;
        };
        if lines.is_empty() {
            return fallback;
        }

        let text = self.display_text();
        let ranges = self.shared_hard_line_ranges();
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

        let target_x = target_x - self.row_alignment_offset(target_layout, target_y);
        let offset_in_line = match target_layout
            .closest_index_for_position(point(target_x, target_y), self.last_line_height)
        {
            Ok(idx) | Err(idx) => idx,
        };
        let line_range = ranges
            .get(target_line_idx)
            .or_else(|| ranges.last())
            .cloned()
            .unwrap_or(0..0);
        crate::components::markdown::inline::clamp_to_char_boundary(
            text, line_range.start + offset_in_line,
        )
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

    /// 用户主动移标/选择时把 IME 组合跨度作废：replace_text_in_range 在不带
    /// 范围时回落到 marked_range，过期的跨度会把字写回已离开的区间。清点放在
    /// move_to/select_to/select_word_at 三个用户意图入口——不放
    /// assign_collapsed_selection_offset 那层，IME 自己刚挂上的跨度也走那条
    /// 路，在那层清会把它抹掉。
    pub(crate) fn drop_stale_ime_marked_range(&mut self, cx: &mut Context<Self>) {
        self.set_marked_range(None, cx);
    }
    /// 光标所在**硬行**的显示范围：Home/End 四条入口与 Up/Down 同源判据
    /// （shared_hard_line_ranges + line_index_for_offset）。行表非空恒成立，
    /// 越界兜底退整块范围。
    pub(crate) fn cursor_line_display_range(&self) -> std::ops::Range<usize> {
        let ranges = self.shared_hard_line_ranges();
        let (line_idx, _) = crate::components::block::element::line_index_for_offset(
            &ranges,
            self.cursor_offset(),
        );
        ranges
            .get(line_idx)
            .cloned()
            .unwrap_or(0..self.visible_len())
    }

    /// 光标所在**视觉行**的显示区间。软换行后 Home/End 要对齐这一行，而不是
    /// 整个硬行：竖屏窄窗口里长段落一按 Home 会跨过上面几行回到段落开头。
    /// 没有排版（未聚焦）或这一硬行没有折行时退回硬行区间。
    pub(crate) fn cursor_visual_row_display_range(&self) -> std::ops::Range<usize> {
        let hard = self.cursor_line_display_range();
        let Some(lines) = self.last_layout.as_deref() else {
            return hard;
        };
        let ranges = self.shared_hard_line_ranges();
        let (line_idx, _) = crate::components::block::element::line_index_for_offset(
            &ranges,
            self.cursor_offset(),
        );
        let Some(line) = lines.get(line_idx) else {
            return hard;
        };
        let local = self.cursor_offset().saturating_sub(hard.start);
        let Some((row_start, row_end)) =
            crate::components::block::element::wrapped_row_range_for_offset(line, local)
        else {
            return hard;
        };
        hard.start + row_start..hard.start + row_end
    }

    pub fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.drop_stale_ime_marked_range(cx);
        self.move_to_with_preferred_x(offset, None, cx);
    }

    pub fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.drop_stale_ime_marked_range(cx);
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

        let (Some(bounds), Some(lines)) = (self.last_bounds.as_ref(), self.last_layout.as_deref())
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
        let ranges = self.shared_hard_line_ranges();
        let relative_y = position.y - bounds.top();
        let Some((line_idx, y_in_line)) = crate::components::block::element::wrapped_line_for_y(
            lines,
            self.last_line_height,
            relative_y,
        ) else {
            return 0;
        };
        let layout = &lines[line_idx];
        let origin_x = crate::components::block::element::aligned_row_left(
            layout,
            *bounds,
            self.text_align(),
            self.last_line_height,
            y_in_line,
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
        // 行表（last_layout）与字节行表错配时钳回有效行，不拿旧行号裸下标。
        let line_range = ranges
            .get(line_idx)
            .or_else(|| ranges.last())
            .cloned()
            .unwrap_or(0..0);
        clamp_to_char_boundary(text, line_range.start + offset_in_line)
    }

    pub(crate) fn active_range_or_cursor_bounds(&self) -> Option<Bounds<Pixels>> {
        let active_range = self
            .marked_range
            .clone()
            .unwrap_or_else(|| self.selected_range.clone());
        if active_range.is_empty() {
            let bounds = self.last_bounds?;
            let lines = self.last_layout.as_deref()?;
            let hard_ranges = self.shared_hard_line_ranges();
            return crate::components::block::element::cursor_bounds_for_offset(
                lines,
                bounds,
                self.last_line_height,
                &hard_ranges,
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
        let lines = self.last_layout.as_deref()?;
        let hard_ranges = self.shared_hard_line_ranges();
        crate::components::block::element::range_bounds(
            lines,
            bounds,
            self.last_line_height,
            hard_ranges.as_slice(),
            range,
            self.text_align(),
        )
    }
}
