use super::*;

impl Editor {

    pub(crate) fn last_root_is_list_item(
        roots: &[Entity<crate::editor::Block>],
        cx: &App,
        fallback: bool,
    ) -> bool {
        roots
            .last()
            .map(|block| block.read(cx).kind().is_list_item())
            .unwrap_or(fallback)
    }

    pub(crate) fn collect_paragraph_block(
        cx: &mut Context<Self>,
        lines: &[String],
        start: usize,
        origins: &[usize],
    ) -> (Entity<crate::editor::Block>, usize) {
        let mut paragraph_lines = vec![lines[start].to_string()];
        let mut index = start + 1;
        while index < lines.len() {
            if (lines[index].trim().is_empty() || looks_like_root_block_start(lines, index))
                && !paragraph_can_continue_through_boundary(&paragraph_lines, lines, index)
            {
                break;
            }
            paragraph_lines.push(lines[index].to_string());
            index += 1;
        }

        let content = paragraph_lines.join("\n");
        let block = native_block(cx, BlockKind::Paragraph, content);
        // 段落一行都不剥（内容就是传进来的那一行），所以宽度就是上级容器已经吃掉的
        // 那几字节——解析期就知道的事实，交给位置换算直接用，不再拿文件行与模型行比。
        let prefixes: Vec<u32> = (start..index)
            .map(|at| origins.get(at).copied().unwrap_or(0) as u32)
            .collect();
        block.update(cx, |block, _cx| {
            block.record.source_line_prefixes = prefixes;
        });
        (block, index)
    }

    /// 容器里连续几行普通文本拼成的段落子块：每行的记号宽度就是上级容器已经吃掉的
    /// 那几字节（解析期的事实），位置换算不再拿文件行与模型行比。
    fn paragraph_child_block(
        cx: &mut Context<Self>,
        text: String,
        prefixes: Vec<u32>,
    ) -> Entity<crate::editor::Block> {
        let block = native_block(cx, BlockKind::Paragraph, text);
        block.update(cx, |block, _cx| {
            block.record.source_line_prefixes = prefixes;
        });
        block
    }

    pub(crate) fn collect_quote_block(
        cx: &mut Context<Self>,
        lines: &[String],
        start: usize,
        origins: &[usize],
    ) -> (Entity<crate::editor::Block>, usize) {
        let end = collect_quote_raw_region(lines, start);
        let region = &lines[start..end];
        let mut dequoted = Vec::with_capacity(region.len());
        // 每一行被 `>` 记号吃掉几字节，是这里现算的事实——带着它往下递归，子块记的
        // 宽度就是**文件那一行**的绝对值（`BlockRecord::source_line_prefixes`）。
        let mut dequoted_origins = Vec::with_capacity(region.len());
        for (offset, line) in region.iter().enumerate() {
            let inherited = origins.get(start + offset).copied().unwrap_or(0);
            if line.trim().is_empty() {
                dequoted.push(String::new());
                dequoted_origins.push(inherited + line.len());
                continue;
            }

            let Some(content) = strip_one_quote_level(line) else {
                return (raw_block(cx, region.join("\n")), end);
            };
            dequoted_origins.push(inherited + (line.len() - content.len()));
            dequoted.push(content);
        }

        let Some(block) = Self::build_native_quote_block(cx, &dequoted, &dequoted_origins) else {
            return (raw_block(cx, region.join("\n")), end);
        };

        (block, end)
    }

