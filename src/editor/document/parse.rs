use super::*;

pub(crate) struct ListMarker {
    pub(crate) kind: BlockKind,
    pub(crate) indent_columns: usize,
    pub(crate) content_indent_columns: usize,
    /// 记号的写法（`+`/`*`/`-`、`.`/`)`），块记录带着它才能显示与序列化都不改用户的原文。
    pub(crate) style: ListMarkerStyle,
    pub(crate) text: String,
}

pub(crate) fn strip_fence_indent(line: &str) -> Option<&str> {
    let indent = line.bytes().take_while(|b| *b == b' ').count();
    (indent <= 3).then_some(&line[indent..])
}

pub(crate) fn collect_html_fallback_region(lines: &[String], start: usize) -> usize {
    let mut index = start + 1;
    while index < lines.len() {
        if lines[index].trim().is_empty()
            || looks_like_root_block_start(lines, index)
            || parse_standalone_image(&lines[index]).is_some()
        {
            break;
        }
        index += 1;
    }
    index
}

pub(crate) fn pending_inline_code_run_len(markdown: &str) -> Option<usize> {
    let mut open_run_len = None;
    let mut chars = markdown.char_indices().peekable();

    while let Some((_, ch)) = chars.next() {
        if open_run_len.is_none() && ch == '\\' {
            let _ = chars.next();
            continue;
        }

        if ch != '`' {
            continue;
        }

        let mut run_len = 1usize;
        while chars.peek().is_some_and(|(_, ch)| *ch == '`') {
            let _ = chars.next();
            run_len += 1;
        }

        if open_run_len == Some(run_len) {
            open_run_len = None;
        } else if open_run_len.is_none() {
            open_run_len = Some(run_len);
        }
    }

    open_run_len
}

pub(crate) fn line_contains_matching_backtick_run(line: &str, run_len: usize) -> bool {
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '`' {
            continue;
        }

        let mut current_run_len = 1usize;
        while chars.peek().is_some_and(|ch| *ch == '`') {
            let _ = chars.next();
            current_run_len += 1;
        }

        if current_run_len == run_len {
            return true;
        }
    }

    false
}

pub(crate) fn paragraph_can_continue_through_boundary(
    paragraph_lines: &[String],
    lines: &[String],
    boundary_index: usize,
) -> bool {
    let Some(run_len) = pending_inline_code_run_len(&paragraph_lines.join("\n")) else {
        return false;
    };

    lines[boundary_index..]
        .iter()
        .any(|line| line_contains_matching_backtick_run(line, run_len))
}

pub(crate) fn parse_opening_fence(line: &str) -> Option<FenceInfo> {
    BlockKind::parse_code_fence_opening(strip_fence_indent(line)?.trim_end())
}

pub(crate) fn is_closing_fence(line: &str, opener: &FenceInfo) -> bool {
    let Some(trimmed) = strip_fence_indent(line).map(str::trim_end) else {
        return false;
    };
    if !trimmed.starts_with(opener.ch) {
        return false;
    }
    let run_len = trimmed.chars().take_while(|&c| c == opener.ch).count();
    if run_len != opener.len {
        return false;
    }
    trimmed[opener.ch.len_utf8() * run_len..].trim().is_empty()
}

pub(crate) fn find_matching_closing_fence(
    lines: &[String],
    start_index: usize,
    opener: &FenceInfo,
) -> Option<usize> {
    for index in (start_index + 1)..lines.len() {
        let line = &lines[index];
        // A fenced block closes at its first matching fence, as in CommonMark.
        // Scanning for a later fence (the previous behavior) let any opener
        // swallow the following blocks whose closing fences are bare, merging
        // them and corrupting them on round-trip (issue #58). A bare closing
        // fence is indistinguishable from an empty opener, so first-match is
        // the only unambiguous rule.
        if is_closing_fence(line, opener) {
            return Some(index);
        }

        // An info-tagged opener can never be a closing fence, so reaching one
        // first means this block was never closed and stays unmatched.
        if parse_opening_fence(line)
            .as_ref()
            .and_then(|fence| fence.language.as_ref())
            .is_some()
        {
            break;
        }
    }

    None
}

