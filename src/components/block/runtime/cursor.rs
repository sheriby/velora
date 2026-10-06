use super::*;

impl Block {
    pub(crate) fn clean_to_current_cursor_offset(&self, clean: usize) -> usize {
        let Some(projection) = &self.projection else {
            return clean;
        };
        projection
            .clean_to_display_cursor
            .get(clean.min(projection.clean_to_display_cursor.len().saturating_sub(1)))
            .copied()
            .unwrap_or(clean)
    }

    pub(crate) fn clean_to_current_cursor_offset_with_affinity(
        &self,
        clean: usize,
        affinity: CollapsedCaretAffinity,
    ) -> usize {
        let Some(projection) = &self.projection else {
            return clean;
        };
        projection
            .display_offset_for_clean_cursor(clean, affinity)
            .unwrap_or_else(|| self.clean_to_current_cursor_offset(clean))
    }

    /// 光标落在块首/块尾时用外侧亲和性。默认映射会把光标放到标记「里面」：块首是
    /// `**`、`<...>`、`[...]()` 这类标记时，打开文件后光标就不在行首了（用户报修），
    /// 行尾同理。块内部的偏移不受影响（在粗体里继续打字仍然留在粗体里）。
    pub(crate) fn caret_affinity_for_clean_offset(
        &self,
        clean: usize,
        fallback: CollapsedCaretAffinity,
    ) -> CollapsedCaretAffinity {
        if clean == 0 {
            CollapsedCaretAffinity::OuterStart
        } else if clean >= self.record.title.visible_text().len() {
            CollapsedCaretAffinity::OuterEnd
        } else {
            fallback
        }
    }

    pub(crate) fn clean_to_current_range_start(&self, clean: usize) -> usize {
        self.clean_to_current_cursor_offset(clean)
    }

    pub(crate) fn clean_to_current_range_end(&self, clean: usize) -> usize {
        self.clean_to_current_cursor_offset(clean)
    }

    pub(crate) fn clean_to_current_range(&self, range: Range<usize>) -> Range<usize> {
        if range.is_empty() {
            let offset = self.clean_to_current_cursor_offset(range.start);
            offset..offset
        } else {
            self.clean_to_current_range_start(range.start)
                ..self.clean_to_current_range_end(range.end)
        }
    }

    /// 块内**干净**可见长度：不含为了编辑而临时显形出来的 `**`、`](` 这类记号。
    ///
    /// 显形是跟着光标走的瞬时状态（光标进到粗体里就显出来，走开又收回去），
    /// 跨块选区的端点存的是这个坐标系，不跟着显示长度变。
    pub(crate) fn clean_visible_len(&self) -> usize {
        self.record.title.visible_len()
    }

    /// 干净可见区间 → 当前显示区间：两端按「块首/块尾取外侧」的亲和性换算，
    /// 于是显形出来的记号落在区间里侧，被区间盖住（高亮铺满整行、按删除
    /// 连同 `**` 一起收掉）。跨块选区把干净端点落到某一根块上时走这一条。
    pub(crate) fn clean_range_to_display_range(&self, range: Range<usize>) -> Range<usize> {
        if range.is_empty() {
            let offset = self.clean_to_current_cursor_offset(range.start);
            return offset..offset;
        }
        let start_affinity = self
            .caret_affinity_for_clean_offset(range.start, CollapsedCaretAffinity::Default);
        let end_affinity =
            self.caret_affinity_for_clean_offset(range.end, CollapsedCaretAffinity::Default);
        self.clean_to_current_cursor_offset_with_affinity(range.start, start_affinity)
            ..self.clean_to_current_cursor_offset_with_affinity(range.end, end_affinity)
    }

    pub(crate) fn current_to_clean_range(&self, range: Range<usize>) -> Range<usize> {
        self.current_to_clean_offset(range.start)..self.current_to_clean_offset(range.end)
    }

    pub(crate) fn current_to_clean_offset(&self, offset: usize) -> usize {
        self.unexpand_offset(offset)
    }

    #[allow(dead_code)]
    pub(crate) fn pointer_target_offset(&self, offset: usize) -> usize {
        self.projection
            .as_ref()
            .map(|projection| projection.pointer_target_offset(offset))
            .unwrap_or(offset)
    }

    pub(crate) fn projected_move_left_target(
        &self,
        offset: usize,
    ) -> Option<(usize, CollapsedCaretAffinity)> {
        self.projection
            .as_ref()
            .and_then(|projection| projection.move_left_target(offset))
    }

    pub(crate) fn projected_move_right_target(
        &self,
        offset: usize,
    ) -> Option<(usize, CollapsedCaretAffinity)> {
        self.projection
            .as_ref()
            .and_then(|projection| projection.move_right_target(offset))
    }

    pub(crate) fn selection_clean_range(&self) -> Range<usize> {
        self.current_to_clean_range(self.selected_range.clone())
    }

