//! ripgrep 引擎封装的测试。
//!
//! 核心是一条**对照测试**：同一份语料、同一种模式，新引擎交出的命中区间集合
//! 必须与 `search_backend.rs` 里那套手写实现逐位相同。手写层马上要退役，这条
//! 就是它临终前留下的口径。已知的、有意为之的差异只有四处，各自有独立用例点名，
//! 不在对照测试的语料里。

use super::super::{CompiledQuery, QueryError, SearchOptions};
use std::ops::Range;

fn options(match_case: bool, whole_word: bool, use_regex: bool, fuzzy: bool) -> SearchOptions {
    SearchOptions {
        match_case,
        whole_word,
        use_regex,
        fuzzy,
    }
}

fn compile(query: &str, options: SearchOptions) -> CompiledQuery {
    CompiledQuery::compile(query, options).expect("编译成功")
}

/// 正则模式（大小写不敏感，与面板默认一致）。跨行档在正则模式下由引擎打开。
fn regex_options() -> SearchOptions {
    SearchOptions {
        use_regex: true,
        ..SearchOptions::default()
    }
}

/// 手写实现那一套算法的**誊本**（逐行喂，行内命中换算成整篇绝对区间）。
///
/// 为什么要在测试里留一份抄来的实现：`SearchMatcher` 换成转调引擎之后，这条对照
/// 就退化成「引擎和引擎比」，等于没比。手写层从生产代码里退役（`8e98c8c`）之后，
/// 唯一的出口证明只能靠把它的算法原样留在测试里——和 §4.7 那条 ordinal 对照测试
/// 用的是同一个办法。誊本取自 `git show e8c8cf3:src/editor/workspace/search_backend.rs`，
/// 连它的三个已知缺陷一起抄（正则模式吃掉 `whole_word`、吃掉 `fuzzy`、非法正则静默
/// 退化成字面量），所以对照矩阵只比**没有有意差异**的那几种形态。
fn hand_written_hits(source: &str, query: &str, options: SearchOptions) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut absolute = 0usize;
    for raw_line in source.split_inclusive('\n') {
        let line = raw_line.strip_suffix('\n').unwrap_or(raw_line);
        for range in hand_written_find_in_line(line, query, options) {
            out.push((absolute + range.start)..(absolute + range.end));
        }
        absolute += raw_line.len();
    }
    out
}

/// 旧 `SearchMatcher::find_in_line` 的分支顺序，一字不改。
fn hand_written_find_in_line(line: &str, query: &str, options: SearchOptions) -> Vec<Range<usize>> {
    if options.use_regex {
        // 旧实现在这里 `.build().ok()`：非法正则静默退化成「没有正则」，
        // 且 `whole_word`、`fuzzy` 两个开关在本分支被完全忽略（缺陷 #1、#2）。
        let regex = regex::RegexBuilder::new(query)
            .case_insensitive(!options.match_case)
            .build();
        if let Ok(regex) = regex {
            return regex.find_iter(line).map(|m| m.start()..m.end()).collect();
        }
    }
    if options.fuzzy && !options.use_regex {
        return hand_written_fuzzy_ranges(line, query);
    }
    let mut ranges = if options.match_case {
        line.match_indices(query)
            .map(|(start, matched)| start..start + matched.len())
            .collect()
    } else {
        hand_written_case_insensitive_ranges(line, query)
    };
    if options.whole_word {
        ranges.retain(|range| hand_written_is_word_boundary(line, range));
    }
    ranges
}

