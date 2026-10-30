//! 源码模式的 markdown 语法高亮：分块高亮接得上、编辑后级联重串。

use super::common::*;
use crate::components::markdown::code_highlight::{CodeHighlightClass, CodeHighlightSpan};
use crate::components::markdown::source_highlight::MarkdownSourceState;

fn heading_spans<'a>(spans: &'a [CodeHighlightSpan]) -> Vec<&'a CodeHighlightSpan> {
    spans
        .iter()
        .filter(|span| matches!(span.class, CodeHighlightClass::MarkdownHeading(_)))
        .collect()
}

/// 切到源码视图后：markdown 分块走手写高亮管线，标题整行有着色。
#[gpui::test]
async fn source_view_markdown_blocks_get_syntax_highlight(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let source = "# 标题甲\n\n正文一段。\n\n- 列表项\n".to_string();
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, source, None));
    redraw(cx);

    editor.update(cx, |editor, cx| editor.toggle_view_mode(cx));
    redraw(cx);

    editor.read_with(cx, |editor, cx| {
        assert_eq!(editor.view_mode, ViewMode::Source);
        let roots = editor.document.root_blocks();
        assert_eq!(roots.len(), 1, "短文档整篇是一根源码分块");
        let block = roots[0].read(cx);
        let result = block
            .code_highlight_result()
            .expect("markdown 源码分块应有高亮结果");
        let text = block.display_text();
        let title_end = text.find("标题甲").expect("标题正文") + "标题甲".len();
        assert!(
            heading_spans(&result.spans).iter().any(|span| {
                span.range.start <= title_end && title_end <= span.range.end
            }),
            "标题行应被标题 span 覆盖: {:?}",
            result.spans
        );
        // 普通段落不该有标题色。
        let body_start = text.find("正文一段").expect("正文");
        assert!(
            !result.spans.iter().any(|span| {
                matches!(span.class, CodeHighlightClass::MarkdownHeading(_))
                    && span.range.start <= body_start
                    && body_start < span.range.end
            }),
            "普通段落不该有标题色: {:?}",
            result.spans
        );
    });
}

/// 围栏跨过 512 行分块的接缝：续块的入口状态带着语言，内容接着按围栏语言着色。
#[gpui::test]
async fn fence_crossing_chunk_seam_stays_highlighted(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let chunk = crate::editor::file_drop::SOURCE_DOCUMENT_CHUNK_LINES;
    let mut source = String::from("```python\n");
    // 开栏在第一块末尾附近，内容一直铺到第二块里。
    while source.matches('\n').count() < chunk + 3 {
        source.push_str("x = 1\n");
    }
    source.push_str("```\n正文\n");
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, source, None));
    redraw(cx);

    editor.update(cx, |editor, cx| editor.toggle_view_mode(cx));
    redraw(cx);

    editor.read_with(cx, |editor, cx| {
        let roots = editor.document.root_blocks();
        assert!(roots.len() >= 2, "夹具该跨块：{} 块", roots.len());
        let second = roots[1].read(cx);
        assert_eq!(
            second.source_fence_entry(),
            Some(MarkdownSourceState::Fence {
                fence_char: '`',
                fence_len: 3,
                language: Some(crate::components::CodeLanguageKey::Python),
            }),
            "第二块的入口状态该是未闭合的 python 围栏"
        );
        let result = second
            .code_highlight_result()
            .expect("markdown 源码分块应有高亮结果");
        assert!(
            result
                .spans
                .iter()
                .any(|span| span.class == CodeHighlightClass::Variable),
            "接缝之后的围栏内容该按 python 着色: {:?}",
            result.spans
        );
    });
}

/// 在上一块里补上闭合围栏：下一块的入口状态清掉，内容不再按围栏语言着色。
#[gpui::test]
async fn closing_a_fence_cascades_state_to_following_chunks(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let chunk = crate::editor::file_drop::SOURCE_DOCUMENT_CHUNK_LINES;
    let mut source = String::from("```python\n");
    while source.matches('\n').count() < chunk + 2 {
        source.push_str("x = 1\n");
    }
    // 到此为止没有闭合围栏：第二块整体都在栏里。
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, source, None));
    redraw(cx);

    editor.update(cx, |editor, cx| editor.toggle_view_mode(cx));
    redraw(cx);

    // 在第一块末尾补一行闭合围栏（经块自己的文本替换路径，触发编辑器级联）。
    let first = editor.read_with(cx, |editor, _cx| {
        editor.document.root_blocks().first().cloned().expect("首块")
    });
    first.update(cx, |block, block_cx| {
        let end = block.visible_len();
        block.prepare_undo_capture(UndoCaptureKind::NonCoalescible, block_cx);
        block.replace_text_in_visible_range(end..end, "\n```", None, false, block_cx);
    });
    redraw(cx);

    editor.read_with(cx, |editor, cx| {
        let roots = editor.document.root_blocks();
        let second = roots[1].read(cx);
        assert_eq!(
            second.source_fence_entry(),
            None,
            "闭合之后第二块的入口状态该清掉"
        );
        let result = second
            .code_highlight_result()
            .expect("markdown 源码分块应有高亮结果");
        assert!(
            !result
                .spans
                .iter()
                .any(|span| span.class == CodeHighlightClass::Variable),
            "围栏闭合后第二块内容不该再按 python 着色: {:?}",
            result.spans
        );
    });
}
/// 渲染 ↔ 源码切一个来回：块树整个换过，高亮与接缝状态要跟着重建，
/// 不能留着渲染态块上的旧缓存。
#[gpui::test]
async fn toggling_view_modes_back_and_forth_keeps_highlighting(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let source = "# 标题甲\n\n```rust\nfn main() {}\n```\n".to_string();
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, source, None));
    redraw(cx);

    for _ in 0..2 {
        editor.update(cx, |editor, cx| editor.toggle_view_mode(cx));
        redraw(cx);

        editor.read_with(cx, |editor, cx| {
            assert_eq!(editor.view_mode, ViewMode::Source);
            let block = editor
                .document
                .root_blocks()
                .first()
                .cloned()
                .expect("源码视图整篇一根块");
            let result = block
                .read(cx)
                .code_highlight_result()
                .expect("markdown 源码分块应有高亮结果");
            let text = block.read(cx).display_text();
            let title_end = text.find("标题甲").expect("标题") + "标题甲".len();
            assert!(
                result.spans.iter().any(|span| {
                    matches!(span.class, CodeHighlightClass::MarkdownHeading(_))
                        && span.range.start <= title_end
                        && title_end <= span.range.end
                }),
                "往返之后标题色还在: {:?}",
                result.spans
            );
        });

        editor.update(cx, |editor, cx| editor.toggle_view_mode(cx));
        redraw(cx);
        editor.read_with(cx, |editor, _cx| {
            assert_eq!(editor.view_mode, ViewMode::Rendered);
        });
    }
}

