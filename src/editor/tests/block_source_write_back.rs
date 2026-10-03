//! 块编辑必须按区间写回缓冲区，而不是等保存时整篇重新序列化。
//!
//! 这一条决定了「未编辑的部分能不能保住原文」：`__下划线__` 会被规范成
//! `**…**`、表格列宽填充会被重算、`(a)` 序号列表会被改写成 `1.`——只要保存走
//! 整篇重投影，用户改一个段落就会把全文的写法洗一遍。所以这里断言两件相反的事：
//! 改动的字节确实进了缓冲区，而**别的块一个字节都没动**。

use super::block_source_spans::{
    assert_spans_tile_the_content, rendered_blocks, root_block_spans, span_ranges,
};
use super::common::*;
use crate::editor::encoding;

/// 一个「重新序列化必然改写」的文档：填充过的表格 + 下划线强调 + 括号序号列表。
pub(super) const LOSSY_SHAPE_FIXTURE: &str = concat!(
    "段落文字\n",
    "\n",
    "| 名称 | 数量 |\n",
    "| ---- | ---- |\n",
    "| 甲   | 1    |\n",
    "\n",
    "强调 __下划线__ 结尾\n",
);

#[gpui::test]
async fn typing_in_a_block_writes_into_the_buffer_without_touching_its_neighbours(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);

    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, LOSSY_SHAPE_FIXTURE.to_string(), None)
    });
    redraw(cx);

    cx.simulate_input("写");
    redraw(cx);

    let buffer_text = editor.read_with(cx, |editor, _cx| editor.buffer.text());

    assert!(
        buffer_text.contains("写段落文字") || buffer_text.contains("段落文字写"),
        "编辑没进缓冲区：{buffer_text:?}"
    );
    // 未编辑的块必须还是磁盘上那个样子：表格填充与下划线写法原样保留。
    assert!(
        buffer_text.contains("| 甲   | 1    |"),
        "表格列宽填充被改写了：{buffer_text:?}"
    );
    assert!(
        buffer_text.contains("强调 __下划线__ 结尾"),
        "下划线强调被规范成了别的写法：{buffer_text:?}"
    );
}
/// 写回之后所有区间必须还能各自对应到自己的源码：变长的编辑靠平移保住邻居位置。
#[gpui::test]
async fn spans_still_tile_the_document_after_an_edit(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, LOSSY_SHAPE_FIXTURE.to_string(), None)
    });
    redraw(cx);

    cx.simulate_input("很长的一段增量文字");
    redraw(cx);

    let (spans, buffer_text) = root_block_spans(&editor, cx);
    assert_spans_tile_the_content(&span_ranges(&spans), &buffer_text, "写回之后");

    // 打进去的字必须落在被编辑那块自己的区间里。
    assert!(
        spans
            .iter()
            .any(|(_, text)| text.contains("很长的一段增量文字")),
        "写回的字节没落进被编辑块的区间：{}",
        rendered_blocks(&spans)
    );

    // 区间没漂：每个块的区间里都得含有它自己的每个词。逐个词比而不比整串，是因为
    // 区间取的是**原文**（`__下划线__`、表格列宽填充），而模型里的写法是规范过的
    // ——这个差别正是「保住原文」的内容，不是漂移。
    editor.read_with(cx, |editor, cx| {
        for (span, text) in &spans {
            let block = editor
                .document
                .root_blocks()
                .iter()
                .find(|block| block.read(cx).record.source_span.as_ref() == Some(span))
                .unwrap_or_else(|| panic!("区间 {span:?} 对不上任何块"));
            let visible = block.read(cx).record.title.visible_text().to_string();
            let missing = visible
                .split_whitespace()
                .filter(|word| !text.contains(word))
                .collect::<Vec<_>>();
            assert!(
                missing.is_empty(),
                "块区间漂了：区间里是 {text:?}，块的内容是 {visible:?}，缺了 {missing:?}"
            );
        }
    });
}

/// 结构变更（回车拆块）按区间写回：缓冲区只多出接缝那一行，别的块一个字节都不动，
/// 而每根块仍然挂着自己的区间。
///
/// 这条以前钉的是「拆块没有区间，所以整篇重投影之后缓冲区等于序列化」。区间档位
/// 接住拆块之后，那个等式不再成立——成立的是更强的性质：不重投影（`source_serializations`
/// 为 0），表里的列宽填充和 `__下划线__` 写法原样保留，末行换行也没被吃掉。
#[gpui::test]
async fn a_structural_edit_writes_through_its_interval_and_reanchors_every_block(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);

    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, LOSSY_SHAPE_FIXTURE.to_string(), None)
    });
    redraw(cx);

    cx.dispatch_action(Newline);
    redraw(cx);

    let buffer_text = editor.read_with(cx, |editor, _cx| editor.buffer.text());
    assert_eq!(
        buffer_text,
        format!("\n{LOSSY_SHAPE_FIXTURE}"),
        "回车拆块应该只在块首插一行，别的字节一个字都不许多改或少改"
    );

    // 每根块都还挂着区间：漏挂的块在位置换算里不存在，表现就是光标落回 0、
    // 点了搜索结果没反应。拆出来的空块记零宽在段首。
    editor.read_with(cx, |editor, cx| {
        let missing = editor
            .document
            .root_blocks()
            .iter()
            .filter(|block| block.read(cx).record.source_span.is_none())
            .count();
        assert_eq!(missing, 0, "拆块后有 {missing} 根块丢了区间");
        assert!(editor.document.root_count() > 3, "拆块后根块数应该增加");
        assert_eq!(
            editor.source_serializations.get(),
            0,
            "拆块还在整篇重新序列化"
        );
    });
    let (spans, buffer_text) = present_root_spans(&editor, cx);
    assert_spans_tile_the_content(&spans, &buffer_text, "结构变更之后");
}

/// 只取已经挂上区间的根块；漏挂的由重投影兜底，不该让测试瞎掉。
fn present_root_spans(
    editor: &gpui::Entity<Editor>,
    cx: &mut gpui::VisualTestContext,
) -> (Vec<std::ops::Range<usize>>, String) {
    editor.read_with(cx, |editor, cx| {
        let spans = editor
            .document
            .root_blocks()
            .iter()
            .filter_map(|block| block.read(cx).record.source_span.clone())
            .collect::<Vec<_>>();
        (spans, editor.buffer.text())
    })
}

