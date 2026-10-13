use super::*;

/// Viterbi-like DP that picks the optimal delimiter stack for each fragment.
/// Stacks are enumerated per fragment and the lowest-cost combination wins.

/// Each fragment's style can be expressed with either Markdown or HTML
/// delimiters.  We minimize the total number of delimiter characters written
/// plus a penalty for HTML variants.  A large penalty is added when a
/// transition would produce 4+ consecutive `*` characters (Markdown ambiguity).
pub(crate) fn choose_fragment_stacks(fragments: &[InlineFragment]) -> Vec<Vec<Delimiter>> {
    // Enumerate the 1-2 possible delimiter stacks for each fragment's style.
    let variants = fragments
        .iter()
        .enumerate()
        .map(|(index, fragment)| {
            stack_variants(
                fragment,
                index.checked_sub(1).and_then(|i| fragments.get(i)),
            )
        })
        .collect::<Vec<_>>();

    // DP table: costs[fragment_index][choice_index]
    let mut costs: Vec<Vec<usize>> = variants
        .iter()
        .map(|choices| vec![usize::MAX; choices.len()])
        .collect();
    let mut previous_choice: Vec<Vec<Option<usize>>> = variants
        .iter()
        .map(|choices| vec![None; choices.len()])
        .collect();

    // Initial fragment: cost from empty stack to each variant.
    for (choice_index, stack) in variants[0].iter().enumerate() {
        costs[0][choice_index] = stack_transition_cost(&[], stack, None, fragments[0].text.chars().next())
            + stack_variant_penalty(stack);
    }

    // Forward pass: compute minimum cost for each fragment's choices.
    for fragment_index in 1..variants.len() {
        for (choice_index, stack) in variants[fragment_index].iter().enumerate() {
            for (prev_index, prev_stack) in variants[fragment_index - 1].iter().enumerate() {
                let prev_cost = costs[fragment_index - 1][prev_index];
                if prev_cost == usize::MAX {
                    continue;
                }

                let cost = prev_cost
                    + stack_transition_cost(
                        prev_stack,
                        stack,
                        fragments[fragment_index - 1].text.chars().next_back(),
                        fragments[fragment_index].text.chars().next(),
                    )
                    + stack_variant_penalty(stack);
                if cost < costs[fragment_index][choice_index] {
                    costs[fragment_index][choice_index] = cost;
                    previous_choice[fragment_index][choice_index] = Some(prev_index);
                }
            }
        }
    }

    // Backtrack: choose the best final stack and trace back through the DP.
    let last_fragment_index = variants.len() - 1;
    let (mut best_choice, _) = variants[last_fragment_index]
        .iter()
        .enumerate()
        .map(|(choice_index, stack)| {
            (
                choice_index,
                costs[last_fragment_index][choice_index]
                    + stack_transition_cost(stack, &[], fragments[last_fragment_index].text.chars().next_back(), None),
            )
        })
        .min_by(|(left_index, left_cost), (right_index, right_cost)| {
            left_cost.cmp(right_cost).then_with(|| {
                stack_preference_key(&variants[last_fragment_index][*left_index]).cmp(
                    &stack_preference_key(&variants[last_fragment_index][*right_index]),
                )
            })
        })
        .unwrap_or((0, 0));

    let mut chosen = vec![Vec::new(); variants.len()];
    for fragment_index in (0..variants.len()).rev() {
        chosen[fragment_index] = variants[fragment_index][best_choice].clone();
        if let Some(prev_index) = previous_choice[fragment_index][best_choice] {
            best_choice = prev_index;
        }
    }

    chosen
}

