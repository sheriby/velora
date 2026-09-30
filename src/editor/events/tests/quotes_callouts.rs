    use super::super::Editor;
    use crate::components::{
        BlockEvent, BlockKind, CalloutVariant, Delete, DeleteBack,
        ExitCodeBlock, Newline,
    };
    use gpui::{AppContext, TestAppContext};

    #[gpui::test]
    async fn request_quote_break_creates_new_root_leaf_quote_group(cx: &mut TestAppContext) {
        let editor = cx.new(|cx| Editor::from_markdown(cx, "> first".to_string(), None));

        editor.update(cx, |editor, cx| {
            let quote = editor.document.first_root().expect("root quote").clone();
            editor.on_block_event(quote, &BlockEvent::RequestQuoteBreak, cx);

            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 2);
            assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::Quote);
            assert_eq!(visible[0].entity.read(cx).display_text(), "first");
            assert_eq!(visible[1].entity.read(cx).kind(), BlockKind::Quote);
            assert_eq!(visible[1].entity.read(cx).display_text(), "");
            assert_eq!(visible[1].entity.read(cx).quote_depth, 1);
            assert_eq!(editor.document.markdown_text(cx), "> first\n\n> ");
            assert_eq!(editor.pending_focus, Some(visible[1].entity.entity_id()));
        });
    }

    #[gpui::test]
    async fn typing_quote_shortcut_immediately_refreshes_rendered_quote_metadata(
        cx: &mut TestAppContext,
    ) {
        let editor = cx.new(|cx| Editor::from_markdown(cx, String::new(), None));

        editor.update(cx, |editor, cx| {
            let paragraph = editor
                .document
                .first_root()
                .expect("root paragraph")
                .clone();
            paragraph.update(cx, |block, cx| {
                block.prepare_undo_capture(crate::components::UndoCaptureKind::CoalescibleText, cx);
                block.replace_text_in_visible_range(0..0, "> ", None, false, cx);
            });
        });

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 1);
            assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::Quote);
            assert_eq!(visible[0].entity.read(cx).display_text(), "");
            assert_eq!(visible[0].entity.read(cx).quote_depth, 1);
            assert_eq!(editor.document.markdown_text(cx), "> ");
        });
    }

    #[gpui::test]
    async fn footnote_reference_jump_and_backref_follow_in_place_definition(
        cx: &mut TestAppContext,
    ) {
        let markdown = "alpha[^note]\n\n[^note]: Footnote body".to_string();
        let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

        editor.update(cx, |editor, cx| {
            let paragraph = editor
                .document
                .first_root()
                .expect("reference paragraph")
                .clone();
            let definition = editor
                .document
                .visible_blocks()
                .iter()
                .find(|visible| visible.entity.read(cx).kind() == BlockKind::FootnoteDefinition)
                .expect("footnote definition block")
                .entity
                .clone();

            editor.on_block_event(
                paragraph.clone(),
                &BlockEvent::RequestJumpToFootnoteDefinition {
                    id: "note".to_string(),
                },
                cx,
            );
            assert_eq!(editor.pending_focus, Some(definition.entity_id()));
            assert_eq!(definition.read(cx).selected_range, 0..0);

            let expected_backref_range = paragraph
                .read(cx)
                .current_range_for_footnote_occurrence(0)
                .expect("resolved footnote occurrence");
            editor.on_block_event(
                definition.clone(),
                &BlockEvent::RequestJumpToFootnoteBackref {
                    id: "note".to_string(),
                },
                cx,
            );
            assert_eq!(editor.pending_focus, Some(paragraph.entity_id()));
            assert_eq!(paragraph.read(cx).selected_range, expected_backref_range);
        });
    }

    #[gpui::test]
    async fn typing_callout_shortcut_materializes_body_and_focuses_it(cx: &mut TestAppContext) {
        let editor = cx.new(|cx| Editor::from_markdown(cx, String::new(), None));

        editor.update(cx, |editor, cx| {
            let paragraph = editor
                .document
                .first_root()
                .expect("root paragraph")
                .clone();
            paragraph.update(cx, |block, cx| {
                block.prepare_undo_capture(crate::components::UndoCaptureKind::CoalescibleText, cx);
                block.replace_text_in_visible_range(0..0, "> [!NOTE]", None, false, cx);
            });
        });

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 2);
            assert_eq!(
                visible[0].entity.read(cx).kind(),
                BlockKind::Callout(CalloutVariant::Note)
            );
            assert_eq!(visible[1].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(visible[1].entity.read(cx).display_text(), "");
            assert_eq!(visible[1].entity.read(cx).quote_depth, 1);
            assert_eq!(editor.document.markdown_text(cx), "> [!NOTE]\n> ");
            assert_eq!(editor.pending_focus, Some(visible[1].entity.entity_id()));
        });
    }

    #[gpui::test]
    async fn request_quote_break_creates_nested_leaf_quote_group(cx: &mut TestAppContext) {
        let editor = cx.new(|cx| Editor::from_markdown(cx, "> outer\n>> inner".to_string(), None));

        editor.update(cx, |editor, cx| {
            let nested_quote = editor.document.visible_blocks()[1].entity.clone();
            editor.on_block_event(nested_quote, &BlockEvent::RequestQuoteBreak, cx);

            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 4);
            assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::Quote);
            assert_eq!(visible[0].entity.read(cx).display_text(), "outer");
            assert_eq!(visible[1].entity.read(cx).kind(), BlockKind::Quote);
            assert_eq!(visible[1].entity.read(cx).display_text(), "inner");
            assert_eq!(visible[1].entity.read(cx).quote_depth, 2);
            assert_eq!(visible[2].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(visible[2].entity.read(cx).display_text(), "");
            assert_eq!(visible[2].entity.read(cx).quote_depth, 1);
            assert_eq!(visible[3].entity.read(cx).kind(), BlockKind::Quote);
            assert_eq!(visible[3].entity.read(cx).display_text(), "");
            assert_eq!(visible[3].entity.read(cx).quote_depth, 2);
            assert_eq!(
                editor.document.markdown_text(cx),
                "> outer\n> > inner\n> \n> > "
            );
            assert_eq!(editor.pending_focus, Some(visible[3].entity.entity_id()));
        });
    }

    #[gpui::test]
    async fn imported_leaf_quote_backspace_twice_downgrades_to_text_block(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        let editor = cx.new(|cx| Editor::from_markdown(cx, "> a".to_string(), None));

        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                let quote = editor.document.first_root().expect("root quote").clone();
                quote.update(cx, |block, block_cx| {
                    block.move_to(block.visible_len(), block_cx);
                    block.on_delete_back(&DeleteBack, window, block_cx);
                });
            });
        });

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 1);
            assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::Quote);
            assert_eq!(visible[0].entity.read(cx).display_text(), "");
            assert_eq!(visible[0].entity.read(cx).quote_depth, 1);
            assert_eq!(editor.document.markdown_text(cx), "> ");
        });

        let empty_quote_id = editor.update(cx, |editor, _cx| {
            editor
                .document
                .first_root()
                .expect("empty quote")
                .entity_id()
        });

        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                let quote = editor.document.first_root().expect("empty quote").clone();
                quote.update(cx, |block, block_cx| {
                    block.move_to(0, block_cx);
                    block.on_delete_back(&DeleteBack, window, block_cx);
                });
            });
        });

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 1);
            assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(visible[0].entity.read(cx).display_text(), "");
            assert_eq!(visible[0].entity.read(cx).quote_depth, 0);
            assert_eq!(visible[0].entity.entity_id(), empty_quote_id);
            assert_eq!(editor.document.markdown_text(cx), "");
        });
    }

    #[gpui::test]
    async fn shortcut_created_leaf_quote_backspace_twice_downgrades_to_text_block(
        cx: &mut TestAppContext,
    ) {
        let cx = cx.add_empty_window();
        let editor = cx.new(|cx| Editor::from_markdown(cx, String::new(), None));

        editor.update(cx, |editor, cx| {
            let paragraph = editor
                .document
                .first_root()
                .expect("root paragraph")
                .clone();
            paragraph.update(cx, |block, cx| {
                block.prepare_undo_capture(crate::components::UndoCaptureKind::CoalescibleText, cx);
                block.replace_text_in_visible_range(0..0, "> ", None, false, cx);
                block.replace_text_in_visible_range(0..0, "a", None, false, cx);
            });
        });

        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                let quote = editor
                    .document
                    .first_root()
                    .expect("shortcut quote")
                    .clone();
                quote.update(cx, |block, block_cx| {
                    block.move_to(block.visible_len(), block_cx);
                    block.on_delete_back(&DeleteBack, window, block_cx);
                });
            });
        });

        editor.update(cx, |editor, cx| {
            let quote = editor.document.first_root().expect("empty shortcut quote");
            assert_eq!(quote.read(cx).kind(), BlockKind::Quote);
            assert_eq!(quote.read(cx).display_text(), "");
            assert_eq!(quote.read(cx).quote_depth, 1);
            assert_eq!(editor.document.markdown_text(cx), "> ");
        });

        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                let quote = editor
                    .document
                    .first_root()
                    .expect("empty shortcut quote")
                    .clone();
                quote.update(cx, |block, block_cx| {
                    block.move_to(0, block_cx);
                    block.on_delete_back(&DeleteBack, window, block_cx);
                });
            });
        });

        editor.update(cx, |editor, cx| {
            let paragraph = editor
                .document
                .first_root()
                .expect("text block after downgrade");
            assert_eq!(paragraph.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(paragraph.read(cx).display_text(), "");
            assert_eq!(editor.document.markdown_text(cx), "");
        });
    }

    #[gpui::test]
    async fn root_quote_break_then_backspace_keeps_text_block_slot_after_group(
        cx: &mut TestAppContext,
    ) {
        let cx = cx.add_empty_window();
        let editor = cx.new(|cx| Editor::from_markdown(cx, "> side\n>\n> 1234".to_string(), None));

        let new_leaf_id = editor.update(cx, |editor, cx| {
            let quote = editor.document.first_root().expect("group quote").clone();
            editor.on_block_event(quote, &BlockEvent::RequestQuoteBreak, cx);
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 2);
            assert_eq!(visible[1].entity.read(cx).kind(), BlockKind::Quote);
            assert_eq!(visible[1].entity.read(cx).display_text(), "");
            visible[1].entity.entity_id()
        });

        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                let new_leaf = editor.document.visible_blocks()[1].entity.clone();
                new_leaf.update(cx, |block, block_cx| {
                    block.move_to(0, block_cx);
                    block.on_delete_back(&DeleteBack, window, block_cx);
                });
            });
        });

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 2);
            assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::Quote);
            assert_eq!(visible[0].entity.read(cx).display_text(), "side\n\n1234");
            assert_eq!(visible[1].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(visible[1].entity.read(cx).display_text(), "");
            assert_eq!(visible[1].entity.entity_id(), new_leaf_id);
            assert_eq!(visible[1].entity.read(cx).quote_depth, 0);
            assert_eq!(editor.document.markdown_text(cx), "> side\n> \n> 1234\n\n");
        });
    }

    #[gpui::test]
    async fn empty_callout_body_backspace_downgrades_parent_to_quote(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        let editor = cx.new(|cx| Editor::from_markdown(cx, "> [!NOTE]\n> ".to_string(), None));

        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                let body = editor.document.visible_blocks()[1].entity.clone();
                body.update(cx, |block, block_cx| {
                    block.move_to(0, block_cx);
                    block.on_delete_back(&DeleteBack, window, block_cx);
                });
            });
        });

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 1);
            assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::Quote);
            assert_eq!(visible[0].entity.read(cx).display_text(), "[!NOTE]");
            assert_eq!(editor.document.markdown_text(cx), "> \\[!NOTE]");
        });
    }

    #[gpui::test]
    async fn callout_exit_break_creates_plain_text_block(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        let editor = cx.new(|cx| Editor::from_markdown(cx, "> [!TIP]\n> body".to_string(), None));

        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                let body = editor.document.visible_blocks()[1].entity.clone();
                body.update(cx, |block, block_cx| {
                    block.on_exit_code_block(&ExitCodeBlock, window, block_cx);
                });
            });
        });

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 3);
            assert_eq!(
                visible[0].entity.read(cx).kind(),
                BlockKind::Callout(CalloutVariant::Tip)
            );
            assert_eq!(visible[2].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(visible[2].entity.read(cx).display_text(), "");
            assert_eq!(visible[2].entity.read(cx).quote_depth, 0);
            assert_eq!(editor.document.markdown_text(cx), "> [!TIP]\n> body\n\n");
            assert_eq!(editor.pending_focus, Some(visible[2].entity.entity_id()));
        });
    }

    #[gpui::test]
    async fn delete_on_empty_leaf_quote_downgrades_to_text_block(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        let editor = cx.new(|cx| Editor::from_markdown(cx, "> ".to_string(), None));

        let empty_quote_id = editor.update(cx, |editor, _cx| {
            editor
                .document
                .first_root()
                .expect("empty quote")
                .entity_id()
        });

        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                let quote = editor.document.first_root().expect("empty quote").clone();
                quote.update(cx, |block, block_cx| {
                    block.move_to(0, block_cx);
                    block.on_delete(&Delete, window, block_cx);
                });
            });
        });

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 1);
            assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(visible[0].entity.read(cx).display_text(), "");
            assert_eq!(visible[0].entity.entity_id(), empty_quote_id);
            assert_eq!(editor.document.markdown_text(cx), "");
        });
    }

    #[gpui::test]
    async fn quote_container_with_children_does_not_collapse_from_leaf_exit_path(
        cx: &mut TestAppContext,
    ) {
        let cx = cx.add_empty_window();
        let editor = cx.new(|cx| Editor::from_markdown(cx, ">\n> - item".to_string(), None));

        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                let quote = editor
                    .document
                    .first_root()
                    .expect("container quote")
                    .clone();
                quote.update(cx, |block, block_cx| {
                    block.move_to(0, block_cx);
                    block.on_delete_back(&DeleteBack, window, block_cx);
                });
            });
        });

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 2);
            assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::Quote);
            assert_eq!(visible[0].entity.read(cx).display_text(), "");
            assert_eq!(visible[0].entity.read(cx).quote_depth, 1);
            assert!(!visible[0].entity.read(cx).children.is_empty());
            assert_eq!(
                visible[1].entity.read(cx).kind(),
                BlockKind::BulletedListItem
            );
            assert_eq!(editor.document.markdown_text(cx), "> - item");
        });
    }

    #[gpui::test]
    async fn quote_newline_inside_title_stays_in_one_source_authoritative_group(
        cx: &mut TestAppContext,
    ) {
        let editor = cx.new(|cx| Editor::from_markdown(cx, "> firstsecond".to_string(), None));

        editor.update(cx, |editor, cx| {
            let quote = editor.document.first_root().expect("root quote").clone();
            quote.update(cx, |block, cx| {
                block.prepare_undo_capture(crate::components::UndoCaptureKind::NonCoalescible, cx);
                block.replace_text_in_visible_range(5..5, "\n", None, false, cx);
            });

            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 1);
            assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::Quote);
            assert_eq!(visible[0].entity.read(cx).display_text(), "first\nsecond");
            assert_eq!(editor.document.markdown_text(cx), "> first\n> second");
        });
    }

    #[gpui::test]
    async fn root_quote_enter_stays_in_same_group(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        let editor = cx.new(|cx| Editor::from_markdown(cx, "> first".to_string(), None));

        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                let quote = editor.document.first_root().expect("root quote").clone();
                quote.update(cx, |block, block_cx| {
                    block.move_to(block.visible_len(), block_cx);
                });
                quote.update(cx, |block, block_cx| {
                    block.on_newline(&Newline, window, block_cx);
                });
            });
        });

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 2);
            assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::Quote);
            assert_eq!(visible[0].entity.read(cx).display_text(), "first");
            assert_eq!(visible[1].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(visible[1].entity.read(cx).display_text(), "");
            assert_eq!(visible[1].entity.read(cx).quote_depth, 1);
            assert_eq!(editor.document.markdown_text(cx), "> first\n> ");
        });
    }

    #[gpui::test]
    async fn multiline_edit_inside_quote_reparses_into_child_blocks(cx: &mut TestAppContext) {
        let editor = cx.new(|cx| Editor::from_markdown(cx, "> first".to_string(), None));

        editor.update(cx, |editor, cx| {
            let quote = editor.document.first_root().expect("root quote").clone();
            quote.update(cx, |block, cx| {
                block.prepare_undo_capture(crate::components::UndoCaptureKind::NonCoalescible, cx);
                block.replace_text_in_visible_range(5..5, "\n- item", None, false, cx);
            });
        });

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 2);
            assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::Quote);
            assert_eq!(visible[0].entity.read(cx).display_text(), "first");
            assert_eq!(
                visible[1].entity.read(cx).kind(),
                BlockKind::BulletedListItem
            );
            assert_eq!(visible[1].entity.read(cx).display_text(), "item");
            assert_eq!(editor.document.markdown_text(cx), "> first\n> - item");
        });
    }