/// 旧 `case_insensitive_ranges`：ASCII 走滑窗，非 ASCII 逐字符小写比较。
fn hand_written_case_insensitive_ranges(line: &str, query: &str) -> Vec<Range<usize>> {
    if query.is_empty() || line.is_empty() || line.len() < query.len() {
        return Vec::new();
    }
    if query.is_ascii() {
        let mut ranges = Vec::new();
        let last = line.len() - query.len();
        let bytes = line.as_bytes();
        let first = query.as_bytes()[0];
        let mut start = 0;
        while start <= last {
            if bytes[start].eq_ignore_ascii_case(&first)
                && bytes[start..start + query.len()].eq_ignore_ascii_case(query.as_bytes())
            {
                ranges.push(start..start + query.len());
                start += query.len();
            } else {
                start += 1;
            }
        }
        return ranges;
    }
    let query_chars: Vec<char> = query.to_lowercase().chars().collect();
    if query_chars.is_empty() {
        return Vec::new();
    }
    let mut ranges = Vec::new();
    let char_positions: Vec<(usize, char)> = line.char_indices().collect();
    for start_index in 0..char_positions.len() {
        let mut query_index = 0usize;
        let mut cursor = start_index;
        while cursor < char_positions.len() && query_index < query_chars.len() {
            let (_, line_char) = char_positions[cursor];
            let mut folded = line_char.to_lowercase();
            let matches = match (folded.next(), folded.next()) {
                (Some(first), None) => first == query_chars[query_index],
                _ => line_char == query_chars[query_index],
            };
            if !matches {
                break;
            }
            query_index += 1;
            cursor += 1;
        }
        if query_index == query_chars.len() {
            let start = char_positions[start_index].0;
            let end = if cursor < char_positions.len() {
                char_positions[cursor].0
            } else {
                line.len()
            };
            ranges.push(start..end);
        }
    }
    ranges
}

/// 旧 `fuzzy_subsequence_ranges`：子序列匹配，区间从首字符覆盖到末字符。
fn hand_written_fuzzy_ranges(line: &str, query: &str) -> Vec<Range<usize>> {
    let query_chars: Vec<char> = query
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .flat_map(|ch| ch.to_lowercase())
        .collect();
    if query_chars.is_empty() || line.is_empty() {
        return Vec::new();
    }
    let positions: Vec<(usize, char)> = line.char_indices().collect();
    let mut ranges = Vec::new();
    for start_index in 0..positions.len() {
        let (start_offset, start_char) = positions[start_index];
        let folded_start = start_char.to_lowercase().next().unwrap_or(start_char);
        if folded_start != query_chars[0] {
            continue;
        }
        let mut query_index = 1usize;
        let mut cursor = start_index + 1;
        while cursor < positions.len() && query_index < query_chars.len() {
            let (_, line_char) = positions[cursor];
            let folded = line_char.to_lowercase().next().unwrap_or(line_char);
            if folded == query_chars[query_index] {
                query_index += 1;
            }
            cursor += 1;
        }
        if query_index == query_chars.len() {
            let end = positions.get(cursor).map(|(offset, _)| *offset).unwrap_or(line.len());
            ranges.push(start_offset..end);
        }
    }
    ranges
}

/// 旧 `is_word_boundary`。
fn hand_written_is_word_boundary(line: &str, range: &Range<usize>) -> bool {
    let word_char = |ch: char| ch.is_alphanumeric() || ch == '_';
    let before = line[..range.start]
        .chars()
        .next_back()
        .map(word_char)
        .unwrap_or(false);
    let after = line[range.end..]
        .chars()
        .next()
        .map(word_char)
        .unwrap_or(false);
    !before && !after
}

/// 中英混排 + 表格 + 代码围栏 + 表情 + 空行 + 无尾换行的语料。
fn corpus() -> Vec<String> {
    vec![
        "# 标题 Heading\n\n正文里出现 needle 两次，needle again。中文查询也来一个：针。\n".to_string(),
        "| col | 中文 |\n|---|---|\n| needle 1 | 数据内容 |\n| Needle 2 | 针 |\n".to_string(),
        "```rust\nfn needle() { let x = 1; }\n```\n\n".to_string(),
        "emoji 🎯 needle 结尾没有换行".to_string(),
        "\n\n\n".to_string(),
        "a.b  a*b  (x)  [abc]  end$  C:\\path  a|b  +?{}()\n".to_string(),
        "buffer vs buffers and my_buffer _needle-needle_ (needle)\n".to_string(),
        "  缩进 needle 缩进\n\t制表 needle\n".to_string(),
        "École école İ istanbul ß ss SS\n".to_string(),
        "needleNEEDLEneedle needle_neEDLE\n".to_string(),
    ]
}

