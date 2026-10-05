//! 搜索匹配层的行为快照。
//!
//! 这一文件的目的不是「断言搜索应该怎样」，而是把匹配层的实际行为逐条记下来，
//! 让任何行为漂移都只能是一条被点名、被解释的改动，而不是一次静默的回归。
//!
//! 阶段 0 建立时其中有六条以 `defect_` 开头，钉的是当时已知的缺陷。接入 ripgrep
//! 引擎之后有三条翻成了「记录修复」，函数名跟着改：
//! - `whole_word_option_applies_in_regex_mode_now`
//!   （原 defect_regex_mode_ignores_the_whole_word_option）；
//! - `invalid_regex_reports_a_diagnosis_and_stops_searching`
//!   （原 defect_invalid_regex_degrades_to_a_literal_search）；
//! - `unicode_case_folding_now_finds_the_dotted_capital_i`
//!   （原 defect_turkish_dotted_capital_i_is_not_found）。
//! 剩下三条仍然是缺陷快照，保留 `defect_` 前缀，等各自的阶段处理：正则模式下
//! 模糊开关被忽略、逐行匹配无法跨行、文档范围的命中序号会重复。
//!
//! 探针实测（`/tmp/rgprobe`）盯住的最大一条风险写在
//! `snapshot_zero_width_regex_matches_only_char_boundaries`：`grep-regex` 的
//! `find_iter` 在零宽命中上按**字节**步进，会吐出落在多字节字符内部的区间，
//! 而现状的 `regex` crate 按**字符**步进不会。那条用例逐位钉住起点集合，是换
//! 引擎时的硬闸门——出口过滤器一旦被删掉，它就该红。

use super::super::{
    SearchMatcher, SearchOptions, find_document_match_from, search_document_source,
};
use std::ops::Range;

fn options(match_case: bool, whole_word: bool, use_regex: bool, fuzzy: bool) -> SearchOptions {
    SearchOptions {
        match_case,
        whole_word,
        use_regex,
        fuzzy,
    }
}

fn plain(query: &str) -> SearchMatcher {
    SearchMatcher::new(query, SearchOptions::default())
}

/// 命中的字节区间 + 命中的原文。快照断言一律用这个形状：区间本身说明位置，
/// 原文保证区间没数错。
fn hits(line: &str, matcher: &SearchMatcher) -> Vec<(usize, usize, String)> {
    matcher
        .find_in_line(line)
        .into_iter()
        .map(|range| (range.start, range.end, line[range.clone()].to_string()))
        .collect()
}

// ------------------------------------------------------------------ 字面量

#[test]
fn snapshot_plain_query_matches_both_cases_by_default() {
    let matcher = plain("needle");
    assert_eq!(
        hits("foo needle bar", &matcher),
        vec![(4, 10, "needle".to_string())]
    );
    assert_eq!(
        hits("second NEEDLE line", &matcher),
        vec![(7, 13, "NEEDLE".to_string())]
    );
}

#[test]
fn snapshot_match_case_only_matches_the_exact_case() {
    let matcher = SearchMatcher::new(
        "needle",
        options(true /* match_case */, false, false, false),
    );
    assert_eq!(
        hits("foo needle bar NEEDLE", &matcher),
        vec![(4, 10, "needle".to_string())]
    );
    assert!(hits("NEEDLE NEEDLE", &matcher).is_empty());
}

#[test]
fn snapshot_plain_query_treats_regex_metacharacters_as_text() {
    // 非正则模式下元字符必须是字面量：这一族在两个引擎里都得一样，
    // 差别只在 ripgrep 是靠 fixed_strings 把转义交给引擎，现状是手工当子串找。
    for (query, line) in [
        ("a.b", "axb and a.b here"),
        ("a*b", "ab a*b abb"),
        ("(x)", "wrap (x) ok"),
        ("[abc]", "pick [abc] not a"),
        ("end$", "end$ and end"),
        ("C:\\path", "put C:\\path here"),
        ("a|b", "a|b alone"),
        ("+?{}()", "+?{}() noise"),
    ] {
        let matcher = plain(query);
        let found = hits(line, &matcher);
        assert_eq!(
            found.len(),
            1,
            "字面量查询 {query:?} 在 {line:?} 里应当命中一次，实得 {found:?}"
        );
        assert_eq!(found[0].2, query, "命中的原文必须就是查询本身");
    }
}

