mod tests {
    use gpui::{AppContext, Bounds, Context, TestAppContext, point, px, size};

    use super::super::{CrossBlockSelection, CrossBlockSelectionEndpoint, Editor};
    use crate::components::{Cut, Undo, UndoCaptureKind};
    use crate::i18n::I18nManager;
    use crate::theme::ThemeManager;

    fn init_editor_test_app(cx: &mut TestAppContext) {
        cx.update(|cx| {
            I18nManager::init(cx);
            ThemeManager::init(cx);
            crate::components::init(cx);
        });
    }

    fn redraw(cx: &mut gpui::VisualTestContext) {
        cx.update(|window, cx| window.draw(cx).clear());
        cx.run_until_parked();
    }

    fn set_selection(
        editor: &mut Editor,
        start_index: usize,
        start_offset: usize,
        end_index: usize,
        end_offset: usize,
        cx: &mut Context<Editor>,
    ) {
        let visible = editor.document.visible_blocks().to_vec();
        let start = visible[start_index].entity.entity_id();
        let end = visible[end_index].entity.entity_id();
        editor.cross_block_selection = Some(CrossBlockSelection {
            anchor: CrossBlockSelectionEndpoint {
                entity_id: start,
                offset: start_offset,
            },
            focus: CrossBlockSelectionEndpoint {
                entity_id: end,
                offset: end_offset,
            },
        });
        editor.sync_cross_block_selection_visuals(cx);
    }

    fn assign_visible_block_bounds(editor: &mut Editor, cx: &mut Context<Editor>) {
        for (index, visible) in editor
            .document
            .visible_blocks()
            .to_vec()
            .into_iter()
            .enumerate()
        {
            visible.entity.update(cx, move |block, _cx| {
                block.last_bounds = Some(Bounds::new(
                    point(px(0.0), px(index as f32 * 32.0)),
                    size(px(400.0), px(24.0)),
                ));
            });
        }
    }

    #[test]
    fn mouse_down_starts_cross_block_drag_after_clearing_old_selection() {
        let mut cx = TestAppContext::single();
        init_editor_test_app(&mut cx);
        let editor =
            cx.new(|cx| Editor::from_markdown(cx, "alpha\n\nbeta\n\ngamma".to_string(), None));

        editor.update(&mut cx, |editor, cx| {
            assign_visible_block_bounds(editor, cx);
            set_selection(editor, 0, 0, 2, 2, cx);
            assert!(editor.cross_block_selection.is_some());
            assert!(
                editor
                    .document
                    .visible_blocks()
                    .iter()
                    .any(|visible| visible.entity.read(cx).editor_selection_range.is_some())
            );

            editor.begin_cross_block_drag_at_point(point(px(8.0), px(4.0)), cx);

            assert!(editor.cross_block_selection.is_none());
            assert!(editor.cross_block_drag.is_some());
            assert!(
                editor
                    .document
                    .visible_blocks()
                    .iter()
                    .all(|visible| visible.entity.read(cx).editor_selection_range.is_none())
            );
        });
        cx.quit();
    }

    #[test]
    fn typing_replaces_cross_block_selection_with_plain_text() {
        let mut cx = TestAppContext::single();
        init_editor_test_app(&mut cx);
        let editor =
            cx.new(|cx| Editor::from_markdown(cx, "alpha\n\nbeta\n\ngamma".to_string(), None));

        editor.update(&mut cx, |editor, cx| {
            set_selection(editor, 0, 2, 2, 2, cx);
            assert!(editor.replace_cross_block_selection_with_text(
                "X",
                None,
                false,
                UndoCaptureKind::CoalescibleText,
                cx
            ));

            assert_eq!(editor.document.markdown_text(cx), "alXmma");
            assert!(editor.cross_block_selection.is_none());
            assert!(editor.cross_block_drag.is_none());
            let block = editor.document.visible_blocks()[0].entity.read(cx);
            assert_eq!(block.selected_range, 3..3);
            assert!(block.marked_range.is_none());
        });
        cx.quit();
    }

