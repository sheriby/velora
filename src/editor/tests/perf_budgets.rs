//! 墙钟预算类性能闸门集中在这里：断言里量真实耗时（`typed < 1200ms` 这类），
//! 慢机器或并发抢 CPU 下会假红，因此整族 `#[ignore]`，默认 `cargo test` 与 CI
//! 都不跑。单独跑：`cargo test --bin velora -- --ignored --nocapture`；夹具由
//! `node scripts/generate-fixtures.mjs fixtures/perf` 生成，缺失即自跳。
//! 确定性计数闸门（数操作遍数、不量时间）不在此列，照常随默认测试跑。
//! 两条跨子系统的墙钟闸门留在各自测试模块里原地 `#[ignore]`（字段私有，
//! 搬过来要放宽生产可见性）：`workspace/tests/search_perf.rs` 的
//! `searching_a_ten_mib_document_stays_within_budget`、`buffer/tests.rs` 的
//! `an_edit_on_an_eight_mib_buffer_costs_nothing_proportional_to_the_text`。
use super::common::*;

#[gpui::test]
async fn typing_does_not_rescan_status_bar_statistics_every_key(cx: &mut TestAppContext) {
    // P2：状态栏整篇字数与「超长块」提示都是整篇扫描，旧实现按 document_revision
    // 缓存 → 每个按键扫一遍（1 MiB 整篇分词 29ms，10 MiB 约 300ms）。改成静默
    // 窗口：打字期间沿用旧值，停手后补算一次。
    init_editor_test_app(cx);
    let markdown = (0..200)
        .map(|index| format!("## 第 {index} 节\n\n第 {index} 段正文，用于字数统计。\n"))
        .collect::<Vec<_>>()
        .join("\n");
    let (editor, cx) = cx.add_window_view(|_window, cx| Editor::from_markdown(cx, markdown, None));
    redraw(cx);
    // 首帧照常算一次。
    editor.update(cx, |editor, cx| {
        editor.cached_total_word_count(cx);
    });
    let before = editor.read_with(cx, |editor, _| editor.word_count_scans.get());

    for _ in 0..5 {
        cx.simulate_input("x");
        redraw(cx);
    }
    let during = editor.read_with(cx, |editor, _| editor.word_count_scans.get()) - before;
    assert!(
        during == 0,
        "连打 5 个字期间重扫了 {during} 次整篇字数，静默窗口没生效"
    );

    // 停手后必须补算，且数字与当前文本一致。静默计时器每轮确认「这一轮
    // 250ms 内文档没再变」，所以打字节拍会让它多等一轮，推进两轮即可。
    for _ in 0..3 {
        cx.executor().advance_clock(Duration::from_millis(400));
        cx.run_until_parked();
    }
    redraw(cx);
    let total = editor.update(cx, |editor, cx| editor.cached_total_word_count(cx));
    editor.read_with(cx, |editor, cx| {
        let expected = editor
            .document
            .visible_blocks()
            .iter()
            .map(|visible| {
                crate::editor::status_bar::count_words(
                    &visible.entity.read(cx).display_text(),
                )
            })
            .sum::<usize>();
        assert_eq!(
            total, expected,
            "静默窗口后补算的字数必须与当前可见文本一致"
        );
    });
    let after = editor.read_with(cx, |editor, _| editor.word_count_scans.get());
    assert!(
        after - before <= 2,
        "补算次数过多：{} 次（预期 ≤ 2）",
        after - before
    );
}

#[gpui::test]
async fn autosave_does_not_capture_the_reading_position(cx: &mut TestAppContext) {
    // 不变量 8：自动保存的防抖回调跑在前景线程，一次 burst 只需要把内容与脏位同步
    // 到标签上。整篇阅读现场（全文哈希 + O(rows) 行测量 + O(blocks) 读实体 + 撤销栈
    // 深拷贝）只有「马上换掉整棵块树、之后要交还」的入口才用得上，自动保存采了没人用。
    init_editor_test_app(cx);
    let path = temp_markdown_path("autosave-view-capture");
    fs::write(&path, "alpha").expect("write document");
    let cleanup = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(cleanup);
    });
    let (editor, cx) = cx.add_window_view({
        let path = path.clone();
        move |_window, cx| Editor::from_markdown(cx, "alpha".to_string(), Some(path))
    });
    redraw(cx);
    cx.simulate_input("!");
    redraw(cx);

    let before = editor.read_with(cx, |editor, _cx| {
        assert!(editor.document_dirty, "前提：打完字这一篇必须是脏的");
        editor.document_view_captures.get()
    });
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    assert_ne!(
        fs::read_to_string(&path).expect("read autosaved document"),
        "alpha",
        "前提：这一秒里自动保存必须真的落盘，否则这条只是在量一个没跑的分支"
    );
    let during = editor.read_with(cx, |editor, _cx| editor.document_view_captures.get()) - before;
    assert_eq!(
        during, 0,
        "自动保存 burst 采了 {during} 次整篇阅读现场：前景做了没人要用的整篇工作"
    );

    // 对称的一半：真正要交还现场的入口照旧必须采。
    let second_path = temp_markdown_path("autosave-view-capture-second");
    fs::write(&second_path, "beta").expect("write second document");
    let cleanup_second = second_path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(cleanup_second);
    });
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(second_path, window, cx);
        });
    });
    let after_switch = editor.read_with(cx, |editor, _cx| editor.document_view_captures.get());
    assert!(
        after_switch > before,
        "换标签必须采一次现场，实测这一趟采了 {} 次",
        after_switch - before
    );
}

#[gpui::test]
#[ignore = "墙钟预算闸门，依赖机器速度；单独跑：cargo test --bin velora -- --ignored"]
async fn one_mib_typing_stays_within_budget(cx: &mut TestAppContext) {
    // P2 大文档输入预算：1 MiB 文档里一次按键的成本必须是「常数次全文遍数 +
    // 有界时间」，而不是随文档线性增长的多遍扫描。夹具由
    // scripts/generate-fixtures.mjs 生成且被 gitignore，缺失就跳过。
    // （10 MiB 夹具单键实测 13s，迭代太慢，先用 1 MiB 收敛行为。）
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/perf/one-mib.md");
    if !fixture.is_file() {
        eprintln!("skipping: generate fixtures with `node scripts/generate-fixtures.mjs fixtures/perf`");
        return;
    }
    init_editor_test_app(cx);
    let markdown = std::fs::read_to_string(&fixture).expect("read fixture");
    assert!(markdown.len() >= 1024 * 1024, "夹具应约 1 MiB");
    let (editor, cx) = cx.add_window_view(|_window, cx| Editor::from_markdown(cx, markdown, None));
    let deadline = Instant::now() + Duration::from_secs(120);
    while editor.read_with(cx, |editor, _| editor.document.pending_tail().is_some()) {
        assert!(Instant::now() < deadline, "续建未完成");
        cx.run_until_parked();
    }
    redraw(cx);

    // 单次全文操作的分解成本（人类可读的诊断输出，断言看计数器）。
    let t = Instant::now();
    let raw_len = editor.read_with(cx, |editor, cx| editor.document.raw_source_text(cx).len());
    let raw_source = t.elapsed();
    let t = Instant::now();
    let title_len = editor.read_with(cx, |editor, cx| {
        editor
            .document
            .visible_blocks()
            .iter()
            .map(|visible| visible.entity.read(cx).record.title_markdown().len())
            .sum::<usize>()
    });
    let title_markdown_first = t.elapsed();
    // 并发跑测时单次测量会被其他用例的负载尖峰污染：共测三次取最小值。
    // 备忘失效（修复前 779ms）是持续性慢，取 min 不影响捕获。
    let title_markdown = (0..2)
        .map(|_| {
            let t = Instant::now();
            editor.read_with(cx, |editor, cx| {
                editor
                    .document
                    .visible_blocks()
                    .iter()
                    .map(|visible| visible.entity.read(cx).record.title_markdown().len())
                    .sum::<usize>()
            });
            t.elapsed()
        })
        .chain([title_markdown_first])
        .min()
        .expect("至少测量一次");
    let t = Instant::now();
    let visible_len = editor.read_with(cx, |editor, cx| {
        editor
            .document
            .visible_blocks()
            .iter()
            .map(|visible| visible.entity.read(cx).display_text().len())
            .sum::<usize>()
    });
    let visible_text = t.elapsed();
    eprintln!(
        "[probe] raw_source {raw_len}B {raw_source:?}；title_markdown {title_len}B {title_markdown:?}；display_text {visible_len}B {visible_text:?}"
    );
    // 真实 Markdown 语料上的等价校验：每 25 块抽一块，比对无映射快路径与
    // 映射版本（夹具含表格、围栏代码、脚注、HTML、公式）。
    let mismatched = editor.read_with(cx, |editor, cx| {
        editor
            .document
            .visible_blocks()
            .iter()
            .step_by(25)
            .find(|visible| {
                let block = visible.entity.read(cx);
                block.record.title.serialize_markdown()
                    != block.record.title.markdown_offset_map().markdown()
            })
            .map(|visible| visible.entity.read(cx).display_text().to_string())
    });
    assert!(
        mismatched.is_none(),
        "真实语料里无映射序列化与映射版本不一致：{mismatched:?}"
    );

    let t = Instant::now();
    let src_len = editor.read_with(cx, |editor, cx| editor.current_document_source(cx).len());
    let serialize = t.elapsed();
    let t = Instant::now();
    editor.update(cx, |editor, _| editor.word_count_cache.set(None));
    let words = editor.update(cx, |editor, cx| editor.cached_total_word_count(cx));
    let word_count = t.elapsed();
    let t = Instant::now();
    let mappings = editor.read_with(cx, |editor, cx| editor.build_source_target_mappings(cx).len());
    let mapping = t.elapsed();
    eprintln!(
        "[measure] 1 MiB 单次全文：序列化 {src_len}B {serialize:?}；字数 {words} 词 {word_count:?}；mapping {mappings} 条 {mapping:?}"
    );
    // 预算（并发跑测下取宽裕上限）：块 markdown 有备忘，整篇序列化不该再付
    // 每块重算的钱（修复前 title_markdown 全量 779ms）。阈值必须远高于机器
    // 的日间波动（2026-09-30 观测 100–115ms），同时仍以 3 倍余量覆盖失效模式。
    assert!(
        title_markdown < Duration::from_millis(250),
        "块 markdown 备忘失效了吗：全量 title_markdown {title_markdown:?}"
    );
    assert!(
        serialize < Duration::from_millis(400),
        "整篇序列化 {serialize:?}，偏出预算（修复前 840ms）"
    );

    let before = perf_passes(&editor, cx);
    let full_mappings_before = editor.read_with(cx, |editor, _| {
        editor.source_mapping_full_builds.get()
    });
    let revision_before = editor.read_with(cx, |editor, _| editor.document_revision);
    let start = Instant::now();
    cx.simulate_input("x");
    redraw(cx);
    let typed = start.elapsed();
    let revision_after = editor.read_with(cx, |editor, _| editor.document_revision);
    eprintln!(
        "[measure] 一次按键：修订 +{}（行计划重建的键含修订/折叠/TOC 版本）",
        revision_after - revision_before,
    );
    let delta = perf_delta(before, perf_passes(&editor, cx));
    let totals = editor.read_with(cx, |editor, _| {
        (
            editor.source_serializations.get(),
            editor.source_serialization_nanos.get(),
            editor.source_mapping_builds.get(),
            editor.source_mapping_nanos.get(),
            editor.row_plan_rebuilds.get(),
            editor.row_plan_nanos.get(),
        )
    });
    eprintln!("[measure] 1 MiB 一次按键 {typed:?}，遍数 (序列化, mapping, 字数, 行计划) = {delta:?}");
    eprintln!(
        "[measure] 本次用例累计：序列化 {} 次 {:.0}ms；mapping {} 次 {:.0}ms；行计划 {} 次 {:.0}ms",
        totals.0,
        totals.1 as f64 / 1e6,
        totals.2,
        totals.3 as f64 / 1e6,
        totals.4,
        totals.5 as f64 / 1e6,
    );
    // 每次按键的全文遍数必须是常数级（与文档大小无关）。
    //
    // 序列化这一项现在是 0：打字走的是区间写回，`mark_dirty_written_back` 声明过
    // 区间，就不该再整篇序列化一遍。读侧（搜索、大纲、状态栏）也全部改读缓冲区。
    // 这条从「≤1」收到「=0」，是 buffer 为事实源换来的实际收益。
    assert_eq!(delta.0, 0, "一次按键出现 {} 次全文序列化", delta.0);
    // 三次按块重建：撤销分组的选区快照、区间写回要找插入点、编辑后的选区快照。
    // 它们都只走「光标所在的那一根块」，成本随块大小而不是随文档大小长。
    assert!(delta.1 <= 3, "一次按键出现 {} 次 mapping 重建", delta.1);
    // 上面那 2 次额度只许是「按这一块重建」（`source_mapping_for_entity`，成本随
    // 被编辑的块走）。整篇重建是 O(文档)：1 MiB 实测一次 227ms，10 MiB 就是秒级，
    // 拿它换掉整篇序列化等于把 13 秒从一列挪到另一列。
    let full_mappings_after = editor.read_with(cx, |editor, _| {
        editor.source_mapping_full_builds.get()
    });
    assert_eq!(
        full_mappings_after - full_mappings_before,
        0,
        "一次按键出现整篇 source mapping 重建"
    );
    assert!(delta.2 <= 1, "一次按键出现 {} 次整篇字数扫描", delta.2);
    assert!(delta.3 <= 2, "一次按键出现 {} 次行计划重建", delta.3);

    let after = perf_passes(&editor, cx);
    let start = Instant::now();
    for _ in 0..5 {
        redraw(cx);
    }
    let idle = start.elapsed();
    let idle_delta = perf_delta(after, perf_passes(&editor, cx));
    eprintln!("[measure] 1 MiB 五个静止帧 {idle:?}，遍数 {idle_delta:?}");
    // 静止帧不许做任何全文级工作。
    assert_eq!(idle_delta, (0, 0, 0, 0), "静止帧出现全文级工作");
}