/// `<!-->` 与 `<!--->` 在 HTML 规格里是完整的空注释：不能当成跨行注释的
/// 开头，否则后面的正文从下一行起整片涂成注释色，而渲染视图里它只是普通
/// 段落——两侧大面积串色。
#[gpui::test]
async fn self_closing_html_comments_do_not_swallow_following_lines(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    for marker in ["<!-->", "<!--->"] {
        let source = format!("{marker}\n普通正文\n");
        let (editor, cx) = cx.add_window_view(move |_window, cx| {
            Editor::from_markdown(cx, source.clone(), None)
        });
        redraw(cx);
        editor.update(cx, |editor, cx| editor.toggle_view_mode(cx));
        redraw(cx);
        editor.read_with(cx, |editor, cx| {
            let block = editor.document.root_blocks()[0].read(cx);
            let result = block.code_highlight_result().expect("高亮结果");
            let text = block.display_text();
            let body = text.find("普通正文").expect("正文");
            assert!(
                !result.spans.iter().any(|span| {
                    matches!(span.class, CodeHighlightClass::Comment)
                        && span.range.start <= body
                        && body < span.range.end
                }),
                "{marker} 之后的正文不该是注释色: {:?}",
                result.spans
            );
            let marker_at = text.find(marker).expect("标记");
            assert!(
                result.spans.iter().any(|span| {
                    matches!(span.class, CodeHighlightClass::Comment)
                        && span.range.start <= marker_at
                        && marker_at < span.range.end
                }),
                "{marker} 本身仍应是注释色: {:?}",
                result.spans
            );
        });
    }
}

/// setext 标题恰好跨 512 行分块接缝：正文行是前一块最后一行、下划线行是后一
/// 块首行。接缝状态要带过「上一行是段落候选」，下划线才不会被当成分隔线。
#[gpui::test]
async fn setext_heading_across_a_chunk_seam_keeps_heading_color(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let chunk = crate::editor::file_drop::SOURCE_DOCUMENT_CHUNK_LINES;
    let mut source = "x\n".repeat(chunk - 1);
    source.push_str("标题\n=====\n正文\n");
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, source, None));
    redraw(cx);
    editor.update(cx, |editor, cx| editor.toggle_view_mode(cx));
    redraw(cx);
    editor.read_with(cx, |editor, cx| {
        let roots = editor.document.root_blocks();
        assert!(roots.len() >= 2, "夹具该跨块：{} 块", roots.len());
        let second = roots[1].read(cx);
        let result = second
            .code_highlight_result()
            .expect("第二块应有高亮结果");
        let text = second.display_text();
        let underline = text.find("=====").expect("下划线行落在第二块");
        assert!(
            result.spans.iter().any(|span| {
                matches!(span.class, CodeHighlightClass::MarkdownHeading(1))
                    && span.range.start <= underline
                    && underline < span.range.end
            }),
            "接缝后的下划线行应出标题色: {:?}",
            result.spans
        );
    });
}

/// 引用块里的围栏代码（`> ```rust`）要进状态机：开栏后逐行按代码色着色，
/// 闭合后引用继续按普通行分类。此前引用续行扫描没有围栏分支，整段只有零散
/// 行内构造色，与渲染视图（引用内完整代码块）不一致。
#[gpui::test]
async fn quoted_fence_content_gets_code_color(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let source = "> ```rust\n> let x = 1;\n> ```\n> 尾注\n";
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, source.into(), None));
    redraw(cx);
    editor.update(cx, |editor, cx| editor.toggle_view_mode(cx));
    redraw(cx);
    editor.read_with(cx, |editor, cx| {
        let block = editor.document.root_blocks()[0].read(cx);
        let text = block.display_text();
        let result = block
            .code_highlight_result()
            .expect("源码分块应有高亮结果");
        let code_at = text.find("let x = 1;").expect("围栏内容");
        assert!(
            result.spans.iter().any(|span| {
                span.class == CodeHighlightClass::MarkdownCode
                    && span.range.start <= code_at
                    && code_at < span.range.end
            }),
            "引用内围栏内容应着代码色: {:?}",
            result.spans
        );
        let tail_at = text.find("尾注").expect("尾注");
        assert!(
            !result.spans.iter().any(|span| {
                span.class == CodeHighlightClass::MarkdownCode
                    && span.range.start <= tail_at
                    && tail_at < span.range.end
            }),
            "闭合后的引用正文不该继续代码色: {:?}",
            result.spans
        );
    });
}
