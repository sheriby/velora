//! Reference-style links and autolink helpers.

use std::collections::HashMap;

use super::image::normalize_reference_label;

pub(crate) const EXTERNAL_LINK_SCHEMES: [&str; 5] = ["http:", "https:", "mailto:", "tel:", "ftp:"];

pub(crate) fn is_safe_export_url(value: &str) -> bool {
    let normalized = value.chars().filter(|character| !character.is_whitespace() && *character != '\0')
        .collect::<String>().to_ascii_lowercase();
    let Some((scheme, _)) = normalized.split_once(':') else {
        return true;
    };
    let has_scheme = scheme.chars().next().is_some_and(|character| character.is_ascii_alphabetic())
        && scheme.chars().all(|character| character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '.'));
    !has_scheme || EXTERNAL_LINK_SCHEMES.iter().any(|allowed| normalized.starts_with(allowed))
}

/// Active fenced code block while scanning for link reference definitions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FenceInfo {
    ch: char,
    len: usize,
}

/// HTML block start that suppresses reference-definition scanning.
enum HtmlBlockStart {
    /// HTML comment beginning with `<!--`.
    Comment,
    /// HTML tag block whose closing behavior depends on the tag.
    Tag {
        name: String,
        self_closing: bool,
        closes_same_line: bool,
    },
}

/// Global reference definition for reference-style links.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LinkReferenceDefinition {
    pub(crate) destination: String,
    #[allow(dead_code)]
    pub(crate) title: Option<String>,
}

pub(crate) type LinkReferenceDefinitions = HashMap<String, LinkReferenceDefinition>;

pub(crate) fn parse_link_reference_definitions(markdown: &str) -> LinkReferenceDefinitions {
    let lines = markdown.split('\n').collect::<Vec<_>>();
    let normalized_lines = lines
        .iter()
        .map(|line| strip_reference_scan_container_prefixes(line).to_string())
        .collect::<Vec<_>>();
    let normalized_refs = normalized_lines
        .iter()
        .map(|line| line.as_str())
        .collect::<Vec<_>>();
    let mut definitions = LinkReferenceDefinitions::new();
    let mut index = 0usize;
    let mut active_fence = None;
    let mut active_html_tag: Option<String> = None;
    let mut active_html_comment = false;

    while index < lines.len() {
        let line = normalized_refs[index];

        if let Some(fence) = active_fence {
            if is_reference_scan_closing_fence(line, fence) {
                active_fence = None;
            }
            index += 1;
            continue;
        }

        if active_html_comment {
            if line.contains("-->") || line.trim().is_empty() {
                active_html_comment = false;
            }
            index += 1;
            continue;
        }

        if let Some(tag_name) = active_html_tag.clone() {
            if line.trim().is_empty()
                || parse_reference_scan_html_close_tag_name(line).as_deref() == Some(&tag_name)
            {
                active_html_tag = None;
            }
            index += 1;
            continue;
        }

        if let Some(fence) = parse_reference_scan_opening_fence(line) {
            if !is_reference_scan_closing_fence(line, fence) {
                active_fence = Some(fence);
            }
            index += 1;
            continue;
        }

        if let Some(html_start) = parse_reference_scan_html_block_start(line) {
            match html_start {
                HtmlBlockStart::Comment => {
                    if !line.contains("-->") {
                        active_html_comment = true;
                    }
                }
                HtmlBlockStart::Tag {
                    name,
                    self_closing,
                    closes_same_line,
                } => {
                    if !self_closing && !closes_same_line {
                        active_html_tag = Some(name);
                    }
                }
            }
            index += 1;
            continue;
        }

        let Some((label, definition, consumed)) =
            parse_link_reference_definition(&normalized_refs, index)
        else {
            index += 1;
            continue;
        };

        definitions.entry(label).or_insert(definition);
        index += consumed;
    }

    definitions
}

