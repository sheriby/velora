use super::*;

/// Markdown and HTML delimiter variants, avoiding ambiguous `****` runs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InlineTextTree {
    pub(crate) fragments: Vec<InlineFragment>,
    /// 源码里由反斜杠转义而来的那些**序列**，在这棵树**可见文本**里的字节区间
    /// （升序、互不重叠；`\*` 是一位，`\</u>` 是四位——一次转义一个区间）。
    ///
    /// 可见文本分不出 `2 * 3` 里那颗没配对的星号和 `\*不强调\*` 里的星号——前者是语法
    /// 候选，用户补一颗就该成强调；后者已经不是语法，重读一遍会把用户的写法改掉，还会
    /// 把可见长度弄短，逼得编辑器放弃按区间写回。这份区别只有源码知道，所以解析时把它
    /// 记下来，往后的每次编辑与重解析都原样带过去。序列化只在区间**起点**补回一个
    /// 反斜杠，未编辑块才能逐字节还原用户写法。
    pub(crate) escaped_offsets: Vec<std::ops::Range<u32>>,
}

impl InlineTextTree {
    pub fn plain(text: impl Into<String>) -> Self {
        Self::from_fragments(vec![InlineFragment {
            text: text.into(),
            style: InlineStyle::default(),
            html_style: None,
            link: None,
            footnote: None,
            math: None,
        }])
    }

    /// 把「强调用哪个记号」这份写法数据清回默认（`*` / `**`）。
    ///
    /// 只有显式的「格式化文档」会调它：打字、保存、撤销这些隐式路径一律不许碰写法。
    /// `escaped_offsets` 不动——反斜杠转义是**语义**（那是个字面星号），抹掉它会让
    /// `\*不强调\*` 变成强调，改的就不是写法而是文档的意思了。
    pub fn reset_emphasis_markers(&mut self) {
        for fragment in &mut self.fragments {
            fragment.style.emphasis_marker = None;
        }
    }

    /// Parse marker-based Markdown into the internal fragment representation.
    ///
    /// Markers (`**`, `*`, `<u>`, `<strong>`, `<em>`) are consumed and
    /// converted to [`InlineStyle`] flags on adjacent fragments.  The
    /// markers themselves are never stored — the tree holds only text
    /// content and style attributes.
    pub fn from_markdown(markdown: &str) -> Self {
        Self::from_markdown_with_link_references(markdown, &LinkReferenceDefinitions::default())
    }

    pub fn from_markdown_with_link_references(
        markdown: &str,
        reference_definitions: &LinkReferenceDefinitions,
    ) -> Self {
        let mut tree = Self::plain(markdown)
            .normalize_inline_syntax_with_link_references(reference_definitions)
            .tree;
        tree.normalize_code_spans();
        tree
    }

    /// Code-span content normalization:
    /// - CRLF/CR line endings are normalized to LF so inline code can render
    ///   across hard lines in the editor.
    /// - If the content is not entirely spaces and both starts AND ends with
    ///   a single space, those two spaces are stripped.
    fn normalize_code_spans(&mut self) {
        for fragment in &mut self.fragments {
            if fragment.style.code && !fragment.text.is_empty() {
                let mut s = fragment.text.replace("\r\n", "\n").replace('\r', "\n");
                let all_space = s.chars().all(|c| c == ' ');
                if !all_space && s.starts_with(' ') && s.ends_with(' ') {
                    s.remove(0);
                    s.pop();
                }
                fragment.text = s;
            }
        }
        self.normalize_fragments();
    }

    pub fn from_fragments(fragments: Vec<InlineFragment>) -> Self {
        let mut tree = Self {
            fragments,
            escaped_offsets: Vec::new(),
        };
        tree.normalize_fragments();
        tree
    }

    pub fn visible_text(&self) -> String {
        let mut text = String::new();
        for fragment in &self.fragments {
            text.push_str(&fragment.text);
        }
        text
    }

    pub fn visible_len(&self) -> usize {
        self.fragments
            .iter()
            .map(|fragment| fragment.text.len())
            .sum()
    }

    pub(crate) fn has_source_preserving_links(&self) -> bool {
        self.fragments.iter().any(|fragment| {
            fragment
                .link
                .as_ref()
                .is_some_and(InlineLink::is_source_preserving)
                || fragment.footnote.is_some()
                || fragment.math.is_some()
        })
    }

    /// Whether any fragment carries an inline `[label](url)` link. Unlike
    /// reference/autolink links these are not "source preserving", but their
    /// `[...](...)` markers are still stripped from the fragment text, so an
    /// edit that re-derives the tree from visible text alone would drop them.
    pub(crate) fn has_inline_links(&self) -> bool {
        self.fragments
            .iter()
            .any(|fragment| matches!(fragment.link, Some(InlineLink::Inline { .. })))
    }