    #[test]
    fn ime_composition_replaces_cross_block_selection_and_marks_inserted_text() {
        let mut cx = TestAppContext::single();
        init_editor_test_app(&mut cx);
        let editor =
            cx.new(|cx| Editor::from_markdown(cx, "alpha\n\nbeta\n\ngamma".to_string(), None));

        editor.update(&mut cx, |editor, cx| {
            set_selection(editor, 0, 2, 2, 2, cx);
            assert!(editor.replace_cross_block_selection_with_text(
                "ni",
                Some(2..2),
                true,
                UndoCaptureKind::ImeComposition,
                cx
            ));

            assert_eq!(editor.document.markdown_text(cx), "alnimma");
            let block = editor.document.visible_blocks()[0].entity.read(cx);
            assert_eq!(block.selected_range, 4..4);
            assert_eq!(block.marked_range, Some(2..4));
            assert!(block.editor_selection_range.is_none());
        });
        cx.quit();
    }

    #[test]
    fn cross_block_selection_marks_visual_ranges_and_copies_markdown() {
        let mut cx = TestAppContext::single();
        init_editor_test_app(&mut cx);
        let editor = cx.new(|cx| {
            Editor::from_markdown(
                cx,
                "alpha **bold**\n\n- item\n\n![alt](image.png)".to_string(),
                None,
            )
        });

        editor.update(&mut cx, |editor, cx| {
            let visible = editor.document.visible_blocks().to_vec();
            assert_eq!(visible.len(), 3);
            let end_len = visible[2].entity.read(cx).visible_len();
            set_selection(editor, 0, 0, 2, end_len, cx);

            assert_eq!(
                editor.cross_block_selected_markdown(cx).as_deref(),
                Some("alpha **bold**\n\n- item\n\n![alt](image.png)")
            );
            for visible in visible {
                let block = visible.entity.read(cx);
                assert_eq!(block.editor_selection_range, Some(0..block.visible_len()));
            }
        });
        cx.quit();
    }

    #[test]
    fn cross_block_cut_writes_markdown_deletes_range_and_undo_restores() {
        let mut app_cx = TestAppContext::single();
        init_editor_test_app(&mut app_cx);
        let original = "alpha\n\nbeta\n\ngamma";
        let (editor, window_cx) = app_cx.add_window_view({
            let original = original.to_string();
            move |_window, cx| Editor::from_markdown(cx, original.clone(), None)
        });
        redraw(window_cx);

        editor.update(window_cx, |editor, cx| {
            set_selection(editor, 0, 2, 2, 2, cx);
            assert_eq!(
                editor.cross_block_selected_markdown(cx).as_deref(),
                Some("pha\n\nbeta\n\nga")
            );
        });
        redraw(window_cx);

        window_cx.dispatch_action(Cut);
        redraw(window_cx);

        assert_eq!(
            window_cx
                .read_from_clipboard()
                .and_then(|item| item.text())
                .as_deref(),
            Some("pha\n\nbeta\n\nga")
        );
        assert_eq!(
            editor.read_with(window_cx, |editor, cx| editor.document.markdown_text(cx)),
            "almma"
        );

        window_cx.dispatch_action(Undo);
        redraw(window_cx);

        assert_eq!(
            editor.read_with(window_cx, |editor, cx| editor.document.markdown_text(cx)),
            original
        );
        editor.read_with(window_cx, |editor, cx| {
            assert_eq!(
                editor.cross_block_selected_markdown(cx).as_deref(),
                Some("pha\n\nbeta\n\nga")
            );
        });
        drop(editor);
        app_cx.quit();
    }

    const TABLE_DOC: &str = "alpha\n\n| a | b |\n| --- | --- |\n| 1 | 2 |\n\ngamma";