pub(crate) fn stack_variants(
    fragment: &InlineFragment,
    previous_fragment: Option<&InlineFragment>,
) -> Vec<Vec<Delimiter>> {
    let style = fragment.style;
    let code_run_len = style.code.then(|| code_delimiter_run_len(&fragment.text));
    // 强调按原文那位的写法写回去（`__粗__` 不该变成 `**粗**`）；没记过的按规范写 `*`。
    let emphasis_marker = style.emphasis_marker.unwrap_or('*');
    let mut markdown_stack = Vec::new();
    if style.bold {
        markdown_stack.push(Delimiter::BoldMarkdown {
            marker: emphasis_marker,
        });
    }
    if style.underline {
        markdown_stack.push(Delimiter::Underline);
    }
    if style.strikethrough {
        markdown_stack.push(Delimiter::StrikethroughMarkdown);
    }
    if style.highlight {
        markdown_stack.push(Delimiter::HighlightMarkdown);
    }
    match style.script {
        InlineScript::Normal => {}
        InlineScript::Superscript
            if can_use_markdown_script_delimiters(previous_fragment, fragment) =>
        {
            markdown_stack.push(Delimiter::SuperscriptMarkdown)
        }
        InlineScript::Superscript => markdown_stack.push(Delimiter::SuperscriptHtml),
        InlineScript::Subscript
            if style.strikethrough
                || !can_use_markdown_script_delimiters(previous_fragment, fragment) =>
        {
            markdown_stack.push(Delimiter::SubscriptHtml)
        }
        InlineScript::Subscript => markdown_stack.push(Delimiter::SubscriptMarkdown),
    }
    if style.italic {
        markdown_stack.push(Delimiter::ItalicMarkdown {
            marker: emphasis_marker,
        });
    }
    // Code is always the innermost delimiter so it nests inside emphasis.
    if let Some(run_len) = code_run_len {
        markdown_stack.push(Delimiter::CodeMarkdown { run_len });
    }

    // HTML 退路：星号的侧翼不合格时靠 `<strong>`/`<em>`，删除线同理靠 `<del>`
    // （导出器认 Markdown 的那套判据比编辑器严，`~~删除：~~` 会按字面出去）。
    let has_alternative = style.bold || style.italic || style.strikethrough;
    if !has_alternative {
        return vec![markdown_stack];
    }

    let mut html_stack = Vec::new();
    if style.bold {
        html_stack.push(Delimiter::BoldHtml);
    }
    if style.underline {
        html_stack.push(Delimiter::Underline);
    }
    if style.strikethrough {
        html_stack.push(Delimiter::StrikethroughHtml);
    }
    if style.highlight {
        html_stack.push(Delimiter::HighlightMarkdown);
    }
    match style.script {
        InlineScript::Normal => {}
        InlineScript::Superscript => html_stack.push(Delimiter::SuperscriptHtml),
        InlineScript::Subscript => html_stack.push(Delimiter::SubscriptHtml),
    }
    if style.italic {
        html_stack.push(Delimiter::ItalicHtml);
    }
    if let Some(run_len) = code_run_len {
        html_stack.push(Delimiter::CodeMarkdown { run_len });
    }

    vec![markdown_stack, html_stack]
}

pub(crate) fn can_use_markdown_script_delimiters(
    previous_fragment: Option<&InlineFragment>,
    fragment: &InlineFragment,
) -> bool {
    // This guard is shared by serialization and inline projection. Markdown
    // script markers need a plain ASCII owner immediately before the script
    // fragment; otherwise we fall back to <sup>/<sub> so the next parse sees
    // the same style boundary.
    let Some(previous) = previous_fragment else {
        return false;
    };
    if previous.style.has_script() {
        return false;
    }
    previous
        .text
        .chars()
        .next_back()
        .is_some_and(|ch| ch.is_ascii_alphanumeric())
        && previous.html_style == fragment.html_style
        && previous.link == fragment.link
        && previous.footnote.is_none()
        && fragment.footnote.is_none()
        && previous.math.is_none()
        && fragment.math.is_none()
        && styles_match_ignoring_script(previous.style, fragment.style)
}

pub(crate) fn styles_match_ignoring_script(left: InlineStyle, right: InlineStyle) -> bool {
    left.bold == right.bold
        && left.italic == right.italic
        && left.emphasis_marker == right.emphasis_marker
        && left.underline == right.underline
        && left.strikethrough == right.strikethrough
        && left.highlight == right.highlight
        && left.code == right.code
}

