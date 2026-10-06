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

use super::code_highlight::{CodeHighlightClass, CodeHighlightSpan};

/// 跨块接缝的状态：本块开头是否正处于某个未闭合的围栏里。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum MarkdownSourceState {
    /// ``` / ~~~ 围栏内；`fence_len` 是开栏定界符的字节数。
    Fence {
        fence_char: char,
        fence_len: usize,
    },
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
    };
    let mut is_first_line = at_document_start;
    for (line_start, line) in MemLines::new(text) {
        scanner.scan_line(line_start, line, is_first_line);
        is_first_line = false;
    }
    let _ = at_document_start;
    scanner.flush_pending_paragraph();
    scanner.spans.sort_by_key(|span| span.range.start);
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

    fn scan_line(&mut self, line_start: usize, line: &str, _is_first_line: bool) {
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
            });
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
            return;
        }

        // 押成「可能是 setext 正文」：下一行是不是下划线，看到再发。
        self.flush_pending_paragraph();
        self.pending_paragraph = Some(line_start..line_end);
    }

    /// 处于围栏里的一行：闭合记号之外的内容暂不着色（行内与嵌套语言着色
    /// 随后的功能点接入）。
    fn scan_inside_state(&mut self, line_start: usize, line: &str, state: &MarkdownSourceState) {
        let MarkdownSourceState::Fence {
            fence_char,
            fence_len,
        } = state;
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
        }
    }

    /// 押着的 setext 候选行：下一行不是下划线时放弃（行内着色随后接入）。
    fn flush_pending_paragraph(&mut self) {
        self.pending_paragraph.take();
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
        // 没跟空格的 `#` 不是标题。
        assert_eq!(class_at("#tag 字", "#tag"), None);
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
    fn fence_opens_and_closes_with_marker_colour() {
        let text = "```rust\nlet a = 1;\n```\n正文";
        let (spans, state) = highlight_with(text, false, None);
        assert!(state.is_none());
        assert_eq!(
            class_at(text, "```rust"),
            Some(CodeHighlightClass::MarkdownMarker)
        );
        assert_eq!(class_at(text, "```",), Some(CodeHighlightClass::MarkdownMarker));
        // 围栏内容暂不着色（嵌套语言着色随后的功能点接入）。
        assert!(classes_covering(text, "let a = 1;").is_empty());
        let _ = spans;
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
    fn spans_are_sorted() {
        let text = "## 标题\n```js\nvar a\n```\n---\n> 引\n- 项";
        let spans = highlight(text);
        for pair in spans.windows(2) {
            assert!(pair[0].0.start < pair[1].0.start, "spans 必须升序: {pair:?}");
        }
    }
}
