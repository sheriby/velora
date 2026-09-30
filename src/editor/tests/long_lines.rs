use super::common::*;


// ===== 超长行折叠（JSONL / 日志类文档性能）=====

/// 打开一个带超长行的代码文档，返回编辑器实体。3 行：短 / 2000 字符长行 / 短。
fn long_line_code_source() -> String {
    format!(
        "short line\n{}\nanother short line\n",
        "x".repeat(2000)
    )
}

/// 把源码写成临时 .log 并以代码文档模式开窗（源码文档按行分块、带行号槽）。
fn open_code_document_window<'a>(
    cx: &'a mut gpui::TestAppContext,
    source: &str,
    name: &str,
) -> (gpui::Entity<crate::editor::Editor>, &'a mut gpui::VisualTestContext) {
    let path =
        std::env::temp_dir().join(format!("velora-long-line-{name}-{}.log", std::process::id()));
    std::fs::write(&path, source).expect("write fixture");
    let source = source.to_string();
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        let mut editor = crate::editor::Editor::from_markdown(cx, String::new(), None);
        editor.replace_document_from_code_source(source, path, cx);
        editor
    });
    cx.run_until_parked();
    (editor, cx)
}

#[gpui::test]
async fn long_source_lines_collapse_to_single_unwrapped_row(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let source = long_line_code_source();
    let (editor, cx) = open_code_document_window(cx, &source, "collapse");

    redraw(cx);
    editor
        .read_with(cx, |editor, cx| {
            let block = editor.document.visible_blocks()[0].entity.read(cx);
            let lines = block.last_layout.as_ref().expect("应完成排版");
            assert_eq!(
                lines.len(),
                4,
                "源文本带行尾换行 = 3 可见行 + 1 空尾行；layout 条目必须与源行范围表一一对应，\
                 超长行折叠成单行、不允许换行炸高"
            );
            for (idx, line) in lines.iter().enumerate() {
                assert!(
                    line.wrap_boundaries().is_empty(),
                    "折叠态第 {idx} 行不应有软换行"
                );
            }
            // 长行单行宽度必须超过视口（否则说明没走不换行路径）。
            let long_width = lines[1].width();
            assert!(
                long_width > gpui::px(800.0),
                "长行应保持单行完整宽度，实际 {long_width:?}"
            );
            assert!(
                !block.expanded_long_lines.contains(&1),
                "默认折叠"
            );
        });
}

#[gpui::test]
async fn gutter_click_expands_long_line_into_wrapped_rows(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let source = long_line_code_source();
    let (editor, cx) = open_code_document_window(cx, &source, "expand");

    redraw(cx);
    // 点行号槽（text_bounds 左侧的 gutter 区）第 2 行（超长行）。
    let click_position = editor.read_with(cx, |editor, cx| {
        let block = editor.document.visible_blocks()[0].entity.read(cx);
        let bounds = block.last_bounds.as_ref().expect("应已布局");
        gpui::point(
            bounds.left() - block.last_gutter_width / 2.0,
            bounds.top() + block.last_line_height * 1.5,
        )
    });
    let toggled = editor.update(cx, |editor, cx| {
        let block = editor.document.visible_blocks()[0].entity.clone();
        block.update(cx, |block, _block_cx| block.toggle_long_line_at_gutter(click_position))
    });
    assert!(toggled, "点行号槽应切换超长行");
    redraw(cx);

    editor
        .read_with(cx, |editor, cx| {
            let block = editor.document.visible_blocks()[0].entity.read(cx);
            assert!(block.expanded_long_lines.contains(&1), "展开状态应记录");
            let lines = block.last_layout.as_ref().expect("应完成排版");
            assert_eq!(lines.len(), 4, "展开不改变源行条目数（含空尾行）");
            assert!(
                !lines[1].wrap_boundaries().is_empty(),
                "展开后长行应按容器宽换行"
            );
            let short_height = lines[0].size(block.last_line_height).height;
            let long_height = lines[1].size(block.last_line_height).height;
            assert!(
                long_height > short_height * 2.0,
                "展开后的长行应显著高于单行（{long_height:?} vs {short_height:?}）"
            );

            // 再点一次收起。
        });

    let click_position = editor.read_with(cx, |editor, cx| {
        let block = editor.document.visible_blocks()[0].entity.read(cx);
        let bounds = block.last_bounds.as_ref().expect("应已布局");
        gpui::point(
            bounds.left() - block.last_gutter_width / 2.0,
            bounds.top() + block.last_line_height * 1.5,
        )
    });
    let toggled = editor.update(cx, |editor, cx| {
        let block = editor.document.visible_blocks()[0].entity.clone();
        block.update(cx, |block, _block_cx| block.toggle_long_line_at_gutter(click_position))
    });
    assert!(toggled, "再次点击应能收起");
    redraw(cx);
    editor
        .read_with(cx, |editor, cx| {
            let block = editor.document.visible_blocks()[0].entity.read(cx);
            assert!(
                !block.expanded_long_lines.contains(&1),
                "收起后不应留在展开集合里"
            );
            let lines = block.last_layout.as_ref().expect("应完成排版");
            assert!(
                lines[1].wrap_boundaries().is_empty(),
                "收起后长行恢复单行"
            );
        });
}

