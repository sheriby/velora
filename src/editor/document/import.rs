use super::*;

impl Editor {
    pub(crate) fn markdown_requires_source_mode_fallback(markdown: &str) -> bool {
        let lines = markdown.lines().map(ToOwned::to_owned).collect::<Vec<_>>();
        let mut index = 0;
        while index < lines.len() {
            if let Some(fence) = parse_opening_fence(&lines[index]) {
                if let Some(closing_index) = find_matching_closing_fence(&lines, index, &fence) {
                    index = closing_index + 1;
                    continue;
                }
                break;
            }
            if is_unsupported_admonition_opening(&lines[index]) {
                return true;
            }
            if is_fenced_div_opening(&lines[index])
                && collect_fenced_div_end(&lines, index).is_none()
            {
                return true;
            }
            index += 1;
        }
        false
    }

    /// 从缓冲区现在的内容解析整棵块树，并把每个根块的源码区间挂回去。
    ///
    /// 这是「重建整棵树」的唯一入口：树是缓冲区的一份投影，输入必须是缓冲区的
    /// 文本，输出必须带区间。漏掉区间的块在位置换算里没有锚点——搜索跳转、
    /// 大纲点击、跨块选区端点都会落回 0。
    pub(crate) fn rebuild_root_blocks_from_buffer(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Vec<Entity<crate::editor::Block>> {
        let source = self.buffer.text();
        let lines = std::sync::Arc::new(Self::split_markdown_lines(&source));
        let (mut roots, root_spans, _consumed) =
            Self::build_root_block_chunk(cx, &lines, ChunkCursor::WHOLE_DOCUMENT);
        if roots.is_empty() {
            roots.push(Self::new_block(
                cx,
                BlockRecord::paragraph(String::new()),
            ));
        }
        Self::attach_root_spans(&self.buffer, &roots, &root_spans, 0, cx);
        roots
    }

    /// Splits normalized Markdown into lines once, so a document can be built in
    /// several chunks that all read the same line array.
    pub(crate) fn split_markdown_lines(markdown: &str) -> Vec<String> {
        markdown.split('\n').map(ToOwned::to_owned).collect()
    }

    /// Builds the next chunk of root blocks from the remaining document lines.
    ///
    /// `lines` must be the untouched remainder of the document (a suffix of the
    /// same array a full pass would read): the importer decides "paragraph
    /// continues through this blank line / block start" by scanning forward, so
    /// a truncated slice would parse differently near the cut.
    pub(crate) fn build_root_block_chunk(
        cx: &mut Context<Self>,
        lines: &[String],
        cursor: ChunkCursor,
    ) -> (
        Vec<Entity<crate::editor::Block>>,
        Vec<std::ops::Range<usize>>,
        usize,
    ) {
        Self::build_blocks_from_lines_internal(cx, lines, true, cursor)
    }

    /// Builds runtime blocks from Markdown lines.
    ///
    /// Native blocks are created only for syntax the runtime editor can edit
    /// safely. More complex valid Markdown regions fall back to
    /// [`BlockKind::RawMarkdown`] so they are preserved exactly on save.
    pub(crate) fn build_blocks_from_lines(
        cx: &mut Context<Self>,
        lines: &[String],
    ) -> Vec<Entity<crate::editor::Block>> {
        Self::build_blocks_from_lines_internal(cx, lines, true, ChunkCursor::WHOLE_DOCUMENT).0
    }

    pub(crate) fn build_blocks_from_lines_internal(
        cx: &mut Context<Self>,
        lines: &[String],
        allow_root_footnote_definitions: bool,
        cursor: ChunkCursor,
    ) -> (
        Vec<Entity<crate::editor::Block>>,
        Vec<std::ops::Range<usize>>,
        usize,
    ) {
        let mut roots = Vec::new();
        // 与 `roots` 同步：每根块消费的行区间（相对传入的 `lines` 切片）。
        let mut spans: Vec<std::ops::Range<usize>> = Vec::new();
        let mut index = 0;

        while index < lines.len() {
            let line = &lines[index];
            // Incremental chunking (roadmap G8): stop once this chunk has built
            // `root_budget` roots, but only where resuming later stays
            // equivalent to one full-document pass. A cut in front of a blank
            // run would move the run's preserved empty paragraphs into the next
            // chunk, and a cut right after a list item leaves the next chunk
            // starting with a list whose serializer blank rule depends on the
            // previous root.
            if roots.len() >= cursor.root_budget && !line.trim().is_empty() {
                break;
            }
            // YAML frontmatter: a `---` fence pair at the very top of the
            // document is preserved byte-exact as a FrontMatter block instead
            // of being parsed as setext headings / thematic breaks (roadmap C1).
            if cursor.is_document_start
                && index == 0
                && roots.is_empty()
                && line.trim_end_matches('\r') == "---"
                && let Some(close) = (1..lines.len()).find(|&close_index| {
                    lines[close_index].trim_end_matches('\r') == "---"
                })
            {
                let front_matter = lines[..=close].join("\n");
                roots.push(Self::new_block(cx, BlockRecord::front_matter(front_matter)));
                spans.push(index..close + 1);
                index = close + 1;
                continue;
            }

            if line.trim().is_empty() {
                let blank_start = index;
                while index < lines.len() && lines[index].trim().is_empty() {
                    index += 1;
                }

                let blank_run_len = index - blank_start;
                let previous_root_is_list_item = Self::last_root_is_list_item(
                    &roots,
                    cx,
                    cursor.previous_root_is_list_item,
                );
                let next_root_is_list_item = lines
                    .get(index)
                    .is_some_and(|line| parse_list_marker(line).is_some());
                let preserved_empty_blocks =
                    if roots.is_empty() && cursor.is_document_start {
                        blank_run_len
                    } else if previous_root_is_list_item && next_root_is_list_item {
                        blank_run_len
                    } else {
                        blank_run_len.saturating_sub(1)
                    };

                for offset in 0..preserved_empty_blocks {
                    roots.push(native_block(cx, BlockKind::Paragraph, String::new()));
                    // 空段落占住空行段里的一条空行；多出来的空行是块间分隔符，
                    // 不属于任何块，于是编辑某个块时永远不会碰到它。
                    spans.push((blank_start + offset)..(blank_start + offset + 1));
                }
                continue;
            }

            if parse_opening_fence(line).is_some() {
                let Some((block, next_index)) = collect_fenced_code_block(cx, lines, index) else {
                    let paragraph = Self::collect_paragraph_block(cx, lines, index);
                    roots.push(paragraph.0);
                    spans.push(index..paragraph.1);
                    index = paragraph.1;
                    continue;
                };

                roots.push(block);
                spans.push(index..next_index);
                index = next_index;
                continue;
            }

            if is_fenced_div_opening(line) {
                let end = collect_fenced_div_end(lines, index).unwrap_or(lines.len());
                roots.push(raw_block(cx, lines[index..end].join("\n")));
                spans.push(index..end);
                index = end;
                continue;
            }

            if let Some((block, end)) = collect_comment_block(cx, lines, index) {
                roots.push(block);
                spans.push(index..end);
                index = end;
                continue;
            }

            if is_block_html_start(line) {
                let end = collect_block_html_region(lines, index);
                roots.push(html_or_raw_block(cx, lines[index..end].join("\n")));
                spans.push(index..end);
                index = end;
                continue;
            }

            if is_footnote_definition_start(line) {
                let end = collect_footnote_definition_region(lines, index);
                if allow_root_footnote_definitions {
                    if let Some(block) =
                        build_native_footnote_definition_block(cx, &lines[index..end])
                    {
                        roots.push(block);
                    } else {
                        roots.push(raw_block(cx, lines[index..end].join("\n")));
                    }
                } else {
                    roots.push(raw_block(cx, lines[index..end].join("\n")));
                }
                spans.push(index..end);
                index = end;
                continue;
            }

            if is_reference_definition_start(line) {
                let end = collect_reference_definition_region(lines, index);
                roots.push(raw_block(cx, lines[index..end].join("\n")));
                spans.push(index..end);
                index = end;
                continue;
            }

            if let Some(level) = lines
                .get(index + 1)
                .and_then(|next| BlockKind::parse_setext_underline(next))
            {
                roots.push(native_block(
                    cx,
                    BlockKind::Heading { level },
                    line.trim_end().to_string(),
                ));
                spans.push(index..index + 2);
                index += 2;
                continue;
            }

            if parse_standalone_image(line).is_some() {
                roots.push(standalone_image_block(cx, line.to_string()));
                spans.push(index..index + 1);
                index += 1;
                continue;
            }

            // `$$` 公式块允许任意缩进，必须排在「4 空格缩进代码块」之前：
            // 缩进 4 格的公式块之前会被当成代码块显示源码。
            if is_display_math_start_at_any_indent(line) {
                let end = collect_display_math_region(lines, index);
                roots.push(math_or_raw_block(
                    cx,
                    dedent_math_region(&lines[index..end]),
                ));
                spans.push(index..end);
                index = end;
                continue;
            }

            if strip_indented_code_prefix(line).is_some() {
                let Some((block, next_index)) = collect_indented_code_block(cx, lines, index)
                else {
                    unreachable!("indented code prefix disappeared after detection");
                };

                roots.push(block);
                spans.push(index..next_index);
                index = next_index;
                continue;
            }

            if parse_list_marker(line).is_some() {
                // A list is the one construct that can be arbitrarily long (a
                // whole document without blank lines parses as one list), so the
                // collector takes the remaining root budget and can stop between
                // top-level items.
                let remaining = cursor.root_budget.saturating_sub(roots.len()).max(1);
                let (blocks, block_spans, next_index) =
                    Self::collect_list_blocks(cx, lines, index, remaining);
                for (block, block_span) in blocks.into_iter().zip(block_spans) {
                    roots.push(block);
                    spans.push(block_span);
                }
                index = next_index;
                continue;
            }

            if is_quote_start(line) {
                let (block, next_index) = Self::collect_quote_block(cx, lines, index);
                roots.push(block);
                spans.push(index..next_index);
                index = next_index;
                continue;
            }

            if let Some((level, content)) = BlockKind::parse_atx_heading_line(line) {
                roots.push(native_block(cx, BlockKind::Heading { level }, content));
                spans.push(index..index + 1);
                index += 1;
                continue;
            }

            if BlockKind::parse_separator_line(line) {
                roots.push(Self::new_block(
                    cx,
                    BlockRecord::new(BlockKind::Separator, InlineTextTree::plain(String::new())),
                ));
                spans.push(index..index + 1);
                index += 1;
                continue;
            }

            if is_root_table_candidate_line(line) {
                let end = collect_root_table_candidate_region(lines, index);
                let region = &lines[index..end];
                if let Some(table) = parse_root_table_region(region) {
                    roots.push(Self::new_block(cx, BlockRecord::table(table)));
                    spans.push(index..end);
                } else {
                    for (offset, line) in region.iter().enumerate() {
                        roots.push(plain_text_paragraph_block(cx, line.clone()));
                        spans.push((index + offset)..(index + offset + 1));
                    }
                }
                index = end;
                continue;
            }

            if let Some(end) = collect_pipeless_table_region(lines, index)
                && let Some(table) = parse_root_table_region(&lines[index..end])
            {
                roots.push(Self::new_block(cx, BlockRecord::table(table)));
                spans.push(index..end);
                index = end;
                continue;
            }

            let paragraph = Self::collect_paragraph_block(cx, lines, index);
            roots.push(paragraph.0);
            spans.push(index..paragraph.1);
            index = paragraph.1;
        }

        debug_assert_eq!(
            roots.len(),
            spans.len(),
            "块与源码区间必须一一对应，导入器漏记了区间"
        );
        (roots, spans, index)
    }
}
