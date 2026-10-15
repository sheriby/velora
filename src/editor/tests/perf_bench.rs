//! 实测套件：把编辑器核心那几笔整篇成本量成数字，和本机基线比趋势。
//!
//! 这一族量的是**时间**：不进 CI（墙钟在并发抢 CPU 下会假红，AGENTS §3），也不负责
//! 判断对错——「每帧/每键做了多少遍整篇」那类确定性判断归计数器闸门，见
//! `docs/architecture/performance.md` §2、§3。这里回答另一个问题：一次整篇工作在这台
//! 机器上到底花多少毫秒，改完之后是变快还是变慢。
//!
//! 跑法（夹具不进版本库，不生成就直接失败）：
//!
//! ```bash
//! node scripts/generate-fixtures.mjs fixtures/perf
//! cargo test --bin velora bench_suite -- --ignored --nocapture --test-threads=1
//!
//! VELORA_BENCH_SAVE=1 cargo test --bin velora bench_suite -- --ignored --nocapture --test-threads=1   # 写/更新本机基线
//! VELORA_BENCH_SIZES=one-mib,ten-mib cargo test --bin velora bench_suite -- --ignored --nocapture --test-threads=1
//! VELORA_BENCH_FILTER=model cargo test --bin velora bench_suite -- --ignored --nocapture --test-threads=1
//! ```
//!
//! 基线一份机器一个文件（`benchmarks/baseline.<系统>.<架构>.dev.txt`）：同一份字节在
//! Intel、Windows、M 系上的绝对值差得看不出趋势，跨机器比只会逼人去放宽预算。
//! 换了机器就跑一次 `VELORA_BENCH_SAVE=1`，把自己那份机器上的数字提交进来。
//!
//! 怎么数：一个单位 = 一次被量的工作。每个样本里做多少个单位是自动放大的（跑到够
//! `TARGET_SAMPLE` 为止），否则几十纳秒级的工作会被计时粒度吞掉。报中位数与最小值，
//! 采样之间的散布进表格——散布大就说明这台机器此刻很吵，别拿这次的数字当结论。
//!
//! 用例类型和跑法都放在 `editor::tests` 子树里：`Editor` 的字段是私有的、几个方法是
//! `pub(super)`，只有这里能拿到，不用为了量性能去放宽生产代码的可见性，也不用新增
//! lib target 或第二个 crate 根。

use super::common::*;
use crate::editor::buffer::TextBuffer;
use crate::editor::persistence::file_content_version_normalized;
use crate::editor::status_bar::count_words;
use crate::components::{parse_image_reference_definitions, parse_link_reference_definitions};
use gpui::Entity;

/// 单个样本的最短时长。
const TARGET_SAMPLE: Duration = Duration::from_millis(20);
/// 每用例的采样次数（取中位数）。
const SAMPLES: usize = 7;
/// 一个样本里最多做多少个单位。
const MAX_UNITS_PER_SAMPLE: u32 = 1 << 13;
/// 比基线慢过这个比例就算回归。墙钟的抖动量级在这里，再紧就会假红。
const DEFAULT_TOLERANCE_PCT: f64 = 30.0;

/// 可用档位，与 `scripts/generate-fixtures.mjs` 生成的文件名一一对应。
const SIZES: [&str; 2] = ["one-mib", "ten-mib"];

/// 交给用例的一次输入。
struct Input {
    /// 档位名，进基线文件的键的一部分。
    size: &'static str,
    markdown: String,
}

/// 干 `count` 个单位。上下文在调用时给：每份现场都挂在测试自己的 `TestAppContext` 上。
type Body = dyn FnMut(&mut TestAppContext, u32);

/// 一个基准用例。
struct Case {
    /// `组.名字`：进基线文件的键，改名等于换指标。
    name: &'static str,
    /// 这活儿跑在哪一层：`text` 只要字符串，`model` 要 gpui 的 App 上下文。
    group: &'static str,
    /// 单位名，只影响表格可读性。
    unit: &'static str,
    /// 造活儿，返回 `(一个单位覆盖的字节数, 干 count 个单位)`；字节数给 0 表示这条
    /// 不按字节归一。返回 `None` 表示这份输入上做不了，跳过并写明原因。
    prepare: fn(&Input, &mut TestAppContext) -> Option<(u64, Box<Body>)>,
    /// `prepare` 返回 `None` 时该说什么。
    skip_hint: &'static str,
}

