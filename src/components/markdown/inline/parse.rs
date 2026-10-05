use super::*;

/// Result of a visible-text replacement operation, containing the
/// normalized tree and a mapping from pre-edit visible offsets to
/// post-edit tree offsets.
#[derive(Clone, Debug)]
pub struct InlineEditResult {
    pub tree: InlineTextTree,
    pub(crate) visible_to_normalized: Vec<usize>,
}

impl InlineEditResult {
    pub fn map_offset(&self, offset: usize) -> usize {
        self.visible_to_normalized
            .get(offset.min(self.visible_to_normalized.len().saturating_sub(1)))
            .copied()
            .unwrap_or(0)
    }

    pub fn map_range(&self, range: &Range<usize>) -> Range<usize> {
        self.map_offset(range.start)..self.map_offset(range.end)
    }
}

/// Ordered preference of delimiter variants used by the DP serializer.
/// Lower rank = more preferred.  Markdown delimiters are preferred over HTML
/// because they are shorter and more idiomatic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Delimiter {
    /// Markdown bold marker using either `*` or `_`.
    BoldMarkdown { marker: char },
    /// Markdown italic marker using either `*` or `_`.
    ItalicMarkdown { marker: char },
    /// Markdown strikethrough marker `~~`.
    StrikethroughMarkdown,
    /// Typora 式的标记文本 `==`。
    HighlightMarkdown,
    /// Markdown superscript marker `^`.
    SuperscriptMarkdown,
    /// Markdown subscript marker `~`.
    SubscriptMarkdown,
    /// HTML underline marker `<u>`.
    Underline,
    /// HTML superscript marker `<sup>`.
    SuperscriptHtml,
    /// HTML subscript marker `<sub>`.
    SubscriptHtml,
    /// HTML bold marker `<strong>`.
    BoldHtml,
    /// HTML italic marker `<em>`.
    ItalicHtml,
    /// Markdown code span marker using a selected backtick run length.
    CodeMarkdown { run_len: usize },
}

impl Delimiter {
    /// Returns the opening marker string.  For code spans this is `run_len`
    /// backticks; for emphasis it's `**`, `*`, `<u>`, etc.
    pub(crate) fn open(self) -> String {
        match self {
            Self::BoldMarkdown { marker } => marker.to_string().repeat(2),
            Self::ItalicMarkdown { marker } => marker.to_string(),
            Self::StrikethroughMarkdown => "~~".into(),
            Self::HighlightMarkdown => "==".into(),
            Self::SuperscriptMarkdown => "^".into(),
            Self::SubscriptMarkdown => "~".into(),
            Self::Underline => "<u>".into(),
            Self::SuperscriptHtml => "<sup>".into(),
            Self::SubscriptHtml => "<sub>".into(),
            Self::BoldHtml => "<strong>".into(),
            Self::ItalicHtml => "<em>".into(),
            Self::CodeMarkdown { run_len } => "`".repeat(run_len),
        }
    }

    pub(crate) fn close(self) -> String {
        match self {
            Self::BoldMarkdown { marker } => marker.to_string().repeat(2),
            Self::ItalicMarkdown { marker } => marker.to_string(),
            Self::StrikethroughMarkdown => "~~".into(),
            Self::HighlightMarkdown => "==".into(),
            Self::SuperscriptMarkdown => "^".into(),
            Self::SubscriptMarkdown => "~".into(),
            Self::Underline => "</u>".into(),
            Self::SuperscriptHtml => "</sup>".into(),
            Self::SubscriptHtml => "</sub>".into(),
            Self::BoldHtml => "</strong>".into(),
            Self::ItalicHtml => "</em>".into(),
            Self::CodeMarkdown { run_len } => "`".repeat(run_len),
        }
    }

    pub(crate) fn token_len(self) -> usize {
        match self {
            Self::CodeMarkdown { run_len } => run_len,
            other => other.open().chars().count(),
        }
    }

    pub(crate) fn preference_rank(self) -> u8 {
        match self {
            Self::BoldMarkdown { .. } => 0,
            Self::Underline => 1,
            Self::StrikethroughMarkdown => 2,
            Self::HighlightMarkdown => 9,
            Self::SuperscriptMarkdown | Self::SubscriptMarkdown => 3,
            Self::ItalicMarkdown { .. } => 4,
            Self::SuperscriptHtml | Self::SubscriptHtml => 5,
            Self::BoldHtml => 6,
            Self::ItalicHtml => 7,
            Self::CodeMarkdown { .. } => 8,
        }
    }

