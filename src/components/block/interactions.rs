//! Action handlers dispatched by GPUI's action system when bound keys are
//! pressed on a focused block.  Each handler maps to a named action declared
//! in [`crate::components::actions`] and delegates structural changes to the
//! parent editor via `BlockEvent` emissions.

pub(super) use std::time::Instant;

pub(super) use gpui::*;

pub(super) use super::CollapsedCaretAffinity;
pub(super) use super::{
    Block, BlockEvent, BlockKind, InlineFormat, InlineTextTree, PastedImageSource, UndoCaptureKind,
};
pub(super) use crate::components::markdown::inline::clamp_range_to_char_boundaries;
use crate::components::{
    BlockDown, BlockUp, BoldSelection, CodeSelection, Copy, Cut, Delete, DeleteBack,
    HighlightSelection, StrikethroughSelection, SubscriptSelection, SuperscriptSelection,
    DismissTransientUi, End, ExitCodeBlock, FocusNext, FocusPrev, Home, IndentBlock,
    ItalicSelection, MoveLeft, MoveRight, Newline, OutdentBlock, Paste, PasteAsPlainText,
    SelectAll, SelectEnd, SelectHome, SelectLeft, SelectRight, UnderlineSelection,
    WordDeleteBack, WordDeleteForward, WordMoveLeft, WordMoveRight, WordSelectLeft,
    WordSelectRight,
};

impl Block {
    fn pasted_image_source_from_clipboard(item: &ClipboardItem) -> Option<PastedImageSource> {
        item.entries().iter().find_map(|entry| match entry {
            ClipboardEntry::Image(image) => Some(PastedImageSource::ClipboardImage(image.clone())),
            ClipboardEntry::String(_) | ClipboardEntry::ExternalPaths(_) => None,
        })
    }

    fn pasted_image_source_from_text(text: &str) -> Option<PastedImageSource> {
        let trimmed = text.trim();
        if trimmed.is_empty() || trimmed.contains('\n') || trimmed.contains('\r') {
            return None;
        }

        Self::pasted_image_path_from_text_item(trimmed).map(PastedImageSource::LocalPath)
    }

    /// Parses a single clipboard text item as a local image path.
    ///
    /// Windows file-copy paste reaches GPUI as a plain drive-letter path; that
    /// must be tested as a path before URL parsing, because `url::Url` treats
    /// the drive letter as a URL scheme.
    fn pasted_image_path_from_text_item(text: &str) -> Option<std::path::PathBuf> {
        let unquoted = text
            .strip_prefix('"')
            .and_then(|rest| rest.strip_suffix('"'))
            .unwrap_or(text);
        let direct_path = std::path::PathBuf::from(unquoted);
        let path = if Self::is_supported_local_image_path(&direct_path) {
            direct_path
        } else if let Ok(url) = url::Url::parse(unquoted) {
            if url.scheme() == "file" {
                url.to_file_path().ok()?
            } else {
                return None;
            }
        } else {
            return None;
        };
        if !Self::is_supported_local_image_path(&path) {
            return None;
        }
        Some(path)
    }

    pub(crate) fn is_supported_local_image_path(path: &std::path::Path) -> bool {
        path.is_file() && Self::has_supported_image_extension(path)
    }

    pub(crate) fn has_supported_image_extension(path: &std::path::Path) -> bool {
        let Some(ext) = path.extension().and_then(|ext| ext.to_str()) else {
            return false;
        };
        // 判据问的是真正把这张图解码画出来的组件，不在这里手抄扩展名表：
        // 抄过一份 9 项的窄表，同一张 .avif 写进 markdown 显示得出、导出内嵌得了，
        // 拖进窗口却被当成非图片忽略（与报修第 12 条同一族）。
        gpui::Img::extensions().contains(&ext.to_ascii_lowercase().as_str())
    }

    pub(crate) fn paste_image_split(&self) -> (InlineTextTree, InlineTextTree) {
        let clean_selected = self.selection_clean_range();
        let (leading, tail) = self.record.title.split_at(clean_selected.start);
        let (_, trailing) = tail.split_at(clean_selected.end.saturating_sub(clean_selected.start));
        (leading, trailing)
    }

    fn is_leaf_quote(&self) -> bool {
        self.kind() == BlockKind::Quote
            && self.children.is_empty()
            && !self.display_text().contains('\n')
    }

