//! 根块必须各自持有一段指向缓冲区的源码区间。
//!
//! 这是「块树只是文本的投影」那条不变式的第一道防护：每个根块的内容必须能从
//! 它自己的区间里读出来，而不是靠序列化反推（那是 `source_mapping.rs` 里七百多行
//! 前缀重建的由来）。分块导入与空行分隔也要保证区间首尾相接、互不重叠，
//! 且**不把块之间的空行算进任何块**——空行不属于任何块，编辑块时才不会碰它。

use super::common::*;
use crate::editor::encoding;

/// 每个根块的（源码区间, 区间里的原文），外加缓冲区全文。
///
/// 用 `unwrap` 而不是 `filter_map`：漏挂区间的块必须让测试炸掉，不能被静默跳过。
pub(super) fn root_block_spans(
    editor: &gpui::Entity<Editor>,
    cx: &mut gpui::VisualTestContext,
) -> (Vec<(std::ops::Range<usize>, String)>, String) {
    editor.read_with(cx, |editor, cx| {
        let spans = editor
            .document
            .root_blocks()
            .iter()
            .map(|block| {
                let span = block
                    .read(cx)
                    .record
                    .source_span
                    .clone()
                    .unwrap_or_else(|| panic!("有块没挂上源码区间"));
                let text = editor.buffer.slice(span.clone());
                (span, text)
            })
            .collect::<Vec<_>>();
        (spans, editor.buffer.text())
    })
}

pub(super) fn span_ranges(spans: &[(std::ops::Range<usize>, String)]) -> Vec<std::ops::Range<usize>> {
    spans.iter().map(|(span, _)| span.clone()).collect()
}

pub(super) fn rendered_blocks(spans: &[(std::ops::Range<usize>, String)]) -> String {
    spans
        .iter()
        .map(|(_, text)| format!("{text:?}"))
        .collect::<Vec<_>>()
        .join(" / ")
}

/// 断言区间递增、互不重叠，且没被任何区间覆盖的字节全是换行符。
///
/// 这条不变式说的是「内容字节 = 各块区间的并」：既不越界吃到邻居，也不漏掉任何
/// 有内容的字节。写回阶段要靠它保证改一个块不会碰到别的块。
pub(super) fn assert_spans_tile_the_content(
    spans: &[std::ops::Range<usize>],
    buffer_text: &str,
    label: &str,
) {
    let mut previous_end = 0usize;
    for span in spans {
        assert!(
            span.start >= previous_end,
            "{label}：块区间重叠或乱序：{span:?}，上一段结束于 {previous_end}"
        );
        assert!(
            span.end <= buffer_text.len(),
            "{label}：区间越过缓冲区末尾：{span:?}，全文 {} 字节",
            buffer_text.len()
        );
        previous_end = span.end;
    }

    let mut is_covered = vec![false; buffer_text.len()];
    for span in spans {
        for cell in &mut is_covered[span.clone()] {
            *cell = true;
        }
    }
    // 逐字节标记再解码：区间端点落在多字节字符中间时才不会把结论糊成一团。
    let uncovered = String::from_utf8(
        buffer_text
            .bytes()
            .zip(is_covered)
            .filter(|(_, covered)| !covered)
            .map(|(byte, _)| byte)
            .collect(),
    )
    .expect("缓冲区里的原文必须是 UTF-8");
    assert!(
        uncovered.chars().all(|ch| ch == '\n'),
        "{label}：不属于任何块的字节里出现了非换行符：{uncovered:?}（区间 {spans:?}）"
    );
}