fn parse_link_reference_definition(
    lines: &[&str],
    start: usize,
) -> Option<(String, LinkReferenceDefinition, usize)> {
    let line = lines.get(start)?;
    let trimmed_end = line.trim_end();
    let leading_spaces = trimmed_end.bytes().take_while(|b| *b == b' ').count();
    if leading_spaces > 3 {
        return None;
    }

    let rest = &trimmed_end[leading_spaces..];
    if !rest.starts_with('[') {
        return None;
    }

    let label_end = find_unescaped_char(rest, 1, b']')?;
    if rest.as_bytes().get(label_end + 1) != Some(&b':') {
        return None;
    }

    let label = normalize_reference_label(&rest[1..label_end])?;
    let mut target = rest[label_end + 2..].trim_start().to_string();
    let mut consumed = 1usize;

    if target.is_empty()
        && let Some(next_line) = lines.get(start + 1)
        && valid_reference_target_line(next_line)
    {
        target = next_line.trim().to_string();
        consumed += 1;
    }

    if !target.is_empty()
        && let Some(next_line) = lines.get(start + consumed)
        && is_reference_definition_title_continuation(next_line)
    {
        target.push(' ');
        target.push_str(next_line.trim());
        consumed += 1;
    }

    let (destination, title) = parse_link_target(&target)?;
    Some((
        label,
        LinkReferenceDefinition { destination, title },
        consumed,
    ))
}

fn strip_reference_scan_container_prefixes(mut line: &str) -> &str {
    loop {
        let original = line;
        if let Some(rest) = strip_reference_scan_quote_prefix(line) {
            line = rest;
            continue;
        }
        if let Some(rest) = strip_reference_scan_list_marker(line) {
            line = rest;
            continue;
        }
        if line == original {
            return line;
        }
    }
}

fn strip_reference_scan_quote_prefix(line: &str) -> Option<&str> {
    let leading_spaces = line.bytes().take_while(|b| *b == b' ').count();
    if leading_spaces > 3 {
        return None;
    }

    let rest = &line[leading_spaces..];
    if !rest.starts_with('>') {
        return None;
    }

    Some(rest[1..].strip_prefix(' ').unwrap_or(&rest[1..]))
}

fn strip_reference_scan_list_marker(line: &str) -> Option<&str> {
    let indent_bytes = line
        .chars()
        .take_while(|ch| matches!(ch, ' ' | '\t'))
        .map(char::len_utf8)
        .sum::<usize>();
    let rest = &line[indent_bytes..];

    if let Some(marker) = rest.chars().next()
        && matches!(marker, '-' | '*' | '+')
    {
        let after_marker = &rest[marker.len_utf8()..];
        return after_marker
            .strip_prefix(' ')
            .or_else(|| after_marker.strip_prefix('\t'));
    }

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

    Some(&rest[digit_len + 2..])
}

fn parse_reference_scan_opening_fence(line: &str) -> Option<FenceInfo> {
    let trimmed = line.trim_end();
    let ch = trimmed.chars().next()?;
    if !matches!(ch, '`' | '~') {
        return None;
    }
    let len = trimmed.chars().take_while(|current| *current == ch).count();
    let rest = &trimmed[ch.len_utf8() * len..];
    if ch == '`' && rest.contains('`') {
        return None;
    }
    (len >= 3).then_some(FenceInfo { ch, len })
}

fn is_reference_scan_closing_fence(line: &str, opener: FenceInfo) -> bool {
    let trimmed = line.trim_end();
    if !trimmed.starts_with(opener.ch) {
        return false;
    }

    let run_len = trimmed
        .chars()
        .take_while(|current| *current == opener.ch)
        .count();
    run_len == opener.len && trimmed[opener.ch.len_utf8() * run_len..].trim().is_empty()
}

