    use super::super::Editor;
    use crate::components::{
        BlockKind, BlockRecord,
        ExitCodeBlock, Newline,
    };
    use gpui::{AppContext, TestAppContext};

    #[gpui::test]
    async fn enter_inside_script_paragraph_creates_new_block(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        let editor = cx.new(|cx| Editor::from_markdown(cx, "H~2~O".to_string(), None));

        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                let block = editor.document.visible_blocks()[0].entity.clone();
                block.update(cx, |block, block_cx| {
                    block.move_to(block.visible_len(), block_cx);
                    block.on_newline(&Newline, window, block_cx);
                });
            });
        });

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 2);
            assert_eq!(visible[0].entity.read(cx).display_text(), "H2O");
            assert_eq!(visible[1].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(visible[1].entity.read(cx).display_text(), "");
            assert_eq!(editor.document.markdown_text(cx), "H~2~O\n\n");
        });
    }

    #[gpui::test]
    async fn enter_inside_inline_math_paragraph_creates_new_block(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        let editor = cx.new(|cx| Editor::from_markdown(cx, "$n^2$".to_string(), None));

        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                let block = editor.document.visible_blocks()[0].entity.clone();
                block.update(cx, |block, block_cx| {
                    block.move_to(block.visible_len(), block_cx);
                    block.on_newline(&Newline, window, block_cx);
                });
            });
        });

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 2);
            assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(visible[0].entity.read(cx).display_text(), "$n^2$");
            assert!(!visible[0].entity.read(cx).uses_raw_text_editing());
            assert_eq!(visible[1].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(visible[1].entity.read(cx).display_text(), "");
            assert_eq!(editor.document.markdown_text(cx), "$n^2$\n\n");
        });
    }

    #[gpui::test]
    async fn trailing_fence_line_enter_closes_code_block(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        let editor =
            cx.new(|cx| Editor::from_markdown(cx, "```rust\nlet x = 1;\n```".to_string(), None));

        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                let block = editor.document.visible_blocks()[0].entity.clone();
                block.update(cx, |block, block_cx| {
                    // Type a closing fence on a fresh last line, then Enter.
                    let end = block.visible_len();
                    block.replace_text_in_visible_range(end..end, "\n```", None, false, block_cx);
                    block.move_to(block.visible_len(), block_cx);
                    block.on_newline(&Newline, window, block_cx);
                });
            });
        });

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 2);
            assert_eq!(
                visible[0].entity.read(cx).kind(),
                BlockKind::CodeBlock {
                    language: Some("rust".into())
                }
            );
            assert_eq!(visible[0].entity.read(cx).display_text(), "let x = 1;");
            assert_eq!(visible[1].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(visible[1].entity.read(cx).display_text(), "");
            assert_eq!(
                editor.document.markdown_text(cx),
                "```rust\nlet x = 1;\n```\n\n"
            );
        });
    }

    #[gpui::test]
    async fn setext_equals_underline_enter_promotes_previous_paragraph_to_h1(
        cx: &mut TestAppContext,
    ) {
        let cx = cx.add_empty_window();
        let editor = cx.new(|cx| Editor::from_markdown(cx, "Title\n\n=====".to_string(), None));

        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                let underline = editor.document.visible_blocks()[1].entity.clone();
                underline.update(cx, |block, block_cx| {
                    block.move_to(block.visible_len(), block_cx);
                    block.on_newline(&Newline, window, block_cx);
                });
            });
        });

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 2);
            assert_eq!(
                visible[0].entity.read(cx).kind(),
                BlockKind::Heading { level: 1 }
            );
            assert_eq!(visible[0].entity.read(cx).display_text(), "Title");
            assert_eq!(visible[1].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(visible[1].entity.read(cx).display_text(), "");
            assert_eq!(editor.document.markdown_text(cx), "# Title\n\n");
        });

        // Reversible: undo restores the two original paragraphs.
        editor.update(cx, |editor, cx| {
            editor.undo_document(cx);
            assert_eq!(editor.document.markdown_text(cx), "Title\n\n=====");
        });
    }

    #[gpui::test]
    async fn setext_dash_underline_enter_promotes_previous_paragraph_to_h2(
        cx: &mut TestAppContext,
    ) {
        let cx = cx.add_empty_window();
        // A bare "-----" in source parses as a thematic break, so simulate the
        // user typing the underline into the paragraph below the title instead.
        let editor = cx.new(|cx| Editor::from_markdown(cx, "Title\n\nx".to_string(), None));

        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                let underline = editor.document.visible_blocks()[1].entity.clone();
                underline.update(cx, |block, block_cx| {
                    let end = block.visible_len();
                    block.replace_text_in_visible_range(0..end, "-----", None, false, block_cx);
                    block.move_to(block.visible_len(), block_cx);
                    block.on_newline(&Newline, window, block_cx);
                });
            });
        });

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(
                visible[0].entity.read(cx).kind(),
                BlockKind::Heading { level: 2 }
            );
            assert_eq!(visible[0].entity.read(cx).display_text(), "Title");
            assert_eq!(editor.document.markdown_text(cx), "## Title\n\n");
        });
    }

    #[gpui::test]
    async fn dash_underline_without_heading_target_stays_a_separator(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        let editor = cx.new(|cx| Editor::from_markdown(cx, String::new(), None));

        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                let block = editor.document.visible_blocks()[0].entity.clone();
                block.update(cx, |block, block_cx| {
                    block.replace_text_in_visible_range(0..0, "-----", None, false, block_cx);
                    block.move_to(block.visible_len(), block_cx);
                    block.on_newline(&Newline, window, block_cx);
                });
            });
        });

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::Separator);
        });
    }

    #[gpui::test]
    async fn equals_underline_without_heading_target_stays_a_paragraph(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        let editor = cx.new(|cx| Editor::from_markdown(cx, String::new(), None));

        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                let block = editor.document.visible_blocks()[0].entity.clone();
                block.update(cx, |block, block_cx| {
                    block.replace_text_in_visible_range(0..0, "=====", None, false, block_cx);
                    block.move_to(block.visible_len(), block_cx);
                    block.on_newline(&Newline, window, block_cx);
                });
            });
        });

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(visible[0].entity.read(cx).display_text(), "=====");
        });
    }

    #[gpui::test]
    async fn math_block_exit_shortcut_creates_plain_text_block(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        let editor = cx.new(|cx| Editor::from_markdown(cx, "$$n^2$$".to_string(), None));

        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                let block = editor.document.visible_blocks()[0].entity.clone();
                block.update(cx, |block, block_cx| {
                    block.on_exit_code_block(&ExitCodeBlock, window, block_cx);
                });
            });
        });

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 2);
            assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::MathBlock);
            assert_eq!(visible[0].entity.read(cx).display_text(), "$$n^2$$");
            assert_eq!(visible[1].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(visible[1].entity.read(cx).display_text(), "");
            assert_eq!(editor.document.markdown_text(cx), "$$n^2$$\n\n");
        });
    }

    #[gpui::test]
    async fn dollar_dollar_enter_creates_editable_math_block(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        let editor = cx.new(|cx| Editor::from_markdown(cx, String::new(), None));

        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                let block = editor.document.visible_blocks()[0].entity.clone();
                block.update(cx, |block, block_cx| {
                    block.replace_text_in_visible_range(
                        0..block.visible_len(),
                        "$$",
                        None,
                        false,
                        block_cx,
                    );
                    block.move_to(block.visible_len(), block_cx);
                    block.on_newline(&Newline, window, block_cx);
                });
            });
        });

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 1);
            let block = visible[0].entity.read(cx);
            assert_eq!(block.kind(), BlockKind::MathBlock);
            assert_eq!(block.display_text(), "$$\n\n$$");
            assert_eq!(block.selected_range, 3..3);
            assert!(block.uses_raw_text_editing());
            assert_eq!(editor.document.markdown_text(cx), "$$\n\n$$");
        });
    }

    #[gpui::test]
    async fn dollar_dollar_prefix_then_enter_wraps_existing_line(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        let editor = cx.new(|cx| Editor::from_markdown(cx, "E = mc^2".to_string(), None));

        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                let block = editor.document.visible_blocks()[0].entity.clone();
                block.update(cx, |block, block_cx| {
                    // Home, type the fence in front of the formula, then Enter.
                    block.move_to(0, block_cx);
                    block.replace_text_in_visible_range(0..0, "$$", None, false, block_cx);
                    block.move_to("$$".len(), block_cx);
                    block.on_newline(&Newline, window, block_cx);
                });
            });
        });

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 1);
            let block = visible[0].entity.read(cx);
            assert_eq!(block.kind(), BlockKind::MathBlock);
            // The pre-existing text is kept as the formula body.
            assert_eq!(block.display_text(), "$$\nE = mc^2\n$$");
            assert_eq!(block.selected_range, "$$\n".len().."$$\n".len());
            assert_eq!(editor.document.markdown_text(cx), "$$\nE = mc^2\n$$");
        });
    }

    #[gpui::test]
    async fn enter_inside_math_block_keeps_local_formula_editing(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        let editor = cx.new(|cx| Editor::from_markdown(cx, "$$n^2$$".to_string(), None));

        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                let block = editor.document.visible_blocks()[0].entity.clone();
                block.update(cx, |block, block_cx| {
                    block.move_to(3, block_cx);
                    block.on_newline(&Newline, window, block_cx);
                });
            });
        });

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 1);
            assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::MathBlock);
            assert_eq!(visible[0].entity.read(cx).display_text(), "$$n\n^2$$");
            assert_eq!(editor.document.markdown_text(cx), "$$n\n^2$$");
        });
    }

    #[gpui::test]
    async fn auto_created_math_block_exit_shortcut_creates_plain_text_block(
        cx: &mut TestAppContext,
    ) {
        let cx = cx.add_empty_window();
        let editor = cx.new(|cx| Editor::from_markdown(cx, String::new(), None));

        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                let block = editor.document.visible_blocks()[0].entity.clone();
                block.update(cx, |block, block_cx| {
                    block.replace_text_in_visible_range(
                        0..block.visible_len(),
                        "$$",
                        None,
                        false,
                        block_cx,
                    );
                    block.move_to(block.visible_len(), block_cx);
                    block.on_newline(&Newline, window, block_cx);
                    block.on_exit_code_block(&ExitCodeBlock, window, block_cx);
                });
            });
        });

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 2);
            assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::MathBlock);
            assert_eq!(visible[0].entity.read(cx).display_text(), "$$\n\n$$");
            assert_eq!(visible[1].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(visible[1].entity.read(cx).display_text(), "");
            assert_eq!(editor.document.markdown_text(cx), "$$\n\n$$\n\n");
        });
    }

    #[gpui::test]
    async fn raw_like_block_exit_shortcut_creates_plain_text_block(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        let cases = [
            (
                BlockRecord::html("<div>\ncontent\n</div>"),
                BlockKind::HtmlBlock,
                "<div>\ncontent\n</div>",
            ),
            (
                BlockRecord::mermaid("```mermaid\nflowchart LR\nA-->B\n```"),
                BlockKind::MermaidBlock,
                "```mermaid\nflowchart LR\nA-->B\n```",
            ),
            (
                BlockRecord::raw_markdown("::: custom\ncontent\n:::"),
                BlockKind::RawMarkdown,
                "::: custom\ncontent\n:::",
            ),
            (
                BlockRecord::comment("<!--\ncomment\n-->"),
                BlockKind::Comment,
                "<!--\ncomment\n-->",
            ),
        ];

        for (record, kind, text) in cases {
            let editor = cx.new(|cx| {
                let mut editor = Editor::from_markdown(cx, String::new(), None);
                let block = Editor::new_block(cx, record.clone());
                editor.document.replace_roots(vec![block], cx);
                editor
            });

            cx.update(|window, cx| {
                editor.update(cx, |editor, cx| {
                    let block = editor.document.visible_blocks()[0].entity.clone();
                    block.update(cx, |block, block_cx| {
                        block.on_exit_code_block(&ExitCodeBlock, window, block_cx);
                    });
                });
            });

            editor.update(cx, |editor, cx| {
                let visible = editor.document.visible_blocks();
                assert_eq!(visible.len(), 2);
                assert_eq!(visible[0].entity.read(cx).kind(), kind);
                assert_eq!(visible[0].entity.read(cx).display_text(), text);
                assert_eq!(visible[1].entity.read(cx).kind(), BlockKind::Paragraph);
                assert_eq!(visible[1].entity.read(cx).display_text(), "");
            });
        }
    }