#[gpui::test]
async fn every_root_block_points_at_its_own_source(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let source = concat!(
        "# 标题\n",
        "\n",
        "段落一\n",
        "续行\n",
        "\n",
        "```\n",
        "let x = 1;\n",
        "```\n",
        "\n",
        "- 甲\n",
        "- 乙\n",
        "\n",
        "| a | b |\n",
        "| --- | --- |\n",
        "| 1 | 2 |\n",
        "\n",
        "\n",
        "末段\n",
    );
    // 预算 2：逼它分多块建，跨块的行基址算错就会在这里暴露。
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown_with_chunk_budget(cx, source.to_string(), None, 2)
    });
    redraw(cx);
    editor.read_with(cx, |editor, _cx| {
        assert!(
            editor.document.pending_tail().is_none(),
            "续建没跑完，测到的是半截文档"
        );
    });

    let (spans, buffer_text) = root_block_spans(&editor, cx);
    let sources: Vec<&str> = spans.iter().map(|(_, text)| text.as_str()).collect();
    assert_eq!(
        sources,
        vec![
            "# 标题",
            "段落一\n续行",
            "```\nlet x = 1;\n```",
            "- 甲",
            "- 乙",
            "| a | b |\n| --- | --- |\n| 1 | 2 |",
            // 连续两个空行留下一个空段落块，它占住其中一条空行。
            "",
            "末段",
        ],
        "块区间取出的源码与文档里的对应段落不一致"
    );
    assert_spans_tile_the_content(&span_ranges(&spans), &buffer_text, "分块导入的文档");
}

/// 末行没有换行符的文档：最后一块的区间必须包含它自己的最后一个字节。
///
/// 「区间右端减掉那个换行符」这条规则在文末是错的——那里根本没有换行符可减。
#[gpui::test]
async fn a_document_without_a_trailing_newline_keeps_its_last_byte_inside_the_block(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);

    for source in ["甲\n乙", "甲\n\n乙", "标题\n\n段落文字"] {
        let (editor, cx) =
            cx.add_window_view(|_window, cx| Editor::from_markdown(cx, source.to_string(), None));
        redraw(cx);

        let (spans, buffer_text) = root_block_spans(&editor, cx);
        let last = spans
            .last()
            .unwrap_or_else(|| panic!("{source:?} 没有建出根块"));
        assert!(
            buffer_text.ends_with(&last.1) && !last.1.is_empty(),
            "最后一块没吃到文档末尾的字节：块 {:?}，缓冲区 {buffer_text:?}",
            last.1
        );
        assert_spans_tile_the_content(&span_ranges(&spans), &buffer_text, "无末行换行的文档");
    }
}

#[gpui::test]
async fn blank_lines_between_blocks_belong_to_no_block(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let source = "甲\n\n\n\n乙\n";
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, source.to_string(), None));
    redraw(cx);

    let (spans, buffer_text) = root_block_spans(&editor, cx);

    // 「甲」和「乙」之间三个空行：两个变成空段落块（零宽区间），多出来的一个是分隔符。
    assert_eq!(spans.len(), 4, "两个段落块 + 两个空段落块");
    let covered_bytes: usize = spans.iter().map(|(span, _)| span.end - span.start).sum();
    assert_eq!(
        covered_bytes,
        "甲".len() + "乙".len(),
        "块区间把分隔空行也算进去了：{}",
        rendered_blocks(&spans)
    );
    assert_spans_tile_the_content(&span_ranges(&spans), &buffer_text, "空行分隔的文档");
}