/// 保存换源到缓冲区：改一处，别的块在**磁盘上**也还是一个字节都没变。
#[gpui::test]
async fn saving_after_an_edit_keeps_the_untouched_blocks_byte_identical(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);

    let path = temp_markdown_path("write-back-save");
    fs::write(&path, LOSSY_SHAPE_FIXTURE.replace('\n', "\r\n")).expect("write fixture");
    let cleanup = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup);
    });

    let document = encoding::load_document(&path).expect("read fixture");
    let open_path = path.clone();
    let (editor, cx) = cx.add_window_view(move |_window, cx| {
        Editor::from_loaded_document(cx, document, Some(open_path))
    });
    redraw(cx);

    cx.simulate_input("写");
    redraw(cx);
    cx.simulate_keystrokes("ctrl-s");
    redraw(cx);

    let saved = fs::read_to_string(&path).expect("read saved file");
    // 只有被编辑的那一块变了：表格填充、`__` 写法、CRLF 与末行换行都还是磁盘上的样子。
    // 走整篇重新序列化的话，这四样会同时被改写（`| --- |`、`**下划线**`、LF、丢末行换行）。
    assert_eq!(
        saved,
        LOSSY_SHAPE_FIXTURE.replace("段落文字", "写段落文字").replace('\n', "\r\n")
    );
    assert!(!editor.read_with(cx, |editor, _cx| editor.document_dirty));
}

/// 拆块也必须保住别的块：回车不该把整篇重新序列化一遍。
#[gpui::test]
async fn splitting_a_block_keeps_the_other_blocks_bytes_untouched(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);

    let path = temp_markdown_path("write-back-split");
    fs::write(&path, LOSSY_SHAPE_FIXTURE.replace('\n', "\r\n")).expect("write fixture");
    let cleanup = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup);
    });

    let document = encoding::load_document(&path).expect("read fixture");
    let open_path = path.clone();
    let (_editor, cx) =
        cx.add_window_view(move |_window, cx| Editor::from_loaded_document(cx, document, Some(open_path)));
    redraw(cx);

    cx.simulate_input("写");
    cx.dispatch_action(Newline);
    redraw(cx);
    cx.simulate_keystrokes("ctrl-s");
    redraw(cx);

    let saved = fs::read_to_string(&path).expect("read saved file");
    assert_eq!(
        saved,
        concat!(
            "写\n\n段落文字\n\n",
            "| 名称 | 数量 |\n",
            "| ---- | ---- |\n",
            "| 甲   | 1    |\n",
            "\n",
            "强调 __下划线__ 结尾\n",
        )
        .replace('\n', "\r\n"),
        "一次回车把未编辑的块也重新序列化了：{saved:?}"
    );
}

/// 拆完再合回来：分隔空行的加减必须正好互相抵消，文档回到「原文 + 那一处改动」。
#[gpui::test]
async fn splitting_then_merging_back_restores_the_original_bytes(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let path = temp_markdown_path("write-back-split-merge");
    fs::write(&path, LOSSY_SHAPE_FIXTURE.replace('\n', "\r\n")).expect("write fixture");
    let cleanup = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup);
    });

    let document = encoding::load_document(&path).expect("read fixture");
    let open_path = path.clone();
    let (editor, cx) = cx.add_window_view(move |_window, cx| {
        Editor::from_loaded_document(cx, document, Some(open_path))
    });
    redraw(cx);

    cx.simulate_input("写");
    cx.dispatch_action(Newline);
    cx.dispatch_action(DeleteBack);
    redraw(cx);
    cx.simulate_keystrokes("ctrl-s");
    redraw(cx);

    let saved = fs::read_to_string(&path).expect("read saved file");
    assert_eq!(
        saved,
        LOSSY_SHAPE_FIXTURE
            .replace("段落文字", "写段落文字")
            .replace('\n', "\r\n"),
        "拆块再合块之后，落盘的不再是「原文 + 一处改动」：{saved:?}"
    );
    let (spans, buffer_text) = root_block_spans(&editor, cx);
    assert_spans_tile_the_content(&span_ranges(&spans), &buffer_text, "拆完再合回来");
}

/// 跨块删除同样只能改选区自己那一段：删掉前两段正文，不该把不相干的表格列宽
/// 填充、`__下划线__` 写法、CRLF 与末行换行一起洗掉。
#[gpui::test]
async fn cross_block_delete_keeps_the_other_blocks_bytes_untouched(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    const FIXTURE: &str = concat!(
        "第一段文字\n",
        "\n",
        "第二段文字\n",
        "\n",
        "| 名称 | 数量 |\n",
        "| ---- | ---- |\n",
        "| 甲   | 1    |\n",
        "\n",
        "强调 __下划线__ 结尾\n",
    );

    let path = temp_markdown_path("write-back-cross-block-delete");
    fs::write(&path, FIXTURE.replace('\n', "\r\n")).expect("write fixture");
    let cleanup = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup);
    });

    let document = encoding::load_document(&path).expect("read fixture");
    let open_path = path.clone();
    let (editor, cx) = cx.add_window_view(move |_window, cx| {
        Editor::from_loaded_document(cx, document, Some(open_path))
    });
    redraw(cx);

    editor.update(cx, |editor, _cx| {
        let visible = editor.document.visible_blocks().to_vec();
        editor.cross_block_selection = Some(crate::editor::CrossBlockSelection {
            anchor: crate::editor::CrossBlockSelectionEndpoint {
                entity_id: visible[0].entity.entity_id(),
                offset: 0,
            },
            focus: crate::editor::CrossBlockSelectionEndpoint {
                entity_id: visible[1].entity.entity_id(),
                offset: usize::MAX,
            },
        });
    });
    cx.dispatch_action(Delete);
    redraw(cx);
    cx.simulate_keystrokes("ctrl-s");
    redraw(cx);

    let saved = fs::read_to_string(&path).expect("read saved file");
    assert_eq!(
        saved,
        concat!(
            "\r\n",
            "\r\n",
            "| 名称 | 数量 |\r\n",
            "| ---- | ---- |\r\n",
            "| 甲   | 1    |\r\n",
            "\r\n",
            "强调 __下划线__ 结尾\r\n",
        ),
        "跨块删除把未编辑的块也重新序列化了：{saved:?}"
    );
}

