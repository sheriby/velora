//! G8 渐进导入的功能正确性：分块任务边界与整篇导入逐字节一致、结构编辑
//! 冲掉挂起任务、流式导入完整落盘。墙钟预算那条（打开耗时）在
//! `perf_budgets.rs`，随整族性能闸门默认 `#[ignore]`。
use super::common::*;

/// roadmap G8：分块导入的任务边界必须与一次整篇导入逐字节一致。
///
/// 小预算把一份小文档切成很多块，覆盖空白行串、列表/段落紧邻、定界符、
/// 表格、围栏、公式、前导 frontmatter 等边界。
#[gpui::test]
async fn progressive_import_matches_single_pass_import(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let markdown = progressive_import_fixture();

    let baseline = progressive_test_editor(cx, markdown.clone(), usize::MAX);
    let (expected_text, expected_blocks, expected_raw) = cx.read(|cx| {
        baseline.read_with(cx, |editor, cx| {
            assert!(editor.document.pending_tail().is_none());
            (
                editor.document.markdown_text(cx),
                editor.document.visible_blocks().len(),
                editor.document.raw_source_text(cx),
            )
        })
    });

    for budget in [1, 2, 3, 5, 8] {
        let editor = progressive_test_editor(cx, markdown.clone(), budget);
        // 未建完时文本就已完整到可保存：已建块是序列化结果，尾段是逐行原文，
        // 重新导入这份文本必须得到与整篇导入相同的文档（保存不丢内容）。
        let mid_stream = cx.read(|cx| {
            editor.read_with(cx, |editor, cx| {
                assert!(editor.document.pending_tail().is_some());
                editor.document.markdown_text(cx)
            })
        });
        assert!(mid_stream.contains("末尾段落"), "预算 {budget}：尾段内容缺失");
        let reparsed = progressive_test_editor(cx, mid_stream, usize::MAX);
        let reparsed_text = cx.read(|cx| {
            reparsed.read_with(cx, |editor, cx| editor.document.markdown_text(cx))
        });
        assert_eq!(
            reparsed_text, expected_text,
            "预算 {budget}：未建完的文本重新导入后与整篇导入不一致"
        );

        let deadline = Instant::now() + Duration::from_secs(30);
        while cx
            .read(|cx| editor.read_with(cx, |editor, _cx| editor.document.pending_tail().is_some()))
        {
            assert!(Instant::now() < deadline, "预算 {budget}：续建未完成");
            cx.run_until_parked();
        }

        cx.read(|cx| {
            editor.read_with(cx, |editor, cx| {
                assert_eq!(
                    editor.document.visible_blocks().len(),
                    expected_blocks,
                    "预算 {budget}：块数与整篇导入不一致"
                );
                assert_eq!(
                    editor.document.markdown_text(cx),
                    expected_text,
                    "预算 {budget}：建完后文本与整篇导入不一致"
                );
                assert_eq!(
                    editor.document.raw_source_text(cx),
                    expected_raw,
                    "预算 {budget}：原文视图与整篇导入不一致"
                );
            })
        });
    }
}

/// roadmap G8：结构编辑（真实的块插入入口）必须先补建完剩余块再改树。
#[gpui::test]
async fn structural_edit_flushes_the_pending_import(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let markdown = progressive_import_fixture();
    let baseline = progressive_test_editor(cx, markdown.clone(), usize::MAX);
    let baseline_count = cx.read(|cx| {
        baseline.read_with(cx, |editor, _cx| editor.document.visible_blocks().len())
    });

    let editor = progressive_test_editor(cx, markdown.clone(), 2);
    cx.read(|cx| {
        editor.read_with(cx, |editor, _cx| {
            assert!(editor.document.pending_tail().is_some());
        })
    });

    cx.update(|cx| {
        editor.update(cx, |editor, cx| {
            let block = Editor::new_block(cx, BlockRecord::paragraph("新插入的块"));
            editor.document.insert_blocks_at(None, 0, vec![block], cx);
        });
    });

    cx.read(|cx| {
        editor.read_with(cx, |editor, cx| {
            let text = editor.document.markdown_text(cx);
            assert!(
                editor.document.pending_tail().is_none(),
                "结构编辑后不应再有挂起的尾段"
            );
            assert!(text.contains("新插入的块"));
            assert!(text.contains("末尾段落"), "尾段内容在结构编辑后丢失");
            assert_eq!(
                editor.document.visible_blocks().len(),
                baseline_count + 1,
                "补建 + 插入后的块数不符"
            );
        })
    });
}