    pub(crate) fn is_html(self) -> bool {
        matches!(
            self,
            Self::BoldHtml | Self::ItalicHtml | Self::SuperscriptHtml | Self::SubscriptHtml
        )
    }
}

/// Inline style flag addressable by editing commands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StyleFlag {
    /// Bold text.
    Bold,
    /// Italic text.
    Italic,
    /// Underlined text.
    Underline,
    /// Strikethrough text.
    Strikethrough,
    /// Typora 式的标记文本（`==…==`）。
    Highlight,
    /// Inline code text.
    Code,
    /// Superscript text.
    Superscript,
    /// Subscript text.
    Subscript,
}

/// Source character plus style and byte range used by inline parsing.
#[derive(Clone)]
pub(crate) struct CharToken {
    pub(crate) ch: char,
    pub(crate) style: InlineStyle,
    pub(crate) html_style: Option<HtmlInlineStyle>,
    pub(crate) source_range: Range<usize>,
    /// 这个字符是字面文本，不能当语法读。编辑可见文本时有两种：反斜杠永远是字面字符
    /// （用户按一次 `\` 就该看到一个，连按两次不该塔缩成一个——用户报修：渲染模式里
    /// 打不出两个连续的反斜杠）；`escaped` 那种是源码用反斜杠换来的字面记号。
    /// 源码里没配对的定界符（`2 * 3` 的星号）不在其列：它仍然是语法候选，用户后来补
    /// 一颗星就该成强调。读 markdown 源文件时恒为 false，转义语义照旧。
    pub(crate) literal_char: bool,
    /// 这个字符是**源码里转义出来的**（`\*` 的那个 `*`）。可见文本里它和一个没配对的
    /// 定界符长得一样，可它已经不是语法候选了：再当定界符读一遍就是改写用户的写法。
    /// `InlineTextTree::escaped_offsets` 记的就是这些位置，重解析时原样带到新树里。
    pub(crate) escaped: bool,
    /// 这个字符所属的**转义序列**在源可见文本里的区间（同一次 `\` 转义的所有字符
    /// 共享同一个身份）。`emit_token` 靠它把同序列的连续字符并成一个转义区间——
    /// `\</u>` 记一段，而相邻的两次 `\*` `\*` 保持两段，序列化才不会洗写法。
    pub(crate) escaped_sequence: Option<std::ops::Range<u32>>,
}

/// Result of parsing a delimited inline region.
pub(crate) struct ParseResult {
    next_index: usize,
    closed: bool,
}

/// Builds the output fragments during normalization (marker parsing).
/// Keeps track of the visible-to-normalized offset mapping so that
/// selections and cursors can be mapped to the normalized tree.
pub(crate) struct NormalizeBuilder {
    pub(crate) fragments: Vec<InlineFragment>,
    pub(crate) visible_to_normalized: Vec<usize>,
    pub(crate) normalized_len: usize,
    /// 输出树里那些「由源码转义而来」的字符的可见字节偏移。
    pub(crate) escaped_offsets: Vec<std::ops::Range<u32>>,
    /// 上一颗带转义身份的 token 的序列身份：同序列的连续 token 并进同一个
    /// 区间，不同序列（`\*\*` 的两颗星）保持各自一段。
    pub(crate) last_escaped_sequence: Option<std::ops::Range<u32>>,
}

impl NormalizeBuilder {
    pub(crate) fn new(input_len: usize) -> Self {
        Self {
            fragments: Vec::new(),
            visible_to_normalized: vec![0; input_len + 1],
            normalized_len: 0,
            escaped_offsets: Vec::new(),
            last_escaped_sequence: None,
        }
    }

    pub(crate) fn drop_token(&mut self, token: &CharToken) {
        for boundary in token.source_range.start..=token.source_range.end {
            self.visible_to_normalized[boundary] = self.normalized_len;
        }
    }