    fn is_leaf_callout(&self) -> bool {
        matches!(self.kind(), BlockKind::Callout(_)) && self.children.is_empty()
    }

    fn is_empty_leaf_quote(&self) -> bool {
        self.is_leaf_quote() && self.selected_range.is_empty() && self.is_empty()
    }

    fn downgrade_leaf_callout_to_quote_at_start(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.is_leaf_callout() || !self.selected_range.is_empty() || self.cursor_offset() != 0 {
            return false;
        }

        let BlockKind::Callout(variant) = self.kind() else {
            return false;
        };
        let header_markdown = variant.header_markdown(&self.record.title.serialize_markdown());
        self.record.kind = BlockKind::Quote;
        self.record
            .set_title(InlineTextTree::from_markdown(&header_markdown));
        self.sync_edit_mode_from_kind();
        self.sync_render_cache();
        self.assign_collapsed_selection_offset(0, CollapsedCaretAffinity::Default, None);
        self.set_marked_range(None, cx);
        self.cursor_blink_epoch = std::time::Instant::now();
        cx.emit(BlockEvent::Changed);
        cx.notify();
        true
    }

    fn downgrade_empty_leaf_quote_to_paragraph(&mut self, cx: &mut Context<Self>) -> bool {
        if self.is_empty_leaf_quote() {
            self.convert_to_paragraph(cx);
            return true;
        }
        false
    }

    /// If the code block's last line is a bare fence (three or more backticks
    /// or tildes, no info string), returns the byte offset to cut from so the
    /// whole line is removed; otherwise `None`.
    fn trailing_code_fence_line_start(&self) -> Option<usize> {
        let text = self.display_text();
        let line_start = text.rfind('\n').map(|idx| idx + 1).unwrap_or(0);
        let is_bare_fence = BlockKind::parse_code_fence_opening(&text[line_start..])
            .is_some_and(|fence| fence.language.is_none());
        // Cut from the preceding newline too, unless the fence is the only line.
        is_bare_fence.then(|| line_start.saturating_sub(1))
    }