/// 把结果交出去，别让编译器把它当没用的值。
///
/// `std::hint::blackbox` 是 nightly 才有的，本仓工具链是 stable（AGENTS §4）。
/// 测试档与产品 dev 同档（本地代码 opt-level=0），本来就不做删除死计算的优化，
/// 这一层是保险。
#[inline(never)]
fn sink<T>(value: T) {
    std::mem::drop(value);
}

fn cases() -> Vec<Case> {
    vec![
        Case {
            name: "text.buffer_serialize",
            group: "text",
            unit: "次整篇序列化",
            skip_hint: "纯字符串工作，不该跳",
            prepare: |input, _cx| {
                let buffer = TextBuffer::from_text(&input.markdown);
                let bytes = input.markdown.len() as u64;
                let body: Box<Body> = Box::new(move |_cx, count| {
                    // `buffer.text()` 就是保存取文本、算哈希、导出取文本共用的那一位。
                    for _ in 0..count {
                        sink(buffer.text());
                    }
                });
                Some((bytes, body))
            },
        },
        Case {
            name: "text.content_version",
            group: "text",
            unit: "次整篇哈希",
            skip_hint: "纯字符串工作，不该跳",
            prepare: |input, _cx| {
                let markdown = input.markdown.clone();
                let bytes = input.markdown.len() as u64;
                let body: Box<Body> = Box::new(move |_cx, count| {
                    // 外部改动检测、自动保存比对、切标签现场采集都付这一位。
                    for _ in 0..count {
                        sink(file_content_version_normalized(&markdown));
                    }
                });
                Some((bytes, body))
            },
        },
        Case {
            name: "text.word_count",
            group: "text",
            unit: "次整篇分词",
            skip_hint: "纯字符串工作，不该跳",
            prepare: |input, _cx| {
                let markdown = input.markdown.clone();
                let bytes = input.markdown.len() as u64;
                let body: Box<Body> = Box::new(move |_cx, count| {
                    // 状态栏字数：整篇扫一遍，所以只在文档安静下来之后补算。
                    for _ in 0..count {
                        sink(count_words(&markdown));
                    }
                });
                Some((bytes, body))
            },
        },
        Case {
            name: "text.reference_definitions",
            group: "text",
            unit: "次整篇引用定义解析",
            skip_hint: "纯字符串工作，不该跳",
            prepare: |input, _cx| {
                let markdown = input.markdown.clone();
                let bytes = input.markdown.len() as u64;
                let body: Box<Body> = Box::new(move |_cx, count| {
                    // `rebuild_image_runtimes` 每次付的就是这两位加起来。
                    for _ in 0..count {
                        sink(parse_image_reference_definitions(&markdown));
                        sink(parse_link_reference_definitions(&markdown));
                    }
                });
                Some((bytes, body))
            },
        },
        Case {
            name: "model.document_rebuild",
            group: "model",
            unit: "次整篇重解析建树",
            skip_hint: "建编辑器失败",
            prepare: |input, cx| {
                let Some(editor) = editor_from_fixture(input, cx) else {
                    return None;
                };
                let bytes = input.markdown.len() as u64;
                let body: Box<Body> = Box::new(move |cx, count| {
                    // 整篇推倒重建：打开文件、兜底重投影走的就是这一位。
                    for _ in 0..count {
                        editor.update(cx, |editor, cx| {
                            editor.rebuild_document_from_buffer(cx);
                        });
                    }
                });
                Some((bytes, body))
            },
        },
        Case {
            name: "model.image_runtime_rebuild",
            group: "model",
            unit: "次整篇引用注册表重建",
            skip_hint: "建编辑器失败",
            prepare: |input, cx| {
                let Some(editor) = editor_from_fixture(input, cx) else {
                    return None;
                };
                let bytes = input.markdown.len() as u64;
                let body: Box<Body> = Box::new(move |cx, count| {
                    // 编辑含 `[`/`<`/链接的块、表格/HTML/frontmatter 里的每次改动都会
                    // 付这一位；它此前不在任何计数器里，只有这里能看见它的单价。
                    for _ in 0..count {
                        editor.update(cx, |editor, cx| {
                            editor.rebuild_image_runtimes(cx);
                        });
                    }
                });
                Some((bytes, body))
            },
        },
        Case {
            name: "model.capture_document_view",
            group: "model",
            unit: "次整篇现场采集",
            skip_hint: "建编辑器失败",
            prepare: |input, cx| {
                let Some(editor) = editor_from_fixture(input, cx) else {
                    return None;
                };
                // 与产品里的调用同形：整篇文本由调用方先序列化好递进来（只序列化这一份）。
                // 序列化自己的单价在 `text.buffer_serialize`，这里不重复计。
                let markdown = input.markdown.clone();
                let bytes = markdown.len() as u64;
                let body: Box<Body> = Box::new(move |cx, count| {
                    for _ in 0..count {
                        let view = editor.update(cx, |editor, cx| {
                            editor.capture_document_view(&markdown, cx)
                        });
                        sink(view.content_len);
                    }
                });
                Some((bytes, body))
            },
        },
        Case {
            name: "model.buffer_edit",
            group: "model",
            unit: "次缓冲区写入",
            skip_hint: "建编辑器失败",
            prepare: |input, cx| {
                let Some(editor) = editor_from_fixture(input, cx) else {
                    return None;
                };
                let body: Box<Body> = Box::new(move |cx, count| {
                    // 只动缓冲区本身：区间平移、脏区间标记、换行索引增量。
                    for _ in 0..count {
                        editor.update(cx, |editor, _cx| {
                            sink(editor.buffer.edit(0..0, "x"));
                        });
                    }
                });
                // 一个单位与文档字节不成比例，不按字节归一。
                Some((0, body))
            },
        },
        Case {
            name: "model.edit_and_undo_record",
            group: "model",
            unit: "次写入加撤销记账",
            skip_hint: "建编辑器失败",
            prepare: |input, cx| {
                let Some(editor) = editor_from_fixture(input, cx) else {
                    return None;
                };
                let body: Box<Body> = Box::new(move |cx, count| {
                    // 加上撤销快照那一笔（`record_buffer_edit`）。
                    for _ in 0..count {
                        editor.update(cx, |editor, _cx| {
                            let applied = editor.buffer.edit(0..0, "x");
                            editor.record_buffer_edit(applied);
                        });
                    }
                });
                Some((0, body))
            },
        },
        Case {
            name: "model.keystroke_through_block",
            group: "model",
            unit: "次按键（走块事件）",
            skip_hint: "拿不到首块",
            prepare: |input, cx| {
                let Some(editor) = editor_from_fixture(input, cx) else {
                    return None;
                };
                let Some(block) = first_block(&editor, cx) else {
                    return None;
                };
                let body: Box<Body> = Box::new(move |cx, count| {
                    // 用户真打字的那条路：块自己改文本并按区间写回缓冲区，编辑器收到
                    // 块事件后走 `mark_dirty_written_back`。含自动保存排程，不含渲染与
                    // 字体度量。
                    for _ in 0..count {
                        let before = editor.read_with(cx, |editor, _cx| editor.document_revision);
                        block.update(cx, |block, cx| {
                            block.replace_text_in_visible_range(0..0, "x", None, false, cx);
                        });
                        cx.run_until_parked();
                        let after = editor.read_with(cx, |editor, _cx| editor.document_revision);
                        // 修订号没动 = 块事件根本没送到编辑器，这条量的就不是按键。
                        assert_eq!(
                            after,
                            before.wrapping_add(1),
                            "块改动没让编辑器进脏路径，基准用例测错了东西"
                        );
                    }
                });
                Some((0, body))
            },
        },
        Case {
            name: "model.document_resync_fallback",
            group: "model",
            unit: "次整篇兜底重投影",
            skip_hint: "建编辑器失败",
            prepare: |input, cx| {
                let Some(editor) = editor_from_fixture(input, cx) else {
                    return None;
                };
                let body: Box<Body> = Box::new(move |cx, count| {
                    // 改了缓冲区却不说明是哪一根块动的，标脏时只能整篇重新投影。按键
                    // 路径上不允许出现这一档；量出单价，才知道这条禁令省下多少。
                    for _ in 0..count {
                        editor.update(cx, |editor, cx| {
                            let applied = editor.buffer.edit(0..0, "x");
                            editor.record_buffer_edit(applied);
                            editor.mark_dirty(cx);
                        });
                    }
                });
                Some((0, body))
            },
        },
    ]
}