    pub(crate) fn has_mixed_inline_visuals(&self) -> bool {
        self.fragments.iter().any(|fragment| {
            fragment.math.is_some()
                || fragment.style.has_script()
                // `![alt](src)` spans inside a paragraph render as inline
                // image widgets on the same mixed-segment path. Not inside
                // inline code: code-span content is literal text.
                || (!fragment.style.code && fragment.text.contains("!["))
        })
    }

    pub(crate) fn has_footnote_references(&self) -> bool {
        self.fragments
            .iter()
            .any(|fragment| fragment.footnote.is_some())
    }

    /// 编辑后的重解析只认得出 `[^1]` 这个源码形状，序号要等注册表贴上来。
    /// 「还没贴上」就是这个谓词：它决定了一次同步有没有活可干。
    pub(crate) fn has_unresolved_footnote_references(&self) -> bool {
        self.fragments.iter().any(|fragment| {
            fragment
                .footnote
                .as_ref()
                .is_some_and(|footnote| footnote.ordinal.is_none())
        })
    }

    pub(crate) fn apply_footnote_reference_state(
        &mut self,
        mut resolve: impl FnMut(&str) -> Option<(usize, usize)>,
    ) {
        for fragment in &mut self.fragments {
            let Some(footnote) = fragment.footnote.as_mut() else {
                continue;
            };
            if let Some((ordinal, occurrence_index)) = resolve(&footnote.id) {
                footnote.ordinal = Some(ordinal);
                footnote.occurrence_index = occurrence_index;
                fragment.text = superscript_ordinal(ordinal);
            } else {
                footnote.ordinal = None;
                footnote.occurrence_index = 0;
                fragment.text = footnote.raw_markdown();
            }
        }
        self.normalize_fragments();
    }

    pub fn render_cache(&self) -> InlineRenderCache {
        InlineRenderCache::from_tree(self)
    }

    /// 树级 `escaped_offsets`（转义序列区间）里起点落在
    /// `[start, start+run 可见长度)` 内的，换算成 run 局部坐标。
    /// 序列不会骑在片段边界上（同一次转义的字符样式相同、合并进同一片段），
    /// 所以只按起点判属就行。供序列化器决定哪些位置保留反斜杠。
    fn escaped_within(
        tree_escaped: &[std::ops::Range<u32>],
        start: usize,
        fragments: &[InlineFragment],
    ) -> Vec<u32> {
        let run_visible_len: u32 = fragments
            .iter()
            .map(|fragment| fragment.text.len() as u32)
            .sum();
        let start = start as u32;
        let end = start + run_visible_len;
        tree_escaped
            .iter()
            .filter(|range| range.start >= start && range.start < end)
            .map(|range| range.start - start)
            .collect()
    }

    /// Serialize fragments back to Markdown text with optimal delimiter choices.
    ///
    /// Each fragment's style flags determine which markers surround its text.
    /// This is the export side of the I/O boundary; the internal fragment
    /// representation never stores raw marker characters.
    pub fn serialize_markdown(&self) -> String {
        if let [fragment] = self.fragments.as_slice()
            && fragment.style == InlineStyle::default()
            && fragment.html_style.is_none()
            && fragment.link.is_none()
            && fragment.footnote.is_none()
            && fragment.math.is_none()
            // 转义写法记在 `escaped_offsets` 里，不在可见文本的字面上：有账就必须
            // 走慢路径，否则 `\!x` 序列化会吞掉用户写的那个反斜杠（逐字节还原被破坏）。
            && self.escaped_offsets.is_empty()
            && !fragment
                .text
                .bytes()
                .any(|byte| matches!(byte, b'\\' | b'*' | b'_' | b'~' | b'^' | b'`' | b'<'))
        {
            return fragment.text.clone();
        }
        self.serialize_markdown_plain()
    }

    /// 只产出 markdown 字符串的序列化（不建偏移映射）。
    ///
    /// [`Self::markdown_offset_map`] 要为每个可见字节和每个 markdown 字节各写
    /// 一张映射表；保存/撤销/导出只要字符串，却为此付出整篇字节数的向量分配
    /// （1 MiB 文档实测 779ms，占单次按键耗时的大半）。这里与映射版本共用同一
    /// 套分隔符与转义规则，靠 `serialize_markdown_matches_offset_map` 用例守住
    /// 两条路径输出一致。
    pub(crate) fn serialize_markdown_plain(&self) -> String {
        if self.fragments.is_empty() {
            return String::new();
        }

        let mut output = String::new();
        // 走查时跟着累计可见字节偏移：`escaped_offsets` 记的是树级可见位置，
        // 交给 run 序列化器前要切成本 run 局部的。
        let mut visible_start = 0usize;
        let mut index = 0usize;
        while index < self.fragments.len() {
            if let Some(footnote) = self.fragments[index].footnote.clone() {
                output.push_str(&footnote.raw_markdown());
                visible_start += self.fragments[index].text.len();
                index += 1;
                continue;
            }

            if let Some(math) = self.fragments[index].math.clone() {
                output.push_str(&math.source);
                visible_start += self.fragments[index].text.len();
                index += 1;
                continue;
            }

            let link = self.fragments[index].link.clone();
            let mut end = index + 1;
            while end < self.fragments.len()
                && self.fragments[end].link == link
                && self.fragments[end].footnote.is_none()
                && self.fragments[end].math.is_none()
            {
                end += 1;
            }

            let run_markdown = serialize_fragment_run_markdown(
                &self.fragments[index..end],
                &Self::escaped_within(&self.escaped_offsets, visible_start, &self.fragments[index..end]),
            );
            if let Some(link) = link {
                output.push_str(link.open_marker());
                output.push_str(&run_markdown);
                if let Some(middle_marker) = link.middle_marker() {
                    output.push_str(middle_marker);
                }
                if let Some(editable_text) = link.editable_text().as_deref() {
                    output.push_str(editable_text);
                }
                output.push_str(link.close_marker());
            } else {
                output.push_str(&run_markdown);
            }

            for fragment in &self.fragments[index..end] {
                visible_start += fragment.text.len();
            }
            index = end;
        }
        output
    }