    pub(crate) fn on_newline(&mut self, _: &Newline, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_read_only() {
            return;
        }

        // Enter is ordered from special editors to rich-text splitting:
        // table/source/code/quote-like blocks keep local newline semantics,
        // while normal rendered blocks emit an editor-level split request.
        if self.is_table_cell() {
            cx.emit(BlockEvent::RequestTableCellMoveVertical { delta: 1 });
            return;
        }

        // 归属判据只看全局那一处：局部 `editor_selection_clean_range` 在端点落在块首/
        // 块尾时是 `None`（干净切片为空就不铺高亮），拿它判断会让回车走本地拆块路径，
        // 整段选区没被替换就被结构事件清掉。
        if self.has_document_selection() {
            cx.emit(BlockEvent::RequestReplaceCrossBlockSelection {
                text: "\n".to_string(),
                selected_range_relative: None,
                mark_inserted_text: false,
                undo_kind: UndoCaptureKind::NonCoalescible,
            });
            return;
        }

        if self.is_source_raw_mode() {
            if !self.selected_range.is_empty() {
                self.replace_text_in_range(None, "", window, cx);
            }
            if self.cursor_offset() == self.visible_len() {
                // 源码分块文档：块尾的换行由块边界表示。交给编辑器在本块
                // 之后插一个空块（尾部块尾回车 = 追加文件末换行，语义相同）。
                cx.emit(BlockEvent::RequestNewline {
                    trailing: InlineTextTree::plain(String::new()),
                    source_already_mutated: false,
                });
                return;
            }
            self.replace_text_in_range(None, "\n", window, cx);
            return;
        }

        if self.kind() == BlockKind::Paragraph
            && self.selected_range.is_empty()
            && self.cursor_offset() == self.visible_len()
            && BlockKind::parse_separator_line(self.display_text())
            // A dash run is also a setext underline; defer it to the editor so a
            // preceding paragraph can become a heading (the editor falls back to
            // a separator when there is no heading target).
            && BlockKind::parse_setext_underline(self.display_text()).is_none()
        {
            self.convert_to_separator(cx);
            cx.emit(BlockEvent::RequestNewline {
                trailing: InlineTextTree::plain(String::new()),
                source_already_mutated: true,
            });
            return;
        }

        // `$$` then Enter opens a display-math block. Keying off the caret sitting
        // right after a leading `$$` (rather than the line being exactly `$$`)
        // means it also fires after pressing Home on an existing line and typing
        // the fence in front of a formula: the rest of the line becomes the math
        // body instead of being split off into a new paragraph.
        if self.kind() == BlockKind::Paragraph
            && self.selected_range.is_empty()
            && self.cursor_offset() == "$$".len()
            && self.display_text().starts_with("$$")
        {
            let body = self.display_text()["$$".len()..].to_string();
            self.enter_math_block(&body, cx);
            return;
        }

        if self.kind() == BlockKind::Paragraph
            && self.selected_range.is_empty()
            && self.cursor_offset() == self.visible_len()
            && let Some(fence) = BlockKind::parse_code_fence_opening(self.display_text())
        {
            self.enter_code_block(fence.language, cx);
            return;
        }

        if self.kind().is_separator() {
            cx.emit(BlockEvent::RequestNewline {
                trailing: InlineTextTree::plain(String::new()),
                source_already_mutated: false,
            });
            return;
        }

        if self.kind().is_list_item() && self.selected_range.is_empty() && self.is_empty() {
            cx.emit(BlockEvent::RequestOutdent);
            return;
        }

        if self.kind() == BlockKind::Quote {
            if !self.selected_range.is_empty() {
                self.replace_text_in_range(None, "", window, cx);
            }
            self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
            self.replace_text_in_range(None, "\n", window, cx);
            return;
        }

        if matches!(self.kind(), BlockKind::Callout(_)) {
            cx.emit(BlockEvent::RequestEnterCalloutBody);
            return;
        }

        // In a code block, Enter inserts a newline into the block content
        // rather than splitting the block.  Pressing Enter on an empty
        // code block exits back to a paragraph.
        if self.kind().is_code_block() {
            if self.selected_range.is_empty() && self.is_empty() {
                self.convert_to_paragraph(cx);
                return;
            }
            // Typing a bare closing fence on the last line and pressing Enter
            // leaves the block, matching source mode.
            if self.selected_range.is_empty()
                && self.cursor_offset() == self.visible_len()
                && let Some(fence_start) = self.trailing_code_fence_line_start()
            {
                let fence_end = self.visible_len();
                self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
                self.replace_text_in_visible_range(fence_start..fence_end, "", None, false, cx);
                cx.emit(BlockEvent::RequestNewline {
                    trailing: InlineTextTree::plain(String::new()),
                    source_already_mutated: true,
                });
                return;
            }
            if !self.selected_range.is_empty() {
                self.replace_text_in_range(None, "", window, cx);
            }
            self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
            self.replace_text_in_range(None, "\n", window, cx);
            return;
        }

        if self.collapsed_caret_inherits_inline_code_style() {
            self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
            self.replace_text_in_range(None, "\n", window, cx);
            return;
        }

        if !self.selected_range.is_empty() {
            self.replace_text_in_range(None, "", window, cx);
        }

        let cursor = self.cursor_offset();
        let (leading, trailing) = self.split_title(cursor);
        // 光标正好压在块内的那个换行上：那个换行就是这次拆块的接缝，两边都不该留着它
        // ——留着会让新块多一个空首行，文件里多一个空行，重新解析还多出一个空块。
        let trailing = if trailing.visible_text().starts_with('\n') {
            trailing.split_at(1).1
        } else {
            trailing
        };
        self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
        // 切下来的那半截属于紧跟着插入的新块：标出来，编辑器才不会把它写成一次删除。
        self.mark_split_truncation();
        self.record.set_title(leading);
        self.mark_changed(cx);
        let cursor = self.visible_len();
        self.assign_collapsed_selection_offset(cursor, CollapsedCaretAffinity::Default, None);
        self.set_marked_range(None, cx);
        cx.emit(BlockEvent::RequestNewline {
            trailing,
            source_already_mutated: true,
        });
    }

    /// 这次删除是否把整块的可见文本清空。
    ///
    /// 源码分块块类（公式、HTML、原始 markdown、mermaid）的字节就是整段 markdown 自己：
    /// 空块在文件里写不出对应字节，留下零宽空块会让块树与缓冲区对不上（读回来是别的块）。
    /// 这一族删空时改请编辑器把块收掉，与空代码块同一条规则（见 `on_delete_back`）。
    fn removal_clears_the_whole_block(&self, removed: &std::ops::Range<usize>) -> bool {
        removed.start == 0 && removed.end >= self.visible_len()
    }

