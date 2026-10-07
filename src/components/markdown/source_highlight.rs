//! Markdown 源码视图的语法高亮（手写逐行扫描器）。
//!
//! 为什么不用 tree-sitter 的 markdown grammar：源码文档按 512 行分块，围栏
//! 会跨过块间接缝，tree-sitter 逐块解析拿不到「进入本块时的状态」，接缝之后
//! 的内容会被当普通 markdown 着色；手写扫描器把「打开的围栏」作为状态带进
//! 带出，分块着色也能接得上。
//!
//! 着色语义对齐 VS Code 的 markdown：标题整行一个标题色、结构记号灰、
//! 链接文字与地址分色——全部取主题的 `md_syntax_*` 字段。

use std::ops::Range;

use super::code_highlight::{
    CodeHighlightClass, CodeHighlightSpan, CodeLanguageKey, highlight_code_block,
    resolve_code_language_key,
};

/// 跨块接缝的状态：本块开头是否正处于某个未闭合的围栏里。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum MarkdownSourceState {
    /// ``` / ~~~ 围栏内。`language` 是开栏信息串解析出的语言，续块内容要
    /// 用同一个语言着色；`fence_len` 是开栏定界符的字节数。
    Fence {
        fence_char: char,
        fence_len: usize,
        language: Option<CodeLanguageKey>,
    },
    /// `$$` 公式块内。
    Math,
    /// 文档头的 YAML frontmatter（`---` … `---`）内。
    Frontmatter,
    /// HTML 注释 `<!--` … `-->` 内。
    HtmlComment,
}

/// 一次扫描的产出：着色区间 + 本块结束时的块级状态（给下一块的入口）。
pub(crate) struct MarkdownSourceHighlight {
    pub(crate) spans: Vec<CodeHighlightSpan>,
    pub(crate) state: Option<MarkdownSourceState>,
}

/// 对一段 markdown 源码文本着色。
///
/// * `at_document_start`——本块第一行是不是文档的第一行（文档头构造只可能
///   从那里开始，用 `source_line_start == 1` 判断）。
/// * `entry`——上一块带出来的状态；分块着色时接缝才能接上。
pub(crate) fn highlight_markdown_source(
    text: &str,
    at_document_start: bool,
    entry: Option<MarkdownSourceState>,
) -> MarkdownSourceHighlight {
    let mut scanner = Scanner {
        text,
        spans: Vec::new(),
        state: entry,
        pending_paragraph: None,
        nested_regions: Vec::new(),
    };
    let mut is_first_line = at_document_start;
    for (line_start, line) in MemLines::new(text) {
        scanner.scan_line(line_start, line, is_first_line);
        is_first_line = false;
    }
    let _ = at_document_start;
    scanner.flush_pending_paragraph();
    scanner.highlight_nested_regions();
    scanner.merge_spans();
    MarkdownSourceHighlight {
        spans: scanner.spans,
        state: scanner.state,
    }
}

/// 按行迭代并带上每行的字节区间（缓冲区是 LF 规范化的，不处理 `\r\n`）。
struct MemLines<'a> {
    text: &'a str,
    cursor: usize,
}

impl<'a> MemLines<'a> {
    fn new(text: &'a str) -> Self {
        Self { text, cursor: 0 }
    }
}

impl<'a> Iterator for MemLines<'a> {
    type Item = (usize, &'a str);

    fn next(&mut self) -> Option<Self::Item> {
        if self.cursor > self.text.len() {
            return None;
        }
        let start = self.cursor;
        let rest = &self.text[start..];
        let item = match rest.find('\n') {
            Some(offset) => {
                self.cursor = start + offset + 1;
                (start, &rest[..offset])
            }
            None => {
                self.cursor = self.text.len() + 1;
                (start, rest)
            }
        };
        Some(item)
    }
}

struct Scanner<'a> {
    text: &'a str,
    spans: Vec<CodeHighlightSpan>,
    state: Option<MarkdownSourceState>,
    /// 可能是 setext 标题正文的上一行：看到下划线行才定性，押着不发。
    pending_paragraph: Option<Range<usize>>,
    /// 带语言的围栏正文区间：主循环收着，扫完统一递归着色。
    nested_regions: Vec<(CodeLanguageKey, Range<usize>)>,
}

impl<'a> Scanner<'a> {
    fn push(&mut self, range: Range<usize>, class: CodeHighlightClass) {
        if range.start < range.end && range.end <= self.text.len() {
            self.spans.push(CodeHighlightSpan { range, class });
        }
    }

    /// 行首缩进 ≤3 个空格之后的内容（制表符按 CommonMark 不算 ≤3 缩进）。
    fn strip_indent(line: &str) -> &str {
        let indent = line.bytes().take_while(|byte| *byte == b' ').count();
        if indent > 3 {
            return "";
        }
        &line[indent..]
    }

