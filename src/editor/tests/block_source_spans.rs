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
fn root_block_spans(
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

fn span_ranges(spans: &[(std::ops::Range<usize>, String)]) -> Vec<std::ops::Range<usize>> {
    spans.iter().map(|(span, _)| span.clone()).collect()
}

fn rendered_blocks(spans: &[(std::ops::Range<usize>, String)]) -> String {
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
fn assert_spans_tile_the_content(
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