/// 阶段 1 闸门要求的 10 MiB 档：一次按键不许出现任何「整篇」遍历。
///
/// 夹具与 1 MiB 那份由 `scripts/generate-fixtures.mjs` 一起生成、同样被 gitignore，
/// 没生成就跳过。这里刻意不做单次全文测量——10 MiB 一次整篇 mapping 实测 4.6s，
/// 而那正是被禁止的行为本身。断言两件事：整篇序列化 0 次、整篇 mapping 重建 0 次，
/// 再加一个墙钟上限兜住「新增了别的整篇工作」。
///
/// 10 MiB markdown 一次按键的闸门：写回层不许出现整篇遍数，大纲也不许再把整篇重扫。
///
/// 成对实测（同一台机器、同一份夹具、dev 无优化）：改前一次按键 546ms，其中大纲
/// 重扫 585499 行（=全文）；改后 241ms，大纲重扫 0 行——侧栏收起又没 `[TOC]` 块时
/// 没人看这份树，打开大纲页签时也只重扫改动那一根块占的那几行。
/// 剩下的 241ms 仍与文档大小同向：可见列表没按视口裁剪，10 万根块的行计划与布局
/// 还在里面（方案 §10 的按窗口物化那一档）。
#[gpui::test]
#[ignore = "墙钟预算闸门，依赖机器速度；单独跑：cargo test --bin velora -- --ignored"]
async fn ten_mib_typing_does_not_scan_the_whole_document(cx: &mut TestAppContext) {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/perf/ten-mib.md");
    if !fixture.is_file() {
        eprintln!("skipping: generate fixtures with `node scripts/generate-fixtures.mjs fixtures/perf`");
        return;
    }
    init_editor_test_app(cx);
    let markdown = std::fs::read_to_string(&fixture).expect("read fixture");
    assert!(markdown.len() >= 10 * 1024 * 1024, "夹具应约 10 MiB");
    let (editor, cx) = cx.add_window_view(|_window, cx| Editor::from_markdown(cx, markdown, None));
    let deadline = Instant::now() + Duration::from_secs(600);
    while editor.read_with(cx, |editor, _| editor.document.pending_tail().is_some()) {
        assert!(Instant::now() < deadline, "续建未完成");
        cx.run_until_parked();
    }
    redraw(cx);

    let before = editor.read_with(cx, |editor, _| {
        (
            editor.source_serializations.get(),
            editor.source_mapping_full_builds.get(),
            editor.outline_lines_scanned.get(),
            editor.buffer.line_count(),
            editor.outline_full_rescans.get(),
        )
    });
    let start = Instant::now();
    cx.simulate_input("x");
    redraw(cx);
    let typed = start.elapsed();
    let after = editor.read_with(cx, |editor, _| {
        (
            editor.source_serializations.get(),
            editor.source_mapping_full_builds.get(),
            editor.outline_lines_scanned.get(),
            editor.buffer.line_count(),
            editor.outline_full_rescans.get(),
        )
    });
    eprintln!(
        "[measure] 10 MiB 一次按键 {typed:?}，整篇遍数 (序列化, mapping) = ({}, {})，大纲扫了 {} 行 / 全文 {} 行，整篇重扫 {} 次",
        after.0 - before.0,
        after.1 - before.1,
        after.2 - before.2,
        after.3,
        after.4 - before.4,
    );
    assert_eq!(after.0 - before.0, 0, "10 MiB 一次按键出现整篇序列化");
    assert_eq!(
        after.1 - before.1,
        0,
        "10 MiB 一次按键出现整篇 source mapping 重建"
    );
    assert_eq!(
        after.4 - before.4,
        0,
        "10 MiB 一次按键让大纲退回整篇重扫：按块增量这条路没生效"
    );
    assert!(
        after.2 - before.2 < after.3 as u64 / 8,
        "10 MiB 一次按键为了大纲重扫了 {} 行（全文 {} 行）：大纲读的是缓冲区全文，\
         改动只落在其中一根块上，就该只重扫那一段",
        after.2 - before.2,
        after.3,
    );
    assert!(
        typed < Duration::from_millis(1_500),
        "10 MiB 一次按键 {typed:?}，偏出预算"
    );
}

/// 一片 ``` 正好压在源码文档的切片接缝上时，大纲的按块增量也不该作废。
///
/// 源码/代码文档按 512 行切片，所以一条围栏的开栏与闭栏可以落在不同的片里：整片
/// 是从围栏**里面**开始的。以前摘要只记「这片结尾还在围栏里吗」，认不出下一片该
/// 接着栏内的状态扫，于是每次同步都退回整篇重扫——闸门数出来的是全文行号。
///
/// 成对实测（同一台机器、同一份夹具改出来的同一份文档，dev 无优化；把下面读的夹具
/// 换成 ten-mib.md 就是 10 MiB 那档）。一次大纲同步的开销：
/// 改前 **142ms、重扫 585499 行（=全文）、每帧整篇重扫 1 次**；
/// 改后 **44ms、重扫 512 行（=改动那一片）、整篇重扫 0 次**。
/// 这 44ms 后来又拆开量过一次（临时打段计时）：按字节数行号那一段约 5ms（换成每块
/// 的换行偏移表之后 0.26ms，见 `asking_for_line_numbers_does_not_read_the_text`），
/// 按块走查 0.54ms，剩下的 **约 37ms 全在「把 5.3 万条标题拼成一棵树」**——标题树是
/// 整篇一份，任何一次改动都要重拼，那是另一档与文档同向的开销。
#[gpui::test]
#[ignore = "墙钟预算闸门，依赖机器速度；单独跑：cargo test --bin velora -- --ignored"]
async fn one_mib_code_document_with_a_fence_on_the_seam_scans_one_chunk(
    cx: &mut TestAppContext,
) {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/perf/one-mib.md");
    if !fixture.is_file() {
        eprintln!("skipping: generate fixtures with `node scripts/generate-fixtures.mjs fixtures/perf`");
        return;
    }
    init_editor_test_app(cx);
    let chunk = crate::editor::file_drop::SOURCE_DOCUMENT_CHUNK_LINES;
    let markdown = std::fs::read_to_string(&fixture).expect("read fixture");
    let mut lines: Vec<&str> = markdown.lines().collect();
    // 开栏压在片 0 的最后一行，闭栏落在片 2 的第一行：中间整片都在围栏里。
    lines[chunk - 1] = "```";
    lines[chunk + 1] = "# 掉在围栏里，不算标题";
    lines[chunk * 2] = "```";
    let source = format!("{}\n", lines.join("\n"));
    let path = temp_fixture_dir().join(format!("velora-seam-gate-{}.py", temp_fixture_token()));
    fs::write(&path, &source).expect("write fixture");
    let cleanup = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup);
    });
    let document = crate::editor::encoding::load_document(&path).expect("load fixture");
    let (editor, cx) = cx.add_window_view(move |_window, cx| {
        Editor::from_loaded_document(cx, document, Some(path.clone()))
    });
    let deadline = Instant::now() + Duration::from_secs(600);
    while editor.read_with(cx, |editor, _| {
        editor.document.pending_source().is_some() || editor.document.pending_tail().is_some()
    }) {
        assert!(Instant::now() < deadline, "续建未完成");
        cx.run_until_parked();
    }
    redraw(cx);

    let counted = |editor: &gpui::Entity<Editor>, cx: &mut gpui::VisualTestContext| {
        editor.read_with(cx, |editor, _| {
            (
                editor.outline_lines_scanned.get(),
                editor.outline_full_rescans.get(),
                editor.buffer.line_count(),
                editor.outline_nanos.get(),
            )
        })
    };
    let sync = |editor: &gpui::Entity<Editor>, cx: &mut gpui::VisualTestContext| {
        editor.update(cx, |editor, cx| editor.sync_workspace_outline(cx));
        cx.run_until_parked();
    };

    // 第一次建大纲就得走按块这条路（侧栏收起时没人看，所以显式要一次）。
    let (_, rescans_before, _, _) = counted(&editor, cx);
    sync(&editor, cx);
    let (_, rescans_after, lines_total, _) = counted(&editor, cx);
    assert_eq!(
        rescans_after, rescans_before,
        "打开这份文档就把大纲整篇重扫了：压在接缝上的围栏让按块增量失效"
    );

    let (scanned_before, rescans_before, _, nanos_before) = counted(&editor, cx);
    let start = Instant::now();
    cx.simulate_input("x");
    redraw(cx);
    let typed = start.elapsed();
    sync(&editor, cx);
    let (scanned_after, rescans_after, _, nanos_after) = counted(&editor, cx);
    eprintln!(
        "[measure] 1 MiB 跨接缝围栏，一次按键 {typed:?}，大纲重扫 {} 行 / 全文 {lines_total} 行（{:.1}ms），整篇重扫 {} 次",
        scanned_after - scanned_before,
        (nanos_after - nanos_before) as f64 / 1e6,
        rescans_after - rescans_before,
    );
    assert_eq!(
        rescans_after - rescans_before,
        0,
        "打一个字让大纲退回整篇重扫"
    );
    assert!(
        scanned_after - scanned_before < lines_total as u64 / 8,
        "一次按键为了大纲重扫了 {} 行（全文 {lines_total} 行）：改动只落在一根块上，\
         就该只重扫那一片",
        scanned_after - scanned_before,
    );
    assert!(
        typed < Duration::from_millis(1_200),
        "1 MiB 代码文档一次按键 {typed:?}，偏出预算"
    );
}

#[gpui::test]
async fn per_keystroke_document_passes_stay_bounded(cx: &mut TestAppContext) {
    // P2 性能守门：大文档里每次按键都不许做整篇级的工作。这里数的是
    // 「全文序列化 / source mapping 重建 / 整篇字数扫描 / 行计划重建」
    // 四种全文遍数，用计数器而不是计时，避免并行跑测试时抖动。
    init_editor_test_app(cx);
    let markdown = (0..400)
        .map(|index| format!("## 第 {index} 节\n\n第 {index} 段正文。\n"))
        .collect::<Vec<_>>()
        .join("\n");
    let (editor, cx) = cx.add_window_view(|_window, cx| Editor::from_markdown(cx, markdown, None));
    redraw(cx);

    let before = perf_passes(&editor, cx);
    cx.simulate_input("x");
    redraw(cx);
    let after_type = perf_passes(&editor, cx);

    let before_idle = after_type;
    for _ in 0..5 {
        redraw(cx);
    }
    let after_idle = perf_passes(&editor, cx);

    let typing = perf_delta(before, after_type);
    eprintln!(
        "[measure] 一次按键 (序列化, mapping, 字数, 行计划) = {typing:?}"
    );
    assert_eq!(typing.0, 0, "一次按键做了 {typing:?} 次全文序列化");
    eprintln!(
        "[measure] 五个静止帧 = {:?}",
        perf_delta(before_idle, after_idle)
    );
}

/// 一次按键不许克隆整篇可见块列表。
///
/// `on_block_events` 原来对每一次块事件都 `flatten_visible_blocks()`，而文本改动只需要
/// 比「可见序列有没有变」。克隆一块要付一次 Entity 引用计数与 LeakDetector 登记：
/// 10 MiB 档 159,683 块克隆一次实测 17ms，占满一次按键（`probe_keystroke_stage_split`
/// 归因探针的块侧耗时从 17.4ms 降到 3.7ms 就是这一处）。
#[gpui::test]
async fn typing_does_not_clone_the_visible_block_list(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let markdown = (0..400)
        .map(|index| format!("## 第 {index} 节\n\n第 {index} 段正文。\n"))
        .collect::<Vec<_>>()
        .join("\n");
    let (editor, cx) = cx.add_window_view(|_window, cx| Editor::from_markdown(cx, markdown, None));
    redraw(cx);

    // 计数器本身要是活的：直接克隆一次必须留下数，否则下面的 0 断言是死的。
    let visible = editor.read_with(cx, |editor, _| editor.document.visible_blocks().len() as u64);
    assert!(visible > 0, "夹具没有可见块");
    let before = whole_document_passes(&editor, cx);
    editor.update(cx, |editor, _cx| {
        let cloned = editor.document.flatten_visible_blocks();
        assert_eq!(cloned.len() as u64, visible, "克隆出来的块数对不上");
    });
    let after = whole_document_passes(&editor, cx);
    assert_eq!(
        after.visible_blocks_cloned - before.visible_blocks_cloned,
        visible,
        "克隆可见列表没进计数器：这条闸门量不到东西"
    );

    for _ in 0..3 {
        let before = whole_document_passes(&editor, cx);
        let revision_before = editor.read_with(cx, |editor, _| editor.document_revision);
        cx.simulate_input("x");
        redraw(cx);
        let revision_after = editor.read_with(cx, |editor, _| editor.document_revision);
        assert_eq!(
            revision_after,
            revision_before.wrapping_add(1),
            "按键没落进编辑器，这条闸门测错了东西"
        );
        let after = whole_document_passes(&editor, cx);
        assert_eq!(
            after.visible_blocks_cloned - before.visible_blocks_cloned,
            0,
            "一次按键克隆了 {} 块可见列表",
            after.visible_blocks_cloned - before.visible_blocks_cloned,
        );
    }
}

#[gpui::test]
async fn ui_zoom_scales_document_text(cx: &mut TestAppContext) {
    // 偏好设置「界面缩放」/ ⌘+/⌘- 必须作用到正文块文本：此前正文块渲染自己拼
    // 主题排版，只套了字号设置，漏掉缩放因子；缩放只作用在编辑器外壳那份主题上。
    init_editor_test_app(cx);
    cx.update(|cx| crate::config::EditorSettings::init(cx, true));
    let (_editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(
            cx,
            "一段用来观察缩放的正文，长到足以在窗口里换行显示。\n".to_string(),
            None,
        )
    });
    redraw(cx);
    let before = cx
        .debug_bounds("block-shell")
        .expect("正文块应渲染")
        .size
        .height;
    cx.update(|_, cx| crate::config::EditorSettings::set_zoom_percent(cx, 200));
    redraw(cx);
    let after = cx
        .debug_bounds("block-shell")
        .expect("正文块应渲染")
        .size
        .height;
    cx.update(|_, cx| crate::config::EditorSettings::set_zoom_percent(cx, 100));
    assert!(
        after > before,
        "界面缩放 200% 后正文块应变高：{before:?} -> {after:?}"
    );
}

