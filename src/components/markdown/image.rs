//! Standalone image syntax, references, and source resolution.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use gpui::SharedUri;

use crate::file_url::percent_decode_or_raw;
use crate::net;

/// Active fenced code block while scanning for image reference definitions.
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

/// Parsed standalone image expression.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ImageSyntax {
    pub(crate) alt: String,
    pub(crate) target: ImageTarget,
}

/// Inline image/text segment used only by native table-cell rendering.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TableCellInlineImageSegment {
    Text(String),
    Image {
        markdown: String,
        syntax: ImageSyntax,
    },
}

/// Image target form before reference resolution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ImageTarget {
    /// Direct image target from `![alt](src "title")`.
    Direct { src: String, title: Option<String> },
    /// Reference image target from `![alt][label]`.
    Reference { label: String },
}

/// Global reference definition for a reference-style image label.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ImageReferenceDefinition {
    pub(crate) src: String,
    pub(crate) title: Option<String>,
}

pub(crate) type ImageReferenceDefinitions = HashMap<String, ImageReferenceDefinition>;

/// Resolved image target ready for path or URL loading.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ResolvedImageTarget {
    pub(crate) src: String,
    pub(crate) title: Option<String>,
}

/// Concrete image source after local-path or remote-URL classification.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ImageResolvedSource {
    /// Filesystem path resolved relative to the current document, when possible.
    Local(PathBuf),
    /// HTTP(S) image URL handled by GPUI's HTTP client.
    Remote(SharedUri),
}

impl ImageSyntax {
    pub(crate) fn resolve_target(
        &self,
        reference_definitions: &ImageReferenceDefinitions,
    ) -> Option<ResolvedImageTarget> {
        match &self.target {
            ImageTarget::Direct { src, title } => Some(ResolvedImageTarget {
                src: src.clone(),
                title: title.clone(),
            }),
            ImageTarget::Reference { label } => {
                let definition = reference_definitions.get(label)?;
                Some(ResolvedImageTarget {
                    src: definition.src.clone(),
                    title: definition.title.clone(),
                })
            }
        }
    }
}

/// Markdown 图片目标 → 本地路径 / 远程 URI 的唯一口径。
///
/// 阅读视图（`src/components/block/runtime/image.rs`、`render/content.rs`）与导出
/// （`src/export/html.rs:local_image_data_uri`）都走这里：以前两边各有一份 percent
/// 解码和 `file:` 处理，同一个写法在正文里能显示、在导出里漏内联（报告 12）。
pub(crate) fn resolve_image_source(source: &str, base_dir: Option<&Path>) -> ImageResolvedSource {
    // 解析期已经剥过一次尖括号（`normalize_image_source`），这里再剥一次，让拿到
    // 裸目标的调用方（HTML `<img>`、引用表、导出）共用同一条规则。
    let destination = angle_bracket_destination(source);

    // 远程写法不做 percent 解码：`%20` 属于 URL 本身，请求时必须原样送出去。
    if net::is_remote_image_source(destination) {
        return ImageResolvedSource::Remote(SharedUri::from(destination.to_string()));
    }

    // `file:` 后紧跟 `/` 才是绝对文件 URL；`file:relative.png` 是相对文档的写法，
    // 按 RFC 3986 剥掉 scheme 再走相对解析——直接丢给 `url` 会被解成根目录下的文件。
    let absolute_file_url = destination.starts_with("file:/");
    let destination = if absolute_file_url {
        destination
    } else {
        destination.strip_prefix("file:").unwrap_or(destination)
    };

    if absolute_file_url
        && let Some(path) = local_path_from_file_url(destination)
    {
        return ImageResolvedSource::Local(path);
    }

    let decoded = percent_decode_or_raw(destination);
    let path = Path::new(decoded.as_ref());
    if path.is_absolute() {
        return ImageResolvedSource::Local(path.to_path_buf());
    }

    let resolved = base_dir
        .map(|dir| dir.join(path))
        .unwrap_or_else(|| path.to_path_buf());
    ImageResolvedSource::Local(resolved)
}

/// 绝对 `file:` URL → 本地绝对路径。scheme、authority 与 Windows 盘符是 URL 语法，
/// 交给 `url` crate（与粘贴图片 `src/components/block/interactions.rs` 同一套）；它
/// 同时负责 percent 解码，所以这条分支不能再解第二遍（`%25` 解两次会变味）。
/// 畸形 URL 与远程 `file:` authority 返回 `None`，退回普通路径口径。
fn local_path_from_file_url(destination: &str) -> Option<PathBuf> {
    let file_url = url::Url::parse(destination).ok()?;
    let path = file_url.to_file_path().ok()?;
    path.is_absolute().then_some(path)
}