    fn scan_line(&mut self, line_start: usize, line: &str, is_first_line: bool) {
        if line.trim().is_empty() {
            self.flush_pending_paragraph();
            return;
        }

        if let Some(state) = self.state.clone() {
            self.scan_inside_state(line_start, line, &state);
            return;
        }

        let content = Self::strip_indent(line);
        let content_start = line_start + (line.len() - content.len());
        let line_end = line_start + line.len();

        // frontmatter 只可能开在文档第一行。
        if is_first_line && content == "---" {
            self.push(content_start..line_end, CodeHighlightClass::MarkdownMarker);
            self.state = Some(MarkdownSourceState::Frontmatter);
            return;
        }

        if let Some(level) = parse_atx_heading(content) {
            self.flush_pending_paragraph();
            self.push(
                content_start..content_start + level as usize,
                CodeHighlightClass::MarkdownMarker,
            );
            self.push(
                content_start + level as usize..line_end,
                CodeHighlightClass::MarkdownHeading(level),
            );
            return;
        }

        // setext 下划线（整行只有 = 或 -）：押着的上一行还在才成立；没有上一行
        // 时 `---` 系是分隔线，单个 `-`/`=` 只是普通文本，继续走后面的分类。
        let trimmed = content.trim_end();
        if line_of_single_char(trimmed, '=') || line_of_single_char(trimmed, '-') {
            let level = if trimmed.starts_with('=') { 1u8 } else { 2u8 };
            if let Some(pending) = self.pending_paragraph.take() {
                self.push(pending, CodeHighlightClass::MarkdownHeading(level));
                self.push(content_start..line_end, CodeHighlightClass::MarkdownHeading(level));
                return;
            }
            if line_of_single_char(trimmed, '-') && trimmed.chars().count() >= 3 {
                self.push(content_start..line_end, CodeHighlightClass::MarkdownMarker);
                return;
            }
        }

        // 分隔线：***、---、___（可夹空格）。
        if is_thematic_break(trimmed) {
            self.flush_pending_paragraph();
            self.push(content_start..line_end, CodeHighlightClass::MarkdownMarker);
            return;
        }

        if let Some(opener) = parse_fence_opener(content) {
            self.flush_pending_paragraph();
            let run_end = content_start + opener.run_len;
            self.push(content_start..run_end, CodeHighlightClass::MarkdownMarker);
            let info = &content[opener.run_len..];
            if info.trim().is_empty() {
                self.push(run_end..line_end, CodeHighlightClass::MarkdownMarker);
            }
            self.state = Some(MarkdownSourceState::Fence {
                fence_char: opener.fence_char,
                fence_len: opener.run_len,
                language: resolve_code_language_key(Some(info.trim())),
            });
            return;
        }

        if content.starts_with("$$") {
            self.flush_pending_paragraph();
            self.push(content_start..content_start + 2, CodeHighlightClass::MarkdownMarker);
            let body = &content[2..];
            match body.find("$$") {
                Some(offset) => {
                    self.push(
                        content_start + 2..content_start + 2 + offset,
                        CodeHighlightClass::MarkdownCode,
                    );
                    self.push(
                        content_start + 2 + offset..content_start + 2 + offset + 2,
                        CodeHighlightClass::MarkdownMarker,
                    );
                }
                None => {
                    self.push(
                        content_start + 2..line_end,
                        CodeHighlightClass::MarkdownCode,
                    );
                    self.state = Some(MarkdownSourceState::Math);
                }
            }
            return;
        }

        if content.starts_with("<!--") {
            self.flush_pending_paragraph();
            match content[4..].find("-->") {
                Some(offset) => {
                    self.push(
                        content_start..content_start + 4 + offset + 3,
                        CodeHighlightClass::Comment,
                    );
                }
                None => {
                    self.push(content_start..line_end, CodeHighlightClass::Comment);
                    self.state = Some(MarkdownSourceState::HtmlComment);
                }
            }
            return;
        }

        if let Some(label_len) = parse_link_definition_label(content) {
            self.flush_pending_paragraph();
            self.push(
                content_start..content_start + label_len,
                CodeHighlightClass::MarkdownLinkText,
            );
            self.push(
                content_start + label_len..line_end,
                CodeHighlightClass::MarkdownLinkUrl,
            );
            return;
        }

        // 引用记号：剥掉行首的 `> ` 链之后，剩下的部分还按块级构造识别
        // （引用里的标题、列表、标注都该有着色）。
        if content.starts_with('>') {
            self.flush_pending_paragraph();
            let marker_len = quote_prefix_len(content);
            self.push(
                content_start..content_start + marker_len,
                CodeHighlightClass::MarkdownMarker,
            );
            self.scan_quoted_remainder(content_start + marker_len, &content[marker_len..], line_end);
            return;
        }

        if let Some(marker_len) = parse_list_marker(content) {
            self.flush_pending_paragraph();
            self.push(
                content_start..content_start + marker_len,
                CodeHighlightClass::MarkdownMarker,
            );
            let rest = &content[marker_len..];
            let rest_start = content_start + marker_len;
            // 任务列表的框也是记号。
            let after_bullet_space = rest.strip_prefix(' ').unwrap_or(rest);
            let box_offset = rest.len() - after_bullet_space.len();
            let after_box = task_box_len(after_bullet_space);
            if let Some(box_len) = after_box {
                self.push(
                    rest_start + box_offset..rest_start + box_offset + box_len,
                    CodeHighlightClass::MarkdownMarker,
                );
            }
            match after_box {
                Some(box_len) => self.scan_inline(
                    rest_start + box_offset + box_len,
                    &after_bullet_space[box_len..],
                ),
                None => self.scan_inline(rest_start, rest),
            }
            return;
        }

        // 押成「可能是 setext 正文」：下一行是不是下划线，看到再发。
        self.flush_pending_paragraph();
        self.pending_paragraph = Some(line_start..line_end);
    }

    /// 处于围栏里的一行：闭合记号之外的内容收进递归区（带语言时整段交给
    /// 既有代码高亮管线）；区间要连续拼接才能整段解析，逐行散着发会丢多行
    /// 结构。
    fn scan_inside_state(&mut self, line_start: usize, line: &str, state: &MarkdownSourceState) {
        let line_end = line_start + line.len();
        match state {
            MarkdownSourceState::Fence {
                fence_char,
                fence_len,
                language,
            } => {
                let content = Self::strip_indent(line);
                let content_start = line_start + (line.len() - content.len());
                let run_chars = content.chars().take_while(|c| *c == *fence_char).count();
                if run_chars * fence_char.len_utf8() >= *fence_len
                    && content[run_chars * fence_char.len_utf8()..].trim().is_empty()
                {
                    self.push(
                        content_start..content_start + run_chars * fence_char.len_utf8(),
                        CodeHighlightClass::MarkdownMarker,
                    );
                    self.state = None;
                    return;
                }
                // 带语言的围栏内容收进递归区；不带语言的保持正文色。
                if let Some(language) = language {
                    self.append_nested_region(*language, line_start..line_end);
                }
            }
            MarkdownSourceState::Math => {
                let content = Self::strip_indent(line);
                let content_start = line_start + (line.len() - content.len());
                match content.find("$$") {
                    Some(offset) => {
                        self.push(
                            content_start..content_start + offset,
                            CodeHighlightClass::MarkdownCode,
                        );
                        self.push(
                            content_start + offset..content_start + offset + 2,
                            CodeHighlightClass::MarkdownMarker,
                        );
                        self.state = None;
                    }
                    None => {
                        self.push(content_start..line_end, CodeHighlightClass::MarkdownCode);
                    }
                }
            }
            MarkdownSourceState::Frontmatter => {
                let content = Self::strip_indent(line);
                let content_start = line_start + (line.len() - content.len());
                if content == "---" || content == "..." {
                    self.push(content_start..line_end, CodeHighlightClass::MarkdownMarker);
                    self.state = None;
                    return;
                }
                self.append_nested_region(CodeLanguageKey::Yaml, line_start..line_end);
            }
            MarkdownSourceState::HtmlComment => match line.find("-->") {
                Some(offset) => {
                    self.push(line_start..line_start + offset + 3, CodeHighlightClass::Comment);
                    self.state = None;
                }
                None => {
                    self.push(line_start..line_end, CodeHighlightClass::Comment);
                }
            },
        }
    }

    fn append_nested_region(&mut self, language: CodeLanguageKey, range: Range<usize>) {
        match self.nested_regions.last_mut() {
            Some((existing, region)) if *existing == language && region.end == range.start => {
                region.end = range.end;
            }
            _ => self.nested_regions.push((language, range)),
        }
    }

