//! Reference-definition scan scaffolding shared by link and image entries.
//!
//! 链接定义与图片定义的扫描本就同构：容器前缀剥离、围栏开闭、HTML 块起止、
//! 标签/目标积累与整个主循环只有一份（评审 P139）。任何一侧的判据修复都
//! 落在这里，两个入口只保留各自的定义行终解析（结果类型不同）。

use std::collections::HashMap;

use super::image::{is_escaped, normalize_reference_label};

/// Active fenced code block while scanning for reference definitions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FenceInfo {
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

/// 引用定义扫描主循环：容器前缀剥离、围栏、HTML 块起止与逐行推进在链接与
/// 图片两个入口同构；只有「这一行算不算一条定义」由 `parse_definition`
/// 注入。同一份文档两条入口必然圈中同样的行区间。
pub(crate) fn scan_reference_definitions<Definition>(
    markdown: &str,
    parse_definition: impl Fn(&[&str], usize) -> Option<(String, Definition, usize)>,
) -> HashMap<String, Definition> {
    let lines = markdown.split('\n').collect::<Vec<_>>();
    let normalized_lines = lines
        .iter()
        .map(|line| strip_reference_scan_container_prefixes(line).to_string())
        .collect::<Vec<_>>();
    let normalized_refs = normalized_lines
        .iter()
        .map(|line| line.as_str())
        .collect::<Vec<_>>();
    let mut definitions = HashMap::new();
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
            // 围栏行只能由后续行闭合；这里若当它是闭合，紧随的内容会漏扫成定义。
            active_fence = Some(fence);
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

        let Some((label, definition, consumed)) = parse_definition(&normalized_refs, index) else {
            index += 1;
            continue;
        };

        definitions.entry(label).or_insert(definition);
        index += consumed;
    }

    definitions
}

/// 解析 `[label]: target` 定义行的标签与目标积累部分：换行写法的目标行与
/// 标题续行两条边在链接与图片两个入口同构；只有 target 的最终解析留给
/// 各自入口（结果类型不同）。
pub(crate) struct ReferenceDefinitionLine {
    pub(crate) label: String,
    pub(crate) target: String,
    pub(crate) consumed: usize,
}

pub(crate) fn parse_reference_definition_line(
    lines: &[&str],
    start: usize,
) -> Option<ReferenceDefinitionLine> {
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

    Some(ReferenceDefinitionLine {
        label,
        target,
        consumed,
    })
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
    let (indent_columns, indent_bytes) = reference_scan_indent(trimmed);
    if indent_columns > 3 {
        return None;
    }
    let trimmed = &trimmed[indent_bytes..];
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
    let (indent_columns, indent_bytes) = reference_scan_indent(trimmed);
    if indent_columns > 3 {
        return false;
    }
    let trimmed = &trimmed[indent_bytes..];
    if !trimmed.starts_with(opener.ch) {
        return false;
    }

    let run_len = trimmed
        .chars()
        .take_while(|current| *current == opener.ch)
        .count();
    run_len >= opener.len && trimmed[opener.ch.len_utf8() * run_len..].trim().is_empty()
}

/// 行首空白折算成列：空格 1 列、制表符补到 4 列的整数倍。引用扫描与真正的
/// 块解析共用「最多 3 列」这一条缩进口径。
fn reference_scan_indent(line: &str) -> (usize, usize) {
    let mut columns = 0usize;
    let mut bytes = 0usize;
    for ch in line.chars() {
        match ch {
            ' ' => columns += 1,
            '\t' => columns += 4 - (columns % 4),
            _ => break,
        }
        bytes += ch.len_utf8();
    }
    (columns, bytes)
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
            .find(|(index, ch)| *ch == '>' && !is_escaped(text, *index))
            .map(|(index, _)| index)
        else {
            return false;
        };
        let inner = &text[1..close];
        if inner
            .char_indices()
            .any(|(offset, ch)| ch == '<' && !is_escaped(inner, offset))
        {
            return false;
        }
        let rest = text[close + 1..].trim();
        return rest.is_empty() || is_double_quoted_title(rest);
    }
    let destination_end = (0..text.len())
        .find(|index| {
            text.as_bytes()[*index].is_ascii_whitespace() && !is_escaped(text, *index)
        })
        .unwrap_or(text.len());
    if destination_end == 0 {
        return false;
    }
    let destination = &text[..destination_end];
    if destination
        .char_indices()
        .any(|(offset, ch)| ch == '<' && !is_escaped(destination, offset))
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
        && !is_escaped(text, text.len() - 1)
}