    /// 推测性解析的快照。正文扫完仍未闭合时用 `rollback` 回到快照，否则这次尝试
    /// 已经写进 builder 的正文会留在输出里，调用方又把整段重新输出一遍（内容翻倍）。
    pub(crate) fn mark(&self) -> (usize, usize, usize) {
        (
            self.fragments.len(),
            self.normalized_len,
            self.escaped_offsets.len(),
        )
    }

    /// 回到 `mark` 时的状态。`visible_to_normalized` 不用备份：调用方会从起始位置
    /// 重新输出同一批 token，同一批边界会被重新写一遍。
    pub(crate) fn rollback(&mut self, mark: (usize, usize, usize)) {
        self.fragments.truncate(mark.0);
        self.normalized_len = mark.1;
        self.escaped_offsets.truncate(mark.2);
    }

    pub(crate) fn emit_token(
        &mut self,
        token: &CharToken,
        extra_style: InlineStyle,
        html_style: Option<HtmlInlineStyle>,
    ) {
        let mut style = token.style;
        if extra_style.bold {
            style.bold = true;
        }
        if extra_style.italic {
            style.italic = true;
        }
        if extra_style.underline {
            style.underline = true;
        }
        if extra_style.strikethrough {
            style.strikethrough = true;
        }
        if extra_style.highlight {
            style.highlight = true;
        }
        if extra_style.code {
            style.code = true;
        }
        if extra_style.has_script() {
            style.script = extra_style.script;
        }
        // 强调定界符的写法也要跟着样式往外传，不然 `_x_` 出来只剩「是斜体」这半个事实。
        style.emphasis_marker = extra_style.emphasis_marker.or(style.emphasis_marker);
        let html_style = merge_html_styles(html_style, token.html_style);

        let text = token.ch.to_string();
        let start = self.normalized_len;
        if token.escaped {
            // 一次转义序列记**一个**区间：同序列的连续字符并进去，序列化才好
            // 在区间起点补回一个反斜杠（`\</u>` 不会洗成 `\<\/\u\>`）；相邻的
            // 两次独立转义（`\*\*`）身份不同，保持两段。
            let end = (start + text.len()) as u32;
            if self.last_escaped_sequence == token.escaped_sequence
                && token.escaped_sequence.is_some()
                && self
                    .escaped_offsets
                    .last()
                    .is_some_and(|range| range.end == start as u32)
            {
                let last = self.escaped_offsets.last_mut().expect("checked above");
                last.end = end;
            } else {
                self.escaped_offsets.push(start as u32..end);
            }
            self.last_escaped_sequence = token.escaped_sequence.clone();
        } else {
            self.last_escaped_sequence = None;
        }
        for boundary in token.source_range.start..=token.source_range.end {
            self.visible_to_normalized[boundary] = start + (boundary - token.source_range.start);
        }
        self.normalized_len += text.len();

        if let Some(last) = self.fragments.last_mut()
            && last.style == style
            && last.html_style == html_style
            && last.link.is_none()
            && last.footnote.is_none()
            && last.math.is_none()
        {
            last.text.push_str(&text);
            return;
        }

        self.fragments.push(InlineFragment {
            text,
            style,
            html_style,
            link: None,
            footnote: None,
            math: None,
        });
    }

    pub(crate) fn emit_inline_math(
        &mut self,
        tokens: &[CharToken],
        math: InlineMath,
        extra_style: InlineStyle,
        extra_html_style: Option<HtmlInlineStyle>,
    ) {
        let source_start = tokens
            .first()
            .map(|token| token.source_range.start)
            .unwrap_or(0);
        let normalized_start = self.normalized_len;
        let source = math.source.clone();
        let visible_len = source.len();

        for token in tokens {
            let token_len = token.source_range.len();
            for delta in 0..=token_len {
                self.visible_to_normalized[token.source_range.start + delta] =
                    normalized_start + (token.source_range.start + delta - source_start);
            }
        }

        self.normalized_len += visible_len;
        self.fragments.push(InlineFragment {
            text: source,
            style: extra_style,
            html_style: extra_html_style,
            link: None,
            footnote: None,
            math: Some(math),
        });
    }
}

