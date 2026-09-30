//! HTML document generation for Markdown export.

pub(super) use std::fs;
pub(super) use std::path::Path;

pub(super) use base64::{Engine as _, engine::general_purpose};
pub(super) use gpui::{Hsla, Rgba};
pub(super) use pulldown_cmark::{CowStr, Event, Options, Parser, Tag, html};

pub(super) use crate::components::{
    inline_math_font_size, is_mermaid_closing_fence, parse_display_math_source,
    parse_html_image_block, parse_mermaid_fence_source, parse_mermaid_fence_start,
    render_latex_to_svg, render_mermaid_to_svg, sanitize_html_for_export,
};
pub(super) use crate::net;
pub(super) use crate::theme::{FontWeightDef, Theme};

/// Builds a full HTML document with embedded CSS derived from the active theme.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn render_html(markdown: &str, theme: &Theme, title: &str) -> String {
    render_html_with_base_dir(markdown, theme, title, None)
}

/// Builds export HTML and resolves local Markdown image paths relative to the source document.
pub(crate) fn render_html_with_base_dir(
    markdown: &str,
    theme: &Theme,
    title: &str,
    base_dir: Option<&Path>,
) -> String {
    render_html_document(markdown, theme, title, base_dir, &crate::export::html::css::theme_css(theme))
}

/// Builds HTML tailored for Chromium's print-to-PDF pipeline.
pub(crate) fn render_chromium_pdf_html_with_base_dir(
    markdown: &str,
    theme: &Theme,
    title: &str,
    base_dir: Option<&Path>,
) -> String {
    render_html_document(
        markdown,
        theme,
        title,
        base_dir,
        &chromium_pdf_theme_css(theme),
    )
}

fn render_html_document(
    markdown: &str,
    theme: &Theme,
    title: &str,
    base_dir: Option<&Path>,
    css: &str,
) -> String {
    let document_lang = if contains_tibetan_text(markdown) || contains_tibetan_text(title) {
        "bo"
    } else {
        "en"
    };
    let body = render_browser_html_body(markdown, theme, base_dir);

    format!(
        "<!doctype html>\n<html lang=\"{}\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<title>{}</title>\n<style>\n{}\n</style>\n</head>\n<body>\n<main class=\"vlt-document\">\n{}</main>\n</body>\n</html>\n",
        document_lang,
        css::escape_html(title),
        css,
        body,
    )
}

fn markdown_options() -> Options {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_GFM);
    options
}

fn render_browser_html_body(markdown: &str, theme: &Theme, base_dir: Option<&Path>) -> String {
    let rewritten = rewrite_visible_comment_blocks(markdown);
    let rewritten = rewrite_unsafe_html_blocks(&rewritten, base_dir);
    let rewritten = rewrite_display_math_blocks(&rewritten, theme);
    let rewritten = rewrite_inline_math(&rewritten, theme);
    let rewritten = rewrite_mermaid_blocks(&rewritten);
    let parser = Parser::new_ext(&rewritten, markdown_options())
        .map(|event| rewrite_local_image_event(event, base_dir));
    let mut body = String::new();
    html::push_html(&mut body, parser);
    body
}

fn rewrite_visible_comment_blocks(markdown: &str) -> String {
    let lines = markdown.split('\n').collect::<Vec<_>>();
    let mut rewritten = Vec::with_capacity(lines.len());
    let mut index = 0usize;
    let mut active_fence: Option<(char, usize)> = None;

    while index < lines.len() {
        let line = lines[index];
        if let Some((marker, run_len)) = active_fence {
            rewritten.push(line.to_string());
            if is_closing_fence(line, marker, run_len) {
                active_fence = None;
            }
            index += 1;
            continue;
        }

        if let Some(fence) = opening_fence(line) {
            active_fence = Some(fence);
            rewritten.push(line.to_string());
            index += 1;
            continue;
        }

        if !is_root_comment_start(line) {
            rewritten.push(line.to_string());
            index += 1;
            continue;
        }

        let start = index;
        let mut end = index;
        while end < lines.len() && !lines[end].contains("-->") {
            end += 1;
        }

        if end >= lines.len() {
            rewritten.push(line.to_string());
            index += 1;
            continue;
        }

        let raw_comment = lines[start..=end].join("\n");
        rewritten.push(format!(
            "<pre class=\"vlt-comment\">{}</pre>",
            css::escape_html(&raw_comment)
        ));
        index = end + 1;
    }

    rewritten.join("\n")
}

