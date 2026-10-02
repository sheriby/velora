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

/// 结构变更（回车拆块）没声明区间：整篇重投影必须让缓冲区重新跟上块树。
#[gpui::test]
async fn a_structural_edit_resyncs_the_buffer_and_reanchors_every_block(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);

    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, LOSSY_SHAPE_FIXTURE.to_string(), None)
    });
    redraw(cx);

    cx.dispatch_action(Newline);
    redraw(cx);

    // 重投影之后没被序列化记到的空块还没有区间（空行归分隔符），所以这里容忍 None；
    // 但已经挂上的区间必须仍然互不重叠、且只含本块内容。
    let (spans, buffer_text) = present_root_spans(&editor, cx);
    assert!(
        buffer_text.contains("\n段落文字"),
        "回车拆块没进缓冲区：{buffer_text:?}"
    );
    assert_spans_tile_the_content(&spans, &buffer_text, "结构变更之后");
    editor.read_with(cx, |editor, cx| {
        assert!(
            editor.document.root_count() > 3,
            "拆块后根块数应该增加"
        );
        // 兜底档位的定义：缓冲区与块树的序列化一致。
        assert_eq!(editor.buffer.text(), editor.document.markdown_text(cx));
    });
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