/// 从磁盘真实漏斗打开的 CRLF 文档：行尾差异最容易让「行区间 → 字节区间」算错。
#[gpui::test]
async fn a_crlf_document_from_disk_anchors_every_root_block(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let source = concat!(
        "---\n",
        "title: 甲\n",
        "---\n",
        "# 标题\n",
        "\n",
        "段落一\n",
        "段落二\n",
        "\n",
        "```\n",
        "\n",
        "let x = 1;\n",
        "```\n",
        "\n",
        "- 甲\n",
        "  - 子项\n",
        "\n",
        "| a | b |\n",
        "| --- | --- |\n",
        "| 1 | 2 |\n",
        "\n",
        "末段\n",
    );
    let path = temp_markdown_path("crlf-source-spans");
    fs::write(&path, source.replace('\n', "\r\n")).expect("write fixture");
    let cleanup = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup);
    });

    let document = encoding::load_document(&path).expect("read fixture");
    let (editor, cx) = cx.add_window_view(move |_window, cx| {
        Editor::from_loaded_document(cx, document, Some(path))
    });
    redraw(cx);

    let (spans, buffer_text) = root_block_spans(&editor, cx);
    assert_spans_tile_the_content(&span_ranges(&spans), &buffer_text, "CRLF 文档");

    // 代码围栏内部的空行属于这个块：区间必须把整个围栏（含空行）都圈进来。
    let fence = spans
        .iter()
        .map(|(_, text)| text.as_str())
        .find(|text| text.starts_with("```"))
        .unwrap_or_else(|| panic!("没有块取到代码围栏的源码：{}", rendered_blocks(&spans)));
    assert_eq!(fence, "```\n\nlet x = 1;\n```");

    // 嵌套列表是一个根块，父项区间要连子项行一起圈住。
    assert!(
        spans
            .iter()
            .any(|(_, text)| text.as_str() == "- 甲\n  - 子项"),
        "父项区间没覆盖子项行：{}",
        rendered_blocks(&spans)
    );
}

/// 每一步之后每个根块都必须带着指向缓冲区的区间。
///
/// 区间是读取侧唯一的锚点：漏了区间的块在位置换算里不存在，表现就是
/// 「点了搜索结果没反应」「光标落回 0」「大纲跳转停在篇首」。凡是重建整棵
/// 树的路径（打字、拆块、撤销、切视图）都必须把区间重新挂上。
#[gpui::test]
async fn every_document_rebuild_leaves_every_root_block_anchored(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let source = concat!(
        "# 标题\n",
        "\n",
        "段落一\n",
        "\n",
        "> 引用\n",
        "\n",
        "| a | b |\n",
        "| --- | --- |\n",
        "| 1 | 2 |\n",
    );
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, source.to_string(), None));
    redraw(cx);

    let mut checked = 0usize;
    let mut assert_anchored = |label: &str, cx: &mut gpui::VisualTestContext| {
        let (spans, buffer_text) = root_block_spans(&editor, cx);
        assert_spans_tile_the_content(&span_ranges(&spans), &buffer_text, label);
        assert!(spans.len() > 1, "{label}：夹具没分出多个块");
        checked += 1;
    };
    assert_anchored("打开", cx);

    cx.simulate_input("写");
    redraw(cx);
    assert_anchored("打字", cx);

    cx.dispatch_action(Newline);
    redraw(cx);
    assert_anchored("拆块", cx);

    editor.update(cx, |editor, cx| editor.undo_document(cx));
    redraw(cx);
    assert_anchored("撤销", cx);

    editor.update(cx, |editor, cx| {
        editor.toggle_view_mode(cx);
        editor.toggle_view_mode(cx);
    });
    redraw(cx);
    assert_anchored("切到源码视图再切回来", cx);

    assert_eq!(checked, 5, "每一步都该检查一次");
}