/// 从夹具建一个编辑器：先让后台续建落地，别把建树成本混进后面每一个单位里。
fn editor_from_fixture(input: &Input, cx: &mut TestAppContext) -> Option<Entity<Editor>> {
    init_editor_test_app(cx);
    let markdown = input.markdown.clone();
    let editor = cx.update(|cx| cx.new(|cx| Editor::from_markdown(cx, markdown, None)));
    cx.run_until_parked();
    Some(editor)
}

fn first_block(editor: &Entity<Editor>, cx: &mut TestAppContext) -> Option<Entity<Block>> {
    editor.read_with(cx, |editor, _cx| editor.document.first_root().cloned())
}

/// 一个用例跑完的结果。
struct Measurement {
    ns_per_unit: f64,
    min_ns_per_unit: f64,
    /// (最大-最小)/中位数，用来提醒这台机器此刻有多吵。
    spread_pct: f64,
    units_per_sample: u32,
    bytes_per_unit: u64,
}

impl Measurement {
    /// 吞吐，MiB/s；没声明字节的工作量给 `None`。
    fn mib_per_sec(&self) -> Option<f64> {
        if self.bytes_per_unit == 0 {
            return None;
        }
        // 字节/纳秒就是 GB/s，再折成 MiB/s（1 GB/s = 1e9/1048576 MiB/s）。
        Some(self.bytes_per_unit as f64 * 1000.0 / (self.ns_per_unit * 1048.576))
    }
}