fn parse_reference_scan_html_block_start(line: &str) -> Option<HtmlBlockStart> {
    let rest = line.trim_start().trim_end();
    if rest.starts_with("<!--") {
        return Some(HtmlBlockStart::Comment);
    }

    let tagged = rest.strip_prefix('<')?;
    if tagged.starts_with('/') {
        return None;
    }

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

    Some(HtmlBlockStart::Tag {
        name: name.to_string(),
        self_closing: rest.ends_with("/>"),
        closes_same_line: rest.contains(&format!("</{name}>")),
    })
}

/// 定义标签之后单独占一行的目的地是否合法：裸目的地不许含空白，尖括号
/// 目的地允许空格；目的地后只接受双引号标题。认不出就当普通文本，绝不把
/// 一整行正文并进定义。
pub(crate) fn valid_reference_target_line(line: &str) -> bool {
    let text = line.trim();
    if text.is_empty() {
        return false;
    }
    if text.starts_with('<') {
        let Some(close) = text
            .char_indices()
            .skip(1)
            .find(|(index, ch)| *ch == '>' && !super::image::is_escaped(text, *index))
            .map(|(index, _)| index)
        else {
            return false;
        };
        let inner = &text[1..close];
        if inner
            .char_indices()
            .any(|(offset, ch)| ch == '<' && !super::image::is_escaped(inner, offset))
        {
            return false;
        }
        let rest = text[close + 1..].trim();
        return rest.is_empty() || is_double_quoted_title(rest);
    }
    let destination_end = (0..text.len())
        .find(|index| {
            text.as_bytes()[*index].is_ascii_whitespace() && !super::image::is_escaped(text, *index)
        })
        .unwrap_or(text.len());
    if destination_end == 0 {
        return false;
    }
    let destination = &text[..destination_end];
    if destination
        .char_indices()
        .any(|(offset, ch)| ch == '<' && !super::image::is_escaped(destination, offset))
    {
        return false;
    }
    let rest = text[destination_end..].trim();
    rest.is_empty() || is_double_quoted_title(rest)
}

fn is_double_quoted_title(text: &str) -> bool {
    text.len() >= 2
        && text.starts_with('"')
        && text.ends_with('"')
        && !super::image::is_escaped(text, text.len() - 1)
}