pub(crate) fn code_delimiter_run_len(text: &str) -> usize {
    let mut longest = 0usize;
    let mut current = 0usize;
    for ch in text.chars() {
        if ch == '`' {
            current += 1;
            longest = longest.max(current);
        } else {
            current = 0;
        }
    }
    longest + 1
}

pub(crate) fn stack_transition_len(from: &[Delimiter], to: &[Delimiter]) -> usize {
    let common = common_prefix_len(from, to);
    let close_len = from[common..]
        .iter()
        .rev()
        .map(|delimiter| delimiter.close().len())
        .sum::<usize>();
    let open_len = to[common..]
        .iter()
        .map(|delimiter| delimiter.open().len())
        .sum::<usize>();
    close_len + open_len
}

/// Cost of closing `from` delimiters and opening `to` delimiters in sequence.
/// Adds a heavy penalty if the resulting string would contain 4+ consecutive
/// `*` characters, which Markdown parsers may interpret ambiguously.
pub(crate) fn stack_transition_cost(
    from: &[Delimiter],
    to: &[Delimiter],
    before: Option<char>,
    after: Option<char>,
) -> usize {
    let marker_len = stack_transition_len(from, to);
    let marker_string = stack_transition_string(from, to);
    let ambiguity_penalty =
        if !from.is_empty() && !to.is_empty() && longest_star_run(&marker_string) >= 4 {
            1_000
        } else {
            0
        };
    let common = common_prefix_len(from, to);
    let transitions = from[common..].iter().rev().map(|delimiter| (delimiter, false))
        .chain(to[common..].iter().map(|delimiter| (delimiter, true)));
    let mut cursor = 0;
    let mut flanking_penalty = 0;
    for (delimiter, opening) in transitions {
        let marker = if opening { delimiter.open() } else { delimiter.close() };
        if let Delimiter::BoldMarkdown { .. }
        | Delimiter::ItalicMarkdown { .. }
        | Delimiter::StrikethroughMarkdown
        | Delimiter::HighlightMarkdown = delimiter
        {
            // 序列化出去的记号要能被导出器读成同样的样式，判据就得按**导出器**的规则；
            // UI 配对仍支持 CJK 放宽（`delimiter_can_*`）。星号按 CommonMark 侧翼（严格），
            // 删除线按 pulldown 的 `~~` 判据（见 `strikethrough_exports_as_markdown`），
            // 两个都不合格时 DP 才落到 HTML 候选。高亮（`==`）读回去的还是编辑器那一对
            // `delimiter_can_*`（导出侧 `locate_highlight_close` 也调它们），判据只有一处，
            // 保持原来的放宽口径。
            let ch = match delimiter {
                Delimiter::StrikethroughMarkdown => '~',
                Delimiter::HighlightMarkdown => '=',
                Delimiter::BoldMarkdown { marker: ch }
                | Delimiter::ItalicMarkdown { marker: ch } => *ch,
                _ => unreachable!(),
            };
            let start = cursor - marker_string[..cursor].chars().rev().take_while(|other| *other == ch).count();
            let end = cursor + marker_string[cursor..].chars().take_while(|other| *other == ch).count();
            let prev = marker_string[..start].chars().next_back().or(before);
            let next = marker_string[end..].chars().next().or(after);
            let valid = match (ch, opening) {
                ('*', true) => emphasis_flanking(prev, next).0,
                ('*', false) => emphasis_flanking(prev, next).1,
                ('~', true) => strikethrough_exports_as_markdown(true, prev, next),
                ('~', false) => strikethrough_exports_as_markdown(false, prev, next),
                (_, true) => delimiter_can_open(ch, prev, next),
                (_, false) => delimiter_can_close(ch, prev, next),
            };
            if !valid {
                flanking_penalty += 1_000;
            }
        }
        cursor += marker.len();
    }
    marker_len + ambiguity_penalty + flanking_penalty
}

pub(crate) fn stack_variant_penalty(stack: &[Delimiter]) -> usize {
    if stack.iter().any(|delimiter| delimiter.is_html()) {
        64
    } else {
        0
    }
}