fn measure(case: &Case, input: &Input, cx: &mut TestAppContext) -> Option<Measurement> {
    let (bytes_per_unit, mut body) = (case.prepare)(input, cx)?;
    // 放大单位数直到一个样本跑得够久；这一段同时兼作预热。
    let mut units = 1u32;
    loop {
        let start = Instant::now();
        body(cx, units);
        if start.elapsed() >= TARGET_SAMPLE || units >= MAX_UNITS_PER_SAMPLE {
            break;
        }
        units *= 2;
    }
    let mut samples = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        let start = Instant::now();
        body(cx, units);
        samples.push(start.elapsed().as_nanos() as f64 / units as f64);
    }
    samples.sort_by(f64::total_cmp);
    let median = samples[SAMPLES / 2];
    let spread_pct = if median > 0.0 {
        (samples[SAMPLES - 1] - samples[0]) / median * 100.0
    } else {
        0.0
    };
    Some(Measurement {
        ns_per_unit: median,
        min_ns_per_unit: samples[0],
        spread_pct,
        units_per_sample: units,
        bytes_per_unit,
    })
}

/// 表格里的一行，也是基线文件里的一行。
struct Row {
    size: &'static str,
    name: &'static str,
    unit: &'static str,
    ns_per_unit: f64,
    min_ns_per_unit: f64,
    spread_pct: f64,
    units_per_sample: u32,
    bytes_per_unit: u64,
    mib_per_sec: Option<f64>,
}

impl Row {
    /// 基线文件里的键：`档位 用例`。
    fn key(&self) -> String {
        format!("{} {}", self.size, self.name)
    }