/// Splits a trailing `{width=NN%}` attribute off a standalone image, as written
/// back by the resize handle (roadmap C10 v2). Returns the remaining markdown
/// plus the parsed percent when the attribute is well-formed.
pub(crate) fn split_standalone_image_width(markdown: &str) -> (&str, Option<u8>) {
    let trimmed = markdown.trim_end();
    let Some(open) = trimmed.rfind('{') else {
        return (markdown, None);
    };
    let Some(inner) = trimmed[open + 1..].strip_suffix('}') else {
        return (markdown, None);
    };
    let Some(value) = inner.strip_prefix("width=") else {
        return (markdown, None);
    };
    let Some(number) = value.strip_suffix('%') else {
        return (markdown, None);
    };
    let Ok(percent) = number.trim().parse::<u8>() else {
        return (markdown, None);
    };
    if !(1..=100).contains(&percent) {
        return (markdown, None);
    }
    (&trimmed[..open], Some(percent))
}

/// Percent from a trailing `{width=NN%}` attribute, when present.
pub(crate) fn standalone_image_width_percent(markdown: &str) -> Option<u8> {
    split_standalone_image_width(markdown).1
}

pub(crate) fn parse_standalone_image(markdown: &str) -> Option<ImageSyntax> {
    let (markdown, _) = split_standalone_image_width(markdown);
    if markdown.contains('\n') || markdown.contains('\r') {
        return None;
    }
    let markdown = markdown.trim();
    if markdown.is_empty() {
        return None;
    }
    if !markdown.starts_with("![") {
        return None;
    }

    let bytes = markdown.as_bytes();
    let mut alt_end = None;
    for index in 2..bytes.len() {
        if bytes[index] == b']' && !is_escaped(markdown, index) {
            alt_end = Some(index);
            break;
        }
    }
    let alt_end = alt_end?;

    let alt = unescape_ascii_punctuation(&markdown[2..alt_end]);
    match bytes.get(alt_end + 1) {
        Some(b'(') if markdown.ends_with(')') => {
            let inner = &markdown[alt_end + 2..markdown.len() - 1];
            let (src, title) = parse_image_target(inner)?;
            Some(ImageSyntax {
                alt,
                target: ImageTarget::Direct { src, title },
            })
        }
        Some(b'[') if markdown.ends_with(']') => {
            let raw_label = &markdown[alt_end + 2..markdown.len() - 1];
            let label_source = if raw_label.is_empty() {
                alt.as_str()
            } else {
                raw_label
            };
            let label = normalize_reference_label(label_source)?;
            Some(ImageSyntax {
                alt,
                target: ImageTarget::Reference { label },
            })
        }
        None => {
            let label = normalize_reference_label(&alt)?;
            Some(ImageSyntax {
                alt,
                target: ImageTarget::Reference { label },
            })
        }
        _ => None,
    }
}

pub(crate) fn parse_table_cell_inline_images(markdown: &str) -> Vec<TableCellInlineImageSegment> {
    let mut segments = Vec::new();
    let mut text_start = 0usize;
    let mut cursor = 0usize;
    let mut found_image = false;

    while cursor < markdown.len() {
        // 行内代码里的 `![alt](src)` 是字面文本：CommonMark 不解析代码段内部的
        // 语法，整段直接跳过（用户报修：表格单元格里被反引号包住的图片语法
        // 渲染成了「无法加载图片」占位框）。
        if markdown.as_bytes()[cursor] == b'`'
            && !is_escaped(markdown, cursor)
            && let Some(end) = inline_code_span_end(markdown, cursor)
        {
            cursor = end;
            continue;
        }

        if markdown[cursor..].starts_with("![")
            && !is_escaped(markdown, cursor)
            && let Some((image_markdown, syntax, end)) = parse_inline_image_at(markdown, cursor)
        {
            if is_link_wrapped_inline_image(markdown, cursor, end) {
                cursor += markdown[cursor..]
                    .chars()
                    .next()
                    .map(char::len_utf8)
                    .unwrap_or(1);
                continue;
            }

            if text_start < cursor {
                segments.push(TableCellInlineImageSegment::Text(
                    markdown[text_start..cursor].to_string(),
                ));
            }
            segments.push(TableCellInlineImageSegment::Image {
                markdown: image_markdown,
                syntax,
            });
            found_image = true;
            cursor = end;
            text_start = cursor;
            continue;
        }

        cursor += markdown[cursor..]
            .chars()
            .next()
            .map(char::len_utf8)
            .unwrap_or(1);
    }

    if text_start < markdown.len() {
        segments.push(TableCellInlineImageSegment::Text(
            markdown[text_start..].to_string(),
        ));
    }

    if found_image {
        segments
    } else {
        vec![TableCellInlineImageSegment::Text(markdown.to_string())]
    }
}