    #[test]
    fn delete_selection_spanning_table_removes_table() {
        let mut cx = TestAppContext::single();
        init_editor_test_app(&mut cx);
        let editor = cx.new(|cx| Editor::from_markdown(cx, TABLE_DOC.to_string(), None));

        editor.update(&mut cx, |editor, cx| {
            let visible = editor.document.visible_blocks().to_vec();
            assert_eq!(visible.len(), 3);
            let end_len = visible[2].entity.read(cx).visible_len();
            // The table sits in the interior of the selection.
            set_selection(editor, 0, 0, 2, end_len, cx);
            assert!(editor.delete_cross_block_selection(cx));

            let text = editor.document.markdown_text(cx);
            assert!(!text.contains('|'), "table should be gone: {text:?}");
            assert!(!text.contains("alpha"));
            assert!(!text.contains("gamma"));
        });
        cx.quit();
    }

    #[test]
    fn delete_selection_with_trailing_table_removes_table() {
        let mut cx = TestAppContext::single();
        init_editor_test_app(&mut cx);
        let editor = cx.new(|cx| Editor::from_markdown(cx, TABLE_DOC.to_string(), None));

        editor.update(&mut cx, |editor, cx| {
            assert_eq!(editor.document.visible_blocks().len(), 3);
            // Selection ends at the start of the table block: the previously
            // broken case where the trailing atomic block was left behind.
            set_selection(editor, 0, 0, 1, 0, cx);
            assert!(editor.delete_cross_block_selection(cx));

            // The table is removed in full; only `gamma` survives (deleting from
            // the document start leaves the table's trailing blank line, which
            // reparses to leading empty paragraphs, harmless and trimmable).
            let text = editor.document.markdown_text(cx);
            assert!(
                !text.contains('|'),
                "trailing table should be gone: {text:?}"
            );
            assert_eq!(text.trim(), "gamma");
        });
        cx.quit();
    }

    #[test]
    fn delete_selection_of_only_table_removes_just_the_table() {
        let mut cx = TestAppContext::single();
        init_editor_test_app(&mut cx);
        let editor = cx.new(|cx| Editor::from_markdown(cx, TABLE_DOC.to_string(), None));

        editor.update(&mut cx, |editor, cx| {
            let visible = editor.document.visible_blocks().to_vec();
            assert_eq!(visible.len(), 3);
            let alpha_len = visible[0].entity.read(cx).visible_len();
            // Drag from the end of the paragraph above onto the table: only the
            // table is removed, and re-parse normalizes the spacing.
            set_selection(editor, 0, alpha_len, 1, 0, cx);
            assert!(editor.delete_cross_block_selection(cx));

            assert_eq!(editor.document.markdown_text(cx), "alpha\n\ngamma");
        });
        cx.quit();
    }

    #[test]
    fn cut_selection_including_table_serializes_and_deletes_it() {
        // Exercise cut's two halves directly (the clipboard markdown and the
        // deleted source range) rather than dispatching the action, keeping this
        // a focused unit test of the cut logic.
        let mut cx = TestAppContext::single();
        init_editor_test_app(&mut cx);
        let editor = cx.new(|cx| Editor::from_markdown(cx, TABLE_DOC.to_string(), None));

        editor.update(&mut cx, |editor, cx| {
            let visible = editor.document.visible_blocks().to_vec();
            assert_eq!(visible.len(), 3);
            let end_len = visible[2].entity.read(cx).visible_len();
            set_selection(editor, 0, 0, 2, end_len, cx);

            // The clipboard markdown serializes the full table, matching what
            // delete removes; otherwise cut would drop it from the clipboard.
            let markdown = editor.cross_block_selected_markdown(cx).unwrap();
            assert!(markdown.contains("| a | b |"), "clipboard: {markdown:?}");
            assert!(markdown.contains("| 1 | 2 |"), "clipboard: {markdown:?}");
            assert!(markdown.contains("alpha") && markdown.contains("gamma"));

            assert!(editor.delete_cross_block_selection(cx));
            assert!(
                !editor.document.markdown_text(cx).contains('|'),
                "document should no longer contain the table"
            );
        });
        cx.quit();
    }