/// 必须与手写层逐位一致的形态。单词边界在正则模式下、非法正则、Unicode 折叠
/// 三处的**有意差异**不在这个矩阵里，各自有独立用例。
fn agreeing_modes() -> Vec<(&'static str, SearchOptions)> {
    vec![
        (
            "字面量 / 忽略大小写",
            options(false, false, false, false),
        ),
        ("字面量 / 区分大小写", options(true, false, false, false)),
        ("正则 / 忽略大小写", options(false, false, true, false)),
        ("正则 / 区分大小写", options(true, false, true, false)),
        ("模糊", options(false, false, false, true)),
        ("字面量 / 全词", options(false, true, false, false)),
        ("字面量 / 全词 / 区分大小写", options(true, true, false, false)),
    ]
}

#[test]
#[ignore = "慢用例（>1s）：本地默认跳过，CI 跑"]
fn engine_agrees_with_the_hand_written_matcher_on_every_shape() {
    let queries = [
        "needle", "NEEDLE", "针", "数据内容", "标题", "a.b", "a*b", "(x)", "[abc]",
        "end$", "C:\\path", "a|b", "+?{}()", "buffer", "needle.*again", r"^#+ ",
        r"needle|针", r"\bneedle\b", "e.", "🎯", "ss", "İ", "学",
    ];
    let mut compared = 0usize;
    let mut skipped = 0usize;
    for source in corpus() {
        for (label, opts) in agreeing_modes() {
            for query in queries {
                if opts.fuzzy && query.is_empty() {
                    continue;
                }
                // 有意差异之一：İ 折叠出两个字符，手写层的逐字符比较认不出，
                // 引擎走正确的 Unicode 折叠能认出。由
                // unicode_case_folding_finds_what_the_hand_written_fold_missed 覆盖。
                if query == "\u{130}" {
                    continue;
                }
                let engine = match CompiledQuery::compile(query, opts) {
                    Ok(engine) => engine,
                    Err(_) => {
                        // 正则模式下有些语料本身是非法正则（`C:\path` 的 `\p`、
                        // `+?{}()` 行首的量词）。新引擎交回诊断、手写层静默退化成
                        // 字面量——这是有意的第四处差异，由
                        // an_invalid_regex_reports_the_engine_diagnosis 单独覆盖，
                        // 不进这张对照矩阵。
                        assert!(
                            opts.use_regex,
                            "非正则模式下查询 {query:?} 不该编译失败（模式 {label}）"
                        );
                        skipped += 1;
                        continue;
                    }
                };
                let got: Vec<Range<usize>> = engine
                    .find_all(source.as_bytes())
                    .into_iter()
                    .map(|hit| hit.range)
                    .collect();
                let want = hand_written_hits(&source, query, opts);
                compared += 1;
                assert_eq!(
                    got, want,
                    "语料 {:?} / 模式 {label} / 查询 {query:?} 命中不一致",
                    &source[..source.len().min(40)]
                );
            }
        }
    }
    assert!(compared > 800, "对照覆盖面不足，只比了 {compared} 组");
    assert!(skipped > 0, "非法正则那一族应当被跳过若干组，实际跳过 {skipped}");
}

#[test]
fn engine_never_returns_a_range_inside_a_multi_byte_character() {
    // 实测过的坑：grep-regex 的 find_iter 在零宽命中上按字节步进，会在
    // 「中」「文」这类三字节字符内部吐出区间。出口过滤必须一直有效，
    // 覆盖各种能匹配空的模式。
    let source = "second 中文 ok\n中中中\n😀 emoji 🎯 needle\n";
    for pattern in ["a*", "x*", r"\b", "^", "$", r"\p{Han}*", "()", ".*"] {
        let engine = compile(pattern, options(false, false, true, false));
        for hit in engine.find_all(source.as_bytes()) {
            assert!(
                source.is_char_boundary(hit.range.start)
                    && source.is_char_boundary(hit.range.end),
                "{pattern:?} 的命中 {:?} 切进了多字节字符中间",
                hit.range
            );
        }
    }
}

