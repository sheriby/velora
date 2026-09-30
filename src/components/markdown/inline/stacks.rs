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
        costs[0][choice_index] = stack_transition_cost(&[], stack) + stack_variant_penalty(stack);
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
                    + stack_transition_cost(prev_stack, stack)
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
                costs[last_fragment_index][choice_index] + stack_transition_cost(stack, &[]),
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
    let mut markdown_stack = Vec::new();
    if style.bold {
        markdown_stack.push(Delimiter::BoldMarkdown { marker: '*' });
    }
    if style.underline {
        markdown_stack.push(Delimiter::Underline);
    }
    if style.strikethrough {
        markdown_stack.push(Delimiter::StrikethroughMarkdown);
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
        markdown_stack.push(Delimiter::ItalicMarkdown { marker: '*' });
    }
    // Code is always the innermost delimiter so it nests inside emphasis.
    if let Some(run_len) = code_run_len {
        markdown_stack.push(Delimiter::CodeMarkdown { run_len });
    }

    let has_emphasis = style.bold || style.italic;
    if !has_emphasis {
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
        html_stack.push(Delimiter::StrikethroughMarkdown);
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
        && left.underline == right.underline
        && left.strikethrough == right.strikethrough
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
pub(crate) fn stack_transition_cost(from: &[Delimiter], to: &[Delimiter]) -> usize {
    let marker_len = stack_transition_len(from, to);
    let marker_string = stack_transition_string(from, to);
    let ambiguity_penalty =
        if !from.is_empty() && !to.is_empty() && longest_star_run(&marker_string) >= 4 {
            1_000
        } else {
            0
        };
    marker_len + ambiguity_penalty
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

pub(crate) fn longest_star_run(text: &str) -> usize {
    let mut max_run = 0;
    let mut current_run = 0;
    for ch in text.chars() {
        if ch == '*' {
            current_run += 1;
            max_run = max_run.max(current_run);
        } else {
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
        StyleFlag::Code => style.code,
        StyleFlag::Superscript => style.script == InlineScript::Superscript,
        StyleFlag::Subscript => style.script == InlineScript::Subscript,
    }
}

pub(crate) fn set_style_flag(mut style: InlineStyle, flag: StyleFlag, enabled: bool) -> InlineStyle {
    match flag {
        StyleFlag::Bold => style.bold = enabled,
        StyleFlag::Italic => style.italic = enabled,
        StyleFlag::Underline => style.underline = enabled,
        StyleFlag::Strikethrough => style.strikethrough = enabled,
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
    if tokens[index].ch != '_' {
        return true;
    }
    let (prev, run_next) = emphasis_run_bounds(tokens, index);
    underscore_can_open(prev, run_next)
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
/// 存盘时整段被重写成 `*` 定界符（用户报修）。
pub(crate) fn underscore_can_open(prev: Option<char>, next: Option<char>) -> bool {
    let Some(next) = next else {
        return false;
    };
    let left_flanking = !is_emphasis_punctuation(next)
        || prev.is_none_or(|ch| ch.is_whitespace() || is_emphasis_punctuation(ch));
    if !left_flanking {
        return false;
    }
    let right_flanking = prev.is_some_and(|ch| !ch.is_whitespace())
        && (prev.is_some_and(|ch| !is_emphasis_punctuation(ch))
            || next.is_whitespace()
            || is_emphasis_punctuation(next));
    !right_flanking || prev.is_some_and(is_emphasis_punctuation)
}

/// `_` 的关闭条件，与 `underscore_can_open` 镜像。
pub(crate) fn underscore_can_close(prev: Option<char>, next: Option<char>) -> bool {
    let Some(prev) = prev else {
        return false;
    };
    if prev.is_whitespace() {
        return false;
    }
    let right_flanking = !is_emphasis_punctuation(prev)
        || next.is_none_or(|ch| ch.is_whitespace() || is_emphasis_punctuation(ch));
    if !right_flanking {
        return false;
    }
    let left_flanking = next.is_some_and(|ch| !ch.is_whitespace())
        && (next.is_some_and(|ch| !is_emphasis_punctuation(ch))
            || prev.is_whitespace()
            || is_emphasis_punctuation(prev));
    !left_flanking || next.is_some_and(is_emphasis_punctuation)
}

/// 侧翼规则需要的「标点」分类。标准库没有 Unicode 标点分类，这里近似：ASCII 走
/// 自己的标点表，其余字符除了字母、数字、空白都算标点（全角标点、日文标点等成立）。
pub(crate) fn is_emphasis_punctuation(ch: char) -> bool {
    if ch.is_ascii() {
        ch.is_ascii_punctuation()
    } else {
        !ch.is_alphanumeric() && !ch.is_whitespace()
    }
}

/// 强调意义上的「词内字符」：既不是空白也不是标点。
pub(crate) fn is_emphasis_word_char(ch: char) -> bool {
    !ch.is_whitespace() && !is_emphasis_punctuation(ch)
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
    if tokens[index].ch != '_' {
        return !tokens[index - 1].ch.is_whitespace();
    }
    let (prev, next) = emphasis_run_bounds(tokens, index);
    underscore_can_close(prev, next)
}
