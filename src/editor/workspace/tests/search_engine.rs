//! ripgrep 引擎封装的测试。
//!
//! 核心是一条**对照测试**：同一份语料、同一种模式，新引擎交出的命中区间集合
//! 必须与 `search_backend.rs` 里那套手写实现逐位相同。手写层马上要退役，这条
//! 就是它临终前留下的口径。已知的、有意为之的差异只有四处，各自有独立用例点名，
//! 不在对照测试的语料里。

use super::super::{CompiledQuery, QueryError, SearchMatcher, SearchOptions};
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

/// 手写实现的口径：逐行喂，行内命中换算成整篇的绝对区间。
fn legacy_hits(source: &str, query: &str, options: SearchOptions) -> Vec<Range<usize>> {
    let matcher = SearchMatcher::new(query, options);
    let mut out = Vec::new();
    let mut absolute = 0usize;
    for raw_line in source.split_inclusive('\n') {
        let line = raw_line.strip_suffix('\n').unwrap_or(raw_line);
        for range in matcher.find_in_line(line) {
            out.push((absolute + range.start)..(absolute + range.end));
        }
        absolute += raw_line.len();
    }
    out
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
    ]
}

#[test]
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
                let want = legacy_hits(&source, query, opts);
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
        legacy_hits(line, "a*", options(false, false, true, false))
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
    assert_eq!(hits, legacy_hits(source, "needle", SearchOptions::default()));
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