    /// 基线文件里的一行：各列空格分隔。`unit` 不带空格由
    /// `bench_case_names_are_unique_and_grouped` 守住（基线按空白分列）。
    fn to_line(&self) -> String {
        let throughput = match self.mib_per_sec {
            Some(value) => format!("{value:.3}"),
            None => "-".to_string(),
        };
        format!(
            "{} {} {} {:.3} {:.3} {:.2} {} {} {}",
            self.size,
            self.name,
            self.unit,
            self.ns_per_unit,
            self.min_ns_per_unit,
            self.spread_pct,
            self.units_per_sample,
            self.bytes_per_unit,
            throughput,
        )
    }
}

fn machine_tag() -> String {
    format!("{}.{}.{}", std::env::consts::OS, std::env::consts::ARCH, "dev")
}

fn baseline_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("benchmarks")
        .join(format!("baseline.{}.txt", machine_tag()))
}

/// 基线文件的数据行：`档位 用例 单位 ns/单位 最小ns/单位 散布% 单位/样本 字节/单位 MiB/s`。
/// 注释行以 `#` 开头，空行忽略。
fn baseline_lines() -> Vec<String> {
    let Ok(text) = fs::read_to_string(baseline_path()) else {
        return Vec::new();
    };
    text.lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
        .map(str::to_string)
        .collect()
}

/// 一行的键：`档位 用例`。
fn line_key(line: &str) -> String {
    let mut columns = line.split_whitespace();
    let (Some(size), Some(name)) = (columns.next(), columns.next()) else {
        return String::new();
    };
    format!("{size} {name}")
}

/// 基线里每条用例的上一次中位纳秒。键不在文件里（新用例、换机器）就是空表。
fn baseline_ns() -> Vec<(String, f64)> {
    baseline_lines()
        .iter()
        .filter_map(|line| {
            let ns: f64 = line.split_whitespace().nth(3)?.parse().ok()?;
            Some((line_key(line), ns))
        })
        .collect()
}

/// 写基线：这一轮量到的用例覆盖旧值，其余行原样留着。
///
/// 不做合并的话，`VELORA_BENCH_SAVE=1` 配一个 `VELORA_BENCH_FILTER` 就会把没跑到的
/// 那些行从文件里删掉，下一次比较只剩个位数条能判。
fn write_baseline(rows: &[Row]) {
    let path = baseline_path();
    fs::create_dir_all(path.parent().expect("基线文件在 benchmarks/ 下"))
        .unwrap_or_else(|error| panic!("建 {} 失败：{error}", path.display()));
    let fresh: std::collections::HashSet<String> = rows.iter().map(Row::key).collect();
    let mut lines: Vec<String> = baseline_lines()
        .into_iter()
        .filter(|line| !fresh.contains(&line_key(line)))
        .collect();
    lines.extend(rows.iter().map(Row::to_line));
    lines.sort();
    let mut text = format!(
        "# Velora 性能基线｜机器 {}｜跑 `VELORA_BENCH_SAVE=1 cargo test --bin velora bench_suite \
         -- --ignored --nocapture --test-threads=1` 重写；只比本机那份，跨机器比不出趋势\n\
         # 档位 用例 单位 ns/单位 最小ns/单位 散布% 单位/样本 字节/单位 MiB/s\n",
        machine_tag()
    );
    for line in &lines {
        text.push_str(line);
        text.push('\n');
    }
    fs::write(&path, &text).unwrap_or_else(|error| panic!("写 {} 失败：{error}", path.display()));
    println!("\n已写基线 {}（{} 条）", path.display(), lines.len());
}

/// 读一个开关型的环境变量：设了且不是空串才算数。空串按没设处理，否则
/// `VELORA_BENCH_SAVE= cargo test …` 会让人以为自己在存基线。
fn env_value(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|value| !value.trim().is_empty())
}

/// 表格里按时长量级换成 ns/µs/ms：纳秒原值有九位数，并排放两列会挤在一起。
/// 基线文件里仍然写原始纳秒，比较按纳秒算。
fn human_ns(ns: f64) -> String {
    if ns >= 1_000_000.0 {
        format!("{:.2} ms", ns / 1_000_000.0)
    } else if ns >= 1_000.0 {
        format!("{:.1} us", ns / 1_000.0)
    } else {
        format!("{:.0} ns", ns)
    }
}

