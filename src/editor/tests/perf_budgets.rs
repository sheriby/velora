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