pub(crate) fn write_stack_transition(output: &mut String, from: &[Delimiter], to: &[Delimiter]) {
    let common = common_prefix_len(from, to);
    for delimiter in from[common..].iter().rev() {
        output.push_str(&delimiter.close());
    }
    for delimiter in &to[common..] {
        output.push_str(&delimiter.open());
    }
}

pub(crate) fn stack_transition_string(from: &[Delimiter], to: &[Delimiter]) -> String {
    let mut output = String::new();
    write_stack_transition(&mut output, from, to);
    output
}

pub(crate) fn common_prefix_len(left: &[Delimiter], right: &[Delimiter]) -> usize {
    let mut index = 0;
    while index < left.len() && index < right.len() && left[index] == right[index] {
        index += 1;
    }
    index
}

pub(crate) fn stack_preference_key(stack: &[Delimiter]) -> Vec<u8> {
    stack
        .iter()
        .map(|delimiter| delimiter.preference_rank())
        .collect()
}

/// 连续 4 个以上的强调定界符在 CommonMark 里是歧义写法。星号与下划线同理，
/// 序列化现在会照原文写 `_`，两种都得盯。
pub(crate) fn longest_star_run(text: &str) -> usize {
    let mut max_run = 0;
    let mut run_char = None;
    let mut current_run = 0;
    for ch in text.chars() {
        if matches!(ch, '*' | '_') {
            if run_char == Some(ch) {
                current_run += 1;
            } else {
                run_char = Some(ch);
                current_run = 1;
            }
            max_run = max_run.max(current_run);
        } else {
            run_char = None;
            current_run = 0;
        }
    }
    max_run
}

pub(crate) fn style_flag_enabled(style: InlineStyle, flag: StyleFlag) -> bool {
    match flag {
        StyleFlag::Bold => style.bold,
        StyleFlag::Italic => style.italic,
        StyleFlag::Underline => style.underline,
        StyleFlag::Strikethrough => style.strikethrough,
        StyleFlag::Highlight => style.highlight,
        StyleFlag::Code => style.code,
        StyleFlag::Superscript => style.script == InlineScript::Superscript,
        StyleFlag::Subscript => style.script == InlineScript::Subscript,
    }
}

pub(crate) fn set_style_flag(mut style: InlineStyle, flag: StyleFlag, enabled: bool) -> InlineStyle {
    match flag {
        // 开强调时把写法定下来（用户新加的强调按规范写 `*`），两个强调都没了就把写法
        // 一起放下——这样「手动加粗」与「从 `**…**` 解析出来」的样式是同一个值，
        // 序列化的往返比较不会因为半个字段的差异而分叉。
        StyleFlag::Bold => {
            style.bold = enabled;
            if enabled {
                style.emphasis_marker = Some(style.emphasis_marker.unwrap_or('*'));
            } else if !style.italic {
                style.emphasis_marker = None;
            }
        }
        StyleFlag::Italic => {
            style.italic = enabled;
            if enabled {
                style.emphasis_marker = Some(style.emphasis_marker.unwrap_or('*'));
            } else if !style.bold {
                style.emphasis_marker = None;
            }
        }
        StyleFlag::Underline => style.underline = enabled,
        StyleFlag::Strikethrough => style.strikethrough = enabled,
        StyleFlag::Highlight => style.highlight = enabled,
        StyleFlag::Code => style.code = enabled,
        StyleFlag::Superscript => {
            style.script = if enabled {
                InlineScript::Superscript
            } else if style.script == InlineScript::Superscript {
                InlineScript::Normal
            } else {
                style.script
            }
        }
        StyleFlag::Subscript => {
            style.script = if enabled {
                InlineScript::Subscript
            } else if style.script == InlineScript::Subscript {
                InlineScript::Normal
            } else {
                style.script
            }
        }
    }
    style
}

pub(crate) fn clamp_to_char_boundary(text: &str, offset: usize) -> usize {
    let clamped = offset.min(text.len());
    if text.is_char_boundary(clamped) {
        return clamped;
    }

    let mut boundary = clamped;
    while boundary > 0 && !text.is_char_boundary(boundary) {
        boundary -= 1;
    }
    boundary
}