fn inline_code_span_end(markdown: &str, start: usize) -> Option<usize> {
    let bytes = markdown.as_bytes();
    let mut opening_end = start;
    while opening_end < bytes.len() && bytes[opening_end] == b'`' {
        opening_end += 1;
    }
    let run_len = opening_end - start;
    let mut cursor = opening_end;
    while cursor < bytes.len() {
        if bytes[cursor] == b'`' && !is_escaped(markdown, cursor) {
            let mut end = cursor;
            while end < bytes.len() && bytes[end] == b'`' {
                end += 1;
            }
            if end - cursor == run_len {
                return Some(end);
            }
            cursor = end;
            continue;
        }
        cursor += markdown[cursor..]
            .chars()
            .next()
            .map(char::len_utf8)
            .unwrap_or(1);
    }
    None
}

fn parse_inline_image_at(markdown: &str, start: usize) -> Option<(String, ImageSyntax, usize)> {
    if !markdown[start..].starts_with("![") {
        return None;
    }

    let alt_end = find_unescaped_char(markdown, start + 2, b']')?;
    let alt = unescape_ascii_punctuation(&markdown[start + 2..alt_end]);
    let next = markdown.as_bytes().get(alt_end + 1).copied();

    match next {
        Some(b'(') => {
            let close = find_unescaped_char(markdown, alt_end + 2, b')')?;
            let inner = &markdown[alt_end + 2..close];
            let (src, title) = parse_image_target(inner)?;
            let end = close + 1;
            Some((
                markdown[start..end].to_string(),
                ImageSyntax {
                    alt,
                    target: ImageTarget::Direct { src, title },
                },
                end,
            ))
        }
        Some(b'[') => {
            let close = find_unescaped_char(markdown, alt_end + 2, b']')?;
            let raw_label = &markdown[alt_end + 2..close];
            let label_source = if raw_label.is_empty() {
                alt.as_str()
            } else {
                raw_label
            };
            let label = normalize_reference_label(label_source)?;
            let end = close + 1;
            Some((
                markdown[start..end].to_string(),
                ImageSyntax {
                    alt,
                    target: ImageTarget::Reference { label },
                },
                end,
            ))
        }
        _ => {
            let label = normalize_reference_label(&alt)?;
            let end = alt_end + 1;
            Some((
                markdown[start..end].to_string(),
                ImageSyntax {
                    alt,
                    target: ImageTarget::Reference { label },
                },
                end,
            ))
        }
    }
}

fn is_link_wrapped_inline_image(markdown: &str, start: usize, end: usize) -> bool {
    let mut cursor = 0usize;
    let mut open_label = None;
    while cursor < start {
        let byte = markdown.as_bytes()[cursor];
        if byte == b'[' && !is_escaped(markdown, cursor) {
            open_label = Some(cursor);
        } else if byte == b']' && !is_escaped(markdown, cursor) {
            open_label = None;
        }
        cursor += markdown[cursor..]
            .chars()
            .next()
            .map(char::len_utf8)
            .unwrap_or(1);
    }

    open_label.is_some() && markdown[end..].starts_with("](")
}

pub(crate) fn parse_image_reference_definitions(markdown: &str) -> ImageReferenceDefinitions {
    let lines = markdown.split('\n').collect::<Vec<_>>();
    let normalized_lines = lines
        .iter()
        .map(|line| strip_reference_scan_container_prefixes(line).to_string())
        .collect::<Vec<_>>();
    let normalized_refs = normalized_lines
        .iter()
        .map(|line| line.as_str())
        .collect::<Vec<_>>();
    let mut definitions = ImageReferenceDefinitions::new();
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
            parse_image_reference_definition(&normalized_refs, index)
        else {
            index += 1;
            continue;
        };

        definitions.entry(label).or_insert(definition);
        index += consumed;
    }

    definitions
}