#[gpui::test]
async fn blank_line_block_renders_as_a_small_gap(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    // 松列表（- a / 空行 / - b）：空行块原本占一整行高 + 上下 padding，加起来
    // 比正文一行还高。空行块应该只剩下一个块间距的高度。
    let (_editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "- a\n\n- b\n".to_string(), None)
    });
    redraw(cx);
    let blank = cx.debug_bounds("block-blank-line").expect("没有找到空行块");
    let normal = cx.debug_bounds("block-shell").expect("没有找到正文块");
    assert!(
        blank.size.height <= px(20.0),
        "空行块高度 {:.1}px，还是太大",
        f32::from(blank.size.height)
    );
    assert!(
        blank.size.height < normal.size.height,
        "空行块 {:.1}px 不比正文块 {:.1}px 矮",
        f32::from(blank.size.height),
        f32::from(normal.size.height)
    );
}

/// 源码模式下状态栏的「行 : 列」每帧都要算，它不许把整篇序列化一遍。
///
/// 这一项是纯读取：文档多大都不该跟着它变贵。旧实现拿 `raw_source_text`（整篇拼出来
/// 的字符串）数换行，10 MiB 文档每帧一次大搬运。
#[gpui::test]
async fn the_status_bar_cursor_readout_does_not_serialize_the_document(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "第一行\n\n第三行 with 中文\n".to_string(), None)
    });
    redraw(cx);
    editor.update(cx, |editor, cx| editor.toggle_view_mode(cx));
    redraw(cx);

    let before = editor.read_with(cx, |editor, _| editor.document.whole_document_renders.get());
    cx.simulate_keystrokes("right");
    redraw(cx);
    let after = editor.read_with(cx, |editor, _| editor.document.whole_document_renders.get());
    assert_eq!(
        before, after,
        "状态栏读行列号时又把整篇序列化了一遍（{before} → {after}）"
    );
}

#[gpui::test]
async fn status_bar_view_mode_toggle_switches_mode(cx: &mut TestAppContext) {
    // 用户需求：右下角的「分钟阅读」换成源码切换按钮，点击切换视图模式。
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "# hello\n\nworld\n".to_string(), None)
    });
    redraw(cx);

    editor.read_with(cx, |editor, _| {
        assert_eq!(editor.view_mode, crate::editor::ViewMode::Rendered);
    });

    let bounds = cx
        .debug_bounds("status-bar-view-mode-toggle")
        .expect("状态栏应渲染视图切换按钮");
    cx.simulate_click(bounds.center(), Modifiers::none());
    redraw(cx);
    editor.read_with(cx, |editor, _| {
        assert_eq!(
            editor.view_mode,
            crate::editor::ViewMode::Source,
            "点击切换按钮应进入源码模式"
        );
    });

    let bounds = cx
        .debug_bounds("status-bar-view-mode-toggle")
        .expect("源码模式下按钮仍在");
    cx.simulate_click(bounds.center(), Modifiers::none());
    redraw(cx);
    editor.read_with(cx, |editor, _| {
        assert_eq!(
            editor.view_mode,
            crate::editor::ViewMode::Rendered,
            "再次点击应切回渲染模式"
        );
    });
}



/// 阶段 2 闸门：一次真实的编辑序列里，整篇渲染（任何来源）应该出现 0 次。
///
/// 走到兜底档位说明这条命令没声明自己的区间，未编辑块的原始字节就此丢掉（表格列宽
/// 填充、`__` 强调写法、CRLF、末行换行都是这样被洗掉的）；没走兜底但顺手把整篇渲染
/// 一遍（刷引用定义、脚注）同样是 O(文档) 的按键成本。每转一条路径，这里就少一个名额。
#[gpui::test]
async fn a_real_editing_session_never_falls_back_to_whole_document_serialization(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(
            cx,
            concat!(
                "- [ ] 任务甲\n",
                "  - 嵌套乙\n",
                "\n",
                "段落文字\n",
                "\n",
                "> [!note]\n",
                "> 标注正文\n",
                "\n",
                "| 名称 | 数量 |\n",
                "| ---- | ---- |\n",
                "| 甲   | 1    |\n",
                "\n",
                "强调 __下划线__ 结尾\n",
            )
            .to_string(),
            None,
        )
    });
    redraw(cx);

    let mut offenders: Vec<&'static str> = Vec::new();
    let mut before = source_serializations(&editor, cx);

    cx.simulate_input("写");
    redraw(cx);
    count_step(&mut offenders, "打字", &mut before, &editor, cx);

    cx.dispatch_action(Newline);
    redraw(cx);
    count_step(&mut offenders, "回车拆块", &mut before, &editor, cx);

    // 块首回车：切出来的是个空块。这一步以前会把本块的区间压塌成零宽，结构写回
    // 没有区间可用，只能整篇重投影。
    let head = visible_block_with_text("段落文字", &editor, cx);
    head.update(cx, |block, _cx| block.selected_range = 0..0);
    cx.update(|window, cx| {
        head.update(cx, |block, cx| block.on_newline(&Newline, window, cx));
    });
    redraw(cx);
    count_step(&mut offenders, "块首回车拆块", &mut before, &editor, cx);


    let task = visible_block_with_text("任务甲", &editor, cx);
    dispatch(&editor, task, crate::components::BlockEvent::ToggleTaskChecked, cx);
    count_step(&mut offenders, "勾任务复选框", &mut before, &editor, cx);

    let nested = visible_block_with_text("嵌套乙", &editor, cx);
    // 子项里打字：嵌套那层的记号宽度得按文件量，块自己的字节才算得出来。
    cx.update(|_window, cx| {
        editor.update(cx, |editor, _cx| editor.focus_block(nested.entity_id()));
        nested.update(cx, |block, block_cx| block.move_to(0, block_cx));
    });
    redraw(cx);
    cx.simulate_input("写");
    redraw(cx);
    count_step(&mut offenders, "子项里打字", &mut before, &editor, cx);

    dispatch(&editor, nested.clone(), crate::components::BlockEvent::RequestIndent, cx);
    count_step(&mut offenders, "缩进", &mut before, &editor, cx);
    dispatch(&editor, nested.clone(), crate::components::BlockEvent::RequestOutdent, cx);
    count_step(&mut offenders, "提级", &mut before, &editor, cx);
    dispatch(
        &editor,
        nested,
        crate::components::BlockEvent::RequestDowngradeNestedListItemToChildParagraph,
        cx,
    );
    count_step(&mut offenders, "嵌套项降级", &mut before, &editor, cx);

    let callout_body = visible_block_with_text("标注正文", &editor, cx);
    dispatch(&editor, callout_body, crate::components::BlockEvent::RequestQuoteBreak, cx);
    count_step(&mut offenders, "标注里拆块", &mut before, &editor, cx);

    editor.update(cx, |editor, cx| {
        let table = editor
            .document
            .root_blocks()
            .iter()
            .find(|root| root.read(cx).kind() == crate::components::BlockKind::Table)
            .cloned()
            .expect("夹具里应有一张表");
        editor.append_table_row(&table, cx);
    });
    count_step(&mut offenders, "表格加一行", &mut before, &editor, cx);

    editor.update(cx, |editor, cx| {
        let table = editor
            .document
            .root_blocks()
            .iter()
            .find(|root| root.read(cx).kind() == crate::components::BlockKind::Table)
            .cloned()
            .expect("表格还在");
        editor.delete_table_row(&table, 1, cx);
    });
    count_step(&mut offenders, "表格删一行", &mut before, &editor, cx);

    let table = editor.read_with(cx, |editor, cx| {
        editor
            .document
            .root_blocks()
            .iter()
            .find(|root| root.read(cx).kind() == crate::components::BlockKind::Table)
            .cloned()
            .expect("表格还在")
    });
    editor.update(cx, |editor, cx| {
        let cell = table
            .read(cx)
            .table_runtime
            .as_ref()
            .and_then(|runtime| runtime.cell(crate::components::TableCellPosition { row: 1, column: 0 }))
            .expect("数据行单元格");
        editor.on_block_event(
            cell.clone(),
            &crate::components::BlockEvent::RequestNewline {
                trailing: InlineTextTree::plain(String::new()),
                source_already_mutated: false,
            },
            cx,
        );
    });
    count_step(&mut offenders, "单元格里回车", &mut before, &editor, cx);

    editor.update(cx, |editor, cx| {
        let table = editor
            .document
            .root_blocks()
            .iter()
            .find(|root| root.read(cx).kind() == crate::components::BlockKind::Table)
            .cloned()
            .expect("表格还在");
        editor.remove_table_block(&table, cx);
    });
    count_step(&mut offenders, "删掉整张表", &mut before, &editor, cx);

    let target = visible_block_with_text("段落文字", &editor, cx);
    dispatch(
        &editor,
        target,
        crate::components::BlockEvent::RequestPasteMultiline {
            leading: InlineTextTree::plain(String::new()),
            lines: vec!["粘贴一".to_string(), "粘贴二".to_string()],
            trailing: InlineTextTree::plain(String::new()),
        },
        cx,
    );
    count_step(&mut offenders, "多行粘贴", &mut before, &editor, cx);

    let underscore = visible_block_with_text("强调 下划线 结尾", &editor, cx);
    dispatch(
        &editor,
        underscore,
        crate::components::BlockEvent::RequestNewline {
            trailing: InlineTextTree::plain(String::new()),
            source_already_mutated: false,
        },
        cx,
    );
    count_step(&mut offenders, "有定界符的块拆行", &mut before, &editor, cx);

    assert!(
        offenders.is_empty(),
        "这些命令还在整篇重新序列化（未编辑块的字节会被洗掉）：{offenders:?}"
    );
}

fn source_serializations(editor: &gpui::Entity<Editor>, cx: &mut gpui::VisualTestContext) -> u64 {
    // 兜底重投影那一遍 + 别的路径上悄悄发生的整篇渲染（引用定义、脚注、源码模式
    // 行列号各是一遍全文）。闸门要的是「这一键有没有做整篇的活」，只数其中一路
    // 等于给另一路开了后门。
    editor.read_with(cx, |editor, _| {
        editor.source_serializations.get() + editor.document.whole_document_renders.get()
    })
}

fn count_step(
    offenders: &mut Vec<&'static str>,
    label: &'static str,
    before: &mut u64,
    editor: &gpui::Entity<Editor>,
    cx: &mut gpui::VisualTestContext,
) {
    let now = source_serializations(editor, cx);
    if now > *before {
        offenders.push(label);
    }
    *before = now;
}

fn visible_block_with_text(
    wanted: &'static str,
    editor: &gpui::Entity<Editor>,
    cx: &mut gpui::VisualTestContext,
) -> gpui::Entity<crate::components::Block> {
    editor.read_with(cx, |editor, cx| {
        editor
            .document
            .visible_blocks()
            .iter()
            .find(|visible| visible.entity.read(cx).display_text() == wanted)
            .map(|visible| visible.entity.clone())
            .unwrap_or_else(|| panic!("夹具里找不到内容为 {wanted:?} 的块"))
    })
}

fn dispatch(
    editor: &gpui::Entity<Editor>,
    block: gpui::Entity<crate::components::Block>,
    event: crate::components::BlockEvent,
    cx: &mut gpui::VisualTestContext,
) {
    editor.update(cx, |editor, cx| {
        editor.on_block_event(block, &event, cx);
    });
    redraw(cx);
}

/// 源码模式（未闭合的 fenced div、不支持的 admonition 触发的整篇兜底）打字：
/// 阶段 2 第 3 条——源码模式就是缓冲区的一份视图，一次按键只该落在光标那一段字节上。
/// 以前这一档走 `raw_source_text`：整篇文本从块树重拼、整篇比较、整篇落笔，
/// 1 MiB 实测一次按键 2.28 秒（大头是把整篇文本重解析成行内树）。闸门数得到它
/// 之后（`ed2df8f`），这一步要求它归零。
#[gpui::test]
async fn source_mode_typing_writes_through_the_buffer_and_preserves_every_other_byte(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    let source = "::: {.column-margin}\n未闭合的 div\n".to_string();
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, source.clone(), None));
    redraw(cx);

    let (mode, roots, before) = editor.read_with(cx, |editor, _cx| {
        (
            editor.view_mode,
            editor.document.root_blocks().len(),
            editor.source_serializations.get(),
        )
    });
    assert_eq!(mode, crate::editor::ViewMode::Source, "夹具该走源码模式兜底");
    assert_eq!(roots, 1, "源码模式该是整篇一个块");

    cx.simulate_input("X");
    redraw(cx);

    let (after, buffer) = editor.read_with(cx, |editor, _cx| {
        (editor.source_serializations.get(), editor.buffer.text())
    });
    assert_eq!(
        after, before,
        "源码模式打字又走了一遍整篇落笔（这一键多付 {after} 次 O(文档) 序列化）"
    );
    assert_eq!(
        buffer,
        "X::: {.column-margin}\n未闭合的 div\n",
        "源码模式打字改动了光标以外不该动的字节"
    );
}

/// 代码/纯文本文档（`.py`、`.txt` 这类整篇按行分块的源码视图）打字同上：一次按键
/// 一次整篇落笔，1 MiB 的 `.rs` 文件就是秒级。这一档的块本来就是缓冲区的一段切片，
/// 区间应该现成。
#[gpui::test]
async fn typing_in_a_code_document_writes_through_the_buffer(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let mut source = String::from("def 甲():\n    return 1\n");
    for index in 0..600 {
        source.push_str(&format!("# 第 {index} 行注释\n"));
    }
    let path = temp_fixture_dir().join(format!("velora-code-gate-{}.py", temp_fixture_token()));
    fs::write(&path, &source).expect("write fixture");
    let cleanup = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup);
    });
    let document = crate::editor::encoding::load_document(&path).expect("read fixture");
    let (editor, cx) = cx.add_window_view(move |_window, cx| {
        Editor::from_loaded_document(cx, document, Some(path.clone()))
    });
    redraw(cx);

    let (mode, roots, before) = editor.read_with(cx, |editor, _cx| {
        (
            editor.view_mode,
            editor.document.root_blocks().len(),
            editor.source_serializations.get(),
        )
    });
    assert_eq!(mode, crate::editor::ViewMode::Source, "代码文档该是源码视图");
    assert!(roots >= 2, "夹具得分成几块才测得出「只动这一块」：{roots} 块");

    cx.simulate_input("X");
    redraw(cx);

    let (after, buffer) = editor.read_with(cx, |editor, _cx| {
        (editor.source_serializations.get(), editor.buffer.text())
    });
    assert_eq!(
        after, before,
        "代码文档打字又走了一遍整篇落笔（这一键多付 {after} 次 O(文档) 序列化）"
    );
    assert_eq!(
        buffer,
        format!("X{source}"),
        "代码文档打字改动了光标以外不该动的字节"
    );
}

