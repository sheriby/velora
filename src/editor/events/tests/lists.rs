    use super::super::Editor;
    use crate::components::{
        BlockEvent, BlockKind, BlockRecord, DeleteBack, Newline,
    };
    use gpui::{AppContext, TestAppContext};

    #[gpui::test]
    async fn typing_numbered_list_shortcut_after_separator_preserves_group_boundary(
        cx: &mut TestAppContext,
    ) {
        let editor =
            cx.new(|cx| Editor::from_markdown(cx, "1. aa\n2. bb\n3. cc".to_string(), None));

        let separator_id = editor.update(cx, |editor, cx| {
            let separator = Editor::new_block(cx, BlockRecord::paragraph(String::new()));
            editor.document.insert_blocks_at(
                None,
                editor.document.root_count(),
                vec![separator.clone()],
                cx,
            );
            separator.entity_id()
        });

        editor.update(cx, |editor, cx| {
            let separator = editor
                .document
                .block_entity_by_id(separator_id)
                .expect("separator paragraph");
            assert!(separator.read(cx).list_group_separator_candidate);
            separator.update(cx, |block, cx| {
                block.prepare_undo_capture(crate::components::UndoCaptureKind::CoalescibleText, cx);
                block.replace_text_in_visible_range(0..0, "1. ", None, false, cx);
            });
        });

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 5);
            assert_eq!(visible[0].entity.read(cx).list_ordinal, Some(1));
            assert_eq!(visible[1].entity.read(cx).list_ordinal, Some(2));
            assert_eq!(visible[2].entity.read(cx).list_ordinal, Some(3));
            assert_eq!(visible[3].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(visible[3].entity.read(cx).display_text(), "");
            assert_eq!(visible[4].entity.entity_id(), separator_id);
            assert_eq!(
                visible[4].entity.read(cx).kind(),
                BlockKind::NumberedListItem
            );
            assert_eq!(visible[4].entity.read(cx).display_text(), "");
            assert_eq!(visible[4].entity.read(cx).list_ordinal, Some(1));
            assert_eq!(
                editor.document.markdown_text(cx),
                "1. aa\n2. bb\n3. cc\n\n1. "
            );
        });
    }

    #[gpui::test]
    async fn request_indent_nests_non_empty_list_item(cx: &mut TestAppContext) {
        let editor = cx.new(|cx| Editor::from_markdown(cx, "- a\n- b".to_string(), None));

        editor.update(cx, |editor, cx| {
            let second = editor.document.visible_blocks()[1].entity.clone();
            editor.on_block_event(second, &BlockEvent::RequestIndent, cx);

            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 2);
            assert_eq!(
                visible[0].entity.read(cx).kind(),
                BlockKind::BulletedListItem
            );
            assert_eq!(
                visible[1].entity.read(cx).kind(),
                BlockKind::BulletedListItem
            );
            assert_eq!(visible[1].entity.read(cx).render_depth, 1);
            assert_eq!(editor.document.markdown_text(cx), "- a\n  - b");
        });
    }

    #[gpui::test]
    async fn request_outdent_lifts_list_child_paragraph_after_parent(cx: &mut TestAppContext) {
        let editor =
            cx.new(|cx| Editor::from_markdown(cx, "- item\n\n  child text".to_string(), None));

        let child_id = editor.update(cx, |editor, cx| {
            let child = editor.document.visible_blocks()[1].entity.clone();
            editor.on_block_event(child.clone(), &BlockEvent::RequestOutdent, cx);
            child.entity_id()
        });

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
            assert_eq!(visible[1].entity.read(cx).render_depth, 0);
            assert_eq!(visible[1].entity.entity_id(), child_id);
            assert_eq!(editor.document.markdown_text(cx), "- item\n\nchild text");
        });
    }

    #[gpui::test]
    async fn empty_list_child_paragraph_backspace_outdents_to_root(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        let editor = cx.new(|cx| Editor::from_markdown(cx, "- item\n\n  child".to_string(), None));

        let child_id = editor.update(cx, |editor, _cx| {
            editor.document.visible_blocks()[1].entity.entity_id()
        });

        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                let child = editor.document.visible_blocks()[1].entity.clone();
                child.update(cx, |block, block_cx| {
                    block.prepare_undo_capture(
                        crate::components::UndoCaptureKind::NonCoalescible,
                        block_cx,
                    );
                    block.replace_text_in_visible_range(
                        0..block.visible_len(),
                        "",
                        None,
                        false,
                        block_cx,
                    );
                    block.move_to(0, block_cx);
                    block.on_delete_back(&DeleteBack, window, block_cx);
                });
            });
        });

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 2);
            assert_eq!(
                visible[0].entity.read(cx).kind(),
                BlockKind::BulletedListItem
            );
            assert_eq!(visible[1].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(visible[1].entity.read(cx).display_text(), "");
            assert_eq!(visible[1].entity.entity_id(), child_id);
            assert_eq!(visible[1].entity.read(cx).render_depth, 0);
            assert_eq!(editor.document.markdown_text(cx), "- item\n\n");
        });
    }

    #[gpui::test]
    async fn empty_list_child_paragraph_enter_continues_same_level(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        let editor = cx.new(|cx| Editor::from_markdown(cx, "- item\n\n  child".to_string(), None));

        let child_id = editor.update(cx, |editor, _cx| {
            editor.document.visible_blocks()[1].entity.entity_id()
        });

        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                let child = editor.document.visible_blocks()[1].entity.clone();
                child.update(cx, |block, block_cx| {
                    block.prepare_undo_capture(
                        crate::components::UndoCaptureKind::NonCoalescible,
                        block_cx,
                    );
                    block.replace_text_in_visible_range(
                        0..block.visible_len(),
                        "",
                        None,
                        false,
                        block_cx,
                    );
                    block.move_to(0, block_cx);
                    block.on_newline(&Newline, window, block_cx);
                });
            });
        });

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 3);
            assert_eq!(
                visible[0].entity.read(cx).kind(),
                BlockKind::BulletedListItem
            );
            assert_eq!(visible[1].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(visible[1].entity.read(cx).display_text(), "");
            assert_eq!(visible[1].entity.entity_id(), child_id);
            assert_eq!(visible[1].entity.read(cx).render_depth, 1);
            assert_eq!(visible[2].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(visible[2].entity.read(cx).display_text(), "");
            assert_eq!(visible[2].entity.read(cx).render_depth, 1);
            assert_eq!(editor.document.markdown_text(cx), "- item\n  \n  ");
        });
    }

    #[gpui::test]
    async fn nested_list_item_backspace_downgrades_to_direct_list_child(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        let editor = cx.new(|cx| Editor::from_markdown(cx, "- a\n  - b".to_string(), None));

        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                let nested = editor.document.visible_blocks()[1].entity.clone();
                nested.update(cx, |block, block_cx| {
                    block.move_to(0, block_cx);
                    block.on_delete_back(&DeleteBack, window, block_cx);
                });
            });
        });

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 2);
            assert_eq!(
                visible[0].entity.read(cx).kind(),
                BlockKind::BulletedListItem
            );
            assert_eq!(visible[1].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(visible[1].entity.read(cx).display_text(), "b");
            assert_eq!(visible[1].entity.read(cx).render_depth, 1);
            assert_eq!(editor.document.markdown_text(cx), "- a\n\n  b");
        });
    }

    #[gpui::test]
    async fn empty_nested_list_item_backspace_twice_exits_to_outer_paragraph(
        cx: &mut TestAppContext,
    ) {
        let cx = cx.add_empty_window();
        let editor = cx.new(|cx| Editor::from_markdown(cx, "- a\n  - ".to_string(), None));

        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                let nested = editor.document.visible_blocks()[1].entity.clone();
                nested.update(cx, |block, block_cx| {
                    block.move_to(0, block_cx);
                    block.on_delete_back(&DeleteBack, window, block_cx);
                });
            });
        });

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 2);
            assert_eq!(visible[1].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(visible[1].entity.read(cx).display_text(), "");
            assert_eq!(visible[1].entity.read(cx).render_depth, 1);
            assert_eq!(editor.document.markdown_text(cx), "- a\n  ");
        });

        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                let child = editor.document.visible_blocks()[1].entity.clone();
                child.update(cx, |block, block_cx| {
                    block.move_to(0, block_cx);
                    block.on_delete_back(&DeleteBack, window, block_cx);
                });
            });
        });

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 2);
            assert_eq!(
                visible[0].entity.read(cx).kind(),
                BlockKind::BulletedListItem
            );
            assert_eq!(visible[1].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(visible[1].entity.read(cx).display_text(), "");
            assert_eq!(visible[1].entity.read(cx).render_depth, 0);
            assert_eq!(editor.document.markdown_text(cx), "- a\n\n");
        });
    }

    #[gpui::test]
    async fn nested_list_item_downgrade_hoists_children_after_paragraph(cx: &mut TestAppContext) {
        let editor =
            cx.new(|cx| Editor::from_markdown(cx, "- a\n  - b\n    - c\n  - d".to_string(), None));

        editor.update(cx, |editor, cx| {
            let nested = editor.document.visible_blocks()[1].entity.clone();
            editor.on_block_event(
                nested,
                &BlockEvent::RequestDowngradeNestedListItemToChildParagraph,
                cx,
            );

            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 4);
            assert_eq!(
                visible[0].entity.read(cx).kind(),
                BlockKind::BulletedListItem
            );
            assert_eq!(visible[1].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(visible[1].entity.read(cx).display_text(), "b");
            assert_eq!(visible[1].entity.read(cx).render_depth, 1);
            assert_eq!(
                visible[2].entity.read(cx).kind(),
                BlockKind::BulletedListItem
            );
            assert_eq!(visible[2].entity.read(cx).display_text(), "c");
            assert_eq!(visible[2].entity.read(cx).render_depth, 1);
            assert_eq!(
                visible[3].entity.read(cx).kind(),
                BlockKind::BulletedListItem
            );
            assert_eq!(visible[3].entity.read(cx).display_text(), "d");
            assert_eq!(visible[3].entity.read(cx).render_depth, 1);
            assert_eq!(
                editor.document.markdown_text(cx),
                "- a\n\n  b\n  - c\n  - d"
            );
        });
    }

    #[gpui::test]
    async fn nested_numbered_and_task_items_backspace_downgrade_to_list_child(
        cx: &mut TestAppContext,
    ) {
        let cx = cx.add_empty_window();

        let numbered = cx.new(|cx| Editor::from_markdown(cx, "1. a\n  1. b".to_string(), None));
        cx.update(|window, cx| {
            numbered.update(cx, |editor, cx| {
                let nested = editor.document.visible_blocks()[1].entity.clone();
                nested.update(cx, |block, block_cx| {
                    block.move_to(0, block_cx);
                    block.on_delete_back(&DeleteBack, window, block_cx);
                });
            });
        });
        numbered.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible[1].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(visible[1].entity.read(cx).display_text(), "b");
            assert_eq!(visible[1].entity.read(cx).render_depth, 1);
            assert_eq!(editor.document.markdown_text(cx), "1. a\n\n  b");
        });

        let task = cx.new(|cx| Editor::from_markdown(cx, "- [ ] a\n  - [ ] b".to_string(), None));
        cx.update(|window, cx| {
            task.update(cx, |editor, cx| {
                let nested = editor.document.visible_blocks()[1].entity.clone();
                nested.update(cx, |block, block_cx| {
                    block.move_to(0, block_cx);
                    block.on_delete_back(&DeleteBack, window, block_cx);
                });
            });
        });
        task.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible[1].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(visible[1].entity.read(cx).display_text(), "b");
            assert_eq!(visible[1].entity.read(cx).render_depth, 1);
            assert_eq!(editor.document.markdown_text(cx), "- [ ] a\n\n  b");
        });
    }