pub(crate) fn is_fenced_div_opening(line: &str) -> bool {
    strip_fence_indent(line)
        .and_then(|line| line.strip_prefix(":::"))
        .is_some_and(|suffix| !suffix.trim().is_empty())
}

pub(crate) fn is_fenced_div_closing(line: &str) -> bool {
    strip_fence_indent(line).is_some_and(|line| line.trim() == ":::")
}

pub(crate) fn collect_fenced_div_end(lines: &[String], start: usize) -> Option<usize> {
    let mut depth = 1usize;
    for (index, line) in lines.iter().enumerate().skip(start + 1) {
        if is_fenced_div_opening(line) {
            depth += 1;
        } else if is_fenced_div_closing(line) {
            depth -= 1;
            if depth == 0 {
                return Some(index + 1);
            }
        }
    }
    None
}

pub(crate) fn is_unsupported_admonition_opening(line: &str) -> bool {
    strip_fence_indent(line).is_some_and(|line| {
        let line = line.trim_start();
        line.starts_with("!!!") || line.starts_with("???")
    })
}

pub(crate) fn leading_indent_columns_and_bytes(line: &str) -> (usize, usize) {
    let mut columns = 0usize;
    let mut bytes = 0usize;
    for ch in line.chars() {
        match ch {
            ' ' => {
                columns += 1;
                bytes += 1;
            }
            '\t' => {
                columns += 4 - (columns % 4);
                bytes += 1;
            }
            _ => break,
        }
    }
    (columns, bytes)
}

pub(crate) fn strip_indented_code_prefix(line: &str) -> Option<&str> {
    if let Some(rest) = line.strip_prefix('\t') {
        Some(rest)
    } else {
        line.strip_prefix("    ")
    }
}

pub(crate) fn display_columns(value: &str) -> usize {
    let mut columns = 0usize;
    for ch in value.chars() {
        match ch {
            '\t' => columns += 4 - (columns % 4),
            _ => columns += 1,
        }
    }
    columns
}

pub(crate) fn strip_leading_columns(line: &str, columns: usize) -> Option<&str> {
    if columns == 0 {
        return Some(line);
    }
    if line.trim().is_empty() {
        return Some("");
    }

    let mut consumed_columns = 0usize;
    for (idx, ch) in line.char_indices() {
        let bytes_after_char = idx + ch.len_utf8();
        match ch {
            ' ' => {
                consumed_columns += 1;
            }
            '\t' => {
                consumed_columns += 4 - (consumed_columns % 4);
            }
            _ => break,
        }

        if consumed_columns >= columns {
            return Some(&line[bytes_after_char..]);
        }
    }

    None
}

/// `dedent_lines` 的同款，另外把「这一行的内容在文件那一行里已经让开了几字节」一起算出来。
///
/// 上级容器（引用的 `> `、列表的缩进）剥掉的字节是解析期就知道的事实；带着它递归下去，
/// 块就能把自己每一行的记号宽度记成**文件口径**的绝对值（`BlockRecord::source_line_prefixes`），
/// 位置换算不必事后拿文件行与模型行比。`origins` 短于 `lines` 时缺的部分按 0 算。
pub(crate) fn dedent_lines_with_origins(
    lines: &[String],
    columns: usize,
    origins: &[usize],
) -> (Vec<String>, Vec<usize>) {
    let mut texts = Vec::with_capacity(lines.len());
    let mut moved = Vec::with_capacity(lines.len());
    for (index, line) in lines.iter().enumerate() {
        let inherited = origins.get(index).copied().unwrap_or(0);
        match strip_leading_columns(line, columns) {
            Some(dedented) => {
                moved.push(inherited + (line.len() - dedented.len()));
                texts.push(dedented.to_string());
            }
            None => {
                moved.push(inherited);
                texts.push(line.clone());
            }
        }
    }
    (texts, moved)
}

pub(crate) fn dedent_lines(lines: &[String], columns: usize) -> Vec<String> {
    lines
        .iter()
        .map(|line| {
            strip_leading_columns(line, columns)
                .unwrap_or(line.as_str())
                .to_string()
        })
        .collect()
}