/// 1 MiB 的文档切到源码视图再打字：这一档以前一次按键 **1.99 秒**（整篇重拼 + 整篇比较 +
/// 整篇落笔，每键一次），改成按区间落笔后实测 **141ms、整篇落笔 0 次**。夹具由
/// `scripts/generate-fixtures.mjs` 生成且被 gitignore，缺失即跳过。
#[gpui::test]
#[ignore = "墙钟预算闸门，依赖机器速度；单独跑：cargo test --bin velora -- --ignored"]
async fn one_mib_source_mode_typing_stays_within_budget(cx: &mut TestAppContext) {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/perf/one-mib.md");
    if !fixture.is_file() {
        eprintln!("skipping: generate fixtures with `node scripts/generate-fixtures.mjs fixtures/perf`");
        return;
    }
    init_editor_test_app(cx);
    let markdown = std::fs::read_to_string(&fixture).expect("read fixture");
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, markdown, None));
    let deadline = Instant::now() + Duration::from_secs(120);
    while editor.read_with(cx, |editor, _| editor.document.pending_tail().is_some()) {
        assert!(Instant::now() < deadline, "续建未完成");
        cx.run_until_parked();
    }
    redraw(cx);
    editor.update(cx, |editor, cx| editor.toggle_view_mode(cx));
    redraw(cx);
    let (mode, serializations_before, full_before, len_before) =
        editor.read_with(cx, |editor, _cx| {
            (
                editor.view_mode,
                editor.source_serializations.get(),
                editor.source_mapping_full_builds.get(),
                editor.buffer.byte_len(),
            )
        });
    assert_eq!(mode, crate::editor::ViewMode::Source, "夹具该切到源码视图");

    let passes_before = perf_passes(&editor, cx);
    let start = Instant::now();
    cx.simulate_input("x");
    redraw(cx);
    let typed = start.elapsed();
    let (serializations, full, len_after) = editor.read_with(cx, |editor, _cx| {
        (
            editor.source_serializations.get() - serializations_before,
            editor.source_mapping_full_builds.get() - full_before,
            editor.buffer.byte_len(),
        )
    });
    let delta = perf_delta(passes_before, perf_passes(&editor, cx));
    eprintln!(
        "[measure] 1 MiB 源码模式一次按键 {typed:?}（整篇落笔 {serializations} 次，整篇 mapping {full} 次，遍数 (序列化, mapping, 字数, 行计划) = {delta:?}）"
    );
    assert_eq!(
        len_after,
        len_before + 1,
        "这个字没进缓冲区：0 次落笔是因为没干活，还是因为压根没打字"
    );
    assert_eq!(serializations, 0, "1 MiB 源码模式打字还在整篇落笔");
    assert_eq!(full, 0, "1 MiB 源码模式打字还在整篇重建 mapping");
    assert!(
        typed < Duration::from_millis(1200),
        "1 MiB 源码模式一次按键 {typed:?}，偏出预算（改前同一份文档实测 1.99 秒）"
    );
}

/// 阶段 2「增量重投影」：在引用块里按回车，只该重投影这一根引用。
///
/// 引用行的换行可能改结构（行首变成 `- 项` 就不再是引用行了），所以这条路要走
/// `normalize_rendered_quote_structure`。但那一步现在做的是「整棵树落进缓冲区 +
/// 整篇重解析」：60 根块的文档里改一根引用，实体全部换掉、未编辑块的字节也被
/// 重新序列化一遍。这里两个数一起守：整篇序列化 0 次，重投影出来的根块数有界。
#[gpui::test]
async fn entering_a_quote_reprojects_only_that_quote(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let mut markdown = String::new();
    for index in 0..30 {
        markdown.push_str(&format!("第 {index} 段正文。\n\n"));
    }
    markdown.push_str("> 引用一\n> 引用二\n\n");
    for index in 0..30 {
        markdown.push_str(&format!("尾段 {index}。\n\n"));
    }
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, markdown, None));
    redraw(cx);

    let quote = editor.read_with(cx, |editor, cx| {
        editor
            .document
            .root_blocks()
            .iter()
            .find(|root| root.read(cx).kind().is_quote_container())
            .cloned()
            .expect("夹具里应有一根引用")
    });
    let (rebuilt_before, serializations_before, root_count) = editor.read_with(cx, |editor, _| {
        (
            editor.roots_reprojected.get(),
            editor.source_serializations.get(),
            editor.document.root_blocks().len(),
        )
    });
    assert!(root_count > 40, "夹具得够大才测得出「跟着文档长」：{root_count} 根");

    quote.update(cx, |block, _cx| block.selected_range = 3..3);
    cx.update(|window, cx| {
        quote.update(cx, |block, cx| block.on_newline(&Newline, window, cx));
    });
    redraw(cx);

    let rebuilt = editor.read_with(cx, |editor, _| editor.roots_reprojected.get()) - rebuilt_before;
    let serializations =
        editor.read_with(cx, |editor, _| editor.source_serializations.get())
            - serializations_before;
    eprintln!("[measure] 一次引用内回车：整篇序列化 {serializations} 次，重投影 {rebuilt} 根 / 全文 {root_count} 根");
    assert_eq!(serializations, 0, "在引用里按回车还在全篇重新序列化");
    assert!(
        rebuilt <= 2,
        "在引用里按回车重投影了 {rebuilt} 根块（全文 {root_count} 根），只该动这一根引用"
    );
}

/// 1 MiB 的代码/纯文本文件（按 512 行一块分块）打一个字：实测 26ms，整篇落笔 0 次。
/// 这条把探针钉成闸门——这一档以前每次按键都要整篇重拼再整篇比较（源码模式那份成本
/// 与文档同长），现在块带着自己的缓冲区区间，只动光标那一段。
#[gpui::test]
#[ignore = "墙钟预算闸门，依赖机器速度；单独跑：cargo test --bin velora -- --ignored"]
async fn one_mib_code_document_typing_stays_within_budget(cx: &mut TestAppContext) {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/perf/one-mib.md");
    if !fixture.is_file() {
        eprintln!("skipping: generate fixtures with `node scripts/generate-fixtures.mjs fixtures/perf`");
        return;
    }
    init_editor_test_app(cx);
    let markdown = std::fs::read_to_string(&fixture).expect("read fixture");
    let path = temp_fixture_dir().join(format!("velora-budget-code-{}.py", temp_fixture_token()));
    fs::write(&path, &markdown).expect("write fixture");
    let cleanup = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup);
    });
    let document = crate::editor::encoding::load_document(&path).expect("read fixture");
    let (editor, cx) = cx.add_window_view(move |_window, cx| {
        Editor::from_loaded_document(cx, document, Some(path.clone()))
    });
    let deadline = Instant::now() + Duration::from_secs(120);
    while editor.read_with(cx, |editor, _| {
        editor.document.pending_source().is_some() || editor.document.pending_tail().is_some()
    }) {
        assert!(Instant::now() < deadline, "续建未完成");
        cx.run_until_parked();
    }
    redraw(cx);

    let (mode, serializations_before, full_before, len_before) =
        editor.read_with(cx, |editor, _cx| {
            (
                editor.view_mode,
                editor.source_serializations.get(),
                editor.source_mapping_full_builds.get(),
                editor.buffer.byte_len(),
            )
        });
    assert_eq!(mode, crate::editor::ViewMode::Source, "代码文件该是源码视图");

    let passes_before = perf_passes(&editor, cx);
    let start = Instant::now();
    cx.simulate_input("x");
    redraw(cx);
    let typed = start.elapsed();
    let (serializations, full, len_after) = editor.read_with(cx, |editor, _cx| {
        (
            editor.source_serializations.get() - serializations_before,
            editor.source_mapping_full_builds.get() - full_before,
            editor.buffer.byte_len(),
        )
    });
    let delta = perf_delta(passes_before, perf_passes(&editor, cx));
    eprintln!(
        "[measure] 1 MiB 代码文档一次按键 {typed:?}（整篇落笔 {serializations} 次，遍数 (序列化, mapping, 字数, 行计划) = {delta:?}）"
    );
    assert_eq!(
        len_after,
        len_before + 1,
        "这个字没进缓冲区：0 次落笔是因为没干活，还是因为压根没打字"
    );
    assert_eq!(serializations, 0, "1 MiB 代码文档打字还在整篇落笔");
    assert_eq!(full, 0, "1 MiB 代码文档打字还在整篇重建 mapping");
    assert!(
        typed < Duration::from_millis(400),
        "1 MiB 代码文档一次按键 {typed:?}，偏出预算（空闲机器上实测 26ms）"
    );
}

/// 一次按键要付的「把整篇文档再走一遍」的遍数。归因探针用它前后作差：
/// 哪一项跟着文档长度长，哪一项就是那一键的线性成本来源。
#[derive(Clone, Copy, Default)]
struct WholeDocumentPasses {
    outline_rebuilds: u64,
    outline_full_rescans: u64,
    outline_lines_scanned: u64,
    outline_nanos: u64,
    snapshot_rebuilds: u64,
    snapshot_nanos: u64,
    row_plan_rebuilds: u64,
    row_plan_nanos: u64,
    source_mapping_full_builds: u64,
    word_count_scans: u64,
    source_serializations: u64,
    whole_document_renders: u64,
    visible_blocks_cloned: u64,
}

fn whole_document_passes(
    editor: &gpui::Entity<Editor>,
    cx: &mut gpui::VisualTestContext,
) -> WholeDocumentPasses {
    editor.read_with(cx, |editor, _cx| WholeDocumentPasses {
        outline_rebuilds: editor.outline_rebuilds.get(),
        outline_full_rescans: editor.outline_full_rescans.get(),
        outline_lines_scanned: editor.outline_lines_scanned.get(),
        outline_nanos: editor.outline_nanos.get(),
        snapshot_rebuilds: editor.document.snapshot_rebuilds.get(),
        snapshot_nanos: editor.document.snapshot_nanos.get(),
        row_plan_rebuilds: editor.row_plan_rebuilds.get(),
        row_plan_nanos: editor.row_plan_nanos.get(),
        source_mapping_full_builds: editor.source_mapping_full_builds.get(),
        word_count_scans: editor.word_count_scans.get(),
        source_serializations: editor.source_serializations.get(),
        whole_document_renders: editor.document.whole_document_renders.get(),
        visible_blocks_cloned: editor.document.visible_blocks_cloned.get(),
    })
}

/// 归因探针：10 MiB 文档一次按键的钱花在哪。跑法：
/// `cargo test probe_attribute_ten_mib -- --ignored --nocapture`。
#[gpui::test]
#[ignore = "诊断探针：手动跑，看 10 MiB 文档一次按键的耗时归因"]
async fn probe_attribute_ten_mib_keystroke(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/perf/ten-mib.md");
    if !fixture.is_file() {
        eprintln!("skipping");
        return;
    }
    let markdown = std::fs::read_to_string(&fixture).expect("read fixture");
    let path = temp_fixture_dir().join(format!("velora-probe-attr-{}.py", temp_fixture_token()));
    fs::write(&path, &markdown).expect("write fixture");
    let cleanup = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup);
    });
    let document = crate::editor::encoding::load_document(&path).expect("read fixture");
    let (editor, cx) = cx.add_window_view(move |_window, cx| {
        Editor::from_loaded_document(cx, document, Some(path.clone()))
    });
    let deadline = Instant::now() + Duration::from_secs(600);
    while editor.read_with(cx, |editor, _| {
        editor.document.pending_source().is_some() || editor.document.pending_tail().is_some()
    }) {
        assert!(Instant::now() < deadline, "续建未完成");
        cx.run_until_parked();
    }
    redraw(cx);
    let lines = editor.read_with(cx, |editor, _| editor.buffer.line_count());
    let roots = editor.read_with(cx, |editor, _| editor.document.root_count());
    for i in 0..3 {
        let before = whole_document_passes(&editor, cx);
        let start = Instant::now();
        cx.simulate_input("x");
        redraw(cx);
        let cost = start.elapsed();
        let after = whole_document_passes(&editor, cx);
        eprintln!(
            "[attr] 第 {i} 次按键 {cost:?}：大纲 {} 次（整篇 {}）/ {} 行 / {:.0}ms；投影重排 {} 次 {:.0}ms；行计划 {}；整篇 mapping {}；字数扫描 {}；整篇落笔 {}；整篇渲染 {}",
            after.outline_rebuilds - before.outline_rebuilds,
            after.outline_full_rescans - before.outline_full_rescans,
            after.outline_lines_scanned - before.outline_lines_scanned,
            (after.outline_nanos - before.outline_nanos) as f64 / 1e6,
            after.snapshot_rebuilds - before.snapshot_rebuilds,
            (after.snapshot_nanos - before.snapshot_nanos) as f64 / 1e6,
            after.row_plan_rebuilds - before.row_plan_rebuilds,
            after.source_mapping_full_builds - before.source_mapping_full_builds,
            after.word_count_scans - before.word_count_scans,
            after.source_serializations - before.source_serializations,
            after.whole_document_renders - before.whole_document_renders,
        );
    }
    eprintln!("[attr] 文档 {lines} 行、{roots} 根块");
}

