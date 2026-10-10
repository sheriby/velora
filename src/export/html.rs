//! HTML document generation for Markdown export.
//!
//! 导出管线只保留一层文本预处理（front matter 剥离），其余全部在
//! pulldown-cmark 的事件流上做后处理。曾经的「先按行改写原始 Markdown、再交给
//! pulldown」方案用手工行启发式重新推导块结构（围栏、引用、Setext、代码跨度），
//! 与阅读视图的解析器分叉，是「应用里正常、导出就错」整类 bug 的根因（`$$` 块后
//! 的文字被吞、嵌套引用里的公式导出成源码、Setext 的 `=` 被当成高亮……）。
//! 事件流方案直接复用 pulldown 已算好的块结构：代码跨度与链接目标天然不被误
//! 改写，引用/列表嵌套里的公式不需要新增特例。

pub(super) use std::fs;
pub(super) use std::path::Path;

use std::collections::HashMap;
use std::ops::Range;

pub(super) use base64::{Engine as _, engine::general_purpose};
pub(super) use gpui::{Hsla, Rgba};
pub(super) use pulldown_cmark::{
    BlockQuoteKind, CodeBlockKind, CowStr, Event, HeadingLevel, Options, Parser, Tag, TagEnd, html,
};

use crate::components::markdown::image::{ImageResolvedSource, resolve_image_source};
use crate::components::markdown::inline::looks_like_currency_between;
pub(super) use crate::components::{
    inline_math_font_size, is_mermaid_info_string, parse_display_math_source,
    parse_html_image_block, render_latex_to_svg, render_mermaid_to_svg, sanitize_html_for_export,
};
use crate::file_url::percent_decode_or_raw;
pub(super) use crate::theme::{FontWeightDef, Theme};

/// Builds a full HTML document with embedded theme CSS.
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
    render_html_document(
        markdown,
        theme,
        title,
        base_dir,
        &crate::export::html::css::theme_css(theme),
    )
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

/// 文档开头的 YAML front matter（`---` 围栏对）不进 pulldown：转成 GitHub 风格的
/// key/value 表后从正文剥掉，否则 `---` 会被拆成分隔线/ Setext 下划线、YAML 正文
/// 被拆成段落和列表。关闭围栏只认 `---`，与编辑器导入的判定保持一致。
fn split_front_matter(markdown: &str) -> Option<(String, &str)> {
    let mut lines = markdown.split_inclusive('\n');
    let opening = lines.next()?;
    if opening.trim_end_matches(['\r', '\n']) != "---" {
        return None;
    }
    let mut consumed = opening.len();
    let mut body = String::new();
    for line in lines {
        if line.trim_end_matches(['\r', '\n']) == "---" {
            consumed += line.len();
            return Some((front_matter_table_html(&body), &markdown[consumed..]));
        }
        consumed += line.len();
        body.push_str(line);
    }
    None
}

/// 把 front matter 主体渲染成两列表格；行解析与编辑器属性卡共用
/// [`frontmatter::parse_front_matter_rows`]，保证两边展示同一份数据。
fn front_matter_table_html(body: &str) -> String {
    let rows = crate::components::markdown::frontmatter::parse_front_matter_rows(body);
    if rows.is_empty() {
        return String::new();
    }
    let mut html = String::from("<div class=\"vlt-front-matter\"><table><tbody>\n");
    for (key, values) in &rows {
        html.push_str("<tr><th>");
        html.push_str(&css::escape_html(key));
        html.push_str("</th><td>");
        if values.len() > 1 {
            html.push_str("<ul>");
            for value in values {
                html.push_str("<li>");
                html.push_str(&css::escape_html(value));
                html.push_str("</li>");
            }
            html.push_str("</ul>");
        } else if let Some(value) = values.first() {
            html.push_str(&css::escape_html(value));
        }
        html.push_str("</td></tr>\n");
    }
    html.push_str("</tbody></table></div>\n");
    html
}

fn render_browser_html_body(markdown: &str, theme: &Theme, base_dir: Option<&Path>) -> String {
    let (front_matter_html, body) = match split_front_matter(markdown) {
        Some((table, rest)) => (table, rest),
        None => (String::new(), markdown),
    };
    let events: Vec<(Event<'_>, Range<usize>)> = Parser::new_ext(body, markdown_options())
        .into_offset_iter()
        .collect();
    let anchors = collect_heading_anchors(&events);
    let rewritten = ExportRewriter::new(body, &events, anchors, theme, base_dir).rewrite();
    let mut output = front_matter_html;
    html::push_html(&mut output, rewritten.into_iter());
    output
}

/// GitHub 风格标题 slug 的**唯一**判据：小写、空白转 `-`、丢掉其它标点，`-`/`_` 与
/// CJK/emoji 原样保留，结果为空则没有锚点。
///
/// 导出 HTML 的标题 `id`（本文件）与应用内 Ctrl+点击 `#锚点` 的落点
/// （`src/editor/window_state.rs:heading_line_for_anchor`）都调这一处：两边各抄一份时
/// 迟早漂移，漂了就是「分享出去的目录能跳、应用里点同一行跳不动」（AGENTS.md §1
/// 「一条语法只准有一处判据」）。
pub(crate) fn heading_slug(text: &str) -> Option<String> {
    let slug: String = text
        .trim()
        .to_lowercase()
        .chars()
        .filter_map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                Some(c)
            } else if c.is_whitespace() {
                Some('-')
            } else {
                None
            }
        })
        .collect();
    (!slug.is_empty()).then_some(slug)
}

/// slug 去重规则与 GitHub 相同：第二次出现追加 `-1`、`-2`……
fn unique_heading_slug(slug: Option<String>, seen: &mut HashMap<String, usize>) -> Option<String> {
    let base = slug?;
    let count = seen.entry(base.clone()).or_insert(0usize);
    let result = if *count == 0 {
        base.clone()
    } else {
        format!("{base}-{count}")
    };
    *count += 1;
    Some(result)
}

#[derive(Clone, Debug)]
struct HeadingAnchor {
    level: u8,
    title: String,
    slug: Option<String>,
}

