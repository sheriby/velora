//! 搜索匹配层：ripgrep 本体的库化封装（进程内调用，不起 `rg` 子进程）。
//!
//! 为什么要这一层：`search_backend.rs` 里那套手写的匹配（自研大小写不敏感滑窗、
//! 逐行喂正则）有三个结构问题——非 ASCII 查询给每一行都分配一张字符表再做
//! O(行长²) 的折叠；正则模式提前返回导致「单词边界」「模糊」两个选项被吃掉；
//! 逐行匹配让任何带换行的模式必然空手。这一层把匹配交给 `grep-searcher` +
//! `grep-regex`，模式组合（大小写 / 字面量 / 单词边界 / 跨行）全部由引擎完成。
//!
//! 三条只有实测才能确定的约束，代码里都按它们写：
//! 1. `SinkMatch::bytes()` 交回的是**命中所在的整行**（含行终止符），不是命中本身。
//!    精确区间必须再跑一次 `Matcher::find_iter` 在它上面求——ripgrep 自己做高亮
//!    也是这个路子，这份常数成本消不掉。
//! 2. `find_iter` 在**零宽命中**上按字节步进，会在多字节字符内部吐出区间。
//!    所有出口统一按字符边界过滤（见 `is_on_char_boundary`），否则这些区间流进
//!    替换路径就是 panic。
//! 3. 跨行匹配要 matcher 与 searcher **两处都**打开：`Searcher` 见到 matcher 仍然
//!    声明行终止符就静默退回逐行策略。

use std::ops::Range;
use std::sync::Arc;

use grep_matcher::Matcher;
use grep_regex::{RegexMatcher, RegexMatcherBuilder};
use grep_searcher::{Searcher, SearcherBuilder, Sink, SinkMatch};

use super::{SearchOptions, fuzzy_subsequence_ranges};

/// 一次命中：在整段文本里的绝对字节区间，加上它起始所在的那一行（1 基）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SearchHit {
    pub(crate) range: Range<usize>,
    pub(crate) line: Option<u64>,
}

/// 模式编译失败。`message` 是 ripgrep 的原始诊断，带 `^` 指出出错列，
/// 直接进搜索面板给用户看。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct QueryError {
    pub(crate) message: String,
}

/// 编译好的查询。`matcher` 为空只有两种情况：查询为空（或全空白），
/// 以及模糊模式——模糊不是 ripgrep 的能力，仍走 `fuzzy_subsequence_ranges`。
#[derive(Clone)]
pub(crate) struct CompiledQuery {
    query: String,
    matcher: Option<Arc<RegexMatcher>>,
    /// 模糊模式生效的标志：只有「开了模糊且没开正则」才走子序列匹配。
    /// 两个开关同时打开时正则优先——与被替换掉的手写层口径一致。
    fuzzy: bool,
}

impl CompiledQuery {
    /// 编译查询。非法正则**不再**静默退化成字面量搜索，而是返回诊断，
    /// 由调用方显示给用户并停止搜索。
    pub(crate) fn compile(query: &str, options: SearchOptions) -> Result<Self, QueryError> {
        let fuzzy = options.fuzzy && !options.use_regex;
        let matcher = if query.trim().is_empty() || fuzzy {
            None
        } else {
            Some(Arc::new(build_matcher(query, options)?))
        };
        Ok(Self {
            query: query.to_string(),
            matcher,
            fuzzy,
        })
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.query.trim().is_empty()
    }

    /// 整段文本里的全部命中，按出现顺序。`haystack` 必须是 UTF-8 字节
    /// （调用方交的都是 `str::as_bytes()`）。
    pub(crate) fn find_all(&self, haystack: &[u8]) -> Vec<SearchHit> {
        if self.is_empty() {
            return Vec::new();
        }
        let Some(matcher) = self.matcher.as_ref() else {
            return self
                .fuzzy
                .then(|| Self::fuzzy_hits(haystack, &self.query))
                .unwrap_or_default();
        };
        let mut hits = Vec::new();
        // 复用同一个 searcher：它内部持有行缓冲，反复搜索不再重新分配。
        let mut searcher = SearcherBuilder::new().line_number(true).build();
        {
            let mut sink = HitSink {
                matcher,
                haystack,
                hits: &mut hits,
            };
            // search_slice 只会返回 sink 里的 io::Error，而这里的 sink 不会失败
            // （RegexMatcher 的错误类型是 NoError）；出错就当没有命中。
            let _ = searcher.search_slice(matcher.as_ref(), haystack, &mut sink);
        }
        hits
    }