/// 归因探针（markdown 那份）：与 `ten_mib_typing_does_not_scan_the_whole_document`
/// 同一份文档、同一个入口，只是多按几次键并打出所有整篇遍数，方便 `sample` 挂上去。
#[gpui::test]
#[ignore = "诊断探针：手动跑，配合 sample 看每键的整篇遍数"]
async fn probe_attribute_ten_mib_markdown_keystroke(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/perf/ten-mib.md");
    if !fixture.is_file() {
        eprintln!("skipping");
        return;
    }
    let markdown = std::fs::read_to_string(&fixture).expect("read fixture");
    let (editor, cx) = cx.add_window_view(|_window, cx| Editor::from_markdown(cx, markdown, None));
    let deadline = Instant::now() + Duration::from_secs(600);
    while editor.read_with(cx, |editor, _| editor.document.pending_tail().is_some()) {
        assert!(Instant::now() < deadline, "续建未完成");
        cx.run_until_parked();
    }
    redraw(cx);
    let roots = editor.read_with(cx, |editor, _| editor.document.root_count());
    for i in 0..6 {
        let before = whole_document_passes(&editor, cx);
        let start = Instant::now();
        cx.simulate_input("x");
        redraw(cx);
        let cost = start.elapsed();
        let after = whole_document_passes(&editor, cx);
        eprintln!(
            "[attr] markdown 第 {i} 次按键 {cost:?}：大纲 {} 次（整篇 {}）/ {} 行 / {:.1}ms；投影重排 {}；行计划 {} 次 {:.1}ms；整篇 mapping {}；整篇落笔 {}；整篇渲染 {}",
            after.outline_rebuilds - before.outline_rebuilds,
            after.outline_full_rescans - before.outline_full_rescans,
            after.outline_lines_scanned - before.outline_lines_scanned,
            (after.outline_nanos - before.outline_nanos) as f64 / 1e6,
            after.snapshot_rebuilds - before.snapshot_rebuilds,
            after.row_plan_rebuilds - before.row_plan_rebuilds,
            (after.row_plan_nanos - before.row_plan_nanos) as f64 / 1e6,
            after.source_mapping_full_builds - before.source_mapping_full_builds,
            after.source_serializations - before.source_serializations,
            after.whole_document_renders - before.whole_document_renders,
        );
    }
    eprintln!("[attr] markdown 文档 {roots} 根块");
}

fn measured_prefixes(editor: &gpui::Entity<Editor>, cx: &mut gpui::VisualTestContext) -> u64 {
    editor.read_with(cx, |editor, _| editor.line_prefix_measured.get())
}

/// 一次真实的编辑序列里，「事后拿文件行与模型行比记号宽度」一次都不该出现。
///
/// 每一行让开几字节是解析期记下的数据（不变式 23），块内行数改了那份账跟着改；比出来
/// 的那条路只是「账还没有」时的退路。这里按形状逐一走过打字、回车、再打字，哪一步还在
/// 比就把形状名字报出来。围栏那一族不在表上：拆它自己那一步里有一回换算落在「模型已经
/// 多一行、文件那一行还没写下去」的窗口里，那一次只能比；落点由
/// `newline_in_a_fenced_code_block_inserts_only_a_line_break` 钉住。
#[gpui::test]
async fn a_real_editing_session_never_measures_marker_widths_after_the_fact(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(
            cx,
            concat!(
                "# 标题甲\n",
                "\n",
                "段落文字\n",
                "\n",
                "- [ ] 任务甲\n",
                "  - 嵌套乙\n",
                "\n",
                "> 引用正文\n",
                "\n",
                "```rust\n",
                "let a = 1;\n",
                "```\n",
                "\n",
                "    let indented = 1;\n",
                "\n",
                "| 名称 | 数量 |\n",
                "| ---- | ---- |\n",
                "| 甲   | 1    |\n",
            )
            .to_string(),
            None,
        )
    });
    redraw(cx);

    let mut offenders: Vec<&'static str> = Vec::new();
    let mut before = measured_prefixes(&editor, cx);
    for (label, wanted) in [
        ("标题里", "标题甲"),
        ("段落里", "段落文字"),
        ("任务项里", "任务甲"),
        ("嵌套项里", "嵌套乙"),
        ("引用里", "引用正文"),
        ("缩进代码块里", "let indented = 1;"),
    ] {
        let target = editor.read_with(cx, |editor, cx| {
            editor
                .document
                .visible_blocks()
                .iter()
                .find(|visible| visible.entity.read(cx).display_text() == wanted)
                .map(|visible| visible.entity.clone())
                .unwrap_or_else(|| panic!("夹具里找不到 {wanted:?}: 可见块 = {:?}", editor.document.visible_blocks().iter().map(|v| v.entity.read(cx).display_text().to_string()).collect::<Vec<_>>()))
        });
        cx.update(|_window, cx| {
            editor.update(cx, |editor, _cx| editor.focus_block(target.entity_id()));
            target.update(cx, |block, block_cx| block.move_to(block.visible_len(), block_cx));
        });
        redraw(cx);

        cx.simulate_input("写");
        redraw(cx);
        let now = measured_prefixes(&editor, cx);
        if now > before {
            offenders.push(label);
        }
        before = now;

        cx.dispatch_action(Newline);
        redraw(cx);
        let now = measured_prefixes(&editor, cx);
        if now > before {
            offenders.push(label);
        }
        before = now;

        cx.simulate_input("字");
        redraw(cx);
        let now = measured_prefixes(&editor, cx);
        if now > before {
            offenders.push(label);
        }
        before = now;
    }

    assert!(
        offenders.is_empty(),
        "这些形状还在事后拿文件行与模型行比记号宽度：{offenders:?}"
    );
}

#[gpui::test]
async fn row_plan_rebuild_reads_only_the_headings_not_every_block(cx: &mut TestAppContext) {
    // 行结构计划每键重建一次，重建里「读了几个块实体」就是它随文档长度长的系数
    // （10 MiB 实测 159,683 个可见块全读一遍，121ms 的大头就在这里）。折叠过滤与
    // 分组扫描要的行元数据，在同步可见列表那一步已经逐块算过一遍并写回块上——
    // 缓存进快照之后，这里只该按标题数读（折叠状态与 chevron），目录那一路只读
    // `[TOC]` 候选，其余块连实体都不碰。
    init_editor_test_app(cx);
    let sections = 100usize;
    let markdown = (0..sections)
        .map(|index| format!("## 第 {index} 节\n\n第 {index} 段正文。\n"))
        .collect::<Vec<_>>()
        .join("\n");
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, markdown, None));
    redraw(cx);

    let blocks = editor.read_with(cx, |editor, _| editor.document.visible_blocks().len());
    editor.update(cx, |editor, _cx| {
        let last = editor.document.visible_blocks()[blocks - 1].entity.entity_id();
        editor.focus_block(last);
        editor.row_plan_block_reads.set(0);
        editor.row_plan_rebuilds.set(0);
    });
    redraw(cx);
    editor.update(cx, |editor, _cx| {
        editor.row_plan_block_reads.set(0);
        editor.row_plan_rebuilds.set(0);
    });

    cx.simulate_input("字");
    redraw(cx);
    // 打字不重排行计划（行元数据没变，见 typing_a_plain_character_...），
    // 换一个真会重排的动作继续测「重建里读了几个实体」：回车拆块。
    let rebuilds_before_newline = editor.read_with(cx, |editor, _| editor.row_plan_rebuilds.get());
    assert_eq!(
        rebuilds_before_newline, 0,
        "打字不该重排行计划（行元数据没变）"
    );
    editor.update(cx, |editor, _cx| {
        editor.row_plan_block_reads.set(0);
        editor.row_plan_rebuilds.set(0);
    });
    cx.dispatch_action(Newline);
    redraw(cx);

    let (reads, rebuilds, snapshot_rebuilds) = editor.read_with(cx, |editor, _| {
        (
            editor.row_plan_block_reads.get(),
            editor.row_plan_rebuilds.get(),
            editor.document.snapshot_rebuilds.get(),
        )
    });
    eprintln!(
        "可见块 {blocks} · 行计划重建 {rebuilds} 次 · 投影重排 {snapshot_rebuilds} 次 · 读实体 {reads} 次"
    );
    assert!(
        rebuilds >= 1,
        "拆块之后行计划没重建，闸门测不到东西"
    );
    assert!(
        reads <= sections as u64 * 2 + 16,
        "一次按键的行计划重建读了 {reads} 个块实体（可见块 {blocks}、标题 {sections}）：\
         行元数据还在逐块问实体要"
    );
}

/// 归因探针（markdown 那份 + **大纲页签打开**）：`[TOC]` 之外，侧栏的大纲页签
/// 是大纲的另一个读者——它一出现，(a) 每次同步都要把 5 万条标题重新拼成一棵树
/// （`install_outline`），(b) 每帧还要把整棵树走成元素树，(c) 每次按键还要逼行计划
/// 重排。三处都已收口：按视口开窗、原地换标签、行计划键换成行元数据版本。
/// 实测一次按键 2.3–2.6s → 正文里 **174–175ms**（行计划 0 次重排）、标题里
/// 197–272ms，剩下的大纲同步 ~88ms 是「每键仍走一遍 10.6 万根块 + 5.3 万条标题克隆」。
/// 跑法：`cargo test probe_attribute_ten_mib_markdown_with_outline -- --ignored --nocapture`。
#[gpui::test]
#[ignore = "诊断探针：手动跑，看大纲打开时每键的耗时切片"]
async fn probe_attribute_ten_mib_markdown_with_outline_open(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/perf/ten-mib.md");
    if !fixture.is_file() {
        eprintln!("skipping");
        return;
    }
    let markdown = std::fs::read_to_string(&fixture).expect("read fixture");
    let (editor, cx) = cx.add_window_view(|_window, cx| Editor::from_markdown(cx, markdown, None));
    let deadline = Instant::now() + Duration::from_secs(600);
    while editor.read_with(cx, |editor, _| editor.document.pending_tail().is_some()) {
        assert!(Instant::now() < deadline, "续建未完成");
        cx.run_until_parked();
    }
    editor.update(cx, |editor, _cx| {
        editor.workspace.is_open = true;
        editor.workspace.active_tab = crate::editor::workspace::WorkspaceTab::Outline;
    });
    redraw(cx);
    editor.read_with(cx, |editor, _| {
        fn count(nodes: &[crate::editor::workspace::WorkspaceTreeNode]) -> usize {
            nodes.iter().map(|node| 1 + count(&node.children)).sum()
        }
        eprintln!(
            "[attr-outline] 目录条目 {} 条、顶层 {}、侧栏节点总数 {}、展开 {} 项",
            editor.workspace.toc_entries.len(),
            editor.workspace.outline_tree.len(),
            count(&editor.workspace.outline_tree),
            editor.workspace.expanded.len(),
        );
    });

    for i in 0..3 {
        let before = whole_document_passes(&editor, cx);
        let start = Instant::now();
        cx.simulate_input("x");
        redraw(cx);
        let cost = start.elapsed();
        let after = whole_document_passes(&editor, cx);
        eprintln!(
            "[attr-outline] 第 {i} 次按键（标题里）{cost:?}：大纲 {} 次（整篇 {}）/ {} 行 / {:.1}ms；行计划 {} 次 {}ms",
            after.outline_rebuilds - before.outline_rebuilds,
            after.outline_full_rescans - before.outline_full_rescans,
            after.outline_lines_scanned - before.outline_lines_scanned,
            (after.outline_nanos - before.outline_nanos) as f64 / 1e6,
            after.row_plan_rebuilds - before.row_plan_rebuilds,
            after.row_plan_nanos - before.row_plan_nanos,
        );
    }

    // 第二段（正文）：行元数据（层级/分组锚点/目录标记）一个没动，行计划
    // 应该整个复用（这是按键的常态）。
    editor.update(cx, |editor, cx| {
        let paragraph = editor.document.root_blocks()[1].clone();
        editor.focus_block(paragraph.entity_id());
        paragraph.update(cx, |block, block_cx| block.move_to(0, block_cx));
    });
    redraw(cx);
    for i in 0..3 {
        let before = whole_document_passes(&editor, cx);
        let start = Instant::now();
        cx.simulate_input("x");
        redraw(cx);
        let cost = start.elapsed();
        let after = whole_document_passes(&editor, cx);
        eprintln!(
            "[attr-outline] 第 {i} 次按键（正文里）{cost:?}：大纲 {} 次（整篇 {}）/ {} 行 / {:.1}ms；行计划 {} 次 {}ms",
            after.outline_rebuilds - before.outline_rebuilds,
            after.outline_full_rescans - before.outline_full_rescans,
            after.outline_lines_scanned - before.outline_lines_scanned,
            (after.outline_nanos - before.outline_nanos) as f64 / 1e6,
            after.row_plan_rebuilds - before.row_plan_rebuilds,
            after.row_plan_nanos - before.row_plan_nanos,
        );
    }
}

/// 整篇落笔给根块挂区间时，每根块只许查一次候选。
///
/// `reattach_root_spans` 原来对每根块在 `block_spans` 里线性 `find` 一遍，是
/// O(根块数 × 条数)：10 MiB 档 106,456 根。采样里这一次 `find` 占满整篇落笔的
/// 绝大部分样本——一次 ⌘Z 35 秒就是这么来的。摊成一张表之后每根块只查一次。
///
/// 探针口径是「查了几条候选」而不是「跑了几轮循环」：查表版每根块 +1，退回线性
/// `find` 时必须改成每比较一条候选 +1（见 `Editor::span_reattach_probes` 的注释），
/// 所以这里断言的是 `<= 根块数`——这个上界只在「每根块恰好一条候选」时成立。
#[gpui::test]
async fn reattaching_root_spans_does_not_scan_candidates_per_block(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let markdown = (0..400)
        .map(|index| format!("## 第 {index} 节\n\n第 {index} 段正文。\n"))
        .collect::<String>();
    let (editor, cx) = cx.add_window_view(|_window, cx| Editor::from_markdown(cx, markdown, None));
    redraw(cx);
    cx.simulate_input("x");
    redraw(cx);

    let roots = editor.read_with(cx, |editor, _cx| editor.document.root_count() as u64);
    assert!(roots > 100, "夹具只有 {roots} 根块，量不出线性与查表的差别");
    editor.update(cx, |editor, _cx| editor.span_reattach_probes.set(0));
    // 走一次整篇落笔（`resync_buffer_from_projection` → 重挂区间）：撤销自己会跳过
    // 这一步，所以这里显式用 `mark_dirty` 那条路把它叫起来。
    editor.update(cx, |editor, cx| {
        let applied = editor.buffer.edit(0..0, "y");
        editor.record_buffer_edit(applied);
        editor.mark_dirty(cx);
    });
    redraw(cx);
    let probes = editor.read_with(cx, |editor, _cx| editor.span_reattach_probes.get());
    assert!(
        probes > 0,
        "这次操作没走重挂区间那一段，闸门测错了东西"
    );
    assert!(
        probes <= roots,
        "给 {roots} 根块挂区间查了 {probes} 条候选：这是逐根块扫一遍候选表"
    );
}