#[test]
fn zero_width_matches_land_on_the_same_positions_as_the_old_engine() {
    // 手写层只在字符边界上给零宽命中；这条把「过滤后一致」钉到具体偏移上，
    // 防止将来把过滤器改掉而没人发现。
    let line = "second 中文 ok";
    let engine = compile("a*", options(false, false, true, false));
    let starts: Vec<usize> = engine
        .find_all(line.as_bytes())
        .into_iter()
        .map(|hit| hit.range.start)
        .collect();
    assert_eq!(starts, vec![0, 1, 2, 3, 4, 5, 6, 7, 10, 13, 14, 15, 16]);
    assert_eq!(
        starts,
        hand_written_hits(line, "a*", options(false, false, true, false))
            .into_iter()
            .map(|range| range.start)
            .collect::<Vec<_>>()
    );
}

#[test]
fn line_numbers_match_the_disk_numbering_of_the_same_text() {
    let source = "one\nneedle two\nthree\nneedle four\n";
    let engine = compile("needle", SearchOptions::default());
    let hits = engine.find_all(source.as_bytes());
    assert_eq!(
        hits.iter()
            .map(|hit| (hit.line, hit.range.clone()))
            .collect::<Vec<_>>(),
        vec![
            // "one\n" 占 4 个字节，所以第二行的 needle 从 4 起。
            (Some(2), 4..10),
            (Some(4), 21..27),
        ]
    );
    // 命中原文核对：行号与区间不能各说各话。
    for hit in &hits {
        assert_eq!(&source[hit.range.clone()], "needle");
        let counted = source[..hit.range.start].matches('\n').count() + 1;
        assert_eq!(
            hit.line,
            Some(counted as u64),
            "line_number 必须等于「前面有几个换行 + 1」"
        );
    }
}

#[test]
fn a_file_with_crlf_line_endings_keeps_the_carriage_return_out_of_matches() {
    // 工作区扫描读的是磁盘原始字节，CRLF 文件的「行」在摘掉 \n 之后仍留着 \r。
    // 手写层就是这个口径（strip_suffix('\n')），保持一致，否则行内偏移会漂一位。
    let source = "a needle here\r\nsecond NEEDLE line\r\n";
    let engine = compile("needle", SearchOptions::default());
    let hits: Vec<Range<usize>> = engine
        .find_all(source.as_bytes())
        .into_iter()
        .map(|hit| hit.range)
        .collect();
    assert_eq!(hits, hand_written_hits(source, "needle", SearchOptions::default()));
    for hit in &hits {
        assert_eq!(&source[hit.clone()].to_lowercase(), "needle");
        assert!(!source[hit.clone()].contains('\r'));
    }
    // 无尾换行的最后一行也要能搜到。
    let engine = compile("line", SearchOptions::default());
    assert_eq!(engine.find_all(source.as_bytes()).len(), 1);
}

#[test]
fn an_empty_query_finds_nothing_and_is_marked_empty() {
    for query in ["", "   ", "\t"] {
        let engine = compile(query, SearchOptions::default());
        assert!(engine.is_empty(), "{query:?} 应当算空查询");
        assert!(engine.find_all(b"anything needle").is_empty());
    }
    assert!(!compile("x", SearchOptions::default()).is_empty());
}

// --------------------------------------------------- 有意的行为改进（对着手写层比）

#[test]
fn whole_word_now_applies_in_regex_mode_too() {
    // 手写层拿到编译好的正则就提前返回，「ab」按钮在正则模式下完全不起作用。
    // 引擎把 word(true) 编进模式里，两档一起生效——这是快照测试里
    // defect_regex_mode_ignores_the_whole_word_option 那条缺陷的修复点。
    let regex_word = options(false, true, true, false);
    let literal_word = options(false, true, false, false);
    let engine = compile("needle", regex_word);
    assert_eq!(engine.find_all(b"NeedleHere").len(), 0, "正则+词边界要排掉 NeedleHere");
    assert_eq!(engine.find_all(b"(needle) and NEEDLE here").len(), 2);
    // 同一查询在字面量档下的取舍必须一模一样。
    let literal = compile("needle", literal_word);
    assert_eq!(literal.find_all(b"NeedleHere").len(), 0);
    assert_eq!(literal.find_all(b"(needle) and NEEDLE here").len(), 2);
}