    pub(crate) fn build_native_quote_block(
        cx: &mut Context<Self>,
        lines: &[String],
        origins: &[usize],
    ) -> Option<Entity<crate::editor::Block>> {
        let inherited = |at: usize| origins.get(at).copied().unwrap_or(0);
        if let Some(header_index) = lines.iter().position(|line| !line.trim().is_empty())
            && let Some((variant, title)) = CalloutVariant::parse_header_line(&lines[header_index])
        {
            return Self::build_native_callout_block(
                cx,
                &lines[header_index + 1..],
                &origins[header_index + 1..],
                variant,
                title,
                // 头那一行的记号照用户写的样子记下来（`note` / `NOTE` 是两种写法）。
                CalloutVariant::header_marker_text(&lines[header_index])
                    .map(|marker| SharedString::from(marker.to_string())),
            );
        }

        let mut title_markdown = String::new();
        // 引用自己那份正文的每一行让开几字节（文件口径）。空行段折成一行、开头的
        // 空行没有内容行，这两种都会让「内容第 i 行」对不上「文件第 i 行」——那时
        // 整份账作废（`title_desynced`），位置换算交回按文件量。
        let mut title_prefixes: Vec<u32> = Vec::new();
        let mut title_desynced = false;
        let mut children = Vec::new();
        let mut index = 0usize;
        let mut pending_blank_lines = 0usize;
        let mut saw_child = false;

        while index < lines.len() {
            let line = &lines[index];
            if line.trim().is_empty() {
                pending_blank_lines += 1;
                index += 1;
                continue;
            }

            if is_table_candidate_line(line) {
                if pending_blank_lines > 0 && (!title_markdown.is_empty() || !children.is_empty()) {
                    append_quote_separator_children(&mut children, pending_blank_lines, cx);
                }
                let table_end = collect_table_candidate_region(lines, index);
                let table_region = &lines[index..table_end];
                if let Some(table) = parse_table_region(table_region) {
                    children.push(Self::new_block(cx, BlockRecord::table(table)));
                } else {
                    children.push(raw_block_from_region(cx, lines, origins, index..table_end));
                }
                saw_child = true;
                pending_blank_lines = 0;
                index = table_end;
                continue;
            }

            if is_footnote_definition_start(line) {
                if pending_blank_lines > 0 && (!title_markdown.is_empty() || !children.is_empty()) {
                    append_quote_separator_children(&mut children, pending_blank_lines, cx);
                }
                let footnote_end = collect_footnote_definition_region(lines, index);
                if let Some(footnote) =
                    build_native_footnote_definition_block(cx, &lines[index..footnote_end])
                {
                    children.push(footnote);
                    saw_child = true;
                    pending_blank_lines = 0;
                    index = footnote_end;
                    continue;
                }
            }

            if let Some((comment, consumed)) = collect_comment_block(cx, lines, index, origins) {
                if pending_blank_lines > 0 && (!title_markdown.is_empty() || !children.is_empty()) {
                    append_quote_separator_children(&mut children, pending_blank_lines, cx);
                }
                children.push(comment);
                saw_child = true;
                pending_blank_lines = 0;
                index = consumed;
                continue;
            }

            if is_block_html_start(line) {
                if pending_blank_lines > 0 && (!title_markdown.is_empty() || !children.is_empty()) {
                    append_quote_separator_children(&mut children, pending_blank_lines, cx);
                }
                let html_end = collect_block_html_region(lines, index);
                children.push(html_or_raw_block_from_region(cx, lines, origins, index..html_end));
                saw_child = true;
                pending_blank_lines = 0;
                index = html_end;
                continue;
            }

            if is_display_math_start(line) {
                if pending_blank_lines > 0 && (!title_markdown.is_empty() || !children.is_empty()) {
                    append_quote_separator_children(&mut children, pending_blank_lines, cx);
                }
                let math_end = collect_display_math_region(lines, index);
                children.push(math_or_raw_block_from_region(cx, lines, origins, index..math_end));
                saw_child = true;
                pending_blank_lines = 0;
                index = math_end;
                continue;
            }

            if let Some(unsupported_end) = collect_unsupported_quote_region(lines, index) {
                if pending_blank_lines > 0 && (!title_markdown.is_empty() || !children.is_empty()) {
                    append_quote_separator_children(&mut children, pending_blank_lines, cx);
                }
                children.push(raw_block_from_region(cx, lines, origins, index..unsupported_end));
                saw_child = true;
                pending_blank_lines = 0;
                index = unsupported_end;
                continue;
            }

            if is_quote_start(line) {
                if pending_blank_lines > 0 && (!title_markdown.is_empty() || !children.is_empty()) {
                    append_quote_separator_children(&mut children, pending_blank_lines, cx);
                }
                let (quote, consumed) = Self::collect_quote_block(cx, lines, index, origins);
                if quote.read(cx).kind() == BlockKind::RawMarkdown {
                    return None;
                }
                children.push(quote);
                saw_child = true;
                pending_blank_lines = 0;
                index = consumed;
                continue;
            }

            if parse_list_marker(line).is_some() {
                if pending_blank_lines > 0 && (!title_markdown.is_empty() || !children.is_empty()) {
                    append_quote_separator_children(&mut children, pending_blank_lines, cx);
                }
                let (list_blocks, _nested_spans, consumed) =
                    Self::collect_list_blocks(cx, lines, index, usize::MAX, origins);
                if list_blocks
                    .iter()
                    .any(|block| block.read(cx).kind() == BlockKind::RawMarkdown)
                {
                    return None;
                }
                children.extend(list_blocks);
                saw_child = true;
                pending_blank_lines = 0;
                index = consumed;
                continue;
            }

            if parse_opening_fence(line).is_some()
                && let Some((code_block, consumed)) =
                    collect_fenced_code_block(cx, lines, index, origins)
            {
                if pending_blank_lines > 0 && (!title_markdown.is_empty() || !children.is_empty()) {
                    append_quote_separator_children(&mut children, pending_blank_lines, cx);
                }
                children.push(code_block);
                saw_child = true;
                pending_blank_lines = 0;
                index = consumed;
                continue;
            }

            if starts_with_standalone_image_child_paragraph(&lines[index..]) {
                if pending_blank_lines > 0 && (!title_markdown.is_empty() || !children.is_empty()) {
                    append_quote_separator_children(&mut children, pending_blank_lines, cx);
                }
                children.push(standalone_image_block(cx, line.to_string()));
                saw_child = true;
                pending_blank_lines = 0;
                index += 1;
                continue;
            }

            if strip_indented_code_prefix(line).is_some()
                && let Some((code_block, consumed)) = collect_indented_code_block(cx, lines, index, origins)
            {
                if pending_blank_lines > 0 && (!title_markdown.is_empty() || !children.is_empty()) {
                    append_quote_separator_children(&mut children, pending_blank_lines, cx);
                }
                children.push(code_block);
                saw_child = true;
                pending_blank_lines = 0;
                index = consumed;
                continue;
            }

            let paragraph_start = index;
            let mut paragraph_lines = vec![line.clone()];
            index += 1;
            while index < lines.len() {
                let next = &lines[index];
                if next.trim().is_empty()
                    || is_quote_start(next)
                    || parse_list_marker(next).is_some()
                    || parse_opening_fence(next).is_some()
                    || strip_indented_code_prefix(next).is_some()
                    || quote_content_starts_unsupported(lines, index)
                {
                    break;
                }

                paragraph_lines.push(next.clone());
                index += 1;
            }

            if is_standalone_image_paragraph(&paragraph_lines) {
                if pending_blank_lines > 0 && (!title_markdown.is_empty() || !children.is_empty()) {
                    append_quote_separator_children(&mut children, pending_blank_lines, cx);
                }
                children.push(standalone_image_block(cx, paragraph_lines.join("\n")));
                saw_child = true;
                pending_blank_lines = 0;
                continue;
            }

            if saw_child {
                if pending_blank_lines > 0 && (!title_markdown.is_empty() || !children.is_empty()) {
                    append_quote_separator_children(&mut children, pending_blank_lines, cx);
                }
                children.push(Self::paragraph_child_block(
                    cx,
                    paragraph_lines.join("\n"),
                    (paragraph_start..index).map(|at| inherited(at) as u32).collect(),
                ));
                pending_blank_lines = 0;
                continue;
            }

            if !title_markdown.is_empty() {
                if pending_blank_lines > 1 {
                    title_desynced = true;
                } else if pending_blank_lines == 1 {
                    // 分隔出来的那一个空内容行，就是紧挨着的前一行空行。
                    let blank = paragraph_start - 1;
                    title_prefixes.push((inherited(blank) + lines[blank].len()) as u32);
                }
                title_markdown.push_str(if pending_blank_lines > 0 {
                    "\n\n"
                } else {
                    "\n"
                });
            } else if pending_blank_lines > 0 {
                // 开头的空行在内容里没有对应行，行号从这一步就错位了。
                title_desynced = true;
            }
            title_markdown.push_str(&paragraph_lines.join("\n"));
            title_prefixes.extend((paragraph_start..index).map(|at| inherited(at) as u32));
            pending_blank_lines = 0;
        }

        if pending_blank_lines > 0 && (!title_markdown.is_empty() || !children.is_empty()) {
            append_quote_separator_children(&mut children, pending_blank_lines, cx);
        }

        let block = native_block(cx, BlockKind::Quote, title_markdown);
        if !title_desynced && !title_prefixes.is_empty() {
            block.update(cx, |block, _cx| {
                block.record.source_line_prefixes = title_prefixes;
            });
        }
        attach_child_blocks(&block, children, cx);
        Some(block)
    }