/// 第一遍扫描：按文档顺序收集标题文本与去重后的锚点 id。
/// `[TOC]` 展开、`id=` 注入和内部链接解析共用这张表，保证三者永远一致。
fn collect_heading_anchors(events: &[(Event<'_>, Range<usize>)]) -> Vec<HeadingAnchor> {
    let mut anchors: Vec<HeadingAnchor> = Vec::new();
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut index = 0usize;
    while index < events.len() {
        let level = match &events[index].0 {
            Event::Start(Tag::Heading { level, .. }) => *level as u8,
            _ => {
                index += 1;
                continue;
            }
        };
        index += 1;
        let mut title = String::new();
        while index < events.len() && !matches!(events[index].0, Event::End(TagEnd::Heading(_))) {
            if let Event::Text(text) | Event::Code(text) = &events[index].0 {
                title.push_str(text.as_ref());
            }
            index += 1;
        }
        let slug = unique_heading_slug(heading_slug(&title), &mut seen);
        anchors.push(HeadingAnchor { level, title, slug });
    }
    anchors
}

/// 一条待替换区间：`[global_start, global_end)` 的源文本换成 `html`。
struct Replacement {
    global_start: usize,
    global_end: usize,
    html: String,
}

/// 内联扫描命中的区间（recon 坐标），尚未映射回全局偏移。
struct DetectedSpan {
    start: usize,
    end: usize,
    kind: SpanKind,
}

enum SpanKind {
    /// `$$ ... $$`（可跨行、可跨空行段落）。
    Display,
    /// `$ ... $`（不许跨行）。
    Inline { latex: String },
    /// `\( ... \)`（不许跨行）。
    Paren { latex: String },
    HighlightOpen,
    HighlightClose,
    Script { tag: &'static str, body: String },
}

/// 段落的「重建原文」：把一段里叶子事件的原始切片按全局偏移拼起来。
/// pulldown 事件的 range 直接指回原文，所以公式体、转义反斜杠、HTML 实体都
/// 按原始字节处理，不需要在事件流之外再推导任何块结构。
struct Recon {
    text: String,
    /// `positions[i]` = `text[i]` 这个字节在原文里的全局偏移。
    positions: Vec<usize>,
}

impl Recon {
    fn is_blank(&self) -> bool {
        self.text.trim().is_empty()
    }

    fn global_start(&self, index: usize) -> Option<usize> {
        self.positions.get(index).copied()
    }

    fn global_end(&self, end_index: usize) -> Option<usize> {
        if end_index == 0 {
            return None;
        }
        self.positions
            .get(end_index - 1)
            .map(|position| position + 1)
    }
}

/// `group` 必须包含配对的 Start/End 事件：Start 的 range.start 是段首锚点，
/// 用来接住「段落第一个字符是被转义标记的前导反斜杠」这种情况。
fn build_recon(body: &str, group: &[(Event<'_>, Range<usize>)]) -> Recon {
    // 逐字节累计再一次性转 UTF-8：拼接段都来自合法文本，lossy 只是保险。
    let mut bytes = Vec::new();
    let mut positions: Vec<usize> = Vec::new();
    let mut last_end: Option<usize> = group.first().map(|(_, range)| range.start);

    for (event, range) in group.iter().skip(1) {
        match event {
            Event::Text(_)
            | Event::FootnoteReference(_)
            | Event::SoftBreak
            | Event::HardBreak => {
                push_recon_gap(body, &mut bytes, &mut positions, last_end, range.start);
                if let Some(slice) = body.get(range.start..range.end) {
                    push_recon_slice(&mut bytes, &mut positions, slice, range.start);
                }
                last_end = Some(range.end);
            }
            Event::Code(_) | Event::InlineHtml(_) => {
                // 代码跨度与内联 HTML 是公式/高亮扫描的屏障：内容不进 recon，
                // 只推进游标。这正是「代码里的 `$` 不再被当公式」的实现点。
                push_recon_gap(body, &mut bytes, &mut positions, last_end, range.start);
                last_end = Some(range.end);
            }
            _ => {}
        }
    }

    Recon {
        text: String::from_utf8_lossy(&bytes).into_owned(),
        positions,
    }
}

fn push_recon_gap(
    body: &str,
    bytes: &mut Vec<u8>,
    positions: &mut Vec<usize>,
    from: Option<usize>,
    to: usize,
) {
    let Some(start) = from else { return };
    if to <= start {
        return;
    }
    let Some(gap) = body.get(start..to) else {
        return;
    };
    // 只有「转义反斜杠」和「跨段合并时的空行」这两个 gap 是正文的一部分；
    // 其余 gap（`> ` 引用标记、`**` 强调围栏）是块结构，跳过。
    let keep = gap == "\\" || gap.chars().all(|ch| ch == '\n' || ch == '\r');
    if keep {
        push_recon_slice(bytes, positions, gap, start);
    }
}

fn push_recon_slice(bytes: &mut Vec<u8>, positions: &mut Vec<usize>, slice: &str, global_start: usize) {
    for (offset, byte) in slice.bytes().enumerate() {
        bytes.push(byte);
        positions.push(global_start + offset);
    }
}

fn detect_inline_spans(text: &str) -> Vec<DetectedSpan> {
    let mut spans: Vec<DetectedSpan> = Vec::new();
    let mut index = 0usize;
    // (开标签在 spans 里的下标, 收尾 `==` 的偏移)
    let mut highlight: Option<(usize, usize)> = None;

    while index < text.len() {
        if let Some((_, close)) = highlight
            && close == index
        {
            spans.push(DetectedSpan {
                start: index,
                end: index + 2,
                kind: SpanKind::HighlightClose,
            });
            highlight = None;
            index += 2;
            continue;
        }

        if text[index..].starts_with("$$") && !is_escaped_ascii(text, index) {
            if let Some(relative) = text[index + 2..].find("$$") {
                let end = index + 4 + relative;
                let span = DetectedSpan {
                    start: index,
                    end,
                    kind: SpanKind::Display,
                };
                invalidate_dangling_highlight(&mut spans, &mut highlight, &span);
                spans.push(span);
                index = end;
                continue;
            }
        }

        if highlight.is_none()
            && text[index..].starts_with("==")
            && !is_escaped_ascii(text, index)
            && let Some(close) = locate_highlight_close(text, index)
        {
            spans.push(DetectedSpan {
                start: index,
                end: index + 2,
                kind: SpanKind::HighlightOpen,
            });
            highlight = Some((spans.len() - 1, close));
            index += 2;
            continue;
        }

        if let Some((end, body)) = locate_inline_dollar_math_source(text, index)
            .or_else(|| locate_inline_paren_math_source(text, index))
        {
            let kind = if text[index..].starts_with('\\') {
                SpanKind::Paren { latex: body }
            } else {
                SpanKind::Inline { latex: body }
            };
            let span = DetectedSpan {
                start: index,
                end,
                kind,
            };
            invalidate_dangling_highlight(&mut spans, &mut highlight, &span);
            spans.push(span);
            index = end;
            continue;
        }

        if let Some((end, body, tag)) = locate_inline_script_source(text, index) {
            let span = DetectedSpan {
                start: index,
                end,
                kind: SpanKind::Script { tag, body },
            };
            invalidate_dangling_highlight(&mut spans, &mut highlight, &span);
            spans.push(span);
            index = end;
            continue;
        }

        match text[index..].chars().next() {
            Some(ch) => index += ch.len_utf8(),
            None => break,
        }
    }

    if let Some((open_index, _)) = highlight {
        invalidate_span(&mut spans, open_index);
    }
    spans
}

/// 其它 span 若吞掉了高亮的收尾 `==`，那对 `<mark>` 作废，避免导出漏出
/// 未配对的标签。作废用越界偏移标记，materialize 自然跳过。
fn invalidate_dangling_highlight(
    spans: &mut [DetectedSpan],
    highlight: &mut Option<(usize, usize)>,
    span: &DetectedSpan,
) {
    let Some((open_index, close)) = *highlight else {
        return;
    };
    if span.start < close && span.end > close {
        invalidate_span(spans, open_index);
        *highlight = None;
    }
}

fn invalidate_span(spans: &mut [DetectedSpan], index: usize) {
    if let Some(span) = spans.get_mut(index) {
        span.start = usize::MAX;
        span.end = usize::MAX;
    }
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
        if line[cursor..].starts_with('\n') {
            return None;
        }
        if line[cursor..].starts_with('$')
            && !line[cursor..].starts_with("$$")
            && !is_escaped_ascii(line, cursor)
        {
            let body = &line[index + 1..cursor];
            // 钱/公式的判据只有一处（`inline::parse::looks_like_currency_between`），
            // 这里只负责从重建原文上取出定界符两侧的字符。闭合 `$` 是 1 字节，所以
            // `cursor + 1` 必是字符边界；仍用 `get` 取值，边界由类型保证而不是靠运气。
            if valid_inline_math_body(body)
                && !looks_like_currency_between(
                    line.get(..index).and_then(|head| head.chars().next_back()),
                    line.get(cursor + 1..).and_then(|tail| tail.chars().next()),
                    body,
                )
            {
                return Some((cursor + 1, body.to_string()));
            }
            return None;
        }
        cursor += line[cursor..].chars().next()?.len_utf8();
    }
    None
}

fn locate_inline_paren_math_source(line: &str, index: usize) -> Option<(usize, String)> {
    // `\(...\)` 是无歧义的 TeX 定界符：这里**不许**过问钱/公式判据
    // （`inline::parse::looks_like_currency_between` 只服务 `$…$`），否则 `\(42\)`
    // 会被当成钱数原样导出（cases/07-numeric-math.md，与阅读视图同一口径）。
    if !line[index..].starts_with("\\(") {
        return None;
    }
    let mut cursor = index + 2;
    while cursor + 1 < line.len() {
        if line[cursor..].starts_with('\n') {
            return None;
        }
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

/// 从 `index` 处那对 `==` 往后找收尾，返回收尾那两位的起始下标。中间为空（`====`）
/// 不算高亮，按字面写法定住；跨行不算配对（与旧的按行扫描行为一致）。
fn locate_highlight_close(line: &str, index: usize) -> Option<usize> {
    let mut cursor = index + 2;
    while cursor < line.len() {
        if line[cursor..].starts_with('\n') {
            return None;
        }
        if cursor > index + 2
            && line[cursor..].starts_with("==")
            && !is_escaped_ascii(line, cursor)
        {
            return Some(cursor);
        }
        cursor += line[cursor..].chars().next()?.len_utf8();
    }
    None
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
        if line[cursor..].starts_with('\n') {
            return None;
        }
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
        && line
            .get(index + 1..)
            .and_then(|rest| rest.chars().next())
            .is_none_or(|ch| ch != '~')
}

fn valid_inline_math_body(body: &str) -> bool {
    !body.is_empty() && !body.contains(['\n', '\r']) && body.trim() == body
}

fn is_escaped_ascii(line: &str, index: usize) -> bool {
    let mut slash_count = 0usize;
    let mut cursor = index;
    while cursor > 0 && line.as_bytes().get(cursor - 1) == Some(&b'\\') {
        slash_count += 1;
        cursor -= 1;
    }
    slash_count % 2 == 1
}

/// 事件流分发描述：先按不可变借用取出需要的信息，再调用会改 self 的方法，
/// 避免 match  scrutinee 借用与 &mut self 冲突。
enum Dispatch {
    Mermaid,
    BlockQuote(Option<BlockQuoteKind>),
    Paragraph,
    Heading(HeadingLevel),
    TableCell,
    ListItem,
    HtmlBlock,
    Pass,
}

struct ExportRewriter<'a> {
    body: &'a str,
    events: &'a [(Event<'a>, Range<usize>)],
    anchors: Vec<HeadingAnchor>,
    anchor_lookup: HashMap<String, String>,
    anchor_cursor: usize,
    theme: &'a Theme,
    base_dir: Option<&'a Path>,
}

impl<'a> ExportRewriter<'a> {
    fn new(
        body: &'a str,
        events: &'a [(Event<'a>, Range<usize>)],
        anchors: Vec<HeadingAnchor>,
        theme: &'a Theme,
        base_dir: Option<&'a Path>,
    ) -> Self {
        let mut anchor_lookup = HashMap::new();
        for anchor in &anchors {
            if let Some(slug) = &anchor.slug {
                anchor_lookup.insert(slug.to_lowercase(), slug.clone());
                anchor_lookup.insert(percent_decode_or_raw(slug).to_lowercase(), slug.clone());
            }
        }
        Self {
            body,
            events,
            anchors,
            anchor_lookup,
            anchor_cursor: 0,
            theme,
            base_dir,
        }
    }

    fn rewrite(mut self) -> Vec<Event<'a>> {
        let mut output = Vec::new();
        let mut index = 0usize;
        while index < self.events.len() {
            let dispatch = match &self.events[index].0 {
                Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info)))
                    if is_mermaid_info_string(Some(info.as_ref())) =>
                {
                    Dispatch::Mermaid
                }
                Event::Start(Tag::BlockQuote(kind)) => Dispatch::BlockQuote(*kind),
                Event::Start(Tag::Paragraph) => Dispatch::Paragraph,
                Event::Start(Tag::Heading { level, .. }) => Dispatch::Heading(*level),
                Event::Start(Tag::TableCell) => Dispatch::TableCell,
                Event::Start(Tag::Item) => Dispatch::ListItem,
                Event::Start(Tag::HtmlBlock) => Dispatch::HtmlBlock,
                _ => Dispatch::Pass,
            };
            let consumed = match dispatch {
                Dispatch::Mermaid => self.rewrite_mermaid_block(index, &mut output),
                Dispatch::BlockQuote(kind) => self.rewrite_blockquote(index, kind, &mut output),
                Dispatch::Paragraph => self.rewrite_paragraph(index, &mut output),
                Dispatch::Heading(level) => self.rewrite_heading(index, level, &mut output),
                Dispatch::TableCell => self.rewrite_table_cell(index, &mut output),
                Dispatch::ListItem => self.rewrite_item(index, &mut output),
                Dispatch::HtmlBlock => self.rewrite_html_block(index, &mut output),
                Dispatch::Pass => {
                    output.extend(self.transform_event(self.events[index].0.clone()));
                    1
                }
            };
            index += consumed.max(1);
        }
        output
    }

    /// 找到与 `start` 处 Start 事件配对的 End 之后的下标（含两端）。
    /// 段落/标题/表格单元不允许嵌套同类块，线性扫描即可。
    fn group_end(&self, start: usize, is_end: impl Fn(&Event<'a>) -> bool) -> usize {
        let mut index = start + 1;
        while index < self.events.len() {
            if is_end(&self.events[index].0) {
                return index + 1;
            }
            index += 1;
        }
        self.events.len()
    }

    fn transform_event(&self, event: Event<'a>) -> Vec<Event<'a>> {
        match event {
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                title,
                id,
            }) => {
                let dest_url = self.rewrite_internal_anchor(dest_url);
                vec![Event::Start(Tag::Link {
                    link_type,
                    dest_url,
                    title,
                    id,
                })]
            }
            Event::Start(Tag::Image {
                link_type,
                dest_url,
                title,
                id,
            }) => {
                let dest_url = local_image_data_uri(dest_url.as_ref(), self.base_dir)
                    .map(CowStr::from)
                    .unwrap_or(dest_url);
                vec![Event::Start(Tag::Image {
                    link_type,
                    dest_url,
                    title,
                    id,
                })]
            }
            Event::Html(content) | Event::InlineHtml(content) => {
                let sanitized = self.sanitize_raw_html(content.as_ref());
                if sanitized.is_empty() {
                    return Vec::new();
                }
                vec![Event::Html(CowStr::from(sanitized))]
            }
            event => vec![event],
        }
    }

    /// 原始 HTML 一律进事件流前净化：注释转可见块、`<img>` 内联本地文件、
    /// 其余走与阅读视图一致的 sanitizer。在事件层处理天然覆盖列表项、引用块、
    /// 表格单元里的 HTML——旧的「只在根级按行改写」做不到这点。
    fn sanitize_raw_html(&self, content: &str) -> String {
        let trimmed = content.trim();
        if trimmed.is_empty() {
            return String::new();
        }
        if trimmed.starts_with("<!--") {
            return format!(
                "<pre class=\"vlt-comment\">{}</pre>",
                css::escape_html(trimmed)
            );
        }
        if let Some(image) = parse_html_image_block(trimmed) {
            let src = local_image_data_uri(&image.src, self.base_dir)
                .unwrap_or_else(|| image.src.clone());
            return image.to_sanitized_html_with_src(&src);
        }
        sanitize_html_for_export(trimmed)
    }

    fn rewrite_internal_anchor(&self, dest_url: CowStr<'a>) -> CowStr<'a> {
        let Some(fragment) = dest_url.strip_prefix('#') else {
            return dest_url;
        };
        let decoded = percent_decode_or_raw(fragment);
        let canonical = self
            .anchor_lookup
            .get(&decoded.to_lowercase())
            .or_else(|| self.anchor_lookup.get(&fragment.to_lowercase()));
        match canonical {
            Some(slug) if slug != fragment => CowStr::from(format!("#{slug}")),
            _ => dest_url,
        }
    }

    fn rewrite_mermaid_block(&self, start: usize, output: &mut Vec<Event<'a>>) -> usize {
        let end = self.group_end(start, |event| matches!(event, Event::End(TagEnd::CodeBlock)));
        let info = match &self.events[start].0 {
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info))) => {
                info.as_ref().trim().to_string()
            }
            _ => return 1,
        };
        let mut content_start = usize::MAX;
        let mut content_end = 0usize;
        for (event, range) in &self.events[start..end] {
            if matches!(event, Event::Text(_)) {
                content_start = content_start.min(range.start);
                content_end = content_end.max(range.end);
            }
        }
        let content = if content_start <= content_end {
            self.body.get(content_start..content_end).unwrap_or_default()
        } else {
            ""
        };
        let body = content.trim_matches(|ch| ch == '\n' || ch == '\r');
        match render_mermaid_to_svg(body) {
            Ok(svg) => {
                let src = data_uri_for_bytes("image/svg+xml", svg.as_bytes());
                output.push(Event::Html(CowStr::from(format!(
                    "<div class=\"vlt-mermaid\"><img alt=\"Mermaid diagram\" src=\"{src}\"></div>"
                ))));
            }
            Err(error) => {
                // 渲染失败必须可见：给出转义后的原始围栏；原因进日志。
                let raw = format!("```{info}\n{body}\n```");
                eprintln!("export: mermaid 渲染失败: {error}");
                output.push(Event::Html(CowStr::from(format!(
                    "<pre class=\"vlt-mermaid-error\">{}</pre>",
                    css::escape_html(&raw)
                ))));
            }
        }
        end - start
    }

    fn rewrite_blockquote(
        &mut self,
        start: usize,
        kind: Option<BlockQuoteKind>,
        output: &mut Vec<Event<'a>>,
    ) -> usize {
        let Some(alert_kind) = kind else {
            return self.rewrite_alert_blockquote(start, output);
        };
        // pulldown 原生告警（`[!NOTE]` 后紧跟换行）：补一个 GitHub 式标题段，
        // 与自定义标题路径的输出形状一致。
        output.push(Event::Start(Tag::BlockQuote(Some(alert_kind))));
        output.push(Event::Html(CowStr::from(format!(
            "<p class=\"markdown-alert-title\">{}</p>",
            css::escape_html(alert_title(alert_kind))
        ))));
        1
    }

    /// 带自定义标题的 GitHub 告警（`> [!NOTE] 标题`）。pulldown 0.13 的扫描器
    /// 要求 `]` 后必须立刻换行，`[!NOTE] 标题` 会退化成普通引用块并漏出字面
    /// `[!NOTE]`；这里在事件流上补齐：改写 BlockQuote 起始标签，把首段劈成
    /// 「标题段 + 正文段」。
    fn rewrite_alert_blockquote(&mut self, start: usize, output: &mut Vec<Event<'a>>) -> usize {
        let next = start + 1;
        let Some(Event::Start(Tag::Paragraph)) = self.events.get(next).map(|(event, _)| event)
        else {
            output.push(self.events[start].0.clone());
            return 1;
        };
        let paragraph_end =
            self.group_end(next, |event| matches!(event, Event::End(TagEnd::Paragraph)));
        let group = &self.events[next..paragraph_end];
        let recon = build_recon(self.body, group);
        let Some((alert_kind, marker_end)) = parse_alert_marker(&recon.text) else {
            output.push(self.events[start].0.clone());
            return 1;
        };

        output.push(Event::Start(Tag::BlockQuote(Some(alert_kind))));

        let spans = detect_inline_spans(&recon.text);
        let replacements = self.materialize_spans_all(&spans, &recon);
        let inner = group_inner(group);

        let title_end_recon = recon
            .text
            .get(marker_end..)
            .and_then(|rest| rest.find('\n'))
            .map(|offset| marker_end + offset)
            .unwrap_or(recon.text.len());
        let title_text = recon
            .text
            .get(marker_end..title_end_recon)
            .unwrap_or_default()
            .trim();

        if title_text.is_empty() {
            output.push(Event::Html(CowStr::from(format!(
                "<p class=\"markdown-alert-title\">{}</p>",
                css::escape_html(alert_title(alert_kind))
            ))));
        } else {
            let title_start = trim_start_recon(&recon.text, marker_end, title_end_recon);
            let title_floor = recon.global_start(title_start).unwrap_or(0);
            let title_end_trimmed = trim_end_recon(&recon.text, marker_end, title_end_recon);
            let title_ceiling = recon
                .global_end(title_end_trimmed)
                .unwrap_or(title_floor + 1);
            output.push(Event::Html(CowStr::from(
                "<p class=\"markdown-alert-title\">",
            )));
            output.extend(self.walk_group(inner, &replacements, title_floor, title_ceiling));
            output.push(Event::Html(CowStr::from("</p>")));
        }

        let body_start_recon = if recon.text.get(title_end_recon..title_end_recon + 1) == Some("\n") {
            title_end_recon + 1
        } else {
            title_end_recon
        };
        let body_rest = recon.text.get(body_start_recon..).unwrap_or_default();
        if !body_rest.trim().is_empty() {
            let body_floor = recon.global_start(body_start_recon).unwrap_or(0);
            let body_ceiling = inner
                .iter()
                .map(|(_, range)| range.end)
                .max()
                .unwrap_or(self.body.len());
            output.push(Event::Html(CowStr::from("<p>")));
            output.extend(self.walk_group(inner, &replacements, body_floor, body_ceiling));
            output.push(Event::Html(CowStr::from("</p>")));
        }

        paragraph_end - start
    }

    fn rewrite_paragraph(&mut self, start: usize, output: &mut Vec<Event<'a>>) -> usize {
        let end = self.group_end(start, |event| matches!(event, Event::End(TagEnd::Paragraph)));
        self.process_content_group(start, end, output, true)
    }

    /// 紧凑列表项的内容直接挂在 `Event::Start(Tag::Item)` 下（pulldown 不为紧凑
    /// 项发 Paragraph 事件），不按段落处理的话公式/高亮在 `- item $x$` 里会失效。
    fn rewrite_item(&mut self, start: usize, output: &mut Vec<Event<'a>>) -> usize {
        let end = self.group_end(start, |event| matches!(event, Event::End(TagEnd::Item)));
        let has_paragraph = self.events[start + 1..end]
            .iter()
            .any(|(event, _)| matches!(event, Event::Start(Tag::Paragraph)));
        if has_paragraph {
            // 宽松列表：子段落会各自走 Paragraph 分发，Item 标签原样透传。
            output.push(self.events[start].0.clone());
            return 1;
        }
        self.process_content_group(start, end, output, false)
    }

    /// HTML 块在 pulldown 里是「每行一个 `Event::Html`」；必须整块合并后再净化，
    /// 否则多行注释会被逐行当成多个块。
    fn rewrite_html_block(&self, start: usize, output: &mut Vec<Event<'a>>) -> usize {
        let end = self.group_end(start, |event| matches!(event, Event::End(TagEnd::HtmlBlock)));
        let mut content = String::new();
        for (event, _) in &self.events[start..end] {
            if let Event::Html(part) = event {
                content.push_str(part.as_ref());
            }
        }
        output.push(self.events[start].0.clone());
        let sanitized = self.sanitize_raw_html(&content);
        if !sanitized.is_empty() {
            output.push(Event::Html(CowStr::from(sanitized)));
        }
        if let Some((event, _)) = self.events.get(end.saturating_sub(1))
            && matches!(event, Event::End(TagEnd::HtmlBlock))
        {
            output.push(event.clone());
        }
        end - start
    }

    /// 段落/紧凑列表项的共同处理：TOC 展开（仅段落）、跨空行块级公式合并（仅段落）、
    /// 段首块级公式出 `<div>`、行内替换（公式/高亮/上下标）。
    fn process_content_group(
        &mut self,
        start: usize,
        end: usize,
        output: &mut Vec<Event<'a>>,
        is_paragraph: bool,
    ) -> usize {
        let group = &self.events[start..end];
        let recon = build_recon(self.body, group);

        if recon.is_blank() {
            output.extend(group.iter().map(|(event, _)| event.clone()));
            return end - start;
        }
        if is_paragraph && recon.text.trim() == "[TOC]" {
            output.extend(self.toc_events());
            return end - start;
        }

        let spans = detect_inline_spans(&recon.text);
        let display_spans: Vec<&DetectedSpan> = spans
            .iter()
            .filter(|span| matches!(span.kind, SpanKind::Display))
            .collect();

        // 段首 `$$` 在本段没闭合：向后并段直到找到闭合的 `$$`（Typora 粘贴里
        // 块中留空行的写法）。失败则按普通段落继续，绝不吞内容。
        if is_paragraph
            && recon.text.trim_start().starts_with("$$")
            && display_spans.is_empty()
            && let Some(merged_end) = self.merge_display_paragraphs(start, end)
        {
            self.emit_merged_display(start, merged_end, output);
            return merged_end - start;
        }

        // 段首被完整块级公式占据：公式出 `<div>`，剩余文字单独成段（内容不丢）。
        if let Some((prefix_count, cursor)) = Self::leading_display_prefix(&display_spans, &recon) {
            let remainder = recon.text.get(cursor..).unwrap_or_default();
            let has_remainder = !remainder.trim().is_empty();
            if !is_paragraph {
                // 紧凑列表项里块级公式仍要包在 <li> 内，否则 `<ul>` 直接套 `<div>`。
                output.push(group[0].0.clone());
            }
            for span in display_spans.iter().take(prefix_count) {
                self.push_display_block(span, &recon, output);
            }
            if has_remainder {
                let floor = recon.global_end(cursor).unwrap_or(0);
                let inner = group_inner(group);
                let ceiling = inner
                    .iter()
                    .map(|(_, range)| range.end)
                    .max()
                    .unwrap_or(self.body.len());
                let tail_spans: Vec<&DetectedSpan> =
                    spans.iter().filter(|span| span.start >= cursor).collect();
                let replacements = self.materialize_spans(&tail_spans, &recon);
                if is_paragraph {
                    output.push(Event::Html(CowStr::from("<p>")));
                }
                output.extend(self.walk_group(inner, &replacements, floor, ceiling));
                if is_paragraph {
                    output.push(Event::Html(CowStr::from("</p>")));
                }
            }
            if !is_paragraph {
                output.push(group[group.len() - 1].0.clone());
            }
            return end - start;
        }

        let replacements = self.materialize_spans_all(&spans, &recon);
        output.extend(self.walk_group(group, &replacements, 0, self.body.len()));
        end - start
    }

    /// 段首是否被连续块级公式占据；返回 (公式个数, 剩余内容起始 recon 偏移)。
    fn leading_display_prefix(
        display_spans: &[&DetectedSpan],
        recon: &Recon,
    ) -> Option<(usize, usize)> {
        let first = *display_spans.first()?;
        if !recon.text.get(..first.start)?.trim().is_empty() {
            return None;
        }
        let mut count = 0usize;
        let mut cursor = 0usize;
        for span in display_spans {
            if span.start < cursor {
                break;
            }
            if !recon.text.get(cursor..span.start)?.trim().is_empty() {
                break;
            }
            cursor = span.end;
            count += 1;
        }
        Some((count, cursor))
    }

    fn push_display_block(&self, span: &DetectedSpan, recon: &Recon, output: &mut Vec<Event<'a>>) {
        let raw = recon
            .text
            .get(span.start..span.end)
            .unwrap_or_default()
            .to_string();
        match parse_display_math_source(&raw) {
            Some(source) => {
                match render_latex_to_svg(
                    &source.body,
                    self.theme.colors.text_default,
                    self.theme.typography.text_size,
                    crate::components::latex::MathLayout::Display,
                ) {
                    Ok(svg) => output.push(Event::Html(CowStr::from(format!(
                        "<div class=\"vlt-math\">{svg}</div>"
                    )))),
                    Err(error) => {
                        eprintln!("export: 公式渲染失败: {error}");
                        output.push(Event::Html(CowStr::from(format!(
                            "<pre class=\"vlt-math-error\">{}</pre>",
                            css::escape_html(&raw)
                        ))));
                    }
                }
            }
            None => output.push(Event::Html(CowStr::from(css::escape_html(&raw)))),
        }
    }

    /// `$$` 开了没关、且后面紧跟纯段落时，向后合并重建直到闭合。
    fn merge_display_paragraphs(&self, start: usize, first_end: usize) -> Option<usize> {
        let mut merged_end = first_end;
        loop {
            let slice = self.events.get(start..merged_end)?;
            let recon = build_recon(self.body, slice);
            let opener = recon.text.find("$$")?;
            if recon.text.get(opener + 2..)?.contains("$$") {
                return Some(merged_end);
            }

            // 下一事件必须是紧跟着的纯段落，否则放弃合并、按原样渲染（不误伤）。
            if !matches!(
                self.events.get(merged_end).map(|(event, _)| event),
                Some(Event::Start(Tag::Paragraph))
            ) {
                return None;
            }
            let next_end =
                self.group_end(merged_end, |event| matches!(event, Event::End(TagEnd::Paragraph)));
            if next_end <= merged_end {
                return None;
            }
            merged_end = next_end;
        }
    }

    /// 把 `start..merged_end` 的多个段落当成一块重建：块级公式出 `<div>`，
    /// 闭合 `$$` 之后还有内容的，剩余部分单独成段。
    fn emit_merged_display(&self, start: usize, merged_end: usize, output: &mut Vec<Event<'a>>) {
        let Some(slice) = self.events.get(start..merged_end) else {
            return;
        };
        let recon = build_recon(self.body, slice);
        let Some(opener) = recon.text.find("$$") else {
            return;
        };
        let Some(relative) = recon.text.get(opener + 2..).and_then(|rest| rest.find("$$")) else {
            return;
        };
        let span_end = opener + 4 + relative;
        let span = DetectedSpan {
            start: opener,
            end: span_end,
            kind: SpanKind::Display,
        };
        self.push_display_block(&span, &recon, output);
        if recon.text.get(span_end..).unwrap_or_default().trim().is_empty() {
            return;
        }
        let floor = recon.global_end(span_end).unwrap_or(self.body.len());
        let inner: Vec<(Event<'a>, Range<usize>)> = slice
            .iter()
            .filter(|(event, _)| {
                !matches!(
                    event,
                    Event::Start(Tag::Paragraph) | Event::End(TagEnd::Paragraph)
                )
            })
            .cloned()
            .collect();
        let ceiling = inner
            .iter()
            .map(|(_, range)| range.end)
            .max()
            .unwrap_or(self.body.len());
        let tail_spans: Vec<DetectedSpan> =
            detect_inline_spans(recon.text.get(span_end..).unwrap_or_default())
                .into_iter()
                .map(|local| DetectedSpan {
                    start: local.start + span_end,
                    end: local.end + span_end,
                    kind: local.kind,
                })
                .collect();
        let replacements = self.materialize_span_list(&tail_spans, &recon);
        output.push(Event::Html(CowStr::from("<p>")));
        output.extend(self.walk_group(&inner, &replacements, floor, ceiling));
        output.push(Event::Html(CowStr::from("</p>")));
    }

    fn rewrite_heading(
        &mut self,
        start: usize,
        level: HeadingLevel,
        output: &mut Vec<Event<'a>>,
    ) -> usize {
        let end = self.group_end(start, |event| matches!(event, Event::End(TagEnd::Heading(_))));
        let group = &self.events[start..end];
        let inner = group_inner(group);
        let recon = build_recon(self.body, group);
        let anchor = self.anchors.get(self.anchor_cursor).cloned();
        self.anchor_cursor += 1;
        let level_number = level as u8;

        let open_tag = match anchor.as_ref().and_then(|entry| entry.slug.as_deref()) {
            Some(slug) => format!("<h{level_number} id=\"{}\">", css::escape_html(slug)),
            None => format!("<h{level_number}>"),
        };
        output.push(Event::Html(CowStr::from(open_tag)));

        let spans = detect_inline_spans(&recon.text);
        let replacements = self.materialize_spans_all(&spans, &recon);
        let ceiling = inner
            .iter()
            .map(|(_, range)| range.end)
            .max()
            .unwrap_or(self.body.len());
        output.extend(self.walk_group(inner, &replacements, 0, ceiling));
        output.push(Event::Html(CowStr::from(format!("</h{level_number}>"))));
        end - start
    }

    fn rewrite_table_cell(&mut self, start: usize, output: &mut Vec<Event<'a>>) -> usize {
        let end = self.group_end(start, |event| matches!(event, Event::End(TagEnd::TableCell)));
        let group = &self.events[start..end];
        let recon = build_recon(self.body, group);
        let spans = detect_inline_spans(&recon.text);
        let replacements = self.materialize_spans_all(&spans, &recon);
        output.extend(self.walk_group(group, &replacements, 0, self.body.len()));
        end - start
    }

    fn materialize_spans_all(
        &self,
        spans: &[DetectedSpan],
        recon: &Recon,
    ) -> Vec<Replacement> {
        self.materialize_span_list(spans, recon)
    }

    fn materialize_spans(
        &self,
        spans: &[&DetectedSpan],
        recon: &Recon,
    ) -> Vec<Replacement> {
        spans
            .iter()
            .copied()
            .filter_map(|span| self.materialize_span(span, recon))
            .collect()
    }

    fn materialize_span_list(
        &self,
        spans: &[DetectedSpan],
        recon: &Recon,
    ) -> Vec<Replacement> {
        spans
            .iter()
            .filter_map(|span| self.materialize_span(span, recon))
            .collect()
    }

    /// 公式体 → 内联 svg 包装；渲染失败退回转义字面量（与阅读视图的失败可见一致）。
    fn render_math_html(
        &self,
        latex: &str,
        layout: crate::components::latex::MathLayout,
        font_size: f32,
        fallback_raw: &str,
    ) -> String {
        match render_latex_to_svg(latex, self.theme.colors.text_default, font_size, layout) {
            Ok(svg) => format!("<span class=\"vlt-inline-math\">{svg}</span>"),
            Err(_) => css::escape_html(fallback_raw),
        }
    }

    /// 把 recon 坐标的 span 换成全局坐标 + 渲染好的 HTML。渲染失败退回转义字面量。
    fn materialize_span(&self, span: &DetectedSpan, recon: &Recon) -> Option<Replacement> {
        let global_start = recon.global_start(span.start)?;
        let global_end = recon.global_end(span.end)?;
        let html = match &span.kind {
            SpanKind::Display => {
                let raw = recon.text.get(span.start..span.end)?;
                let source = parse_display_math_source(raw)?;
                self.render_math_html(
                    &source.body,
                    crate::components::latex::MathLayout::Display,
                    self.theme.typography.text_size,
                    raw,
                )
            }
            SpanKind::Inline { latex } | SpanKind::Paren { latex } => {
                let raw = recon.text.get(span.start..span.end).unwrap_or_default();
                self.render_math_html(
                    latex,
                    crate::components::latex::MathLayout::Inline,
                    inline_math_font_size(self.theme.typography.text_size),
                    raw,
                )
            }
            SpanKind::HighlightOpen => "<mark>".to_string(),
            SpanKind::HighlightClose => "</mark>".to_string(),
            SpanKind::Script { tag, body } => {
                format!("<{tag}>{}</{tag}>", css::escape_html(body))
            }
        };
        Some(Replacement {
            global_start,
            global_end,
            html,
        })
    }

    /// 按替换表重建一组事件：被替换覆盖的叶子整段丢弃，Text 叶子在边界处切片。
    /// Text 事件的内容与原文切片只在 HTML 实体处不一致（`&amp;` → `&`），而实体
    /// 内部不含公式/高亮标记，因此只在长度相等的叶子上切片。
    fn walk_group(
        &self,
        events: &[(Event<'a>, Range<usize>)],
        replacements: &[Replacement],
        floor: usize,
        ceiling: usize,
    ) -> Vec<Event<'a>> {
        let mut output = Vec::new();
        // 上一个替换消费到的全局偏移；用来跳过「起点落在替换区间内部」的叶子
        // （块级公式跨 SoftBreak 时，下一行的行首就在区间里）。
        let mut consumed_until = floor;
        for (event, range) in events {
            let (raw_start, raw_end) = (range.start, range.end);
            if raw_end <= floor || raw_start >= ceiling {
                continue;
            }
            let is_tag = matches!(event, Event::Start(_) | Event::End(_));
            // 已被替换吃掉的游标只对叶子生效；标签事件的 range 是包裹区间，
            // 若按游标跳过会连 `</td>`、`</li>` 一起吞掉。
            if !is_tag && raw_end <= consumed_until {
                continue;
            }
            if is_tag {
                // 标签事件的 range 是「包裹区间」，只有被严格包住才丢弃；
                // 与替换等长的根标签（如整格公式的 <td>）必须保留。
                let covering_tag = replacements
                    .iter()
                    .find(|item| item.global_start < raw_start && raw_end < item.global_end);
                if let Some(item) = covering_tag {
                    consumed_until = consumed_until.max(item.global_end);
                    continue;
                }
                output.extend(self.transform_event(event.clone()));
                continue;
            }
            if let Event::Text(content) = event {
                let start = raw_start.max(consumed_until).max(floor);
                let end = raw_end.min(ceiling);
                if start >= end {
                    continue;
                }
                if content.len() != raw_end - raw_start {
                    // HTML 实体事件：替换边界不会落进它内部（实体名不含公式标记）；
                    // 被整个包住就丢，否则整段保留。
                    if let Some(item) = replacements
                        .iter()
                        .find(|item| item.global_start <= raw_start && raw_end <= item.global_end)
                    {
                        consumed_until = consumed_until.max(item.global_end);
                    } else {
                        output.push(event.clone());
                    }
                    continue;
                }
                let mut cursor = start;
                for item in replacements
                    .iter()
                    .filter(|item| item.global_end > start && item.global_start < end)
                {
                    if item.global_end <= cursor {
                        continue;
                    }
                    if item.global_start > cursor
                        && let Some(slice) = self.body.get(cursor..item.global_start)
                    {
                        output.push(Event::Text(CowStr::Borrowed(slice)));
                    }
                    output.push(Event::Html(CowStr::from(item.html.clone())));
                    consumed_until = consumed_until.max(item.global_end);
                    cursor = item.global_end.max(cursor).min(end);
                }
                if cursor < end
                    && let Some(slice) = self.body.get(cursor..end)
                    && !slice.is_empty()
                {
                    output.push(Event::Text(CowStr::Borrowed(slice)));
                }
                continue;
            }
            // 非 Text 叶子（Code/SoftBreak/FootnoteReference 等）：整段被包住才丢，
            // 否则整段保留。Code 屏障保证替换 span 不会落进代码跨度内部。
            if let Some(item) = replacements
                .iter()
                .find(|item| item.global_start <= raw_start && raw_end <= item.global_end)
            {
                consumed_until = consumed_until.max(item.global_end);
                continue;
            }
            output.extend(self.transform_event(event.clone()));
        }
        output
    }

    fn toc_events(&self) -> Vec<Event<'a>> {
        let mut markup = String::from("<nav class=\"vlt-toc\">\n<ul>\n");
        for entry in &self.anchors {
            let label = css::escape_html(entry.title.trim());
            match &entry.slug {
                Some(slug) => markup.push_str(&format!(
                    "<li class=\"vlt-toc-{}\"><a href=\"#{slug}\">{label}</a></li>\n",
                    entry.level
                )),
                None => markup.push_str(&format!(
                    "<li class=\"vlt-toc-{}\">{label}</li>\n",
                    entry.level
                )),
            }
        }
        markup.push_str("</ul>\n</nav>");
        vec![Event::Html(CowStr::from(markup))]
    }
}