#[test]
fn an_invalid_regex_reports_the_engine_diagnosis_instead_of_degrading() {
    // 手写层 build().ok() 之后当字面量继续搜；这里必须交回诊断并停止搜索。
    for bad in ["a{2,", "(unclosed", "a|*", r"\p{Bad}", "[z-a]", r"(?i(a"] {
        let error = match CompiledQuery::compile(bad, options(false, false, true, false)) {
            Ok(_) => panic!("非法正则 {bad:?} 必须编译失败"),
            Err(error) => error,
        };
        assert!(
            error.message.contains("regex parse error"),
            "{bad:?} 的诊断应当来自引擎：{error:?}"
        );
        assert!(
            error.message.contains("error:"),
            "{bad:?} 的诊断应当带出错原因：{}",
            error.message
        );
    }
    let message = match CompiledQuery::compile("[z-a]", options(false, false, true, false)) {
        Ok(_) => panic!("[z-a] 必须编译失败"),
        Err(QueryError { message }) => message,
    };
    assert!(message.contains("invalid character class range"), "{message}");
}

#[test]
fn a_literal_query_needs_no_escaping_and_accepts_regex_metacharacters() {
    // fixed_strings 让元字符天然是字面量，不再依赖手写层「当子串找」那条分支。
    for query in ["a.b", "a*b", "(x)", "[abc]", "end$", "C:\\path", "a|b"] {
        let engine = compile(query, SearchOptions::default());
        let line = format!("prefix {query} suffix");
        let hits = engine.find_all(line.as_bytes());
        assert_eq!(hits.len(), 1, "{query:?} 应当按字面量命中一次：{hits:?}");
        assert_eq!(&line[hits[0].range.clone()], query);
    }
}

#[test]
fn unicode_case_folding_finds_what_the_hand_written_fold_missed() {
    // 手写层用「逐字符 to_lowercase」比较，İ 折叠出两个字符于是永远不相等。
    let engine = compile("\u{130}", SearchOptions::default());
    assert_eq!(
        engine.find_all("Istanbul \u{130}stanbul".as_bytes()).len(),
        1,
        "İ 的大小写不敏感匹配现在应当成立"
    );
    // 重音字母这一族两边都得对（手写层本来就是对的）。
    assert_eq!(
        compile("école", SearchOptions::default())
            .find_all("École primaire".as_bytes())
            .len(),
        1
    );
}

#[test]
fn fuzzy_mode_still_spans_first_to_last_matched_character() {
    let engine = compile("abc", options(false, false, false, true));
    let line = "xxa yy b zz c";
    let hits = engine.find_all(line.as_bytes());
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].range, 2..line.len());
    assert_eq!(hits[0].line, Some(1));
}

#[test]
fn a_multi_line_document_scans_without_losing_the_last_line() {
    let source = "needle at start\nmiddle\nneedle at end";
    let engine = compile("needle", SearchOptions::default());
    let hits = engine.find_all(source.as_bytes());
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[1].line, Some(3), "无尾换行的最后一行也要数到");
    assert_eq!(&source[hits[1].range.clone()], "needle");
}

#[test]
fn find_in_line_matches_a_single_line_without_offsets() {
    let engine = compile("needle", SearchOptions::default());
    assert_eq!(
        engine.find_in_line("a needle and NEEDLE"),
        vec![2..8, 13..19]
    );
    assert!(engine.find_in_line("").is_empty());
}

