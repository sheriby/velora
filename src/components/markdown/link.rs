//! Reference-style links and autolink helpers.

use std::collections::HashMap;

use super::image::is_escaped;
use super::references::{parse_reference_definition_line, scan_reference_definitions};

pub(crate) const EXTERNAL_LINK_SCHEMES: [&str; 5] = ["http:", "https:", "mailto:", "tel:", "ftp:"];

/// 图片源的安全判据：在外链白名单之外，额外放行 `data:image/` 的位图数据
/// URI；`data:image/svg+xml` 仍按不可信脚本容器拒绝（既有导出安全用例的
/// 口径），`data:text/html` 之类同样拒绝。
pub(crate) fn is_safe_image_url(value: &str) -> bool {
    let normalized = value
        .chars()
        .filter(|character| !character.is_whitespace() && *character != '\0')
        .collect::<String>()
        .to_ascii_lowercase();
    (normalized.starts_with("data:image/") && !normalized.starts_with("data:image/svg+xml"))
        || is_safe_export_url(value)
}

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

/// Global reference definition for reference-style links.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LinkReferenceDefinition {
    pub(crate) destination: String,
    #[allow(dead_code)]
    pub(crate) title: Option<String>,
}

pub(crate) type LinkReferenceDefinitions = HashMap<String, LinkReferenceDefinition>;

pub(crate) fn parse_link_reference_definitions(markdown: &str) -> LinkReferenceDefinitions {
    scan_reference_definitions(markdown, parse_link_reference_definition)
}

fn parse_link_reference_definition(
    lines: &[&str],
    start: usize,
) -> Option<(String, LinkReferenceDefinition, usize)> {
    let line = parse_reference_definition_line(lines, start)?;
    let (destination, title) = parse_link_target(&line.target)?;
    Some((
        line.label,
        LinkReferenceDefinition { destination, title },
        line.consumed,
    ))
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
    if is_escaped(inner, close_index) {
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
            if is_escaped(input, index) {
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
            && !is_escaped(input, index)
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
        matches!(ch, '<' | '>') && !is_escaped(inner, offset)
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
    fn reference_scan_recognizes_longer_and_indented_fences() {
        let definitions = parse_link_reference_definitions(
            "```md\n[inside]: /inside\n````\n\n[outside]: /outside\n\n   ```\n[also]: /also\n   ```\n\n[last]: /last",
        );
        assert!(!definitions.contains_key("inside"));
        assert!(!definitions.contains_key("also"));
        assert_eq!(
            definitions.get("outside").map(|definition| definition.destination.as_str()),
            Some("/outside")
        );
        assert_eq!(
            definitions.get("last").map(|definition| definition.destination.as_str()),
            Some("/last")
        );
    }

    #[test]
    fn title_continuation_stops_before_indented_code() {
        let definitions = parse_link_reference_definitions("[a]: /url\n    \"title\"\n[b]: /b");
        assert_eq!(
            definitions.get("a"),
            Some(&LinkReferenceDefinition {
                destination: "/url".to_string(),
                title: None,
            })
        );
        assert!(definitions.contains_key("b"));

        let indented_title = parse_link_reference_definitions("[a]: /url\n   \"title\"");
        assert_eq!(
            indented_title.get("a"),
            Some(&LinkReferenceDefinition {
                destination: "/url".to_string(),
                title: Some("title".to_string()),
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