/// Whether a line opens a list item, using the same oracle the importer uses
/// when it decides whether a blank run continues the current list.
///
/// The serializer needs it for a not-yet-built tail: the separator before the
/// tail follows the same list-group rule as the separator before a parsed root.
pub(crate) fn line_is_list_marker(line: &str) -> bool {
    parse_list_marker(line).is_some()
}

pub(crate) fn parse_list_marker(line: &str) -> Option<ListMarker> {
    let (indent_columns, indent_bytes) = leading_indent_columns_and_bytes(line);
    let rest = &line[indent_bytes..];

    if let Some(marker) = rest.chars().next()
        && matches!(marker, '-' | '*' | '+')
    {
        let after_marker = &rest[marker.len_utf8()..];
        let separator_len = after_marker
            .chars()
            .next()
            .filter(|ch| matches!(ch, ' ' | '\t'))
            .map(char::len_utf8)?;
        let text = after_marker
            .strip_prefix(' ')
            .or_else(|| after_marker.strip_prefix('\t'))?;
        let (kind, text) =
            if let Some((checked, prefix_len)) = BlockKind::parse_task_list_item_prefix(text) {
                (
                    BlockKind::TaskListItem { checked },
                    text[prefix_len..].to_string(),
                )
            } else {
                (BlockKind::BulletedListItem, text.to_string())
            };
        return Some(ListMarker {
            kind,
            indent_columns,
            content_indent_columns: display_columns(
                &line[..indent_bytes + marker.len_utf8() + separator_len],
            ),
            style: ListMarkerStyle {
                bullet: Some(marker),
                delimiter: None,
            },
            text,
        });
    }

    let (digit_len, marker_len, text) = parse_ordered_list_marker(rest)?;
    let delimiter = match rest.as_bytes().get(digit_len) {
        Some(b'.') => '.',
        Some(b')') => ')',
        _ => return None,
    };
    Some(ListMarker {
        kind: BlockKind::NumberedListItem,
        indent_columns,
        content_indent_columns: display_columns(&line[..indent_bytes + digit_len + marker_len]),
        style: ListMarkerStyle {
            bullet: None,
            delimiter: Some(delimiter),
        },
        text: text.to_string(),
    })
}

pub(crate) fn parse_ordered_list_marker(rest: &str) -> Option<(usize, usize, &str)> {
    let digit_len = rest.bytes().take_while(|b| b.is_ascii_digit()).count();
    if !(1..=9).contains(&digit_len) {
        return None;
    }

    let marker = *rest.as_bytes().get(digit_len)?;
    if !matches!(marker, b'.' | b')') {
        return None;
    }

    let separator = *rest.as_bytes().get(digit_len + 1)?;
    if !matches!(separator, b' ' | b'\t') {
        return None;
    }

    Some((digit_len, 2, &rest[digit_len + 2..]))
}

pub(crate) fn strip_one_quote_level(line: &str) -> Option<String> {
    let leading_spaces = line.bytes().take_while(|b| *b == b' ').count();
    if leading_spaces > 3 {
        return None;
    }

    let rest = &line[leading_spaces..];
    if !rest.starts_with('>') {
        return None;
    }

    Some(
        rest[1..]
            .strip_prefix(' ')
            .unwrap_or(&rest[1..])
            .to_string(),
    )
}

pub(crate) fn is_quote_start(line: &str) -> bool {
    let trimmed_end = line.trim_end();
    let leading_spaces = trimmed_end.bytes().take_while(|b| *b == b' ').count();
    leading_spaces <= 3 && trimmed_end[leading_spaces..].starts_with('>')
}

pub(crate) fn is_reference_definition_start(line: &str) -> bool {
    let trimmed_end = line.trim_end();
    let leading_spaces = trimmed_end.bytes().take_while(|b| *b == b' ').count();
    if leading_spaces > 3 {
        return false;
    }

    let rest = &trimmed_end[leading_spaces..];
    let Some(label_end) = rest.find("]:") else {
        return false;
    };
    rest.starts_with('[') && label_end > 1
}

