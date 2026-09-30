use super::*;

impl Block {
    pub(crate) fn on_end(&mut self, _: &End, _window: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.visible_len(), cx);
    }

    pub(crate) fn on_select_left(
        &mut self,
        _: &SelectLeft,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some((target, _)) = self.projected_move_left_target(self.cursor_offset()) {
            self.select_to(target, cx);
        } else {
            self.select_to(self.previous_boundary(self.cursor_offset()), cx);
        }
    }

    pub(crate) fn on_select_right(
        &mut self,
        _: &SelectRight,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some((target, _)) = self.projected_move_right_target(self.cursor_offset()) {
            self.select_to(target, cx);
        } else {
            self.select_to(self.next_boundary(self.cursor_offset()), cx);
        }
    }

    pub(crate) fn on_word_move_left(
        &mut self,
        _: &WordMoveLeft,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_to(self.previous_word_start(self.cursor_offset()), cx);
    }

    pub(crate) fn on_word_move_right(
        &mut self,
        _: &WordMoveRight,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_to(self.next_word_start(self.cursor_offset()), cx);
    }

    pub(crate) fn on_word_select_left(
        &mut self,
        _: &WordSelectLeft,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_to(self.previous_word_start(self.cursor_offset()), cx);
    }

    pub(crate) fn on_word_select_right(
        &mut self,
        _: &WordSelectRight,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_to(self.next_word_start(self.cursor_offset()), cx);
    }

    pub(crate) fn on_block_up(
        &mut self,
        _: &BlockUp,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.emit(BlockEvent::RequestBlockUp);
    }

    pub(crate) fn on_block_down(
        &mut self,
        _: &BlockDown,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.emit(BlockEvent::RequestBlockDown);
    }

    fn select_all_text(&mut self, cx: &mut Context<Self>) {
        self.move_to(0, cx);
        self.select_to(self.visible_len(), cx);
    }

    pub(crate) fn on_select_all(
        &mut self,
        _: &SelectAll,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.show_source_line_numbers() {
            self.select_all_text(cx);
        } else {
            cx.emit(BlockEvent::RequestRenderedSelectAll);
        }
    }

    pub(crate) fn on_select_home(
        &mut self,
        _: &SelectHome,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_to(0, cx);
    }

    pub(crate) fn on_select_end(
        &mut self,
        _: &SelectEnd,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_to(self.visible_len(), cx);
    }

    pub(crate) fn on_copy(&mut self, _: &Copy, _window: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            // 选择偏移理论上已收敛到字符边界，但文本变更后可能失效；切片前再夹一次。
            let text = self.display_text();
            let range = clamp_range_to_char_boundaries(text, self.selected_range.clone());
            let selected = text[range].to_string();
            cx.write_to_clipboard(ClipboardItem::new_string(selected));
        }
    }

    pub(crate) fn on_cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
            let text = self.display_text();
            let range = clamp_range_to_char_boundaries(text, self.selected_range.clone());
            let selected = text[range].to_string();
            cx.write_to_clipboard(ClipboardItem::new_string(selected));
            self.replace_text_in_range(None, "", window, cx);
        }
    }

    /// 把选中文本变成指向 `url` 的链接（B5 的可测试入口）。
    pub(crate) fn paste_url_as_link(
        &mut self,
        url: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.editor_selection_range.is_some() || self.selected_range.is_empty() {
            return;
        }
        let text = self.display_text();
        let range = clamp_range_to_char_boundaries(text, self.selected_range.clone());
        let selected = text[range].to_string();
        self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
        self.replace_text_in_visible_range(
            self.selected_range.clone(),
            &format!("[{selected}]({url})"),
            None,
            false,
            cx,
        );
        cx.notify();
        let _ = window;
    }

    /// Whether the pasted text is a bare http(s) URL.
    fn is_bare_url(value: &str) -> bool {
        !value.is_empty()
            && !value.chars().any(char::is_whitespace)
            && (value.starts_with("http://") || value.starts_with("https://"))
    }

    pub(crate) fn on_paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if self.kind().is_separator() && !self.uses_raw_text_editing() {
            return;
        }

        if let Some(item) = cx.read_from_clipboard() {
            if let Some(source) = Self::pasted_image_source_from_clipboard(&item) {
                let (leading, trailing) = self.paste_image_split();
                cx.emit(BlockEvent::RequestPasteImage {
                    leading,
                    source,
                    trailing,
                });
                return;
            }

            let Some(text) = item.text() else {
                return;
            };
            // 选中文本后粘贴 URL → 生成 [选中](url) 链接（roadmap B5）。
            let trimmed = text.trim();
            if Self::is_bare_url(trimmed)
                && self.editor_selection_range.is_none()
                && !self.selected_range.is_empty()
            {
                self.paste_url_as_link(trimmed, window, cx);
                return;
            }
            // Clipboard HTML flavors convert to Markdown here (roadmap B3);
            // plain-text clipboards pass through untouched.
            #[cfg(target_os = "macos")]
            let text = crate::components::markdown::html_paste::maybe_markdown_from_clipboard(&text);
            if let Some(source) = Self::pasted_image_source_from_text(&text) {
                let (leading, trailing) = self.paste_image_split();
                cx.emit(BlockEvent::RequestPasteImage {
                    leading,
                    source,
                    trailing,
                });
                return;
            }

            // Only rendered rich-text blocks apply paste correction. Raw/code
            // contexts preserve bytes, and table cells flatten newlines so the
            // surrounding table structure is not accidentally split.
            if self.editor_selection_range.is_some() {
                cx.emit(BlockEvent::RequestReplaceCrossBlockSelection {
                    text,
                    selected_range_relative: None,
                    mark_inserted_text: false,
                    undo_kind: UndoCaptureKind::NonCoalescible,
                });
                return;
            }

            if self.is_table_cell() {
                let flattened = text.replace("\r\n", " ").replace(['\r', '\n'], " ");
                self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
                self.replace_text_in_range(None, &flattened, window, cx);
                return;
            }

            if self.uses_raw_text_editing() {
                self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
                self.replace_text_in_range(None, &text, window, cx);
                return;
            }

            if text.contains('\n') || text.contains('\r') {
                let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
                if self.quote_depth > 0 {
                    self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
                    self.replace_text_in_range(None, &normalized, window, cx);
                    return;
                }
                let clean_selected = self.selection_clean_range();
                let (leading, tail) = self.record.title.split_at(clean_selected.start);
                let (_, trailing) =
                    tail.split_at(clean_selected.end.saturating_sub(clean_selected.start));
                let lines = normalized
                    .split('\n')
                    .map(ToOwned::to_owned)
                    .collect::<Vec<_>>();
                let split_physical_lines = should_split_plain_multiline_paste(&lines);
                cx.emit(BlockEvent::RequestPasteMultiline {
                    leading,
                    lines,
                    trailing,
                    split_physical_lines,
                });
                return;
            }

            self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
            self.replace_text_in_range(None, &text, window, cx);
        }
    }

    /// 是超长行时切换展开态。返回是否发生了切换。
    pub(crate) fn toggle_long_line_at_gutter(&mut self, position: Point<Pixels>) -> bool {
        let (Some(bounds), Some(lines)) = (self.last_bounds.as_ref(), self.last_layout.as_ref())
        else {
            return false;
        };
        if self.last_gutter_width <= px(0.0) {
            return false;
        }
        // 行号槽区间 = [text_bounds.left - gutter, text_bounds.left]。
        if position.x < bounds.left() - self.last_gutter_width || position.x > bounds.left() {
            return false;
        }
        let relative_y = position.y - bounds.top();
        if relative_y < px(0.0) {
            return false;
        }
        let Some((line_idx, _)) =
            crate::components::block::element::wrapped_line_for_y(lines, self.last_line_height, relative_y)
        else {
            return false;
        };
        self.toggle_long_line_expanded(line_idx)
    }

    pub(crate) fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // 行号槽点击：切换超长行的折叠/展开。要在落光标之前截住，
        // 否则点行号会把光标塞进行首。
        if self.long_line_folding_enabled() && self.toggle_long_line_at_gutter(event.position) {
            cx.notify();
            cx.stop_propagation();
            return;
        }

        if self.showing_rendered_image() {
            self.is_selecting = false;
            self.request_image_edit_expansion();
            if self.focus_handle.is_focused(window) {
                if self.sync_image_focus_state(true) {
                    cx.notify();
                }
            } else {
                cx.emit(BlockEvent::RequestFocus);
            }
            cx.stop_propagation();
            return;
        }

        let offset = self.index_for_mouse_position(event.position);
        let was_focused = self.focus_handle.is_focused(window);

        // Cmd/Ctrl+click follows a rendered link instead of editing it, so the
        // block is neither focused nor selected; the link opens on mouse-up.
        if event.modifiers.secondary() && self.pointer_link_hit(event.position).is_some() {
            self.is_selecting = false;
            cx.stop_propagation();
            return;
        }

        if event.click_count >= 2 && !event.modifiers.shift {
            // 双击选词（用户要求）：选中所点的字词段。聚焦与不聚焦两个
            // 分支都要处理——第一次单击只聚焦，第二次（已聚焦）才成词选。
            self.is_selecting = true;
            self.select_word_at(offset, cx);
            if !was_focused {
                cx.emit(BlockEvent::RequestFocus);
            }
            return;
        }

        if was_focused {
            self.is_selecting = true;
            if event.modifiers.shift {
                self.select_to(offset, cx);
            } else {
                self.move_to(offset, cx);
            }
        } else {
            self.is_selecting = false;
            self.move_to(offset, cx);
            cx.emit(BlockEvent::RequestFocus);
        }
    }

    /// 双击选词：选中 `offset` 所在的字词段（Unicode UAX#29 词边界：
    /// 空白与标点各自成段，CJK 按字素分组）。
    pub(crate) fn select_word_at(&mut self, offset: usize, cx: &mut Context<Self>) {
        use unicode_segmentation::UnicodeSegmentation;
        let text = self.display_text();
        if text.is_empty() {
            return;
        }
        let offset = offset.min(text.len());
        let mut chosen: Option<(usize, usize)> = None;
        for (start, segment) in text.split_word_bound_indices() {
            let end = start + segment.len();
            let contains = start <= offset && offset < end;
            let at_tail = offset == text.len() && end == text.len();
            if contains || at_tail {
                chosen = Some((start, end));
                break;
            }
        }
        let Some((start, end)) = chosen else {
            return;
        };
        self.selected_range = start..end;
        self.selection_reversed = false;
        cx.notify();
    }

    /// Resolve the inline link under a pointer position against the most recent
    /// rendered text layout, if any. Returns `None` while the block shows raw
    /// source or when the pointer is not over a link.
    pub(crate) fn pointer_link_hit(&self, position: Point<Pixels>) -> Option<super::super::super::InlineLinkHit> {
        self.last_layout
            .as_ref()
            .zip(self.last_bounds)
            .and_then(|(lines, bounds)| {
                crate::components::block::element::link_at_position(
                    self,
                    lines,
                    bounds,
                    self.last_line_height,
                    position,
                )
            })
            .cloned()
    }

    /// Handle mouse-down on a rendered inline link (in a mixed inline-visual
    /// block). A Cmd/Ctrl+click is claimed here so it follows the link instead
    /// of focusing the block; the destination opens on the matching mouse-up.
    pub(crate) fn on_rendered_link_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Only Cmd/Ctrl+click follows the link; a plain click falls through so
        // the block focuses for editing like any other inline text.
        if event.modifiers.secondary() {
            cx.stop_propagation();
        }
    }

    /// Open a rendered inline link's destination directly (no confirmation).
    pub(crate) fn open_rendered_link(
        &mut self,
        link: &super::super::super::InlineLinkHit,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        cx.emit(BlockEvent::RequestOpenLink {
            open_target: link.open_target.clone(),
        });
    }

    pub(crate) fn on_mouse_up(
        &mut self,
        event: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(drag) = self.image_resize_drag.take()
            && (self.image_width_factor - drag.base_factor).abs() > 0.005
        {
            self.write_image_width_back_to_source(cx);
        }
        self.is_selecting = false;

        // Cmd/Ctrl+click follows a rendered link, using the same open-link
        // prompt as the double-click gesture below.
        if event.modifiers.secondary()
            && let Some(link) = self.pointer_link_hit(event.position)
        {
            self.open_rendered_link(&link, cx);
            return;
        }

        if event.click_count >= 2 {
            let footnote = self
                .last_layout
                .as_ref()
                .zip(self.last_bounds)
                .and_then(|(lines, bounds)| {
                    crate::components::block::element::footnote_at_position(
                        self,
                        lines,
                        bounds,
                        self.last_line_height,
                        event.position,
                    )
                })
                .cloned();
            if let Some(footnote) = footnote {
                cx.stop_propagation();
                cx.emit(BlockEvent::RequestJumpToFootnoteDefinition { id: footnote.id });
                return;
            }

            if let Some(link) = self.pointer_link_hit(event.position) {
                self.open_rendered_link(&link, cx);
            }
        }
    }

    pub(crate) fn on_footnote_backref_mouse_down(
        &mut self,
        _: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        if !self.focus_handle.is_focused(window) {
            cx.emit(BlockEvent::RequestFocus);
        }
    }

    pub(crate) fn on_footnote_backref_mouse_up(
        &mut self,
        _: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = self.footnote_definition_id() else {
            return;
        };
        cx.stop_propagation();
        cx.emit(BlockEvent::RequestJumpToFootnoteBackref { id });
    }

    pub(crate) fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // 图片拖拽缩放进行中（roadmap C10）：水平位移换算为宽度因子。
        if let Some(drag) = self.image_resize_drag.as_mut() {
            let delta = f32::from(event.position.x) - drag.start_x;
            self.image_width_factor =
                (drag.base_factor + delta / 400.0).clamp(0.2, 1.0);
            cx.notify();
            return;
        }
        if self.is_selecting {
            // A stale selecting flag can survive a missed mouse-up. Only extend
            // the selection while the platform still reports an active drag.
            if !event.dragging() {
                self.is_selecting = false;
                cx.notify();
                return;
            }
            self.select_to(self.index_for_mouse_position(event.position), cx);
        }
    }

    /// 缩放结束后把宽度因子写回源码 `{width=NN%}`（roadmap C10 v2）；
    /// 100% 时移除属性以保持源码干净。
    pub(crate) fn write_image_width_back_to_source(&mut self, cx: &mut Context<Self>) {
        if self.image_runtime().is_none() {
            return;
        }
        let current = self.display_text().to_string();
        let (base, _) = crate::components::markdown::image::split_standalone_image_width(&current);
        let base = base.trim().to_string();
        if base.is_empty() {
            return;
        }
        let percent = (self.image_width_factor * 100.0).round().clamp(20.0, 100.0) as u32;
        let next = if percent >= 100 {
            base
        } else {
            format!("{base}{{width={percent}%}}")
        };
        if next == current {
            return;
        }
        self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
        let len = self.visible_len();
        self.replace_text_in_visible_range(0..len, &next, None, false, cx);
    }

    /// 「复制代码块内容」（roadmap B9）：写入剪贴板并短暂显示 ✓ 反馈。
    pub(crate) fn on_code_copy_button(
        &mut self,
        _: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        let code = self.record.title.visible_text();
        if code.is_empty() {
            return;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(code.to_string()));
        self.code_copied_at = Some(Instant::now());
        cx.notify();
        cx.spawn(async move |block, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(1200))
                .await;
            let _ = block.update(cx, |block, cx| {
                if block.code_copied_at.take().is_some() {
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(crate) fn on_task_checkbox_mouse_down(
        &mut self,
        _: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        if !self.focus_handle.is_focused(window) {
            cx.emit(BlockEvent::RequestFocus);
        }
    }

    pub(crate) fn on_task_checkbox_mouse_up(
        &mut self,
        _: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.kind().is_task_list_item() || self.is_source_raw_mode() {
            return;
        }

        cx.stop_propagation();
        cx.emit(BlockEvent::ToggleTaskChecked);
    }

    pub(crate) fn on_table_append_column_zone_hover(
        &mut self,
        hovered: &bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_table_append_column_hover_part(None, Some(*hovered), None, cx);
    }

    pub(crate) fn on_table_append_column_button_hover(
        &mut self,
        hovered: &bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_table_append_column_hover_part(None, None, Some(*hovered), cx);
    }

    pub(crate) fn on_table_append_row_zone_hover(
        &mut self,
        hovered: &bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_table_append_row_hover_part(None, Some(*hovered), None, cx);
    }

    pub(crate) fn on_table_append_row_button_hover(
        &mut self,
        hovered: &bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_table_append_row_hover_part(None, None, Some(*hovered), cx);
    }

    pub(crate) fn on_table_append_column_edge_hover(
        &mut self,
        hovered: &bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_table_append_column_hover_part(Some(*hovered), None, None, cx);
    }

    pub(crate) fn on_table_append_row_edge_hover(
        &mut self,
        hovered: &bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_table_append_row_hover_part(Some(*hovered), None, None, cx);
    }

    pub(crate) fn on_append_table_column(
        &mut self,
        _: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.kind() == BlockKind::Table {
            cx.emit(BlockEvent::RequestAppendTableColumn);
        }
    }

    pub(crate) fn on_append_table_row(
        &mut self,
        _: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.kind() == BlockKind::Table {
            cx.emit(BlockEvent::RequestAppendTableRow);
        }
    }

    pub(crate) fn on_bold_selection(
        &mut self,
        _: &BoldSelection,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_inline_format(InlineFormat::Bold, cx);
    }

    pub(crate) fn on_italic_selection(
        &mut self,
        _: &ItalicSelection,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_inline_format(InlineFormat::Italic, cx);
    }

    pub(crate) fn on_underline_selection(
        &mut self,
        _: &UnderlineSelection,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_inline_format(InlineFormat::Underline, cx);
    }

    pub(crate) fn on_code_selection(
        &mut self,
        _: &CodeSelection,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_inline_format(InlineFormat::Code, cx);
    }

    pub(crate) fn on_exit_code_block(
        &mut self,
        _: &ExitCodeBlock,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let exits_multiline_block = self.is_table_cell() || self.kind().is_multiline_text_block();

        if exits_multiline_block {
            cx.emit(BlockEvent::RequestNewline {
                trailing: InlineTextTree::plain(String::new()),
                source_already_mutated: false,
            });
        } else if self.callout_depth > 0 {
            cx.emit(BlockEvent::RequestCalloutBreak);
        } else if self.quote_depth > 0 {
            cx.emit(BlockEvent::RequestQuoteBreak);
        }
    }
}