/// 把片段摊平成 token 流。`visible_text_mode` 为真时输入是**可见文本**（编辑路径）：
/// `escaped_offsets`（旧树的可见字节偏移）标出的那些字符是源码用反斜杠换来的字面记号，
/// 反斜杠自己也是字面字符。
pub(crate) fn flatten_tokens(
    fragments: &[InlineFragment],
    visible_text_mode: bool,
    escaped_offsets: &[std::ops::Range<u32>],
) -> Vec<CharToken> {
    let mut tokens = Vec::new();
    let mut visible_offset = 0usize;

    for fragment in fragments {
        // 脚注与行内公式存的是各自的 markdown 原文，序列化按原样写出，不是可见文本。
        let holds_raw_markdown = fragment.footnote.is_some() || fragment.math.is_some();
        for ch in fragment.text.chars() {
            let len = ch.len_utf8();
            // 这些区间按起点升序且互不重叠（解析与搬运都从左往右），二分定位
            // 起点，避免一块里转义多了就退化成每个字符扫一遍表。
            let escaped_sequence = if visible_text_mode && !holds_raw_markdown {
                escaped_sequence_at(escaped_offsets, visible_offset as u32)
            } else {
                None
            };
            let escaped = escaped_sequence.is_some();
            tokens.push(CharToken {
                ch,
                style: fragment.style,
                html_style: fragment.html_style,
                source_range: visible_offset..visible_offset + len,
                literal_char: visible_text_mode
                    && !holds_raw_markdown
                    && (ch == '\\' || escaped),
                escaped,
                escaped_sequence,
            });
            visible_offset += len;
        }
    }

    tokens
}

/// 可见字节偏移落在哪个转义序列区间里（区间按起点升序、互不重叠）。
fn escaped_sequence_at(
    escaped_offsets: &[std::ops::Range<u32>],
    offset: u32,
) -> Option<std::ops::Range<u32>> {
    let index = escaped_offsets.partition_point(|range| range.start <= offset);
    if index > 0 && offset < escaped_offsets[index - 1].end {
        Some(escaped_offsets[index - 1].clone())
    } else {
        None
    }
}