/// 撤销只在改动真的碰了引用/定义行时才重建整篇引用注册表。
///
/// 以前撤销整篇重解析，注册表总是跟着表格收尾重建一遍（10 MiB 档 766ms）；
/// 区域重投影之后，没碰定义行的撤销一次都不该付。正控保证收窄没把该动的漏掉：
/// 改定义行本身，撤销必须重建。
#[gpui::test]
async fn undoing_rebuilds_the_reference_registry_only_for_definition_changes(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    let markdown = (0..40)
        .map(|index| format!("## 第 {index} 节\n\n[asset{index}]: ./assets/{index}.png\n"))
        .collect::<String>();
    let editor = cx.update(|cx| cx.new(|cx| Editor::from_markdown(cx, markdown, None)));
    cx.run_until_parked();
    let roots = editor.read_with(cx, |editor, _cx| editor.document.root_blocks().to_vec());
    let first = roots.first().cloned().expect("夹具应有根块");
    first.update(cx, |block, cx| {
        block.replace_text_in_visible_range(0..0, "x", None, false, cx);
    });
    cx.run_until_parked();

    let before = editor.read_with(cx, |editor, _cx| editor.reference_registry_rebuilds.get());
    editor.update(cx, |editor, cx| editor.undo_document(cx));
    cx.run_until_parked();
    let after = editor.read_with(cx, |editor, _cx| editor.reference_registry_rebuilds.get());
    assert_eq!(
        after, before,
        "撤销标题里的改动重建了 {} 遍整篇引用注册表，这次改动没碰定义行",
        after - before
    );

    // 正控：改定义行本身。
    let definition = editor.read_with(cx, |editor, cx| {
        editor
            .document
            .root_blocks()
            .iter()
            .find(|root| {
                root.read(cx)
                    .record
                    .title_markdown()
                    .contains("[asset0]:")
            })
            .cloned()
            .expect("夹具里应有定义行")
    });
    definition.update(cx, |block, cx| {
        block.replace_text_in_visible_range(0..0, "x", None, false, cx);
    });
    cx.run_until_parked();
    let before_definition_undo =
        editor.read_with(cx, |editor, _cx| editor.reference_registry_rebuilds.get());
    editor.update(cx, |editor, cx| editor.undo_document(cx));
    cx.run_until_parked();
    let after_definition_undo =
        editor.read_with(cx, |editor, _cx| editor.reference_registry_rebuilds.get());
    assert!(
        after_definition_undo > before_definition_undo,
        "撤销定义行上的改动没重建引用注册表"
    );
}

/// 打字不许因为段落里有 `[` 或 `<` 就整篇重建引用注册表。
///
/// 定义行必含 `]:`、脚注引用必含 `[^`，屏幕收窄到这两条之后，一个带
/// `[文字](链接)` 的段落（或一句 `a < b`）每键不再付一次整篇注册表重建
/// （10 MiB 档一次 766ms）。正控保证收窄没把该动的漏掉：改定义行本身必须重建。
#[gpui::test]
async fn typing_a_link_paragraph_does_not_rebuild_the_reference_registry(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let markdown = "段落里有 [文字](https://example.com) 与 a < b。\n\n[asset]: ./assets/a.png\n\n![图][asset]\n";
    let editor = cx.update(|cx| cx.new(|cx| Editor::from_markdown(cx, markdown.to_string(), None)));
    cx.run_until_parked();

    let blocks = editor.read_with(cx, |editor, _cx| editor.document.root_blocks().to_vec());
    assert!(
        blocks.len() >= 3,
        "夹具应有段落、定义行、图片三块，实际 {}",
        blocks.len()
    );
    let paragraph = blocks[0].clone();
    let definition = blocks[1].clone();

    let before = editor.read_with(cx, |editor, _cx| editor.reference_registry_rebuilds.get());
    assert!(before > 0, "建文档时就没重建过注册表，这条闸门量不到东西");

    paragraph.update(cx, |block, cx| {
        block.replace_text_in_visible_range(0..0, "x", None, false, cx);
    });
    cx.run_until_parked();
    let after_paragraph = editor.read_with(cx, |editor, _cx| {
        editor.reference_registry_rebuilds.get()
    });
    assert_eq!(
        after_paragraph, before,
        "带链接的段落里打字触发了整篇引用注册表重建"
    );

    // 正控：定义行本身改了必须重建（收窄不许把这一条漏掉）。
    definition.update(cx, |block, cx| {
        block.replace_text_in_visible_range(0..0, "y", None, false, cx);
    });
    cx.run_until_parked();
    let after_definition = editor.read_with(cx, |editor, _cx| {
        editor.reference_registry_rebuilds.get()
    });
    assert!(
        after_definition > after_paragraph,
        "改了定义行却没重建引用注册表"
    );
}

/// 把定义行**引入**文档的那一次编辑也必须重建引用注册表。
///
/// 屏幕（`block_may_change_document_references`）读的是块在缓冲区里的源码区间，
/// 而这一笔要到 `on_block_changed` 后半段才落笔——所以它看到的是改动**之前**的
/// 字节。原来没有 `]:`、这一下才写进去的编辑（粘贴一行定义、把定义从别处搬过来、
/// 打完整定义的最后那一键）在「只看缓冲区」的口径下看不见，注册表就停在旧的定义
/// 集上：引用式图片指着旧地址或压根解不出来。这条闸门钉住「落笔前后两份文本都要看」。
#[gpui::test]
async fn introducing_a_reference_definition_refreshes_the_reference_registry(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    let markdown = "第一段文字\n\n![图][asset]\n";
    let editor = cx.update(|cx| cx.new(|cx| Editor::from_markdown(cx, markdown.to_string(), None)));
    cx.run_until_parked();

    let blocks = editor.read_with(cx, |editor, _cx| editor.document.root_blocks().to_vec());
    assert!(blocks.len() >= 2, "夹具应有段落与图片两块，实际 {}", blocks.len());
    let paragraph = blocks[0].clone();
    let image = blocks[1].clone();

    let before = editor.read_with(cx, |editor, _cx| editor.reference_registry_rebuilds.get());
    assert!(before > 0, "建文档时就没重建过注册表，这条闸门量不到东西");

    let end = paragraph.read_with(cx, |paragraph, _cx| paragraph.visible_len());
    paragraph.update(cx, |block, cx| {
        block.replace_text_in_visible_range(0..end, "[asset]: ./assets/a.png", None, false, cx);
    });
    cx.run_until_parked();

    let after = editor.read_with(cx, |editor, _cx| editor.reference_registry_rebuilds.get());
    assert!(
        after > before,
        "这一笔把定义行写进了段落，引用注册表却没重建"
    );
    assert_eq!(
        image.read_with(cx, |image, _cx| image
            .image_runtime()
            .map(|runtime| runtime.src.clone())),
        Some("./assets/a.png".into()),
        "定义行已经写进文档，引用式图片却还指着旧地址"
    );
}

/// 没有源码区间的脚注定义块改标签，也要重建注册表。
///
/// 屏幕在块**没有可信源码区间**时退回行内树那份宽口径。收窄（`596be0e`）把宽口径里的
/// 「块本身是定义类（`FootnoteDefinition` / `Table` / `HtmlBlock` / `RawMarkdown` /
/// `FrontMatter`）」那一档漏掉了：容器里的脚注定义是**子块**（没有区间），它的行内树
/// 标题就是标签本身（`display_text() == "note"`），既没有 `]:` 也没有 `[^`——于是改标签
/// 那一笔答 false，`footnote_registry` 的键停在旧标签上：`[^note]` 还指着改了名的定义，
/// `[^other]` 解析不出来。
#[gpui::test]
async fn renaming_a_footnote_definition_sub_block_refreshes_the_registry(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    let markdown = "Root reference.[^note]\n\n> [^note]: Nested quote footnote";
    let editor = cx.update(|cx| cx.new(|cx| Editor::from_markdown(cx, markdown.to_string(), None)));
    cx.run_until_parked();

    let definition = editor.read_with(cx, |editor, cx| {
        editor
            .document
            .visible_blocks()
            .iter()
            .find(|visible| visible.entity.read(cx).kind() == BlockKind::FootnoteDefinition)
            .expect("引用容器里应有脚注定义子块")
            .entity
            .clone()
    });
    let before = editor.read_with(cx, |editor, _cx| editor.reference_registry_rebuilds.get());

    definition.update(cx, |definition, cx| {
        definition
            .record
            .set_title(crate::components::InlineTextTree::plain("other"));
        definition.sync_render_cache();
        cx.emit(BlockEvent::Changed);
    });
    cx.run_until_parked();

    let (after, has_new, has_old) = editor.read_with(cx, |editor, _cx| {
        (
            editor.reference_registry_rebuilds.get(),
            editor.footnote_registry.bindings.contains_key("other"),
            editor.footnote_registry.bindings.contains_key("note"),
        )
    });
    assert!(
        after > before,
        "改了脚注定义的标签却没重建引用注册表（键会停在旧标签上）"
    );
    assert!(has_new, "新标签没进脚注注册表");
    assert!(!has_old, "旧标签还留在脚注注册表里");
}

/// 表格格子里打字不许每键整篇重建引用注册表。
///
/// 格子没有源码区间（区间挂在表根块上），屏幕于是退回行内树的宽口径：格子里有
/// `[文字](链接)` 或 `a < b`，每键就付一次整篇注册表重建（10 MiB 档一次 766ms）。
/// 而定义行不可能长在表格行里（解析器只认行首的 `[label]:`，表格行首是 `|`），
/// 格子这一格只可能改脚注引用表：`[^label]` 决定「哪几块引用了这个脚注」。
#[gpui::test]
async fn typing_in_a_table_cell_does_not_rebuild_the_reference_registry(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    let markdown = "| 列 | 值 |\n| --- | --- |\n| [文字](https://example.com) | a < b |\n";
    let editor = cx.update(|cx| cx.new(|cx| Editor::from_markdown(cx, markdown.to_string(), None)));
    cx.run_until_parked();

    let cell = editor.read_with(cx, |editor, cx| {
        editor
            .table_cells
            .values()
            .map(|binding| binding.cell.clone())
            .find(|cell| {
                cell.read(cx)
                    .record
                    .title
                    .fragments
                    .iter()
                    .any(|fragment| fragment.link.is_some())
            })
            .expect("夹具应有一个带链接的格子")
    });
    let before = editor.read_with(cx, |editor, _cx| editor.reference_registry_rebuilds.get());
    assert!(before > 0, "建文档时就没重建过注册表，这条闸门量不到东西");

    cell.update(cx, |cell, cx| {
        cell.replace_text_in_visible_range(0..0, "x", None, false, cx);
    });
    cx.run_until_parked();
    let after = editor.read_with(cx, |editor, _cx| editor.reference_registry_rebuilds.get());
    assert_eq!(after, before, "格子里打字触发了整篇引用注册表重建");

    // 正控：格子里新引入脚注引用（`[^`）必须重建——它改的是「哪几块引用了这个脚注」。
    cell.update(cx, |cell, cx| {
        cell.replace_text_in_visible_range(0..0, "[^note]", None, false, cx);
    });
    cx.run_until_parked();
    let after_footnote = editor.read_with(cx, |editor, _cx| editor.reference_registry_rebuilds.get());
    assert!(
        after_footnote > after,
        "格子里引入脚注引用却没重建引用注册表"
    );
}

/// 一次 ⌘Z 不许再做一遍整篇落笔。
///
/// 撤销先重放逆操作、再 `rebuild_document_from_buffer`：块树整篇来自缓冲区，缓冲区就是
/// 唯一事实源。`replay_history_group` 为此置了 `skip_next_resync`，于是收尾那次
/// `mark_dirty` 不该再走「树 → 缓冲区」的整篇序列化（10 MiB 档那一位值 17.9 秒）。
/// 这条闸门钉住那面跳过标志——有人把它挪掉，撤销就会多付一整篇序列化加逐根块重挂。
#[gpui::test]
async fn undo_does_not_fall_back_to_a_whole_document_resync(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let markdown = (0..400)
        .map(|index| format!("## 第 {index} 节\n\n第 {index} 段正文。\n"))
        .collect::<String>();
    let (editor, cx) = cx.add_window_view(|_window, cx| Editor::from_markdown(cx, markdown, None));
    redraw(cx);
    for _ in 0..3 {
        cx.simulate_input("x");
        redraw(cx);
    }

    let before = whole_document_passes(&editor, cx);
    let revision_before = editor.read_with(cx, |editor, _cx| editor.document_revision);
    editor.update(cx, |editor, cx| editor.undo_document(cx));
    redraw(cx);
    let after = whole_document_passes(&editor, cx);
    let revision_after = editor.read_with(cx, |editor, _cx| editor.document_revision);
    assert!(
        revision_after > revision_before,
        "撤销没落进编辑器，这条闸门测错了东西"
    );
    assert_eq!(
        after.source_serializations - before.source_serializations
            + (after.whole_document_renders - before.whole_document_renders),
        0,
        "一次 ⌘Z 做了整篇兜底重投影"
    );
}

/// 稳态帧不许克隆整篇根块列表。
///
/// 侧栏打开时 `sync_workspace_outline` 每帧跑一次，而它的短路判断只需要根块数：
/// 原来那行 `root_blocks().to_vec()` 写在判断**之前**，于是每帧克隆一次全部根块
/// （克隆一根要付一次 Entity 引用计数与 LeakDetector 登记，10 MiB 档 106,456 根，
/// 独立评审实测 3.7–4.7ms/帧）。
#[gpui::test]
async fn steady_frames_do_not_clone_the_outline_root_blocks(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let source = (0..300)
        .map(|index| format!("# 标题 {index}\n\n正文 {index}\n\n"))
        .collect::<String>();
    let (editor, cx) = cx.add_window_view(|_window, cx| Editor::from_markdown(cx, source, None));
    redraw(cx);
    editor.update(cx, |editor, _cx| {
        editor.workspace.is_open = true;
        editor.workspace.active_tab = crate::editor::workspace::WorkspaceTab::Outline;
    });
    redraw(cx);
    redraw(cx);

    // 先证明计数器是活的：拆一次块会改根块数，大纲必须重扫，克隆要留下数。
    let before = editor.read_with(cx, |editor, _cx| editor.outline_root_blocks_cloned.get());
    let revision_before = editor.read_with(cx, |editor, _cx| editor.document_revision);
    cx.simulate_input("\n");
    redraw(cx);
    redraw(cx);
    let revision_after = editor.read_with(cx, |editor, _cx| editor.document_revision);
    assert!(
        revision_after > revision_before,
        "回车没落进编辑器，这条闸门测错了东西"
    );
    let after_edit = editor.read_with(cx, |editor, _cx| editor.outline_root_blocks_cloned.get());
    assert!(
        after_edit > before,
        "大纲重扫时没克隆根块列表，计数器是死的：{before} -> {after_edit}"
    );

    // 闸门本体：静止帧一根都不许克隆。
    let steady = editor.read_with(cx, |editor, _cx| editor.outline_root_blocks_cloned.get());
    for _ in 0..3 {
        redraw(cx);
    }
    let after_idle = editor.read_with(cx, |editor, _cx| editor.outline_root_blocks_cloned.get());
    assert_eq!(
        after_idle,
        steady,
        "静止帧克隆了 {} 根根块",
        after_idle - steady
    );
}