pub(crate) fn is_footnote_definition_start(line: &str) -> bool {
    let trimmed_end = line.trim_end();
    let leading_spaces = trimmed_end.bytes().take_while(|b| *b == b' ').count();
    if leading_spaces > 3 {
        return false;
    }

    let rest = &trimmed_end[leading_spaces..];
    let Some(label_end) = rest.find("]:") else {
        return false;
    };
    rest.starts_with("[^") && label_end > 2
}

pub(crate) fn is_reference_definition_title_continuation(line: &str) -> bool {
    let (_, indent_bytes) = leading_indent_columns_and_bytes(line);
    if indent_bytes == 0 {
        return false;
    }

    let trimmed = line[indent_bytes..].trim();
    (trimmed.starts_with('"') && trimmed.ends_with('"'))
        || (trimmed.starts_with('\'') && trimmed.ends_with('\''))
        || (trimmed.starts_with('(') && trimmed.ends_with(')'))
}

pub(crate) fn is_block_html_start(line: &str) -> bool {
    parse_html_block_start(line).is_some()
}

pub(crate) fn collect_closed_html_comment_region(lines: &[String], start: usize) -> Option<usize> {
    match parse_html_block_start(&lines[start])? {
        HtmlBlockStart::Comment => {}
        HtmlBlockStart::Tag { .. } => return None,
    }

    if lines[start].contains("-->") {
        return Some(start + 1);
    }

    let mut index = start + 1;
    while index < lines.len() {
        if lines[index].contains("-->") {
            return Some(index + 1);
        }
        index += 1;
    }

    None
}

pub(crate) fn collect_block_html_region(lines: &[String], start: usize) -> usize {
    match parse_html_block_start(&lines[start]) {
        Some(HtmlBlockStart::Comment) => collect_closed_html_comment_region(lines, start)
            .unwrap_or_else(|| collect_html_fallback_region(lines, start)),
        Some(HtmlBlockStart::Tag {
            self_closing,
            closes_same_line,
            ..
        }) if self_closing || closes_same_line => start + 1,
        Some(HtmlBlockStart::Tag { name, .. })
            if is_raw_text_html_tag(&name) || is_html_container_tag(&name) =>
        {
            collect_raw_text_html_region(lines, start, &name)
        }
        // CommonMark HTML block kinds 6 and 7 end at the next blank line.
        Some(HtmlBlockStart::Tag { .. }) | None => collect_markdown_html_region(lines, start),
    }
}

/// CommonMark HTML block kind 1: the region runs to the matching end tag, and
/// blank lines inside it do not end the block.
pub(crate) fn collect_raw_text_html_region(lines: &[String], start: usize, name: &str) -> usize {
    let mut index = start + 1;
    while index < lines.len() {
        if parse_html_close_tag_name(&lines[index]).is_some_and(|close| close == name) {
            return index + 1;
        }
        index += 1;
    }
    collect_html_fallback_region(lines, start)
}

/// HTML blocks that end at the next blank line. A standalone image line also
/// ends the region so it stays a native image block.
pub(crate) fn collect_markdown_html_region(lines: &[String], start: usize) -> usize {
    let mut index = start + 1;
    while index < lines.len() {
        if lines[index].trim().is_empty() || parse_standalone_image(&lines[index]).is_some() {
            break;
        }
        index += 1;
    }
    index
}

pub(crate) fn collect_reference_definition_region(lines: &[String], start: usize) -> usize {
    let mut index = start + 1;
    while index < lines.len() && is_reference_definition_title_continuation(&lines[index]) {
        index += 1;
    }
    index
}

pub(crate) fn collect_footnote_definition_region(lines: &[String], start: usize) -> usize {
    let mut index = start + 1;
    while index < lines.len() {
        let line = &lines[index];
        if line.trim().is_empty() {
            index += 1;
            continue;
        }

        let (indent_columns, _) = leading_indent_columns_and_bytes(line);
        if indent_columns > 0 {
            index += 1;
            continue;
        }

        break;
    }
    index
}

pub(crate) fn is_display_math_start(line: &str) -> bool {
    strip_fence_indent(line)
        .map(str::trim_end)
        .is_some_and(|rest| rest.starts_with("$$"))
}