    /// 排序并合并相邻同类区间；产出必须有序（run 构建按序号推进）。
    fn merge_spans(&mut self) {
        self.spans.sort_by_key(|span| span.range.start);
        let mut merged: Vec<CodeHighlightSpan> = Vec::with_capacity(self.spans.len());
        for span in self.spans.drain(..) {
            if span.range.start >= span.range.end {
                continue;
            }
            match merged.last_mut() {
                Some(last) if last.class == span.class && last.range.end >= span.range.start => {
                    last.range.end = last.range.end.max(span.range.end);
                }
                _ => merged.push(span),
            }
        }
        self.spans = merged;
    }

    /// 把收好的围栏正文递归交给代码高亮，区间平移回本块坐标。
    fn highlight_nested_regions(&mut self) {
        let regions = std::mem::take(&mut self.nested_regions);
        for (language, region) in regions {
            let content = &self.text[region.clone()];
            if let Some(result) = highlight_code_block(Some(language_key_name(language)), content) {
                for span in result.spans {
                    self.push(
                        region.start + span.range.start..region.start + span.range.end,
                        span.class,
                    );
                }
            }
        }
    }

    /// 引用记号之后的剩余部分：只识别标题、列表记号与标注标签；
    /// 行内构造着色随后的功能点接入。
    fn scan_quoted_remainder(&mut self, rest_start: usize, rest: &str, line_end: usize) {
        let trimmed = rest.strip_prefix(' ').unwrap_or(rest);
        let trimmed_start = rest_start + (rest.len() - trimmed.len());
        if let Some(level) = parse_atx_heading(trimmed) {
            self.push(
                trimmed_start..trimmed_start + level as usize,
                CodeHighlightClass::MarkdownMarker,
            );
            self.push(
                trimmed_start + level as usize..line_end,
                CodeHighlightClass::MarkdownHeading(level),
            );
            return;
        }
        if let Some(label_len) = parse_callout_label(trimmed) {
            self.push(
                trimmed_start..trimmed_start + label_len,
                CodeHighlightClass::MarkdownLabel,
            );
            self.scan_inline(trimmed_start + label_len, &trimmed[label_len..]);
            return;
        }
        if let Some(marker_len) = parse_list_marker(trimmed) {
            self.push(
                trimmed_start..trimmed_start + marker_len,
                CodeHighlightClass::MarkdownMarker,
            );
            let rest = &trimmed[marker_len..];
            let rest_start = trimmed_start + marker_len;
            // 任务列表的框也是记号（与顶层列表同一条口径）。
            let after_bullet_space = rest.strip_prefix(' ').unwrap_or(rest);
            let box_offset = rest.len() - after_bullet_space.len();
            let after_box = task_box_len(after_bullet_space);
            if let Some(box_len) = after_box {
                self.push(
                    rest_start + box_offset..rest_start + box_offset + box_len,
                    CodeHighlightClass::MarkdownMarker,
                );
            }
            match after_box {
                Some(box_len) => self.scan_inline(
                    rest_start + box_offset + box_len,
                    &after_bullet_space[box_len..],
                ),
                None => self.scan_inline(rest_start, rest),
            }
            return;
        }
        self.scan_inline(rest_start, rest);
    }

    /// 押着的 setext 候选行：下一行不是下划线时按普通正文发行内着色。
    fn flush_pending_paragraph(&mut self) {
        if let Some(range) = self.pending_paragraph.take() {
            let line = &self.text[range.clone()];
            self.scan_inline(range.start, line);
        }
    }

    /// 行内构造扫描：代码 span 优先（里面的 `*`、`[` 都不是记号），然后转义、
    /// 强调、删除线、链接、图片、自动链接、脚注引用、双链与标签。
    fn scan_inline(&mut self, range_start: usize, line: &str) {
        let base = range_start;
        let bytes = line.as_bytes();
        let mut i = 0usize;
        while i < line.len() {
            match bytes[i] {
                b'\\' if i + 1 < line.len() => {
                    let next_len = utf8_len(bytes[i + 1]);
                    self.push(base + i..base + i + 1 + next_len, CodeHighlightClass::MarkdownEscape);
                    i += 1 + next_len;
                }
                b'`' => {
                    let run = char_run(bytes, i, b'`');
                    match find_backtick_run(bytes, i + run, run) {
                        Some(close) => {
                            self.push(base + i..base + close + run, CodeHighlightClass::MarkdownCode);
                            i = close + run;
                        }
                        None => {
                            i += run;
                        }
                    }
                }
                b'*' | b'_' => {
                    let marker = bytes[i];
                    let run = char_run(bytes, i, marker);
                    if run >= 2 {
                        match find_emphasis_close(bytes, i + run, marker, run) {
                            Some(close) => {
                                self.push(
                                    base + i..base + i + run,
                                    CodeHighlightClass::MarkdownEmphasisMarker,
                                );
                                self.push(
                                    base + i + run..base + close,
                                    CodeHighlightClass::MarkdownStrong,
                                );
                                self.push(
                                    base + close..base + close + run,
                                    CodeHighlightClass::MarkdownEmphasisMarker,
                                );
                                i = close + run;
                            }
                            None => {
                                i += run;
                            }
                        }
                    } else {
                        // 单字符斜体：`_` 还要求不在词中（snake_case 不是斜体）。
                        let opener_ok = i + 1 < line.len()
                            && !bytes[i + 1].is_ascii_whitespace()
                            && (marker == b'*'
                                || i == 0
                                || bytes[i - 1].is_ascii_whitespace()
                                || bytes[i - 1].is_ascii_punctuation());
                        let close = if opener_ok {
                            find_emphasis_close(bytes, i + 1, marker, 1)
                        } else {
                            None
                        };
                        match close {
                            Some(close) if close > i + 1 => {
                                self.push(
                                    base + i..base + i + 1,
                                    CodeHighlightClass::MarkdownEmphasisMarker,
                                );
                                self.push(
                                    base + i + 1..base + close,
                                    CodeHighlightClass::MarkdownEmphasis,
                                );
                                self.push(
                                    base + close..base + close + 1,
                                    CodeHighlightClass::MarkdownEmphasisMarker,
                                );
                                i = close + 1;
                            }
                            _ => {
                                i += 1;
                            }
                        }
                    }
                }
                b'~' => {
                    let run = char_run(bytes, i, b'~');
                    if run == 2 {
                        match find_exact(bytes, i + 2, b"~~") {
                            Some(close) => {
                                self.push(
                                    base + i..base + i + 2,
                                    CodeHighlightClass::MarkdownEmphasisMarker,
                                );
                                self.push(
                                    base + i + 2..base + close,
                                    CodeHighlightClass::MarkdownStrikethrough,
                                );
                                self.push(
                                    base + close..base + close + 2,
                                    CodeHighlightClass::MarkdownEmphasisMarker,
                                );
                                i = close + 2;
                            }
                            None => {
                                i += run;
                            }
                        }
                    } else {
                        i += run;
                    }
                }
                b'!' if i + 1 < line.len() && bytes[i + 1] == b'[' => {
                    i += self.try_scan_link(base, bytes, i + 1, true);
                }
                b'[' => {
                    i += self.try_scan_link(base, bytes, i, false);
                }
                b'<' => {
                    i = match autolink_end(line, i) {
                        Some(end) => {
                            self.push(base + i..base + end, CodeHighlightClass::MarkdownLinkUrl);
                            end
                        }
                        None => i + 1,
                    };
                }
                b'#' if i == 0 || bytes[i - 1].is_ascii_whitespace() => {
                    let mut end = i + 1;
                    while end < line.len() && !bytes[end].is_ascii_whitespace() {
                        end += 1;
                    }
                    if end > i + 1 {
                        self.push(base + i..base + end, CodeHighlightClass::MarkdownLabel);
                        i = end;
                    } else {
                        i += 1;
                    }
                }
                _ => {
                    i += 1;
                }
            }
        }
    }