    pub(crate) fn markdown_offset_map(&self) -> InlineMarkdownOffsetMap {
        if self.fragments.is_empty() {
            return InlineMarkdownOffsetMap {
                markdown: String::new(),
                visible_to_markdown: vec![0],
                markdown_to_visible: vec![0],
            };
        }

        let mut output = String::new();
        let mut visible_to_markdown = vec![0; self.visible_len() + 1];
        let mut markdown_to_visible = vec![0];
        let mut visible_cursor = 0usize;
        let mut index = 0usize;
        while index < self.fragments.len() {
            if let Some(footnote) = self.fragments[index].footnote.clone() {
                let raw_markdown = footnote.raw_markdown();
                let raw_len = raw_markdown.len();
                let run_visible_len = self.fragments[index].text.len();
                let run_start = output.len();
                output.push_str(&raw_markdown);
                let run_end = output.len();

                for local_visible in 0..=run_visible_len {
                    let mapped = if run_visible_len == 0 {
                        0
                    } else {
                        (raw_len * local_visible) / run_visible_len
                    };
                    visible_to_markdown[visible_cursor + local_visible] = run_start + mapped;
                }

                markdown_to_visible.resize(run_end + 1, visible_cursor);
                for local_markdown in 0..=raw_len {
                    let mapped = if raw_len == 0 {
                        0
                    } else {
                        (run_visible_len * local_markdown) / raw_len
                    };
                    markdown_to_visible[run_start + local_markdown] = visible_cursor + mapped;
                }

                visible_cursor += run_visible_len;
                index += 1;
                continue;
            }

            if let Some(math) = self.fragments[index].math.clone() {
                let raw_markdown = math.source;
                let raw_len = raw_markdown.len();
                let run_visible_len = self.fragments[index].text.len();
                let run_start = output.len();
                output.push_str(&raw_markdown);
                let run_end = output.len();

                for local_visible in 0..=run_visible_len {
                    visible_to_markdown[visible_cursor + local_visible] =
                        run_start + local_visible.min(raw_len);
                }

                markdown_to_visible.resize(run_end + 1, visible_cursor);
                for local_markdown in 0..=raw_len {
                    markdown_to_visible[run_start + local_markdown] =
                        visible_cursor + local_markdown.min(run_visible_len);
                }

                visible_cursor += run_visible_len;
                index += 1;
                continue;
            }

            let link = self.fragments[index].link.clone();
            let mut end = index + 1;
            while end < self.fragments.len()
                && self.fragments[end].link == link
                && self.fragments[end].footnote.is_none()
                && self.fragments[end].math.is_none()
            {
                end += 1;
            }

            let run_map = serialize_fragment_run_markdown_with_offset_map(
                &self.fragments[index..end],
                &Self::escaped_within(&self.escaped_offsets, visible_cursor, &self.fragments[index..end]),
            );
            if let Some(link) = link {
                let run_visible_len = run_map.visible_to_markdown.len().saturating_sub(1);
                let link_start = output.len();
                let editable_text = link.editable_text();
                output.push_str(link.open_marker());
                output.push_str(run_map.markdown());
                if let Some(middle_marker) = link.middle_marker() {
                    output.push_str(middle_marker);
                }
                if let Some(editable_text) = editable_text.as_deref() {
                    output.push_str(editable_text);
                }
                output.push_str(link.close_marker());
                let link_end = output.len();
                let label_markdown_start = link_start + link.open_marker().len();

                for local_visible in 0..=run_visible_len {
                    visible_to_markdown[visible_cursor + local_visible] =
                        label_markdown_start + run_map.visible_to_markdown_offset(local_visible);
                }

                markdown_to_visible.resize(link_end + 1, visible_cursor);
                for local in 0..=link.open_marker().len() {
                    markdown_to_visible[link_start + local] = visible_cursor;
                }
                for local_markdown in 0..run_map.markdown().len() {
                    markdown_to_visible[label_markdown_start + local_markdown] =
                        visible_cursor + run_map.markdown_to_visible_offset(local_markdown);
                }

                let label_markdown_end = label_markdown_start + run_map.markdown().len();
                markdown_to_visible[label_markdown_end] = visible_cursor + run_visible_len;

                let suffix_start = label_markdown_end;
                let suffix_len = link.middle_marker().map(str::len).unwrap_or(0)
                    + editable_text.as_ref().map(String::len).unwrap_or(0)
                    + link.close_marker().len();
                for local in 0..=suffix_len {
                    markdown_to_visible[suffix_start + local] = visible_cursor + run_visible_len;
                }
                visible_cursor += run_visible_len;
            } else {
                let run_start = output.len();
                output.push_str(run_map.markdown());
                let run_end = output.len();

                let run_visible_len = run_map.visible_to_markdown.len().saturating_sub(1);
                for local_visible in 0..=run_visible_len {
                    visible_to_markdown[visible_cursor + local_visible] =
                        run_start + run_map.visible_to_markdown_offset(local_visible);
                }

                markdown_to_visible.resize(run_end + 1, visible_cursor);
                for local_markdown in 0..=run_map.markdown().len() {
                    markdown_to_visible[run_start + local_markdown] =
                        visible_cursor + run_map.markdown_to_visible_offset(local_markdown);
                }
                visible_cursor += run_visible_len;
            }

            index = end;
        }

        InlineMarkdownOffsetMap {
            markdown: output,
            visible_to_markdown,
            markdown_to_visible,
        }
    }
}