/// 跑一遍套件，返回每一行的量。缺夹具直接失败：静默跳过会得到一份「全绿但
/// 什么都没量」的基线。
fn run_suite(cx: &mut TestAppContext) -> Vec<Row> {
    let filter = env_value("VELORA_BENCH_FILTER");
    let requested_sizes = env_value("VELORA_BENCH_SIZES");
    let sizes: Vec<&'static str> = match requested_sizes.as_deref() {
        Some(raw) => {
            let mut picked = Vec::new();
            for requested in raw.split(',').map(str::trim).filter(|name| !name.is_empty()) {
                let Some(index) = SIZES.iter().position(|candidate| *candidate == requested)
                else {
                    panic!("未知档位 `{requested}`，可选：{}", SIZES.join(", "));
                };
                picked.push(SIZES[index]);
            }
            if picked.is_empty() {
                panic!("VELORA_BENCH_SIZES 至少要给一个档位");
            }
            picked
        }
        None => SIZES[..1].to_vec(),
    };
    let all = cases();
    let selected: Vec<&Case> = all
        .iter()
        .filter(|case| {
            filter
                .as_ref()
                .is_none_or(|needle| case.name.contains(needle.as_str()))
        })
        .collect();

    println!(
        "{} 条用例｜档位 {}｜{} {}｜本地代码 opt-level=0、重依赖 opt-level=3\
         （与日常 dev 构建同档）",
        selected.len(),
        sizes.join(" + "),
        std::env::consts::OS,
        std::env::consts::ARCH,
    );

    let mut rows: Vec<Row> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    for size in sizes {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/perf")
            .join(format!("{size}.md"));
        let markdown = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!(
                "缺夹具 {}：先跑 `node scripts/generate-fixtures.mjs fixtures/perf`（{error}）",
                path.display()
            ));
        let input = Input { size, markdown };
        println!("\n== {size}（{} 字节）", input.markdown.len());
        println!(
            "{:<34}{:>15}{:>13}{:>10}{:>7}{:>10}",
            "用例", "中位/单位", "最小", "MiB/s", "散布", "单位/样本"
        );
        for case in &selected {
            let Some(result) = measure(case, &input, cx) else {
                println!("{:<34}{:>15}", case.name, "跳过");
                skipped.push(format!("{}@{}：{}", case.name, input.size, case.skip_hint));
                continue;
            };
            let throughput = result
                .mib_per_sec()
                .map(|value| format!("{value:.1}"))
                .unwrap_or_else(|| "-".to_string());
            println!(
                "{:<34}{:>15}{:>13}{:>10}{:>6.0}%{:>10}",
                case.name,
                human_ns(result.ns_per_unit),
                human_ns(result.min_ns_per_unit),
                throughput,
                result.spread_pct,
                result.units_per_sample,
            );
            rows.push(Row {
                size: input.size,
                name: case.name,
                unit: case.unit,
                ns_per_unit: result.ns_per_unit,
                min_ns_per_unit: result.min_ns_per_unit,
                spread_pct: result.spread_pct,
                units_per_sample: result.units_per_sample,
                bytes_per_unit: result.bytes_per_unit,
                mib_per_sec: result.mib_per_sec(),
            });
        }
    }
    if !skipped.is_empty() {
        println!("\n跳过的用例：");
        for line in &skipped {
            println!("  - {line}");
        }
    }
    rows
}