/// 把字节区间两端收敛到字符边界。鼠标位置、markdown 空间换算得到的偏移都可能
/// 落在多字节字符内部，直接切片会 panic（release 下 panic = abort），
/// 所以按偏移切片前统一走这里。
pub(crate) fn clamp_range_to_char_boundaries(text: &str, range: Range<usize>) -> Range<usize> {
    let start = clamp_to_char_boundary(text, range.start);
    let end = clamp_to_char_boundary(text, range.end).max(start);
    start..end
}

pub(crate) fn can_open_emphasis(tokens: &[CharToken], index: usize, len: usize) -> bool {
    let Some(next) = tokens.get(index + len) else {
        return false;
    };
    if next.ch.is_whitespace() {
        return false;
    }
    let (prev, run_next) = emphasis_run_bounds(tokens, index);
    delimiter_can_open(tokens[index].ch, prev, run_next)
}

pub(crate) fn delimiter_can_open(marker: char, prev: Option<char>, next: Option<char>) -> bool {
    if next.is_none_or(char::is_whitespace) {
        return false;
    }
    match marker {
        '_' => underscore_can_open(prev, next),
        // `*` 家族按星号规则（左翼即可开，词内允许）；`~~`/`==` 与 GFM/Typora 同口径
        // 走同一套标点侧翼——没有它时 `2~~(3+4)~~5` 会被整段划掉而导出按字面。
        '*' | '~' | '=' => cjk_emphasis_flanking(prev, next).0,
        _ => true,
    }
}

pub(crate) fn delimiter_can_close(marker: char, prev: Option<char>, next: Option<char>) -> bool {
    if prev.is_none_or(char::is_whitespace) {
        return false;
    }
    match marker {
        '_' => underscore_can_close(prev, next),
        '*' | '~' | '=' => cjk_emphasis_flanking(prev, next).1,
        _ => true,
    }
}

fn emphasis_flanking(prev: Option<char>, next: Option<char>) -> (bool, bool) {
    let left = next.is_some_and(|ch| !ch.is_whitespace())
        && (next.is_some_and(|ch| !is_emphasis_punctuation(ch))
            || prev.is_none_or(|ch| ch.is_whitespace() || is_emphasis_punctuation(ch)));
    let right = prev.is_some_and(|ch| !ch.is_whitespace())
        && (prev.is_some_and(|ch| !is_emphasis_punctuation(ch))
            || next.is_none_or(|ch| ch.is_whitespace() || is_emphasis_punctuation(ch)));
    (left, right)
}

/// 定界符串是同一字符的极大连续串，侧翼判定看整串前后的字符。按字符逐个判会把
/// `foo__bar__baz` 里串内第二个下划线当成独立定界符（它前面是标点意义上的 `_`），
/// 于是整串被拆开。返回 (串前字符, 串后字符)。
pub(crate) fn emphasis_run_bounds(tokens: &[CharToken], index: usize) -> (Option<char>, Option<char>) {
    let ch = tokens[index].ch;
    let mut start = index;
    while start > 0 && tokens[start - 1].ch == ch {
        start -= 1;
    }
    let mut end = index;
    while end + 1 < tokens.len() && tokens[end + 1].ch == ch {
        end += 1;
    }
    (
        start.checked_sub(1).map(|prev| tokens[prev].ch),
        tokens.get(end + 1).map(|token| token.ch),
    )
}

/// `_` 的开启条件（CommonMark 侧翼规则）：左侧成翼，且不同时右侧成翼，除非前面是标点。
/// 没有这条限制时 `topic_embedding_attention` 会解析成 文本 + 斜体 + 文本，
/// 存盘时整段被重写成 `*` 定界符。
pub(crate) fn underscore_can_open(prev: Option<char>, next: Option<char>) -> bool {
    let (left, right) = emphasis_flanking(prev, next);
    left && (!right || prev.is_some_and(is_emphasis_punctuation))
}

/// `_` 的关闭条件，与 `underscore_can_open` 镜像。
pub(crate) fn underscore_can_close(prev: Option<char>, next: Option<char>) -> bool {
    let (left, right) = emphasis_flanking(prev, next);
    right && (!left || next.is_some_and(is_emphasis_punctuation))
}