fn parse_reference_scan_html_close_tag_name(line: &str) -> Option<String> {
    let rest = line.trim_start().trim_end();
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

pub(crate) fn parse_link_target(inner: &str) -> Option<(String, Option<String>)> {
    if inner.is_empty() {
        return None;
    }

    if let Some((destination, title)) = split_target_title(inner) {
        // title 要做 CommonMark 反斜杠反转义（判据单点
        // unescape_ascii_punctuation）：存储值是「解析后的语义」，写回时才
        // 加转义。只做字面截取的话「解析 ∘ 序列化」不是对合——源码
        // `ti\"tle` 存成 `ti\"tle`，写回转义成 `ti\\"tle`，每保存一次多
        // 一层反斜杠，永不收敛（destination 侧的
        // normalize_link_destination 早有同一步，title 漏了）。
        let title = unescape_ascii_punctuation(title);
        return Some((normalize_markdown_destination(destination)?, Some(title)));
    }

    Some((normalize_markdown_destination(inner)?, None))
}

/// 把 `目标 标题` 拆成两段：标题的定界符可以是 `"`、`'` 或圆括号，
/// 开定界符前必须有空白才认——`/url(a)` 的圆括号属于目的地。
/// 圆括号标题按配平扫描，标题内部成对的括号不会截断。
pub(crate) fn split_target_title(inner: &str) -> Option<(&str, &str)> {
    let close = inner.chars().last()?;
    let open = match close {
        '"' => '"',
        '\'' => '\'',
        ')' => '(',
        _ => return None,
    };
    let close_index = inner.len() - close.len_utf8();
    if super::image::is_escaped(inner, close_index) {
        return None;
    }
    let open_index = find_open_title_delimiter(inner, close_index, open)?;
    let destination = inner[..open_index].trim_end();
    if destination.is_empty() {
        return None;
    }
    Some((destination, &inner[open_index + 1..close_index]))
}

fn find_open_title_delimiter(input: &str, close_index: usize, open: char) -> Option<usize> {
    let bytes = input.as_bytes();
    if open == '(' {
        let mut depth = 0usize;
        for index in (0..close_index).rev() {
            if super::image::is_escaped(input, index) {
                continue;
            }
            match bytes[index] {
                b')' => depth += 1,
                b'(' => {
                    if depth == 0 {
                        return (index > 0 && bytes[index - 1].is_ascii_whitespace())
                            .then_some(index);
                    }
                    depth -= 1;
                }
                _ => {}
            }
        }
        return None;
    }
    (0..close_index).rev().find(|&index| {
        bytes[index] == open as u8
            && !super::image::is_escaped(input, index)
            && index > 0
            && bytes[index - 1].is_ascii_whitespace()
    })
}

/// 目的地归一（链接与图片共用）：`<…>` 内部允许空格，只不许出现未转义的
/// `<`、`>` 与换行；`<>` 是合法的空目标。尖括号没闭合或内部非法时整条
/// 目标不成立，不再把带尖括号的字面串当成文件名。
pub(crate) fn normalize_markdown_destination(destination: &str) -> Option<String> {
    let Some(inner) = destination.strip_prefix('<') else {
        return Some(unescape_ascii_punctuation(destination));
    };
    let inner = inner.strip_suffix('>')?;
    if inner.contains(['\n', '\r']) {
        return None;
    }
    if inner.char_indices().any(|(offset, ch)| {
        matches!(ch, '<' | '>') && !super::image::is_escaped(inner, offset)
    }) {
        return None;
    }
    Some(unescape_ascii_punctuation(inner))
}

/// 可转义集只认 `is_commonmark_escapable` 这一处判据（行内目标的换行符
/// 不可达，与 `is_ascii_punctuation` 等价，但判据必须单点）。
pub(crate) fn unescape_ascii_punctuation(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\'
            && chars
                .peek()
                .is_some_and(|next| crate::components::markdown::inline::is_commonmark_escapable(*next))
        {
            output.push(chars.next().expect("peeked punctuation must exist"));
        } else {
            output.push(ch);
        }
    }
    output
}

fn is_reference_definition_title_continuation(line: &str) -> bool {
    let indent_bytes = line
        .chars()
        .take_while(|ch| matches!(ch, ' ' | '\t'))
        .map(char::len_utf8)
        .sum::<usize>();
    if indent_bytes == 0 {
        return false;
    }

    let trimmed = line[indent_bytes..].trim();
    (trimmed.starts_with('"') && trimmed.ends_with('"'))
        || (trimmed.starts_with('\'') && trimmed.ends_with('\''))
        || (trimmed.starts_with('(') && trimmed.ends_with(')'))
}

fn find_unescaped_char(input: &str, start: usize, target: u8) -> Option<usize> {
    let bytes = input.as_bytes();
    (start..bytes.len()).find(|&index| bytes[index] == target && !super::image::is_escaped(input, index))
}


#[cfg(test)]
mod tests {
    use super::{
        LinkReferenceDefinition, parse_link_reference_definitions, parse_link_target,
    };

    /// 尖括号目标里的空格是合法的：`[a](<报告 2026.pdf>)` 的目标就是
    /// `报告 2026.pdf`。图片那侧同一个坑已经修过（报修第 12 条），链接这侧
    /// 还拿「是不是受支持的目标」当剥尖括号的门槛，于是带空格或被门外的
    /// scheme 挡住时，尖括号被留在目标里，链接既点不动也存不回原样。
    #[test]
    fn angle_bracket_destination_strips_brackets_even_with_a_space() {
        assert_eq!(
            parse_link_target("<报告 2026.pdf>"),
            Some(("报告 2026.pdf".to_string(), None))
        );
        assert_eq!(
            parse_link_target("<https://example.com/a b>"),
            Some(("https://example.com/a b".to_string(), None))
        );
        // 带标题那一支同样要先剥尖括号再解转义。
        assert_eq!(
            parse_link_target("<assets/a b.png> \"t\""),
            Some(("assets/a b.png".to_string(), Some("t".to_string())))
        );
        // 未转义的 `<`/`>` 与换行仍然不算尖括号目标：那本来就是没写完的写法。
        // 内含未转义 `<` 的尖括号形式不是合法目标：整段链接不成立。
        assert_eq!(parse_link_target("<a<b>"), None);
        // `<>` 是合法的空目标，不是字面文件名。
        assert_eq!(parse_link_target("<>"), Some((String::new(), None)));
    }