/// `$$` 起始行、允许任意缩进。
///
/// 人们常把公式写在列表项或缩进块里（Windows 上 Typora 粘过来的文档尤其多），
/// 这类行的缩进可能是 4 个空格或制表符；`is_display_math_start` 只认 ≤3 空格，
/// 于是整块公式会掉进「4 空格缩进代码块」分支、原样显示成代码。
pub(crate) fn is_display_math_start_at_any_indent(line: &str) -> bool {
    line.trim_start().starts_with("$$")
}

/// 把公式块区域整体去掉公共缩进后拼成 Markdown 文本。
///
/// `parse_display_math_source` 只接受 ≤3 空格缩进的 `$$`，这里先按区域里**最少**的
/// 非空行缩进去缩进，缩进 4 格以上的公式块也能识别。
pub(crate) fn dedent_math_region(region: &[String]) -> String {
    let indent = region
        .iter()
        .filter(|line| !line.trim().is_empty())
        .map(|line| leading_indent_columns_and_bytes(line).0)
        .min()
        .unwrap_or(0);
    dedent_lines(region, indent).join("\n")
}

pub(crate) fn collect_display_math_region(lines: &[String], start: usize) -> usize {
    // 两端都 trim：调用方可能带着任意缩进过来（`  $$` / `    $$`），
    // 只去掉尾部空白会把缩进当成「同一行里有第二个 `$$`」。
    let opener = lines[start].trim();
    if opener != "$$" && opener.get(2..).is_some_and(|rest| rest.contains("$$")) {
        return start + 1;
    }

    let mut index = start + 1;
    while index < lines.len() {
        // 结束行不要求独占一行：`\end{aligned}$$` 也算收尾。
        if lines[index].trim_end().ends_with("$$") {
            return index + 1;
        }

        if lines[index].trim().is_empty() {
            let mut lookahead = index + 1;
            while lookahead < lines.len() && lines[lookahead].trim().is_empty() {
                lookahead += 1;
            }

            if lookahead >= lines.len() || looks_like_root_block_start(lines, lookahead) {
                return lookahead;
            }
        }

        index += 1;
    }

    lines.len()
}

pub(crate) fn parse_html_block_start(line: &str) -> Option<HtmlBlockStart> {
    let rest = strip_fence_indent(line)?.trim_end();
    if rest.starts_with("<!--") {
        return Some(HtmlBlockStart::Comment);
    }

    let tagged = rest.strip_prefix('<')?;
    let closing = tagged.starts_with('/');
    let tagged = tagged.strip_prefix('/').unwrap_or(tagged);

    let name_len = tagged
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '-')
        .count();
    if name_len == 0 {
        return None;
    }

    let name = &tagged[..name_len];
    let suffix = &tagged[name_len..];
    let next = suffix.chars().next()?;
    if !matches!(next, '>' | ' ' | '\t' | '/') {
        return None;
    }

    if closing {
        // Only the CommonMark block-level names open a block; other closing
        // tags stay inline text. A closing tag never wraps following lines.
        return is_block_level_html_tag(name).then(|| HtmlBlockStart::Tag {
            name: name.to_ascii_lowercase(),
            self_closing: true,
            closes_same_line: true,
        });
    }

    Some(HtmlBlockStart::Tag {
        name: name.to_string(),
        self_closing: rest.ends_with("/>") || is_html_void_block_tag(name),
        closes_same_line: rest.contains(&format!("</{name}>")),
    })
}

pub(crate) fn is_html_void_block_tag(name: &str) -> bool {
    matches!(name.to_ascii_lowercase().as_str(), "br" | "hr" | "img")
}

pub(crate) fn parse_html_close_tag_name(line: &str) -> Option<String> {
    let rest = strip_fence_indent(line)?.trim_end();
    let tagged = rest.strip_prefix("</")?;
    let name_len = tagged
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '-')
        .count();
    if name_len == 0 {
        return None;
    }

    let name = &tagged[..name_len];
    let suffix = &tagged[name_len..];
    let next = suffix.chars().next()?;
    if !matches!(next, '>' | ' ' | '\t') {
        return None;
    }

    Some(name.to_string())
}

pub(crate) fn collect_quote_raw_region(lines: &[String], start: usize) -> usize {
    let mut index = start;
    while index < lines.len() {
        let line = &lines[index];
        if line.trim().is_empty() || !is_quote_start(line) {
            break;
        }
        index += 1;
    }
    index
}