/// 源码/代码文档的根块也要各自持有区间，且区间里的字节 == 块自己那份文本。
///
/// 这一档的块本来就是缓冲区的一段切片，所以「切得对不对」是可以逐块对照的：
/// 区间不重叠、不越界，块文本与区间字节一模一样。少了这条，写回会拿过期区间
/// 把字节写到别的块身上（回车拆块第一次付费的整篇落笔就是这么来的）。
#[gpui::test]
async fn a_code_document_tiles_the_buffer_with_block_spans(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let mut source = String::new();
    for index in 0..1200 {
        source.push_str(&format!("print({index})\n"));
    }
    let path = std::env::temp_dir().join(format!("velora-code-tiles-{}.py", std::process::id()));
    fs::write(&path, &source).expect("write fixture");
    let cleanup = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup);
    });
    let document = encoding::load_document(&path).expect("read fixture");
    let (editor, cx) = cx.add_window_view(move |_window, cx| {
        Editor::from_loaded_document(cx, document, Some(path.clone()))
    });
    let deadline = Instant::now() + Duration::from_secs(120);
    while editor.read_with(cx, |editor, _| editor.document.pending_source().is_some()) {
        assert!(Instant::now() < deadline, "分块续建未完成");
        cx.run_until_parked();
    }
    redraw(cx);

    let assert_tiles = |label: &str, cx: &mut gpui::VisualTestContext| {
        let (spans, buffer_text) = root_block_spans(&editor, cx);
        assert_spans_tile_the_content(&span_ranges(&spans), &buffer_text, label);
        let mismatched = editor.read_with(cx, |editor, cx| {
            editor
                .document
                .root_blocks()
                .iter()
                .zip(spans.iter())
                .find(|(block, (span, _))| block.read(cx).display_text() != editor.buffer.slice(span.clone()))
                .map(|(block, (span, _))| (block.entity_id(), span.clone()))
        });
        assert!(
            mismatched.is_none(),
            "{label}：有块的文本与它的区间字节不一致：{mismatched:?}"
        );
        assert!(spans.len() > 1, "{label}：夹具该分出多块");
    };
    assert_tiles("打开", cx);

    cx.simulate_input("x");
    redraw(cx);
    assert_tiles("打字", cx);

    cx.dispatch_action(Newline);
    redraw(cx);
    assert_tiles("回车拆块", cx);

    editor.update(cx, |editor, cx| editor.undo_document(cx));
    redraw(cx);
    assert_tiles("撤销", cx);
}

/// 源码分块文档的行号就是**文件里的行号**：每块的首行等于它区间起点在缓冲区里的行号，
/// 前面多一行，后面的块整体跟着挪。
///
/// 这个数以前靠逐块 `display_text().split('\n').count()` 累出来——按一个键就把整篇文本
/// 再扫一遍（预算「每键全文遍历次数 = 0」上的一处漏项）。现在改问缓冲区（行索引是
/// Fenwick 里两次查询），这条守卫钉住语义：行号与 `source_span` 说的是同一件事。
#[gpui::test]
async fn source_document_line_numbers_follow_the_buffer(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let mut source = String::new();
    for index in 0..1200 {
        source.push_str(&format!("print({index})\n"));
    }
    let path = std::env::temp_dir().join(format!("velora-code-lines-{}.py", std::process::id()));
    fs::write(&path, source).expect("write fixture");
    let cleanup = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup);
    });
    let document = encoding::load_document(&path).expect("read fixture");
    let (editor, cx) = cx.add_window_view(move |_window, cx| {
        Editor::from_loaded_document(cx, document, Some(path.clone()))
    });
    let deadline = Instant::now() + Duration::from_secs(120);
    while editor.read_with(cx, |editor, _| editor.document.pending_source().is_some()) {
        assert!(Instant::now() < deadline, "分块续建未完成");
        cx.run_until_parked();
    }
    redraw(cx);

    let assert_line_numbers = |label: &str, cx: &mut gpui::VisualTestContext| {
        let wrong = editor.read_with(cx, |editor, cx| {
            editor
                .document
                .root_blocks()
                .iter()
                .filter_map(|block| {
                    let span = block.read(cx).record.source_span.clone()?;
                    let shown = block.read(cx).source_line_start();
                    let truth = editor.buffer.line_of(span.start) + 1;
                    (shown != truth).then_some((shown, truth))
                })
                .take(3)
                .collect::<Vec<_>>()
        });
        assert!(
            wrong.is_empty(),
            "{label}：有块的行号不是缓冲区的行号（显示, 应该）{wrong:?}"
        );
    };
    assert_line_numbers("打开", cx);

    let second_before = editor.read_with(cx, |editor, cx| {
        editor.document.root_blocks()[1].read(cx).source_line_start()
    });
    let root = editor.read_with(cx, |editor, _cx| {
        editor.document.root_blocks()[0].clone()
    });
    cx.update(|_window, cx| {
        editor.update(cx, |editor, _cx| editor.focus_block(root.entity_id()));
        root.update(cx, |block, block_cx| block.move_to(0, block_cx));
    });
    redraw(cx);
    cx.dispatch_action(Newline);
    redraw(cx);

    assert_line_numbers("在最前面插一行之后", cx);
    let second_after = editor.read_with(cx, |editor, cx| {
        editor.document.root_blocks()[1].read(cx).source_line_start()
    });
    assert_eq!(
        second_after,
        second_before + 1,
        "前面多了一行，后面那块的行号没跟着挪"
    );
}