/// 与 [`serialize_fragment_run_markdown_with_offset_map`] 同一套分隔符选择与
/// 转义规则，只产出 markdown 字符串（不建映射表）。两条路径的一致性由
/// `serialize_markdown_matches_offset_map` 用例守住。`escaped`：本 run 可见
/// 文本里「源码本就转义」的字节偏移（升序、局部坐标），只有这些位置写反斜杠。
fn serialize_fragment_run_markdown(fragments: &[InlineFragment], escaped: &[u32]) -> String {
    if fragments.is_empty() {
        return String::new();
    }

    let stacks = choose_fragment_stacks(fragments);
    let mut output = String::new();
    let mut current_stack: Vec<Delimiter> = Vec::new();
    let mut current_html_style: Option<HtmlInlineStyle> = None;
    let mut fragment_visible_start = 0usize;

    for (fragment, next_stack) in fragments.iter().zip(stacks.iter()) {
        if current_html_style != fragment.html_style {
            output.push_str(&stack_transition_string(&current_stack, &[]));
            current_stack.clear();

            if current_html_style.is_some() {
                output.push_str("</span>");
            }
            if let Some(style) = fragment.html_style
                && let Some(marker) = html_style_open_marker(style)
            {
                output.push_str(&marker);
            }
            current_html_style = fragment.html_style;
        }

        output.push_str(&stack_transition_string(&current_stack, next_stack));

        if let Some(math) = fragment.math.as_ref() {
            output.push_str(&math.source);
        } else if fragment.style.line_break {
            for _ in fragment.text.chars() {
                output.push_str("<br>");
            }
        } else if fragment.style.code {
            output.push_str(&escape_code_span_text(&fragment.text));
        } else {
            let local = escaped_within(escaped, fragment_visible_start, &fragment.text);
            output.push_str(&escape_literal_text(&fragment.text, &local));
        }

        fragment_visible_start += fragment.text.len();
        current_stack = next_stack.clone();
    }

    output.push_str(&stack_transition_string(&current_stack, &[]));
    if current_html_style.is_some() {
        output.push_str("</span>");
    }

    output
}

/// 把 run 级的转义位置切到单个片段的局部坐标（`start`：片段在 run 可见文本
/// 里的起点）。片段外的位置丢弃，片段内的整体左移。
fn escaped_within(escaped: &[u32], start: usize, text: &str) -> Vec<u32> {
    let end = (start + text.len()) as u32;
    let start = start as u32;
    escaped
        .iter()
        .copied()
        .filter(|offset| *offset >= start && *offset < end)
        .map(|offset| offset - start)
        .collect()
}