/// 缩进一条列表项只该动这一条：它挂到上一条底下，被换掉的字节区间是这两条自己。
#[gpui::test]
async fn indenting_a_list_item_keeps_the_other_blocks_bytes_untouched(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);

    const FIXTURE: &str = concat!(
        "- 甲\n",
        "- 乙\n",
        "\n",
        "| 名称 | 数量 |\n",
        "| ---- | ---- |\n",
        "| 丙丁 | 12   |\n",
        "\n",
        "强调 __下划线__ 结尾\n",
    );

    let path = temp_markdown_path("write-back-indent");
    fs::write(&path, FIXTURE.replace('\n', "\r\n")).expect("write fixture");
    let cleanup = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup);
    });

    let document = encoding::load_document(&path).expect("read fixture");
    let open_path = path.clone();
    let (editor, cx) = cx.add_window_view(move |_window, cx| {
        Editor::from_loaded_document(cx, document, Some(open_path))
    });
    redraw(cx);

    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| {
            let second = editor.document.visible_blocks()[1].entity.clone();
            editor.on_block_event(second, &BlockEvent::RequestIndent, cx);
        });
    });
    redraw(cx);
    cx.simulate_keystrokes("ctrl-s");
    redraw(cx);

    let saved = fs::read_to_string(&path).expect("read saved file");
    assert_eq!(
        saved,
        FIXTURE.replace("- 甲\n- 乙", "- 甲\n  - 乙").replace('\n', "\r\n"),
        "缩进改写了这两条列表项之外的字节：{saved:?}"
    );
}

/// 手打一行表格把它接在已有表格下面：被换掉的区间是「这张表 + 那一行」，表外的块
/// 一个字节都不动（这一张表自己按新列宽重排是允许的，它就是要被改的那块）。
#[gpui::test]
async fn typing_a_table_row_keeps_the_other_blocks_bytes_untouched(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    const FIXTURE: &str = concat!(
        "段落文字\n",
        "\n",
        "| 名称 | 数量 |\n",
        "| ---- | ---- |\n",
        "| 甲   | 1    |\n",
        "\n",
        "| 丙 | 3 |\n",
        "\n",
        "强调 __下划线__ 结尾\n",
    );

    let path = temp_markdown_path("write-back-typed-row");
    fs::write(&path, FIXTURE.replace('\n', "\r\n")).expect("write fixture");
    let cleanup = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup);
    });

    let document = encoding::load_document(&path).expect("read fixture");
    let open_path = path.clone();
    let (editor, cx) = cx.add_window_view(move |_window, cx| {
        Editor::from_loaded_document(cx, document, Some(open_path))
    });
    redraw(cx);

    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| {
            let row = editor
                .document
                .visible_blocks()
                .iter()
                .find(|visible| visible.entity.read(cx).display_text() == "| 丙 | 3 |")
                .map(|visible| visible.entity.clone())
                .expect("夹具里应有一行待接进的表格行");
            editor.on_block_event(
                row,
                &BlockEvent::RequestNewline {
                    trailing: InlineTextTree::plain(String::new()),
                    source_already_mutated: false,
                },
                cx,
            );
        });
    });
    redraw(cx);
    cx.simulate_keystrokes("ctrl-s");
    redraw(cx);

    let saved = fs::read_to_string(&path).expect("read saved file");
    assert!(
        saved.starts_with("段落文字\r\n\r\n|"),
        "表前面的块被改写了：{saved:?}"
    );
    assert!(
        saved.ends_with("强调 __下划线__ 结尾\r\n"),
        "表后面的块、行结束符或末行换行被改写了：{saved:?}"
    );
    assert!(
        saved.contains("丙") && saved.contains("3"),
        "手打的那一行没接进表里：{saved:?}"
    );
}

/// 手打分隔行成一张表：被换掉的是那两行，表外的块一个字节都不动。
#[gpui::test]
async fn forming_a_table_from_typed_rows_keeps_the_other_blocks_bytes_untouched(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);

    const FIXTURE: &str = concat!(
        "段落文字\n",
        "\n",
        "名称 | 数量\n",
        "\n",
        "---- | ----\n",
        "\n",
        "强调 __下划线__ 结尾\n",
    );

    let path = temp_markdown_path("write-back-form-table");
    fs::write(&path, FIXTURE.replace('\n', "\r\n")).expect("write fixture");
    let cleanup = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup);
    });

    let document = encoding::load_document(&path).expect("read fixture");
    let open_path = path.clone();
    let (editor, cx) = cx.add_window_view(move |_window, cx| {
        Editor::from_loaded_document(cx, document, Some(open_path))
    });
    redraw(cx);

    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| {
            let delimiter = editor
                .document
                .visible_blocks()
                .iter()
                .find(|visible| visible.entity.read(cx).display_text() == "---- | ----")
                .map(|visible| visible.entity.clone())
                .expect("夹具里应有一行分隔行");
            editor.on_block_event(
                delimiter,
                &BlockEvent::RequestNewline {
                    trailing: InlineTextTree::plain(String::new()),
                    source_already_mutated: false,
                },
                cx,
            );
        });
    });
    redraw(cx);
    cx.simulate_keystrokes("ctrl-s");
    redraw(cx);

    let saved = fs::read_to_string(&path).expect("read saved file");
    assert!(
        saved.starts_with("段落文字\r\n\r\n"),
        "表前面的块被改写了：{saved:?}"
    );
    assert!(
        saved.ends_with("\r\n强调 __下划线__ 结尾\r\n"),
        "表后面的块、行结束符或末行换行被改写了：{saved:?}"
    );
    assert!(
        saved.contains("名称") && saved.contains("---"),
        "手打的两行没合成一张表：{saved:?}"
    );
}

/// 引用里按回车拆出下一个引用块：被换掉的只有那一段引用，表外的块一个字节都不动。
#[gpui::test]
async fn breaking_a_quote_keeps_the_other_blocks_bytes_untouched(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    const FIXTURE: &str = concat!(
        "段落文字\n",
        "\n",
        "> 引用一行\n",
        "\n",
        "| 名称 | 数量 |\n",
        "| ---- | ---- |\n",
        "| 甲   | 1    |\n",
        "\n",
        "强调 __下划线__ 结尾\n",
    );

    let path = temp_markdown_path("write-back-quote-break");
    fs::write(&path, FIXTURE.replace('\n', "\r\n")).expect("write fixture");
    let cleanup = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup);
    });

    let document = encoding::load_document(&path).expect("read fixture");
    let open_path = path.clone();
    let (editor, cx) = cx.add_window_view(move |_window, cx| {
        Editor::from_loaded_document(cx, document, Some(open_path))
    });
    redraw(cx);

    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| {
            let quote = editor
                .document
                .visible_blocks()
                .iter()
                .find(|visible| visible.entity.read(cx).kind() == BlockKind::Quote)
                .map(|visible| visible.entity.clone())
                .expect("夹具里应有一个引用块");
            editor.on_block_event(quote, &BlockEvent::RequestQuoteBreak, cx);
        });
    });
    redraw(cx);
    cx.simulate_keystrokes("ctrl-s");
    redraw(cx);

    let saved = fs::read_to_string(&path).expect("read saved file");
    assert!(
        saved.starts_with("段落文字\r\n\r\n> 引用一行\r\n"),
        "引用前面或引用自己的字节被改写了：{saved:?}"
    );
    assert!(
        saved.ends_with("强调 __下划线__ 结尾\r\n"),
        "引用后面的块、行结束符或末行换行被改写了：{saved:?}"
    );
    assert!(
        saved.contains("| ---- | ---- |\r\n| 甲   | 1    |"),
        "拆引用把不相干表格的列宽填充重排了：{saved:?}"
    );
}