fn rewrite_inline_math(markdown: &str, theme: &Theme) -> String {
    let mut rewritten = Vec::new();
    let mut active_fence: Option<(char, usize)> = None;
    for line in markdown.split('\n') {
        if let Some((marker, run_len)) = active_fence {
            rewritten.push(line.to_string());
            if is_closing_fence(line, marker, run_len) {
                active_fence = None;
            }
            continue;
        }

        if let Some(fence) = opening_fence(line) {
            active_fence = Some(fence);
            rewritten.push(line.to_string());
            continue;
        }

        rewritten.push(rewrite_inline_math_line(line, theme));
    }

    rewritten.join("\n")
}

fn rewrite_inline_math_line(line: &str, theme: &Theme) -> String {
    let mut output = String::with_capacity(line.len());
    let mut index = 0usize;
    while index < line.len() {
        if line[index..].starts_with('`') {
            let run_len = line[index..]
                .bytes()
                .take_while(|byte| *byte == b'`')
                .count();
            if let Some(close) = find_backtick_run(line, index + run_len, run_len) {
                output.push_str(&line[index..close + run_len]);
                index = close + run_len;
                continue;
            }
        }

        if let Some((end, body)) = locate_inline_dollar_math_source(line, index)
            .or_else(|| locate_inline_paren_math_source(line, index))
        {
            match render_latex_to_svg(
                &body,
                theme.colors.text_default,
                inline_math_font_size(theme.typography.text_size),
                crate::components::latex::MathLayout::Inline,
            ) {
                Ok(svg) => {
                    output.push_str(&format!("<span class=\"vlt-inline-math\">{svg}</span>"))
                }
                Err(_) => output.push_str(&css::escape_html(&line[index..end])),
            }
            index = end;
            continue;
        }

        if let Some((end, body, tag)) = locate_inline_script_source(line, index) {
            output.push_str(&format!("<{tag}>{}</{tag}>", css::escape_html(&body)));
            index = end;
            continue;
        }

        let Some(ch) = line[index..].chars().next() else {
            break;
        };
        output.push(ch);
        index += ch.len_utf8();
    }
    output
}

fn find_backtick_run(line: &str, mut index: usize, run_len: usize) -> Option<usize> {
    while index < line.len() {
        if line[index..].starts_with(&"`".repeat(run_len)) {
            return Some(index);
        }
        index += line[index..].chars().next()?.len_utf8();
    }
    None
}

fn locate_inline_dollar_math_source(line: &str, index: usize) -> Option<(usize, String)> {
    if !line[index..].starts_with('$')
        || line[index..].starts_with("$$")
        || is_escaped_ascii(line, index)
    {
        return None;
    }
    let mut cursor = index + 1;
    while cursor < line.len() {
        if line[cursor..].starts_with('$')
            && !line[cursor..].starts_with("$$")
            && !is_escaped_ascii(line, cursor)
        {
            let body = &line[index + 1..cursor];
            if valid_inline_math_body(body)
                && !looks_like_export_currency(line, index, cursor, body)
            {
                return Some((cursor + 1, body.to_string()));
            }
            return None;
        }
        cursor += line[cursor..].chars().next()?.len_utf8();
    }
    None
}

fn locate_inline_script_source(line: &str, index: usize) -> Option<(usize, String, &'static str)> {
    if is_escaped_ascii(line, index) {
        return None;
    }

    if line[index..].starts_with('^') {
        locate_script_close(line, index, '^').map(|(end, body)| (end, body, "sup"))
    } else if is_single_tilde_marker(line, index) {
        locate_script_close(line, index, '~').map(|(end, body)| (end, body, "sub"))
    } else {
        None
    }
}