#[test]
fn snapshot_repeated_letters_do_not_overlap() {
    // "aaa" 找 "aa"：两个口径都得只给一个不重叠的命中。
    assert_eq!(
        hits("aaa", &plain("aa")),
        vec![(0, 2, "aa".to_string())]
    );
    let case_sensitive = SearchMatcher::new("aa", options(true, false, false, false));
    assert_eq!(
        hits("aaa", &case_sensitive),
        vec![(0, 2, "aa".to_string())]
    );
}

#[test]
fn snapshot_empty_and_whitespace_only_queries_find_nothing() {
    assert!(plain("").find_in_line("anything").is_empty());
    assert!(plain("   ").is_empty());
    assert!(!plain("x").is_empty());
}

// ------------------------------------------------------------------ 中文

#[test]
fn snapshot_cjk_query_reports_byte_offsets_on_char_boundaries() {
    let line = "这段中文里也有针，第二个针在这里。";
    let matcher = plain("针");
    let found = hits(line, &matcher);
    assert_eq!(found.len(), 2, "两个「针」都该找到：{found:?}");
    for (start, end, text) in &found {
        assert_eq!(text, "针");
        assert!(
            line.is_char_boundary(*start) && line.is_char_boundary(*end),
            "命中区间 {start}..{end} 切进了多字节字符中间"
        );
    }
    // 第二个针的位置：第一个「针」之后隔着「，第二个」四个字符（含全角逗号）。
    assert_eq!(found[1].0, found[0].1 + "，第二个".len());
}

#[test]
fn snapshot_cjk_word_query_matches() {
    let line = "| needle 1 | value | 数据内容 |";
    let found = hits(line, &plain("数据内容"));
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].2, "数据内容");
}

#[test]
fn snapshot_emoji_offsets_survive() {
    let line = "C:\\path\\to file 0x1F600 emoji 🎯 needle";
    let found = hits(line, &plain("needle"));
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].2, "needle");
    assert!(line.is_char_boundary(found[0].0) && line.is_char_boundary(found[0].1));
}

#[test]
fn snapshot_accented_latin_folds_case() {
    let found = hits("École primaire", &plain("école"));
    assert_eq!(found.len(), 1, "重音字母的大小写折叠应当生效：{found:?}");
    assert_eq!(found[0].2, "École");
}

// ------------------------------------------------------------------ 单词边界

#[test]
fn snapshot_whole_word_rejects_word_character_neighbours() {
    let matcher = SearchMatcher::new(
        "buffer",
        options(false, true /* whole_word */, false, false),
    );
    assert_eq!(
        hits("buffer vs buffers and my_buffer", &matcher),
        vec![(0, 6, "buffer".to_string())],
        "紧邻字母或下划线的都必须被排掉"
    );
}

#[test]
fn snapshot_whole_word_accepts_punctuation_neighbours() {
    let matcher = SearchMatcher::new("foo", options(false, true, false, false));
    assert_eq!(
        hits("(foo) [foo] \"foo\" foo.", &matcher).len(),
        4,
        "全角标点与括号紧邻时仍是完整单词"
    );
}