/// ATX 标题的内容起点是**解析期**记下的数据，不是事后拿文件行与模型行比出来的。
///
/// 记号宽度按模型拼（一律 `# `）会漂位：缩进过的 `  # 标题`、`#  记号后两个空格`
/// 都不是「两个字节」。这一档先只管根块自己那一行（引用/列表里的子块还要把上级容器
/// 吃掉的字节一起记，那是后面的事），所以用例都是顶格文档里的单行标题。
#[gpui::test]
async fn an_atx_heading_remembers_where_its_content_starts(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let cases: &[(&str, u32)] = &[
        ("# 顶格标题", 2),
        ("  # 两格缩进", 4),
        ("### 三级标题", 4),
        ("#  记号后两个空格", 2),
        ("# 尾部井号 #", 2),
    ];
    for (line, expected) in cases {
        let (editor, cx) = cx.add_window_view(|_window, cx| {
            Editor::from_markdown(cx, format!("{line}\n\n正文。\n"), None)
        });
        cx.run_until_parked();
        editor.read_with(cx, |editor, cx| {
            let heading = editor.document.root_blocks()[0].clone();
            let record = heading.read(cx).record.clone();
            assert_eq!(
                record.source_line_prefixes,
                vec![*expected],
                "「{line}」的记号宽度没在解析期记下来"
            );
            // 记下的那一位必须正落在内容上：从它起读，文件里就是这一块的内容。
            let span = record.source_span.clone().expect("标题块该有源码区间");
            let content = record.title.markdown_offset_map().markdown().to_string();
            let from = span.start + *expected as usize;
            assert_eq!(
                editor.buffer.slice(from..from + content.len()),
                content,
                "「{line}」按记下的宽度读不出自己的内容"
            );
        });
    }
}

/// 打字用的那一行记号宽度，来自解析期记下的数据，不是事后拿文件行与模型行比出来的。
///
/// 这两个计数器是 #33 的量表：`line_prefix_measured` 该随着一族一族形状迁移一路降到 0
/// （引用/列表里的子块、多行块续行、代码围栏、表格格子还没记到，仍要走比出来那条路）。
#[gpui::test]
async fn typing_in_an_atx_heading_uses_the_parse_time_prefix(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "# 甲标题\n\n## 乙标题\n".to_string(), None)
    });
    editor.update(cx, |editor, _cx| {
        let root = editor.document.root_blocks()[0].clone();
        editor.focus_block(root.entity_id());
    });
    redraw(cx);

    let before = editor.read_with(cx, |editor, _| {
        (
            editor.line_prefix_from_record.get(),
            editor.line_prefix_measured.get(),
        )
    });
    cx.simulate_input("写");
    redraw(cx);
    let after = editor.read_with(cx, |editor, _| {
        (
            editor.line_prefix_from_record.get(),
            editor.line_prefix_measured.get(),
        )
    });
    assert!(
        after.0 > before.0,
        "这一次按键没用上解析期记下的记号宽度（记下来的行数没涨）"
    );
    assert_eq!(
        after.1 - before.1,
        0,
        "标题那一行的宽度还在事后拿文件行与模型行比：比出来的数是猜的，\
         缩进过、少个空格、行内有转义就漂一位，字会写进邻居的字节里"
    );
    // 落点还是对的：写在内容最前面的那个字，落在 `# ` 之后。
    editor.read_with(cx, |editor, _| {
        assert_eq!(
            editor.buffer.slice(editor.buffer.line_range(0)),
            "# 写甲标题",
            "记号宽度换了来源，落点就该一样对"
        );
    });
}

