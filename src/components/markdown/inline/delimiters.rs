use super::*;


pub(crate) fn match_open_delimiter(tokens: &[CharToken], index: usize) -> Option<Delimiter> {
    if matches_sequence(tokens, index, "<strong>") {
        Some(Delimiter::BoldHtml)
    } else if matches_sequence(tokens, index, "<em>") {
        Some(Delimiter::ItalicHtml)
    } else if matches_sequence(tokens, index, "<u>") {
        Some(Delimiter::Underline)
    } else if matches_sequence(tokens, index, "~~") {
        Some(Delimiter::StrikethroughMarkdown)
    } else if matches_sequence(tokens, index, "==") {
        Some(Delimiter::HighlightMarkdown)
    } else if matches_sequence(tokens, index, "^") && can_open_script(tokens, index, '^') {
        Some(Delimiter::SuperscriptMarkdown)
    } else if is_single_tilde_delimiter(tokens, index) && can_open_script(tokens, index, '~') {
        Some(Delimiter::SubscriptMarkdown)
    } else if matches_sequence(tokens, index, "**") && can_open_emphasis(tokens, index, 2) {
        Some(Delimiter::BoldMarkdown { marker: '*' })
    } else if matches_sequence(tokens, index, "__") && can_open_emphasis(tokens, index, 2) {
        Some(Delimiter::BoldMarkdown { marker: '_' })
    } else if matches_sequence(tokens, index, "*") && can_open_emphasis(tokens, index, 1) {
        Some(Delimiter::ItalicMarkdown { marker: '*' })
    } else if matches_sequence(tokens, index, "_") && can_open_emphasis(tokens, index, 1) {
        Some(Delimiter::ItalicMarkdown { marker: '_' })
    } else if tokens[index].ch == '`' {
        // Count the run of consecutive backticks.
        let run_len = backtick_run_len(tokens, index);
        // A backtick run is only a valid opener if it is NOT immediately
        // followed by another backtick (no double-counting).
        if run_len > 0 {
            Some(Delimiter::CodeMarkdown { run_len })
        } else {
            None
        }
    } else {
        None
    }
}

/// Returns the length of the consecutive backtick run starting at `index`.
pub(crate) fn backtick_run_len(tokens: &[CharToken], index: usize) -> usize {
    let mut len = 0;
    while index + len < tokens.len() && tokens[index + len].ch == '`' {
        len += 1;
    }
    // A backtick run is only valid if it's not immediately preceded by an
    // additional backtick (the run must start at `index`).
    if index > 0 && tokens[index - 1].ch == '`' {
        return 0;
    }
    len
}

pub(crate) fn has_closing_delimiter(tokens: &[CharToken], index: usize, delimiter: Delimiter) -> bool {
    let skip = delimiter.token_len();
    let close_str = delimiter.close();

    // For code spans we look for a matching-length backtick run;
    // for emphasis we just scan for the close string.
    if let Delimiter::CodeMarkdown { .. } = delimiter {
        let mut cursor = index + skip;
        while cursor < tokens.len() {
            if tokens[cursor].ch == '\\'
                && let Some(escaped_len) = escaped_sequence_token_len(tokens, cursor)
            {
                cursor += 1 + escaped_len;
                continue;
            }

            if tokens[cursor].ch == '`' && backtick_run_len(tokens, cursor) == skip {
                return true;
            }

            cursor += 1;
        }
        return false;
    }

    if matches!(
        delimiter,
        Delimiter::SuperscriptMarkdown | Delimiter::SubscriptMarkdown
    ) {
        let marker = match delimiter {
            Delimiter::SuperscriptMarkdown => '^',
            Delimiter::SubscriptMarkdown => '~',
            _ => unreachable!(),
        };
        return locate_script_close(tokens, index + skip, marker).is_some();
    }

    let body_start = index + skip;
    let requires_body = emphasis_requires_body(delimiter);
    let mut cursor = body_start;
    while cursor < tokens.len() {
        if tokens[cursor].ch == '\\'
            && let Some(escaped_len) = escaped_sequence_token_len(tokens, cursor)
        {
            cursor += 1 + escaped_len;
            continue;
        }

        if matches_sequence(tokens, cursor, &close_str) {
            // Emphasis spans must enclose at least one character; a close
            // sitting immediately after the open (e.g. `**` or `*` `*`) is an
            // empty span and is treated as literal text instead.
            if requires_body && cursor == body_start {
                cursor += 1;
                continue;
            }
            // 与 `parse_until` 用同一套闭合判定。否则这里报“有闭合”、正文扫描却拒
            // 绝全部候选（例如 `_a_b` 里的下划线不能闭合），正文就会被输出两次。
            if can_close_emphasis(tokens, cursor) {
                return true;
            }
            cursor += 1;
            continue;
        }

        cursor += 1;
    }

    false
}