#[test]
fn snapshot_whole_word_treats_han_characters_as_word_characters() {
    // `is_word_boundary` 用 `char::is_alphanumeric() || ch == '_'` 判定，汉字对
    // `is_alphanumeric()` 为真，所以**相邻汉字之间没有词边界**。这一条要在换引擎
    // 前后保持一致：ripgrep 的 `\b` 走 Unicode 单词字符定义，同样把汉字当单词字符。
    let matcher = SearchMatcher::new("针", options(false, true, false, false));
    assert!(
        hits("有针", &matcher).is_empty(),
        "左边紧邻汉字「有」不是完整单词"
    );
    assert!(
        hits("针头", &matcher).is_empty(),
        "右边紧邻汉字「头」不是完整单词"
    );
    assert_eq!(
        hits("，针。", &matcher).len(),
        1,
        "两侧都是标点时才算完整单词"
    );
    assert_eq!(
        hits("有针，第二针。", &matcher).len(),
        0,
        "第二个针的左边是汉字「二」，所以整行一个都不算——「逗号算边界」的直觉在这行里不成立"
    );
}

// ------------------------------------------------------------------ 正则模式

#[test]
fn snapshot_regex_alternation_and_class() {
    let matcher = SearchMatcher::new(
        "ne(e)dle|数据",
        options(false, false, true /* use_regex */, false),
    );
    assert_eq!(
        hits("a needle and 数据内容", &matcher),
        vec![
            (2, 8, "needle".to_string()),
            // 「数据」是两个汉字，各 3 字节。
            (13, 19, "数据".to_string()),
        ]
    );
}

#[test]
fn snapshot_regex_anchors_apply_per_line() {
    let matcher = SearchMatcher::new("^# ", options(false, false, true, false));
    assert_eq!(hits("# Heading", &matcher), vec![(0, 2, "# ".to_string())]);
    assert!(
        hits("see # Heading", &matcher).is_empty(),
        "^ 只认行首"
    );
}

#[test]
fn snapshot_regex_case_insensitive_by_default() {
    let matcher = SearchMatcher::new(
        "seconD [A-Z]+ linE",
        options(false, false, true, false),
    );
    assert_eq!(
        hits("second NEEDLE line", &matcher).len(),
        1,
        "默认不区分大小写，所以 [A-Z] 也能匹配小写"
    );
}

// ------------------------------------------------------------------ 模糊模式

#[test]
fn snapshot_fuzzy_range_spans_first_to_last_matched_char() {
    let matcher = SearchMatcher::new("abc", options(false, false, false, true));
    let line = "xxa yy b zz c";
    let found = hits(line, &matcher);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].0, 2, "起点必须是查询首字符出现的位置");
    assert_eq!(found[0].1, line.len(), "终点必须是最后一个被吃掉的字符之后");
    assert_eq!(found[0].2, line[2..].to_string());
}

#[test]
fn snapshot_fuzzy_is_case_insensitive() {
    let matcher = SearchMatcher::new("ab", options(true, false, false, true));
    assert_eq!(hits("A...B", &matcher).len(), 1, "模糊模式一律忽略大小写");
}

// ------------------------------------------------------------------ 已知缺陷

#[test]
fn whole_word_option_applies_in_regex_mode_now() {
    // 阶段 0 快照里这条叫 defect_regex_mode_ignores_the_whole_word_option：手写层
    // 拿到编译好的正则就提前返回（search_backend.rs:43），whole_word 的事后过滤
    // 永远走不到，实测把 NeedleHere 也算命中。换成引擎后词边界编进模式本身，
    // 两档一致，所以断言从「记录缺陷」翻成「记录修复」。
    let regex_word = SearchMatcher::new("needle", options(false, true, true, false));
    assert!(
        hits("NeedleHere", &regex_word).is_empty(),
        "正则模式 + 词边界必须排掉 NeedleHere"
    );
    assert_eq!(hits("(needle) and NEEDLE here", &regex_word).len(), 2);
    let literal_word = SearchMatcher::new("needle", options(false, true, false, false));
    assert!(hits("NeedleHere", &literal_word).is_empty());
    assert_eq!(hits("(needle) and NEEDLE here", &literal_word).len(), 2);
    // 不加词边界时两档照旧能命中粘连的那一个。
    let loose = SearchMatcher::new("needle", options(false, false, true, false));
    assert_eq!(hits("NeedleHere", &loose).len(), 1);
}

