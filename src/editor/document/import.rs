use super::*;

impl Editor {
    pub(crate) fn markdown_requires_source_mode_fallback(markdown: &str) -> bool {
        let lines = markdown.lines().map(ToOwned::to_owned).collect::<Vec<_>>();
        let lookahead = MarkdownLookahead::new(&lines);
        Self::markdown_lines_require_source_mode_fallback(&lines, &lookahead)
    }

    pub(crate) fn markdown_lines_require_source_mode_fallback(lines: &[String], lookahead: &MarkdownLookahead) -> bool {
        let mut index = 0;
        while index < lines.len() {
            if let Some(fence) = parse_opening_fence(&lines[index]) {
                if let Some(closing_index) = lookahead.matching_fence(lines, index, &fence, 0) {
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
        let lookahead = MarkdownLookahead::new(&lines);
        let (mut roots, root_spans, _consumed) =
            Self::build_root_block_chunk(cx, &lines, ChunkCursor::WHOLE_DOCUMENT, &lookahead, 0);
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
        self.reproject_root_range(root_index, root_index, false, cx)
    }

    /// [`reproject_root_region`](Self::reproject_root_region) 的多根版本：
    /// 把 `[first, last]` 这几根一起当成一段重解析。撤销一次回车会删掉两根之间的
    /// 换行、把它们并成一根，只按单根重解析会在本段行数处给不出根边界。
    pub(crate) fn reproject_root_range(
        &mut self,
        first: usize,
        last: usize,
        reuse_leaf: bool,
        cx: &mut Context<Self>,
    ) -> Option<usize> {
        self.reproject_root_region_impl(first, last, reuse_leaf, cx)
    }

    fn reproject_root_region_impl(&mut self, first: usize, last: usize, reuse_leaf: bool, cx: &mut Context<Self>) -> Option<usize> {
        const REGION_PROBE_LINES: usize = 2;
        // 懒导入没建完时接缝归尾部那段，窗口解析与整篇解析对不上，走整篇那条路。
        if self.document.pending_tail().is_some() {
            return None;
        }
        if last < first {
            return None;
        }
        let roots = self.document.root_blocks();
        let root = roots.get(first)?.clone();
        let last_root = roots.get(last)?.clone();
        let span_start = self.document.source_span_of(root.entity_id())?.start;
        let span_end = self.document.source_span_of(last_root.entity_id())?.end;
        if span_end > self.buffer.byte_len() {
            return None;
        }
        let line_base = self.buffer.line_of(span_start);
        let last_line = self.buffer.line_of(span_end);
        let region_lines = last_line + 1 - line_base;
        let previous_root_is_list_item = first
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
        if reuse_leaf && within == 1 && last == first {
            if let Some(next) = new_roots.first() {
                let record = next.read(cx).record.clone();
                let reusable = root.read(cx).kind() == next.read(cx).kind()
                    && root.read(cx).children.is_empty() && next.read(cx).children.is_empty()
                    && record.table.is_none();
                if reusable {
                    root.update(cx, |block, cx| {
                        let id = block.record.id;
                        block.record = record;
                        block.record.id = id;
                        block.sync_render_cache();
                        cx.notify();
                    });
                    if let Some(span) = Self::root_span_ranges(&self.buffer, &spans[..within], line_base).first() {
                        self.document.set_source_span(root.entity_id(), span.clone());
                    }
                    self.document.refresh_row_spacing_for(root.entity_id(), cx);
                    return Some(region_lines);
                }
            }
        }
        self.attach_root_spans(&new_roots[..within], &spans[..within], line_base);
        self.document
            .replace_root_range(first..last + 1, new_roots[..within].to_vec(), cx);
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
        lookahead: &MarkdownLookahead,
        line_base: usize,
    ) -> (
        Vec<Entity<crate::editor::Block>>,
        Vec<std::ops::Range<usize>>,
        usize,
    ) {
        Self::build_blocks_from_lines_with_lookahead(cx, lines, true, cursor, &[], lookahead, line_base)
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
        Self::build_blocks_from_lines_with_origins(cx, lines, allow_root_footnote_definitions, cursor, &[])
    }

    pub(super) fn build_blocks_from_lines_with_origins(
        cx: &mut Context<Self>,
        lines: &[String],
        allow_root_footnote_definitions: bool,
        cursor: ChunkCursor,
        origins: &[usize],
    ) -> (
        Vec<Entity<crate::editor::Block>>,
        Vec<std::ops::Range<usize>>,
        usize,
    ) {
        let lookahead = MarkdownLookahead::new(lines);
        Self::build_blocks_from_lines_with_lookahead(cx, lines, allow_root_footnote_definitions, cursor, origins, &lookahead, 0)
    }

    fn build_blocks_from_lines_with_lookahead(
        cx: &mut Context<Self>,
        lines: &[String],
        allow_root_footnote_definitions: bool,
        cursor: ChunkCursor,
        origins: &[usize],
        lookahead: &MarkdownLookahead,
        line_base: usize,
    ) -> (Vec<Entity<crate::editor::Block>>, Vec<std::ops::Range<usize>>, usize) {
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
            // 闭合搜索带一条常识约束：空行之后必须紧跟闭合——以 `---`（水平分隔
            // 线）开头的普通文档，后文随便哪个 `---` 都不该把中间隔着空行的标题
            // 正文整篇吞成 YAML 块。内容行本身不设形状约束：YAML 里什么都长得像
            // （`#` 是注释、缩进是块标量），唯一站得住的信号是空行。
            if cursor.is_document_start
                && index == 0
                && roots.is_empty()
                && crate::components::markdown::frontmatter::is_front_matter_fence_line(line)
            {
                let mut close = None;
                for candidate in 1..lines.len() {
                    let candidate_line = &lines[candidate];
                    if crate::components::markdown::frontmatter::is_front_matter_fence_line(candidate_line) {
                        close = Some(candidate);
                        break;
                    }
                    if candidate_line.trim().is_empty() {
                        // 空 FrontMatter 脚手架（`---\n\n---`）的空行后面就是闭合；
                        // 再往后还没等到闭合就放弃。
                        let closes_next = lines
                            .get(candidate + 1)
                            .is_some_and(|next| {
                                crate::components::markdown::frontmatter::is_front_matter_fence_line(next)
                            });
                        if closes_next {
                            close = Some(candidate + 1);
                        }
                        break;
                    }
                }
                if let Some(close) = close {
                    let front_matter = lines[..=close].join("\n");
                    roots.push(Self::new_block(cx, BlockRecord::front_matter(front_matter)));
                    spans.push(index..close + 1);
                    index = close + 1;
                    continue;
                }
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
                        block.record.source_line_prefixes = vec![origins.get(blank_start + offset).copied().unwrap_or(0) as u32];
                    });
                    roots.push(empty);
                    // 空段落占住空行段里的一条空行；多出来的空行是块间分隔符，
                    // 不属于任何块，于是编辑某个块时永远不会碰到它。
                    spans.push((blank_start + offset)..(blank_start + offset + 1));
                }
                continue;
            }

            if parse_opening_fence(line).is_some() {
                let Some((block, next_index)) = collect_fenced_code_block(cx, lines, index, origins, lookahead, line_base) else {
                    let paragraph = Self::collect_paragraph_block(cx, lines, index, origins, lookahead, line_base);
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

            if let Some((block, end)) = collect_comment_block(cx, lines, index, origins) {
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
                        build_native_footnote_definition_block(cx, &lines[index..end], origins.get(index..end).unwrap_or(&[]))
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
                .filter(|_| line_is_setext_heading_content(line))
            {
                let heading = native_block(
                    cx,
                    BlockKind::Heading { level },
                    line.trim_end().to_string(),
                );
                // Setext 内容行不剥自己的记号，只继承上级容器已消费的前缀。
                // 下划线不在标题文本里，不给它记内容偏移。
                heading.update(cx, |heading, _cx| {
                    heading.record.source_line_prefixes = vec![origins.get(index).copied().unwrap_or(0) as u32];
                    // 下划线原文照记：写回按它落笔，`Title\n=` 才不会被改成 `# Title`。
                    if let Some(underline) = lines.get(index + 1) {
                        heading.record.setext_underline = Some(underline.clone());
                    }
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
                let region = &lines[index..end];
                let original = region.join("\n");
                let dedented = crate::components::latex::dedent_math_region(region);
                // 顶格的经典形状（`$$` 行、正文、`$$` 行）走「模型只存正文」那一支：
                // 围栏归结构，用户改不到定界符（见 `BlockRecord::math_body`）。
                // 被剥过缩进的写法不走那一支——`push_math_body_mapping` 把正文起点算成
                // 「区域起点 + 顶格围栏 3 字节」，缩进量会把偏移与块区间一起算错。它与
                // 容器子块同一条路：整段存原文（含围栏），每行被剥掉的缩进记进
                // `source_line_prefixes`，换算与写回都问这一份账。
                let block = if dedented == original {
                    math_body_or_raw_block(cx, dedented)
                } else {
                    let block = math_or_raw_block(cx, dedented.clone());
                    let prefixes = region
                        .iter()
                        .zip(dedented.split('\n'))
                        .map(|(file_line, model_line)| {
                            file_line.len().saturating_sub(model_line.len()) as u32
                        })
                        .collect::<Vec<_>>();
                    if prefixes.iter().any(|prefix| *prefix != 0) {
                        block.update(cx, |block, _cx| {
                            block.record.source_line_prefixes = prefixes;
                        });
                    }
                    block
                };
                roots.push(block);
                spans.push(index..end);
                index = end;
                continue;
            }

            if strip_indented_code_prefix(line).is_some() {
                let Some((block, next_index)) = collect_indented_code_block(cx, lines, index, origins)
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
                    Self::collect_list_blocks(cx, lines, index, remaining, origins);
                for (block, block_span) in blocks.into_iter().zip(block_spans) {
                    roots.push(block);
                    spans.push(block_span);
                }
                index = next_index;
                continue;
            }

            if is_quote_start(line) {
                let (block, next_index) = Self::collect_quote_block(cx, lines, index, origins);
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
                    heading.record.source_line_prefixes = vec![(origins.get(index).copied().unwrap_or(0) + marker_len) as u32];
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
                record.separator_text =
                    Some(SharedString::from(line.trim().to_string()));
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

            let paragraph = Self::collect_paragraph_block(cx, lines, index, origins, lookahead, line_base);
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