/// 标注里按回车跳出标注也一样只能改那一段。
#[gpui::test]
async fn breaking_out_of_a_callout_keeps_the_other_blocks_bytes_untouched(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);

    const FIXTURE: &str = concat!(
        "段落文字\n",
        "\n",
        "> [!NOTE] 标注一行\n",
        ">\n",
        "> 正文一行\n",
        "\n",
        "| 名称 | 数量 |\n",
        "| ---- | ---- |\n",
        "| 甲   | 1    |\n",
        "\n",
        "强调 __下划线__ 结尾\n",
    );

    let path = temp_markdown_path("write-back-callout-break");
    fs::write(&path, FIXTURE.replace('\n', "\r\n")).expect("write fixture");
    let cleanup = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup);
    });

    let document = encoding::load_document(&path).expect("read fixture");
    let open_path = path.clone();
    let (editor, cx) = cx.add_window_view(move |_window, cx| {
        Editor::from_loaded_document(cx, document, Some(open_path))
    });
    redraw(cx);

    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| {
            let body = editor
                .document
                .visible_blocks()
                .iter()
                .find(|visible| visible.entity.read(cx).display_text() == "正文一行")
                .map(|visible| visible.entity.clone())
                .expect("夹具里应有标注正文那一行");
            editor.on_block_event(body, &BlockEvent::RequestCalloutBreak, cx);
        });
    });
    redraw(cx);
    cx.simulate_keystrokes("ctrl-s");
    redraw(cx);

    let saved = fs::read_to_string(&path).expect("read saved file");
    assert!(
        saved.contains("> [!NOTE] 标注一行"),
        "标注自己丢了：{saved:?}"
    );
    assert!(
        saved.contains("| ---- | ---- |\r\n| 甲   | 1    |"),
        "跳出标注把不相干表格的列宽填充重排了：{saved:?}"
    );
    assert!(
        saved.ends_with("强调 __下划线__ 结尾\r\n"),
        "跳出标注改写了后面的块、行结束符或末行换行：{saved:?}"
    );
}

/// 在空段落上按退格是删掉那一段：只能动那一行，别处的字节一个不改。
#[gpui::test]
async fn deleting_an_empty_paragraph_keeps_the_other_blocks_bytes_untouched(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);

    const FIXTURE: &str = concat!(
        "| 名称 | 数量 |\n",
        "| ---- | ---- |\n",
        "| 甲   | 1    |\n",
        "\n",
        "\n",
        "强调 __下划线__ 结尾\n",
    );

    let path = temp_markdown_path("write-back-delete-empty");
    fs::write(&path, FIXTURE.replace('\n', "\r\n")).expect("write fixture");
    let cleanup = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup);
    });

    let document = encoding::load_document(&path).expect("read fixture");
    let open_path = path.clone();
    let (editor, cx) = cx.add_window_view(move |_window, cx| {
        Editor::from_loaded_document(cx, document, Some(open_path))
    });
    redraw(cx);

    let expected = FIXTURE.replace("|\n\n\n强调", "|\n\n强调");
    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| {
            let empty = editor
                .document
                .visible_blocks()
                .iter()
                .find(|visible| {
                    let block = visible.entity.read(cx);
                    block.kind() == BlockKind::Paragraph && block.display_text().is_empty()
                })
                .map(|visible| visible.entity.clone())
                .expect("夹具里应有一个空段落");
            editor.on_block_event(empty, &BlockEvent::RequestDelete, cx);
        });
    });
    redraw(cx);

    let buffer_text = editor.read_with(cx, |editor, _cx| editor.buffer.text());
    assert_eq!(
        buffer_text, expected,
        "删一个空段落改写了别的字节，或者在文档里留下了多余空行：{buffer_text:?}"
    );

    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| editor.save_document(window, cx));
    });
    redraw(cx);
    let saved = fs::read_to_string(&path).expect("read saved file");
    assert_eq!(
        saved,
        expected.replace('\n', "\r\n"),
        "保存落盘的不是缓冲区里那份字节：{saved:?}"
    );
}

/// 提级一条嵌套列表项也只该动列表这一段：后面的段落、表格与行结束符不该被重排。
#[gpui::test]
async fn outdenting_a_list_item_keeps_the_other_blocks_bytes_untouched(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    const FIXTURE: &str = concat!(
        "- 甲\n",
        "  - 乙\n",
        "\n",
        "| 名称 | 数量 |\n",
        "| ---- | ---- |\n",
        "| 丙丁 | 12   |\n",
        "\n",
        "强调 __下划线__ 结尾\n",
    );

    let path = temp_markdown_path("write-back-outdent");
    fs::write(&path, FIXTURE.replace('\n', "\r\n")).expect("write fixture");
    let cleanup = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup);
    });

    let document = encoding::load_document(&path).expect("read fixture");
    let open_path = path.clone();
    let (editor, cx) = cx.add_window_view(move |_window, cx| {
        Editor::from_loaded_document(cx, document, Some(open_path))
    });
    redraw(cx);

    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| {
            let nested = editor.document.visible_blocks()[1].entity.clone();
            editor.on_block_event(nested, &BlockEvent::RequestOutdent, cx);
        });
    });
    redraw(cx);
    cx.simulate_keystrokes("ctrl-s");
    redraw(cx);

    let saved = fs::read_to_string(&path).expect("read saved file");
    assert!(
        saved.ends_with("\r\n\r\n强调 __下划线__ 结尾\r\n"),
        "提级改写了列表之外的字节：{saved:?}"
    );
    assert!(
        saved.contains("| ---- | ---- |\r\n| 丙丁 | 12   |"),
        "提级把不相干表格的列宽填充重排了：{saved:?}"
    );
    assert!(
        saved.starts_with("- 甲\r\n") && !saved.contains("  - 乙") && saved.contains("- 乙"),
        "提级没落到文件里：{saved:?}"
    );
}