/// 和基线比：慢过容差记一条回归。容差用 `VELORA_BENCH_TOLERANCE`（百分数）覆盖，
/// 只用来在改完之后收紧一次、验证这道门真的会红。
fn compare_with_baseline(rows: &[Row], tolerance_pct: f64) -> Vec<String> {
    let baseline = baseline_ns();
    if baseline.is_empty() {
        println!(
            "\n没有本机基线 {}：跑一次 `VELORA_BENCH_SAVE=1 cargo test --bin velora bench_suite \
             -- --ignored --nocapture --test-threads=1` 生成并提交。这一轮只出表，不判回归。",
            baseline_path().display()
        );
        return Vec::new();
    }
    let mut regressions = Vec::new();
    let mut faster = 0usize;
    let mut compared = 0usize;
    for row in rows {
        let key = row.key();
        let Some((_, previous)) = baseline.iter().find(|(baseline_key, _)| *baseline_key == key)
        else {
            println!("  基线里没有 {key}（新增用例？跑一次 VELORA_BENCH_SAVE=1 补上）");
            continue;
        };
        compared += 1;
        if *previous <= 0.0 {
            continue;
        }
        let change_pct = (row.ns_per_unit - previous) / previous * 100.0;
        if change_pct > tolerance_pct {
            regressions.push(format!(
                "{key}：{previous:.0} ns → {:.0} ns（{change_pct:+.0}%，容差 {tolerance_pct:.0}%）",
                row.ns_per_unit
            ));
        } else if change_pct < -tolerance_pct {
            faster += 1;
        }
    }
    println!("\n与 {} 比了 {compared} 条（容差 {tolerance_pct:.0}%）", baseline_path().display());
    if faster > 0 {
        println!("  其中 {faster} 条比基线快过容差：这一轮不判，只有 VELORA_BENCH_SAVE=1 才把数字写回文件");
    }
    regressions
}

#[gpui::test]
#[ignore = "墙钟预算闸门，依赖机器速度；单独跑：cargo test --bin velora bench_suite -- \
--ignored --nocapture --test-threads=1"]
fn bench_suite_measures_the_core_costs(cx: &mut TestAppContext) {
    let rows = run_suite(cx);
    assert!(!rows.is_empty(), "一条用例都没跑起来：检查档位名与用例过滤");

    let tolerance_pct = env_value("VELORA_BENCH_TOLERANCE")
        .map(|raw| {
            raw.trim()
                .parse::<f64>()
                .unwrap_or_else(|error| panic!("VELORA_BENCH_TOLERANCE 要的是百分数：{error}"))
        })
        .unwrap_or(DEFAULT_TOLERANCE_PCT);

    if env_value("VELORA_BENCH_SAVE").is_some() {
        write_baseline(&rows);
        return;
    }

    let regressions = compare_with_baseline(&rows, tolerance_pct);
    assert!(
        regressions.is_empty(),
        "比本机基线慢过容差（{tolerance_pct:.0}%）：\n{}",
        regressions.join("\n")
    );
}

/// 用例名是基线文件里的键：改名等于换指标，重名会让基线读起来对不上号。
#[test]
fn bench_case_names_are_unique_and_grouped() {
    let all = cases();
    let mut seen = std::collections::HashSet::new();
    for case in &all {
        assert!(
            case.name.starts_with(&format!("{}.", case.group)),
            "{} 的名字没带组名前缀",
            case.name
        );
        assert!(
            seen.insert(case.name),
            "用例名重复：{}（基线文件里会互相覆盖）",
            case.name
        );
        assert!(
            !case.unit.chars().any(char::is_whitespace),
            "{} 的单位名不能带空白：基线文件按空白分列",
            case.name
        );
    }
}

/// 基线靠「按空白分列、第 4 列是中位纳秒」这条约定读。加一列或换顺序而不改读取，
/// 比较就会拿错数字去判回归。
#[test]
fn baseline_row_round_trips_through_a_line() {
    let row = Row {
        size: "one-mib",
        name: "text.probe",
        unit: "次探测",
        ns_per_unit: 1234.5,
        min_ns_per_unit: 1200.0,
        spread_pct: 2.5,
        units_per_sample: 8,
        bytes_per_unit: 0,
        mib_per_sec: None,
    };
    let line = row.to_line();
    assert_eq!(line_key(&line), row.key(), "写出来的行读不回同一个键");
    let columns: Vec<&str> = line.split_whitespace().collect();
    assert_eq!(columns.len(), 9, "基线一行的列数变了：{line}");
    let parsed: f64 = columns[3].parse().expect("第 4 列必须是纳秒数");
    assert!((parsed - 1234.5).abs() < 0.01, "第 4 列不是中位纳秒：{line}");
}