#[test]
fn first_per_line_keeps_the_first_hit_of_each_matching_line() {
    let source = "needle one and NEEDLE two\nquiet\n第三行 needle 在此\n";
    let engine = compile("needle", SearchOptions::default());
    let hits = engine.find_first_per_line(source.as_bytes(), 10);
    assert_eq!(
        hits.iter()
            .map(|hit| (hit.range.clone(), hit.line))
            .collect::<Vec<_>>(),
        vec![(0..6, Some(1)), (42..48, Some(3))],
        "每根含命中的行只留第一个命中，行号按 LF 数（第三行起点 32，第三行 6 字节 + 空格 1 字节）"
    );
}

#[test]
fn first_per_line_stops_once_it_has_enough_lines() {
    // 工作区的 200 条上限走的是这条早停：收满就不读文件剩余部分。
    let source = "a needle\n".repeat(500);
    let engine = compile("needle", SearchOptions::default());
    let hits = engine.find_first_per_line(source.as_bytes(), 3);
    assert_eq!(hits.len(), 3, "要 3 行就给 3 行");
    assert_eq!(
        hits.iter().map(|hit| hit.line).collect::<Vec<_>>(),
        vec![Some(1), Some(2), Some(3)]
    );
    assert!(engine.find_first_per_line(source.as_bytes(), 0).is_empty());
}

#[test]
fn first_per_line_is_just_find_all_grouped_by_line() {
    // 两条出口必须是同一个规则：早停只是提前收工，不能改变留下来的那些。
    let corpora = [
        "needle one and NEEDLE two\nquiet\n第三行 needle 在此\n",
        "第一行 needle\n第二行 needle 与 needle 两次\n没有命中\n结尾 needle",
        "a\nneedle\r\nneedle at crlf\r\n\n\nneedle\n",
        "中文 needle 中文\n",
    ];
    let modes = [
        ("默认", SearchOptions::default()),
        (
            "区分大小写",
            SearchOptions {
                match_case: true,
                ..SearchOptions::default()
            },
        ),
        (
            "正则",
            SearchOptions {
                use_regex: true,
                ..SearchOptions::default()
            },
        ),
        (
            "模糊",
            SearchOptions {
                fuzzy: true,
                ..SearchOptions::default()
            },
        ),
    ];
    for source in corpora {
        for (label, options) in modes {
            let engine = compile("needle", options);
            let grouped =
                CompiledQuery::first_hit_per_line(engine.find_all(source.as_bytes()), usize::MAX);
            let early_stopped = engine.find_first_per_line(source.as_bytes(), usize::MAX);
            assert_eq!(
                early_stopped, grouped,
                "语料 {source:?} 模式 {label}：早停出口与全表分组必须同一条"
            );
        }
    }
}

#[test]
fn a_regex_pattern_can_span_lines() {
    // 阶段 3 的入口：正则模式下引擎开着 `-U` 那一档，模式里写了换行就真跨行。
    let source = "first\nneedle.\nsecond\nlast\n";
    let engine = compile("needle\\.\\nsecond", regex_options());
    let hits = engine.find_all(source.as_bytes());
    assert_eq!(
        hits.iter().map(|hit| hit.range.clone()).collect::<Vec<_>>(),
        vec![6..20],
        "命中要一直延伸到第二行的 second：{hits:?}"
    );
    assert_eq!(&source[6..20], "needle.\nsecond");
    assert_eq!(hits[0].line, Some(2), "行号报的是命中**起始**所在行");
}

#[test]
fn a_dot_still_does_not_cross_a_line() {
    // 防直觉的一条（风险登记 R12）：开着跨行不等于 `.` 能跨行——`.` 默认不匹配
    // `\n`，要跨得写 `\n`、`[\s\S]` 或开 `(?s)`。与 ripgrep 的行为一致。
    let source = "first\nneedle.\nsecond\nlast\n";
    let engine = compile("needle.{0,12}last", regex_options());
    assert!(engine.find_all(source.as_bytes()).is_empty(), "`.` 不该跨过换行");
    let engine = compile("needle.*second", regex_options());
    assert!(engine.find_all(source.as_bytes()).is_empty(), "同一根行里没有 second");
    let engine = compile("needle[\\s\\S]*?last", regex_options());
    let hits = engine.find_all(source.as_bytes());
    assert_eq!(
        hits.iter().map(|hit| hit.range.clone()).collect::<Vec<_>>(),
        vec![6..25],
        "写了 [\\s\\S] 就该跨到 last"
    );
}

