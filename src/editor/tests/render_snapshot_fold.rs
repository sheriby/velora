use super::common::*;

#[gpui::test]
async fn render_structure_snapshot_for_key_blocks(cx: &mut TestAppContext) {
    // roadmap G7：关键块渲染结构的黄金快照。任何解析/渲染回归改动若
    // 改变块序列或文本，需同步更新此快照并在 PR 中说明。
    let source = "# Title\n\nBody with **bold**, `code` and [link](https://x).\n\n- one\n- two\n\n- [ ] task\n\n> quoted\n\n```rust\nlet x = 1;\n```\n\n| a | b |\n| --- | --- |\n| 1 | 2 |\n";
    let editor = cx.new(|cx| Editor::from_markdown(cx, source.into(), None));

    editor.read_with(cx, |editor, cx| {
        let visible = editor.document.visible_blocks();
        let snapshot: Vec<(String, String)> = visible
            .iter()
            .map(|visible| {
                let block = visible.entity.read(cx);
                (
                    format!("{:?}", block.kind()),
                    block.display_text().to_string(),
                )
            })
            .collect();

        let expected = vec![
            ("Heading { level: 1 }".to_string(), "Title".to_string()),
            (
                "Paragraph".to_string(),
                "Body with bold, code and link.".to_string(),
            ),
            (
                "BulletedListItem".to_string(),
                "one".to_string(),
            ),
            (
                "BulletedListItem".to_string(),
                "two".to_string(),
            ),
            // 列表组与下一块之间的空段落分隔（设计使然）。
            ("Paragraph".to_string(), String::new()),
            (
                "TaskListItem { checked: false }".to_string(),
                "task".to_string(),
            ),
            ("Quote".to_string(), "quoted".to_string()),
            (
                "CodeBlock { language: Some(\"rust\") }".to_string(),
                "let x = 1;".to_string(),
            ),
            // 表格内容由 table runtime 渲染，display_text 为空（设计使然）。
            ("Table".to_string(), String::new()),        ];
        assert_eq!(snapshot, expected, "render structure snapshot mismatch");
    });
}

#[gpui::test]
async fn heading_fold_hides_section_content(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let source = "## Section\n\nalpha\n\nbeta\n\n## Next\n\ngamma";
    let editor = cx.new(|cx| Editor::from_markdown(cx, source.into(), None));

    editor.update(cx, |editor, cx| {
        let visible = editor.document.visible_blocks();
        assert_eq!(visible.len(), 5); // H, alpha, beta, H2, gamma
        let heading = visible[0].entity.clone();
        heading.update(cx, |block, _cx| block.folded = true);

        let filtered = editor
            .apply_heading_fold_filter(
                editor.document.visible_blocks().to_vec(),
                cx,
            )
            .iter()
            .map(|visible| visible.entity.read(cx).display_text().to_string())
            .collect::<Vec<_>>();

        // 折叠章节内容 alpha/beta 被隐藏，下一同级标题保持可见。
        assert_eq!(
            filtered,
            vec!["Section".to_string(), "Next".to_string(), "gamma".to_string()]
        );
    });
}

#[gpui::test]
async fn heading_fold_chevron_marks_only_foldable_headings(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let source = "## Section\n\nalpha\n\n### Child\n\nbeta\n\n## Empty\n\n## Next\n\ngamma";
    let editor = cx.new(|cx| Editor::from_markdown(cx, source.into(), None));

    editor.update(cx, |editor, cx| {
        editor.apply_heading_fold_filter(editor.document.visible_blocks().to_vec(), cx);
        let visible = editor.document.visible_blocks().to_vec();
        let foldable = visible
            .iter()
            .filter(|visible| matches!(visible.entity.read(cx).kind(), BlockKind::Heading { .. }))
            .map(|visible| visible.entity.read(cx).foldable)
            .collect::<Vec<_>>();
        // Section / Child / Next 后方有章节内容；Empty 紧跟同级标题，没有可折叠内容。
        assert_eq!(foldable, vec![true, true, false, true]);
    });
}

#[gpui::test]
async fn broken_image_placeholder_stays_compact_inside_the_column(cx: &mut TestAppContext) {
    // 用户报修：callout 列表项里的图片读不出来时，占位框按「视口估算宽度」画成
    // 一条横穿整屏的空心条，冲出 callout 右边界。占位框现在贴着文字收紧，
    // 上限只到所在列的可用宽度。
    init_editor_test_app(cx);
    let markdown = "> [!IMPORTANT] 混合块\n>\n> - bold\n> - ![image](missing-image.png)\n";
    let (_editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, markdown.into(), None));
    // 宽窗口：旧实现按视口估算出 definite 宽度，占位框会拉成一条空心长条。
    cx.update(|window, _cx| window.resize(gpui::size(px(1400.0), px(900.0))));
    redraw(cx);

    let viewport_width = cx.update(|window, _cx| window.viewport_size().width);
    let bounds = cx
        .debug_bounds("image-placeholder")
        .expect("broken image should render a placeholder box");
    assert!(
        bounds.size.width <= px(400.0),
        "占位框应贴着文字收紧，实测宽度 {:?}",
        bounds.size.width
    );
    assert!(
        bounds.right() < viewport_width,
        "占位框右边 {:?} 不应超出视口宽度 {viewport_width:?}",
        bounds.right()
    );
}