/// 勾一个任务复选框只该改那一行的 `[ ]`：整篇重新序列化会把别处的列宽填充、
/// `__下划线__` 写法、CRLF 与末行换行一起洗掉。
#[gpui::test]
async fn toggling_a_task_checkbox_keeps_the_other_blocks_bytes_untouched(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);

    const FIXTURE: &str = concat!(
        "- [ ] 买牛奶\n",
        "\n",
        "| 名称 | 数量 |\n",
        "| ---- | ---- |\n",
        "| 甲   | 1    |\n",
        "\n",
        "强调 __下划线__ 结尾\n",
    );

    let path = temp_markdown_path("write-back-task-toggle");
    fs::write(&path, FIXTURE.replace('\n', "\r\n")).expect("write fixture");
    let cleanup = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup);
    });

    let document = encoding::load_document(&path).expect("read fixture");
    let open_path = path.clone();
    let (editor, cx) = cx.add_window_view(move |_window, cx| {
        Editor::from_loaded_document(cx, document, Some(open_path))
    });
    redraw(cx);

    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| {
            let task = editor
                .document
                .root_blocks()
                .iter()
                .find(|root| matches!(root.read(cx).kind(), BlockKind::TaskListItem { .. }))
                .cloned()
                .expect("夹具里应有一条任务");
            editor.on_block_event(task, &BlockEvent::ToggleTaskChecked, cx);
        });
    });
    redraw(cx);
    cx.simulate_keystrokes("ctrl-s");
    redraw(cx);

    let saved = fs::read_to_string(&path).expect("read saved file");
    assert_eq!(
        saved,
        FIXTURE
            .replace("- [ ] 买牛奶", "- [x] 买牛奶")
            .replace('\n', "\r\n"),
        "勾一个复选框改写了勾选项之外的字节：{saved:?}"
    );
}

/// 打字打进单元格也只该动那一格：单元格在缓冲区里有自己的字节区间，写回就该
/// 落在那段区间上，连同一张表里别的列的填充都不该重排，更不许动表外的块。
#[gpui::test]
async fn typing_in_a_table_cell_keeps_the_padding_and_the_neighbours(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);

    let path = temp_markdown_path("write-back-table-cell");
    fs::write(&path, LOSSY_SHAPE_FIXTURE.replace('\n', "\r\n")).expect("write fixture");
    let cleanup = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup);
    });

    let document = encoding::load_document(&path).expect("read fixture");
    let open_path = path.clone();
    let (editor, cx) = cx.add_window_view(move |_window, cx| {
        Editor::from_loaded_document(cx, document, Some(open_path))
    });
    redraw(cx);

    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| {
            let table = editor
                .document
                .root_blocks()
                .iter()
                .find(|root| root.read(cx).kind() == BlockKind::Table)
                .cloned()
                .expect("夹具里应有一张表");
            // 视觉行 0 是表头，1 是第一条数据行：光标落在「甲」这一格。
            assert!(
                editor.focus_table_cell_position(
                    &table,
                    crate::components::TableCellPosition { row: 1, column: 0 },
                    cx
                ),
                "定位不到数据行的单元格"
            );
        });
    });
    redraw(cx);

    cx.simulate_input("写");
    redraw(cx);
    cx.simulate_keystrokes("ctrl-s");
    redraw(cx);

    let saved = fs::read_to_string(&path).expect("read saved file");
    let row = saved
        .split("\r\n")
        .find(|line| line.contains("甲"))
        .expect("数据行还在文件里");
    assert!(row.contains("写"), "单元格里的字没进文件：{saved:?}");
    assert!(
        row.contains("| 1    |"),
        "同一行其它列的列宽填充被重排了：{saved:?}"
    );
    assert!(
        saved.starts_with("段落文字\r\n\r\n| 名称 | 数量 |\r\n| ---- | ---- |\r\n"),
        "表外与表头的字节被改写了：{saved:?}"
    );
    assert!(
        saved.ends_with("\r\n\r\n强调 __下划线__ 结尾\r\n"),
        "表后面的块、行结束符或末行换行被改写了：{saved:?}"
    );
}

/// 表格结构命令只该动这张表：加一行不许把文档里别的块改写。
///
/// 表自己那几行重新排布是允许的（新列宽要容下新的一行），管的是**表外**：段落、
/// `__下划线__` 写法、CRLF、末行换行都得逐字节还是磁盘上那样。
#[gpui::test]
async fn adding_a_table_row_keeps_the_other_blocks_bytes_untouched(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let path = temp_markdown_path("write-back-table-row");
    fs::write(&path, LOSSY_SHAPE_FIXTURE.replace('\n', "\r\n")).expect("write fixture");
    let cleanup = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup);
    });

    let document = encoding::load_document(&path).expect("read fixture");
    let open_path = path.clone();
    let (editor, cx) = cx.add_window_view(move |_window, cx| {
        Editor::from_loaded_document(cx, document, Some(open_path))
    });
    redraw(cx);

    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| {
            let table = editor
                .document
                .root_blocks()
                .iter()
                .find(|root| root.read(cx).kind() == BlockKind::Table)
                .cloned()
                .expect("夹具里应有一张表");
            editor.append_table_row(&table, cx);
        });
    });
    redraw(cx);
    cx.simulate_keystrokes("ctrl-s");
    redraw(cx);

    let saved = fs::read_to_string(&path).expect("read saved file");
    assert!(
        saved.starts_with("段落文字\r\n\r\n|"),
        "表前面的块被改写了：{saved:?}"
    );
    assert!(
        saved.ends_with("\r\n\r\n强调 __下划线__ 结尾\r\n"),
        "表后面的块、行结束符或末行换行被改写了：{saved:?}"
    );

    let table_lines = saved
        .split("\r\n")
        .filter(|line| line.starts_with('|'))
        .collect::<Vec<_>>();
    assert_eq!(
        table_lines.len(),
        4,
        "加完一行应该是 4 行表格内容（表头 + 分隔 + 两行数据）：{saved:?}"
    );
    assert!(
        table_lines
            .iter()
            .any(|line| line.contains("名称") && line.contains("数量")),
        "表头内容丢了：{table_lines:?}"
    );
    assert!(
        table_lines
            .iter()
            .any(|line| line.contains("甲") && line.contains("1")),
        "原有数据行的内容丢了：{table_lines:?}"
    );
}