    /// 每根「含命中的行」只留第一个命中，最多取 `max_lines` 行，够数就让引擎
    /// 停下不再读后面的内容。对应 ripgrep CLI 的 `-m/--max-count`（按行计），
    /// 工作区结果列表的口径正是「一行一条」，所以它用这个而不是 `find_all`。
    pub(crate) fn find_first_per_line(&self, haystack: &[u8], max_lines: usize) -> Vec<SearchHit> {
        if self.is_empty() || max_lines == 0 {
            return Vec::new();
        }
        let Some(matcher) = self.matcher.as_ref() else {
            // 模糊模式没有行级早停可用（子序列匹配走的是整行文本），
            // 先算全表再按行取第一条。
            let all = match self.fuzzy {
                true => Self::fuzzy_hits(haystack, &self.query),
                false => return Vec::new(),
            };
            return Self::first_hit_per_line(all, max_lines);
        };
        let mut hits = Vec::new();
        let mut searcher = SearcherBuilder::new().line_number(true).build();
        {
            let mut sink = FirstPerLineSink {
                matcher,
                haystack,
                hits: &mut hits,
                last_line: None,
                max_lines,
            };
            let _ = searcher.search_slice(matcher.as_ref(), haystack, &mut sink);
        }
        hits
    }

    /// 把「每行可能有好几个命中」的表压成「每行只留第一个」，取满 `max_lines` 行为止。
    pub(crate) fn first_hit_per_line(hits: Vec<SearchHit>, max_lines: usize) -> Vec<SearchHit> {
        let mut out = Vec::with_capacity(max_lines.min(hits.len()));
        let mut last_line: Option<u64> = None;
        for hit in hits {
            if out.len() >= max_lines {
                break;
            }
            let Some(line) = hit.line else {
                out.push(hit);
                continue;
            };
            if Some(line) == last_line {
                continue;
            }
            last_line = Some(line);
            out.push(hit);
        }
        out
    }

    /// 单行里的命中区间。给「手上只有一行文本」的调用方用（逐行扫磁盘文件、
    /// 逐块扫缓冲区切片）。
    pub(crate) fn find_in_line(&self, line: &str) -> Vec<Range<usize>> {
        self.find_all(line.as_bytes())
            .into_iter()
            .map(|hit| hit.range)
            .collect()
    }

    /// 模糊模式：逐行找子序列，区间偏移换成整段文本里的绝对位置。
    fn fuzzy_hits(haystack: &[u8], query: &str) -> Vec<SearchHit> {
        let Ok(text) = std::str::from_utf8(haystack) else {
            return Vec::new();
        };
        let mut hits = Vec::new();
        let mut absolute = 0usize;
        let mut line_number = 0usize;
        for raw_line in text.split_inclusive('\n') {
            line_number += 1;
            let line = raw_line.strip_suffix('\n').unwrap_or(raw_line);
            for range in fuzzy_subsequence_ranges(line, query) {
                // 子序列匹配天生不切字符，但和引擎那条出口规矩保持一致，一律核对。
                let absolute_range = (absolute + range.start)..(absolute + range.end);
                if CompiledQuery::is_on_char_boundary(haystack, &absolute_range) {
                    hits.push(SearchHit {
                        range: absolute_range,
                        line: Some(line_number as u64),
                    });
                }
            }
            absolute += raw_line.len();
        }
        hits
    }

    /// 硬闸门：零宽命中可能落在多字节字符内部，出口一律滤掉。
    fn is_on_char_boundary(haystack: &[u8], range: &Range<usize>) -> bool {
        range.start <= haystack.len()
            && range.end <= haystack.len()
            && is_char_boundary(haystack, range.start)
            && is_char_boundary(haystack, range.end)
    }
}

/// 字节切片上的 `str::is_char_boundary`：越界处之后是 UTF-8 的**延续字节**
/// （`0b10xx_xxxx`）就不在字符边界上；文本末尾算边界。
fn is_char_boundary(haystack: &[u8], at: usize) -> bool {
    match haystack.get(at) {
        Some(byte) => byte & 0xC0 != 0x80,
        None => true,
    }
}