pub(crate) fn quote_content_starts_unsupported(lines: &[String], index: usize) -> bool {
    let line = &lines[index];
    is_block_html_start(line)
        || is_footnote_definition_start(line)
        || is_reference_definition_start(line)
        || is_root_table_candidate_line(line)
        || is_display_math_start(line)
        || BlockKind::parse_atx_heading_line(line).is_some()
        || BlockKind::parse_separator_line(line)
        || lines
            .get(index + 1)
            .and_then(|next| BlockKind::parse_setext_underline(next))
            .is_some()
}

pub(crate) fn collect_unsupported_quote_region(lines: &[String], start: usize) -> Option<usize> {
    if start >= lines.len() {
        return None;
    }

    let line = &lines[start];
    if is_block_html_start(line) {
        return Some(collect_block_html_region(lines, start));
    }
    if is_footnote_definition_start(line) {
        return Some(collect_footnote_definition_region(lines, start));
    }
    if is_reference_definition_start(line) {
        return Some(collect_reference_definition_region(lines, start));
    }
    if is_root_table_candidate_line(line) {
        return Some(collect_root_table_candidate_region(lines, start));
    }
    if is_display_math_start(line) {
        return Some(collect_display_math_region(lines, start));
    }
    if BlockKind::parse_atx_heading_line(line).is_some() || BlockKind::parse_separator_line(line) {
        return Some(start + 1);
    }
    if lines
        .get(start + 1)
        .and_then(|next| BlockKind::parse_setext_underline(next))
        .is_some()
    {
        return Some((start + 2).min(lines.len()));
    }

    None
}

pub(crate) fn collect_list_item_region(lines: &[String], start: usize, marker_indent_columns: usize) -> usize {
    let mut index = start + 1;
    let mut pending_blank_lines = 0usize;
    while index < lines.len() {
        let line = &lines[index];
        if line.trim().is_empty() {
            pending_blank_lines += 1;
            index += 1;
            continue;
        }

        if parse_list_marker(line)
            .is_some_and(|marker| marker.indent_columns <= marker_indent_columns)
        {
            return index.saturating_sub(pending_blank_lines);
        }

        if parse_list_marker(line).is_some() {
            pending_blank_lines = 0;
            index += 1;
            continue;
        }

        let (indent_columns, _) = leading_indent_columns_and_bytes(line);
        if indent_columns > marker_indent_columns || pending_blank_lines == 0 {
            pending_blank_lines = 0;
            index += 1;
            continue;
        }

        return index.saturating_sub(pending_blank_lines);
    }
    index
}

pub(crate) fn looks_like_root_block_start(lines: &[String], index: usize) -> bool {
    let line = &lines[index];
    if line.trim().is_empty() {
        return true;
    }

    parse_opening_fence(line).is_some()
        || is_block_html_start(line)
        || is_footnote_definition_start(line)
        || is_reference_definition_start(line)
        || strip_indented_code_prefix(line).is_some()
        || parse_list_marker(line).is_some()
        || is_quote_start(line)
        || BlockKind::parse_atx_heading_line(line).is_some()
        || BlockKind::parse_separator_line(line)
        || lines
            .get(index + 1)
            .and_then(|next| BlockKind::parse_setext_underline(next))
            .is_some()
        || is_root_table_candidate_line(line)
        || is_display_math_start(line)
}

pub(crate) fn attach_child_blocks(
    parent: &Entity<crate::editor::Block>,
    children: Vec<Entity<crate::editor::Block>>,
    cx: &mut Context<Editor>,
) {
    if children.is_empty() {
        return;
    }

    parent.update(cx, move |parent, _cx| {
        parent.children.extend(children);
    });
}

pub(crate) fn build_code_block(
    cx: &mut Context<Editor>,
    language: Option<SharedString>,
    content: String,
) -> Entity<crate::editor::Block> {
    Editor::new_block(
        cx,
        BlockRecord::new(
            BlockKind::CodeBlock { language },
            InlineTextTree::plain(content),
        ),
    )
}