/// 段落同样按解析期记下的宽度落笔：根段落一行都没剥，每行宽度都是 0，这是数据不是猜。
///
/// 多行段落（续行、行尾硬换行 `\`）尤其要看这份账——按模型拼续行缩进以前会把硬换行的
/// `\` 序列化成 `\\`，一位之差把后面所有行的落点带漂。
#[gpui::test]
async fn typing_in_a_paragraph_uses_the_parse_time_prefix(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "正文甲\n正文乙\n".to_string(), None)
    });
    editor.update(cx, |editor, _cx| {
        let root = editor.document.root_blocks()[0].clone();
        editor.focus_block(root.entity_id());
    });
    redraw(cx);

    let before = editor.read_with(cx, |editor, _| {
        (
            editor.line_prefix_from_record.get(),
            editor.line_prefix_measured.get(),
        )
    });
    // 焦点落在块上时光标在第一行行首：这一行的落点靠的是记下来的宽度，不是比出来的。
    cx.simulate_input("写");
    redraw(cx);
    let after = editor.read_with(cx, |editor, _| {
        (
            editor.line_prefix_from_record.get(),
            editor.line_prefix_measured.get(),
        )
    });
    assert!(
        after.0 > before.0,
        "打字没用上解析期记下的记号宽度（记下来的行数没涨）"
    );
    assert_eq!(after.1 - before.1, 0, "这两行段落还在事后拿文件行与模型行比");
    editor.read_with(cx, |editor, _| {
        assert_eq!(editor.buffer.text(), "写正文甲\n正文乙\n");
    });
}

/// 列表项也一样：项标记（缩进、子弹写法、任务框）占几位是解析期记下的数据。
///
/// `+`、`1)`、制表符分隔、`- [x]` 这些写法的宽度都不一样，按模型拼「两个空格 + `- `」
/// 一族里最容易漂的一族——以前每次换算都要拿文件行重量一遍。
#[gpui::test]
async fn typing_in_a_list_item_uses_the_parse_time_prefix(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    for (name, source, expected) in [
        ("破折号", "- 项甲\n", "- 写项甲\n"),
        ("加号子弹", "+ 项乙\n", "+ 写项乙\n"),
        ("带括号序号", "1) 项丙\n", "1) 写项丙\n"),
        ("制表符分隔", "-\t项丁\n", "-\t写项丁\n"),
        ("任务框", "- [x] 项戊\n", "- [x] 写项戊\n"),
    ] {
        let (editor, cx) =
            cx.add_window_view(|_window, cx| Editor::from_markdown(cx, source.to_string(), None));
        editor.update(cx, |editor, _cx| {
            let root = editor.document.root_blocks()[0].clone();
            editor.focus_block(root.entity_id());
        });
        redraw(cx);

        let before = editor.read_with(cx, |editor, _| {
            (
                editor.line_prefix_from_record.get(),
                editor.line_prefix_measured.get(),
            )
        });
        cx.simulate_input("写");
        redraw(cx);
        let after = editor.read_with(cx, |editor, _| {
            (
                editor.line_prefix_from_record.get(),
                editor.line_prefix_measured.get(),
                editor.buffer.text(),
            )
        });
        assert!(after.0 > before.0, "{name}：打字没用上解析期记下的记号宽度");
        assert_eq!(
            after.1 - before.1,
            0,
            "{name}：这一行还在事后拿文件行与模型行比"
        );
        assert_eq!(after.2, expected, "{name}：字落错了字节");
    }
}