#[test]
fn a_match_ending_with_a_newline_keeps_every_byte() {
    // 实测抓到的坑：`local_matches` 原本无条件把 report 字节尾部的换行摘掉
    // （行式策略需要，因为那时 bytes() 是整行），跨行策略下 bytes() 就是命中本身，
    // 一摘就把 `needle\\n` 这类模式整个吃掉，`[\\s\\S]+` 也少算一个字节。
    let source = "needle\nsecond\n";
    let cases = [
        ("needle\\n", 0..7),
        ("n.*?e\\n", 0..7),
        ("[\\s\\S]+", 0..14),
        ("needle\\nsecond", 0..13),
    ];
    for (pattern, expected) in cases {
        let engine = compile(pattern, regex_options());
        let hits = engine.find_all(source.as_bytes());
        assert_eq!(
            hits.iter().map(|hit| hit.range.clone()).collect::<Vec<_>>(),
            vec![expected.clone()],
            "模式 {pattern} 的区间应当含住结尾的换行：{hits:?}"
        );
        assert!(
            source.is_char_boundary(expected.start) && source.is_char_boundary(expected.end),
            "模式 {pattern} 的区间两端都必须在字符边界上"
        );
    }
}

#[test]
fn turning_on_multi_line_does_not_move_the_line_anchored_semantics() {
    // 开着 `-U` 之后，`^` 仍然按「每根行的行首」算——这与换引擎前逐行喂的口径
    // 相同（旧实现把每一行单独喂给正则，`^` 自然就是行首）。这条挡的是
    // 「开跨行把 ^ 改成整篇开头」这种静默漂移。
    let source = "first\nneedle.\nsecond\nlast\n";
    let engine = compile("^needle", regex_options());
    let hits = engine.find_all(source.as_bytes());
    assert_eq!(
        hits.iter().map(|hit| hit.range.clone()).collect::<Vec<_>>(),
        vec![6..12]
    );
    assert_eq!(hits[0].line, Some(2));
    // 字面量模式（输入框单行，跨不跨行都没区别）结果一致。
    let plain = compile("needle", SearchOptions::default());
    assert_eq!(
        plain.find_all(source.as_bytes())
            .iter()
            .map(|hit| hit.range.clone())
            .collect::<Vec<_>>(),
        vec![6..12]
    );
}

#[test]
fn a_zero_width_match_on_a_line_boundary_is_reported_once() {
    // 跨行档的实测坑：行尾/行首是同一个字节位置，零宽命中会被当成「上一行结尾」
    // 和「下一行开头」各报一次。区间相同的相邻两次报告就是同一个命中，只留一条——
    // 否则结果列表、跳转都会多出一个不存在的命中，`x*` 在 "ab\nab\n" 上实测就是这样。
    let source = "ab\nab\n";
    let engine = compile("x*", regex_options());
    let hits = engine.find_all(source.as_bytes());
    assert_eq!(
        hits.iter().map(|hit| hit.range.clone()).collect::<Vec<_>>(),
        vec![0..0, 1..1, 2..2, 3..3, 4..4, 5..5, 6..6],
        "每个位置一条，行交界不许重复"
    );
    let mut starts = hits
        .iter()
        .map(|hit| hit.range.start)
        .collect::<Vec<_>>();
    starts.dedup();
    assert_eq!(starts.len(), hits.len(), "命中区间必须互不重复");

    // 空行同样只在交界处报一次。
    let engine = compile("x*", regex_options());
    let ranges = engine
        .find_all("a\n\nb\n".as_bytes())
        .iter()
        .map(|hit| hit.range.clone())
        .collect::<Vec<_>>();
    let mut deduped = ranges.clone();
    deduped.dedup();
    assert_eq!(deduped, ranges, "空行相邻的两个边界也不该重复：{ranges:?}");
}