fn serialize_fragment_run_markdown_with_offset_map(
    fragments: &[InlineFragment],
    escaped: &[u32],
) -> InlineMarkdownOffsetMap {
    if fragments.is_empty() {
        return InlineMarkdownOffsetMap {
            markdown: String::new(),
            visible_to_markdown: vec![0],
            markdown_to_visible: vec![0],
        };
    }

    let stacks = choose_fragment_stacks(fragments);
    let mut output = String::new();
    let total_visible_len = fragments
        .iter()
        .map(|fragment| fragment.text.len())
        .sum::<usize>();
    let mut visible_to_markdown = vec![0; total_visible_len + 1];
    let mut markdown_to_visible = vec![0];
    let mut current_stack: Vec<Delimiter> = Vec::new();
    let mut current_html_style: Option<HtmlInlineStyle> = None;
    let mut visible_cursor = 0usize;

    for (fragment, next_stack) in fragments.iter().zip(stacks.iter()) {
        if current_html_style != fragment.html_style {
            let transition = stack_transition_string(&current_stack, &[]);
            push_markdown_marker(
                &mut output,
                &mut markdown_to_visible,
                visible_cursor,
                &transition,
            );
            current_stack.clear();

            if current_html_style.is_some() {
                push_markdown_marker(
                    &mut output,
                    &mut markdown_to_visible,
                    visible_cursor,
                    "</span>",
                );
            }
            if let Some(style) = fragment.html_style
                && let Some(marker) = html_style_open_marker(style)
            {
                push_markdown_marker(
                    &mut output,
                    &mut markdown_to_visible,
                    visible_cursor,
                    &marker,
                );
            }
            current_html_style = fragment.html_style;
        }

        let transition = stack_transition_string(&current_stack, next_stack);
        let transition_start = output.len();
        output.push_str(&transition);
        markdown_to_visible.resize(output.len() + 1, visible_cursor);
        for local in 0..=transition.len() {
            markdown_to_visible[transition_start + local] = visible_cursor;
        }

        let escaped = if let Some(math) = fragment.math.as_ref() {
            identity_text_with_offset_map(&math.source)
        } else if fragment.style.line_break {
            html_line_break_offset_map(fragment.text.chars().count())
        } else if fragment.style.code {
            escape_code_span_text_with_offset_map(&fragment.text)
        } else {
            let local = escaped_within(escaped, visible_cursor, &fragment.text);
            escape_literal_text_with_offset_map(&fragment.text, &local)
        };
        let escaped_start = output.len();
        output.push_str(escaped.markdown());
        for local_visible in 0..=fragment.text.len() {
            visible_to_markdown[visible_cursor + local_visible] =
                escaped_start + escaped.visible_to_markdown_offset(local_visible);
        }
        markdown_to_visible.resize(output.len() + 1, visible_cursor);
        for local_markdown in 0..=escaped.markdown().len() {
            markdown_to_visible[escaped_start + local_markdown] =
                visible_cursor + escaped.markdown_to_visible_offset(local_markdown);
        }
        visible_cursor += fragment.text.len();
        current_stack = next_stack.clone();
    }

    let transition = stack_transition_string(&current_stack, &[]);
    push_markdown_marker(
        &mut output,
        &mut markdown_to_visible,
        visible_cursor,
        &transition,
    );
    if current_html_style.is_some() {
        push_markdown_marker(
            &mut output,
            &mut markdown_to_visible,
            visible_cursor,
            "</span>",
        );
    }

    InlineMarkdownOffsetMap {
        markdown: output,
        visible_to_markdown,
        markdown_to_visible,
    }
}

fn push_markdown_marker(
    output: &mut String,
    markdown_to_visible: &mut Vec<usize>,
    visible_cursor: usize,
    marker: &str,
) {
    if marker.is_empty() {
        return;
    }
    let marker_start = output.len();
    output.push_str(marker);
    markdown_to_visible.resize(output.len() + 1, visible_cursor);
    for local in 0..=marker.len() {
        markdown_to_visible[marker_start + local] = visible_cursor;
    }
}

fn identity_text_with_offset_map(text: &str) -> InlineMarkdownOffsetMap {
    InlineMarkdownOffsetMap {
        markdown: text.to_string(),
        visible_to_markdown: (0..=text.len()).collect(),
        markdown_to_visible: (0..=text.len()).collect(),
    }
}

/// `break_count` 个断点各序列化成一个 `<br>`：可见 1 字节 ↔ markdown 4 字节，
/// 与 [`serialize_fragment_run_markdown`] 里 `line_break` 分支的写法保持一致
/// （两条序列化路径不许漂移，见 `serialize_markdown_matches_offset_map`）。
fn html_line_break_offset_map(break_count: usize) -> InlineMarkdownOffsetMap {
    let markdown = "<br>".repeat(break_count);
    InlineMarkdownOffsetMap {
        markdown,
        visible_to_markdown: (0..=break_count).map(|index| index * 4).collect(),
        markdown_to_visible: (0..=(break_count * 4))
            .map(|offset| (offset / 4).min(break_count))
            .collect(),
    }
}

fn html_style_open_marker(style: HtmlInlineStyle) -> Option<String> {
    style
        .to_css()
        .map(|css| format!("<span style=\"{}\">", escape_html_attr(&css)))
}