/// 多行粘贴也一样只能改粘贴落点那一段。这条管的是「结构一变就整篇重投影」：
/// 粘贴把一段变三段，块序列变了，如果这时退回整篇重新序列化，不相干的表格列宽
/// 填充和 `__下划线__` 写法会跟着被洗，磁盘上的 CRLF 与末行换行也一起没了。
#[gpui::test]
async fn multiline_paste_keeps_the_other_blocks_bytes_untouched(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let path = temp_markdown_path("write-back-paste");
    fs::write(&path, LOSSY_SHAPE_FIXTURE.replace('\n', "\r\n")).expect("write fixture");
    let cleanup = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup);
    });

    let document = encoding::load_document(&path).expect("read fixture");
    let open_path = path.clone();
    let (editor, cx) = cx.add_window_view(move |_window, cx| {
        Editor::from_loaded_document(cx, document, Some(open_path))
    });
    redraw(cx);

    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| {
            let block = editor.document.visible_blocks()[0].entity.clone();
            editor.on_block_event(
                block,
                &BlockEvent::RequestPasteMultiline {
                    leading: InlineTextTree::plain(String::new()),
                    lines: vec!["粘贴一".to_string(), "粘贴二".to_string()],
                    trailing: InlineTextTree::plain("段落文字".to_string()),
                    split_physical_lines: true,
                },
                cx,
            );
        });
    });
    redraw(cx);
    cx.simulate_keystrokes("ctrl-s");
    redraw(cx);

    let saved = fs::read_to_string(&path).expect("read saved file");
    assert!(
        saved.contains("粘贴一") && saved.contains("粘贴二") && saved.contains("段落文字"),
        "粘贴的内容没落进文档：{saved:?}"
    );
    assert!(
        saved.contains("| 甲   | 1    |"),
        "粘贴把表格列宽填充重算了：{saved:?}"
    );
    assert!(
        saved.contains("强调 __下划线__ 结尾"),
        "粘贴把下划线强调规范成了别的写法：{saved:?}"
    );
    assert!(
        saved.contains("\r\n") && saved.ends_with('\n') && !saved.contains("\n\n\n"),
        "粘贴把行结束符或末行换行弄丢了：{saved:?}"
    );
}

/// 手打 Setext 下划线成标题：被换掉的只有那两行，别处的字节一个不动。
#[gpui::test]
async fn forming_a_setext_heading_keeps_the_other_blocks_bytes_untouched(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    const FIXTURE: &str = concat!(
        "| 名称 | 数量 |\n",
        "| ---- | ---- |\n",
        "| 甲   | 1    |\n",
        "\n",
        "标题文字\n",
        "\n",
        "====\n",
        "\n",
        "强调 __下划线__ 结尾\n",
    );

    let path = temp_markdown_path("write-back-setext");
    fs::write(&path, FIXTURE.replace('\n', "\r\n")).expect("write fixture");
    let cleanup = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup);
    });

    let document = encoding::load_document(&path).expect("read fixture");
    let open_path = path.clone();
    let (editor, cx) = cx.add_window_view(move |_window, cx| {
        Editor::from_loaded_document(cx, document, Some(open_path))
    });
    redraw(cx);

    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| {
            let underline = editor
                .document
                .visible_blocks()
                .iter()
                .find(|visible| visible.entity.read(cx).display_text() == "====")
                .map(|visible| visible.entity.clone())
                .expect("夹具里应有一条 Setext 下划线");
            editor.on_block_event(
                underline,
                &BlockEvent::RequestNewline {
                    trailing: InlineTextTree::plain(String::new()),
                    source_already_mutated: false,
                },
                cx,
            );
        });
    });
    redraw(cx);

    let buffer_text = editor.read_with(cx, |editor, _cx| editor.buffer.text());
    assert!(
        buffer_text.starts_with("| 名称 | 数量 |\n| ---- | ---- |\n| 甲   | 1    |\n"),
        "Setext 成标题把前面表格的列宽填充重算了：{buffer_text:?}"
    );
    assert!(
        buffer_text.ends_with("强调 __下划线__ 结尾\n"),
        "Setext 成标题把后面的块规范化了：{buffer_text:?}"
    );
    assert!(
        buffer_text.contains("标题文字"),
        "标题文字丢了：{buffer_text:?}"
    );
}

/// 单元格里按回车是在表后面插一个空段落：文件里多出来的应该只有那一行空行。
///
/// 整篇重投影会把这张表的列宽填充（`| ---- |`、`| 甲   | 1    |`）和表外的
/// `__下划线__` 写法一起洗掉，所以这里断言的是**除那一行空行外逐字节不变**。
#[gpui::test]
async fn pressing_enter_in_a_table_cell_only_inserts_a_blank_line(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let path = temp_markdown_path("write-back-table-enter");
    fs::write(&path, LOSSY_SHAPE_FIXTURE.replace('\n', "\r\n")).expect("write fixture");
    let cleanup = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup);
    });

    let document = encoding::load_document(&path).expect("read fixture");
    let open_path = path.clone();
    let (editor, cx) = cx.add_window_view(move |_window, cx| {
        Editor::from_loaded_document(cx, document, Some(open_path))
    });
    redraw(cx);

    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| {
            let table = editor
                .document
                .root_blocks()
                .iter()
                .find(|root| root.read(cx).kind() == BlockKind::Table)
                .cloned()
                .expect("夹具里应有一张表");
            let cell = table
                .read(cx)
                .table_runtime
                .as_ref()
                .and_then(|runtime| runtime.cell(crate::components::TableCellPosition { row: 1, column: 0 }))
                .expect("夹具里的表应有数据行的单元格");
            editor.on_block_event(
                cell.clone(),
                &BlockEvent::RequestNewline {
                    trailing: InlineTextTree::plain(String::new()),
                    source_already_mutated: false,
                },
                cx,
            );
        });
    });
    redraw(cx);

    let buffer_text = editor.read_with(cx, |editor, _cx| editor.buffer.text());
    assert_eq!(
        buffer_text,
        "段落文字\n\n| 名称 | 数量 |\n| ---- | ---- |\n| 甲   | 1    |\n\n\n强调 __下划线__ 结尾\n",
        "单元格里回车不该重排整篇文档"
    );
    // 插进去的空行会让后面那一段整体右移一格：区间必须还各自对得上自己的源码。
    let (spans, buffer_text) = present_root_spans(&editor, cx);
    assert_spans_tile_the_content(&spans, &buffer_text, "表后插入空段落之后");
}