/// Recursive-descent parser that consumes [`CharToken`]s and reconstructs
/// the normalized inline tree.  Matching delimiters are consumed (dropped);
/// unmatched ones are emitted as literal text.  Nested styles are handled by
/// recursive calls that accumulate `extra_style`.
pub(crate) fn parse_until(
    tokens: &[CharToken],
    mut index: usize,
    end_delimiter: Option<Delimiter>,
    extra_style: InlineStyle,
    extra_html_style: Option<HtmlInlineStyle>,
    builder: &mut NormalizeBuilder,
    inside_code: bool,
    reference_definitions: &LinkReferenceDefinitions,
) -> ParseResult {
    let body_start = index;
    while index < tokens.len() {
        // 字面字符只做文本：既不开定界符，也不闭合正在扫的那一对。
        if tokens[index].literal_char {
            builder.emit_token(&tokens[index], extra_style, extra_html_style);
            index += 1;
            continue;
        }

        // Check for closing delimiter.
        if let Some(ref end_delim) = end_delimiter {
            let mut closed = match end_delim {
                Delimiter::CodeMarkdown { run_len } => {
                    tokens[index].ch == '`' && backtick_run_len(tokens, index) == *run_len
                }
                Delimiter::SuperscriptMarkdown => {
                    tokens[index].ch == '^' && can_close_emphasis(tokens, index)
                }
                Delimiter::SubscriptMarkdown => {
                    is_single_tilde_delimiter(tokens, index) && can_close_emphasis(tokens, index)
                }
                _ => {
                    matches_sequence(tokens, index, &end_delim.close())
                        && can_close_emphasis(tokens, index)
                }
            };

            // Emphasis spans must enclose at least one character; reject a
            // close at the very start of the body so empty spans stay literal.
            if closed && index == body_start && emphasis_requires_body(*end_delim) {
                closed = false;
            }

            if closed {
                let close_len = end_delim.close().chars().count();
                for token in &tokens[index..index + close_len] {
                    builder.drop_token(token);
                }
                return ParseResult {
                    next_index: index + close_len,
                    closed: true,
                };
            }
        }

        if !inside_code
            && let Some(next_index) =
                parse_inline_math(tokens, index, extra_style, extra_html_style, builder)
        {
            index = next_index;
            continue;
        }

        if !inside_code
            && tokens[index].ch == '\\'
            && !tokens[index].literal_char
            && let Some(escaped_len) = escaped_sequence_token_len(tokens, index)
        {
            builder.drop_token(&tokens[index]);
            let escaped_start = index + 1;
            let escaped_end = escaped_start + escaped_len;
            // 同一次转义的字符共享一个**序列身份**（源码里这一段转义的跨度），
            // emit_token 按身份把连续字符并成一个转义区间。
            let sequence = tokens[escaped_start..escaped_end]
                .first()
                .map(|first| {
                    let end = tokens[escaped_end - 1].source_range.end;
                    first.source_range.start as u32..end as u32
                })
                .unwrap_or(0..0);
            for token in &tokens[escaped_start..escaped_end] {
                // 转义来的字符从此是字面文本：把这一位带到输出树上，下一次重解析
                // 就不该再把它读成定界符（可见文本模式下靠它分辨 `\*` 与没配对的 `*`）。
                let mut escaped_token = token.clone();
                escaped_token.escaped = true;
                escaped_token.escaped_sequence = Some(sequence.clone());
                builder.emit_token(&escaped_token, extra_style, extra_html_style);
            }
            index = escaped_end;
            continue;
        }

        // Inside a code span, all text (including markers) is literal.
        if !inside_code {
            if tokens[index].ch == '['
                && let Some(next_index) =
                    parse_footnote_reference(tokens, index, extra_style, extra_html_style, builder)
            {
                index = next_index;
                continue;
            }

            if let Some(next_index) = parse_inline_link(
                tokens,
                index,
                extra_style,
                extra_html_style,
                builder,
                reference_definitions,
            ) {
                index = next_index;
                continue;
            }

            if tokens[index].ch == '<'
                && let Some(next_index) = parse_inline_html_container(
                    tokens,
                    index,
                    extra_style,
                    extra_html_style,
                    builder,
                    reference_definitions,
                )
            {
                index = next_index;
                continue;
            }

            if tokens[index].ch == '<'
                && let Some(next_index) = parse_autolink(
                    tokens,
                    index,
                    extra_style,
                    extra_html_style,
                    builder,
                    reference_definitions,
                )
            {
                index = next_index;
                continue;
            }

            if let Some(delimiter) = match_open_delimiter(tokens, index) {
                if has_closing_delimiter(tokens, index, delimiter) {
                    let mark = builder.mark();
                    for token in &tokens[index..index + delimiter.token_len()] {
                        builder.drop_token(token);
                    }
                    let inner_start = index + delimiter.token_len();
                    let is_code_delim = matches!(delimiter, Delimiter::CodeMarkdown { .. });
                    let parsed = parse_until(
                        tokens,
                        inner_start,
                        Some(delimiter),
                        extra_style.apply(delimiter),
                        extra_html_style,
                        builder,
                        is_code_delim,
                        reference_definitions,
                    );
                    if parsed.closed {
                        index = parsed.next_index;
                        continue;
                    }
                    // 预判有闭合、正文扫描却全部拒绝时回滚这次尝试，让定界符按字面
                    // 文本走（不回滚会同时留下正文和重扫的字面文本）。
                    builder.rollback(mark);
                } else if delimiter.token_len() > 1 {
                    // Keep an unclosed multi-character opener (`**`, `__`, `~~`,
                    // backtick run) literal as one unit. Emitting just its first
                    // char would let the rest open a shorter span (e.g. `**bold*`
                    // -> `*` + italic `bold`), which is committed on every
                    // keystroke and loses the intended bold.
                    for token in &tokens[index..index + delimiter.token_len()] {
                        builder.emit_token(token, extra_style, extra_html_style);
                    }
                    index += delimiter.token_len();
                    continue;
                }
            }
        }

        builder.emit_token(&tokens[index], extra_style, extra_html_style);
        index += 1;
    }

    ParseResult {
        next_index: tokens.len(),
        closed: false,
    }
}