#[test]
fn defect_regex_mode_ignores_the_fuzzy_option() {
    // 现状：正则模式提前返回，fuzzy 同样被吃掉。
    let matcher = SearchMatcher::new("abc", options(false, false, true, true));
    assert!(
        hits("a...b...c", &matcher).is_empty(),
        "正则模式下模糊匹配没生效——这是缺陷"
    );
    let fuzzy_only = SearchMatcher::new("abc", options(false, false, false, true));
    assert_eq!(hits("a...b...c", &fuzzy_only).len(), 1);
}

#[test]
fn invalid_regex_reports_a_diagnosis_and_stops_searching() {
    // 阶段 0 快照里这条叫 defect_invalid_regex_degrades_to_a_literal_search：
    // 手写层 build().ok() 之后把整条非法正则当**字面量**继续搜，用户以为在跑
    // 正则。换成引擎后交回诊断，并且不再给出结果。
    for bad in ["a{2,", "(unclosed", "a|*", "[z-a]"] {
        let matcher = SearchMatcher::new(bad, options(false, false, true, false));
        assert!(
            matcher.find_in_line(&format!("prefix {bad} suffix")).is_empty(),
            "非法正则 {bad:?} 不能再搜出任何东西"
        );
        let message = matcher
            .error_message()
            .unwrap_or_else(|| panic!("非法正则 {bad:?} 必须交回诊断"));
        assert!(
            message.contains("regex parse error") && message.contains("error:"),
            "{bad:?} 的诊断应当来自引擎且说明原因：{message}"
        );
    }
    // 合法的查询不该带诊断。
    assert_eq!(
        SearchMatcher::new("a+", options(false, false, true, false)).error_message(),
        None
    );
}

#[test]
fn unicode_case_folding_now_finds_the_dotted_capital_i() {
    // 阶段 0 快照里这条叫 defect_turkish_dotted_capital_i_is_not_found：
    // 手写层用「逐字符 to_lowercase」比较，İ 折叠出两个字符（i + U+0307），
    // 于是那个分支落回「原字符 == 查询字符」，永远不相等。引擎走正确的
    // Unicode 折叠，现在找得到。
    let matcher = plain("\u{130}");
    let line = "Istanbul \u{130}stanbul";
    assert_eq!(
        hits(line, &matcher),
        vec![(9, 11, "\u{130}".to_string())],
        "İ 的大小写不敏感匹配现在应当成立"
    );
    // 重音字母这一族本来就对着，翻修之后不能退回去。
    assert_eq!(hits("École primaire", &plain("école")).len(), 1);
}

// ------------------------------------------------------------------ 字符边界

#[test]
fn snapshot_zero_width_regex_matches_only_char_boundaries() {
    // 这一条是换引擎时唯一的硬闸门：grep-regex 的 find_iter 在零宽命中上按
    // **字节**步进，会在「中」「文」这些三字节字符内部吐出 8..8 / 9..9 /
    // 11..11 / 12..12；现状的 regex crate 按**字符**步进所以不会。
    // 一旦这些越界区间流进替换路径（find_replace.rs:408 的 line[start..end]）
    // 就是 panic。所以新引擎必须在出口把它们滤掉，这条断言原样保持绿。
    let line = "second 中文 ok";
    let matcher = SearchMatcher::new("a*", options(false, false, true, false));
    let found = matcher.find_in_line(line);
    assert!(!found.is_empty(), "a* 至少要在每个字符边界给一个零宽命中");
    for range in &found {
        assert!(
            line.is_char_boundary(range.start) && line.is_char_boundary(range.end),
            "零宽命中 {}..{} 切进了多字节字符中间",
            range.start,
            range.end
        );
        assert!(range.is_empty(), "这一族的命中必须全是零宽");
    }
    // 逐位钉住起点：换引擎后多出任何一个非边界起点，这条就红。
    let starts: Vec<usize> = found.iter().map(|range| range.start).collect();
    assert_eq!(
        starts,
        vec![0, 1, 2, 3, 4, 5, 6, 7, 10, 13, 14, 15, 16],
        "零宽命中的起点集合 = 每个字符边界的位置"
    );
}