    /// `[`（图片传 `[` 的位置并置 `image`）处尝试识别链接构造，返回消费掉的
    /// 字节数（至少 1，识别失败时只当普通字符）。`base` 是本行在整块文本里的
    /// 起始偏移——push 的是整块坐标，漏加就会把这一行的链接色画到别的行上。
    fn try_scan_link(&mut self, base: usize, bytes: &[u8], open: usize, image: bool) -> usize {
        let marker_open = if image { open - 1 } else { open };
        let marker_len = if image { 2 } else { 1 };
        let Some(close) = find_unescaped(bytes, open + 1, b']') else {
            return 1;
        };
        // [label](url)
        if bytes.get(close + 1) == Some(&b'(') {
            if let Some(paren) = find_unescaped(bytes, close + 2, b')') {
                self.push(
                    base + marker_open..base + marker_open + marker_len,
                    CodeHighlightClass::MarkdownMarker,
                );
                self.push(
                    base + open + 1..base + close,
                    CodeHighlightClass::MarkdownLinkText,
                );
                self.push(
                    base + close..base + close + 2,
                    CodeHighlightClass::MarkdownMarker,
                );
                self.push(
                    base + close + 2..base + paren,
                    CodeHighlightClass::MarkdownLinkUrl,
                );
                self.push(
                    base + paren..base + paren + 1,
                    CodeHighlightClass::MarkdownMarker,
                );
                return paren + 1 - marker_open;
            }
        }
        // [label][ref]
        if bytes.get(close + 1) == Some(&b'[') {
            if let Some(ref_close) = find_unescaped(bytes, close + 2, b']') {
                self.push(
                    base + marker_open..base + marker_open + marker_len,
                    CodeHighlightClass::MarkdownMarker,
                );
                self.push(
                    base + open + 1..base + close,
                    CodeHighlightClass::MarkdownLinkText,
                );
                self.push(
                    base + close..base + close + 2,
                    CodeHighlightClass::MarkdownMarker,
                );
                self.push(
                    base + close + 2..base + ref_close,
                    CodeHighlightClass::MarkdownLinkUrl,
                );
                self.push(
                    base + ref_close..base + ref_close + 1,
                    CodeHighlightClass::MarkdownMarker,
                );
                return ref_close + 1 - marker_open;
            }
        }
        // [[wikilink]]：外层括号做记号，目标上链接色。
        if bytes.get(open + 1) == Some(&b'[') && bytes.get(close + 1) == Some(&b']') {
            self.push(
                base + marker_open..base + open + 2,
                CodeHighlightClass::MarkdownMarker,
            );
            self.push(
                base + open + 2..base + close,
                CodeHighlightClass::MarkdownLinkText,
            );
            self.push(
                base + close..base + close + 2,
                CodeHighlightClass::MarkdownMarker,
            );
            return close + 2 - marker_open;
        }
        // [^footnote] 引用（后面没跟地址的才算）。
        if bytes.get(open + 1) == Some(&b'^') && close > open + 2 {
            self.push(
                base + marker_open..base + close + 1,
                CodeHighlightClass::MarkdownLabel,
            );
            return close + 1 - marker_open;
        }
        1
    }
}

fn utf8_len(first_byte: u8) -> usize {
    match first_byte {
        0x00..=0x7f => 1,
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        _ => 4,
    }
}

fn char_run(bytes: &[u8], start: usize, marker: u8) -> usize {
    let mut end = start;
    while end < bytes.len() && bytes[end] == marker {
        end += 1;
    }
    end - start
}

/// 从 `from` 起找一段等长的反引号闭合 run。
fn find_backtick_run(bytes: &[u8], from: usize, len: usize) -> Option<usize> {
    let mut i = from;
    while i < bytes.len() {
        if bytes[i] == b'`' {
            let run = char_run(bytes, i, b'`');
            if run == len {
                return Some(i);
            }
            i += run;
        } else {
            i += 1;
        }
    }
    None
}

fn find_exact(bytes: &[u8], from: usize, pattern: &[u8]) -> Option<usize> {
    bytes[from..]
        .windows(pattern.len())
        .position(|window| window == pattern)
        .map(|offset| from + offset)
}

fn find_unescaped(bytes: &[u8], from: usize, marker: u8) -> Option<usize> {
    let mut i = from;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => {
                i += 2;
            }
            byte if byte == marker => return Some(i),
            _ => {
                i += 1;
            }
        }
    }
    None
}

/// 强调闭定界符：与开定界符同字符、长度不少于 `len`，且前一个字符不是空白
/// （内容不能为空）。
fn find_emphasis_close(bytes: &[u8], from: usize, marker: u8, len: usize) -> Option<usize> {
    let mut i = from;
    while i < bytes.len() {
        if bytes[i] == marker {
            let run = char_run(bytes, i, marker);
            if run >= len && i > from && !bytes[i - 1].is_ascii_whitespace() {
                return Some(i);
            }
            i += run;
        } else {
            i += 1;
        }
    }
    None
}

/// `<scheme:...>` / `<email@host>` 形式的自动链接，返回闭合 `>` 之后的位置。
fn autolink_end(line: &str, open: usize) -> Option<usize> {
    let bytes = line.as_bytes();
    let close = find_unescaped(bytes, open + 1, b'>')?;
    let inner = &line[open + 1..close];
    let is_uri = inner.contains(':')
        && inner.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && inner
            .chars()
            .take_while(|c| *c != ':')
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'));
    let is_email = inner.contains('@')
        && !inner.starts_with('@')
        && !inner.ends_with('@')
        && !inner.contains(char::is_whitespace);
    (is_uri || is_email).then_some(close + 1)
}