    #[test]
    fn delete_selection_spanning_code_block_removes_it() {
        let mut cx = TestAppContext::single();
        init_editor_test_app(&mut cx);
        // Code blocks edit their raw text, so they are deletable as an ordinary
        // text range; this documents that visible_len-based behavior.
        let doc = "alpha\n\n```\ncode\n```\n\ngamma";
        let editor = cx.new(|cx| Editor::from_markdown(cx, doc.to_string(), None));

        editor.update(&mut cx, |editor, cx| {
            let visible = editor.document.visible_blocks().to_vec();
            assert_eq!(visible.len(), 3);
            let end_len = visible[2].entity.read(cx).visible_len();
            set_selection(editor, 0, 0, 2, end_len, cx);
            assert!(editor.delete_cross_block_selection(cx));

            let text = editor.document.markdown_text(cx);
            assert!(
                !text.contains("code"),
                "code block should be gone: {text:?}"
            );
        });
        cx.quit();
    }

    #[test]
    fn delete_selection_ending_on_trailing_empty_paragraph_removes_table() {
        let mut cx = TestAppContext::single();
        init_editor_test_app(&mut cx);
        let doc = "alpha\n\n| a | b |\n| --- | --- |\n| 1 | 2 |";
        let editor = cx.new(|cx| Editor::from_markdown(cx, doc.to_string(), None));

        editor.update(&mut cx, |editor, cx| {
            // Append a trailing empty paragraph, exactly as inserting a table at
            // the end of a document does. Ending the selection on it used to
            // abort deletion because empty roots had no source span.
            let empty =
                Editor::new_block(cx, crate::components::BlockRecord::paragraph(String::new()));
            let index = editor.document.root_count();
            editor
                .document
                .insert_blocks_at(None, index, vec![empty], cx);

            let visible = editor.document.visible_blocks().to_vec();
            assert_eq!(visible.len(), 3);
            let alpha_len = visible[0].entity.read(cx).visible_len();
            // From the end of `alpha` onto the trailing empty paragraph.
            set_selection(editor, 0, alpha_len, 2, 0, cx);
            assert!(editor.delete_cross_block_selection(cx));

            let text = editor.document.markdown_text(cx);
            assert!(!text.contains('|'), "table should be gone: {text:?}");
            assert_eq!(text.trim(), "alpha");
        });
        cx.quit();
    }

    #[test]
    fn delete_selection_starting_on_empty_paragraph_removes_table() {
        let mut cx = TestAppContext::single();
        init_editor_test_app(&mut cx);
        let doc = "| a | b |\n| --- | --- |\n| 1 | 2 |\n\ngamma";
        let editor = cx.new(|cx| Editor::from_markdown(cx, doc.to_string(), None));

        editor.update(&mut cx, |editor, cx| {
            // Prepend a leading empty paragraph; starting the highlight on it used
            // to abort deletion (the user's "drag up from the text below into an
            // empty block above the table" case).
            let empty =
                Editor::new_block(cx, crate::components::BlockRecord::paragraph(String::new()));
            editor.document.insert_blocks_at(None, 0, vec![empty], cx);

            let visible = editor.document.visible_blocks().to_vec();
            assert_eq!(visible.len(), 3);
            // From the empty paragraph (index 0) to the start of `gamma`.
            set_selection(editor, 0, 0, 2, 0, cx);
            assert!(editor.delete_cross_block_selection(cx));


            let text = editor.document.markdown_text(cx);
            assert!(!text.contains('|'), "table should be gone: {text:?}");
            assert_eq!(text.trim(), "gamma");
        });
        cx.quit();
    }
}