pub(crate) fn parse_inline_math(
    tokens: &[CharToken],
    index: usize,
    extra_style: InlineStyle,
    extra_html_style: Option<HtmlInlineStyle>,
    builder: &mut NormalizeBuilder,
) -> Option<usize> {
    let (body_start, close_start, close_end, delimiter) = if tokens.get(index)?.ch == '$' {
        if matches_sequence(tokens, index, "$$") || token_is_backslash_escaped(tokens, index) {
            return None;
        }
        let close = locate_inline_dollar_math_close(tokens, index + 1)?;
        (index + 1, close, close, InlineMathDelimiter::Dollar)
    } else if matches_sequence(tokens, index, "\\(") {
        let close = locate_inline_paren_math_close(tokens, index + 2)?;
        (index + 2, close, close + 1, InlineMathDelimiter::Paren)
    } else {
        return None;
    };

    if body_start >= close_start {
        return None;
    }
    if tokens[body_start..close_start]
        .iter()
        .any(|token| token.ch == '\n' || token.ch == '\r')
    {
        return None;
    }
    if tokens[body_start].ch.is_whitespace() || tokens[close_start - 1].ch.is_whitespace() {
        return None;
    }

    let source = tokens_to_string(&tokens[index..=close_end]);
    let body = tokens_to_string(&tokens[body_start..close_start]);
    if looks_like_obvious_currency(tokens, index, close_end, &body) {
        return None;
    }

    let math = InlineMath {
        source,
        body,
        delimiter,
    };
    builder.emit_inline_math(
        &tokens[index..=close_end],
        math,
        extra_style,
        extra_html_style,
    );
    Some(close_end + 1)
}

pub(crate) fn locate_inline_dollar_math_close(tokens: &[CharToken], mut cursor: usize) -> Option<usize> {
    while cursor < tokens.len() {
        let token = &tokens[cursor];
        if token.ch == '\n' || token.ch == '\r' {
            return None;
        }
        if token.ch == '$'
            && !token_is_backslash_escaped(tokens, cursor)
            && !matches_sequence(tokens, cursor, "$$")
        {
            return Some(cursor);
        }
        cursor += 1;
    }
    None
}

pub(crate) fn locate_inline_paren_math_close(tokens: &[CharToken], mut cursor: usize) -> Option<usize> {
    while cursor + 1 < tokens.len() {
        if tokens[cursor].ch == '\n' || tokens[cursor].ch == '\r' {
            return None;
        }
        if matches_sequence(tokens, cursor, "\\)") {
            return Some(cursor);
        }
        cursor += 1;
    }
    None
}

pub(crate) fn token_is_backslash_escaped(tokens: &[CharToken], index: usize) -> bool {
    if index == 0 {
        return false;
    }
    let mut cursor = index;
    let mut slash_count = 0usize;
    while cursor > 0 && tokens[cursor - 1].ch == '\\' {
        // 可见文本模式下的反斜杠是字面字符，不算转义前缀。
        if tokens[cursor - 1].literal_char {
            break;
        }
        slash_count += 1;
        cursor -= 1;
    }
    slash_count % 2 == 1
}

pub(crate) fn looks_like_obvious_currency(
    tokens: &[CharToken],
    open_index: usize,
    close_index: usize,
    body: &str,
) -> bool {
    let prev_is_digit = open_index
        .checked_sub(1)
        .and_then(|idx| tokens.get(idx))
        .is_some_and(|token| token.ch.is_ascii_digit());
    let next_is_digit = tokens
        .get(close_index + 1)
        .is_some_and(|token| token.ch.is_ascii_digit());
    if prev_is_digit || next_is_digit {
        return true;
    }

    body.chars()
        .all(|ch| ch.is_ascii_digit() || matches!(ch, '.' | ',' | '_'))
        && body.chars().any(|ch| ch.is_ascii_digit())
        && body.len() > 1
}