pub(crate) fn is_reference_definition_title_continuation(line: &str) -> bool {
    let (indent_columns, indent_bytes) = reference_scan_indent(line);
    if indent_columns == 0 || indent_columns > 3 {
        return false;
    }

    let trimmed = line[indent_bytes..].trim();
    (trimmed.starts_with('"') && trimmed.ends_with('"'))
        || (trimmed.starts_with('\'') && trimmed.ends_with('\''))
        || (trimmed.starts_with('(') && trimmed.ends_with(')'))
}

pub(crate) fn find_unescaped_char(input: &str, start: usize, target: u8) -> Option<usize> {
    let bytes = input.as_bytes();
    (start..bytes.len()).find(|&index| bytes[index] == target && !is_escaped(input, index))
}

#[cfg(test)]
mod tests {
    use super::scan_reference_definitions;

    /// 扫描脚手架判据单点（评审 P139）：链接与图片两个入口不许再各自抄
    /// 围栏/HTML 块/容器前缀/未转义查找的扫描逻辑——抄本漂移会让同一篇
    /// 文档的链接表与图片表对围栏或缩进的判定不一致。
    #[test]
    fn reference_scan_scaffolding_is_a_single_shared_criterion() {
        let link_source = include_str!("link.rs");
        let image_source = include_str!("image.rs");
        for duplicated in [
            concat!("fn strip_reference_scan_quote_prefix"),
            concat!("fn strip_reference_scan_list_marker"),
            concat!("fn parse_reference_scan_html_block_start"),
            concat!("fn parse_reference_scan_html_close_tag_name"),
            concat!("fn parse_reference_scan_opening_fence"),
            concat!("fn is_reference_scan_closing_fence"),
            concat!("fn find_unescaped_char"),
        ] {
            assert!(
                !link_source.contains(duplicated) && !image_source.contains(duplicated),
                "扫描脚手架不许在入口文件留抄本：{duplicated}"
            );
        }
        for entry_point in [link_source, image_source] {
            assert!(
                entry_point.contains("scan_reference_definitions("),
                "两个入口都必须走共享扫描主循环"
            );
        }
    }

    /// 两条入口对同一篇文档必须圈中同样的行：围栏里、HTML 注释里的伪定义
    /// 都不认，引语/列表前缀下的定义都认。
    #[test]
    fn link_and_image_scans_agree_on_which_lines_hold_definitions() {
        let markdown = [
            "```md",
            "[fenced]: /ignored",
            "```",
            "",
            "<!-- [commented]: /also-ignored -->",
            "",
            "> [quoted]: /quoted-url",
            "- [listed]: /listed-url",
            "[real]: /real-url",
        ]
        .join("\n");
        let links = super::super::link::parse_link_reference_definitions(&markdown);
        let images = super::super::image::parse_image_reference_definitions(&markdown);
        let mut link_keys = links.keys().cloned().collect::<Vec<_>>();
        let mut image_keys = images.keys().cloned().collect::<Vec<_>>();
        link_keys.sort();
        image_keys.sort();
        assert_eq!(link_keys, image_keys, "两条入口必须圈中同样的行");
        assert!(!links.contains_key("fenced"), "围栏里不许认定义");
        assert!(!links.contains_key("commented"), "注释里不许认定义");
        for label in ["quoted", "listed", "real"] {
            assert!(links.contains_key(label), "容器前缀下的定义要认出：{label}");
        }
    }

    /// 泛型主循环本身可脱离入口直接驱动（注入一个只认首行的假解析器）：
    /// 围栏与 HTML 块的推进语义由这一份实现保证。
    #[test]
    fn scan_loop_drives_injected_definition_parser() {
        let lines_held = |markdown: &str| {
            scan_reference_definitions::<()>(markdown, |lines, start| {
                let line = lines.get(start)?;
                let label = line
                    .strip_suffix(": def")?
                    .trim()
                    .trim_start_matches('[')
                    .trim_end_matches(']')
                    .to_string();
                Some((label, (), 1))
            })
        };
        let held = lines_held("[a]: def\n```\n[b]: def\n```\n[c]: def");
        assert!(held.contains_key("a") && held.contains_key("c"));
        assert!(!held.contains_key("b"), "围栏内容不许交给注入的解析器");
    }
}