pub(crate) fn collect_fenced_code_block(
    cx: &mut Context<Editor>,
    lines: &[String],
    start: usize,
    origins: &[usize],
) -> Option<(Entity<crate::editor::Block>, usize)> {
    let fence = parse_opening_fence(&lines[start])?;
    let closing_index = find_matching_closing_fence(lines, start, &fence)?;
    if is_mermaid_info_string(fence.language.as_ref().map(|language| language.as_ref())) {
        let raw = lines[start..=closing_index].join("\n");
        return Some((
            Editor::new_block(cx, BlockRecord::mermaid(raw)),
            closing_index + 1,
        ));
    }

    // Length is known: closing_index - (start + 1). slice.to_vec()
    // allocates the exact capacity in one shot, vs Vec::new() + while-push
    // which doubles the buffer 2-3 times for any non-trivial code block.
    let code_lines = lines[start + 1..closing_index].to_vec();
    let block = build_code_block(cx, fence.language.clone(), code_lines.join("\n"));
    // 每一行让开几字节 = 上级容器吃掉的（`origins`）+ 本行没剥的东西（内容行就是
    // 切片里那一行，原样进模型）；开闭两行的缩进是 `strip_fence_indent` 当场知道的。
    let prefixes: Vec<u32> = (start + 1..closing_index)
        .map(|at| (origins.get(at).copied().unwrap_or(0)) as u32)
        .collect();
    let open_indent = lines[start].len() - strip_fence_indent(&lines[start]).unwrap_or("").len();
    let close_indent =
        lines[closing_index].len() - strip_fence_indent(&lines[closing_index]).unwrap_or("").len();
    block.update(cx, |block, _cx| {
        block.record.source_line_prefixes = prefixes;
        block.record.source_fence_lines = Some((open_indent as u32, close_indent as u32));
    });

    Some((block, closing_index + 1))
}

pub(crate) fn collect_indented_code_block(
    cx: &mut Context<Editor>,
    lines: &[String],
    start: usize,
    origins: &[usize],
) -> Option<(Entity<crate::editor::Block>, usize)> {
    let stripped = strip_indented_code_prefix(&lines[start])?;
    // 每一行被缩进记号吃掉几字节：本行剥掉的（`strip_indented_code_prefix` 当场知道）
    // 加上上级容器已经剥掉的（`origins`）。空行整行都是记号。
    let mut prefixes = vec![(origins.get(start).copied().unwrap_or(0)
        + (lines[start].len() - stripped.len())) as u32];
    let mut code_lines = vec![stripped.to_string()];
    let mut code_index = start + 1;
    while code_index < lines.len() {
        let inherited = origins.get(code_index).copied().unwrap_or(0);
        if let Some(stripped) = strip_indented_code_prefix(&lines[code_index]) {
            prefixes.push((inherited + (lines[code_index].len() - stripped.len())) as u32);
            code_lines.push(stripped.to_string());
            code_index += 1;
        } else if lines[code_index].trim().is_empty() {
            prefixes.push((inherited + lines[code_index].len()) as u32);
            code_lines.push(String::new());
            code_index += 1;
        } else {
            break;
        }
    }

    let block = build_code_block(cx, None, code_lines.join("\n"));
    block.update(cx, |block, _cx| {
        block.record.source_line_prefixes = prefixes;
    });
    Some((block, code_index))
}

pub(crate) fn raw_block(cx: &mut Context<Editor>, markdown: String) -> Entity<crate::editor::Block> {
    Editor::new_block(cx, BlockRecord::raw_markdown(markdown))
}

pub(crate) fn comment_block(cx: &mut Context<Editor>, markdown: String) -> Entity<crate::editor::Block> {
    Editor::new_block(cx, BlockRecord::comment(markdown))
}

pub(crate) fn html_or_raw_block(cx: &mut Context<Editor>, markdown: String) -> Entity<crate::editor::Block> {
    let document = parse_html_document(&markdown);
    // A stray closing tag parses to an empty document. It stays a block so the
    // source round-trips byte for byte, and `Block::renders_nothing` keeps it
    // out of the rendered view.
    if document.safety == HtmlSafetyClass::RawTextBlock {
        raw_block(cx, markdown)
    } else {
        let mut record = BlockRecord::html(markdown);
        record.html = Some(document);
        Editor::new_block(cx, record)
    }
}

