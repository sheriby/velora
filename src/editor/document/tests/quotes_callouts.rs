    use gpui::{AppContext, TestAppContext};

    
    use crate::components::{BlockKind, CalloutVariant, Editor};

    #[gpui::test]
    async fn imports_quote_with_list_children(cx: &mut TestAppContext) {
        let editor = cx.new(|cx| {
            Editor::from_markdown(
                cx,
                "> Quote with list:\n> - item 1\n> - [ ] task item".to_string(),
                None,
            )
        });

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 3);
            assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::Quote);
            assert_eq!(
                visible[0].entity.read(cx).display_text(),
                "Quote with list:"
            );
            assert_eq!(
                visible[1].entity.read(cx).kind(),
                BlockKind::BulletedListItem
            );
            assert_eq!(visible[1].entity.read(cx).display_text(), "item 1");
            assert_eq!(visible[1].entity.read(cx).quote_depth, 1);
            assert_eq!(
                visible[2].entity.read(cx).kind(),
                BlockKind::TaskListItem { checked: false }
            );
            assert_eq!(visible[2].entity.read(cx).quote_depth, 1);
            assert_eq!(
                editor.document.markdown_text(cx),
                "> Quote with list:\n> - item 1\n> - [ ] task item"
            );
        });
    }

    #[gpui::test]
    async fn imports_quote_with_code_block_child(cx: &mut TestAppContext) {
        let editor = cx.new(|cx| {
            Editor::from_markdown(
                cx,
                "> Quote with code block:\n>\n>     fn main() {\n>         println!(\"hi\");\n>     }"
                    .to_string(),
                None,
            )
        });

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 3);
            assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::Quote);
            assert_eq!(
                visible[0].entity.read(cx).display_text(),
                "Quote with code block:"
            );
            assert_eq!(visible[1].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(visible[1].entity.read(cx).display_text(), "");
            assert_eq!(visible[1].entity.read(cx).quote_depth, 1);
            assert_eq!(
                visible[2].entity.read(cx).kind(),
                BlockKind::CodeBlock { language: None }
            );
            assert_eq!(visible[2].entity.read(cx).quote_depth, 1);
            assert_eq!(
                visible[2].entity.read(cx).display_text(),
                "fn main() {\n    println!(\"hi\");\n}"
            );
            assert_eq!(
                editor.document.markdown_text(cx),
                "> Quote with code block:\n> \n> ```\n> fn main() {\n>     println!(\"hi\");\n> }\n> ```"
            );
        });
    }

    #[gpui::test]
    async fn imports_quote_with_standalone_image_child(cx: &mut TestAppContext) {
        let markdown = "> ![alt](./img.png)".to_string();
        let editor = cx.new(|cx| Editor::from_markdown(cx, markdown.clone(), None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 2);
            assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::Quote);
            assert_eq!(visible[0].entity.read(cx).display_text(), "");
            assert_eq!(visible[1].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(visible[1].entity.read(cx).quote_depth, 1);
            assert_eq!(editor.document.markdown_text(cx), markdown);
        });
    }

    #[gpui::test]
    async fn imports_bulleted_list_item_with_standalone_image_title(cx: &mut TestAppContext) {
        let markdown = "- ![alt](./img.png)".to_string();
        let editor = cx.new(|cx| Editor::from_markdown(cx, markdown.clone(), None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 1);
            assert_eq!(
                visible[0].entity.read(cx).kind(),
                BlockKind::BulletedListItem
            );
            assert!(visible[0].entity.read(cx).children.is_empty());
            assert_eq!(editor.document.markdown_text(cx), markdown);
        });
    }

    #[gpui::test]
    async fn imports_list_item_with_standalone_image_child(cx: &mut TestAppContext) {
        let markdown = "- item\n  ![alt](./img.png)".to_string();
        let editor = cx.new(|cx| Editor::from_markdown(cx, markdown.clone(), None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 2);
            assert_eq!(
                visible[0].entity.read(cx).kind(),
                BlockKind::BulletedListItem
            );
            assert_eq!(visible[0].entity.read(cx).display_text(), "item");
            assert_eq!(visible[1].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(visible[1].entity.read(cx).render_depth, 1);
            assert_eq!(editor.document.markdown_text(cx), markdown);
        });
    }

    #[gpui::test]
    async fn imports_list_image_title_with_native_child_paragraph(cx: &mut TestAppContext) {
        let markdown = "- ![alt](./img.png)\n  child text".to_string();
        let canonical_markdown = "- ![alt](./img.png)\n\n  child text";
        let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 2);
            assert_eq!(
                visible[0].entity.read(cx).kind(),
                BlockKind::BulletedListItem
            );
            assert_eq!(visible[1].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(visible[1].entity.read(cx).display_text(), "child text");
            assert_eq!(visible[1].entity.read(cx).render_depth, 1);
            assert_eq!(editor.document.markdown_text(cx), canonical_markdown);
        });
    }

    #[gpui::test]
    async fn imports_quote_with_numbered_list_image_item(cx: &mut TestAppContext) {
        let markdown = "> 1. ![alt](./img.png)".to_string();
        let editor = cx.new(|cx| Editor::from_markdown(cx, markdown.clone(), None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 2);
            assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::Quote);
            assert_eq!(
                visible[1].entity.read(cx).kind(),
                BlockKind::NumberedListItem
            );
            assert_eq!(visible[1].entity.read(cx).quote_depth, 1);
            assert_eq!(editor.document.markdown_text(cx), markdown);
        });
    }

    #[gpui::test]
    async fn imports_callout_with_task_list_image_item_and_child(cx: &mut TestAppContext) {
        let markdown = "> [!NOTE]\n> - [ ] ![cover][img]\n>   ![detail](./detail.png)\n>\n> [img]: ./cover.png".to_string();
        let canonical_markdown = "> [!NOTE]\n> - [ ] ![cover][img]\n>   ![detail](./detail.png)\n> \n> [img]: ./cover.png";
        let editor = cx.new(|cx| Editor::from_markdown(cx, markdown.clone(), None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(
                visible[0].entity.read(cx).kind(),
                BlockKind::Callout(CalloutVariant::Note)
            );
            assert_eq!(
                visible[1].entity.read(cx).kind(),
                BlockKind::TaskListItem { checked: false }
            );
            assert_eq!(visible[1].entity.read(cx).quote_depth, 1);
            assert_eq!(visible[1].entity.read(cx).callout_depth, 1);
            assert_eq!(visible[2].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(visible[2].entity.read(cx).render_depth, 1);
            assert_eq!(visible[2].entity.read(cx).quote_depth, 1);
            assert_eq!(visible[2].entity.read(cx).callout_depth, 1);
            assert_eq!(editor.document.markdown_text(cx), canonical_markdown);
        });
    }

    #[gpui::test]
    async fn imports_callout_with_standalone_image_child(cx: &mut TestAppContext) {
        let markdown = "> [!NOTE]\n> ![alt](./img.png)".to_string();
        let editor = cx.new(|cx| Editor::from_markdown(cx, markdown.clone(), None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(
                visible[0].entity.read(cx).kind(),
                BlockKind::Callout(CalloutVariant::Note)
            );
            assert_eq!(visible[1].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(visible[1].entity.read(cx).quote_depth, 1);
            assert_eq!(visible[1].entity.read(cx).callout_depth, 1);
            assert_eq!(editor.document.markdown_text(cx), markdown);
        });
    }

    #[gpui::test]
    async fn imports_quote_list_item_with_native_child_paragraph(cx: &mut TestAppContext) {
        let markdown = "> - item\n>\n>     child text".to_string();
        let canonical_markdown = "> - item\n> \n>   child text";
        let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 3);
            assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::Quote);
            assert_eq!(
                visible[1].entity.read(cx).kind(),
                BlockKind::BulletedListItem
            );
            assert_eq!(visible[1].entity.read(cx).quote_depth, 1);
            assert_eq!(visible[2].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(visible[2].entity.read(cx).display_text(), "child text");
            assert_eq!(visible[2].entity.read(cx).render_depth, 1);
            assert_eq!(visible[2].entity.read(cx).quote_depth, 1);
            assert_eq!(editor.document.markdown_text(cx), canonical_markdown);
        });
    }

    #[gpui::test]
    async fn imports_callout_list_item_with_native_child_paragraph(cx: &mut TestAppContext) {
        let markdown = "> [!NOTE]\n> - item\n>\n>     child text".to_string();
        let canonical_markdown = "> [!NOTE]\n> - item\n> \n>   child text";
        let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(
                visible[0].entity.read(cx).kind(),
                BlockKind::Callout(CalloutVariant::Note)
            );
            assert_eq!(
                visible[1].entity.read(cx).kind(),
                BlockKind::BulletedListItem
            );
            assert_eq!(visible[1].entity.read(cx).quote_depth, 1);
            assert_eq!(visible[1].entity.read(cx).callout_depth, 1);
            assert_eq!(visible[2].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(visible[2].entity.read(cx).display_text(), "child text");
            assert_eq!(visible[2].entity.read(cx).render_depth, 1);
            assert_eq!(visible[2].entity.read(cx).quote_depth, 1);
            assert_eq!(visible[2].entity.read(cx).callout_depth, 1);
            assert_eq!(editor.document.markdown_text(cx), canonical_markdown);
        });
    }

    #[gpui::test]
    async fn quote_does_not_promote_multiline_image_paragraph_to_child(cx: &mut TestAppContext) {
        let markdown = "> ![alt](./img.png)\n> tail".to_string();
        let editor = cx.new(|cx| Editor::from_markdown(cx, markdown.clone(), None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 1);
            assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::Quote);
            assert_eq!(editor.document.markdown_text(cx), markdown);
        });
    }

    #[gpui::test]
    async fn imports_callout_from_quote_header(cx: &mut TestAppContext) {
        let editor = cx.new(|cx| Editor::from_markdown(cx, "> [!NOTE]".to_string(), None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 1);
            assert_eq!(
                visible[0].entity.read(cx).kind(),
                BlockKind::Callout(CalloutVariant::Note)
            );
            assert_eq!(visible[0].entity.read(cx).display_text(), "");
            assert!(visible[0].entity.read(cx).children.is_empty());
            assert_eq!(visible[0].entity.read(cx).quote_depth, 1);
            assert_eq!(editor.document.markdown_text(cx), "> [!NOTE]");
        });
    }

    #[gpui::test]
    async fn imports_important_callout_case_insensitively(cx: &mut TestAppContext) {
        let editor = cx
            .new(|cx| Editor::from_markdown(cx, "> [!important] Optional title".to_string(), None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 1);
            assert_eq!(
                visible[0].entity.read(cx).kind(),
                BlockKind::Callout(CalloutVariant::Important)
            );
            assert_eq!(visible[0].entity.read(cx).display_text(), "Optional title");
            assert_eq!(
                editor.document.markdown_text(cx),
                "> [!IMPORTANT] Optional title"
            );
        });
    }

    #[gpui::test]
    async fn imports_callout_title_and_nested_quote_child(cx: &mut TestAppContext) {
        let editor = cx.new(|cx| {
            Editor::from_markdown(
                cx,
                "> [!WARNING] Custom title\n> body\n> > nested".to_string(),
                None,
            )
        });

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 3);
            assert_eq!(
                visible[0].entity.read(cx).kind(),
                BlockKind::Callout(CalloutVariant::Warning)
            );
            assert_eq!(visible[0].entity.read(cx).display_text(), "Custom title");
            assert_eq!(visible[1].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(visible[1].entity.read(cx).display_text(), "body");
            assert_eq!(visible[2].entity.read(cx).kind(), BlockKind::Quote);
            assert_eq!(visible[2].entity.read(cx).display_text(), "nested");
            assert_eq!(visible[2].entity.read(cx).quote_depth, 2);
            assert_eq!(
                editor.document.markdown_text(cx),
                "> [!WARNING] Custom title\n> body\n> > nested"
            );
        });
    }

    #[gpui::test]
    async fn imports_callout_with_multiline_nested_quote_child(cx: &mut TestAppContext) {
        let markdown = [
            "> [!WARNING] Custom title",
            "> body",
            "> > inner one",
            "> >",
            "> > inner two",
            "> after",
        ]
        .join("\n");
        let canonical_markdown = [
            "> [!WARNING] Custom title",
            "> body",
            "> > inner one",
            "> > ",
            "> > inner two",
            "> after",
        ]
        .join("\n");
        let editor = cx.new(|cx| Editor::from_markdown(cx, markdown.clone(), None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 4);
            assert_eq!(
                visible[0].entity.read(cx).kind(),
                BlockKind::Callout(CalloutVariant::Warning)
            );
            assert_eq!(visible[1].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(visible[1].entity.read(cx).display_text(), "body");
            assert_eq!(visible[2].entity.read(cx).kind(), BlockKind::Quote);
            assert_eq!(
                visible[2].entity.read(cx).display_text(),
                "inner one\n\ninner two"
            );
            assert_eq!(visible[2].entity.read(cx).quote_depth, 2);
            assert!(
                visible[2]
                    .entity
                    .read(cx)
                    .visible_quote_group_anchor
                    .is_some()
            );
            assert_eq!(visible[3].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(visible[3].entity.read(cx).display_text(), "after");
            assert_eq!(visible[3].entity.read(cx).quote_depth, 1);
            assert!(
                visible[3]
                    .entity
                    .read(cx)
                    .visible_quote_group_anchor
                    .is_none()
            );
            assert_eq!(editor.document.markdown_text(cx), canonical_markdown);
        });
    }

    #[gpui::test]
    async fn unknown_callout_marker_stays_plain_quote(cx: &mut TestAppContext) {
        let editor = cx.new(|cx| Editor::from_markdown(cx, "> [!UNKNOWN]".to_string(), None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 1);
            assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::Quote);
            assert_eq!(visible[0].entity.read(cx).display_text(), "[!UNKNOWN]");
            assert_eq!(editor.document.markdown_text(cx), "> [!UNKNOWN]");
        });
    }

    #[gpui::test]
    async fn preserves_separator_between_quote_title_and_nested_child(cx: &mut TestAppContext) {
        let editor =
            cx.new(|cx| Editor::from_markdown(cx, "> outer\n>\n>> inner".to_string(), None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 3);
            assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::Quote);
            assert_eq!(visible[0].entity.read(cx).display_text(), "outer");
            assert_eq!(visible[1].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(visible[1].entity.read(cx).display_text(), "");
            assert_eq!(visible[1].entity.read(cx).quote_depth, 1);
            assert_eq!(visible[2].entity.read(cx).kind(), BlockKind::Quote);
            assert_eq!(visible[2].entity.read(cx).display_text(), "inner");
            assert_eq!(visible[2].entity.read(cx).quote_depth, 2);
            assert_eq!(editor.document.markdown_text(cx), "> outer\n> \n> > inner");
        });
    }

    #[gpui::test]
    async fn imports_quote_with_native_table_child(cx: &mut TestAppContext) {
        let markdown = "> Quote with table:\n> | A | B |\n> | --- | --- |\n> | 1 | 2 |".to_string();
        let editor = cx.new(|cx| Editor::from_markdown(cx, markdown.clone(), None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 2);
            assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::Quote);
            assert_eq!(
                visible[0].entity.read(cx).display_text(),
                "Quote with table:"
            );
            assert_eq!(visible[1].entity.read(cx).kind(), BlockKind::Table);
            assert_eq!(visible[1].entity.read(cx).quote_depth, 1);
            let table = visible[1]
                .entity
                .read(cx)
                .record
                .table
                .as_ref()
                .expect("native nested table");
            assert_eq!(table.header.len(), 2);
            assert_eq!(table.rows.len(), 1);
            assert_eq!(table.rows[0].len(), 2);
            assert_eq!(editor.document.markdown_text(cx), markdown);
        });
    }

    #[gpui::test]
    async fn invalid_table_inside_quote_preserves_outer_quote_and_raw_child(
        cx: &mut TestAppContext,
    ) {
        let markdown = "> Quote with broken table:\n> | A |\n> | --- | --- |\n> | 1 |".to_string();
        let editor = cx.new(|cx| Editor::from_markdown(cx, markdown.clone(), None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 2);
            assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::Quote);
            assert_eq!(
                visible[0].entity.read(cx).display_text(),
                "Quote with broken table:"
            );
            assert_eq!(visible[1].entity.read(cx).kind(), BlockKind::RawMarkdown);
            assert_eq!(
                visible[1].entity.read(cx).display_text(),
                "| A |\n| --- | --- |\n| 1 |"
            );
            assert_eq!(visible[1].entity.read(cx).quote_depth, 1);
            assert_eq!(editor.document.markdown_text(cx), markdown);
        });
    }

    #[gpui::test]
    async fn final_mixed_mega_block_preserves_important_callout_with_native_table_and_native_footnote(
        cx: &mut TestAppContext,
    ) {
        let markdown = "> [!IMPORTANT]\n> Final mixed block that combines:\n>\n> - **bold**\n> - *italic*\n> - `inline code`\n> - [link](https://example.com)\n> - ![image](https://example.com/image.png)\n> - ~~strike~~\n>\n> And a table:\n>\n> | k | v |\n> | --- | --- |\n> | a | 1 |\n> | b | 2 |\n>\n> And a fenced code block:\n>\n> ```ts\n> export const answer = 42;\n> ```\n>\n> And a footnote reference.[^final]\n>\n> [^final]: Final footnote text with nested list:\n>   - one\n>   - two".to_string();
        let canonical_markdown = "> [!IMPORTANT]\n> Final mixed block that combines:\n> \n> - **bold**\n> - *italic*\n> - `inline code`\n> - [link](https://example.com)\n> - ![image](https://example.com/image.png)\n> - ~~strike~~\n> \n> And a table:\n> \n> | k | v |\n> | --- | --- |\n> | a | 1 |\n> | b | 2 |\n> \n> And a fenced code block:\n> \n> ```ts\n> export const answer = 42;\n> ```\n> \n> And a footnote reference.[^final]\n> \n> [^final]: Final footnote text with nested list:\n> \n>     - one\n>     - two";
        let editor = cx.new(|cx| Editor::from_markdown(cx, markdown.clone(), None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(
                visible[0].entity.read(cx).kind(),
                BlockKind::Callout(CalloutVariant::Important)
            );
            assert!(visible.iter().any(|visible| {
                let block = visible.entity.read(cx);
                block.kind() == BlockKind::BulletedListItem && block.quote_depth == 1
            }));
            assert!(visible.iter().any(|visible| {
                let block = visible.entity.read(cx);
                block.kind()
                    == BlockKind::CodeBlock {
                        language: Some("ts".into()),
                    }
                    && block.display_text().contains("export const answer = 42;")
            }));
            assert!(visible.iter().any(|visible| {
                let block = visible.entity.read(cx);
                block.kind() == BlockKind::Table
                    && block.quote_depth == 1
                    && block.record.table.as_ref().is_some_and(|table| {
                        table.header.len() == 2
                            && table.rows.len() == 2
                            && table.header[0].serialize_markdown() == "k"
                            && table.rows[1][1].serialize_markdown() == "2"
                    })
            }));
            assert!(visible.iter().any(|visible| {
                let block = visible.entity.read(cx);
                block.kind() == BlockKind::Paragraph
                    && block.display_text().contains("And a table:")
                    && block.quote_depth == 1
            }));
            assert!(visible.iter().any(|visible| {
                let block = visible.entity.read(cx);
                block.kind() == BlockKind::FootnoteDefinition
                    && block.display_text() == "final"
                    && block.quote_depth == 1
            }));
            assert!(visible.iter().any(|visible| {
                let block = visible.entity.read(cx);
                block.kind() == BlockKind::Paragraph
                    && block.display_text() == "Final footnote text with nested list:"
                    && block.footnote_anchor.is_some()
                    && block.quote_depth == 1
            }));
            assert!(
                visible
                    .iter()
                    .filter(|visible| {
                        let block = visible.entity.read(cx);
                        block.kind() == BlockKind::BulletedListItem
                            && block.footnote_anchor.is_some()
                            && block.quote_depth == 1
                    })
                    .count()
                    >= 2
            );
            assert_eq!(editor.document.markdown_text(cx), canonical_markdown);
        });
    }

    #[gpui::test]
    async fn unsupported_nested_block_preserves_native_list_item_with_raw_child(
        cx: &mut TestAppContext,
    ) {
        let editor = cx.new(|cx| {
            Editor::from_markdown(
                cx,
                "- native before\n- raw item\n  <div>\n  inner\n  </div>\n- native after"
                    .to_string(),
                None,
            )
        });

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 4);
            assert_eq!(
                visible[0].entity.read(cx).kind(),
                BlockKind::BulletedListItem
            );
            assert_eq!(visible[0].entity.read(cx).display_text(), "native before");
            assert_eq!(
                visible[1].entity.read(cx).kind(),
                BlockKind::BulletedListItem
            );
            assert_eq!(visible[1].entity.read(cx).display_text(), "raw item");
            assert_eq!(visible[2].entity.read(cx).kind(), BlockKind::HtmlBlock);
            assert!(visible[2].entity.read(cx).display_text().contains("<div>"));
            assert_eq!(
                visible[3].entity.read(cx).kind(),
                BlockKind::BulletedListItem
            );
            assert_eq!(visible[3].entity.read(cx).display_text(), "native after");
        });
    }