pub(crate) fn parse_footnote_reference(
    tokens: &[CharToken],
    index: usize,
    extra_style: InlineStyle,
    extra_html_style: Option<HtmlInlineStyle>,
    builder: &mut NormalizeBuilder,
) -> Option<usize> {
    if tokens.get(index)?.ch != '[' || tokens.get(index + 1)?.ch != '^' {
        return None;
    }

    let mut cursor = index + 2;
    let end_index = loop {
        let token = tokens.get(cursor)?;
        // 可见文本模式下的反斜杠是字面字符，不能跳过后一个字符。
        if token.ch == '\\' && !token.literal_char {
            cursor += 2;
            continue;
        }
        if token.ch == ']' {
            break cursor;
        }
        cursor += 1;
    };

    let raw_markdown = tokens_to_string(&tokens[index..=end_index]);
    let id = parse_inline_footnote_reference(&raw_markdown)?;
    let fragments = vec![InlineFragment {
        text: raw_markdown.clone(),
        style: extra_style,
        html_style: extra_html_style,
        link: None,
        footnote: Some(InlineFootnoteReference {
            id,
            ordinal: None,
            occurrence_index: 0,
        }),
        math: None,
    }];

    let normalized_start = builder.normalized_len;
    let visible_len = raw_markdown.len();
    let normalized_end = normalized_start + visible_len;
    for token in &tokens[index..=end_index] {
        let token_len = token.source_range.len();
        for delta in 0..=token_len {
            builder.visible_to_normalized[token.source_range.start + delta] = normalized_start
                + (token.source_range.start + delta - tokens[index].source_range.start);
        }
    }

    for fragment in fragments {
        builder.normalized_len += fragment.text.len();
        if let Some(last) = builder.fragments.last_mut()
            && last.style == fragment.style
            && last.html_style == fragment.html_style
            && last.link == fragment.link
            && last.footnote == fragment.footnote
            && last.math.is_none()
            && fragment.math.is_none()
        {
            last.text.push_str(&fragment.text);
        } else {
            builder.fragments.push(fragment);
        }
    }

    for boundary in tokens[end_index].source_range.end..=tokens[end_index].source_range.end {
        builder.visible_to_normalized[boundary] = normalized_end;
    }

    Some(end_index + 1)
}

pub(crate) fn parse_inline_link(
    tokens: &[CharToken],
    index: usize,
    extra_style: InlineStyle,
    extra_html_style: Option<HtmlInlineStyle>,
    builder: &mut NormalizeBuilder,
    reference_definitions: &LinkReferenceDefinitions,
) -> Option<usize> {
    let located = locate_inline_link(tokens, index, reference_definitions)?;
    let label_end = located.label_end;
    let label_tokens = &tokens[index + 1..label_end];
    let label_markdown = tokens_to_string(label_tokens);
    let mut label_result = InlineTextTree::plain(label_markdown)
        .normalize_inline_syntax_with_link_references(reference_definitions);
    apply_extra_style_to_fragments(
        &mut label_result.tree.fragments,
        extra_style,
        extra_html_style,
    );
    let link = located.link;

    let normalized_start = builder.normalized_len;
    let label_len = label_result.tree.visible_len();

    for boundary in tokens[index].source_range.start..=tokens[index].source_range.end {
        builder.visible_to_normalized[boundary] = normalized_start;
    }

    let mut local_boundary = 0usize;
    for token in label_tokens {
        let token_len = token.source_range.len();
        for delta in 0..=token_len {
            builder.visible_to_normalized[token.source_range.start + delta] =
                normalized_start + label_result.visible_to_normalized[local_boundary + delta];
        }
        local_boundary += token_len;
    }

    let normalized_end = normalized_start + label_len;
    for token in &tokens[label_end..=located.end_index] {
        for boundary in token.source_range.start..=token.source_range.end {
            builder.visible_to_normalized[boundary] = normalized_end;
        }
    }

    for mut fragment in label_result.tree.fragments {
        fragment.link = Some(link.clone());
        fragment.footnote = None;
        fragment.math = None;
        builder.normalized_len += fragment.text.len();
        if let Some(last) = builder.fragments.last_mut()
            && last.style == fragment.style
            && last.html_style == fragment.html_style
            && last.link == fragment.link
            && last.footnote == fragment.footnote
            && last.math.is_none()
            && fragment.math.is_none()
        {
            last.text.push_str(&fragment.text);
        } else {
            builder.fragments.push(fragment);
        }
    }

    Some(located.end_index + 1)
}