#[gpui::test]
async fn collapsed_long_line_has_no_horizontal_scrolling(cx: &mut TestAppContext) {
    // 用户定版行为：折叠单行不许横向滚动，超出部分直接裁切，只能点行号展开。
    init_editor_test_app(cx);
    let source = long_line_code_source();
    let (editor, cx) = open_code_document_window(cx, &source, "noscroll");
    redraw(cx);

    // 整个文档树里不应存在任何代码块的横向滚动容器。
    let block_ids = editor.read_with(cx, |editor, _cx| {
        editor
            .document
            .visible_blocks()
            .iter()
            .map(|visible| visible.entity.entity_id())
            .collect::<Vec<_>>()
    });
    let mut scroll_containers = 0;
    for id in block_ids {
        // debug_bounds 只收 'static 选择器，测试里泄漏这几个短字符串无妨。
        let code_sel: &'static str = Box::leak(format!("code-x-scroll-{id}").into_boxed_str());
        let source_sel: &'static str = Box::leak(format!("source-x-scroll-{id}").into_boxed_str());
        if cx.debug_bounds(code_sel).is_some() || cx.debug_bounds(source_sel).is_some() {
            scroll_containers += 1;
        }
    }
    assert_eq!(scroll_containers, 0, "折叠单行不允许横向滚动：不应有横滚容器");

    // 文本元素宽度被钳在容器宽内（不溢出），行依然单行不换行。
    editor.read_with(cx, |editor, cx| {
        let block = editor.document.visible_blocks()[0].entity.read(cx);
        let lines = block.last_layout.as_ref().expect("应完成排版");
        assert!(
            lines[1].wrap_boundaries().is_empty(),
            "折叠行必须保持单行"
        );
        let bounds = block.last_bounds.as_ref().expect("应已布局");
        assert!(
            bounds.size.width < gpui::px(2500.0),
            "文本区应被钳在容器宽内（裁切显示），实际 {:?}",
            bounds.size
        );
    });
}

#[gpui::test]
async fn collapsed_long_line_is_truncated_to_display_cap(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let mut source = String::new();
    source.push_str(&"y".repeat(30_000));
    source.push_str("\nshort\n");
    let (editor, cx) = open_code_document_window(cx, &source, "truncate");

    redraw(cx);
    editor
        .read_with(cx, |editor, cx| {
            let block = editor.document.visible_blocks()[0].entity.read(cx);
            let lines = block.last_layout.as_ref().expect("应完成排版");
            assert_eq!(lines.len(), 3, "长行 + short + 空尾行 = 3 条目");
            let shaped_chars = lines[0].text.chars().count();
            assert!(
                shaped_chars < 21_000,
                "折叠态 3 万字符的行应截断到显示上限附近，实际 {shaped_chars}"
            );
            assert!(
                lines[0].text.contains("已截断"),
                "截断行尾应有提示：{}",
                &lines[0].text[lines[0].text.len() - 80..]
            );
            // 截断行的文本不再是原始全文，但索引映射仍以原始文本行范围表为准：
            // 点击行首得到原始偏移 0。
            let origin_index = block.index_for_mouse_position(gpui::point(
                block.last_bounds.expect("应已布局").left() + gpui::px(1.0),
                block.last_bounds.expect("应已布局").top() + gpui::px(1.0),
            ));
            assert_eq!(origin_index, 0, "点击长行行首应映射到原始文本偏移 0");
        });
}

#[gpui::test]
async fn drop_open_mode_matches_workspace_open_mode(cx: &mut TestAppContext) {
    // 拖拽与工作区树打开必须共用同一判定：只有 .md/.markdown 按 Markdown
    // 解析；.jsonl 等一律代码文档（此前拖拽走 is_code_file 白名单，.jsonl
    // 被误当 Markdown）。
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        crate::editor::Editor::from_markdown(cx, String::new(), None)
    });

    let jsonl = std::env::temp_dir().join(format!("velora-drop-{}.jsonl", std::process::id()));
    std::fs::write(&jsonl, "{\"a\":1}\n{\"a\":2}\n").expect("write jsonl");
    let markdown_ext =
        std::env::temp_dir().join(format!("velora-drop-{}.markdown", std::process::id()));
    std::fs::write(&markdown_ext, "# 标题\n\n正文\n").expect("write markdown");

    editor.update(cx, |editor, cx| {
        editor
            .replace_document_from_path(&jsonl, cx)
            .expect("jsonl should open");
    });
    editor.read_with(cx, |editor, _| {
        assert!(
            editor.code_tab_active(),
            ".jsonl 拖拽打开必须是代码文档模式"
        );
    });

    editor.update(cx, |editor, cx| {
        editor
            .replace_document_from_path(&markdown_ext, cx)
            .expect("markdown should open");
    });
    editor.read_with(cx, |editor, _| {
        assert!(
            !editor.code_tab_active(),
            ".markdown 拖拽打开必须是 Markdown 渲染模式"
        );
    });
}