/// 数学块编辑态的 LaTeX 源码着色（公式块聚焦时整个文本走
/// `build_code_text_runs`，着色数据从这里来）。
///
/// 逐字符线性扫描：`$$` 定界符结构灰、`\命令` 用关键字色、数字用数字色、
/// 花括号/方括号标点、`^ _ &` 运算符、`%` 到行尾注释；字母与汉字保持
/// 正文色不出 span——公式主体本来就以默认色最可读。
pub(crate) fn highlight_latex_source(text: &str) -> Vec<CodeHighlightSpan> {
    let bytes = text.as_bytes();
    let mut spans = Vec::new();
    let mut push = |start: usize, end: usize, class: CodeHighlightClass| {
        if start < end {
            spans.push(CodeHighlightSpan {
                range: start..end,
                class,
            });
        }
    };
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => {
                if i + 1 < bytes.len() && bytes[i + 1].is_ascii_alphabetic() {
                    let mut end = i + 1;
                    while end < bytes.len() && bytes[end].is_ascii_alphabetic() {
                        end += 1;
                    }
                    push(i, end, CodeHighlightClass::Keyword);
                    i = end;
                } else if i + 1 < bytes.len() && bytes[i + 1] == b'\\' {
                    // `\\` 换行命令。
                    push(i, i + 2, CodeHighlightClass::Punctuation);
                    i += 2;
                } else {
                    push(i, i + 1, CodeHighlightClass::Punctuation);
                    i += 1;
                }
            }
            b'0'..=b'9' => {
                let mut end = i + 1;
                while end < bytes.len() && bytes[end].is_ascii_digit() {
                    end += 1;
                }
                // 小数：数字后跟 `.` 再跟数字一并收进。
                if end + 1 < bytes.len() && bytes[end] == b'.' && bytes[end + 1].is_ascii_digit() {
                    end += 2;
                    while end < bytes.len() && bytes[end].is_ascii_digit() {
                        end += 1;
                    }
                }
                push(i, end, CodeHighlightClass::Number);
                i = end;
            }
            b'{' | b'}' | b'[' | b']' => {
                push(i, i + 1, CodeHighlightClass::Punctuation);
                i += 1;
            }
            b'^' | b'_' | b'&' => {
                push(i, i + 1, CodeHighlightClass::Operator);
                i += 1;
            }
            b'$' if text[i..].starts_with("$$") => {
                push(i, i + 2, CodeHighlightClass::MarkdownMarker);
                i += 2;
            }
            b'%' => {
                let end = text[i..].find('\n').map(|offset| i + offset).unwrap_or(text.len());
                push(i, end, CodeHighlightClass::Comment);
                i = end;
            }
            _ => {
                i += 1;
            }
        }
    }
    spans
}

fn language_key_name(key: CodeLanguageKey) -> &'static str {
    match key {
        CodeLanguageKey::Rust => "rust",
        CodeLanguageKey::JavaScript => "javascript",
        CodeLanguageKey::JavaScriptJsx => "jsx",
        CodeLanguageKey::TypeScript => "typescript",
        CodeLanguageKey::TypeScriptTsx => "tsx",
        CodeLanguageKey::Json => "json",
        CodeLanguageKey::Markdown => "markdown",
        CodeLanguageKey::Bash => "bash",
        CodeLanguageKey::C => "c",
        CodeLanguageKey::Cpp => "cpp",
        CodeLanguageKey::CSharp => "csharp",
        CodeLanguageKey::Css => "css",
        CodeLanguageKey::Go => "go",
        CodeLanguageKey::Html => "html",
        CodeLanguageKey::Java => "java",
        CodeLanguageKey::Php => "php",
        CodeLanguageKey::Python => "python",
        CodeLanguageKey::Ruby => "ruby",
        CodeLanguageKey::Yaml => "yaml",
        CodeLanguageKey::Toml => "toml",
        CodeLanguageKey::Mermaid => "mermaid",
        CodeLanguageKey::PlainText => "text",
    }
}

/// 整行（去尾空白）只由同一个字符构成且非空。
fn line_of_single_char(trimmed: &str, ch: char) -> bool {
    !trimmed.is_empty() && trimmed.chars().all(|c| c == ch)
}

fn is_thematic_break(trimmed: &str) -> bool {
    let compact: String = trimmed.chars().filter(|c| !c.is_whitespace()).collect();
    compact.len() >= 3
        && (line_of_single_char(&compact, '*')
            || line_of_single_char(&compact, '-')
            || line_of_single_char(&compact, '_'))
}