/// 把模式组合交给引擎：大小写、字面量、单词边界三档与 ripgrep CLI 的
/// `--ignore-case` / `--fixed-strings` / `--word-regexp` 一一对应。
fn build_matcher(query: &str, options: SearchOptions) -> Result<RegexMatcher, QueryError> {
    let mut builder = RegexMatcherBuilder::new();
    builder.case_insensitive(!options.match_case);
    if !options.use_regex {
        builder.fixed_strings(true);
    }
    if options.whole_word {
        builder.word(true);
    }
    builder
        .build(query)
        .map_err(|error| QueryError { message: error.to_string() })
}

/// `Sink`：把引擎报出的「命中所在行」还原成精确的命中区间。
struct HitSink<'a> {
    matcher: &'a Arc<RegexMatcher>,
    haystack: &'a [u8],
    hits: &'a mut Vec<SearchHit>,
}

impl Sink for HitSink<'_> {
    type Error = std::io::Error;

    fn matched(&mut self, _searcher: &Searcher, report: &SinkMatch) -> Result<bool, Self::Error> {
        let base = report.absolute_byte_offset() as usize;
        for range in local_matches(self.matcher, report, self.haystack, base)? {
            self.hits.push(SearchHit {
                range,
                line: report.line_number(),
            });
        }
        Ok(true)
    }
}

/// 每根含命中的行只留第一个命中，留够 `max_lines` 行就交回 `false` 让引擎停止读取。
struct FirstPerLineSink<'a> {
    matcher: &'a Arc<RegexMatcher>,
    haystack: &'a [u8],
    hits: &'a mut Vec<SearchHit>,
    /// 上一个已收录的命中起始行。行模式下每个 report 就是一行，这个字段是
    /// 为「同一个行号被报两次」兜底——跨行模式（阶段 3）一定会用到。
    last_line: Option<u64>,
    max_lines: usize,
}

impl Sink for FirstPerLineSink<'_> {
    type Error = std::io::Error;

    fn matched(&mut self, _searcher: &Searcher, report: &SinkMatch) -> Result<bool, Self::Error> {
        let base = report.absolute_byte_offset() as usize;
        let line = report.line_number();
        if self.last_line == line && line.is_some() {
            return Ok(self.hits.len() < self.max_lines);
        }
        let Some(range) = local_matches(self.matcher, report, self.haystack, base)?
            .into_iter()
            .next()
        else {
            return Ok(true);
        };
        self.last_line = line;
        self.hits.push(SearchHit { range, line });
        // 收满就停：`false` 让 grep-searcher 不再往下看剩余内容。
        Ok(self.hits.len() < self.max_lines)
    }
}

/// 一次 report 里的全部命中区间（绝对字节）。
///
/// `SinkMatch::bytes()` 给的是命中所在的**整行**，不是命中本身，所以要在这段
/// 字节上再跑一次 `Matcher::find_iter` 才能拿到精确区间——ripgrep 自己做高亮
/// 也是这个路子。
fn local_matches(
    matcher: &RegexMatcher,
    report: &SinkMatch<'_>,
    haystack: &[u8],
    base: usize,
) -> Result<Vec<Range<usize>>, std::io::Error> {
    // 逐行策略下 bytes() 是整行含终止符；把终止符摘掉，命中就不会跨行——
    // 这与被替换掉的手写层口径一致（`strip_suffix('\n')`）。
    let reported = report.bytes();
    let line = match reported.last() {
        Some(b'\n') => &reported[..reported.len() - 1],
        _ => reported,
    };
    let mut local: Vec<(usize, usize)> = Vec::new();
    // RegexMatcher 的错误类型是 NoError（不可能失败）；这里只是把结果消费掉，
    // 真出错也只会退回「无命中」，不会 panic（NoError 的 Display 自己是会 panic 的，
    // 所以不要把 error 拿出来格式化）。
    matcher
        .find_iter(line, |matched| {
            local.push((matched.start(), matched.end()));
            true
        })
        .map_err(|_| std::io::Error::other("matcher failed"))?;
    Ok(local
        .into_iter()
        .map(|(start, end)| (base + start)..(base + end))
        // 硬闸门：落在多字节字符内部的区间一律丢掉（见模块说明第 2 条）。
        .filter(|range| CompiledQuery::is_on_char_boundary(haystack, range))
        .collect())
}