/// 侧翼规则需要的「标点」分类。标准库没有 Unicode 标点分类，这里近似：ASCII 走
/// 自己的标点表，其余字符除了字母、数字、空白都算标点（全角标点、日文标点等成立）。
/// 导出器（pulldown 0.13 的 GFM `~~`）读不读得出来：序列化侧的判据。
///
/// 与编辑器放宽过的 `delimiter_can_*`（CJK 口径）不是一回事，也不能合成一条：编辑器
/// 认 `测试~~删除：~~测试`（`markdown-cjk-friendly` 的口径），而 pulldown 的
/// `delim_run_can_close` 对 `run_len > 1` 的 `~` 串在前一个字符是标点、后一个不是空白
/// 或标点时判不成立——同一份文件界面划掉、导出按字面。这里按它的规则判，
/// 判不成立就让 DP 落到 `<del>` 那支候选（`StrikethroughHtml`）。
///
/// 依据（pulldown-cmark 0.13.4 `src/firstpass.rs`，`~~` 走 `count == 2`）：
/// - 开：`next` 存在且不是空白（`run_len > 1` 时不再看标点）；
/// - 闭：`prev` 存在且不是空白，且（`prev` 不是标点，或 `next` 是空白/标点/没有）。
fn strikethrough_exports_as_markdown(opening: bool, prev: Option<char>, next: Option<char>) -> bool {
    if opening {
        return next.is_some_and(|ch| !ch.is_whitespace());
    }
    prev.is_some_and(|ch| !ch.is_whitespace())
        && (!prev.is_some_and(is_emphasis_punctuation)
            || next.is_none_or(|ch| ch.is_whitespace() || is_emphasis_punctuation(ch)))
}

pub(crate) fn is_emphasis_punctuation(ch: char) -> bool {
    if ch.is_ascii() {
        ch.is_ascii_punctuation()
    } else {
        !ch.is_alphanumeric() && !ch.is_whitespace()
    }
}

/// [CJK character] 的范围，取自 `markdown-cjk-friendly` 规范的 `ranges.md`
/// （Unicode 18；该文件由它仓库里的 `print-ranges` 生成）。这里是逐条抄录，不做取舍：
/// 判定只服务于强调侧翼，少抄一段只会让某种装饰符号旁的强调退回 CommonMark 行为。
///
/// [CJK character]: https://github.com/tats-u/markdown-cjk-friendly/blob/main/ranges.md
fn is_cjk_character(ch: char) -> bool {
    matches!(
        ch,
        '\u{1100}'..='\u{11FF}' | '\u{20A9}' | '\u{2329}'..='\u{232A}' | '\u{2630}'..='\u{2637}' |
        '\u{268A}'..='\u{268F}' | '\u{2E80}'..='\u{2E99}' | '\u{2E9B}'..='\u{2EF3}' |
        '\u{2F00}'..='\u{2FD5}' | '\u{2FF0}'..='\u{303E}' | '\u{3041}'..='\u{3096}' |
        '\u{3099}'..='\u{30FF}' | '\u{3105}'..='\u{312F}' | '\u{3131}'..='\u{318E}' |
        '\u{3190}'..='\u{31E5}' | '\u{31EF}'..='\u{321E}' | '\u{3220}'..='\u{3247}' |
        '\u{3250}'..='\u{A48C}' | '\u{A490}'..='\u{A4C6}' | '\u{A960}'..='\u{A97C}' |
        '\u{AC00}'..='\u{D7A3}' | '\u{D7B0}'..='\u{D7C6}' | '\u{D7CB}'..='\u{D7FB}' |
        '\u{F900}'..='\u{FAFF}' | '\u{FE10}'..='\u{FE19}' | '\u{FE30}'..='\u{FE52}' |
        '\u{FE54}'..='\u{FE66}' | '\u{FE68}'..='\u{FE6B}' | '\u{FF01}'..='\u{FFBE}' |
        '\u{FFC2}'..='\u{FFC7}' | '\u{FFCA}'..='\u{FFCF}' | '\u{FFD2}'..='\u{FFD7}' |
        '\u{FFDA}'..='\u{FFDC}' | '\u{FFE0}'..='\u{FFE6}' | '\u{FFE8}'..='\u{FFEE}' |
        '\u{16FE0}'..='\u{16FE4}' | '\u{16FF0}'..='\u{16FF6}' | '\u{17000}'..='\u{18CDA}' |
        '\u{18CFF}'..='\u{18D20}' | '\u{18D80}'..='\u{18DF2}' | '\u{18E00}'..='\u{19191}' |
        '\u{191A0}'..='\u{191D2}' | '\u{1AFF0}'..='\u{1AFF3}' | '\u{1AFF5}'..='\u{1AFFB}' |
        '\u{1AFFD}'..='\u{1AFFE}' | '\u{1B000}'..='\u{1B128}' | '\u{1B132}' | '\u{1B150}'..='\u{1B152}'
        | '\u{1B155}' | '\u{1B164}'..='\u{1B168}' | '\u{1B170}'..='\u{1B2FB}' |
        '\u{1D300}'..='\u{1D356}' | '\u{1D360}'..='\u{1D376}' | '\u{1F1AE}' | '\u{1F200}' | '\u{1F202}'
        | '\u{1F210}'..='\u{1F219}' | '\u{1F21B}'..='\u{1F22E}' | '\u{1F230}'..='\u{1F231}' |
        '\u{1F237}' | '\u{1F23B}' | '\u{1F240}'..='\u{1F248}' | '\u{1F260}'..='\u{1F265}' | '\u{1F7DA}'
        | '\u{20000}'..='\u{3FFFD}' | '\u{FE00}'..='\u{FE0E}'
    )
}