fn locate_script_close(line: &str, index: usize, marker: char) -> Option<(usize, String)> {
    let prev = previous_char(line, index)?;
    if !prev.is_ascii_alphanumeric() {
        return None;
    }

    let body_start = index + marker.len_utf8();
    let first = line[body_start..].chars().next()?;
    if !first.is_ascii_alphanumeric() {
        return None;
    }

    let mut cursor = body_start;
    while cursor < line.len() {
        if line[cursor..].starts_with(marker)
            && !is_escaped_ascii(line, cursor)
            && (marker != '~' || is_single_tilde_marker(line, cursor))
        {
            let body = &line[body_start..cursor];
            return body
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric())
                .then(|| (cursor + marker.len_utf8(), body.to_string()));
        }
        cursor += line[cursor..].chars().next()?.len_utf8();
    }

    None
}

fn previous_char(line: &str, index: usize) -> Option<char> {
    line.get(..index)?.chars().next_back()
}

fn is_single_tilde_marker(line: &str, index: usize) -> bool {
    line[index..].starts_with('~')
        && previous_char(line, index).is_none_or(|ch| ch != '~')
        && line[index + 1..].chars().next().is_none_or(|ch| ch != '~')
}

fn locate_inline_paren_math_source(line: &str, index: usize) -> Option<(usize, String)> {
    if !line[index..].starts_with("\\(") {
        return None;
    }
    let mut cursor = index + 2;
    while cursor + 1 < line.len() {
        if line[cursor..].starts_with("\\)") {
            let body = &line[index + 2..cursor];
            if valid_inline_math_body(body) {
                return Some((cursor + 2, body.to_string()));
            }
            return None;
        }
        cursor += line[cursor..].chars().next()?.len_utf8();
    }
    None
}

fn valid_inline_math_body(body: &str) -> bool {
    !body.is_empty() && !body.contains(['\n', '\r']) && body.trim() == body && !body.is_empty()
}

fn is_escaped_ascii(line: &str, index: usize) -> bool {
    let mut slash_count = 0usize;
    let mut cursor = index;
    while cursor > 0 && line.as_bytes()[cursor - 1] == b'\\' {
        slash_count += 1;
        cursor -= 1;
    }
    slash_count % 2 == 1
}

fn looks_like_export_currency(line: &str, open: usize, close: usize, body: &str) -> bool {
    let prev_is_digit = open > 0 && line.as_bytes()[open - 1].is_ascii_digit();
    let next_is_digit = close + 1 < line.len() && line.as_bytes()[close + 1].is_ascii_digit();
    (prev_is_digit || next_is_digit)
        || (body
            .chars()
            .all(|ch| ch.is_ascii_digit() || matches!(ch, '.' | ',' | '_'))
            && body.chars().any(|ch| ch.is_ascii_digit())
            && body.len() > 1)
}

fn rewrite_unsafe_html_blocks(markdown: &str, base_dir: Option<&Path>) -> String {
    let lines = markdown.split('\n').collect::<Vec<_>>();
    let mut rewritten = Vec::with_capacity(lines.len());
    let mut index = 0usize;
    let mut active_fence: Option<(char, usize)> = None;

    while index < lines.len() {
        let line = lines[index];
        if let Some((marker, run_len)) = active_fence {
            rewritten.push(line.to_string());
            if is_closing_fence(line, marker, run_len) {
                active_fence = None;
            }
            index += 1;
            continue;
        }

        if let Some(fence) = opening_fence(line) {
            active_fence = Some(fence);
            rewritten.push(line.to_string());
            index += 1;
            continue;
        }

        let Some(html_start) = root_html_start(line) else {
            rewritten.push(line.to_string());
            index += 1;
            continue;
        };

        let end = collect_export_html_region(&lines, index, &html_start);
        let raw = lines[index..end].join("\n");
        if let Some(image) = parse_html_image_block(&raw) {
            let src =
                local_image_data_uri(&image.src, base_dir).unwrap_or_else(|| image.src.clone());
            rewritten.push(image.to_sanitized_html_with_src(&src));
        } else {
            rewritten.push(sanitize_html_for_export(&raw));
        }
        index = end;
    }

    rewritten.join("\n")
}