/// 状态栏找选区文本不许扫全篇，也不许报出别的块上的残影选区。
///
/// 单块选区只可能在活动编辑目标那一块上（它是唯一持有焦点的块）。旧实现从文档第一块
/// 起逐块找「第一个非空选区」：没有选区时（打字常态）一帧读遍全篇；文档前部留着上次
/// 交互的残影时，报给状态栏的又是那一块的字。
#[gpui::test]
async fn status_bar_finds_the_selection_on_the_active_block_only(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let source = (0..300)
        .map(|index| format!("## 第 {index} 节\n\n第 {index} 段正文。\n"))
        .collect::<String>();
    let (editor, cx) = cx.add_window_view(|_window, cx| Editor::from_markdown(cx, source, None));
    redraw(cx);

    let (first_id, active_id) = editor.read_with(cx, |editor, _cx| {
        let visible = editor.document.visible_blocks();
        (
            visible[0].entity.entity_id(),
            visible[150].entity.entity_id(),
        )
    });
    editor.update(cx, |editor, cx| {
        let active = editor.document.block_entity_by_id(active_id).expect("活动块");
        active.update(cx, |block, cx| {
            block.selected_range = 0..0;
            cx.notify();
        });
        editor.active_entity_id = Some(active_id);
        editor.pending_focus = None;
    });

    // 没有选区：一个块实体都不该读。
    let before = editor.read_with(cx, |editor, _cx| editor.status_bar_selection_block_reads.get());
    let text = editor.read_with(cx, |editor, cx| editor.selected_visible_text(cx));
    let after = editor.read_with(cx, |editor, _cx| editor.status_bar_selection_block_reads.get());
    assert_eq!(text, None, "没有选区却报出了选区文本：{text:?}");
    assert_eq!(
        after,
        before,
        "没有选区时状态栏按可见顺序读了 {} 个块",
        after - before
    );

    // 正控：活动块自己真有选区时必须报出来，而且仍然不扫别的块。
    editor.update(cx, |editor, cx| {
        let active = editor.document.block_entity_by_id(active_id).expect("活动块");
        active.update(cx, |block, cx| {
            block.selected_range = 0..3;
            cx.notify();
        });
    });
    let before = editor.read_with(cx, |editor, _cx| editor.status_bar_selection_block_reads.get());
    let text = editor.read_with(cx, |editor, cx| editor.selected_visible_text(cx));
    let after = editor.read_with(cx, |editor, _cx| editor.status_bar_selection_block_reads.get());
    assert!(
        text.as_deref().is_some_and(|selected| !selected.is_empty()),
        "活动块上有选区却没报出来：{text:?}"
    );
    assert_eq!(after, before, "活动块有选区时还是扫了别的块");

    // 残影：文档第一块留着旧选区，活动块光标折叠——报出来的必须是「没有选区」。
    editor.update(cx, |editor, cx| {
        let residue = editor.document.block_entity_by_id(first_id).expect("残影块");
        residue.update(cx, |block, cx| {
            block.selected_range = 0..3;
            cx.notify();
        });
        let active = editor.document.block_entity_by_id(active_id).expect("活动块");
        active.update(cx, |block, cx| {
            block.selected_range = 0..0;
            cx.notify();
        });
    });
    let text = editor.read_with(cx, |editor, cx| editor.selected_visible_text(cx));
    assert_eq!(
        text, None,
        "折叠光标下报出了别的块上的残影选区：{text:?}"
    );
}

/// 侧栏大纲的渲染闸门：3,000 条标题全展开时，一帧只许把视口里那几十行建成元素。
/// 10 MiB 代码文档实测 53,227 条大纲，旧实现每帧把整棵树走成元素树（打开大纲页签
/// 后一次按键 2.3–2.6s 里的大头，其他线性成本加起来不到 300ms）——大纲面板和正文
/// 一样要按视口开窗。
#[gpui::test]
async fn outline_panel_renders_only_the_rows_in_the_viewport(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let source = (0..3000)
        .map(|index| format!("# 标题 {index}\n\n正文 {index}\n\n"))
        .collect::<String>();
    let (editor, cx) = cx.add_window_view(|_window, cx| Editor::from_markdown(cx, source, None));
    redraw(cx);
    editor.update(cx, |editor, _cx| {
        editor.workspace.is_open = true;
        editor.workspace.active_tab = crate::editor::workspace::WorkspaceTab::Outline;
    });
    redraw(cx);
    redraw(cx);
    let (rows, headings) = editor.read_with(cx, |editor, _cx| {
        (
            editor.panel_rows_rendered.get(),
            editor.workspace.outline_tree.len(),
        )
    });
    assert_eq!(headings, 3000, "前置：大纲树应有 3000 条标题");
    assert!(rows > 0, "大纲面板一帧都没渲染，闸门测不到东西");
    assert!(
        rows <= 200,
        "一帧渲染了 {rows} 行大纲（共 {headings} 条）：侧栏没有按视口裁剪"
    );

    // 滚动之后窗口要跟着走：滚到第 1000 行附近，那一行得落在窗口里。
    // 偏移用**负值**：gpui 里内容偏移是「已滚过距离的相反数」，向下滚只会是
    // `[-max, 0]`（`workspace/tests/sidebar.rs` 用真滚轮断言过 `offset < 0`）。
    // 注入正值是物理上不可能的状态，窗口算错也照样绿。
    editor.update(cx, |editor, _cx| {
        editor.workspace.tree_scroll_handle.set_offset(gpui::point(
            gpui::px(0.0),
            gpui::px(-(6.0 + 1000.0 * 24.0)),
        ));
    });
    redraw(cx);
    let (first, rows) = editor.read_with(cx, |editor, _cx| {
        (
            editor.panel_first_row_rendered.get(),
            editor.panel_rows_rendered.get(),
        )
    });
    assert!(
        (first as usize) <= 1000 && 1000 < first as usize + rows as usize,
        "滚到第 1000 行后窗口是 {first}..{}：窗口没跟着滚动走",
        first as usize + rows as usize
    );
}

/// 在标题里打字是大纲最常见的改动：文字换了，层级与行号一个没动。
/// 旧实现把 5.3 万条标题重新拼成一棵树（10 MiB 实测 130ms/键）；这类改动
/// 应该原地改标签，不重拼树。
#[gpui::test]
async fn typing_inside_a_heading_updates_the_outline_in_place(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let markdown = "# 甲\n\n正文一\n\n## 甲二\n\n# 乙\n\n正文二\n".to_string();
    let (editor, cx) = cx.add_window_view(|_window, cx| Editor::from_markdown(cx, markdown, None));
    redraw(cx);
    editor.update(cx, |editor, cx| {
        editor.workspace.is_open = true;
        editor.workspace.active_tab = crate::editor::workspace::WorkspaceTab::Outline;
        let first = editor.document.root_blocks()[0].clone();
        editor.focus_block(first.entity_id());
        first.update(cx, |block, block_cx| block.move_to(0, block_cx));
    });
    redraw(cx);
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(editor.workspace.toc_entries.len(), 3, "前置：三个标题");
    });

    let before = editor.read_with(cx, |editor, _| editor.outline_rebuilds.get());
    cx.simulate_input("新");
    redraw(cx);
    let after = editor.read_with(cx, |editor, _| editor.outline_rebuilds.get());
    assert_eq!(
        after - before,
        0,
        "在标题里打一个字重拼了整棵大纲树（层级与行号都没动）"
    );
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(
            editor.workspace.outline_tree[0].label, "新甲",
            "树上那个节点的标签没跟着换，面板会显示旧标题"
        );
        assert_eq!(
            editor.workspace.toc_entries[0].title, "新甲",
            "`[TOC]` 读的条目没跟着换"
        );
    });
}

/// 行计划的缓存键不该是「文档修订」：打字只改块内文字，行元数据（层级、分组
/// 锚点、目录标记）一个没动时，16 万行的计划不该重排（10 MiB 实测一次重排
/// ~42ms、每键两次）。修的是键——换成一个只在行元数据真变时才动的版本。
#[gpui::test]
async fn typing_a_plain_character_does_not_rebuild_the_row_plan(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let markdown = (0..300)
        .map(|index| format!("第 {index} 段正文。\n\n"))
        .collect::<String>();
    let (editor, cx) = cx.add_window_view(|_window, cx| Editor::from_markdown(cx, markdown, None));
    redraw(cx);
    editor.update(cx, |editor, cx| {
        let first = editor.document.root_blocks()[0].clone();
        editor.focus_block(first.entity_id());
        first.update(cx, |block, block_cx| block.move_to(0, block_cx));
    });
    redraw(cx);

    let before = editor.read_with(cx, |editor, _| editor.row_plan_rebuilds.get());
    cx.simulate_input("字");
    redraw(cx);
    let after = editor.read_with(cx, |editor, _| editor.row_plan_rebuilds.get());
    assert_eq!(
        after, before,
        "行元数据没变，打一个字却重排了整篇行计划"
    );

    // 结构变了（回车拆块）就必须重排，别让闸门被「永远不重排」蒙过去。
    cx.dispatch_action(Newline);
    redraw(cx);
    let after_newline = editor.read_with(cx, |editor, _| editor.row_plan_rebuilds.get());
    assert!(
        after_newline > after,
        "拆块之后行计划没重排（可见列表变了）"
    );
}

/// 没有 `[TOC]` 块时，在标题里打字也不该重排行计划：大纲原地换标签会
/// 推进 `toc_state_version`（那是为了让行计划的折叠过滤把新条目推给 `[TOC]`
/// 块），正文里没人看目录时这一推进只是白白下架整张计划。侧栏开着（大纲是
/// 读者）但没有 `[TOC]` 块，就是这一档。
#[gpui::test]
async fn typing_in_a_heading_without_a_toc_block_reuses_the_row_plan(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let markdown = "# 甲\n\n正文一\n\n# 乙\n\n正文二\n".to_string();
    let (editor, cx) = cx.add_window_view(|_window, cx| Editor::from_markdown(cx, markdown, None));
    redraw(cx);
    editor.update(cx, |editor, cx| {
        editor.workspace.is_open = true;
        editor.workspace.active_tab = crate::editor::workspace::WorkspaceTab::Outline;
        let first = editor.document.root_blocks()[0].clone();
        editor.focus_block(first.entity_id());
        first.update(cx, |block, block_cx| block.move_to(0, block_cx));
    });
    redraw(cx);
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(editor.workspace.toc_entries.len(), 2, "前置：大纲是读者");
    });

    let before = editor.read_with(cx, |editor, _| editor.row_plan_rebuilds.get());
    cx.simulate_input("新");
    redraw(cx);
    let after = editor.read_with(cx, |editor, _| editor.row_plan_rebuilds.get());
    assert_eq!(
        after, before,
        "没有 `[TOC]` 块，在标题里打一个字却重排了整篇行计划"
    );
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(editor.workspace.toc_entries[0].title, "新甲", "大纲条目没跟上");
    });
}

/// 段首打 `# ` 是「同一个实体就地换 kind」：行计划的行元数据必须跟着刷新
/// （heading_level 等），否则行距停在段落档、错到下一次结构变化为止。
#[gpui::test]
async fn typing_a_heading_prefix_updates_the_row_metadata(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "正文一段。\n".to_string(), None));
    redraw(cx);
    editor.update(cx, |editor, cx| {
        let first = editor.document.root_blocks()[0].clone();
        editor.focus_block(first.entity_id());
        first.update(cx, |block, block_cx| block.move_to(0, block_cx));
    });
    redraw(cx);

    // 在段首敲 `# `：实体没换、可见列表没变，但块的 kind 变成了标题。
    cx.simulate_input("# ");
    redraw(cx);

    editor.read_with(cx, |editor, _cx| {
        assert!(
            editor
                .document
                .root_blocks()[0]
                .read(_cx)
                .kind()
                == crate::components::BlockKind::Heading { level: 1 },
            "前置：段首 # 加空格应把段落变成一级标题"
        );
        let spacing = editor.document.row_spacing_at(0);
        assert_eq!(
            spacing.heading_level,
            Some(1),
            "行元数据没跟上 kind 变化：行距会停在段落档"
        );
    });
}


