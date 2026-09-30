    use gpui::{AppContext, TestAppContext};

    
    use crate::components::{BlockKind, CalloutVariant, Editor, HtmlCssColor};
    #[gpui::test]

    async fn imports_safe_inline_html_line_as_native_html_block(cx: &mut TestAppContext) {
        let markdown = "<span style='color:blue;'>Anaconda</span>: https://example.com".to_string();
        let editor = cx.new(|cx| Editor::from_markdown(cx, markdown.clone(), None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 1);
            let block = visible[0].entity.read(cx);
            assert_eq!(block.kind(), BlockKind::HtmlBlock);
            assert_eq!(block.display_text(), markdown);
            assert_eq!(editor.document.markdown_text(cx), markdown);
        });
    }

    #[gpui::test]
    async fn imports_standalone_html_image_as_native_html_block(cx: &mut TestAppContext) {
        let markdown =
            "<img src=\"./assets/pic.png\" alt=\"alt text\" style=\"zoom:80%;\" />".to_string();
        let editor = cx.new(|cx| Editor::from_markdown(cx, markdown.clone(), None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 1);
            let block = visible[0].entity.read(cx);
            assert_eq!(block.kind(), BlockKind::HtmlBlock);
            assert_eq!(block.display_text(), markdown);
            assert_eq!(editor.document.markdown_text(cx), markdown);
        });
    }

    #[gpui::test]
    async fn imports_list_items_with_inline_span_style_as_text_not_links(cx: &mut TestAppContext) {
        let markdown = [
            "- Anaconda的安装需要留意<span style='color:blue;'>磁盘预留空间、系统环境变量</span>等问题",
            "- Pycharm的安装需要留意<span style='color:blue;'>专业版破解、python解释器关联</span>等问题",
            "- GPU版本的 Pytorch v1.5.0安装需要留意本机<span style='color:blue;'>英伟达驱动`CUDA+cuDNN`</span>",
        ]
        .join("\n");
        let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 3);
            for block in visible {
                assert_eq!(block.entity.read(cx).kind(), BlockKind::BulletedListItem);
            }

            let first = visible[0].entity.read(cx);
            let span_start = "Anaconda的安装需要留意".len();
            assert_eq!(first.inline_link_at(span_start), None);
            assert!(matches!(
                first
                    .inline_html_style_at(span_start)
                    .and_then(|style| style.color),
                Some(HtmlCssColor::Rgba(color))
                    if color.red == 0 && color.green == 0 && color.blue == 255
            ));
            assert_eq!(
                first.display_text(),
                "Anaconda的安装需要留意磁盘预留空间、系统环境变量等问题"
            );

            let third = visible[2].entity.read(cx);
            let code_start = "GPU版本的 Pytorch v1.5.0安装需要留意本机英伟达驱动".len();
            assert!(third.inline_style_at(code_start).code);
            assert_eq!(third.inline_link_at(code_start), None);
            assert!(third.inline_html_style_at(code_start).is_some());
        });
    }

    #[gpui::test]
    async fn risky_html_tag_stays_raw_markdown(cx: &mut TestAppContext) {
        let markdown = "<script>alert(1)</script>".to_string();
        let editor = cx.new(|cx| Editor::from_markdown(cx, markdown.clone(), None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 1);
            assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::RawMarkdown);
            assert_eq!(visible[0].entity.read(cx).display_text(), markdown);
            assert_eq!(editor.document.markdown_text(cx), markdown);
        });
    }

    #[gpui::test]
    async fn safe_html_with_risky_child_uses_html_block_and_preserves_source(
        cx: &mut TestAppContext,
    ) {
        let markdown = "<div>safe<script>alert(1)</script>tail</div>".to_string();
        let editor = cx.new(|cx| Editor::from_markdown(cx, markdown.clone(), None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 1);
            let block = visible[0].entity.read(cx);
            assert_eq!(block.kind(), BlockKind::HtmlBlock);
            assert!(
                block
                    .record
                    .html
                    .as_ref()
                    .is_some_and(|html| html.is_semantic())
            );
            assert_eq!(editor.document.markdown_text(cx), markdown);
        });
    }

    #[gpui::test]
    async fn imports_closed_html_comment_as_native_comment_block(cx: &mut TestAppContext) {
        let markdown = "<!--\n xxx \n-->".to_string();
        let editor = cx.new(|cx| Editor::from_markdown(cx, markdown.clone(), None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 1);
            assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::Comment);
            assert_eq!(visible[0].entity.read(cx).display_text(), markdown);
            assert_eq!(editor.document.markdown_text(cx), markdown);
        });
    }

    #[gpui::test]
    async fn html_comment_closes_at_first_marker_and_resumes_block_parsing(
        cx: &mut TestAppContext,
    ) {
        let markdown = "before\n<!--\na\n--> trailing\n# after".to_string();
        let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 3);
            assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(visible[1].entity.read(cx).kind(), BlockKind::Comment);
            assert_eq!(
                visible[1].entity.read(cx).display_text(),
                "<!--\na\n--> trailing"
            );
            assert_eq!(
                visible[2].entity.read(cx).kind(),
                BlockKind::Heading { level: 1 }
            );
            assert_eq!(visible[2].entity.read(cx).display_text(), "after");
            assert_eq!(
                editor.document.markdown_text(cx),
                "before\n\n<!--\na\n--> trailing\n\n# after"
            );
        });
    }

    #[gpui::test]
    async fn unclosed_html_comment_stays_raw_and_does_not_absorb_following_paragraph(
        cx: &mut TestAppContext,
    ) {
        let markdown = "<!--\na\n\nparagraph".to_string();
        let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 2);
            assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::RawMarkdown);
            assert_eq!(visible[0].entity.read(cx).display_text(), "<!--\na");
            assert_eq!(visible[1].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(visible[1].entity.read(cx).display_text(), "paragraph");
            assert_eq!(editor.document.markdown_text(cx), "<!--\na\n\nparagraph");
        });
    }

    #[gpui::test]
    async fn imports_comment_blocks_inside_list_quote_and_callout(cx: &mut TestAppContext) {
        let list_editor =
            cx.new(|cx| Editor::from_markdown(cx, "- item\n  <!--\n  list\n  -->".into(), None));
        list_editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 2);
            assert_eq!(
                visible[0].entity.read(cx).kind(),
                BlockKind::BulletedListItem
            );
            assert_eq!(visible[1].entity.read(cx).kind(), BlockKind::Comment);
            assert_eq!(visible[1].entity.read(cx).display_text(), "<!--\nlist\n-->");
            assert_eq!(visible[1].entity.read(cx).render_depth, 1);
            assert_eq!(
                editor.document.markdown_text(cx),
                "- item\n  <!--\n  list\n  -->"
            );
        });

        let quote_editor = cx.new(|cx| {
            Editor::from_markdown(cx, "> quote\n>\n> <!--\n> quoted\n> -->".into(), None)
        });
        quote_editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 3);
            assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::Quote);
            assert_eq!(visible[2].entity.read(cx).kind(), BlockKind::Comment);
            assert_eq!(
                visible[2].entity.read(cx).display_text(),
                "<!--\nquoted\n-->"
            );
            assert_eq!(visible[2].entity.read(cx).quote_depth, 1);
            assert_eq!(
                editor.document.markdown_text(cx),
                "> quote\n> \n> <!--\n> quoted\n> -->"
            );
        });

        let callout_editor = cx.new(|cx| {
            Editor::from_markdown(
                cx,
                "> [!NOTE] Title\n>\n> <!--\n> callout\n> -->".into(),
                None,
            )
        });
        callout_editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 3);
            assert_eq!(
                visible[0].entity.read(cx).kind(),
                BlockKind::Callout(CalloutVariant::Note)
            );
            assert_eq!(visible[2].entity.read(cx).kind(), BlockKind::Comment);
            assert_eq!(
                visible[2].entity.read(cx).display_text(),
                "<!--\ncallout\n-->"
            );
            assert_eq!(visible[2].entity.read(cx).callout_depth, 1);
            assert_eq!(
                editor.document.markdown_text(cx),
                "> [!NOTE] Title\n> \n> <!--\n> callout\n> -->"
            );
        });
    }

    #[gpui::test]
    async fn parses_multiline_root_footnote_definition_as_native_block(cx: &mut TestAppContext) {
        let markdown = "[^note]: Footnote text with **bold**\n    - item 1\n    - item 2\n\n    Second paragraph.".to_string();
        let canonical_markdown = "[^note]: Footnote text with **bold**\n\n    - item 1\n    - item 2\n\n    Second paragraph.";
        let editor = cx.new(|cx| Editor::from_markdown(cx, markdown.clone(), None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 5);
            assert_eq!(
                visible[0].entity.read(cx).kind(),
                BlockKind::FootnoteDefinition
            );
            assert_eq!(visible[0].entity.read(cx).display_text(), "note");
            assert_eq!(visible[1].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(
                visible[1].entity.read(cx).display_text(),
                "Footnote text with bold"
            );
            assert_eq!(
                visible[2].entity.read(cx).kind(),
                BlockKind::BulletedListItem
            );
            assert_eq!(visible[2].entity.read(cx).display_text(), "item 1");
            assert_eq!(
                visible[3].entity.read(cx).kind(),
                BlockKind::BulletedListItem
            );
            assert_eq!(visible[3].entity.read(cx).display_text(), "item 2");
            assert_eq!(visible[4].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(
                visible[4].entity.read(cx).display_text(),
                "Second paragraph."
            );
            assert_eq!(editor.document.markdown_text(cx), canonical_markdown);
        });
    }

    #[gpui::test]
    async fn nested_quote_footnote_definition_upgrades_to_native_block(cx: &mut TestAppContext) {
        let markdown = "> outer\n>\n> [^note]: nested footnote".to_string();
        let canonical_markdown = "> outer\n> \n> [^note]: nested footnote";
        let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 4);
            assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::Quote);
            assert_eq!(visible[0].entity.read(cx).display_text(), "outer");
            assert_eq!(visible[1].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(visible[1].entity.read(cx).display_text(), "");
            assert_eq!(visible[1].entity.read(cx).quote_depth, 1);
            assert_eq!(
                visible[2].entity.read(cx).kind(),
                BlockKind::FootnoteDefinition
            );
            assert_eq!(visible[2].entity.read(cx).display_text(), "note");
            assert_eq!(visible[2].entity.read(cx).quote_depth, 1);
            assert_eq!(visible[3].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(visible[3].entity.read(cx).display_text(), "nested footnote");
            assert_eq!(visible[3].entity.read(cx).quote_depth, 1);
            assert!(visible[3].entity.read(cx).footnote_anchor.is_some());
            assert_eq!(editor.document.markdown_text(cx), canonical_markdown);
        });
    }