fn rewrite_display_math_blocks(markdown: &str, theme: &Theme) -> String {
    let lines = markdown.split('\n').collect::<Vec<_>>();
    let mut rewritten = Vec::with_capacity(lines.len());
    let mut index = 0usize;
    let mut active_fence: Option<(char, usize)> = None;

    while index < lines.len() {
        let line = lines[index];
        if let Some((marker, run_len)) = active_fence {
            rewritten.push(line.to_string());
            if is_closing_fence(line, marker, run_len) {
                active_fence = None;
            }
            index += 1;
            continue;
        }

        if let Some(fence) = opening_fence(line) {
            active_fence = Some(fence);
            rewritten.push(line.to_string());
            index += 1;
            continue;
        }

        if !is_root_display_math_start(line) {
            rewritten.push(line.to_string());
            index += 1;
            continue;
        }

        let end = collect_display_math_region(&lines, index);
        let raw = lines[index..end].join("\n");
        if let Some(source) = parse_display_math_source(&raw) {
            match render_latex_to_svg(
                &source.body,
                theme.colors.text_default,
                theme.typography.text_size,
                crate::components::latex::MathLayout::Display,
            ) {
                Ok(svg) => rewritten.push(format!("<div class=\"vlt-math\">{svg}</div>")),
                Err(_) => rewritten.push(format!(
                    "<pre class=\"vlt-math-error\">{}</pre>",
                    css::escape_html(&raw)
                )),
            }
        } else {
            rewritten.push(raw);
        }
        index = end;
    }

    rewritten.join("\n")
}

fn rewrite_mermaid_blocks(markdown: &str) -> String {
    let lines = markdown.split('\n').collect::<Vec<_>>();
    let mut rewritten = Vec::with_capacity(lines.len());
    let mut index = 0usize;

    while index < lines.len() {
        let line = lines[index];
        let Some(fence) = parse_mermaid_fence_start(line) else {
            rewritten.push(line.to_string());
            index += 1;
            continue;
        };

        let mut end = index + 1;
        while end < lines.len() && !is_mermaid_closing_fence(lines[end], fence) {
            end += 1;
        }
        if end >= lines.len() {
            rewritten.push(line.to_string());
            index += 1;
            continue;
        }

        let raw = lines[index..=end].join("\n");
        if let Some(source) = parse_mermaid_fence_source(&raw) {
            match render_mermaid_to_svg(&source.body) {
                Ok(svg) => {
                    let src = data_uri_for_bytes("image/svg+xml", svg.as_bytes());
                    rewritten.push(format!(
                        "<div class=\"vlt-mermaid\"><img alt=\"Mermaid diagram\" src=\"{src}\"></div>"
                    ));
                }
                Err(_) => rewritten.push(format!(
                    "<pre class=\"vlt-mermaid-error\">{}</pre>",
                    css::escape_html(&raw)
                )),
            }
        } else {
            rewritten.push(raw);
        }
        index = end + 1;
    }

    rewritten.join("\n")
}

fn rewrite_local_image_event<'a>(event: Event<'a>, base_dir: Option<&Path>) -> Event<'a> {
    match event {
        Event::Start(Tag::Image {
            link_type,
            dest_url,
            title,
            id,
        }) => {
            let dest_url = local_image_data_uri(dest_url.as_ref(), base_dir)
                .map(CowStr::from)
                .unwrap_or(dest_url);
            Event::Start(Tag::Image {
                link_type,
                dest_url,
                title,
                id,
            })
        }
        event => event,
    }
}

fn local_image_data_uri(source: &str, base_dir: Option<&Path>) -> Option<String> {
    if source.is_empty()
        || source.starts_with('#')
        || source.starts_with("data:")
        || net::is_remote_image_source(source)
    {
        return None;
    }

    let path = Path::new(source);
    let resolved = if path.is_absolute() {
        path.to_path_buf()
    } else {
        base_dir?.join(path)
    };
    let mime = image_mime_from_path(&resolved)?;
    let bytes = fs::read(&resolved).ok()?;
    Some(data_uri_for_bytes(mime, &bytes))
}