#[test]
fn snapshot_other_zero_width_patterns_stay_on_char_boundaries() {
    let line = "中 a 文 b";
    for pattern in [r"^", r"\b", "x*", r"\p{Han}*"] {
        let matcher = SearchMatcher::new(pattern, options(false, false, true, false));
        for range in matcher.find_in_line(line) {
            assert!(
                line.is_char_boundary(range.start) && line.is_char_boundary(range.end),
                "{pattern:?} 的命中 {}..{} 切进了字符中间",
                range.start,
                range.end
            );
        }
    }
}

// ------------------------------------------------------------------ 跨行

#[test]
fn defect_patterns_cannot_span_lines() {
    // 搜索是逐行喂的，所以带 \n 的模式永远空手。现状实现与调用方都建立在
    // 「一个命中不跨行」之上，跨行能力要等阶段 3。
    let source = "a needle.\nsecond line\n";
    let matcher = SearchMatcher::new(
        r"needle\.\nsecond",
        options(false, false, true, false),
    );
    assert_eq!(
        find_document_match_from(source, &matcher, 0, false),
        None,
        "跨行模式在当前实现里必然搜不到"
    );
    // 同一段文本，行内模式就有。
    let inline = SearchMatcher::new("needle", options(false, false, true, false));
    assert_eq!(
        find_document_match_from(source, &inline, 0, false),
        Some(Range { start: 2, end: 8 })
    );
}

// ------------------------------------------------------------------ 导航

#[test]
fn snapshot_find_document_match_from_walks_and_wraps() {
    let source = "aa needle bb needle cc\nneedle dd\n";
    let matcher = plain("needle");
    let first = find_document_match_from(source, &matcher, 0, false).expect("first");
    assert_eq!(first, Range { start: 3, end: 9 });
    let second = find_document_match_from(source, &matcher, first.end, false).expect("second");
    assert_eq!(second, Range { start: 13, end: 19 });
    let third = find_document_match_from(source, &matcher, second.end, false).expect("third");
    assert_eq!(third, Range { start: source.find("needle dd").unwrap(), end: source.find("needle dd").unwrap() + 6 });
    // 走到头再往前：回绕到第一个。
    let wrap = find_document_match_from(source, &matcher, source.len(), false).expect("wrap");
    assert_eq!(wrap, first);
    // 反向：从文档末尾往回走。
    let back = find_document_match_from(source, &matcher, source.len(), true).expect("back");
    assert_eq!(back, third);
    let back2 = find_document_match_from(source, &matcher, back.start, true).expect("back2");
    assert_eq!(back2, second);
    // 反向走到头：回绕到最后一个。
    let backwrap = find_document_match_from(source, &matcher, 0, true).expect("backwrap");
    assert_eq!(backwrap, third);
}

#[test]
fn snapshot_find_document_match_from_is_empty_query_safe() {
    assert_eq!(find_document_match_from("anything", &plain(""), 0, false), None);
    assert_eq!(
        find_document_match_from("anything", &plain("nope"), 0, false),
        None
    );
    // from 落在多字节字符中间时不许 panic：现状会往前退到边界。
    let source = "needle 中文 needle";
    let range = find_document_match_from(source, &plain("中文"), 9, false).expect("found");
    assert_eq!(&source[range.clone()], "中文");
}

#[test]
fn snapshot_find_document_match_from_clamps_past_the_end() {
    let source = "only needle here";
    let range = find_document_match_from(source, &plain("needle"), source.len() + 500, false)
        .expect("wraps to first");
    assert_eq!(&source[range.clone()], "needle");
}

// ------------------------------------------------------------------ 结果列表

