    use gpui::{AppContext, TestAppContext};

    
    use crate::components::{BlockKind, Editor};
    #[gpui::test]

    async fn test_md_fixture_keeps_mixed_supported_and_raw_sections_visible(
        cx: &mut TestAppContext,
    ) {
        let markdown = include_str!("../../../../fixtures/markdown-baseline.md").to_string();
        let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert!(visible.len() > 40);

            assert!(visible.iter().any(|block| {
                let block = block.entity.read(cx);
                block.kind() == BlockKind::Heading { level: 1 }
                    && block.display_text() == "Markdown Rendering Test Suite"
            }));

            assert!(visible.iter().any(|block| {
                let block = block.entity.read(cx);
                block.kind() == BlockKind::Quote
                    && block.display_text().contains("Blockquote paragraph one.")
            }));

            assert!(visible.iter().any(|block| {
                let block = block.entity.read(cx);
                block.kind().is_code_block()
                    && block
                        .display_text()
                        .contains("println!(\"fenced code block\");")
            }));

            assert!(visible.iter().any(|block| {
                let block = block.entity.read(cx);
                block.kind() == BlockKind::TaskListItem { checked: false }
                    && block.display_text().contains("Unchecked task")
            }));

            assert!(visible.iter().any(|block| {
                let block = block.entity.read(cx);
                block.kind() == BlockKind::BulletedListItem
                    && block.display_text() == "Mixed list item"
            }));

            assert!(visible.iter().any(|block| {
                let block = block.entity.read(cx);
                block.kind().is_code_block() && block.display_text().contains("let x = 1;")
            }));

            let multiline_code = visible
                .iter()
                .find(|block| {
                    block
                        .entity
                        .read(cx)
                        .display_text()
                        .starts_with("Code span across line breaks:")
                })
                .expect("multiline inline code sample")
                .entity
                .read(cx);
            assert!(multiline_code.display_text().contains("line 1\nline 2"));
            let multiline_prefix = "Code span across line breaks:\n".len();
            assert!(multiline_code.inline_spans().iter().any(|span| {
                span.style.code
                    && span.range == (multiline_prefix..multiline_prefix + "line 1\nline 2".len())
            }));

            let backtick_sample = visible
                .iter()
                .find(|block| {
                    block
                        .entity
                        .read(cx)
                        .display_text()
                        .starts_with("Backticks in normal text:")
                })
                .expect("literal backtick sample")
                .entity
                .read(cx);
            assert_eq!(
                backtick_sample.display_text(),
                "Backticks in normal text: ` and `` and ```"
            );
            let backtick_prefix = "Backticks in normal text: ".len();
            let expected_code_ranges = vec![
                backtick_prefix..backtick_prefix + 1,
                backtick_prefix + 6..backtick_prefix + 8,
                backtick_prefix + 13..backtick_prefix + 16,
            ];
            let actual_code_ranges = backtick_sample
                .inline_spans()
                .iter()
                .filter(|span| span.style.code)
                .map(|span| span.range.clone())
                .collect::<Vec<_>>();
            assert_eq!(actual_code_ranges, expected_code_ranges);
            assert!(!backtick_sample.inline_style_at(backtick_prefix + 2).code);
            assert!(!backtick_sample.inline_style_at(backtick_prefix + 9).code);

            assert!(visible.iter().any(|block| {
                let block = block.entity.read(cx);
                block.kind() == BlockKind::Quote
                    && block.display_text().contains("quoted paragraph two")
            }));

            assert!(visible.iter().any(|block| {
                let block = block.entity.read(cx);
                block.kind() == BlockKind::Table
                    && block
                        .record
                        .table
                        .as_ref()
                        .is_some_and(|table| table.header.len() == 3 && table.rows.len() >= 2)
            }));

            assert!(visible.iter().any(|block| {
                let block = block.entity.read(cx);
                block.kind() == BlockKind::HtmlBlock && block.display_text().contains("<details>")
            }));

            assert!(!visible.iter().any(|block| {
                let block = block.entity.read(cx);
                block.kind() == BlockKind::RawMarkdown
                    && block.display_text().contains("- Mixed list item")
            }));
        });
    }

    #[gpui::test]
    async fn list_followed_by_blank_line_and_root_paragraph_stays_separate(
        cx: &mut TestAppContext,
    ) {
        let editor = cx.new(|cx| Editor::from_markdown(cx, "- item\n\ntext".to_string(), None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 2);
            assert_eq!(
                visible[0].entity.read(cx).kind(),
                BlockKind::BulletedListItem
            );
            assert_eq!(visible[0].entity.read(cx).display_text(), "item");
            assert_eq!(visible[1].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(visible[1].entity.read(cx).display_text(), "text");
            assert_eq!(visible[1].entity.read(cx).render_depth, 0);
        });
    }

    #[gpui::test]
    async fn mode_switch_preserves_root_paragraph_after_list(cx: &mut TestAppContext) {
        let editor = cx.new(|cx| Editor::from_markdown(cx, "- item\n\ntext".to_string(), None));

        editor.update(cx, |editor, cx| {
            editor.toggle_view_mode(cx);
            assert!(matches!(editor.view_mode, crate::editor::ViewMode::Source));
            editor.toggle_view_mode(cx);
            assert!(matches!(editor.view_mode, crate::editor::ViewMode::Rendered));

            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 2);
            assert_eq!(
                visible[0].entity.read(cx).kind(),
                BlockKind::BulletedListItem
            );
            assert_eq!(visible[1].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(visible[1].entity.read(cx).display_text(), "text");
            assert_eq!(visible[1].entity.read(cx).render_depth, 0);
        });
    }

    #[gpui::test]
    async fn list_empty_root_and_following_paragraph_stay_outside_list(cx: &mut TestAppContext) {
        let editor = cx.new(|cx| Editor::from_markdown(cx, "- item\n\n\ntext".to_string(), None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 3);
            assert_eq!(
                visible[0].entity.read(cx).kind(),
                BlockKind::BulletedListItem
            );
            assert_eq!(visible[1].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(visible[1].entity.read(cx).display_text(), "");
            assert_eq!(visible[1].entity.read(cx).render_depth, 0);
            assert_eq!(visible[2].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(visible[2].entity.read(cx).display_text(), "text");
            assert_eq!(visible[2].entity.read(cx).render_depth, 0);
        });
    }

    #[gpui::test]
    async fn blank_line_then_indented_text_upgrades_to_native_list_child_paragraph(
        cx: &mut TestAppContext,
    ) {
        let editor =
            cx.new(|cx| Editor::from_markdown(cx, "- item\n\n    child text".to_string(), None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 2);
            assert_eq!(
                visible[0].entity.read(cx).kind(),
                BlockKind::BulletedListItem
            );
            assert_eq!(visible[0].entity.read(cx).display_text(), "item");
            assert_eq!(visible[1].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(visible[1].entity.read(cx).display_text(), "child text");
            assert_eq!(visible[1].entity.read(cx).render_depth, 1);
            assert_eq!(editor.document.markdown_text(cx), "- item\n\n  child text");
        });
    }

    #[gpui::test]
    async fn preserves_reference_definitions_and_stops_quote_at_first_non_quoted_line(
        cx: &mut TestAppContext,
    ) {
        let reference_editor = cx.new(|cx| {
            Editor::from_markdown(
                cx,
                "[id]: http://example.com/\n    \"Title\"".to_string(),
                None,
            )
        });
        reference_editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 1);
            assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::RawMarkdown);
            assert_eq!(
                editor.document.markdown_text(cx),
                "[id]: http://example.com/\n    \"Title\""
            );
        });

        let quote_editor =
            cx.new(|cx| Editor::from_markdown(cx, "> quoted\ncontinued".to_string(), None));
        quote_editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 2);
            assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::Quote);
            assert_eq!(visible[0].entity.read(cx).display_text(), "quoted");
            assert_eq!(visible[1].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(visible[1].entity.read(cx).display_text(), "continued");
            assert_eq!(editor.document.markdown_text(cx), "> quoted\n\ncontinued");
        });
    }

    #[gpui::test]
    async fn simple_quote_does_not_consume_following_root_blocks(cx: &mut TestAppContext) {
        let editor = cx.new(|cx| {
            Editor::from_markdown(
                cx,
                "> quoted line\n> second line\n\n---\n\n## Next".to_string(),
                None,
            )
        });

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 3);
            assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::Quote);
            assert_eq!(
                visible[0].entity.read(cx).display_text(),
                "quoted line\nsecond line"
            );
            assert_eq!(visible[1].entity.read(cx).kind(), BlockKind::Separator);
            assert_eq!(
                visible[2].entity.read(cx).kind(),
                BlockKind::Heading { level: 2 }
            );
            assert_eq!(visible[2].entity.read(cx).display_text(), "Next");
        });
    }

    #[gpui::test]
    async fn non_quoted_line_after_quote_becomes_plain_paragraph_before_heading(
        cx: &mut TestAppContext,
    ) {
        let editor = cx.new(|cx| {
            Editor::from_markdown(cx, "> quoted\ncontinued\n\n## Next".to_string(), None)
        });

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 3);
            assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::Quote);
            assert_eq!(visible[0].entity.read(cx).display_text(), "quoted");
            assert_eq!(visible[1].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(visible[1].entity.read(cx).display_text(), "continued");
            assert_eq!(
                visible[2].entity.read(cx).kind(),
                BlockKind::Heading { level: 2 }
            );
            assert_eq!(visible[2].entity.read(cx).display_text(), "Next");
        });
    }

    #[gpui::test]
    async fn preserves_empty_root_blocks_across_round_trip(cx: &mut TestAppContext) {
        let editor =
            cx.new(|cx| Editor::from_markdown(cx, "alpha\n\n\nbeta\n\n".to_string(), None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 4);
            assert_eq!(visible[0].entity.read(cx).display_text(), "alpha");
            assert_eq!(visible[1].entity.read(cx).display_text(), "");
            assert_eq!(visible[2].entity.read(cx).display_text(), "beta");
            assert_eq!(visible[3].entity.read(cx).display_text(), "");
            assert_eq!(editor.document.markdown_text(cx), "alpha\n\n\nbeta\n\n");
        });

        editor.update(cx, |editor, cx| {
            editor.toggle_view_mode(cx);
            assert!(matches!(editor.view_mode, crate::editor::ViewMode::Source));
            editor.toggle_view_mode(cx);
            assert!(matches!(editor.view_mode, crate::editor::ViewMode::Rendered));

            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 4);
            assert_eq!(visible[0].entity.read(cx).display_text(), "alpha");
            assert_eq!(visible[1].entity.read(cx).display_text(), "");
            assert_eq!(visible[2].entity.read(cx).display_text(), "beta");
            assert_eq!(visible[3].entity.read(cx).display_text(), "");
        });
    }

    #[gpui::test]
    async fn imports_blank_line_inside_inline_code_as_single_paragraph(cx: &mut TestAppContext) {
        let editor = cx.new(|cx| Editor::from_markdown(cx, "`line 1\n\nline 2`".to_string(), None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 1);
            let block = visible[0].entity.read(cx);
            let text = "line 1\n\nline 2";
            assert_eq!(block.kind(), BlockKind::Paragraph);
            assert_eq!(block.display_text(), text);
            assert!(
                block
                    .inline_spans()
                    .iter()
                    .any(|span| { span.style.code && span.range == (0..text.len()) })
            );
            assert_eq!(editor.document.markdown_text(cx), "`line 1\n\nline 2`");
        });
    }

    #[gpui::test]
    async fn unclosed_inline_code_does_not_absorb_blank_line_paragraph(cx: &mut TestAppContext) {
        let editor = cx.new(|cx| Editor::from_markdown(cx, "`line 1\n\nline 2".to_string(), None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 2);
            assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(visible[0].entity.read(cx).display_text(), "`line 1");
            assert_eq!(visible[1].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(visible[1].entity.read(cx).display_text(), "line 2");
        });
    }

    #[gpui::test]
    async fn preserves_multiple_leading_blank_lines_as_empty_blocks(cx: &mut TestAppContext) {
        let editor = cx.new(|cx| Editor::from_markdown(cx, "\n\nalpha".to_string(), None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 3);
            assert_eq!(visible[0].entity.read(cx).display_text(), "");
            assert_eq!(visible[1].entity.read(cx).display_text(), "");
            assert_eq!(visible[2].entity.read(cx).display_text(), "alpha");
            assert_eq!(editor.document.markdown_text(cx), "\n\nalpha");
        });
    }

    #[gpui::test]
    async fn preserves_multiple_trailing_blank_lines_as_empty_blocks(cx: &mut TestAppContext) {
        let editor = cx.new(|cx| Editor::from_markdown(cx, "alpha\n\n\n".to_string(), None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 3);
            assert_eq!(visible[0].entity.read(cx).display_text(), "alpha");
            assert_eq!(visible[1].entity.read(cx).display_text(), "");
            assert_eq!(visible[2].entity.read(cx).display_text(), "");
            assert_eq!(editor.document.markdown_text(cx), "alpha\n\n\n");
        });
    }

    #[gpui::test]
    async fn single_trailing_newline_does_not_create_visible_empty_block(cx: &mut TestAppContext) {
        let editor = cx.new(|cx| Editor::from_markdown(cx, "alpha\n".to_string(), None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 1);
            assert_eq!(visible[0].entity.read(cx).display_text(), "alpha");
            assert_eq!(editor.document.markdown_text(cx), "alpha");
        });
    }

    #[gpui::test]
    async fn empty_document_keeps_single_editable_empty_block(cx: &mut TestAppContext) {
        let editor = cx.new(|cx| Editor::from_markdown(cx, String::new(), None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 1);
            assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(visible[0].entity.read(cx).display_text(), "");
            assert_eq!(editor.document.markdown_text(cx), "");
        });
    }
