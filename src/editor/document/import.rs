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
                // 没有配平的闭合围栏：这一行按普通行走下去，继续扫后面的行。
                // 直接 `break` 会把后文里真正需要源码模式的形状（`!!!` 标注、
                // 未闭合的 `:::` div）一起放过，界面就在渲染态画出一团认不出的语法。
                index += 1;
                continue;
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
        self.roots_reprojected
            .set(self.roots_reprojected.get() + roots.len() as u64);
        self.attach_root_spans(&roots, &root_spans, 0);
        roots
    }

    /// 只重解析缓冲区里第 `root_index` 根块占的那几行字节，把它换成新解析出来的
    /// 那几根，区间之外的块一个实体都不换。
    ///
    /// 引用行的换行会改结构（行首变成 `- 项` 就不再是引用行了），所以改完字节必须
    /// 重新解析才能刷新投影。整篇重解析付的是「文档有多少根块」的代价：每根块的
    /// 折叠状态、光标现场、渲染缓存全丢，未编辑的块还要连自己的字节重新序列化一遍。
    /// 这里只吃这一段——解析窗口固定多看两行探针：解析必须在本段的行数处给出
    /// 根边界，给不出（本段被解析成跨过边界的大根，结构外扩）说明本段之外的块
    /// 也得跟着变，返回 `None` 由调用方退回整篇重投影。
    ///
    /// 窗口必须**一次性带上探针**再解析：先试本段、被吃再逐步加探针的写法里，
    /// 第一轮窗口恰好等于本段行数、解析永远即时接受，探针迭代从未真正运行过。
    pub(crate) fn reproject_root_region(
        &mut self,
        root_index: usize,
        cx: &mut Context<Self>,
    ) -> Option<usize> {
        const REGION_PROBE_LINES: usize = 2;
        // 懒导入没建完时接缝归尾部那段，窗口解析与整篇解析对不上，走整篇那条路。
        if self.document.pending_tail().is_some() {
            return None;
        }
        let roots = self.document.root_blocks().to_vec();
        let root = roots.get(root_index)?.clone();
        let span = self.document.source_span_of(root.entity_id())?;
        if span.end > self.buffer.byte_len() {
            return None;
        }
        let line_base = self.buffer.line_of(span.start);
        let last_line = self.buffer.line_of(span.end);
        let region_lines = last_line + 1 - line_base;
        let previous_root_is_list_item = root_index
            .checked_sub(1)
            .and_then(|index| roots.get(index))
            .is_some_and(|previous| previous.read(cx).kind().is_list_item());
        let total_lines = self.buffer.line_count();

        let cut = (last_line + 1 + REGION_PROBE_LINES).min(total_lines);
        let lines: Vec<String> = (line_base..cut)
            .map(|line| self.buffer.slice(self.buffer.line_range(line)))
            .collect();
        let cursor = ChunkCursor {
            root_budget: usize::MAX,
            is_document_start: line_base == 0,
            previous_root_is_list_item,
        };
        let (new_roots, spans, _consumed) =
            Self::build_blocks_from_lines_internal(cx, &lines, true, cursor);
        // 验收标准：窗口解析必须在本段行数处给出一个**根边界**——本段换成的几根
        // 恰好铺满原来的行数，探针区里解析出来的后续根不属于本次安装（它们本来
        // 就是隔壁的根）。本段被解析成跨过边界的大根（结构外扩）时找不到边界，
        // 退 `None` 让调用方整篇重投影。
        let within = spans
            .iter()
            .take_while(|span| span.end <= region_lines)
            .count();
        if within == 0 || spans[within - 1].end != region_lines {
            return None;
        }
        self.roots_reprojected
            .set(self.roots_reprojected.get() + within as u64);
        self.attach_root_spans(&new_roots[..within], &spans[..within], line_base);
        self.document
            .replace_root_range(root_index..root_index + 1, new_roots[..within].to_vec(), cx);
        Some(region_lines)
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
                    let empty = native_block(cx, BlockKind::Paragraph, String::new());
                    // 空段落占的那一行同样没剥任何东西。
                    empty.update(cx, |block, _cx| {
                        block.record.source_line_prefixes = vec![0];
                    });
                    roots.push(empty);
                    // 空段落占住空行段里的一条空行；多出来的空行是块间分隔符，
                    // 不属于任何块，于是编辑某个块时永远不会碰到它。
                    spans.push((blank_start + offset)..(blank_start + offset + 1));
                }
                continue;
            }

            if parse_opening_fence(line).is_some() {
                let Some((block, next_index)) = collect_fenced_code_block(cx, lines, index, &[]) else {
                    let paragraph = Self::collect_paragraph_block(cx, lines, index, &[]);
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

            if let Some((block, end)) = collect_comment_block(cx, lines, index, &[]) {
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
                let heading = native_block(
                    cx,
                    BlockKind::Heading { level },
                    line.trim_end().to_string(),
                );
                // Setext 标题的内容行**没有记号**：整行（连前导空白）就是内容，所以宽度是 0
                // ——这是这里一眼看到的事实，不是事后拿文件行与模型行比出来的。底下那一行是
                // 下划线，不在模型里，位置表按文件的行长度走（`file_lens`），不需要为它记什么。
                heading.update(cx, |heading, _cx| {
                    heading.record.source_line_prefixes = vec![0];
                });
                roots.push(heading);
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
                let Some((block, next_index)) = collect_indented_code_block(cx, lines, index, &[])
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
                    Self::collect_list_blocks(cx, lines, index, remaining, &[]);
                for (block, block_span) in blocks.into_iter().zip(block_spans) {
                    roots.push(block);
                    spans.push(block_span);
                }
                index = next_index;
                continue;
            }

            if is_quote_start(line) {
                let (block, next_index) = Self::collect_quote_block(cx, lines, index, &[]);
                roots.push(block);
                spans.push(index..next_index);
                index = next_index;
                continue;
            }

            if let Some((level, content, marker_len)) =
                BlockKind::parse_atx_heading_line_with_marker(line)
            {
                let heading = native_block(cx, BlockKind::Heading { level }, content);
                // 记号（本行前导缩进 + `#…` + 那一个空格）占几位，是剥它的那段代码
                // 当场就知道的事实：记下来，位置换算就不必事后拿文件行与模型行比。
                heading.update(cx, |heading, _cx| {
                    heading.record.source_line_prefixes = vec![marker_len as u32];
                });
                roots.push(heading);
                spans.push(index..index + 1);
                index += 1;
                continue;
            }

            if BlockKind::parse_separator_line(line) {
                let mut record =
                    BlockRecord::new(BlockKind::Separator, InlineTextTree::plain(String::new()));
                record.separator_marker = crate::components::SeparatorMarker::detect(line);
                roots.push(Self::new_block(cx, record));
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

            let paragraph = Self::collect_paragraph_block(cx, lines, index, &[]);
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