/// 去掉段落/标题组两端的容器事件，留下内容事件。
fn group_inner<'a>(
    group: &'a [(Event<'a>, Range<usize>)],
) -> &'a [(Event<'a>, Range<usize>)] {
    let end = group.len().saturating_sub(1);
    if end == 0 {
        return &[];
    }
    &group[1..end]
}

const ALERT_TYPES: [(&str, BlockQuoteKind); 5] = [
    ("NOTE", BlockQuoteKind::Note),
    ("TIP", BlockQuoteKind::Tip),
    ("IMPORTANT", BlockQuoteKind::Important),
    ("WARNING", BlockQuoteKind::Warning),
    ("CAUTION", BlockQuoteKind::Caution),
];

fn alert_title(kind: BlockQuoteKind) -> &'static str {
    match kind {
        BlockQuoteKind::Note => "Note",
        BlockQuoteKind::Tip => "Tip",
        BlockQuoteKind::Important => "Important",
        BlockQuoteKind::Warning => "Warning",
        BlockQuoteKind::Caution => "Caution",
    }
}

/// recon 文本开头是否是 `[!TYPE]` 告警标记；返回类型与标记结束的 recon 偏移。
/// 与 pulldown 的 `scan_blockquote_tag` 对齐：大小写不敏感、`]` 后必须紧跟空白
/// 或行尾——差别只在「同行还有自定义标题」时 pulldown 放弃、我们接住。
fn parse_alert_marker(text: &str) -> Option<(BlockQuoteKind, usize)> {
    let trimmed = text.trim_start_matches([' ', '\t']);
    let offset = text.len() - trimmed.len();
    let inner = trimmed.strip_prefix("[!")?;
    for (name, kind) in ALERT_TYPES {
        let Some(head) = inner.as_bytes().get(..name.len()) else {
            continue;
        };
        if !head.eq_ignore_ascii_case(name.as_bytes()) {
            continue;
        }
        let Some(rest) = inner.get(name.len()..) else {
            continue;
        };
        let Some(after_bracket) = rest.strip_prefix(']') else {
            continue;
        };
        if after_bracket.is_empty()
            || after_bracket.starts_with(' ')
            || after_bracket.starts_with('\t')
            || after_bracket.starts_with('\n')
        {
            return Some((kind, offset + 2 + name.len() + 1));
        }
    }
    None
}