pub(crate) fn parse_autolink(
    tokens: &[CharToken],
    index: usize,
    extra_style: InlineStyle,
    extra_html_style: Option<HtmlInlineStyle>,
    builder: &mut NormalizeBuilder,
    _reference_definitions: &LinkReferenceDefinitions,
) -> Option<usize> {
    let end_index = locate_autolink(tokens, index)?;
    let target_tokens = &tokens[index + 1..end_index];
    let target = tokens_to_string(target_tokens);
    let fragments = vec![InlineFragment {
        text: target.clone(),
        style: extra_style,
        html_style: extra_html_style,
        link: Some(InlineLink::Autolink {
            target: target.clone(),
        }),
        footnote: None,
        math: None,
    }];

    let normalized_start = builder.normalized_len;
    let target_len = target.len();

    for boundary in tokens[index].source_range.start..=tokens[index].source_range.end {
        builder.visible_to_normalized[boundary] = normalized_start;
    }

    let mut local_boundary = 0usize;
    for token in target_tokens {
        let token_len = token.source_range.len();
        for delta in 0..=token_len {
            builder.visible_to_normalized[token.source_range.start + delta] =
                normalized_start + local_boundary + delta;
        }
        local_boundary += token_len;
    }

    let normalized_end = normalized_start + target_len;
    for boundary in tokens[end_index].source_range.start..=tokens[end_index].source_range.end {
        builder.visible_to_normalized[boundary] = normalized_end;
    }

    for fragment in fragments {
        builder.normalized_len += fragment.text.len();
        if let Some(last) = builder.fragments.last_mut()
            && last.style == fragment.style
            && last.html_style == fragment.html_style
            && last.link == fragment.link
            && last.footnote == fragment.footnote
            && last.math.is_none()
            && fragment.math.is_none()
        {
            last.text.push_str(&fragment.text);
        } else {
            builder.fragments.push(fragment);
        }
    }

    Some(end_index + 1)
}

#[derive(Clone, Debug)]
pub(crate) struct InlineHtmlTag {
    pub(crate) name: String,
    pub(crate) attrs: Vec<HtmlAttr>,
    pub(crate) end_index: usize,
    pub(crate) self_closing: bool,
}

pub(crate) fn parse_inline_html_container(
    tokens: &[CharToken],
    index: usize,
    extra_style: InlineStyle,
    extra_html_style: Option<HtmlInlineStyle>,
    builder: &mut NormalizeBuilder,
    reference_definitions: &LinkReferenceDefinitions,
) -> Option<usize> {
    let tag = locate_inline_html_open_tag(tokens, index)?;
    if tag.self_closing || !is_inline_tag(&tag.name) || has_dangerous_attrs(&tag.attrs) {
        return None;
    }

    let (close_start, close_end) =
        locate_matching_inline_html_close(tokens, tag.end_index + 1, &tag.name)?;
    let tag_style = inline_html_semantic_style(&tag.name, extra_style);
    let html_style = merge_html_styles(extra_html_style, inline_html_style(&tag));
    if tag_style == extra_style && html_style == extra_html_style {
        return None;
    }

    for token in &tokens[index..=tag.end_index] {
        builder.drop_token(token);
    }
    let _ = parse_until(
        &tokens[tag.end_index + 1..close_start],
        0,
        None,
        tag_style,
        html_style,
        builder,
        false,
        reference_definitions,
    );
    for token in &tokens[close_start..=close_end] {
        builder.drop_token(token);
    }

    Some(close_end + 1)
}

pub(crate) fn inline_html_semantic_style(name: &str, style: InlineStyle) -> InlineStyle {
    match name {
        "strong" | "b" => style.with_bold(),
        "em" | "i" => style.with_italic(),
        "u" | "ins" => style.with_underline(),
        "del" => style.with_strikethrough(),
        "code" | "kbd" => style.with_code(),
        "sup" => style.with_superscript(),
        "sub" => style.with_subscript(),
        _ => style,
    }
}

pub(crate) fn inline_html_style(tag: &InlineHtmlTag) -> Option<HtmlInlineStyle> {
    let node = HtmlNode {
        kind: HtmlNodeKind::InlineSemantic,
        tag_name: tag.name.clone(),
        attrs: tag.attrs.clone(),
        children: Vec::new(),
        raw_source: String::new(),
    };
    let style = style_for_node(&node);
    (!style.is_empty()).then_some(style)
}

pub(crate) fn merge_html_styles(
    parent: Option<HtmlInlineStyle>,
    child: Option<HtmlInlineStyle>,
) -> Option<HtmlInlineStyle> {
    let mut merged = parent.unwrap_or_default();
    if let Some(child) = child {
        if child.color.is_some() {
            merged.color = child.color;
        }
        if child.background_color.is_some() {
            merged.background_color = child.background_color;
        }
        if child.font_size.is_some() {
            merged.font_size = child.font_size;
        }
    }

    (!merged.is_empty()).then_some(merged)
}