/// ATX 标题：`#{1,6}` 后面必须跟空格或行尾。
fn parse_atx_heading(content: &str) -> Option<u8> {
    let hashes = content.bytes().take_while(|byte| *byte == b'#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let rest = &content[hashes..];
    (rest.is_empty() || rest.starts_with(' ')).then_some(hashes as u8)
}

struct FenceOpener {
    fence_char: char,
    /// 定界符的字节宽度。
    run_len: usize,
}

/// 围栏开栏：≥3 个 ` 或 ~。信息串里不能再有定界字符（CommonMark）。
fn parse_fence_opener(content: &str) -> Option<FenceOpener> {
    let first = content.chars().next()?;
    if first != '`' && first != '~' {
        return None;
    }
    let run_chars = content.chars().take_while(|c| *c == first).count();
    if run_chars < 3 {
        return None;
    }
    let run_len = run_chars * first.len_utf8();
    if content[run_len..].contains(first) {
        return None;
    }
    Some(FenceOpener {
        fence_char: first,
        run_len,
    })
}

/// 列表记号宽度（不含前导缩进）：`-`/`*`/`+` 或 `1.`/`1)`，后面要跟空白或行尾。
fn parse_list_marker(content: &str) -> Option<usize> {
    let bytes = content.as_bytes();
    let after_bullet = |marker_len: usize| -> Option<usize> {
        match bytes.get(marker_len) {
            None => Some(marker_len),
            Some(byte) if byte.is_ascii_whitespace() => Some(marker_len),
            _ => None,
        }
    };
    match bytes.first() {
        Some(b'-') | Some(b'*') | Some(b'+') => after_bullet(1),
        Some(byte) if byte.is_ascii_digit() => {
            let digits = bytes.iter().take_while(|b| b.is_ascii_digit()).count();
            if digits > 9 {
                return None;
            }
            match bytes.get(digits) {
                Some(b'.') | Some(b')') => after_bullet(digits + 1),
                _ => None,
            }
        }
        _ => None,
    }
}

/// 任务框 `[ ]` / `[x]` / `[X]` 的宽度，后面要跟空白或行尾。
fn task_box_len(rest: &str) -> Option<usize> {
    let bytes = rest.as_bytes();
    if bytes.first() != Some(&b'[') || bytes.len() < 3 {
        return None;
    }
    let valid = matches!(bytes[1], b' ' | b'x' | b'X') && bytes[2] == b']';
    if !valid {
        return None;
    }
    match bytes.get(3) {
        None => Some(3),
        Some(byte) if byte.is_ascii_whitespace() => Some(3),
        _ => None,
    }
}

/// 标注标签 `[!NOTE]` 之类，大小写不限。
fn parse_callout_label(rest: &str) -> Option<usize> {
    let rest = rest.strip_prefix('[')?;
    let bang = rest.strip_prefix('!')?;
    let end = bang.find(']')?;
    let word = &bang[..end];
    (!word.is_empty() && word.chars().all(|c| c.is_ascii_alphanumeric())).then_some(2 + end + 1)
}

/// 引用定义 `[label]:`。
fn parse_link_definition_label(content: &str) -> Option<usize> {
    if !content.starts_with('[') {
        return None;
    }
    let close = content[1..].find("]:")? + 1;
    (close > 1).then_some(close + 1)
}

/// 引用记号链的宽度：`>` 与其后至多一个空白，逐层嵌套。
fn quote_prefix_len(content: &str) -> usize {
    let mut total = 0usize;
    let mut rest = content;
    loop {
        if !rest.starts_with('>') {
            return total;
        }
        total += 1;
        rest = &rest[1..];
        if let Some(stripped) = rest.strip_prefix(' ').or_else(|| rest.strip_prefix('\t')) {
            total += 1;
            rest = stripped;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn highlight(text: &str) -> Vec<(Range<usize>, CodeHighlightClass)> {
        highlight_with(text, false, None).0
    }

    fn highlight_with(
        text: &str,
        at_document_start: bool,
        entry: Option<MarkdownSourceState>,
    ) -> (Vec<(Range<usize>, CodeHighlightClass)>, Option<MarkdownSourceState>) {
        let result = highlight_markdown_source(text, at_document_start, entry);
        (
            result
                .spans
                .iter()
                .map(|span| (span.range.clone(), span.class))
                .collect(),
            result.state,
        )
    }

    /// `needle` 整段落进的 span 的类（span 必须完整盖住 needle）。
    fn classes_covering(text: &str, needle: &str) -> Vec<CodeHighlightClass> {
        let start = text.find(needle).expect("needle in text");
        let end = start + needle.len();
        highlight(text)
            .into_iter()
            .filter(|(range, _)| range.start <= start && end <= range.end)
            .map(|(_, class)| class)
            .collect()
    }

    fn span_covering(text: &str, needle: &str) -> bool {
        !classes_covering(text, needle).is_empty()
    }
    fn class_at(text: &str, needle: &str) -> Option<CodeHighlightClass> {
        let start = text.find(needle).expect("needle in text");
        highlight(text)
            .into_iter()
            .find(|(range, _)| range.start <= start && start < range.end)
            .map(|(_, class)| class)
    }

    #[test]
    fn atx_heading_colors_marker_and_text() {
        let text = "## 界面预览";
        assert_eq!(class_at(text, "##"), Some(CodeHighlightClass::MarkdownMarker));
        assert_eq!(
            classes_covering(text, "界面预览"),
            vec![CodeHighlightClass::MarkdownHeading(2)]
        );
        // 没跟空格的 `#tag` 不是标题，是标签。
        assert_eq!(class_at("#tag 字", "#tag"), Some(CodeHighlightClass::MarkdownLabel));
    }

    #[test]
    fn setext_underline_promotes_previous_line() {
        let text = "标题甲\n======\n正文";
        assert_eq!(
            classes_covering(text, "标题甲"),
            vec![CodeHighlightClass::MarkdownHeading(1)]
        );
        assert_eq!(
            classes_covering(text, "======"),
            vec![CodeHighlightClass::MarkdownHeading(1)]
        );
        // 前面没有正文行时 `---` 是分隔线。
        assert_eq!(
            class_at("---\n正文", "---"),
            Some(CodeHighlightClass::MarkdownMarker)
        );
    }

    #[test]
    fn fenced_code_with_language_highlights_content() {
        let text = "```rust\nfn main() {}\n```\n";
        let (spans, state) = highlight_with(text, false, None);
        assert!(state.is_none());
        assert!(
            spans
                .iter()
                .any(|(range, class)| *class == CodeHighlightClass::Keyword
                    && &text[range.clone()] == "fn"),
            "rust 围栏内容应有 tree-sitter 关键字着色: {spans:?}"
        );
        assert_eq!(
            class_at(text, "```rust"),
            Some(CodeHighlightClass::MarkdownMarker)
        );
    }

    #[test]
    fn fence_crossing_chunk_seam_threads_state() {
        let first = "正文\n```python\nx = 1\n";
        let (first_spans, state) = highlight_with(first, false, None);
        assert_eq!(
            state,
            Some(MarkdownSourceState::Fence {
                fence_char: '`',
                fence_len: 3,
                language: Some(CodeLanguageKey::Python),
            })
        );
        assert!(
            first_spans
                .iter()
                .any(|(range, class)| *class == CodeHighlightClass::Variable
                    && &first[range.clone()] == "x"),
            "接缝前的围栏内容应已按 python 着色: {first_spans:?}"
        );

        let second = "y = 2\n```\n正文二";
        let (second_spans, second_state) = highlight_with(second, false, state);
        assert_eq!(second_state, None);
        assert!(
            second_spans
                .iter()
                .any(|(range, class)| *class == CodeHighlightClass::Variable
                    && &second[range.clone()] == "y"),
            "接缝之后的内容要接着用同一个语言着色: {second_spans:?}"
        );
        assert!(classes_covering(second, "正文二").is_empty());
    }

    #[test]
    fn unterminated_tilde_fence_threads_state() {
        let text = "~~~";
        let (spans, state) = highlight_with(text, false, None);
        assert_eq!(
            state,
            Some(MarkdownSourceState::Fence {
                fence_char: '~',
                fence_len: 3,
                language: None,
            })
        );
        assert_eq!(spans.len(), 1);
    }

    #[test]
    fn blockquote_and_list_and_task_markers() {
        let text = "> 引用正文\n\n- [ ] 待办项\n- 已完成\n1. 有序项";
        assert_eq!(class_at(text, "> 引用"), Some(CodeHighlightClass::MarkdownMarker));
        assert_eq!(class_at(text, "[ ]"), Some(CodeHighlightClass::MarkdownMarker));
        // 列表记号是记号色。
        let dash_pos = text.find("\n- ").unwrap() + 1;
        let dash_span = highlight(text)
            .into_iter()
            .find(|(range, _)| range.start == dash_pos)
            .map(|(_, class)| class);
        assert_eq!(dash_span, Some(CodeHighlightClass::MarkdownMarker));
    }

    #[test]
    fn quote_inner_list_and_inline_constructs() {
        // 引用剥掉 `> ` 之后，剩余部分还要按列表与行内构造着色。
        let text = "> - [ ] 引用里的待办\n> 1. 有序 **加粗**";
        assert_eq!(class_at(text, "[ ]"), Some(CodeHighlightClass::MarkdownMarker));
        // 有序记号之后的内容不带记号色。
        assert!(classes_covering(text, "有序").is_empty());
        assert!(span_covering(text, "1."));
        assert_eq!(
            classes_covering(text, "加粗"),
            vec![CodeHighlightClass::MarkdownStrong]
        );
    }

    #[test]
    fn quote_inner_heading_and_callout() {
        let text = "> ## 引用里的标题\n\n> [!NOTE] 标注说明";
        assert_eq!(
            classes_covering(text, "引用里的标题"),
            vec![CodeHighlightClass::MarkdownHeading(2)]
        );
        assert_eq!(
            class_at(text, "[!NOTE]"),
            Some(CodeHighlightClass::MarkdownLabel)
        );
    }

    #[test]
    fn link_definitions_color_label_and_destination() {
        let text = "[甲]: https://example.com";
        assert_eq!(
            classes_covering(text, "[甲]"),
            vec![CodeHighlightClass::MarkdownLinkText]
        );
        assert_eq!(
            classes_covering(text, "https://example.com"),
            vec![CodeHighlightClass::MarkdownLinkUrl]
        );
    }

    #[test]
    fn inline_code_span_takes_precedence_over_emphasis() {
        let text = "前 `**not bold**` 后";
        assert_eq!(
            classes_covering(text, "**not bold**"),
            vec![CodeHighlightClass::MarkdownCode]
        );
    }

    #[test]
    fn emphasis_uses_glyph_classes_not_color() {
        let text = "粗 **加粗字** 斜 *斜体字* 删 ~~划掉~~";
        assert_eq!(
            classes_covering(text, "加粗字"),
            vec![CodeHighlightClass::MarkdownStrong]
        );
        assert_eq!(
            classes_covering(text, "斜体字"),
            vec![CodeHighlightClass::MarkdownEmphasis]
        );
        assert_eq!(
            classes_covering(text, "划掉"),
            vec![CodeHighlightClass::MarkdownStrikethrough]
        );
        assert_eq!(
            class_at(text, "**"),
            Some(CodeHighlightClass::MarkdownEmphasisMarker)
        );
    }

    #[test]
    fn intraword_underscore_does_not_emphasize() {
        let text = "snake_case_word 与 *真斜体*";
        assert!(classes_covering(text, "snake_case_word").is_empty());
        assert_eq!(
            classes_covering(text, "真斜体"),
            vec![CodeHighlightClass::MarkdownEmphasis]
        );
    }

    #[test]
    fn links_split_label_and_url() {
        let text = "看 [文档](https://example.com/a) 与 ![图](img/甲.png)";
        assert_eq!(
            classes_covering(text, "文档"),
            vec![CodeHighlightClass::MarkdownLinkText]
        );
        assert_eq!(
            classes_covering(text, "https://example.com/a"),
            vec![CodeHighlightClass::MarkdownLinkUrl]
        );
        assert_eq!(
            classes_covering(text, "图"),
            vec![CodeHighlightClass::MarkdownLinkText]
        );
        assert_eq!(
            classes_covering(text, "img/甲.png"),
            vec![CodeHighlightClass::MarkdownLinkUrl]
        );
        assert_eq!(class_at(text, "[文档]"), Some(CodeHighlightClass::MarkdownMarker));
        assert_eq!(class_at(text, "![图]"), Some(CodeHighlightClass::MarkdownMarker));
    }

    #[test]
    fn autolinks_and_footnotes_and_wikilinks() {
        let text = "<https://example.com> 与 [^1] 注脚 与 [[笔记名]]";
        assert_eq!(
            classes_covering(text, "<https://example.com>"),
            vec![CodeHighlightClass::MarkdownLinkUrl]
        );
        assert_eq!(class_at(text, "[^1]"), Some(CodeHighlightClass::MarkdownLabel));
        assert_eq!(
            classes_covering(text, "笔记名"),
            vec![CodeHighlightClass::MarkdownLinkText]
        );
    }

    #[test]
    fn escapes_take_two_bytes_and_are_multibyte_safe() {
        let text = "字面 \\* 不强调 \\」中";
        let spans = highlight(text);
        assert!(spans
            .iter()
            .any(|(range, class)| *class == CodeHighlightClass::MarkdownEscape
                && &text[range.clone()] == "\\*"));
        for (range, _) in spans {
            assert!(text.is_char_boundary(range.start) && text.is_char_boundary(range.end));
        }
    }

    #[test]
    fn links_on_later_lines_do_not_bleed_into_earlier_ones() {
        // 回归：try_scan_link 曾用行内相对偏移当整块坐标，第二行的链接色
        // 会画到第一行的文字上（用户报修：README 第 7 行前半截被串色）。
        let text = "# 标题\n\n看 [文档](https://example.com/a) 与 ![图](img/甲.png)\n";
        let link_line_start = text.find("看 [文档]").expect("链接行");
        let spans = highlight(text);
        for (range, class) in &spans {
            assert!(
                !(range.end <= text.find("\n\n").unwrap()
                    && matches!(class, CodeHighlightClass::MarkdownLinkText | CodeHighlightClass::MarkdownLinkUrl)),
                "标题行的 span 不该是链接色: {range:?} {class:?}"
            );
            let _ = link_line_start;
        }
        assert_eq!(
            classes_covering(text, "文档"),
            vec![CodeHighlightClass::MarkdownLinkText]
        );
        assert_eq!(
            classes_covering(text, "https://example.com/a"),
            vec![CodeHighlightClass::MarkdownLinkUrl]
        );
        assert_eq!(
            classes_covering(text, "img/甲.png"),
            vec![CodeHighlightClass::MarkdownLinkUrl]
        );
    }

    #[test]
    fn display_math_blocks_color_content() {
        let single = "$$x^2 + y^2$$";
        assert_eq!(class_at(single, "$$"), Some(CodeHighlightClass::MarkdownMarker));
        assert_eq!(
            classes_covering(single, "x^2 + y^2"),
            vec![CodeHighlightClass::MarkdownCode]
        );

        let multi = "$$\n\\int_0^1 x dx\n$$";
        assert_eq!(
            classes_covering(multi, "\\int_0^1 x dx"),
            vec![CodeHighlightClass::MarkdownCode]
        );
    }

    #[test]
    fn frontmatter_highlights_as_yaml_only_at_document_start() {
        let doc = "---\ntitle: \"甲\"\n---\n正文";
        let (spans, state) = highlight_with(doc, true, None);
        assert!(state.is_none());
        assert!(
            spans
                .iter()
                .any(|(range, class)| *class == CodeHighlightClass::String
                    && doc[range.clone()].contains("甲")),
            "frontmatter 正文应按 YAML 着色: {spans:?}"
        );

        // 文档中段的 `---` 不是 frontmatter，只是分隔线。
        let mid = "正文一\n\n---\n\ntitle: \"乙\"\n";
        let (mid_spans, _) = highlight_with(mid, false, None);
        assert!(
            !mid_spans
                .iter()
                .any(|(range, _)| mid[range.clone()].contains("乙")),
            "非文档头的 --- 之后的文本不该按 YAML 着色: {mid_spans:?}"
        );
    }

    #[test]
    fn unterminated_frontmatter_threads_state() {
        let (spans, state) = highlight_with("---\nkey: value\n", true, None);
        assert_eq!(state, Some(MarkdownSourceState::Frontmatter));
        assert!(spans
            .iter()
            .any(|(range, class)| *class == CodeHighlightClass::MarkdownMarker
                && range.start == 0));
        assert!(spans
            .iter()
            .any(|(range, class)| !matches!(class, CodeHighlightClass::MarkdownMarker)
                && range.start > 4));
    }

    #[test]
    fn html_comments_color_across_lines() {
        let text = "<!-- 第一行\n第二行 -->\n正文";
        let (spans, state) = highlight_with(text, false, None);
        assert!(state.is_none());
        assert!(
            spans.iter().any(|(range, class)| *class == CodeHighlightClass::Comment
                && text[range.clone()].contains("第一行")),
            "注释首行应着注释色: {spans:?}"
        );
        assert!(
            spans.iter().any(|(range, class)| *class == CodeHighlightClass::Comment
                && text[range.clone()].contains("第二行")),
            "注释续行应着注释色: {spans:?}"
        );
    }

    #[test]
    fn display_math_opens_only_at_line_start_like_the_parser() {
        // 与 display_math/块解析器同口径：`$$` 只在（≤3 缩进的）行首开公式；
        // 行中的金额、非行首的 $$ 都不进公式状态。
        let text = "价格 $$5 与 $6 不同";
        let (spans, state) = highlight_with(text, false, None);
        assert!(state.is_none());
        assert!(
            !spans
                .iter()
                .any(|(range, _)| text[range.clone()].contains("$$")),
            "行中 $$ 不该着公式色: {spans:?}"
        );

        let indented = "   $$x$$";
        assert_eq!(
            classes_covering(indented, "x"),
            vec![CodeHighlightClass::MarkdownCode],
            "≤3 空格缩进的行首 $$ 仍是公式"
        );
        let _ = state;
    }

    #[test]
    fn memlines_handles_empty_text_and_missing_trailing_newline() {
        // 行迭代是扫描器的地基：空文本、无换行尾、连续换行、多字节行界，
        // 任何一处差一都会让后续扫描整体错位。
        // 空文本产出一个空行（高亮侧无害：没有 span 会落在它上面）。
        let text = "";
        assert_eq!(MemLines::new(text).collect::<Vec<_>>(), vec![(0, "")]);

        let text = "a";
        let lines: Vec<_> = MemLines::new(text).collect();
        assert_eq!(lines, vec![(0, "a")]);

        let text = "甲\n\n乙";
        let lines: Vec<_> = MemLines::new(text).collect();
        assert_eq!(
            lines,
            vec![(0, "甲"), (4, ""), (5, "乙")],
            "甲 = 3 字节，第二个空行的起点在 4+1"
        );
    }

    #[test]
    fn list_marker_takes_at_most_nine_digits() {
        // CommonMark：有序记号最多 9 位数字，超过按普通文本。
        assert_eq!(
            class_at("123456789. 项", "123456789."),
            Some(CodeHighlightClass::MarkdownMarker)
        );
        assert!(classes_covering("1234567890. 项", "1234567890.").is_empty());
    }

    #[test]
    fn fence_info_string_cannot_contain_the_fence_char() {
        // 信息串里再出现定界字符就不是围栏（CommonMark），整行当正文。
        let text = "```a`b\n正文";
        let (spans, state) = highlight_with(text, false, None);
        assert!(state.is_none(), "信息串含反引号不该开栏");
        assert!(spans.is_empty(), "不该有围栏记号: {spans:?}");
    }

    #[test]
    fn latex_source_colors_commands_numbers_and_fences() {
        let text = "$$\nF = \\int_{0}^{1} ma\\\\\n$$";
        let spans: Vec<(Range<usize>, CodeHighlightClass)> = highlight_latex_source(text)
            .into_iter()
            .map(|span| (span.range, span.class))
            .collect();
        let class_of = |needle: &str| {
            let start = text.find(needle).expect("needle");
            spans
                .iter()
                .find(|(range, _)| range.start <= start && start < range.end)
                .map(|(_, class)| *class)
        };
        assert_eq!(class_of("$"), Some(CodeHighlightClass::MarkdownMarker));
        assert_eq!(class_of("\\int"), Some(CodeHighlightClass::Keyword));
        assert_eq!(class_of("0"), Some(CodeHighlightClass::Number));
        assert_eq!(class_of("}"), Some(CodeHighlightClass::Punctuation));
        assert_eq!(class_of("^"), Some(CodeHighlightClass::Operator));
        // `\\` 换行命令是标点；正文 ma 不着色。
        assert_eq!(class_of("\\\\"), Some(CodeHighlightClass::Punctuation));
        assert!(spans.iter().all(|(range, _)| &text[range.clone()] != "ma"));
        // 区间升序不重叠。
        for pair in spans.windows(2) {
            assert!(pair[0].0.end <= pair[1].0.start, "{pair:?}");
        }
    }

    #[test]
    fn latex_source_comments_run_to_end_of_line_and_multibyte_is_safe() {
        let text = "x^2 % 注释 αβ\n\\frac{1}{2}";
        let spans = highlight_latex_source(text);
        assert!(
            spans.iter().any(|span| span.class == CodeHighlightClass::Comment
                && text[span.range.clone()].contains("αβ")),
            "注释应吃到行尾且多字节安全"
        );
        for span in &spans {
            assert!(text.is_char_boundary(span.range.start) && text.is_char_boundary(span.range.end));
        }
    }

    #[test]
    fn spans_are_sorted() {
        let text = "## 标题\n```js\nvar a\n```\n---\n> 引\n- 项";
        let spans = highlight(text);
        for pair in spans.windows(2) {
            assert!(pair[0].0.start < pair[1].0.start, "spans 必须升序: {pair:?}");
        }
    }

    #[test]
    fn spans_are_sorted_and_disjoint_on_rich_documents() {
        // run 构建按序推进且假定互不重叠；这两条不变量破了，颜色就会串段。
        let text = "## 标题 `code` **粗** [链](url) > 引\n```js\nvar a\n```\n$$x$$\n\n第二段 [甲](u) 乙 **丙**\\n";
        let spans = highlight(text);
        for pair in spans.windows(2) {
            assert!(pair[0].0.start < pair[1].0.start, "spans 必须升序: {pair:?}");
            assert!(pair[0].0.end <= pair[1].0.start, "spans 不得重叠: {pair:?}");
        }
    }
}