    pub(crate) fn build_native_callout_block(
        cx: &mut Context<Self>,
        lines: &[String],
        origins: &[usize],
        variant: CalloutVariant,
        title: String,
        marker: Option<SharedString>,
    ) -> Option<Entity<crate::editor::Block>> {
        let inherited = |at: usize| origins.get(at).copied().unwrap_or(0);
        let mut children = Vec::new();
        let mut index = 0usize;
        let mut pending_blank_lines = 0usize;

        while index < lines.len() {
            let line = &lines[index];
            if line.trim().is_empty() {
                pending_blank_lines += 1;
                index += 1;
                continue;
            }

            if pending_blank_lines > 0 {
                append_quote_separator_children(&mut children, pending_blank_lines, cx);
                pending_blank_lines = 0;
            }

            if is_table_candidate_line(line) {
                let table_end = collect_table_candidate_region(lines, index);
                let table_region = &lines[index..table_end];
                if let Some(table) = parse_table_region(table_region) {
                    children.push(Self::new_block(cx, BlockRecord::table(table)));
                } else {
                    children.push(raw_block_from_region(cx, lines, origins, index..table_end));
                }
                index = table_end;
                continue;
            }

            if is_footnote_definition_start(line) {
                let footnote_end = collect_footnote_definition_region(lines, index);
                if let Some(footnote) =
                    build_native_footnote_definition_block(cx, &lines[index..footnote_end])
                {
                    children.push(footnote);
                    index = footnote_end;
                    continue;
                }
            }

            if let Some((comment, consumed)) = collect_comment_block(cx, lines, index, origins) {
                children.push(comment);
                index = consumed;
                continue;
            }

            if is_block_html_start(line) {
                let html_end = collect_block_html_region(lines, index);
                children.push(html_or_raw_block_from_region(cx, lines, origins, index..html_end));
                index = html_end;
                continue;
            }

            if is_display_math_start(line) {
                let math_end = collect_display_math_region(lines, index);
                children.push(math_or_raw_block_from_region(cx, lines, origins, index..math_end));
                index = math_end;
                continue;
            }

            if let Some(unsupported_end) = collect_unsupported_quote_region(lines, index) {
                children.push(raw_block_from_region(cx, lines, origins, index..unsupported_end));
                index = unsupported_end;
                continue;
            }

            if is_quote_start(line) {
                let (quote, consumed) = Self::collect_quote_block(cx, lines, index, origins);
                if quote.read(cx).kind() == BlockKind::RawMarkdown {
                    return None;
                }
                children.push(quote);
                index = consumed;
                continue;
            }

            if parse_list_marker(line).is_some() {
                let (list_blocks, _nested_spans, consumed) =
                    Self::collect_list_blocks(cx, lines, index, usize::MAX, origins);
                if list_blocks
                    .iter()
                    .any(|block| block.read(cx).kind() == BlockKind::RawMarkdown)
                {
                    return None;
                }
                children.extend(list_blocks);
                index = consumed;
                continue;
            }

            if parse_opening_fence(line).is_some()
                && let Some((code_block, consumed)) =
                    collect_fenced_code_block(cx, lines, index, origins)
            {
                children.push(code_block);
                index = consumed;
                continue;
            }

            if starts_with_standalone_image_child_paragraph(&lines[index..]) {
                children.push(standalone_image_block(cx, line.to_string()));
                index += 1;
                continue;
            }

            if strip_indented_code_prefix(line).is_some()
                && let Some((code_block, consumed)) = collect_indented_code_block(cx, lines, index, origins)
            {
                children.push(code_block);
                index = consumed;
                continue;
            }

            let paragraph_start = index;
            let mut paragraph_lines = vec![line.clone()];
            index += 1;
            while index < lines.len() {
                let next = &lines[index];
                if next.trim().is_empty()
                    || is_quote_start(next)
                    || parse_list_marker(next).is_some()
                    || parse_opening_fence(next).is_some()
                    || strip_indented_code_prefix(next).is_some()
                    || quote_content_starts_unsupported(lines, index)
                {
                    break;
                }

                paragraph_lines.push(next.clone());
                index += 1;
            }

            children.push(Self::paragraph_child_block(
                cx,
                paragraph_lines.join("\n"),
                (paragraph_start..index).map(|at| inherited(at) as u32).collect(),
            ));
        }

        if pending_blank_lines > 0 {
            append_quote_separator_children(&mut children, pending_blank_lines, cx);
        }

        let block = Editor::new_block(
            cx,
            BlockRecord::new(
                BlockKind::Callout(variant),
                InlineTextTree::from_markdown(&title),
            ),
        );
        block.update(cx, |block, _cx| {
            block.record.callout_marker = marker;
        });
        attach_child_blocks(&block, children, cx);
        Some(block)
    }