/// Whether `delimiter` requires a non-empty body. Emphasis and strikethrough
/// markers must enclose at least one character; code spans may be empty and
/// script markers already constrain their bodies elsewhere.
pub(crate) fn emphasis_requires_body(delimiter: Delimiter) -> bool {
    matches!(
        delimiter,
        Delimiter::BoldMarkdown { .. }
            | Delimiter::ItalicMarkdown { .. }
            | Delimiter::StrikethroughMarkdown
            | Delimiter::HighlightMarkdown
            | Delimiter::BoldHtml
            | Delimiter::ItalicHtml
            | Delimiter::Underline
    )
}

pub(crate) fn locate_script_close(tokens: &[CharToken], mut cursor: usize, marker: char) -> Option<usize> {
    let body_start = cursor;
    while cursor < tokens.len() {
        if tokens[cursor].ch == '\\'
            && let Some(escaped_len) = escaped_sequence_token_len(tokens, cursor)
        {
            cursor += 1 + escaped_len;
            continue;
        }

        let is_close = if marker == '~' {
            is_single_tilde_delimiter(tokens, cursor)
        } else {
            tokens[cursor].ch == marker
        };
        if is_close {
            return valid_script_body(tokens, body_start, cursor).then_some(cursor);
        }

        cursor += 1;
    }

    None
}

pub(crate) fn valid_script_body(tokens: &[CharToken], start: usize, end: usize) -> bool {
    start < end
        && tokens[start..end]
            .iter()
            .all(|token| token.ch.is_ascii_alphanumeric())
}

pub(crate) fn is_single_tilde_delimiter(tokens: &[CharToken], index: usize) -> bool {
    tokens.get(index).is_some_and(|token| token.ch == '~')
        && index
            .checked_sub(1)
            .and_then(|prev| tokens.get(prev))
            .is_none_or(|token| token.ch != '~')
        && tokens.get(index + 1).is_none_or(|token| token.ch != '~')
}

pub(crate) fn matches_sequence(tokens: &[CharToken], index: usize, sequence: &str) -> bool {
    sequence
        .chars()
        .enumerate()
        .all(|(offset, ch)| tokens.get(index + offset).is_some_and(|t| t.ch == ch))
}

/// CommonMark 可转义集的**唯一判据**：全部 ASCII 标点
/// （``!"#$%&'()*+,-./:;<=>?@[\]^_`{|}~``），外加换行——`\` + 行尾是硬换行。
/// 解析（`escaped_sequence_token_len`）与序列化（`backslash_needs_escape`、表格单元格
/// 切分）都从这里取答案；曾各自维护一份窄白名单，才有「`\#` 显示多余反斜杠」「序列化
/// 与解析对同一处写法判断相反」这类报修。
pub(crate) const fn is_commonmark_escapable(ch: char) -> bool {
    ch.is_ascii_punctuation() || matches!(ch, '\n' | '\r')
}

pub(crate) fn escaped_sequence_token_len(tokens: &[CharToken], index: usize) -> Option<usize> {
    let next_index = index + 1;
    if next_index >= tokens.len() {
        return None;
    }
    // 编辑可见文本时反斜杠是字面字符：两个反斜杠不是"转义的反斜杠"，`\*` 也不吃掉星号。
    if tokens[index].literal_char {
        return None;
    }

    if matches_sequence(tokens, next_index, "</strong>") {
        Some(9)
    } else if matches_sequence(tokens, next_index, "<strong>") {
        Some(8)
    } else if matches_sequence(tokens, next_index, "</em>") {
        Some(5)
    } else if matches_sequence(tokens, next_index, "<em>") {
        Some(4)
    } else if matches_sequence(tokens, next_index, "</u>") {
        Some(4)
    } else if matches_sequence(tokens, next_index, "<u>") {
        Some(3)
    } else if is_commonmark_escapable(tokens[next_index].ch) {
        Some(1)
    } else {
        None
    }
}