/// CJK 标点 = 规范意义上的标点里那些 CJK 字：`：` `。` `，` `「` `」` `（` `）` 都在里面。
fn is_cjk_punctuation(ch: char) -> bool {
    is_emphasis_punctuation(ch) && is_cjk_character(ch)
}

/// 侧翼判定里的「标点」。CommonMark 按 Unicode 标点判，可中文标点后面不跟空格，
/// 于是 `**注意：**这是重点` 的收尾标记永远不成右翼，整段按字面画。按
/// `markdown-cjk-friendly` 的口径把 CJK 标点排除，CJK 字还顶替规则里要求的「空白或标点」。
fn is_non_cjk_punctuation(ch: char) -> bool {
    is_emphasis_punctuation(ch) && !is_cjk_punctuation(ch)
}

/// `*` 家族的侧翼判定（CJK 口径）。`_` 家族仍走 `emphasis_flanking`：词内下划线必须继续
/// 按字面保留（`topic_embedding_attention`），两边判据不同是故意的。
fn cjk_emphasis_flanking(prev: Option<char>, next: Option<char>) -> (bool, bool) {
    let left = next.is_some_and(|ch| !ch.is_whitespace())
        && (!next.is_some_and(is_non_cjk_punctuation)
            || prev.is_none_or(|ch| {
                ch.is_whitespace() || is_non_cjk_punctuation(ch) || is_cjk_character(ch)
            }));
    let right = prev.is_some_and(|ch| !ch.is_whitespace())
        && (!prev.is_some_and(is_non_cjk_punctuation)
            || next.is_none_or(|ch| {
                ch.is_whitespace() || is_non_cjk_punctuation(ch) || is_cjk_character(ch)
            }));
    (left, right)
}

pub(crate) fn can_open_script(tokens: &[CharToken], index: usize, marker: char) -> bool {
    if token_is_backslash_escaped(tokens, index) {
        return false;
    }

    if marker == '~' && !is_single_tilde_delimiter(tokens, index) {
        return false;
    }

    index > 0
        && tokens[index - 1].ch.is_ascii_alphanumeric()
        && tokens
            .get(index + 1)
            .is_some_and(|token| token.ch.is_ascii_alphanumeric())
}

pub(crate) fn can_close_emphasis(tokens: &[CharToken], index: usize) -> bool {
    if index == 0 {
        return false;
    }
    let (prev, next) = emphasis_run_bounds(tokens, index);
    delimiter_can_close(tokens[index].ch, prev, next)
}