pub(crate) fn math_or_raw_block(cx: &mut Context<Editor>, markdown: String) -> Entity<crate::editor::Block> {
    if parse_display_math_source(&markdown).is_some() {
        Editor::new_block(cx, BlockRecord::math(markdown))
    } else {
        raw_block(cx, markdown)
    }
}

pub(crate) fn collect_comment_block(
    cx: &mut Context<Editor>,
    lines: &[String],
    start: usize,
) -> Option<(Entity<crate::editor::Block>, usize)> {
    let end = collect_closed_html_comment_region(lines, start)?;
    Some((comment_block(cx, lines[start..end].join("\n")), end))
}

pub(crate) fn native_block(
    cx: &mut Context<Editor>,
    kind: BlockKind,
    markdown: String,
) -> Entity<crate::editor::Block> {
    let record = BlockRecord::new(kind, InlineTextTree::from_markdown(&markdown));
    Editor::new_block(cx, record)
}

pub(crate) fn standalone_image_block(cx: &mut Context<Editor>, markdown: String) -> Entity<crate::editor::Block> {
    Editor::new_block(cx, BlockRecord::paragraph(markdown.trim().to_string()))
}

pub(crate) fn is_standalone_image_paragraph(lines: &[String]) -> bool {
    lines.len() == 1 && parse_standalone_image(&lines[0]).is_some()
}

pub(crate) fn starts_with_standalone_image_child_paragraph(lines: &[String]) -> bool {
    if lines.is_empty() || !is_standalone_image_paragraph(&lines[..1]) {
        return false;
    }

    lines.get(1).is_none_or(|next| {
        next.trim().is_empty()
            || parse_list_marker(next).is_some()
            || is_quote_start(next)
            || parse_opening_fence(next).is_some()
            || strip_indented_code_prefix(next).is_some()
            || is_block_html_start(next)
            || is_footnote_definition_start(next)
            || is_reference_definition_start(next)
            || is_root_table_candidate_line(next)
            || is_display_math_start(next)
    })
}

pub(crate) fn append_markdown_to_block(
    block: &Entity<crate::editor::Block>,
    separator: &str,
    markdown: &str,
    cx: &mut Context<Editor>,
) {
    block.update(cx, |block, _cx| {
        let mut title = block.record.title.clone();
        if !separator.is_empty() {
            title.append_tree(InlineTextTree::plain(separator.to_string()));
        }
        title.append_tree(InlineTextTree::from_markdown(markdown));
        block.record.set_title(title);
        block.sync_edit_mode_from_kind();
        block.sync_render_cache();
    });
}

pub(crate) fn plain_text_paragraph_block(cx: &mut Context<Editor>, text: String) -> Entity<crate::editor::Block> {
    Editor::new_block(cx, BlockRecord::paragraph(text))
}

pub(crate) fn append_quote_separator_children(
    children: &mut Vec<Entity<crate::editor::Block>>,
    count: usize,
    cx: &mut Context<Editor>,
) {
    for _ in 0..count {
        children.push(native_block(cx, BlockKind::Paragraph, String::new()));
    }
}

pub(crate) fn build_native_footnote_definition_block(
    cx: &mut Context<Editor>,
    lines: &[String],
) -> Option<Entity<crate::editor::Block>> {
    let (id, first_line) = parse_footnote_definition_head(lines.first()?)?;
    let mut body_lines = Vec::new();
    if !first_line.is_empty() {
        body_lines.push(first_line);
    }

    for line in lines.iter().skip(1) {
        if line.trim().is_empty() {
            body_lines.push(String::new());
        } else {
            body_lines.push(
                strip_leading_columns(line, 4)
                    .unwrap_or(line.as_str())
                    .to_string(),
            );
        }
    }

    let children =
        Editor::build_blocks_from_lines_internal(cx, &body_lines, false, ChunkCursor::WHOLE_DOCUMENT)
            .0;
    let block = Editor::new_block(
        cx,
        BlockRecord::new(BlockKind::FootnoteDefinition, InlineTextTree::plain(id)),
    );
    attach_child_blocks(&block, children, cx);
    Some(block)
}