/// 序列化只在 `escaped` 标出的位置写反斜杠——那些是**源码本就带着**的转义
/// （可见文本里「源码用反斜杠换来的字面记号」的字节偏移，升序）。
///
/// 缓冲区是事实源：未编辑块的序列化必须逐字节还原用户写法，裸的 `*`/`~`/`_`
/// 不许被洗成 `\*`/`\~`/`\_`——多出的反斜杠会让「块 markdown ↔ 缓冲区字节」
/// 的对齐整体漂移（搜索高亮、选区落点全部错位，用户报修）。重新解析同一段
/// 原文得到的仍是同一棵树：当时的定界符读法就是原文的读法。用户新敲的字面
/// 记号同理保持原样（仍是语法候选，补齐配对才成强调）。
/// 裸反斜杠写回时是否要转义成 `\\`：只有当后面跟着的字符会开启一次转义
/// （重新解析会把这个反斜杠吃掉，用户的 `\\` 缩成 `\`、`\*` 丢星号）才需要。
/// 判据与 `escaped_sequence_token_len` 同源（`is_commonmark_escapable`），两端不许各写一份。
/// `C:\Users`、`a\b` 这类后面跟普通字符的反斜杠保持原样，字节不动。
/// 片段末尾的反斜杠看不见下一个片段的首字符（链接的 `[`、别的片段的 `*`），
/// 保守起见转义。
fn backslash_needs_escape(text: &str, index: usize) -> bool {
    match text[index + 1..].chars().next() {
        None => true,
        Some(next) => is_commonmark_escapable(next),
    }
}

pub(crate) fn escape_literal_text(text: &str, escaped: &[u32]) -> String {
    let mut output = String::with_capacity(text.len());
    let mut index = 0usize;
    let mut escaped_iter = escaped.iter().copied();
    let mut next_escaped = escaped_iter.next();
    while index < text.len() {
        let ch = text[index..].chars().next().unwrap();
        if next_escaped == Some(index as u32) {
            output.push('\\');
            next_escaped = escaped_iter.next();
        } else if ch == '\\' && backslash_needs_escape(text, index) {
            output.push('\\');
        }
        output.push(ch);
        index += ch.len_utf8();
    }
    output
}

/// 代码片段（行内 code）的空白填充规则，与映射版本一致。
pub(crate) fn escape_code_span_text(text: &str) -> String {
    let needs_padding = !text.is_empty()
        && !text.chars().all(|ch| ch == ' ')
        && (text.starts_with([' ', '`']) || text.ends_with([' ', '`']));
    if !needs_padding {
        return text.to_string();
    }
    let mut markdown = String::with_capacity(text.len() + 2);
    markdown.push(' ');
    markdown.push_str(text);
    markdown.push(' ');
    markdown
}

pub(crate) fn escape_literal_text_with_offset_map(
    text: &str,
    escaped: &[u32],
) -> InlineMarkdownOffsetMap {
    let mut markdown = String::with_capacity(text.len());
    let mut visible_to_markdown = vec![0; text.len() + 1];
    let mut markdown_to_visible = vec![0];
    let mut index = 0usize;
    let mut escaped_iter = escaped.iter().copied();
    let mut next_escaped = escaped_iter.next();

    while index < text.len() {
        visible_to_markdown[index] = markdown.len();
        let ch = text[index..].chars().next().unwrap();
        let start = markdown.len();
        if next_escaped == Some(index as u32) {
            markdown.push('\\');
            next_escaped = escaped_iter.next();
        } else if ch == '\\' && backslash_needs_escape(text, index) {
            markdown.push('\\');
        }
        markdown.push(ch);
        markdown_to_visible.resize(markdown.len() + 1, index);
        for local in start..markdown.len() {
            markdown_to_visible[local] = index;
        }
        index += ch.len_utf8();
    }
    visible_to_markdown[text.len()] = markdown.len();
    markdown_to_visible[markdown.len()] = text.len();

    InlineMarkdownOffsetMap {
        markdown,
        visible_to_markdown,
        markdown_to_visible,
    }
}

pub(crate) fn escape_code_span_text_with_offset_map(text: &str) -> InlineMarkdownOffsetMap {
    let needs_padding = !text.is_empty()
        && !text.chars().all(|ch| ch == ' ')
        && (text.starts_with([' ', '`']) || text.ends_with([' ', '`']));
    let leading_padding = usize::from(needs_padding);

    let mut markdown = String::new();
    if needs_padding {
        markdown.push(' ');
    }
    markdown.push_str(text);
    if needs_padding {
        markdown.push(' ');
    }

    let mut visible_to_markdown = vec![0; text.len() + 1];
    for (visible, markdown_offset) in visible_to_markdown.iter_mut().enumerate() {
        *markdown_offset = leading_padding + visible;
    }

    let content_start = leading_padding;
    let content_end = leading_padding + text.len();
    let mut markdown_to_visible = vec![0; markdown.len() + 1];
    for (markdown_offset, visible) in markdown_to_visible.iter_mut().enumerate() {
        *visible = if markdown_offset <= content_start {
            0
        } else if markdown_offset >= content_end {
            text.len()
        } else {
            markdown_offset - content_start
        };
    }

    InlineMarkdownOffsetMap {
        markdown,
        visible_to_markdown,
        markdown_to_visible,
    }
}