/// roadmap G8 的墙钟预算（自 import_perf.rs 集中于此）：10 MiB 文档打开只建
/// 首块（3 s 预算罩住的就是首块），其余在窗口可交互时后台续建。相对判据
/// （打开 < 整篇/3）与逐块 µs 预算在并发跑测下也不抖；夹具缺失即自跳。
#[gpui::test]
#[ignore = "墙钟预算闸门，依赖机器速度；单独跑：cargo test --bin velora -- --ignored"]
async fn large_document_opens_within_budget(cx: &mut TestAppContext) {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/perf/ten-mib.md");
    if !fixture.is_file() {
        eprintln!("skipping: generate fixtures with `node scripts/generate-fixtures.mjs fixtures/perf`");
        return;
    }
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
    });
    let markdown = std::fs::read_to_string(&fixture).expect("read fixture");
    let bytes = markdown.len();
    assert!(bytes >= 10 * 1024 * 1024, "fixture should be ~10 MiB");

    let start = std::time::Instant::now();
    let editor = cx.update(|cx| {
        cx.new(|cx| Editor::from_markdown(cx, markdown.clone(), None))
    });
    let open_elapsed = start.elapsed();
    eprintln!("G8: 10 MiB open(first chunk): {open_elapsed:?}");

    let (first_blocks, pending, source_len) = cx.read(|cx| {
        editor.read_with(cx, |editor, cx| {
            (
                editor.document.visible_blocks().len(),
                editor.document.pending_tail().is_some(),
                editor.document.markdown_text(cx).len(),
            )
        })
    });
    assert!(first_blocks > 0);
    assert!(pending, "超大文档应先只建首块，其余挂起");
    assert!(source_len >= 9 * 1024 * 1024, "未建完时序列化丢内容: {source_len}");
    assert!(
        first_blocks <= 4_000,
        "打开时应只建首块（上限 4000），实测 {first_blocks} 块"
    );

    let deadline = Instant::now() + Duration::from_secs(180);
    while cx.read(|cx| editor.read_with(cx, |editor, _cx| editor.document.pending_tail().is_some()))
    {
        assert!(Instant::now() < deadline, "续建未在预算时间内完成");
        cx.run_until_parked();
    }

    let (blocks, text_len, text) = cx.read(|cx| {
        editor.read_with(cx, |editor, cx| {
            (
                editor.document.visible_blocks().len(),
                editor.document.markdown_text(cx).len(),
                editor.document.markdown_text(cx),
            )
        })
    });
    let total_elapsed = start.elapsed();
    eprintln!("G8: 续建完成 {total_elapsed:?}，{blocks} 块，文本 {text_len} 字节");
    assert!(
        blocks > first_blocks * 10,
        "续建后块数未增长: {first_blocks} -> {blocks}"
    );
    // 打开只付首块的钱：打开耗时必须显著小于整篇建块成本。相对判据在并发
    // 跑测下稳定，但打开的固定开销会随机器状态漂移，取 1/3 仍能抓住
    // 「打开付整篇的钱」的失效模式（比值≈1）。
    assert!(
        open_elapsed * 3 < total_elapsed,
        "打开 {open_elapsed:?} 与整篇建块 {total_elapsed:?} 不成比例：打开可能又付了整篇的钱"
    );
    assert!(text_len >= 9 * 1024 * 1024, "续建后文本仍不完整: {text_len}");

    let single_pass = cx.update(|cx| {
        cx.new(|cx| {
            Editor::from_markdown_with_chunk_budget(cx, markdown.clone(), None, usize::MAX)
        })
    });
    let expected = cx.read(|cx| {
        single_pass.read_with(cx, |editor, cx| {
            assert!(editor.document.pending_tail().is_none());
            editor.document.markdown_text(cx)
        })
    });
    assert_eq!(text, expected, "分块导入与整篇导入结果不一致");

    // 防回归：首块 + 续建的总成本仍应在每块预算内（400 µs 容忍并发抢 CPU，
    // 抓的是 2 倍以上的灾难性退步）。
    let per_block_us = total_elapsed.as_micros() as f64 / blocks.max(1) as f64;
    eprintln!("G8: {per_block_us:.1} µs/block（首块 + 续建，debug）");
    assert!(
        per_block_us <= 400.0,
        "per-block open cost regressed: {per_block_us:.1} µs > 400 µs budget"
    );
}

/// 状态栏选词的墙钟预算（自 selection_mouse.rs 集中于此）：600 块文档上
/// `selected_visible_text` 单次不许退回 O(整篇)（旧实现实测 38ms/次）。
#[gpui::test]
#[ignore = "墙钟预算闸门，依赖机器速度；单独跑：cargo test --bin velora -- --ignored"]
async fn selection_word_count_stays_cheap_on_a_long_document(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let markdown = (0..300)
        .map(|index| {
            format!(
                "## 第 {index} 节标题\n\n这是第 {index} 段中文正文，足够长以便换行，含标点与英文 mixed text。\n"
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let (editor, cx) = cx.add_window_view(|_window, cx| Editor::from_markdown(cx, markdown, None));
    redraw(cx);

    editor.update(cx, |editor, cx| {
        let visible = editor.document.visible_blocks().to_vec();
        let first = visible[0].entity.entity_id();
        let last = visible[visible.len() - 1].entity.entity_id();
        editor.cross_block_selection = Some(crate::editor::CrossBlockSelection {
            anchor: crate::editor::CrossBlockSelectionEndpoint {
                entity_id: first,
                offset: 0,
            },
            focus: crate::editor::CrossBlockSelectionEndpoint {
                entity_id: last,
                offset: usize::MAX,
            },
        });

        let text = editor.selected_visible_text(cx).expect("selection text");
        assert!(text.contains("第 0 节标题"), "选中文本应包含首块内容");

        let calls = 20;
        let start = Instant::now();
        for _ in 0..calls {
            let _ = editor.selected_visible_text(cx);
        }
        let per_call = start.elapsed() / calls;
        println!(
            "[measure] selected_visible_text 单次 = {per_call:?}（可见块 {} 个）",
            visible.len()
        );
        assert!(
            per_call < Duration::from_millis(5),
            "状态栏选词路径又变回 O(整篇) 了：{per_call:?}（可见块 {} 个）",
            visible.len()
        );
    });
}

/// P7 预算守卫（自 loading_chunks.rs 集中于此）：1 MiB 级代码文档同步
/// 构造必须在预算内（当前 dev 实测 ~50ms，给 10x 余量），流式续建完成
/// 后序列化必须逐字节还原。
#[gpui::test]
#[ignore = "墙钟预算闸门，依赖机器速度；单独跑：cargo test --bin velora -- --ignored"]
async fn large_code_document_opens_within_budget(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let mut source = String::with_capacity(1 << 20);
    for index in 0..9_891 {
        source.push_str(&format!(
            "2026-09-28T12:00:00.000Z INFO  [mod{}::sub] request id={} duration={}ms status=OK\n",
            index % 7,
            index,
            index % 97
        ));
    }
    assert!(source.len() > 800_000 && source.len() < 1_100_000);
    let path = temp_fixture_dir().join(format!("velora-budget-{}.log", temp_fixture_token()));
    fs::write(&path, &source).expect("write budget fixture");
    let expected_source = source.clone();

    let start = Instant::now();
    let editor =
        cx.update(|cx| crate::app_menu::open_editor_window(cx, source.clone(), Some(path)));
    let open_elapsed = start.elapsed();
    assert!(
        open_elapsed.as_millis() < 500,
        "1MiB 代码文档打开耗时 {}ms，超出 500ms 预算",
        open_elapsed.as_millis()
    );
    cx.run_until_parked();

    editor
        .read_with(cx, |editor, cx| {
            assert!(matches!(editor.view_mode, ViewMode::Source));
            let blocks = editor.document.visible_blocks().len();
            assert!(
                (17..=22).contains(&blocks),
                "1MiB 日志应切成约 20 块，实际 {}",
                blocks
            );
            assert_eq!(
                editor.current_document_source(cx),
                expected_source,
                "流式续建完成后必须逐字节还原"
            );
        })
        .expect("editor window should be open");
}

/// 引用容器里的无源码区间 Raw 块打字，不许按「定义类宽口径」无条件整篇重建
/// 引用注册表。Raw/HTML/FrontMatter 的内容就是缓冲区字节：复用与有区间块同一把
/// 尺（`contains_reference_definition_marker`，定义必含 `]:`、脚注引用必含 `[^`）
/// 扫块自身文本即可，10 MiB 档一次 766ms 的重建不该随打字付。
#[gpui::test]
async fn typing_in_a_container_raw_block_does_not_rebuild_the_reference_registry(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    let markdown = "> Quote with broken table:\n> | A |\n> | --- | --- |\n> | 1 |";
    let editor = cx.update(|cx| cx.new(|cx| Editor::from_markdown(cx, markdown.to_string(), None)));
    cx.run_until_parked();

    let raw = editor.read_with(cx, |editor, cx| {
        editor
            .document
            .visible_blocks()
            .iter()
            .find(|visible| visible.entity.read(cx).kind() == BlockKind::RawMarkdown)
            .expect("夹具该有引用容器内的 RawMarkdown 子块")
            .entity
            .clone()
    });
    assert!(
        editor
            .read_with(cx, |editor, _cx| editor.document.source_span_of(raw.entity_id()).is_none()),
        "夹具的 Raw 子块带了源码区间，这条闸门量不到宽口径那一档"
    );

    let before = editor.read_with(cx, |editor, _cx| editor.reference_registry_rebuilds.get());
    raw.update(cx, |block, cx| {
        block.replace_text_in_visible_range(0..0, "x", None, false, cx);
    });
    cx.run_until_parked();
    let after = editor.read_with(cx, |editor, _cx| editor.reference_registry_rebuilds.get());
    assert_eq!(
        after, before,
        "引用容器里的 Raw 块打字触发了整篇引用注册表重建"
    );

    // 正控：Raw 块里真的写进脚注引用记号（`[^`）就必须重建——Raw 的内容就是
    // 缓冲区字节，注册表解析器读得到它。
    let before_marker = after;
    raw.update(cx, |block, cx| {
        block.replace_text_in_visible_range(0..0, "[^n]", None, false, cx);
    });
    cx.run_until_parked();
    let after_marker =
        editor.read_with(cx, |editor, _cx| editor.reference_registry_rebuilds.get());
    assert!(
        after_marker > before_marker,
        "Raw 块引入脚注引用却没重建引用注册表"
    );
}

/// 引用容器内的结构编辑（合并）不许无条件整篇重建引用注册表：`normalize_
/// rendered_quote_structure` 的旧尾部是 `rebuild_table_runtimes`（内部本来就
/// 整篇重建注册表）+ 又一次直接 `rebuild_image_runtimes`——一次合并付两遍。
/// 区域里没有定义记号（`]:`/`[^`）时注册表不可能变，只该把换上来的块接上现有
/// 注册表。
#[gpui::test]
async fn quote_break_without_reference_markers_skips_the_registry_rebuild(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    // 定义行与引用式图片在引用区域外：注册表非空、且与这次合并无关。
    let markdown =
        "![图][asset]\n\n[asset]: ./assets/a.png\n\n> 引用第一行\n> 引用第二行\n";
    let editor = cx.update(|cx| cx.new(|cx| Editor::from_markdown(cx, markdown.to_string(), None)));
    cx.run_until_parked();

    let image_src_before = editor.read_with(cx, |editor, cx| {
        editor
            .document
            .root_blocks()
            .first()
            .and_then(|block| block.read(cx).image_runtime())
            .map(|runtime| runtime.src.clone())
    });
    assert_eq!(
        image_src_before.as_deref(),
        Some("./assets/a.png"),
        "夹具的引用式图片没解开，这条闸门测错了东西"
    );

    let before = editor.read_with(cx, |editor, _cx| editor.reference_registry_rebuilds.get());
    editor.update(cx, |editor, cx| {
        // 拆引用是 normalize_rendered_quote_structure 的直接触发口（合并走的是
        // 子块路径，而这个夹具的整段引用是单块多行标题）。
        let quote = editor
            .document
            .visible_blocks()
            .iter()
            .find(|visible| visible.entity.read(cx).kind() == BlockKind::Quote)
            .map(|visible| visible.entity.clone())
            .expect("夹具该有一个引用块");
        editor.focus_block(quote.entity_id());
        quote.update(cx, |_block, cx| cx.emit(BlockEvent::RequestQuoteBreak));
    });
    cx.run_until_parked();

    let (after, image_src_after) = editor.read_with(cx, |editor, cx| {
        (
            editor.reference_registry_rebuilds.get(),
            editor
                .document
                .root_blocks()
                .first()
                .and_then(|block| block.read(cx).image_runtime())
                .map(|runtime| runtime.src.clone()),
        )
    });
    assert_eq!(
        after, before,
        "引用内合并在区域没有定义记号时也整篇重建了引用注册表"
    );
    assert_eq!(
        image_src_after.as_deref(),
        Some("./assets/a.png"),
        "跳过重建后引用式图片反而解不开了"
    );
    editor.read_with(cx, |editor, cx| {
        // 拆引用行为不变：原引用文本还在，后面多出一个空引用接手。
        let quote_kinds = editor
            .document
            .visible_blocks()
            .iter()
            .filter(|visible| visible.entity.read(cx).quote_depth > 0)
            .map(|visible| {
                (
                    visible.entity.read(cx).kind(),
                    visible.entity.read(cx).display_text().to_string(),
                )
            })
            .collect::<Vec<_>>();
        assert!(
            quote_kinds.iter().any(|(kind, text)| {
                *kind == BlockKind::Quote && text == "引用第一行\n引用第二行"
            }),
            "原引用的内容被拆引用动了：{quote_kinds:?}"
        );
        assert!(
            quote_kinds.len() >= 2,
            "拆引用没在后面补新引用：{quote_kinds:?}"
        );
    });

    // 正控：区域里真的有定义记号（`[^`）时必须照旧重建。
    let markdown_with_marker =
        "![图][asset]\n\n[asset]: ./assets/a.png\n\n> 引用第一行 [^note]\n> 引用第二行\n";
    let editor_marker = cx.update(|cx| {
        cx.new(|cx| Editor::from_markdown(cx, markdown_with_marker.to_string(), None))
    });
    cx.run_until_parked();
    let before_marker = editor_marker
        .read_with(cx, |editor, _cx| editor.reference_registry_rebuilds.get());
    editor_marker.update(cx, |editor, cx| {
        let quote = editor
            .document
            .visible_blocks()
            .iter()
            .find(|visible| visible.entity.read(cx).kind() == BlockKind::Quote)
            .map(|visible| visible.entity.clone())
            .expect("夹具该有一个引用块");
        editor.focus_block(quote.entity_id());
        quote.update(cx, |_block, cx| cx.emit(BlockEvent::RequestQuoteBreak));
    });
    cx.run_until_parked();
    let after_marker = editor_marker
        .read_with(cx, |editor, _cx| editor.reference_registry_rebuilds.get());
    assert!(
        after_marker > before_marker,
        "区域里带定义记号的合并没有重建引用注册表"
    );
}

/// 公式块里 `$$` 前后的文字段每帧都会重走一遍行内 Markdown 解析；源码不变
/// 时结果恒同。缓存按源码串建好之后，静止帧的解析次数必须是 0。
#[gpui::test]
async fn math_blocks_do_not_reparse_text_segments_on_every_frame(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "$$x^2$$ 说明文字\n\n正文\n".into(), None)
    });
    // 让公式块失焦：阅读态才走分段渲染。
    editor.update(cx, |editor, _cx| {
        let body = editor.document.root_blocks()[1].clone();
        editor.focus_block(body.entity_id());
    });
    redraw(cx);
    redraw(cx);
    let before = crate::components::math_text_parse_count();
    redraw(cx);
    let after = crate::components::math_text_parse_count();
    assert_eq!(
        after - before,
        0,
        "静止帧重复解析了公式块里的文字段"
    );
}
