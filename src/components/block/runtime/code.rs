//! Code-block runtime cache management.

use super::*;

fn normalize_code_language_input(text: &str) -> String {
    text.replace("\r\n", " ")
        .replace(['\r', '\n'], " ")
        .trim()
        .to_string()
}

impl Block {
    /// 代码块里打字、回车改的就是「块自己那几行」，行数一变，解析期记下的每行继承量
    /// 要跟着改：拆出来的那一行还排在同一行里，让开的字节与原来那一行相同。
    ///
    /// 不维护这份账，读侧一核对行数就作废、交回「拿文件行与模型行比」那一条：实测在
    /// `let a = 1;` 里回车再打一个字，比出来的那条路把模型新行开头的那个空格当成了
    /// 容器让开的位数，字写到了空格之后（` 写1;`）。改不动的形状（一次改动跨过多行
    /// 又生出多行）直接把账作废，宁可退回量那条路，也不要一份错账。
    pub(super) fn adjust_code_line_prefixes_for_text_edit(&mut self, old_text: &str, new_text: &str) {
        let recorded = self.record.source_line_prefixes.clone();
        let old_lines = old_text.split('\n').count();
        if recorded.is_empty() || recorded.len() != old_lines || old_text == new_text {
            return;
        }

        let common_prefix = old_text
            .bytes()
            .zip(new_text.bytes())
            .take_while(|(before, after)| before == after)
            .count();
        let rest_old = &old_text[common_prefix..];
        let rest_new = &new_text[common_prefix..];
        let common_suffix = rest_old
            .bytes()
            .rev()
            .zip(rest_new.bytes().rev())
            .take_while(|(before, after)| before == after)
            .count();
        let changed_old = &rest_old[..rest_old.len() - common_suffix];
        let changed_new = &rest_new[..rest_new.len() - common_suffix];

        let first_line = old_text[..common_prefix].matches('\n').count();
        let last_line = first_line + changed_old.matches('\n').count();
        let next_lines = 1 + changed_new.matches('\n').count();

        let kept = if next_lines == 1 || last_line == first_line {
            vec![recorded[first_line]; next_lines]
        } else {
            self.record.source_line_prefixes.clear();
            return;
        };
        self.record.source_line_prefixes.splice(first_line..=last_line, kept);
    }

    pub(crate) fn code_highlight_result(&self) -> Option<&CodeHighlightResult> {
        self.code_highlight.as_ref()
    }

    pub(super) fn sync_code_highlight(&mut self) {
        self.code_highlight = match &self.record.kind {
            BlockKind::CodeBlock { language } => highlight_code_block(
                language.as_deref().map(|value| &**value),
                self.render_cache.visible_text(),
            ),
            _ => None,
        };
    }

    pub(crate) fn code_language_text(&self) -> &str {
        match &self.record.kind {
            BlockKind::CodeBlock {
                language: Some(language),
            } => language.as_ref(),
            _ => "",
        }
    }

    pub(crate) fn code_language_cursor_offset(&self) -> usize {
        if self.code_language_selection_reversed {
            self.code_language_selected_range.start
        } else {
            self.code_language_selected_range.end
        }
    }

