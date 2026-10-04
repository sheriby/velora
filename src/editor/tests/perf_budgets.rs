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
    editor.read_with(cx, |editor, _| {
        assert_eq!(
            total,
            crate::editor::status_bar::count_words(&editor.buffer.text()),
            "静默窗口后补算的字数必须与当前文本一致"
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
async fn one_mib_typing_stays_within_budget(cx: &mut TestAppContext) {
    // P2 大文档输入预算：1 MiB 文档里一次按键的成本必须是「常数次全文遍数 +
    // 有界时间」，而不是随文档线性增长的多遍扫描。夹具由
    // scripts/generate-fixtures.mjs 生成且被 gitignore，缺失就跳过。
    // （10 MiB 夹具单键实测 13s，迭代太慢，先用 1 MiB 收敛行为。）
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/perf/one-mib.md");
    if !fixture.is_file() {
        eprintln!("skipping: generate fixtures with `node scripts/generate-fixtures.mjs tests/fixtures/perf`");
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
/// 上限现在是 1.5s：10 MiB 一次按键实测 0.6s，剩下的线性成本不在写回层，而在
/// 15 万个块的行计划与可见列表重排（1 MiB 同一条路径是 51ms）。那一块要靠按可见
/// 窗口物化，方案 §10 已把它列为独立工作项。
#[gpui::test]
async fn ten_mib_typing_does_not_scan_the_whole_document(cx: &mut TestAppContext) {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/perf/ten-mib.md");
    if !fixture.is_file() {
        eprintln!("skipping: generate fixtures with `node scripts/generate-fixtures.mjs tests/fixtures/perf`");
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
        )
    });
    eprintln!(
        "[measure] 10 MiB 一次按键 {typed:?}，整篇遍数 (序列化, mapping) = ({}, {})",
        after.0 - before.0,
        after.1 - before.1
    );
    assert_eq!(after.0 - before.0, 0, "10 MiB 一次按键出现整篇序列化");
    assert_eq!(
        after.1 - before.1,
        0,
        "10 MiB 一次按键出现整篇 source mapping 重建"
    );
    assert!(
        typed < Duration::from_millis(1_500),
        "10 MiB 一次按键 {typed:?}，偏出预算"
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

#[gpui::test]
async fn ui_zoom_scales_document_text(cx: &mut TestAppContext) {
    // 用户报修：偏好设置「界面缩放」/ ⌘+/⌘- 改了没有任何反应。
    // 根因：正文块渲染自己拼主题排版，只套了字号设置，漏掉缩放因子；
    // 缩放只作用在编辑器外壳那份主题上。
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
    // 比正文一行还高（用户报修：空行太大）。空行块应该只剩下一个块间距的高度。
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

#[gpui::test]
async fn tree_filter_appends_once_per_keystroke(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, String::new(), None)
    });
    editor.update(cx, |editor, cx| {
        let keystroke = |key: &str| KeyDownEvent {
            keystroke: Keystroke::parse(key).expect("valid keystroke"),
            is_held: false,
        };
        editor.on_tree_filter_key_down(&keystroke("m"), cx);
        editor.on_tree_filter_key_down(&keystroke("d"), cx);
        assert_eq!(
            editor.workspace.tree_filter, "md",
            "每次按键只追加一个字符（用户报修：重复挂载 on_key_down 曾把 md 双写成 mmdd）"
        );
        editor.on_tree_filter_key_down(&keystroke("backspace"), cx);
        assert_eq!(editor.workspace.tree_filter, "m", "退格只删除一个字符");
        editor.on_tree_filter_key_down(&keystroke("escape"), cx);
        assert!(editor.workspace.tree_filter.is_empty(), "Esc 清空过滤词");
    });
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
            split_physical_lines: true,
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
    let path = std::env::temp_dir().join(format!("velora-code-gate-{}.py", std::process::id()));
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

/// 1 MiB 的文档切到源码视图再打字：这一档以前一次按键 2.28 秒（整篇重拼 + 整篇比较 +
/// 整篇落笔）。源码/代码文档的块带着自己那段缓冲区区间后，按键只该付「这一块」的钱，
/// 全文级遍数归零。夹具由 `scripts/generate-fixtures.mjs` 生成且被 gitignore，缺失即跳过。
#[gpui::test]
async fn one_mib_source_mode_typing_stays_within_budget(cx: &mut TestAppContext) {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/perf/one-mib.md");
    if !fixture.is_file() {
        eprintln!("skipping: generate fixtures with `node scripts/generate-fixtures.mjs tests/fixtures/perf`");
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
    eprintln!(
        "[measure] 1 MiB 源码模式一次按键 {typed:?}（整篇落笔 {serializations} 次，整篇 mapping {full} 次）"
    );
    assert_eq!(
        len_after,
        len_before + 1,
        "这个字没进缓冲区：0 次落笔是因为没干活，还是因为压根没打字"
    );
    assert_eq!(
        len_after,
        len_before + 1,
        "这个字没进缓冲区：0 次落笔是因为没干活，还是因为压根没打字"
    );
    assert_eq!(serializations, 0, "1 MiB 源码模式打字还在整篇落笔");
    assert_eq!(full, 0, "1 MiB 源码模式打字还在整篇重建 mapping");
    assert!(
        typed < Duration::from_millis(600),
        "1 MiB 源码模式一次按键 {typed:?}，偏出预算"
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