/// 空的标注降级成引用只该改这一段：`> [!注意]` 换成 `>`，别的块一个字节都不动。
#[gpui::test]
async fn downgrading_an_empty_callout_keeps_the_other_blocks_bytes_untouched(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);

    const FIXTURE: &str = concat!(
        "段落文字\n",
        "\n",
        "| 名称 | 数量 |\n",
        "| ---- | ---- |\n",
        "| 甲   | 1    |\n",
        "\n",
        "> [!note]\n",
        ">\n",
        "\n",
        "强调 __下划线__ 结尾\n",
    );

    let path = temp_markdown_path("write-back-callout-downgrade");
    fs::write(&path, FIXTURE.replace('\n', "\r\n")).expect("write fixture");
    let cleanup = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup);
    });

    let document = encoding::load_document(&path).expect("read fixture");
    let open_path = path.clone();
    let (editor, cx) = cx.add_window_view(move |_window, cx| {
        Editor::from_loaded_document(cx, document, Some(open_path))
    });
    redraw(cx);

    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| {
            let body = editor
                .document
                .visible_blocks()
                .iter()
                .find(|visible| {
                    let block = visible.entity.read(cx);
                    if block.kind() != BlockKind::Paragraph || !block.display_text().is_empty() {
                        return false;
                    }
                    let block_id = visible.entity.entity_id();
                    editor
                        .document
                        .find_block_location(block_id)
                        .and_then(|location| location.parent)
                        .is_some_and(|parent| parent.read(cx).kind().callout_variant().is_some())
                })
                .map(|visible| visible.entity.clone())
                .expect("夹具里应有标注的空正文");
            editor.on_block_event(body, &BlockEvent::RequestDelete, cx);
        });
    });
    redraw(cx);

    let buffer_text = editor.read_with(cx, |editor, _cx| editor.buffer.text());
    assert!(
        buffer_text.starts_with("段落文字\n\n| 名称 | 数量 |\n| ---- | ---- |\n| 甲   | 1    |\n"),
        "标注降级把表格的列宽填充重算了：{buffer_text:?}"
    );
    assert!(
        buffer_text.ends_with("强调 __下划线__ 结尾\n"),
        "标注降级把后面的块规范化了，或丢了末行换行：{buffer_text:?}"
    );
    assert!(
        buffer_text.contains("> \\[!NOTE]"),
        "降级后的引用没进缓冲区：{buffer_text:?}"
    );
    // 这一根块从两行变一行，后面的块整体左移：区间必须还各自对得上自己的源码。
    let (spans, buffer_text) = present_root_spans(&editor, cx);
    assert_spans_tile_the_content(&spans, &buffer_text, "标注降级成引用之后");
}

/// 删掉整张表（删最后一行/列时表整个没了，原位留一个空段落）：文件里少的应该
/// 只有那三行表，接缝剩下的那一行空行就是那个空段落。别处的字节照旧不动。
#[gpui::test]
async fn deleting_a_whole_table_keeps_the_other_blocks_bytes_untouched(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let path = temp_markdown_path("write-back-drop-table");
    fs::write(&path, LOSSY_SHAPE_FIXTURE.replace('\n', "\r\n")).expect("write fixture");
    let cleanup = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup);
    });

    let document = encoding::load_document(&path).expect("read fixture");
    let open_path = path.clone();
    let (editor, cx) = cx.add_window_view(move |_window, cx| {
        Editor::from_loaded_document(cx, document, Some(open_path))
    });
    redraw(cx);

    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| {
            let table = editor
                .document
                .root_blocks()
                .iter()
                .find(|root| root.read(cx).kind() == BlockKind::Table)
                .cloned()
                .expect("夹具里应有一张表");
            editor.remove_table_block(&table, cx);
        });
    });
    redraw(cx);

    let buffer_text = editor.read_with(cx, |editor, _cx| editor.buffer.text());
    assert_eq!(
        buffer_text,
        "段落文字\n\n\n强调 __下划线__ 结尾\n",
        "删掉整张表不该把别处重排一遍"
    );

    cx.simulate_keystrokes("ctrl-s");
    redraw(cx);
    let saved = fs::read_to_string(&path).expect("read saved file");
    assert_eq!(
        saved,
        "段落文字\r\n\r\n\r\n强调 __下划线__ 结尾\r\n",
        "保存写出去的字节不再是「原文减去那张表」"
    );
    let (spans, buffer_text) = present_root_spans(&editor, cx);
    assert_spans_tile_the_content(&spans, &buffer_text, "整张表删掉之后");
}

/// 嵌套列表项降成子段落只该动列表那两行：表外与列表外的字节照旧。
#[gpui::test]
async fn downgrading_a_nested_list_item_keeps_the_other_blocks_bytes_untouched(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);

    const FIXTURE: &str = concat!(
        "段落文字\n",
        "\n",
        "- 甲\n",
        "  - 乙\n",
        "\n",
        "| 名称 | 数量 |\n",
        "| ---- | ---- |\n",
        "| 甲   | 1    |\n",
        "\n",
        "强调 __下划线__ 结尾\n",
    );

    let path = temp_markdown_path("write-back-nested-downgrade");
    fs::write(&path, FIXTURE.replace('\n', "\r\n")).expect("write fixture");
    let cleanup = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup);
    });

    let document = encoding::load_document(&path).expect("read fixture");
    let open_path = path.clone();
    let (editor, cx) = cx.add_window_view(move |_window, cx| {
        Editor::from_loaded_document(cx, document, Some(open_path))
    });
    redraw(cx);

    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| {
            let nested = editor
                .document
                .visible_blocks()
                .get(2)
                .map(|visible| visible.entity.clone())
                .expect("夹具里应有嵌套的列表项");
            editor.on_block_event(
                nested,
                &BlockEvent::RequestDowngradeNestedListItemToChildParagraph,
                cx,
            );
        });
    });
    redraw(cx);

    let buffer_text = editor.read_with(cx, |editor, _cx| editor.buffer.text());
    assert!(
        buffer_text.starts_with("段落文字\n\n- 甲\n"),
        "降级把列表之前或首项的字节改了：{buffer_text:?}"
    );
    assert!(
        buffer_text.contains("| 名称 | 数量 |\n| ---- | ---- |\n| 甲   | 1    |"),
        "降级把表格的列宽填充重算了：{buffer_text:?}"
    );
    assert!(
        buffer_text.ends_with("强调 __下划线__ 结尾\n"),
        "降级把后面的块规范化了，或丢了末行换行：{buffer_text:?}"
    );
    let (spans, buffer_text) = present_root_spans(&editor, cx);
    assert_spans_tile_the_content(&spans, &buffer_text, "嵌套列表项降级之后");
}

/// 段落并进引用容器：合并之后区间必须还跟得上缓冲区。
///
/// 容器里的块没有自己的源码区间，写回时拿的是整根块的区间——一旦那个区间过期，
/// `buffer.edit` 就会拿一个比缓冲区还长的区间来写（实测越界 panic）。
#[gpui::test]
async fn merging_a_paragraph_into_a_quote_container_keeps_the_spans_valid(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "> hello\n\nworld".to_string(), None)
    });
    redraw(cx);

    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| {
            let target = editor
                .document
                .root_blocks()
                .iter()
                .find(|root| root.read(cx).record.title.visible_text() == "world")
                .cloned()
                .expect("夹具里应有 world 段");
            let content = target.read(cx).record.title.clone();
            editor.focus_block(target.entity_id());
            target.update(cx, |block, cx| block.move_to(0, cx));
            editor.on_block_event(target, &BlockEvent::RequestMergeIntoPrev { content }, cx);
        });
    });
    redraw(cx);

    let buffer_text = editor.read_with(cx, |editor, _cx| editor.buffer.text());
    assert!(buffer_text.contains("world"), "合并没进缓冲区：{buffer_text:?}");
    let (spans, buffer_text) = present_root_spans(&editor, cx);
    assert_spans_tile_the_content(&spans, &buffer_text, "段落并进引用容器之后");
}