pub(crate) fn normalize_reference_label(label: &str) -> Option<String> {
    // Single-pass concat: walk the words once, push to the output with a
    // leading separator on all but the first. Avoids the intermediate
    // Vec<&str> allocation that split_whitespace().collect::<Vec<_>>()
    // produces before .join("") copies again.
    let mut normalized = String::with_capacity(label.len());
    for word in label.split_whitespace() {
        if !normalized.is_empty() {
            normalized.push(' ');
        }
        normalized.push_str(word);
    }
    if normalized.is_empty() {
        None
    } else {
        Some(normalized.to_lowercase())
    }
}

fn parse_image_reference_definition(
    lines: &[&str],
    start: usize,
) -> Option<(String, ImageReferenceDefinition, usize)> {
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

    if let Some(next_line) = lines.get(start + 1)
        && is_reference_definition_title_continuation(next_line)
    {
        if !target.is_empty() {
            target.push(' ');
        }
        target.push_str(next_line.trim());
        consumed += 1;
    }

    let (src, title) = parse_image_target(&target)?;
    Some((label, ImageReferenceDefinition { src, title }, consumed))
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

fn parse_image_target(inner: &str) -> Option<(String, Option<String>)> {
    if inner.is_empty() {
        return None;
    }

    if inner.ends_with('"') {
        let close_quote = inner.len() - 1;
        if !is_escaped(inner, close_quote)
            && let Some(open_quote) = find_open_title_quote(inner, close_quote)
        {
            let src = inner[..open_quote.saturating_sub(1)].trim_end();
            let title = inner[open_quote + 1..close_quote].to_string();
            if src.is_empty() {
                return None;
            }
            return Some((normalize_image_source(src), Some(title)));
        }
    }

    Some((normalize_image_source(inner), None))
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

fn find_open_title_quote(input: &str, close_quote: usize) -> Option<usize> {
    let bytes = input.as_bytes();
    (0..close_quote).rev().find(|&index| {
        bytes[index] == b'"'
            && !is_escaped(input, index)
            && index > 0
            && bytes[index - 1].is_ascii_whitespace()
    })
}

fn normalize_image_source(source: &str) -> String {
    // 先剥尖括号再解转义：`<assets/blue\>card.png>` 里的 `\>` 在尖括号内部是合法的
    // 转义，反过来先解转义会把定界符本身解没。
    unescape_ascii_punctuation(angle_bracket_destination(source))
}

/// CommonMark 的 `<…>` 目标：内部允许空格，只不许出现未转义的 `<`、`>` 和换行。
///
/// 这里以前用「`Uri::from_str` 能不能解析」当判据，于是 `<assets/blue card.png>`
/// 因为空格不是合法 URI 字符而保留尖括号，整个字符串被当成文件名去找——带空格的本地
/// 路径在 `<…>` 写法下永远加载失败（报告 12）。尖括号是 Markdown 的语法，不是 URI
/// 的一部分，判定也只能按 Markdown 的规则来。
fn angle_bracket_destination(source: &str) -> &str {
    let Some(inner) = source
        .strip_prefix('<')
        .and_then(|rest| rest.strip_suffix('>'))
    else {
        return source;
    };
    // `<>` 是空目标，CommonMark 不认，原样留给调用方判空。
    if inner.is_empty() || inner.contains(['\n', '\r']) {
        return source;
    }
    let bytes = inner.as_bytes();
    for (index, byte) in bytes.iter().enumerate() {
        if matches!(byte, b'<' | b'>') && !is_escaped(inner, index) {
            return source;
        }
    }
    inner
}

fn unescape_ascii_punctuation(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' && chars.peek().is_some_and(|next| next.is_ascii_punctuation()) {
            output.push(chars.next().expect("peeked punctuation must exist"));
        } else {
            output.push(ch);
        }
    }
    output
}

fn find_unescaped_char(input: &str, start: usize, target: u8) -> Option<usize> {
    let bytes = input.as_bytes();
    (start..bytes.len()).find(|&index| bytes[index] == target && !is_escaped(input, index))
}

fn is_escaped(input: &str, index: usize) -> bool {
    if index == 0 {
        return false;
    }

    let bytes = input.as_bytes();
    let mut backslashes = 0usize;
    let mut cursor = index;
    while cursor > 0 {
        cursor -= 1;
        if bytes[cursor] == b'\\' {
            backslashes += 1;
        } else {
            break;
        }
    }
    backslashes % 2 == 1
}


#[cfg(test)]
mod tests;