fn image_mime_from_path(path: &Path) -> Option<&'static str> {
    let extension = path.extension()?.to_string_lossy().to_ascii_lowercase();
    match extension.as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "svg" => Some("image/svg+xml"),
        "bmp" => Some("image/bmp"),
        _ => None,
    }
}

fn data_uri_for_bytes(mime: &str, bytes: &[u8]) -> String {
    format!(
        "data:{mime};base64,{}",
        general_purpose::STANDARD.encode(bytes)
    )
}

#[derive(Clone, Debug)]
struct ExportHtmlStart {
    name: String,
    self_closing: bool,
    closes_same_line: bool,
}

fn root_html_start(line: &str) -> Option<ExportHtmlStart> {
    let trimmed = line.trim_start();
    if line.len() - trimmed.len() > 3 || trimmed.starts_with("<!--") {
        return None;
    }

    let tagged = trimmed.strip_prefix('<')?;
    if tagged.starts_with('/') || tagged.starts_with('!') || tagged.starts_with('?') {
        return None;
    }
    let name_len = tagged
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '-')
        .map(char::len_utf8)
        .sum::<usize>();
    if name_len == 0 {
        return None;
    }
    let name = tagged[..name_len].to_ascii_lowercase();
    let suffix = &tagged[name_len..];
    let next = suffix.chars().next()?;
    if !matches!(next, '>' | ' ' | '\t' | '/') {
        return None;
    }
    Some(ExportHtmlStart {
        self_closing: trimmed.ends_with("/>") || is_export_void_html_tag(&name),
        closes_same_line: trimmed.to_ascii_lowercase().contains(&format!("</{name}>")),
        name,
    })
}

fn is_export_void_html_tag(name: &str) -> bool {
    matches!(name, "br" | "hr" | "img")
}

fn collect_export_html_region(lines: &[&str], start: usize, html: &ExportHtmlStart) -> usize {
    if html.self_closing || html.closes_same_line {
        return start + 1;
    }

    let close = format!("</{}>", html.name);
    let mut index = start + 1;
    while index < lines.len() {
        let line = lines[index];
        if line.to_ascii_lowercase().contains(&close) {
            return index + 1;
        }
        if line.trim().is_empty() {
            return index;
        }
        index += 1;
    }

    lines.len()
}

fn opening_fence(line: &str) -> Option<(char, usize)> {
    let trimmed = line.trim_start();
    if line.len() - trimmed.len() > 3 {
        return None;
    }

    let marker = trimmed.chars().next()?;
    if marker != '`' && marker != '~' {
        return None;
    }

    let run_len = trimmed.chars().take_while(|ch| *ch == marker).count();
    (run_len >= 3).then_some((marker, run_len))
}

fn is_closing_fence(line: &str, marker: char, opening_run_len: usize) -> bool {
    let trimmed = line.trim_start();
    if line.len() - trimmed.len() > 3 {
        return false;
    }

    let run_len = trimmed.chars().take_while(|ch| *ch == marker).count();
    run_len >= opening_run_len && trimmed[marker.len_utf8() * run_len..].trim().is_empty()
}

fn is_root_comment_start(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("<!--") && line.len() - trimmed.len() <= 3
}

fn is_root_display_math_start(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("$$") && line.len() - trimmed.len() <= 3
}

fn collect_display_math_region(lines: &[&str], start: usize) -> usize {
    let opener = lines[start].trim_start().trim_end();
    if opener != "$$" && opener[2..].contains("$$") {
        return start + 1;
    }

    let mut index = start + 1;
    while index < lines.len() {
        if lines[index].trim() == "$$" {
            return index + 1;
        }
        if lines[index].trim().is_empty() {
            return index;
        }
        index += 1;
    }
    lines.len()
}


pub(crate) use css::{chromium_pdf_theme_css, contains_tibetan_text, css_color, prepare_print_html};
mod css;

#[cfg(test)]
mod tests;