    pub(crate) fn collect_list_blocks(
        cx: &mut Context<Self>,
        lines: &[String],
        start: usize,
        item_budget: usize,
        origins: &[usize],
    ) -> (
        Vec<Entity<crate::editor::Block>>,
        Vec<std::ops::Range<usize>>,
        usize,
    ) {
        let mut roots = Vec::new();
        // 与 `roots` 同步：每个顶层项消费的行区间。递归进去的子项区间用不上
        // （子项在父项区间里），所以只在顶层这一层记账。
        let mut spans: Vec<std::ops::Range<usize>> = Vec::new();
        let mut index = start;
        // 「这一行已经被上级容器吃掉了几个字节」。根块传的是空表（一律 0）。
        let inherited = |at: usize| origins.get(at).copied().unwrap_or(0);

        while index < lines.len() {
            // Stop on a top-level item boundary once this call has built its
            // share; the next chunk resumes at this marker line, which parses
            // the same way a full pass would (every item is self-contained).
            if roots.len() >= item_budget {
                break;
            }
            let Some(marker) = parse_list_marker(&lines[index]) else {
                break;
            };

            let item_end = collect_list_item_region(lines, index, marker.indent_columns);

            // 公式直接跟在项标记后面（`- $$ ... $$`）：项自己的正文就是公式开头，
            // 而 `collect_list_item_region` 已把后续缩进行归入本项。把这两部分合成
            // 一块公式，否则 `$$` 会留在项文本里原样显示成源码。
            let item_math = if is_display_math_start_at_any_indent(&marker.text) {
                let mut region = vec![marker.text.clone()];
                region.extend(dedent_lines(
                    &lines[index + 1..item_end],
                    marker.content_indent_columns,
                ));
                let consumed = collect_display_math_region(&region, 0);
                let markdown = dedent_math_region(&region[..consumed]);
                parse_display_math_source(&markdown).is_some().then_some((markdown, consumed))
            } else {
                None
            };

            // 项标记（缩进 + 子弹/序号 + 它后面那个空格 + 任务框）在本行里占几位，
            // 是剥它的那段代码当场知道的事实：记下来给位置换算用，别再事后比。
            // 项正文接着被并进公式块的那种情形不算——那一行的内容不在块里。
            let marker_is_math = item_math.is_some();
            let marker_width = lines[index].len() - marker.text.len();
            let item_text = if item_math.is_some() {
                String::new()
            } else {
                marker.text
            };
            let block = native_block(cx, marker.kind.clone(), item_text);
            // 记号的写法是原文的一部分：块带着它，显示与序列化才不改用户写的 `+`、`1)`。
            // 有序项写下的那个号也一起带上（`5. 一` 的 5），界面才不至于把它数回 1。
            block.update(cx, |block, _cx| {
                block.record.list_marker = marker.style;
                block.record.list_start = marker.numbered_start;
            });
            // 项自己内容的每一行让开几字节，一行一行跟着 `append_markdown_to_block` 记。
            // 空行段超过一行时模型里的空行比文件里的少，行号对不上，整块的账作废。
            let mut item_prefixes: Vec<u32> =
                if marker_is_math { Vec::new() } else { vec![(inherited(index) + marker_width) as u32] };
            let mut prefixes_desynced = marker_is_math;
            let mut body_index = index + 1;
            let mut pending_blank_lines = 0usize;
            let mut fallback_raw = false;
            let mut saw_child = false;

            if let Some((markdown, consumed)) = item_math {
                attach_child_blocks(&block, vec![math_or_raw_block(cx, markdown)], cx);
                // 区域第一行是项自己的标记行，后续行从 `index + consumed` 接着看。
                body_index = index + consumed;
                saw_child = true;
            }

            while body_index < item_end {
                let line = &lines[body_index];
                if line.trim().is_empty() {
                    pending_blank_lines += 1;
                    body_index += 1;
                    continue;
                }

                let (line_indent_columns, _) = leading_indent_columns_and_bytes(line);
                if line_indent_columns > marker.indent_columns {
                    let range = body_index..item_end;
                    let slice_origins: Vec<usize> =
                        range.clone().map(inherited).collect();
                    let (anchor_dedented, anchor_origins) = dedent_lines_with_origins(
                        &lines[range],
                        line_indent_columns,
                        &slice_origins,
                    );

                    if parse_list_marker(&anchor_dedented[0]).is_some() {
                        let (children, _child_spans, consumed) = Self::collect_list_blocks(
                            cx,
                            &anchor_dedented,
                            0,
                            usize::MAX,
                            &anchor_origins,
                        );
                        attach_item_child(&block, children, lines, origins, body_index, pending_blank_lines, cx);
                        body_index += consumed;
                        pending_blank_lines = 0;
                        saw_child = true;
                        continue;
                    }

                    if is_quote_start(&anchor_dedented[0]) {
                        let (quote, consumed) =
                            Self::collect_quote_block(cx, &anchor_dedented, 0, &anchor_origins);
                        if quote.read(cx).kind() == BlockKind::RawMarkdown {
                            fallback_raw = true;
                            break;
                        }

                        attach_item_child(&block, vec![quote], lines, origins, body_index, pending_blank_lines, cx);
                        body_index += consumed;
                        pending_blank_lines = 0;
                        saw_child = true;
                        continue;
                    }

                    if parse_opening_fence(&anchor_dedented[0]).is_some()
                        && let Some((code_block, consumed)) =
                            collect_fenced_code_block(cx, &anchor_dedented, 0, &anchor_origins)
                    {
                        attach_item_child(&block, vec![code_block], lines, origins, body_index, pending_blank_lines, cx);
                        body_index += consumed;
                        pending_blank_lines = 0;
                        saw_child = true;
                        continue;
                    }

                    if is_root_table_candidate_line(&anchor_dedented[0]) {
                        let table_end = collect_root_table_candidate_region(&anchor_dedented, 0);
                        let table_region = &anchor_dedented[..table_end];
                        let child = if let Some(table) = parse_root_table_region(table_region) {
                            Self::new_block(cx, BlockRecord::table(table))
                        } else {
                            raw_block_from_region(cx, &anchor_dedented, &anchor_origins, 0..table_end)
                        };
                        attach_item_child(&block, vec![child], lines, origins, body_index, pending_blank_lines, cx);
                        body_index += table_end;
                        pending_blank_lines = 0;
                        saw_child = true;
                        continue;
                    }

                    if starts_with_standalone_image_child_paragraph(&anchor_dedented) {
                        attach_item_child(
                            &block,
                            vec![standalone_image_block(cx, anchor_dedented[0].clone())],
                            lines, origins, body_index, pending_blank_lines, cx,
                        );
                        body_index += 1;
                        pending_blank_lines = 0;
                        saw_child = true;
                        continue;
                    }

                    if line_indent_columns >= marker.content_indent_columns {
                        let content_range = body_index..item_end;
                        let content_slice_origins: Vec<usize> =
                            content_range.clone().map(inherited).collect();
                        let (content_dedented, content_origins) = dedent_lines_with_origins(
                            &lines[content_range],
                            marker.content_indent_columns,
                            &content_slice_origins,
                        );
                        if strip_indented_code_prefix(&content_dedented[0]).is_some() {
                            let Some((code_block, consumed)) =
                                collect_indented_code_block(cx, &content_dedented, 0, &content_origins)
                            else {
                                unreachable!(
                                    "indented code prefix disappeared after child detection"
                                );
                            };

                            attach_item_child(&block, vec![code_block], lines, origins, body_index, pending_blank_lines, cx);
                            body_index += consumed;
                            pending_blank_lines = 0;
                            saw_child = true;
                            continue;
                        }
                    }

                    if is_reference_definition_start(&anchor_dedented[0]) {
                        let consumed = collect_reference_definition_region(&anchor_dedented, 0);
                        attach_item_child(
                            &block,
                            vec![raw_block_from_region(cx, &anchor_dedented, &anchor_origins, 0..consumed)],
                            lines, origins, body_index, pending_blank_lines, cx,
                        );
                        body_index += consumed;
                        pending_blank_lines = 0;
                        saw_child = true;
                        continue;
                    }

                    if let Some((comment, consumed)) =
                        collect_comment_block(cx, &anchor_dedented, 0, &anchor_origins)
                    {
                        attach_item_child(&block, vec![comment], lines, origins, body_index, pending_blank_lines, cx);
                        body_index += consumed;
                        pending_blank_lines = 0;
                        saw_child = true;
                        continue;
                    }

                    if is_block_html_start(&anchor_dedented[0]) {
                        let consumed = collect_block_html_region(&anchor_dedented, 0);
                        attach_item_child(
                            &block,
                            vec![html_or_raw_block_from_region(
                                cx,
                                &anchor_dedented,
                                &anchor_origins,
                                0..consumed,
                            )],
                            lines,
                            origins,
                            body_index,
                            pending_blank_lines,
                            cx,
                        );
                        body_index += consumed;
                        pending_blank_lines = 0;
                        saw_child = true;
                        continue;
                    }

                    if is_footnote_definition_start(&anchor_dedented[0]) {
                        let consumed = collect_footnote_definition_region(&anchor_dedented, 0);
                        attach_item_child(
                            &block,
                            vec![raw_block_from_region(cx, &anchor_dedented, &anchor_origins, 0..consumed)],
                            lines, origins, body_index, pending_blank_lines, cx,
                        );
                        body_index += consumed;
                        pending_blank_lines = 0;
                        saw_child = true;
                        continue;
                    }

                    if is_display_math_start(&anchor_dedented[0]) {
                        let consumed = collect_display_math_region(&anchor_dedented, 0);
                        attach_item_child(
                            &block,
                            vec![math_or_raw_block(
                                cx,
                                dedent_math_region(&anchor_dedented[..consumed]),
                            )],
                            lines,
                            origins,
                            body_index,
                            pending_blank_lines,
                            cx,
                        );
                        body_index += consumed;
                        pending_blank_lines = 0;
                        saw_child = true;
                        continue;
                    }

                    let should_promote_plain_child = pending_blank_lines > 0
                        || saw_child
                        || block.read(cx).display_text().is_empty()
                        || parse_standalone_image(&block.read(cx).record.title_markdown())
                            .is_some();
                    if should_promote_plain_child {
                        let (paragraph, consumed) =
                            Self::collect_paragraph_block(cx, &anchor_dedented, 0, &anchor_origins);
                        attach_item_child(&block, vec![paragraph], lines, origins, body_index, pending_blank_lines, cx);
                        body_index += consumed;
                        pending_blank_lines = 0;
                        saw_child = true;
                        continue;
                    }
                }

                if line_indent_columns >= marker.content_indent_columns {
                    let content_range = body_index..item_end;
                    let content_slice_origins: Vec<usize> =
                        content_range.clone().map(inherited).collect();
                    let (content_dedented, content_origins) = dedent_lines_with_origins(
                        &lines[content_range],
                        marker.content_indent_columns,
                        &content_slice_origins,
                    );
                    if strip_indented_code_prefix(&content_dedented[0]).is_some() {
                        let Some((code_block, consumed)) =
                            collect_indented_code_block(cx, &content_dedented, 0, &content_origins)
                        else {
                            unreachable!("indented code prefix disappeared after detection");
                        };

                        attach_item_child(&block, vec![code_block], lines, origins, body_index, pending_blank_lines, cx);
                        body_index += consumed;
                        pending_blank_lines = 0;
                        saw_child = true;
                        continue;
                    }
                }

                let trimmed = line.trim_start_matches([' ', '\t']);
                append_markdown_to_block(
                    &block,
                    if pending_blank_lines > 0 {
                        "\n\n"
                    } else {
                        "\n"
                    },
                    trimmed,
                    cx,
                );
                if pending_blank_lines == 1 {
                    // 分隔出来的那一个空内容行，就是文件里紧挨着的前一行空行。
                    let blank = body_index - 1;
                    item_prefixes.push((inherited(blank) + lines[blank].len()) as u32);
                } else if pending_blank_lines > 1 {
                    prefixes_desynced = true;
                }
                item_prefixes
                    .push((inherited(body_index) + (line.len() - trimmed.len())) as u32);
                pending_blank_lines = 0;
                body_index += 1;
            }

            if fallback_raw {
                roots.push(raw_block(cx, lines[index..item_end].join("\n")));
            } else {
                if !prefixes_desynced {
                    block.update(cx, |block, _cx| {
                        block.record.source_line_prefixes = item_prefixes;
                    });
                }
                roots.push(block);
            }
            spans.push(index..item_end);
            index = item_end;
        }

        debug_assert_eq!(
            roots.len(),
            spans.len(),
            "列表项与源码区间必须一一对应"
        );
        (roots, spans, index)
    }
}