    pub(crate) fn on_delete_back(
        &mut self,
        _: &DeleteBack,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_read_only() {
            return;
        }

        let cursor = self.cursor_offset();
        if self.selected_range.is_empty()
            && self.marked_range.is_none()
            && let (Some(start), Some(end)) = (cursor.checked_sub(1), cursor.checked_add(1))
            && self.math_brace_context_at(start)
            && self.display_text().get(start..end) == Some("{}")
        {
            self.prepare_undo_capture(self.text_edit_undo_kind(), cx);
            self.replace_text_in_visible_range(start..end, "", None, false, cx);
            return;
        }
        if self.is_table_cell() {
            if self.selected_range.is_empty() {
                let previous = self.previous_boundary(self.cursor_offset());
                if previous == self.cursor_offset() {
                    return;
                }
                self.prepare_undo_capture(self.text_edit_undo_kind(), cx);
                self.select_to(previous, cx);
            }
            self.replace_text_in_range(None, "", window, cx);
            return;
        }

        if self.is_source_raw_mode() {
            // 空的源码分块块（模型里一个字都没有，例如刚插进来的空公式块）再按退格就是
            // 「收掉这一块」——与空代码块同一条规则。这一条必须排在合并边界之前：排在后面
            // 就永远轮不到它，空块只会把光标送到上一块去。
            if self.selected_range.is_empty() && self.display_text().is_empty() {
                cx.emit(BlockEvent::RequestDelete);
                return;
            }
            if self.selected_range.is_empty() && self.cursor_offset() == 0 {
                // 源码分块文档：块首退格 = 删除与前一块之间的边界换行。
                cx.emit(BlockEvent::RequestMergeIntoPrev {
                    content: self.record.title.clone(),
                });
                return;
            }
            if self.selected_range.is_empty() {
                self.prepare_undo_capture(self.text_edit_undo_kind(), cx);
                self.select_to(self.previous_boundary(self.cursor_offset()), cx);
            }
            // 源码分块块类的字节就是整段 markdown 自己（围栏、`$$`、HTML 标签都在里面）：
            // 空块在文件里写不出对应字节，留下零宽空块会让块树与缓冲区对不上（读回来是别的块）。
            // 先按用户的意思把文本删掉（写回缓冲区），再请编辑器收掉这个空块。
            let clears_the_block = self.removal_clears_the_whole_block(&self.selected_range);
            self.replace_text_in_range(None, "", window, cx);
            if clears_the_block {
                cx.emit(BlockEvent::RequestDelete);
            }
            return;
        }

        if self.selected_range.is_empty() && self.cursor_offset() == 0 {
            if self.kind() == BlockKind::Paragraph && self.is_direct_list_child() && self.is_empty()
            {
                cx.emit(BlockEvent::RequestOutdent);
                return;
            }
            if self.is_nested_list_item() {
                cx.emit(BlockEvent::RequestDowngradeNestedListItemToChildParagraph);
                return;
            }
            match self.kind() {
                BlockKind::BulletedListItem
                | BlockKind::TaskListItem { .. }
                | BlockKind::NumberedListItem => {
                    cx.emit(BlockEvent::RequestOutdent);
                    return;
                }
                BlockKind::Heading { .. } => {
                    self.convert_to_paragraph(cx);
                    return;
                }
                BlockKind::Quote => {
                    if self.is_leaf_quote() {
                        self.convert_to_paragraph(cx);
                    }
                    return;
                }
                BlockKind::Callout(_) => {
                    if self.downgrade_leaf_callout_to_quote_at_start(cx) {
                        return;
                    }
                    return;
                }
                BlockKind::Separator => {
                    self.convert_to_paragraph(cx);
                    return;
                }
                BlockKind::CodeBlock { .. } => {
                    self.convert_to_paragraph(cx);
                    return;
                }
                _ => {}
            }
        }

        if self.downgrade_leaf_callout_to_quote_at_start(cx)
            || self.downgrade_empty_leaf_quote_to_paragraph(cx)
        {
            return;
        }

        if self.selected_range.is_empty() && self.display_text().is_empty() {
            cx.emit(BlockEvent::RequestDelete);
            return;
        }

        if self.selected_range.is_empty() && self.cursor_offset() == 0 {
            cx.emit(BlockEvent::RequestMergeIntoPrev {
                content: self.record.title.clone(),
            });
            return;
        }

        if self.selected_range.is_empty() {
            self.prepare_undo_capture(self.text_edit_undo_kind(), cx);
            self.select_to(self.previous_boundary(self.cursor_offset()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    pub(crate) fn on_delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_read_only() {
            return;
        }

        if self.is_table_cell() {
            if self.selected_range.is_empty() {
                let next = self.next_boundary(self.cursor_offset());
                if next == self.cursor_offset() {
                    return;
                }
                self.prepare_undo_capture(self.text_edit_undo_kind(), cx);
                self.select_to(next, cx);
            }
            self.replace_text_in_range(None, "", window, cx);
            return;
        }

        if self.is_source_raw_mode() {
            // 与 `on_delete_back` 同一条规则：空块的前向删除也是收掉这一块。
            if self.selected_range.is_empty() && self.display_text().is_empty() {
                cx.emit(BlockEvent::RequestDelete);
                return;
            }
            if self.selected_range.is_empty() && self.cursor_offset() == self.visible_len() {
                // 源码分块文档：块尾前向删除 = 删除与下一块之间的边界换行。
                cx.emit(BlockEvent::RequestMergeFromNext);
                return;
            }
            if self.selected_range.is_empty() {
                self.prepare_undo_capture(self.text_edit_undo_kind(), cx);
                self.select_to(self.next_boundary(self.cursor_offset()), cx);
            }
            // 与 `on_delete_back` 同一个口径：先删文本，再收掉被删空的块。
            let clears_the_block = self.removal_clears_the_whole_block(&self.selected_range);
            self.replace_text_in_range(None, "", window, cx);
            if clears_the_block {
                cx.emit(BlockEvent::RequestDelete);
            }
            return;
        }

        if self.downgrade_leaf_callout_to_quote_at_start(cx)
            || self.downgrade_empty_leaf_quote_to_paragraph(cx)
        {
            return;
        }

        if self.kind().is_separator() {
            self.convert_to_paragraph(cx);
            return;
        }

        if self.selected_range.is_empty() && self.display_text().is_empty() {
            cx.emit(BlockEvent::RequestDelete);
            return;
        }

        if self.selected_range.is_empty() && self.cursor_offset() == self.visible_len() {
            // 块尾前向删除与源码分块同一语义：把下一块并进来（删除边界换行）。
            cx.emit(BlockEvent::RequestMergeFromNext);
            return;
        }

        if self.selected_range.is_empty() {
            self.prepare_undo_capture(self.text_edit_undo_kind(), cx);
            self.select_to(self.next_boundary(self.cursor_offset()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    pub(crate) fn on_word_delete_back(
        &mut self,
        _: &WordDeleteBack,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_read_only() {
            return;
        }

        if self.selected_range.is_empty() {
            if self.cursor_offset() == 0 {
                // Nothing to the left in this block; defer to grapheme
                // backspace, which handles block merge and downgrades.
                self.on_delete_back(&DeleteBack, window, cx);
                return;
            }
            self.prepare_undo_capture(self.text_edit_undo_kind(), cx);
            self.select_to(self.previous_word_start(self.cursor_offset()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    pub(crate) fn on_word_delete_forward(
        &mut self,
        _: &WordDeleteForward,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_read_only() {
            return;
        }

        if self.selected_range.is_empty() {
            if self.cursor_offset() == self.visible_len() {
                // Nothing to the right in this block; defer to grapheme
                // delete, which handles block merge and separator removal.
                self.on_delete(&Delete, window, cx);
                return;
            }
            self.prepare_undo_capture(self.text_edit_undo_kind(), cx);
            self.select_to(self.next_word_start(self.cursor_offset()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    pub(crate) fn on_indent_block(
        &mut self,
        _: &IndentBlock,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_read_only() {
            return;
        }

        if self.is_table_cell() {
            cx.emit(BlockEvent::RequestTableCellMoveHorizontal { delta: 1 });
            return;
        }
        if self.can_adjust_list_nesting() {
            cx.emit(BlockEvent::RequestIndent);
            return;
        }
        if self.kind() == BlockKind::Paragraph || self.kind().is_code_block() {
            self.replace_text_in_range(None, "    ", window, cx);
        }
    }

    pub(crate) fn on_outdent_block(
        &mut self,
        _: &OutdentBlock,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_read_only() {
            return;
        }

        if self.is_table_cell() {
            cx.emit(BlockEvent::RequestTableCellMoveHorizontal { delta: -1 });
            return;
        }
        if self.can_outdent_list_nesting() {
            cx.emit(BlockEvent::RequestOutdent);
        }
    }

    pub(crate) fn on_block_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.keystroke.key != "tab" {
            return;
        }

        let modifiers = event.keystroke.modifiers;
        if modifiers.control || modifiers.platform || modifiers.alt || modifiers.function {
            return;
        }

        if self.code_language_focus_handle.is_focused(window) {
            return;
        }

        if modifiers.shift {
            self.on_outdent_block(&OutdentBlock, window, cx);
        } else {
            self.on_indent_block(&IndentBlock, window, cx);
        }
        cx.stop_propagation();
    }

    pub(crate) fn on_focus_prev(
        &mut self,
        _: &FocusPrev,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let preferred_x = self.vertical_anchor_x();
        if !self.move_cursor_vertically(-1, preferred_x, cx) {
            if self.is_table_cell() {
                cx.emit(BlockEvent::RequestTableCellMoveVertical { delta: -1 });
                return;
            }
            cx.emit(BlockEvent::RequestFocusPrev {
                preferred_x: Some(f32::from(preferred_x)),
            });
        }
    }

    pub(crate) fn on_focus_next(
        &mut self,
        _: &FocusNext,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let preferred_x = self.vertical_anchor_x();
        if !self.move_cursor_vertically(1, preferred_x, cx) {
            if self.is_table_cell() {
                cx.emit(BlockEvent::RequestTableCellMoveVertical { delta: 1 });
                return;
            }
            // In a code block, Down from the last content line steps into the
            // language field rather than leaving the block, so the language is
            // reachable by keyboard. A further Down there exits the block.
            if self.kind().is_code_block() && !self.code_language_focus_handle.is_focused(window) {
                self.code_language_focus_handle.focus(window, cx);
                cx.notify();
                return;
            }
            cx.emit(BlockEvent::RequestFocusNext {
                preferred_x: Some(f32::from(preferred_x)),
            });
        }
    }

    pub(crate) fn on_move_left(
        &mut self,
        _: &MoveLeft,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selected_range.is_empty() {
            if let Some((target, affinity)) = self.projected_move_left_target(self.cursor_offset())
            {
                self.assign_collapsed_selection_offset(target, affinity, None);
                self.cursor_blink_epoch = std::time::Instant::now();
                cx.notify();
            } else {
                let previous = self.previous_boundary(self.cursor_offset());
                // At the start of a table cell, step into the previous cell
                // rather than stalling at the edge (same path as Shift+Tab).
                if previous == self.cursor_offset() && self.is_table_cell() {
                    cx.emit(BlockEvent::RequestTableCellMoveHorizontal { delta: -1 });
                    return;
                }
                self.move_to(previous, cx);
            }
        } else {
            self.move_to(self.selected_range.start, cx);
        }
    }

    pub(crate) fn on_move_right(
        &mut self,
        _: &MoveRight,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selected_range.is_empty() {
            if let Some((target, affinity)) =
                self.projected_move_right_target(self.selected_range.end)
            {
                self.assign_collapsed_selection_offset(target, affinity, None);
                self.cursor_blink_epoch = std::time::Instant::now();
                cx.notify();
            } else {
                let next = self.next_boundary(self.selected_range.end);
                // At the end of a table cell, step into the next cell rather
                // than stalling at the edge (same path as Tab).
                if next == self.selected_range.end && self.is_table_cell() {
                    cx.emit(BlockEvent::RequestTableCellMoveHorizontal { delta: 1 });
                    return;
                }
                self.move_to(next, cx);
            }
        } else {
            self.move_to(self.selected_range.end, cx);
        }
    }

    pub(crate) fn on_home(&mut self, _: &Home, _window: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.cursor_line_display_range().start, cx);
    }
}

mod code_language;
mod keys;

#[cfg(test)]
mod tests;