/// recon 偏移 → 去掉首尾空白后的真实内容边界（标题/正文切分用）。
fn trim_start_recon(text: &str, from: usize, to: usize) -> usize {
    let slice = text.get(from..to).unwrap_or_default();
    from + (slice.len() - slice.trim_start().len())
}

fn trim_end_recon(text: &str, from: usize, to: usize) -> usize {
    let slice = text.get(from..to).unwrap_or_default();
    from + slice.trim_end().len()
}

/// 导出能内联的本地图片格式 = `gpui::Img::extensions()`（真正把这张图解码画出来的
/// 那个组件报出的能力清单）。这里只剩一列 MIME：data URI 必须自带 MIME，而 gpui 不
/// 提供，所以这张表躲不掉——但**内联资格不在表里判**，问的是 `Img`。键集与清单逐条
/// 对齐由 `html/tests.rs` 的两条用例钉住：过去这里手抄过一份更窄的扩展名表，应用显示
/// 正常的 TIFF/AVIF/ICO 在共享的单文件 HTML 里静默留成相对路径（报告 12 的下游症状）。
const IMAGE_MIME_BY_EXTENSION: &[(&str, &str)] = &[
    ("avif", "image/avif"),
    ("bmp", "image/bmp"),
    ("dds", "image/x-dds"),
    ("exr", "image/aces"),
    ("ff", "image/x-farbfeld"),
    ("farbfeld", "image/x-farbfeld"),
    ("gif", "image/gif"),
    ("hdr", "image/vnd.radiance"),
    ("ico", "image/vnd.microsoft.icon"),
    ("jpeg", "image/jpeg"),
    ("jpg", "image/jpeg"),
    ("pam", "image/x-portable-anymap"),
    ("pbm", "image/x-portable-bitmap"),
    ("pgm", "image/x-portable-graymap"),
    ("png", "image/png"),
    ("ppm", "image/x-portable-pixmap"),
    ("qoi", "image/qoi"),
    ("svg", "image/svg+xml"),
    ("tga", "image/x-tga"),
    ("tif", "image/tiff"),
    ("tiff", "image/tiff"),
    ("webp", "image/webp"),
];