/// 拆块的接缝只该往文件里插换行：整块文本一个字都不重贴。
///
/// 块在光标处把自己切成两半时，先有一次 Changed 把「切掉的后半截」写成一次删除，
/// 于是这一块在缓冲区里塌成零宽；随后的结构写回没有区间可用，只能整篇重投影——
/// 表里、段里的每个 `__下划线__` 都被顺手规范掉。这一条钉住的是那一步：文件与拆块
/// 前相比只多了接缝的几个换行，块里其余字节（含另一行的写法）与块外的块全部原样。
/// 至于接缝该多吃一个换行还是少吃一个，是拆块语义的问题，另条测试盯着。
#[gpui::test]
async fn splitting_a_wrapped_paragraph_inserts_only_the_seam_newlines(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);

    const FIXTURE: &str = concat!(
        "段落\n",
        "\n",
        "第一行文字\n第二行文字\n",
        "\n",
        "强调 __下划线__ 结尾\n",
    );

    let path = temp_markdown_path("write-back-split-seam");
    fs::write(&path, FIXTURE.replace('\n', "\r\n")).expect("write fixture");
    let cleanup = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup);
    });

    let document = encoding::load_document(&path).expect("read fixture");
    let open_path = path.clone();
    let (editor, cx) = cx.add_window_view(move |_window, cx| {
        Editor::from_loaded_document(cx, document, Some(open_path))
    });
    redraw(cx);

    let wrapped = editor.read_with(cx, |editor, cx| {
        editor
            .document
            .visible_blocks()
            .iter()
            .find(|visible| visible.entity.read(cx).display_text() == "第一行文字\n第二行文字")
            .map(|visible| visible.entity.clone())
            .expect("夹具里应有一跨行的段落")
    });
    cx.update(|_window, cx| {
        wrapped.update(cx, |block, _cx| block.selected_range = 15..15);
    });
    cx.update(|window, cx| {
        wrapped.update(cx, |block, cx| block.on_newline(&Newline, window, cx));
    });
    redraw(cx);

    let buffer_text = editor.read_with(cx, |editor, _cx| editor.buffer.text());
    assert_only_newlines_inserted(FIXTURE, &buffer_text, "跨行段落拆块");
    // 块里那一行用户没碰过的写法（`__下划线__`）必须还是原来那几个字节。
    assert!(
        buffer_text.ends_with("强调 __下划线__ 结尾\n"),
        "拆块把相邻块的写法规范掉了：{buffer_text:?}"
    );
    let (spans, buffer_text) = present_root_spans(&editor, cx);
    assert_spans_tile_the_content(&spans, &buffer_text, "拆行段落之后");
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(
            editor.source_serializations.get(),
            0,
            "拆块触发了整篇重新序列化"
        );
    });
}

/// `after` 必须是 `before` 在某一处**只插入若干换行**得到的：不许有改写，也不许有删除。
fn assert_only_newlines_inserted(before: &str, after: &str, label: &str) {
    let common_prefix = before
        .bytes()
        .zip(after.bytes())
        .take_while(|(old, new)| old == new)
        .count();
    let prefix = (0..=common_prefix)
        .rev()
        .find(|count| before.is_char_boundary(*count) && after.is_char_boundary(*count))
        .unwrap_or(0);
    let rest_before = &before[prefix..];
    let rest_after = &after[prefix..];
    let common_suffix = rest_before
        .bytes()
        .rev()
        .zip(rest_after.bytes().rev())
        .take_while(|(old, new)| old == new)
        .count();
    let suffix = (0..=common_suffix)
        .rev()
        .find(|count| {
            rest_before.is_char_boundary(rest_before.len() - count)
                && rest_after.is_char_boundary(rest_after.len() - count)
        })
        .unwrap_or(0);
    let deleted = &rest_before[..rest_before.len() - suffix];
    let inserted = &rest_after[..rest_after.len() - suffix];
    assert_eq!(
        deleted, "",
        "{label} 改写了原有字节（删掉了 {deleted:?}，插入了 {inserted:?}）\n    之前: {before:?}\n    之后: {after:?}"
    );
    assert!(
        inserted.bytes().all(|byte| byte == b'\n'),
        "{label} 插入的不只是接缝换行，而是 {inserted:?}"
    );
}

/// 行中拆块：文件里多出来的应该只有「结束这一行 + 一个分隔空行」这两个换行。
#[gpui::test]
async fn splitting_a_paragraph_mid_line_inserts_the_block_break(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    const FIXTURE: &str = concat!(
        "段落\n",
        "\n",
        "第一段文字\n",
        "\n",
        "强调 __下划线__ 结尾\n",
    );

    let path = temp_markdown_path("write-back-split-midline");
    fs::write(&path, FIXTURE.replace('\n', "\r\n")).expect("write fixture");
    let cleanup = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup);
    });

    let document = encoding::load_document(&path).expect("read fixture");
    let open_path = path.clone();
    let (editor, cx) = cx.add_window_view(move |_window, cx| {
        Editor::from_loaded_document(cx, document, Some(open_path))
    });
    redraw(cx);

    let target = editor.read_with(cx, |editor, cx| {
        editor
            .document
            .visible_blocks()
            .iter()
            .find(|visible| visible.entity.read(cx).display_text() == "第一段文字")
            .map(|visible| visible.entity.clone())
            .expect("夹具里应有「第一段文字」")
    });
    cx.update(|_window, cx| {
        target.update(cx, |block, _cx| block.selected_range = 6..6);
    });
    cx.update(|window, cx| {
        target.update(cx, |block, cx| block.on_newline(&Newline, window, cx));
    });
    redraw(cx);

    let buffer_text = editor.read_with(cx, |editor, _cx| editor.buffer.text());
    assert_eq!(
        buffer_text,
        "段落\n\n第一\n\n段文字\n\n强调 __下划线__ 结尾\n",
        "行中拆块重贴了整块文本：{buffer_text:?}"
    );
    let (spans, buffer_text) = present_root_spans(&editor, cx);
    assert_spans_tile_the_content(&spans, &buffer_text, "行中拆块之后");
}
