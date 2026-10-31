//! Standalone image syntax, references, and source resolution.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use gpui::SharedUri;

use crate::file_url::{local_path_from_file_url, percent_decode_or_raw, strip_file_scheme_prefix};
use crate::net;

use super::references::{find_unescaped_char, parse_reference_definition_line, scan_reference_definitions};

/// Parsed standalone image expression.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ImageSyntax {
    pub(crate) alt: String,
    pub(crate) target: ImageTarget,
}

/// 单元格图片解析不出可用来源时的降级文本（评审 P146，判据单点）。
///
/// 与 CommonMark 的图片语义一致显示 alt。「标题整格」与「行内分词」两条
/// 渲染路径共用这一处；整格路径曾回退渲染整段 markdown 原文（`!` 与括号
/// 齐出），同一张表在不同渲染时刻会看到两种失败表现。
pub(crate) fn unavailable_image_fallback_text(syntax: &ImageSyntax) -> &str {
    &syntax.alt
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
    // 解析期已经剥过一次尖括号（`link::normalize_markdown_destination`），这里
    // 再剥一次，让拿到裸目标的调用方（HTML `<img>`、引用表、导出）共用同一条规则。
    let destination = angle_bracket_destination(source);

    // 远程写法不做 percent 解码：`%20` 属于 URL 本身，请求时必须原样送出去。
    // 协议相对 `//host/x` 在这里补成 https，不能落到下面的本地路径分支。
    if let Some(remote) = net::remote_image_uri(destination) {
        return ImageResolvedSource::Remote(SharedUri::from(remote));
    }
    // 与 is_safe_image_url 同一条大小写口径：`DATA:image/png` 过了安全判据
    // 就必须同样走远端分支，落到本地路径分支必然渲染失败。
    if destination.to_ascii_lowercase().starts_with("data:image/") {
        return ImageResolvedSource::Remote(SharedUri::from(destination.to_string()));
    }

    // `file:` 后紧跟 `/` 才是绝对文件 URL；`file:relative.png` 是相对文档的写法，
    // 按 RFC 3986 剥掉 scheme 再走相对解析——直接丢给 `url` 会被解成根目录下的文件。
    // scheme 比较按语法小写归一（`FILE:///…` 与 `file:///…` 是同一个 URL），非 `file`
    // 的 scheme（`note:backup.png`）不剥，原样当相对路径。
    let after_file_scheme = strip_file_scheme_prefix(destination);
    let absolute_file_url = after_file_scheme.is_some_and(|rest| rest.starts_with('/'));
    let destination = match after_file_scheme {
        // 绝对 URL 整条交给 `url` crate（它自己的 scheme 解析就是大小写不敏感的）。
        _ if absolute_file_url => destination,
        Some(rest) => rest,
        None => destination,
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

/// 写法按任一平台语法像机器本地绝对路径。
///
/// 不能换用 `Path::is_absolute`：那是运行平台的语义——Windows 上 `/Users/…`
/// 有根无前缀会被判相对，macOS 上 `C:\…` 同理。导出侧的隐私判据（内联不成的
/// 绝对路径图不得把作者目录写进分享物）识别的是文档带的写法，判成相对就会
/// 换个平台原样泄漏。`C:note.png` 这种盘符相对写法也按绝对收进：内联成立时
/// 根本走不到这条判据，只有内联不成才从「原样留」变成中性占位。
pub(crate) fn path_writing_is_absolute(path: &Path) -> bool {
    if path.is_absolute() {
        return true;
    }
    let text = path.to_string_lossy();
    if text.starts_with('/') || text.starts_with('\\') {
        return true;
    }
    let bytes = text.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
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

    let alt = super::link::unescape_ascii_punctuation(&markdown[2..alt_end]);
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
        // 语法，整段直接跳过（此前表格单元格里被反引号包住的图片语法
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
        // 代码段内反斜杠是普通字符：闭合反引号不看前一位是不是转义前缀。
        if bytes[cursor] == b'`' {
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
    let alt = super::link::unescape_ascii_punctuation(&markdown[start + 2..alt_end]);
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
    scan_reference_definitions(markdown, parse_image_reference_definition)
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
    let line = parse_reference_definition_line(lines, start)?;
    let (src, title) = parse_image_target(&line.target)?;
    Some((
        line.label,
        ImageReferenceDefinition { src, title },
        line.consumed,
    ))
}

fn parse_image_target(inner: &str) -> Option<(String, Option<String>)> {
    if inner.is_empty() {
        return None;
    }

    if let Some((src, title)) = super::link::split_target_title(inner) {
        return Some((
            super::link::normalize_markdown_destination(src)?,
            Some(super::link::unescape_ascii_punctuation(title)),
        ));
    }

    Some((super::link::normalize_markdown_destination(inner)?, None))
}


/// CommonMark 的 `<…>` 目标：内部允许空格，只不许出现未转义的 `<`、`>` 和换行。
///
/// 这里以前用「`Uri::from_str` 能不能解析」当判据，于是 `<assets/blue card.png>`
/// 因为空格不是合法 URI 字符而保留尖括号，整个字符串被当成文件名去找——带空格的本地
/// 路径在 `<…>` 写法下永远加载失败（报告 12）。尖括号是 Markdown 的语法，不是 URI
/// 的一部分，判定也只能按 Markdown 的规则来。n
pub(crate) fn angle_bracket_destination(source: &str) -> &str {
    let Some(inner) = source
        .strip_prefix('<')
        .and_then(|rest| rest.strip_suffix('>'))
    else {
        return source;
    };
    if inner.is_empty() {
        return "";
    }
    if inner.contains(['\n', '\r']) {
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

/// Markdown 文本里这个字符前的反斜杠是不是转义前缀（奇数个才算）。
///
/// 链接目标与图片目标共用这一份：两边曾各写一遍同样的扫描循环，
/// 判据则同走 `inline::backslash_run_escapes`。
pub(crate) fn is_escaped(input: &str, index: usize) -> bool {
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
    crate::components::markdown::inline::backslash_run_escapes(backslashes)
}


#[cfg(test)]
mod tests;