pub(crate) fn local_image_data_uri(source: &str, base_dir: Option<&Path>) -> Option<String> {
    // 路径解析只有 `resolve_image_source` 一处口径：阅读视图能显示的图，导出必须
    // 也能内联，同一个写法不允许两边解出两个结果。
    let ImageResolvedSource::Local(path) = resolve_image_source(source, base_dir) else {
        return None;
    };
    // 未落盘的文档没有 base_dir，相对目标不能按进程工作目录去读，导出保持原样。
    if base_dir.is_none() && !path.is_absolute() {
        return None;
    }
    let mime = image_mime_from_path(&path)?;
    let bytes = fs::read(&path).ok()?;
    Some(data_uri_for_bytes(mime, &bytes))
}

/// 本地图片路径 → data URI 的 MIME。应用画不出的扩展名给 `None`（导出保持原始写法），
/// 判据就是渲染这张图的那个组件 `gpui::Img::extensions()`，不再手抄第二份格式表。
fn image_mime_from_path(path: &Path) -> Option<&'static str> {
    let extension = path.extension()?.to_string_lossy().to_ascii_lowercase();
    if !gpui::Img::extensions().contains(&extension.as_str()) {
        return None;
    }
    IMAGE_MIME_BY_EXTENSION
        .iter()
        .find(|(candidate, _)| *candidate == extension)
        .map(|(_, mime)| *mime)
}

fn data_uri_for_bytes(mime: &str, bytes: &[u8]) -> String {
    format!(
        "data:{mime};base64,{}",
        general_purpose::STANDARD.encode(bytes)
    )
}

pub(crate) use css::{chromium_pdf_theme_css, contains_tibetan_text, prepare_print_html};
#[cfg(test)]
pub(crate) use css::css_color;
mod css;

#[cfg(test)]
mod tests;