#[test]
fn snapshot_search_document_source_line_offsets_and_preview() {
    let source = "# Title\nthe needle appears twice, needle again\nlast line\n";
    let matcher = plain("needle");
    let found = search_document_source(
        source,
        &matcher,
        std::path::Path::new("/tmp/x.md"),
        "x.md",
        200,
    );
    assert_eq!(found.len(), 2, "一行里的两个命中都要列出来：{found:?}");
    let line_two_start = 8usize; // "# Title\n" 之后
    assert_eq!(found[0].line, Some(2), "行号是 1 基的磁盘行号");
    assert_eq!(found[0].match_range, Some(Range { start: 4, end: 10 }));
    assert_eq!(
        found[0].source_range,
        Some(Range {
            start: line_two_start + 4,
            end: line_two_start + 10
        })
    );
    assert!(
        found[0].match_range.as_ref().unwrap().end <= found[1].match_range.as_ref().unwrap().start,
        "同一行里的两个命中必须按位置递增且不重叠"
    );
    assert_eq!(found[1].line, Some(2));
    assert!(found[0].preview.contains("needle"));
    assert_eq!(found[0].label, "x.md");
}

#[test]
fn defect_document_scope_ordinals_are_not_unique() {
    // 现状：`search_document_source` 里同一行的第二个命中走 else 分支，取的是
    // **已经自增过**的 `content_ordinal`，于是它和下一根含词行的第一个命中拿到同一个
    // 序号（search_backend.rs:487-494）。
    //
    // 这一条现在是**潜伏**的而不是用户可见的：文档范围的命中跳转读的是
    // `source_range`（render_search.rs:771），不读 ordinal；ordinal 只有工作区范围
    // 在用，而工作区那边每行只收一个命中（search_backend.rs:392），序号天然唯一。
    // 记下来是因为阶段 3 要把工作区也接上同一个命中表，届时两套 ordinal 口径必须
    // 分开算，不能顺手把这个重复值当成「第 k 个含词行」。
    let source = "needle one\nno hit here\nneedle two and needle three\nneedle four\n";
    let found = search_document_source(
        source,
        &plain("needle"),
        std::path::Path::new("/tmp/x.md"),
        "x.md",
        200,
    );
    assert_eq!(
        found
            .iter()
            .map(|hit| (hit.line, hit.match_ordinal))
            .collect::<Vec<_>>(),
        vec![
            (Some(1), Some(0)),
            (Some(3), Some(1)),
            // 第三行的第二个命中 = 2，而第四行的第一个命中也是 2。
            (Some(3), Some(2)),
            (Some(4), Some(2)),
        ],
        "文档范围的 ordinal 会重复——这是缺陷，记录现状"
    );
}

#[test]
fn snapshot_search_document_source_honours_the_limit() {
    let source = "needle\n".repeat(30);
    let found = search_document_source(
        &source,
        &plain("needle"),
        std::path::Path::new("/tmp/x.md"),
        "x.md",
        7,
    );
    assert_eq!(found.len(), 7);
}

#[test]
fn snapshot_search_document_source_keeps_a_line_without_a_newline() {
    let source = "needle at the very end";
    let found = search_document_source(
        source,
        &plain("needle"),
        std::path::Path::new("/tmp/x.md"),
        "x.md",
        200,
    );
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].line, Some(1));
    assert_eq!(&source[found[0].source_range.clone().unwrap()], "needle");
}

// ------------------------------------------------------------------ 文件名

#[test]
fn snapshot_filename_matching_is_substring_or_fuzzy() {
    let matcher = plain("readme");
    assert!(matcher.matches_filename("README.md"));
    assert!(!matcher.matches_filename("main.rs"));
    let case_sensitive = SearchMatcher::new("README", options(true, false, false, false));
    assert!(case_sensitive.matches_filename("README.md"));
    assert!(!case_sensitive.matches_filename("readme.md"));
    let fuzzy = SearchMatcher::new("rdme", options(false, false, false, true));
    assert!(fuzzy.matches_filename("readme.md"));
    assert!(!fuzzy.matches_filename("main.rs"));
}