    pub(crate) fn code_language_range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        Self::utf8_range_to_utf16_in(self.code_language_text(), range)
    }

    pub(crate) fn code_language_range_from_utf16(
        &self,
        range_utf16: &Range<usize>,
    ) -> Range<usize> {
        Self::utf16_range_to_utf8_in(self.code_language_text(), range_utf16)
    }

    pub(crate) fn previous_code_language_boundary(&self, offset: usize) -> usize {
        self.code_language_text()
            .grapheme_indices(true)
            .rev()
            .find_map(|(idx, _)| (idx < offset).then_some(idx))
            .unwrap_or(0)
    }

    pub(crate) fn next_code_language_boundary(&self, offset: usize) -> usize {
        self.code_language_text()
            .grapheme_indices(true)
            .find_map(|(idx, _)| (idx > offset).then_some(idx))
            .unwrap_or(self.code_language_text().len())
    }

    pub(crate) fn move_code_language_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let clamped = offset.min(self.code_language_text().len());
        self.code_language_selected_range = clamped..clamped;
        self.code_language_selection_reversed = false;
        self.code_language_marked_range = None;
        self.cursor_blink_epoch = Instant::now();
        cx.notify();
    }

    pub(crate) fn select_code_language_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let clamped = offset.min(self.code_language_text().len());
        if self.code_language_selection_reversed {
            self.code_language_selected_range.start = clamped;
        } else {
            self.code_language_selected_range.end = clamped;
        }
        if self.code_language_selected_range.end < self.code_language_selected_range.start {
            self.code_language_selection_reversed = !self.code_language_selection_reversed;
            self.code_language_selected_range =
                self.code_language_selected_range.end..self.code_language_selected_range.start;
        }
        self.cursor_blink_epoch = Instant::now();
        cx.notify();
    }

    pub(crate) fn replace_code_language_text_in_range(
        &mut self,
        range: Range<usize>,
        new_text: &str,
        selected_range_relative: Option<Range<usize>>,
        mark_inserted_text: bool,
        cx: &mut Context<Self>,
    ) {
        if !self.kind().is_code_block() {
            return;
        }

        if self.code_language_marked_range.is_some() {
            if !mark_inserted_text {
                self.prepare_undo_capture(UndoCaptureKind::ImeCompositionCommit, cx);
            }
        } else if mark_inserted_text {
            self.prepare_undo_capture(UndoCaptureKind::ImeComposition, cx);
        } else {
            self.prepare_undo_capture(UndoCaptureKind::CoalescibleText, cx);
        }

        let current = self.code_language_text().to_string();
        let range = range.start.min(current.len())..range.end.min(current.len());
        let inserted = new_text.replace("\r\n", " ").replace(['\r', '\n'], " ");
        let mut raw_next = String::new();
        raw_next.push_str(&current[..range.start]);
        raw_next.push_str(&inserted);
        raw_next.push_str(&current[range.end..]);

        let trimmed_start = raw_next.len() - raw_next.trim_start().len();
        let normalized = normalize_code_language_input(&raw_next);
        let normalized_len = normalized.len();
        let raw_inserted_end = range.start + inserted.len();
        let next_cursor = selected_range_relative
            .as_ref()
            .map(|relative| range.start + relative.end)
            .unwrap_or(raw_inserted_end)
            .saturating_sub(trimmed_start)
            .min(normalized_len);
        let next_selection = selected_range_relative
            .as_ref()
            .map(|relative| {
                let start = (range.start + relative.start)
                    .saturating_sub(trimmed_start)
                    .min(normalized_len);
                let end = (range.start + relative.end)
                    .saturating_sub(trimmed_start)
                    .min(normalized_len);
                start.min(end)..start.max(end)
            })
            .unwrap_or_else(|| next_cursor..next_cursor);
        let next_marked = if mark_inserted_text && !inserted.is_empty() {
            let start = range
                .start
                .saturating_sub(trimmed_start)
                .min(normalized_len);
            let end = raw_inserted_end
                .saturating_sub(trimmed_start)
                .min(normalized_len);
            (start < end).then_some(start..end)
        } else {
            None
        };

        // 在光标处插几个字符这一种形状交给编辑器按源码位置点写回：整块重新序列化
        // 会把围栏的写法（`~~~~`）打成 ```。归一化动过别处就不算纯插入。
        self.pending_visible_insertion = if range.is_empty()
            && !inserted.is_empty()
            && trimmed_start == 0
            && normalized.len() == raw_next.len()
        {
            Some((range.start, inserted.clone()))
        } else {
            None
        };

        let old_language = match &self.record.kind {
            BlockKind::CodeBlock { language } => language.clone(),
            _ => None,
        };
        self.record.kind = BlockKind::CodeBlock {
            language: (!normalized.is_empty()).then(|| SharedString::from(normalized)),
        };
        self.code_language_selected_range = next_selection;
        self.code_language_selection_reversed = selected_range_relative
            .as_ref()
            .is_some_and(|relative| relative.end < relative.start);
        self.code_language_marked_range = next_marked;
        self.cursor_blink_epoch = Instant::now();
        self.sync_code_highlight();

        let next_language = match &self.record.kind {
            BlockKind::CodeBlock { language } => language.clone(),
            _ => None,
        };
        if old_language != next_language {
            cx.emit(BlockEvent::Changed);
        }
        cx.notify();
    }

    pub(crate) fn code_language_index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        let text = self.code_language_text();
        if text.is_empty() {
            return 0;
        }

        let (Some(bounds), Some(line)) = (
            self.code_language_last_bounds.as_ref(),
            self.code_language_last_layout.as_ref(),
        ) else {
            return 0;
        };
        if position.x <= bounds.left() {
            return 0;
        }
        if position.x >= bounds.right() {
            return text.len();
        }
        line.closest_index_for_x(position.x - bounds.left())
    }

    pub(crate) fn reset_code_language_input_layout(&mut self) {
        self.code_language_last_layout = None;
        self.code_language_last_bounds = None;
        self.code_language_is_selecting = false;
    }
}