    #[test]
    fn parses_link_reference_definitions_with_title_and_first_wins() {
        let definitions = parse_link_reference_definitions(
            "[Ref Link]: https://first.example \"Caption\"\n[ref link]: https://second.example",
        );
        assert_eq!(
            definitions.get("ref link"),
            Some(&LinkReferenceDefinition {
                destination: "https://first.example".to_string(),
                title: Some("Caption".to_string()),
            })
        );
    }

    #[test]
    fn parses_single_quoted_and_parenthesized_titles() {
        assert_eq!(
            parse_link_target("/url '标题'"),
            Some(("/url".to_string(), Some("标题".to_string())))
        );
        assert_eq!(
            parse_link_target("/url (标题)"),
            Some(("/url".to_string(), Some("标题".to_string())))
        );
        // 圆括号标题按配平扫描：标题内部成对的括号不截断。
        assert_eq!(
            parse_link_target("/url (a (b))"),
            Some(("/url".to_string(), Some("a (b)".to_string())))
        );
        // 没有空白分隔的圆括号是目的地的一部分，不是标题。
        assert_eq!(
            parse_link_target("/url(带括号)"),
            Some(("/url(带括号)".to_string(), None))
        );
    }

    #[test]
    fn parses_destination_written_on_the_next_line() {
        let definitions = parse_link_reference_definitions(
            "[ref]:\nhttps://next.example \"Caption\"\n[angle]:\n<assets/a b.png>",
        );
        assert_eq!(
            definitions.get("ref"),
            Some(&LinkReferenceDefinition {
                destination: "https://next.example".to_string(),
                title: Some("Caption".to_string()),
            })
        );
        assert_eq!(
            definitions.get("angle"),
            Some(&LinkReferenceDefinition {
                destination: "assets/a b.png".to_string(),
                title: None,
            })
        );
    }

    #[test]
    fn plain_text_after_a_label_is_not_a_destination() {
        let definitions = parse_link_reference_definitions("[bad]:\nnot a url");
        assert!(
            definitions.is_empty(),
            "不带空白约束的普通文本被当成了换行目的地"
        );
    }

    #[test]
    fn parses_container_scoped_link_reference_definitions_and_skips_raw_blocks() {
        let definitions = parse_link_reference_definitions(
            [
                "> [quoted ref]: https://quoted.example \"Quoted\"",
                "- [list ref]: https://list.example",
                "1) [ordered ref]: https://ordered.example",
                "> ```md",
                "> [code ref]: https://ignored-code.example",
                "> ```",
                "",
                "<div>",
                "[html ref]: https://ignored-html.example",
                "</div>",
            ]
            .join("\n")
            .as_str(),
        );

        assert_eq!(
            definitions.get("quoted ref"),
            Some(&LinkReferenceDefinition {
                destination: "https://quoted.example".to_string(),
                title: Some("Quoted".to_string()),
            })
        );
        assert_eq!(
            definitions.get("list ref"),
            Some(&LinkReferenceDefinition {
                destination: "https://list.example".to_string(),
                title: None,
            })
        );
        assert_eq!(
            definitions.get("ordered ref"),
            Some(&LinkReferenceDefinition {
                destination: "https://ordered.example".to_string(),
                title: None,
            })
        );
        assert!(!definitions.contains_key("code ref"));
        assert!(!definitions.contains_key("html ref"));
    }

}
