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

    pub(crate) fn build_root_blocks_from_markdown(
        cx: &mut Context<Self>,
        markdown: &str,
    ) -> Vec<Entity<crate::editor::Block>> {
        let lines = markdown
            .split('\n')
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        Self::build_blocks_from_lines_internal(cx, &lines, true, ChunkCursor::WHOLE_DOCUMENT).0
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
    ) -> (Vec<Entity<crate::editor::Block>>, usize) {
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
    ) -> (Vec<Entity<crate::editor::Block>>, usize) {
        let mut roots = Vec::new();
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

                for _ in 0..preserved_empty_blocks {
                    roots.push(native_block(cx, BlockKind::Paragraph, String::new()));
                }
                continue;
            }

            if parse_opening_fence(line).is_some() {
                let Some((block, next_index)) = collect_fenced_code_block(cx, lines, index) else {
                    let paragraph = Self::collect_paragraph_block(cx, lines, index);
                    roots.push(paragraph.0);
                    index = paragraph.1;
                    continue;
                };

                roots.push(block);
                index = next_index;
                continue;
            }

            if is_fenced_div_opening(line) {
                let end = collect_fenced_div_end(lines, index).unwrap_or(lines.len());
                roots.push(raw_block(cx, lines[index..end].join("\n")));
                index = end;
                continue;
            }

            if let Some((block, end)) = collect_comment_block(cx, lines, index) {
                roots.push(block);
                index = end;
                continue;
            }

            if is_block_html_start(line) {
                let end = collect_block_html_region(lines, index);
                roots.push(html_or_raw_block(cx, lines[index..end].join("\n")));
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
                index = end;
                continue;
            }

            if is_reference_definition_start(line) {
                let end = collect_reference_definition_region(lines, index);
                roots.push(raw_block(cx, lines[index..end].join("\n")));
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
                index += 2;
                continue;
            }

            if parse_standalone_image(line).is_some() {
                roots.push(standalone_image_block(cx, line.to_string()));
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
                index = end;
                continue;
            }

            if strip_indented_code_prefix(line).is_some() {
                let Some((block, next_index)) = collect_indented_code_block(cx, lines, index)
                else {
                    unreachable!("indented code prefix disappeared after detection");
                };

                roots.push(block);
                index = next_index;
                continue;
            }

            if parse_list_marker(line).is_some() {
                // A list is the one construct that can be arbitrarily long (a
                // whole document without blank lines parses as one list), so the
                // collector takes the remaining root budget and can stop between
                // top-level items.
                let remaining = cursor.root_budget.saturating_sub(roots.len()).max(1);
                let (blocks, next_index) = Self::collect_list_blocks(cx, lines, index, remaining);
                roots.extend(blocks);
                index = next_index;
                continue;
            }

            if is_quote_start(line) {
                let (block, next_index) = Self::collect_quote_block(cx, lines, index);
                roots.push(block);
                index = next_index;
                continue;
            }

            if let Some((level, content)) = BlockKind::parse_atx_heading_line(line) {
                roots.push(native_block(cx, BlockKind::Heading { level }, content));
                index += 1;
                continue;
            }

            if BlockKind::parse_separator_line(line) {
                roots.push(Self::new_block(
                    cx,
                    BlockRecord::new(BlockKind::Separator, InlineTextTree::plain(String::new())),
                ));
                index += 1;
                continue;
            }

            if is_root_table_candidate_line(line) {
                let end = collect_root_table_candidate_region(lines, index);
                let region = &lines[index..end];
                if let Some(table) = parse_root_table_region(region) {
                    roots.push(Self::new_block(cx, BlockRecord::table(table)));
                } else {
                    roots.extend(
                        region
                            .iter()
                            .cloned()
                            .map(|line| plain_text_paragraph_block(cx, line)),
                    );
                }
                index = end;
                continue;
            }

            if let Some(end) = collect_pipeless_table_region(lines, index)
                && let Some(table) = parse_root_table_region(&lines[index..end])
            {
                roots.push(Self::new_block(cx, BlockRecord::table(table)));
                index = end;
                continue;
            }

            let paragraph = Self::collect_paragraph_block(cx, lines, index);
            roots.push(paragraph.0);
            index = paragraph.1;
        }

        (roots, index)
    }
}