fn escape_html_attr(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '"' => escaped.push_str("&quot;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

impl InlineTextTree {
    pub fn split_at(&self, offset: usize) -> (Self, Self) {
        let clamped = offset.min(self.visible_len());
        let mut left = Vec::new();
        let mut right = Vec::new();
        let mut consumed = 0;

        for fragment in &self.fragments {
            let fragment_len = fragment.text.len();
            let fragment_start = consumed;
            let fragment_end = fragment_start + fragment_len;

            if clamped <= fragment_start {
                right.push(fragment.clone());
            } else if clamped >= fragment_end {
                left.push(fragment.clone());
            } else {
                let split_offset = clamp_to_char_boundary(&fragment.text, clamped - fragment_start);
                if split_offset > 0 {
                    left.push(InlineFragment {
                        text: fragment.text[..split_offset].to_string(),
                        style: fragment.style,
                        html_style: fragment.html_style,
                        link: fragment.link.clone(),
                        footnote: fragment.footnote.clone(),
                        math: None,
                    });
                }
                if split_offset < fragment_len {
                    right.push(InlineFragment {
                        text: fragment.text[split_offset..].to_string(),
                        style: fragment.style,
                        html_style: fragment.html_style,
                        link: fragment.link.clone(),
                        footnote: fragment.footnote.clone(),
                        math: None,
                    });
                }
            }

            consumed = fragment_end;
        }

        let mut left = Self::from_fragments(left);
        let mut right = Self::from_fragments(right);
        // 转义序列跟着切口分两半：整个落在左半的原样保留；骑在切口上的拆成
        // 两段（切口落在字符边界上，见 clamp_to_char_boundary）；右半整体左移。
        let mut left_ranges = Vec::new();
        let mut right_ranges = Vec::new();
        for range in &self.escaped_offsets {
            let (start, end) = (range.start as usize, range.end as usize);
            if end <= clamped {
                left_ranges.push(range.clone());
            } else if start >= clamped {
                right_ranges.push((start - clamped) as u32..(end - clamped) as u32);
            } else {
                left_ranges.push(range.start..clamped as u32);
                right_ranges.push(0..(end - clamped) as u32);
            }
        }
        left.escaped_offsets = left_ranges;
        right.escaped_offsets = right_ranges;
        (left, right)
    }

    pub fn append_tree(&mut self, other: Self) {
        let base = self.visible_len() as u32;
        self.escaped_offsets.extend(
            other
                .escaped_offsets
                .iter()
                .map(|range| (range.start + base)..(range.end + base)),
        );
        self.fragments.extend(other.fragments);
        self.normalize_fragments();
    }

    pub(crate) fn replace_fragment_range(
        &mut self,
        range: Range<usize>,
        replacement: Vec<InlineFragment>,
    ) {
        self.fragments.splice(range, replacement);
        // 片段整体换了一批，可见偏移全变了，而替换内容（链接的一段）不是从转义解析
        // 来的——这份记录留着只会指错位置，宁可丢掉转义保护。
        self.escaped_offsets.clear();
        self.normalize_fragments();
    }

    pub fn remove_visible_prefix(&mut self, prefix_len: usize) {
        let (_, tail) = self.split_at(prefix_len);
        *self = tail;
    }

    pub fn attributes_for_insertion_at(&self, offset: usize) -> InlineInsertionAttributes {
        if self.fragments.is_empty() {
            return InlineInsertionAttributes::default();
        }

        let clamped = offset.min(self.visible_len());
        let mut consumed = 0;

        for (index, fragment) in self.fragments.iter().enumerate() {
            let fragment_len = fragment.text.len();
            let fragment_start = consumed;
            let fragment_end = fragment_start + fragment_len;

            if fragment_start < clamped && clamped < fragment_end {
                return InlineInsertionAttributes {
                    style: fragment.style,
                    html_style: fragment.html_style,
                    link: fragment.link.clone(),
                    footnote: fragment.footnote.clone(),
                    math: None,
                };
            }

            // Typing at a delimited-fragment boundary should produce plain
            // text, not extend the span past its visible closing/opening
            // marker when the caret is outside.
            if clamped == fragment_end && index + 1 == self.fragments.len() {
                return if fragment.style.code || fragment.style.strikethrough || fragment.style.highlight {
                    InlineInsertionAttributes::default()
                } else {
                    InlineInsertionAttributes {
                        style: fragment.style,
                        html_style: fragment.html_style,
                        link: fragment.link.clone(),
                        footnote: fragment.footnote.clone(),
                        math: None,
                    }
                };
            }

            if clamped == fragment_start && index == 0 {
                return if fragment.style.code || fragment.style.strikethrough || fragment.style.highlight {
                    InlineInsertionAttributes::default()
                } else {
                    InlineInsertionAttributes {
                        style: fragment.style,
                        html_style: fragment.html_style,
                        link: fragment.link.clone(),
                        footnote: fragment.footnote.clone(),
                        math: None,
                    }
                };
            }

            consumed = fragment_end;
        }

        InlineInsertionAttributes::default()
    }

    pub fn toggle_bold(&mut self, range: Range<usize>) -> bool {
        self.toggle_style(range, StyleFlag::Bold)
    }

    pub fn toggle_italic(&mut self, range: Range<usize>) -> bool {
        self.toggle_style(range, StyleFlag::Italic)
    }

    pub fn toggle_underline(&mut self, range: Range<usize>) -> bool {
        self.toggle_style(range, StyleFlag::Underline)
    }

    pub fn toggle_strikethrough(&mut self, range: Range<usize>) -> bool {
        self.toggle_style(range, StyleFlag::Strikethrough)
    }

    pub fn toggle_code(&mut self, range: Range<usize>) -> bool {
        self.toggle_style(range, StyleFlag::Code)
    }

    /// 上标 `^x^`。
    pub fn toggle_superscript(&mut self, range: Range<usize>) -> bool {
        self.toggle_style(range, StyleFlag::Superscript)
    }

    /// 下标 `~x~`。
    pub fn toggle_subscript(&mut self, range: Range<usize>) -> bool {
        self.toggle_style(range, StyleFlag::Subscript)
    }

    /// Typora 式的标记文本 `==x==`。
    pub fn toggle_highlight(&mut self, range: Range<usize>) -> bool {
        self.toggle_style(range, StyleFlag::Highlight)
    }

    /// 剥掉这一段里所有成对的行内样式记号：粗体、斜体、下划线、删除线、标记文本、
    /// 行内代码、上标、下标。链接不动（那是结构不是样式），HTML 行内样式也不动——
    /// 那一族来自粘贴进来的 `<span style=…>`，不在这一档口径里。
    /// 切法与 `toggle_style` 一致：只在选区覆盖到的那一段上生效，边界处把片段切开。
    pub fn clear_styles_in_range(&mut self, range: Range<usize>) -> bool {
        if range.is_empty() {
            return false;
        }

        let clamped_start = range.start.min(self.visible_len());
        let clamped_end = range.end.min(self.visible_len());
        if clamped_start >= clamped_end {
            return false;
        }

        let (before, tail) = self.split_at(clamped_start);
        let (mut middle, after) = tail.split_at(clamped_end - clamped_start);
        let had_style = middle
            .fragments
            .iter()
            .any(|fragment| fragment.style != InlineStyle::default());
        if !had_style {
            return false;
        }

        for fragment in &mut middle.fragments {
            // 断点标记不是「格式」：清样式只清粗体/斜体那一族，`<br>` 的写法要保住。
            fragment.style = InlineStyle {
                line_break: fragment.style.line_break,
                ..InlineStyle::default()
            };
        }
        middle.normalize_fragments();

        let mut next = before;
        next.append_tree(middle);
        next.append_tree(after);
        *self = next;
        true
    }

    /// 这一段覆盖到的片段里有没有挂着成对的行内样式：[`Self::clear_styles_in_range`] 的
    /// 只读版，口径与它一致——链接、脚注、公式与 HTML 行内样式都不算这一档要清的东西，
    /// 所以只选中这几个字面时这里返回 false（点了也确实什么都不会变）。
    pub fn has_styles_in_range(&self, range: Range<usize>) -> bool {
        if range.is_empty() {
            return false;
        }
        let start = range.start.min(self.visible_len());
        let end = range.end.min(self.visible_len());
        if start >= end {
            return false;
        }
        let mut offset = 0usize;
        self.fragments.iter().any(|fragment| {
            let length = fragment.text.len();
            let overlaps = offset < end && offset + length > start;
            offset += length;
            overlaps && fragment.style != InlineStyle::default()
        })
    }

    pub fn unwrap_styles_on_fragments(&mut self, targets: &[(usize, StyleFlag)]) {
        if targets.is_empty() {
            return;
        }

        for (fragment_index, flag) in targets {
            if let Some(fragment) = self.fragments.get_mut(*fragment_index) {
                fragment.style = set_style_flag(fragment.style, *flag, false);
            }
        }
        self.normalize_fragments();
    }

    #[allow(dead_code)]
    pub fn replace_visible_range(
        &self,
        range: Range<usize>,
        new_text: &str,
        inserted_attributes: InlineInsertionAttributes,
    ) -> InlineEditResult {
        self.replace_visible_range_with_link_references(
            range,
            new_text,
            inserted_attributes,
            &LinkReferenceDefinitions::default(),
        )
    }

    pub fn replace_visible_range_with_link_references(
        &self,
        range: Range<usize>,
        new_text: &str,
        inserted_attributes: InlineInsertionAttributes,
        reference_definitions: &LinkReferenceDefinitions,
    ) -> InlineEditResult {
        let clamped_start = range.start.min(self.visible_len());
        let clamped_end = range.end.min(self.visible_len());
        let (before, tail) = self.split_at(clamped_start);
        let (_, after) = tail.split_at(clamped_end.saturating_sub(clamped_start));

        let mut temp = before;
        if !new_text.is_empty() {
            temp.fragments.push(InlineFragment {
                text: new_text.to_string(),
                style: inserted_attributes.style,
                html_style: inserted_attributes.html_style,
                link: inserted_attributes.link,
                footnote: inserted_attributes.footnote,
                math: inserted_attributes.math,
            });
        }
        temp.append_tree(after);
        temp.normalize_fragments();
        temp.normalize_visible_text_with_link_references(reference_definitions)
    }

    /// Like `replace_visible_range` but skips marker normalization so
    /// that backticks, stars, and other delimiters are stored as-is.
    /// Used for source-mode editing where the text must remain raw.
    pub fn replace_visible_range_raw(
        &self,
        range: Range<usize>,
        new_text: &str,
        inserted_attributes: InlineInsertionAttributes,
    ) -> InlineEditResult {
        let clamped_start = range.start.min(self.visible_len());
        let clamped_end = range.end.min(self.visible_len());
        let (before, tail) = self.split_at(clamped_start);
        let (_, after) = tail.split_at(clamped_end.saturating_sub(clamped_start));

        let mut temp = before;
        if !new_text.is_empty() {
            temp.fragments.push(InlineFragment {
                text: new_text.to_string(),
                style: inserted_attributes.style,
                html_style: inserted_attributes.html_style,
                link: inserted_attributes.link,
                footnote: inserted_attributes.footnote,
                math: inserted_attributes.math,
            });
        }
        temp.append_tree(after);
        temp.normalize_fragments();
        let len = temp.visible_len();
        InlineEditResult {
            tree: InlineTextTree::from_fragments(temp.fragments),
            visible_to_normalized: (0..=len).collect(),
        }
    }

    /// Core marker-to-style normalizer: scans the fragment text for
    /// delimiter sequences (`**`, `*`, `<u>`, etc.), removes them, and
    /// applies the corresponding [`InlineStyle`] to the text between
    /// matching pairs.  Unmatched delimiters are emitted as literal text.
    #[allow(dead_code)]
    pub fn normalize_inline_syntax(&self) -> InlineEditResult {
        self.normalize_inline_syntax_with_link_references(&LinkReferenceDefinitions::default())
    }

    /// 归一化 markdown 源文本：反斜杠是转义前缀（读文件、解析链接标签）。
    pub fn normalize_inline_syntax_with_link_references(
        &self,
        reference_definitions: &LinkReferenceDefinitions,
    ) -> InlineEditResult {
        self.normalize_inline_text_with_link_references(reference_definitions, false)
    }

    /// 归一化**可见文本**（编辑后重解析）：转义来的记号与反斜杠是字面字符，不是定界符。
    /// 用户按一次 `\` 就应该看到一个反斜杠，连按两次不该塔缩成一个（用户报修：
    /// 渲染模式里打不出两个连续的反斜杠）。
    pub fn normalize_visible_text_with_link_references(
        &self,
        reference_definitions: &LinkReferenceDefinitions,
    ) -> InlineEditResult {
        self.normalize_inline_text_with_link_references(reference_definitions, true)
    }

    fn normalize_inline_text_with_link_references(
        &self,
        reference_definitions: &LinkReferenceDefinitions,
        visible_text_mode: bool,
    ) -> InlineEditResult {
        let visible_text = self.visible_text();
        let tokens = flatten_tokens(&self.fragments, visible_text_mode, &self.escaped_offsets);
        let mut builder = NormalizeBuilder::new(visible_text.len());
        let _ = parse_until(
            &tokens,
            0,
            None,
            InlineStyle::default(),
            None,
            &mut builder,
            false,
            reference_definitions,
        );
        let mut tree = InlineTextTree::from_fragments(builder.fragments);
        // 转义带来的字面字符在输出树里换了位置，跟着这次解析的偏移表一起搬过去。
        tree.escaped_offsets = builder.escaped_offsets;
        InlineEditResult {
            tree,
            visible_to_normalized: builder.visible_to_normalized,
        }
    }

    fn toggle_style(&mut self, range: Range<usize>, flag: StyleFlag) -> bool {
        if range.is_empty() {
            return false;
        }

        let clamped_start = range.start.min(self.visible_len());
        let clamped_end = range.end.min(self.visible_len());
        if clamped_start >= clamped_end {
            return false;
        }

        let (before, tail) = self.split_at(clamped_start);
        let (mut middle, after) = tail.split_at(clamped_end - clamped_start);
        let should_remove = middle
            .fragments
            .iter()
            .all(|fragment| style_flag_enabled(fragment.style, flag));

        for fragment in &mut middle.fragments {
            fragment.style = set_style_flag(fragment.style, flag, !should_remove);
        }
        middle.normalize_fragments();

        let mut next = before;
        next.append_tree(middle);
        next.append_tree(after);
        *self = next;
        true
    }

    fn normalize_fragments(&mut self) {
        let mut normalized: Vec<InlineFragment> = Vec::new();
        for fragment in self.fragments.drain(..) {
            if fragment.text.is_empty() {
                continue;
            }

            if let Some(last) = normalized.last_mut()
                && last.style == fragment.style
                && last.html_style == fragment.html_style
                && last.link == fragment.link
                && last.footnote == fragment.footnote
                && last.math.is_none()
                && fragment.math.is_none()
            {
                last.text.push_str(&fragment.text);
                continue;
            }

            normalized.push(fragment);
        }
        self.fragments = normalized;
    }
}