    pub(crate) fn current_range_to_markdown_range(&self, range: Range<usize>) -> Range<usize> {
        if self.uses_raw_text_editing() || self.kind().is_code_block() {
            return range.start.min(self.visible_len())..range.end.min(self.visible_len());
        }

        // 带标记的块里，行尾/初始光标可能落在可见文本之外（标记占位没换算回来），
        // 先收敛到可见范围，否则映射会落到 `<...>` 内部这样的地方。
        let visible_len = self.visible_len();
        let range = range.start.min(visible_len)..range.end.min(visible_len);
        // 自动链接的「标签」就是 URL，不能按可编辑标签映射（那会把插入点放进 `<...>`
        // 里），交给下面的边界处理。
        if let Some(link_run) = self
            .projected_link_run_fully_covering_range(&range)
            .filter(|run| !matches!(run.link, InlineLink::Autolink { .. }))
        {
            let map = self.record.title.markdown_offset_map();
            let label_markdown_start = map.visible_to_markdown_offset(link_run.clean_range.start);
            let run_markdown_start =
                label_markdown_start.saturating_sub(link_run.link.open_marker().len());
            let start = run_markdown_start
                + range
                    .start
                    .saturating_sub(link_run.display_range.start)
                    .min(link_run.display_range.len());
            let end = run_markdown_start
                + range
                    .end
                    .saturating_sub(link_run.display_range.start)
                    .min(link_run.display_range.len());
            return start..end;
        }

        if let Some(footnote_run) = self
            .projection
            .as_ref()
            .and_then(|projection| projection.footnote_run_fully_covering_range(&range))
        {
            let raw = footnote_run.footnote.raw_markdown();
            let raw_len = raw.len();
            let local_start = range
                .start
                .saturating_sub(footnote_run.display_range.start)
                .min(footnote_run.display_range.len());
            let local_end = range
                .end
                .saturating_sub(footnote_run.display_range.start)
                .min(footnote_run.display_range.len());
            let mapped_start = (raw_len * local_start) / footnote_run.display_range.len().max(1);
            let mapped_end = (raw_len * local_end) / footnote_run.display_range.len().max(1);
            let map = self.record.title.markdown_offset_map();
            let run_markdown_start = map.visible_to_markdown_offset(footnote_run.clean_range.start);
            return run_markdown_start + mapped_start..run_markdown_start + mapped_end;
        }

        let clean_range = self.current_to_clean_range(range.clone());
        if let Some(mapped) = self.autolink_boundary_markdown_range(&clean_range) {
            return mapped;
        }
        self.record
            .title
            .markdown_offset_map()
            .visible_to_markdown_range(clean_range)
    }

    /// 光标贴在自动链接的可见文本边缘时，把插入点映射到 `<`/`>` 之外。
    /// 自动链接的「标签」就是 URL 本身，插到里面会把链接写坏，转义字符也会直接落进
    /// 显示文本（用户报修：行首自动链接前按反斜杠，可见数量翻倍）。
    pub(crate) fn autolink_boundary_markdown_range(&self, clean_range: &Range<usize>) -> Option<Range<usize>> {
        if clean_range.start != clean_range.end {
            return None;
        }
        let map = self.record.title.markdown_offset_map();
        let mut visible_start = 0;
        for fragment in &self.record.title.fragments {
            let visible_end = visible_start + fragment.text.len();
            if let Some(link @ InlineLink::Autolink { .. }) = fragment.link.as_ref() {
                if clean_range.start == visible_start {
                    let label_start = map.visible_to_markdown_offset(visible_start);
                    let offset = label_start.saturating_sub(link.open_marker().len());
                    return Some(offset..offset);
                }
                if clean_range.start == visible_end {
                    let label_end = map.visible_to_markdown_offset(visible_end);
                    let offset = label_end + link.close_marker().len();
                    return Some(offset..offset);
                }
            }
            visible_start = visible_end;
        }
        None
    }

    pub(crate) fn markdown_range_to_current_range(&self, range: Range<usize>) -> Range<usize> {
        if self.uses_raw_text_editing() || self.kind().is_code_block() {
            let len = self.visible_len();
            return range.start.min(len)..range.end.min(len);
        }

        let clean_range = self
            .record
            .title
            .markdown_offset_map()
            .markdown_to_visible_range(range);
        self.clean_to_current_range(clean_range)
    }

    pub(crate) fn markdown_offset_to_current_offset(&self, offset: usize) -> usize {
        self.markdown_range_to_current_range(offset..offset).start
    }

    pub(crate) fn prepare_undo_capture(&self, kind: UndoCaptureKind, cx: &mut Context<Self>) {
        cx.emit(BlockEvent::PrepareUndo { kind });
    }

    pub(crate) fn utf16_to_utf8_in(text: &str, offset: usize) -> usize {
        let mut utf8_offset = 0;
        let mut utf16_count = 0;

        for ch in text.chars() {
            if utf16_count >= offset {
                break;
            }
            utf16_count += ch.len_utf16();
            utf8_offset += ch.len_utf8();
        }

        utf8_offset
    }

    pub(crate) fn utf8_to_utf16_in(text: &str, offset: usize) -> usize {
        let mut utf16_offset = 0;
        let mut utf8_count = 0;

        for ch in text.chars() {
            if utf8_count >= offset {
                break;
            }
            utf8_count += ch.len_utf8();
            utf16_offset += ch.len_utf16();
        }

        utf16_offset
    }

    pub(crate) fn utf16_range_to_utf8_in(text: &str, range_utf16: &Range<usize>) -> Range<usize> {
        Self::utf16_to_utf8_in(text, range_utf16.start)
            ..Self::utf16_to_utf8_in(text, range_utf16.end)
    }

    pub(crate) fn utf8_range_to_utf16_in(text: &str, range: &Range<usize>) -> Range<usize> {
        Self::utf8_to_utf16_in(text, range.start)..Self::utf8_to_utf16_in(text, range.end)
    }

}