/// roadmap G8：超大文档续建完成后，保存到磁盘的仍是完整文本。
#[gpui::test]
async fn streamed_document_saves_complete_text_to_disk(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let markdown = progressive_import_fixture();
    let path = temp_markdown_path("progressive-streamed-save");
    fs::write(&path, &markdown).expect("write fixture");
    let cleanup_path = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup_path);
    });

    let expected_source = markdown.clone();
    let (editor, cx) = cx.add_window_view({
        let path = path.clone();
        move |_window, cx| {
            Editor::from_markdown_with_chunk_budget(cx, markdown.clone(), Some(path), 2)
        }
    });
    redraw(cx);
    cx.read(|cx| {
        editor.read_with(cx, |editor, _cx| {
            assert!(
                editor.document.pending_tail().is_none(),
                "窗口打开后续建任务应已跑完"
            );
        })
    });

    cx.simulate_input(" x");
    redraw(cx);
    editor.read_with(cx, |editor, _cx| {
        assert!(editor.document_dirty);
    });

    cx.dispatch_action(SaveDocument);
    redraw(cx);
    // 新语义：保存写缓冲区，落盘的就是「原文件 + 那一处改动」。
    // 旧断言拿整篇重新序列化的结果来比，等于默认了未编辑的块可以被改写。
    let saved = fs::read_to_string(&path).expect("read saved markdown");
    assert_eq!(
        saved,
        format!(" x{expected_source}"),
        "续建的文档保存后不是「原文 + 一处改动」"
    );
    assert!(saved.contains("末尾段落"), "续建后的文档缺少尾段");
    editor.read_with(cx, |editor, _cx| {
        assert!(!editor.document_dirty);
    });
}

fn progressive_test_editor(
    cx: &mut TestAppContext,
    markdown: String,
    chunk_budget: usize,
) -> gpui::Entity<Editor> {
    cx.update(|cx| {
        cx.new(|cx| {
            Editor::from_markdown_with_chunk_budget(cx, markdown.clone(), None, chunk_budget)
        })
    })
}

/// 一份刻意包含各种任务边界的短文档：frontmatter、懒惰续行、未闭合反引号、
/// 空行串、列表与段落紧邻、有序列表、引用、围栏、表格、定界符、缩进代码、
/// 公式、任务项、结尾无换行。
fn progressive_import_fixture() -> String {
    [
        "---",
        "title: 边界",
        "---",
        "",
        "开头段落",
        "紧跟的第二行",
        "",
        "# 标题",
        "正文 `未闭合的反引号",
        "",
        "后面的行补上闭合 `",
        "",
        "- 一",
        "- 二",
        "  - 嵌套项",
        "",
        "   ",
        "",
        "1. 甲",
        "2. 乙",
        "",
        "> 引用",
        "> 续行",
        "",
        "```rust",
        "fn main() {}",
        "```",
        "",
        "| a | b |",
        "| --- | --- |",
        "| 1 | 2 |",
        "表格后的段落",
        "",
        "标题二",
        "===",
        "",
        "段落紧邻列表",
        "- 紧邻项",
        "",
        "---",
        "中段分隔线后的内容",
        "---",
        "",
        "",
        "",
        "   缩进代码",
        "",
        "$$",
        "x^2",
        "$$",
        "",
        "- [ ] 任务",
        "",
        "1. 续号甲",
        "2. 续号乙",
        "",
        "末尾段落",
    ]
    .join("\n")
}

