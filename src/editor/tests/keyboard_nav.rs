use super::common::*;

#[gpui::test]
async fn toggle_view_mode_preserves_paragraph_caret_position(cx: &mut TestAppContext) {
    let editor = cx.new(|cx| Editor::from_markdown(cx, "alpha\n\nbeta".to_string(), None));

    editor.update(cx, |editor, cx| {
        let target = editor.document.visible_blocks()[1].entity.clone();
        target.update(cx, |block, _cx| {
            block.selected_range = 2..2;
        });
        editor.active_entity_id = Some(target.entity_id());

        editor.toggle_view_mode(cx);
        assert!(matches!(editor.view_mode, ViewMode::Source));
        let source = editor.document.first_root().expect("source root").clone();
        assert_eq!(source.read(cx).selected_range, 9..9);
        assert!(source.read(cx).show_source_line_numbers());

        editor.toggle_view_mode(cx);
        assert!(matches!(editor.view_mode, ViewMode::Rendered));
        let visible = editor.document.visible_blocks();
        assert_eq!(visible.len(), 2);
        assert!(
            visible
                .iter()
                .all(|visible| !visible.entity.read(cx).show_source_line_numbers())
        );
        assert_eq!(visible[1].entity.read(cx).display_text(), "beta");
        assert_eq!(visible[1].entity.read(cx).selected_range, 2..2);
        assert_eq!(editor.pending_focus, Some(visible[1].entity.entity_id()));
    });
}

#[gpui::test]
async fn toggle_view_mode_ends_stale_code_block_pointer_selection(cx: &mut TestAppContext) {
    let editor =
        cx.new(|cx| Editor::from_markdown(cx, "```rust\nfn main() {}\n```".to_string(), None));

    editor.update(cx, |editor, cx| {
        let target = editor.document.visible_blocks()[0].entity.clone();
        target.update(cx, |block, _cx| {
            block.selected_range = 3..7;
            block.is_selecting = true;
            block.code_language_is_selecting = true;
        });
        editor.active_entity_id = Some(target.entity_id());

        editor.toggle_view_mode(cx);

        assert!(matches!(editor.view_mode, ViewMode::Source));
        target.read_with(cx, |block, _cx| {
            assert!(!block.is_selecting);
            assert!(!block.code_language_is_selecting);
            assert_eq!(block.selected_range, 3..7);
        });
    });
}

#[gpui::test]
async fn ctrl_tab_toggles_view_mode(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "alpha".to_string(), None));

    redraw(cx);
    cx.simulate_keystrokes("ctrl-tab");
    redraw(cx);

    editor.update(cx, |editor, _cx| {
        assert!(matches!(editor.view_mode, ViewMode::Source));
    });

    cx.simulate_keystrokes("ctrl-tab");
    redraw(cx);

    editor.update(cx, |editor, _cx| {
        assert!(matches!(editor.view_mode, ViewMode::Rendered));
    });
}

#[gpui::test]
async fn ctrl_a_selects_entire_source_document_in_source_mode(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "alpha\n\nbeta".to_string(), None)
    });

    editor.update(cx, |editor, cx| {
        editor.toggle_view_mode(cx);
        assert!(matches!(editor.view_mode, ViewMode::Source));
        let source = editor.document.visible_blocks()[0].entity.clone();
        editor.focus_block(source.entity_id());
        source.update(cx, |block, _cx| {
            block.selected_range = 1..3;
        });
    });
    redraw(cx);

    cx.simulate_keystrokes("ctrl-a");
    redraw(cx);

    editor.read_with(cx, |editor, cx| {
        let source = editor.document.visible_blocks()[0].entity.read(cx);
        assert_eq!(source.selected_range, 0..source.visible_len());
        assert!(editor.cross_block_selection.is_none());
    });
}

#[gpui::test]
async fn ctrl_a_selects_only_focused_block_text_in_rendered_mode(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "alpha\n\nbeta".to_string(), None)
    });

    editor.update(cx, |editor, cx| {
        let block = editor.document.visible_blocks()[1].entity.clone();
        editor.focus_block(block.entity_id());
        block.update(cx, |block, _cx| {
            block.selected_range = 1..1;
        });
    });
    redraw(cx);

    cx.simulate_keystrokes("ctrl-a");
    redraw(cx);

    editor.read_with(cx, |editor, cx| {
        let first = editor.document.visible_blocks()[0].entity.read(cx);
        let second = editor.document.visible_blocks()[1].entity.read(cx);
        assert_eq!(first.selected_range, 0..0);
        assert_eq!(second.selected_range, 0..second.visible_len());
        assert!(editor.cross_block_selection.is_none());
    });
}

#[gpui::test]
async fn repeated_ctrl_a_selects_all_rendered_blocks(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let markdown =
        "alpha\n\n| a | b |\n| --- | --- |\n| 1 | 2 |\n\n```rust\nfn main() {}\n```\n\ngamma";
    let (editor, cx) = cx.add_window_view({
        let markdown = markdown.to_string();
        move |_window, cx| Editor::from_markdown(cx, markdown.clone(), None)
    });

    editor.update(cx, |editor, cx| {
        let block = editor.document.visible_blocks()[0].entity.clone();
        editor.focus_block(block.entity_id());
        block.update(cx, |block, block_cx| {
            block.move_to(0, block_cx);
        });
    });
    redraw(cx);

    cx.simulate_keystrokes("ctrl-a");
    redraw(cx);

    editor.read_with(cx, |editor, cx| {
        let first = editor.document.visible_blocks()[0].entity.read(cx);
        assert_eq!(first.selected_range, 0..first.visible_len());
        assert!(editor.cross_block_selection.is_none());
    });

    cx.simulate_keystrokes("ctrl-a");
    redraw(cx);

    editor.read_with(cx, |editor, cx| {
        let visible = editor.document.visible_blocks();
        let first_id = visible[0].entity.entity_id();
        let last = visible.last().expect("visible blocks");
        let last_id = last.entity.entity_id();
        let last_len = last.entity.read(cx).visible_len();
        let selection = editor
            .cross_block_selection
            .expect("second Ctrl+A should select the rendered document");
        assert_eq!(selection.anchor.entity_id, first_id);
        assert_eq!(selection.anchor.offset, 0);
        assert_eq!(selection.focus.entity_id, last_id);
        assert_eq!(selection.focus.offset, last_len);
        for visible in visible {
            let block = visible.entity.read(cx);
            let len = block.visible_len();
            if len > 0 {
                assert_eq!(block.editor_selection_range, Some(0..len));
            }
        }
    });

    let selected_after_second = editor.read_with(cx, |editor, _cx| editor.cross_block_selection);
    cx.simulate_keystrokes("ctrl-a");
    redraw(cx);

    editor.read_with(cx, |editor, cx| {
        assert_eq!(
            editor.cross_block_selection, selected_after_second,
            "third Ctrl+A should keep the full rendered document selected"
        );
        for visible in editor.document.visible_blocks() {
            let block = visible.entity.read(cx);
            let len = block.visible_len();
            if len > 0 {
                assert_eq!(block.editor_selection_range, Some(0..len));
            }
        }
    });
}

#[gpui::test]
async fn rendered_ctrl_a_cycle_expires_before_second_press(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "alpha\n\nbeta".to_string(), None)
    });

    editor.update(cx, |editor, cx| {
        let block = editor.document.visible_blocks()[1].entity.clone();
        editor.focus_block(block.entity_id());
        block.update(cx, |block, block_cx| {
            block.move_to(1, block_cx);
        });
    });
    redraw(cx);

    cx.simulate_keystrokes("ctrl-a");
    redraw(cx);

    editor.update(cx, |editor, cx| {
        let block = editor.document.visible_blocks()[1].entity.clone();
        block.update(cx, |block, _cx| {
            block.selected_range = 1..1;
        });
        let cycle = editor
            .rendered_select_all_cycle
            .as_mut()
            .expect("first Ctrl+A should arm the rendered select-all cycle");
        cycle.last_pressed_at =
            Instant::now() - (Editor::RENDERED_SELECT_ALL_CYCLE_WINDOW + Duration::from_millis(1));
    });

    cx.simulate_keystrokes("ctrl-a");
    redraw(cx);

    editor.read_with(cx, |editor, cx| {
        let second = editor.document.visible_blocks()[1].entity.read(cx);
        assert_eq!(second.selected_range, 0..second.visible_len());
        assert!(editor.cross_block_selection.is_none());
        assert_eq!(
            editor
                .rendered_select_all_cycle
                .expect("cycle should be reset by expired second press")
                .count,
            1
        );
    });
}

#[gpui::test]
async fn tab_key_inserts_tab_in_focused_paragraph(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "ab".to_string(), None));

    editor.update(cx, |editor, cx| {
        let block = editor.document.visible_blocks()[0].entity.clone();
        editor.focus_block(block.entity_id());
        block.update(cx, |block, block_cx| {
            block.move_to(1, block_cx);
        });
    });
    redraw(cx);

    cx.simulate_keystrokes("tab");
    redraw(cx);

    editor.update(cx, |editor, cx| {
        let block = editor.document.visible_blocks()[0].entity.clone();
        assert_eq!(block.read(cx).display_text(), "a    b");
        assert_eq!(editor.document.markdown_text(cx), "a    b");
    });
}

#[gpui::test]
async fn tab_key_inserts_tab_in_focused_code_block(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "```rust\nab\n```".to_string(), None)
    });

    editor.update(cx, |editor, cx| {
        let block = editor.document.visible_blocks()[0].entity.clone();
        editor.focus_block(block.entity_id());
        block.update(cx, |block, block_cx| {
            block.move_to(1, block_cx);
        });
    });
    redraw(cx);

    cx.simulate_keystrokes("tab");
    redraw(cx);

    editor.update(cx, |editor, cx| {
        let block = editor.document.visible_blocks()[0].entity.clone();
        assert_eq!(block.read(cx).display_text(), "a    b");
        assert_eq!(editor.document.markdown_text(cx), "```rust\na    b\n```");
    });
}

#[gpui::test]
async fn captured_tab_key_inserts_visible_indent_in_paragraph(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "ab".to_string(), None));

    editor.update(cx, |editor, cx| {
        let block = editor.document.visible_blocks()[0].entity.clone();
        editor.focus_block(block.entity_id());
        block.update(cx, |block, block_cx| {
            block.move_to(1, block_cx);
        });
    });
    redraw(cx);

    let event = KeyDownEvent {
        keystroke: Keystroke::parse("tab").expect("valid tab keystroke"),
        is_held: false,
    };
    editor.update_in(cx, |editor, window, cx| {
        editor.on_editor_key_down_capture(&event, window, cx);
    });
    redraw(cx);

    editor.update(cx, |editor, cx| {
        let block = editor.document.visible_blocks()[0].entity.clone();
        assert_eq!(block.read(cx).display_text(), "a    b");
    });
}

#[gpui::test]
async fn down_from_code_content_focuses_language_input(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "```rust\nab\n```".to_string(), None)
    });

    // Settle focus on the code content first (and clear any pending focus that a
    // later redraw would otherwise re-apply and steal back).
    editor.update_in(cx, |editor, _window, _cx| {
        let block = editor.document.visible_blocks()[0].entity.clone();
        editor.focus_block(block.entity_id());
    });
    redraw(cx);

    editor.update_in(cx, |editor, window, cx| {
        let block = editor.document.visible_blocks()[0].entity.clone();
        block.update(cx, |block, block_cx| {
            block.move_to(block.visible_len(), block_cx);
            block.on_focus_next(&FocusNext, window, block_cx);
        });
        assert!(
            block.read(cx).code_language_focus_handle.is_focused(window),
            "Down from the last code line should focus the language field"
        );
    });
}

#[gpui::test]
async fn down_from_code_language_at_document_end_creates_trailing_paragraph(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "```rust\nab\n```".to_string(), None)
    });

    editor.update_in(cx, |editor, window, cx| {
        let block = editor.document.visible_blocks()[0].entity.clone();
        editor.focus_block(block.entity_id());
        block.update(cx, |block, block_cx| {
            block.code_language_focus_handle.focus(window);
            block.on_code_language_focus_next(&FocusNext, window, block_cx);
        });
    });
    redraw(cx);

    editor.update(cx, |editor, cx| {
        let roots = editor.document.root_blocks();
        assert_eq!(roots.len(), 2, "a trailing paragraph should be created");
        assert_eq!(roots[1].read(cx).kind(), BlockKind::Paragraph);
        assert_eq!(roots[1].read(cx).display_text(), "");
    });
}

#[gpui::test]
async fn enter_in_code_language_does_not_exit_block(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "```rust\nab\n```".to_string(), None)
    });

    editor.update_in(cx, |editor, window, cx| {
        let block = editor.document.visible_blocks()[0].entity.clone();
        editor.focus_block(block.entity_id());
        block.update(cx, |block, block_cx| {
            block.code_language_focus_handle.focus(window);
            block.on_code_language_newline(&Newline, window, block_cx);
        });
    });
    redraw(cx);

    editor.update(cx, |editor, _cx| {
        // Enter must not leave the block, so no trailing paragraph appears.
        assert_eq!(editor.document.root_count(), 1);
    });
}

#[gpui::test]
async fn captured_tab_key_does_not_modify_code_language_input(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "```rust\nab\n```".to_string(), None)
    });

    editor.update_in(cx, |editor, window, cx| {
        let block = editor.document.visible_blocks()[0].entity.clone();
        editor.focus_block(block.entity_id());
        block.update(cx, |block, block_cx| {
            block.move_to(1, block_cx);
        });
        block.update(cx, |block, _cx| {
            block.code_language_focus_handle.focus(window);
        });
    });
    redraw(cx);

    editor.update_in(cx, |editor, window, cx| {
        let block = editor.document.visible_blocks()[0].entity.clone();
        block.update(cx, |block, _cx| {
            block.code_language_focus_handle.focus(window);
        });
    });

    let event = KeyDownEvent {
        keystroke: Keystroke::parse("tab").expect("valid tab keystroke"),
        is_held: false,
    };
    editor.update_in(cx, |editor, window, cx| {
        editor.on_editor_key_down_capture(&event, window, cx);
    });
    redraw(cx);

    editor.update(cx, |editor, cx| {
        let block = editor.document.visible_blocks()[0].entity.clone();
        let block = block.read(cx);
        assert_eq!(block.code_language_text(), "rust");
        assert_eq!(block.display_text(), "ab");
    });
}

#[gpui::test]
async fn tab_key_keeps_list_indent_semantics(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "- a\n- b".to_string(), None));

    editor.update(cx, |editor, cx| {
        let second = editor.document.visible_blocks()[1].entity.clone();
        editor.focus_block(second.entity_id());
        second.update(cx, |block, block_cx| {
            block.move_to(block.visible_len(), block_cx);
        });
    });
    redraw(cx);

    cx.simulate_keystrokes("tab");
    redraw(cx);

    editor.update(cx, |editor, cx| {
        let visible = editor.document.visible_blocks();
        assert_eq!(visible.len(), 2);
        assert_eq!(visible[1].entity.read(cx).render_depth, 1);
        assert_eq!(editor.document.markdown_text(cx), "- a\n  - b");
    });
}

#[gpui::test]
async fn tab_key_keeps_table_cell_navigation(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let markdown = ["| A | B |", "| --- | --- |", "| 1 | 2 |"].join("\n");
    let (editor, cx) =
        cx.add_window_view(move |_window, cx| Editor::from_markdown(cx, markdown, None));

    let second_cell_id = editor.update(cx, |editor, cx| {
        let table = editor.document.first_root().expect("table root").clone();
        let runtime = table
            .read(cx)
            .table_runtime
            .as_ref()
            .expect("table runtime")
            .clone();
        let first = runtime.rows[0][0].clone();
        let second = runtime.rows[0][1].clone();
        editor.focus_block(first.entity_id());
        first.update(cx, |block, block_cx| {
            block.move_to(block.visible_len(), block_cx);
        });
        second.entity_id()
    });
    redraw(cx);

    cx.simulate_keystrokes("tab");
    redraw(cx);

    editor.update(cx, |editor, _cx| {
        assert_eq!(editor.active_entity_id, Some(second_cell_id));
    });
}

#[gpui::test]
async fn right_arrow_at_cell_end_moves_to_next_cell(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let markdown = ["| A | B |", "| --- | --- |", "| 1 | 2 |"].join("\n");
    let (editor, cx) =
        cx.add_window_view(move |_window, cx| Editor::from_markdown(cx, markdown, None));

    let second_cell_id = editor.update(cx, |editor, cx| {
        let table = editor.document.first_root().expect("table root").clone();
        let runtime = table
            .read(cx)
            .table_runtime
            .as_ref()
            .expect("table runtime")
            .clone();
        let first = runtime.rows[0][0].clone();
        let second = runtime.rows[0][1].clone();
        editor.focus_block(first.entity_id());
        first.update(cx, |block, block_cx| {
            block.move_to(block.visible_len(), block_cx);
        });
        second.entity_id()
    });
    redraw(cx);

    cx.simulate_keystrokes("right");
    redraw(cx);

    editor.update(cx, |editor, _cx| {
        assert_eq!(editor.active_entity_id, Some(second_cell_id));
    });
}

#[gpui::test]
async fn left_arrow_at_cell_start_moves_to_previous_cell(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let markdown = ["| A | B |", "| --- | --- |", "| 1 | 2 |"].join("\n");
    let (editor, cx) =
        cx.add_window_view(move |_window, cx| Editor::from_markdown(cx, markdown, None));

    let first_cell_id = editor.update(cx, |editor, cx| {
        let table = editor.document.first_root().expect("table root").clone();
        let runtime = table
            .read(cx)
            .table_runtime
            .as_ref()
            .expect("table runtime")
            .clone();
        let first = runtime.rows[0][0].clone();
        let second = runtime.rows[0][1].clone();
        editor.focus_block(second.entity_id());
        second.update(cx, |block, block_cx| {
            block.move_to(0, block_cx);
        });
        first.entity_id()
    });
    redraw(cx);

    cx.simulate_keystrokes("left");
    redraw(cx);

    editor.update(cx, |editor, _cx| {
        assert_eq!(editor.active_entity_id, Some(first_cell_id));
    });
}

#[gpui::test]
async fn inserting_table_at_document_end_adds_trailing_paragraph(cx: &mut TestAppContext) {
    let cx = cx.add_empty_window();
    let editor = cx.new(|cx| Editor::from_markdown(cx, String::new(), None));

    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.table_insert_dialog = Some(crate::editor::context_menu::TableInsertDialogState {
                target: crate::editor::context_menu::TableInsertTarget::Append,
                body_rows: 2,
                columns: 2,
            });
            editor.on_confirm_table_insert_dialog(&ClickEvent::default(), window, cx);
        });
    });

    editor.update(cx, |editor, cx| {
        let roots = editor.document.visible_blocks();
        let kinds = roots
            .iter()
            .map(|visible| visible.entity.read(cx).kind())
            .collect::<Vec<_>>();
        let table_index = kinds
            .iter()
            .position(|kind| *kind == BlockKind::Table)
            .expect("table inserted");
        // The table is the last meaningful block, so an empty paragraph is
        // appended after it to give the caret somewhere to land.
        assert_eq!(kinds.get(table_index + 1), Some(&BlockKind::Paragraph));
        assert_eq!(table_index + 1, kinds.len() - 1);
        assert_eq!(roots[table_index + 1].entity.read(cx).display_text(), "");
    });
}

#[gpui::test]
async fn ctrl_enter_exits_focused_math_block(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "$$n^2$$".to_string(), None));

    editor.update(cx, |editor, cx| {
        let block = editor.document.visible_blocks()[0].entity.clone();
        editor.focus_block(block.entity_id());
        block.update(cx, |block, block_cx| {
            block.move_to(block.visible_len(), block_cx);
        });
    });
    redraw(cx);

    cx.simulate_keystrokes("ctrl-enter");
    redraw(cx);

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
async fn ctrl_enter_exits_focused_table_cell(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let markdown = ["| A | B |", "| --- | --- |", "| 1 | 2 |"].join("\n");
    let (editor, cx) =
        cx.add_window_view(move |_window, cx| Editor::from_markdown(cx, markdown, None));

    editor.update(cx, |editor, cx| {
        let table = editor.document.first_root().expect("table root").clone();
        let cell = table
            .read(cx)
            .table_runtime
            .as_ref()
            .expect("table runtime")
            .rows[0][0]
            .clone();
        editor.focus_block(cell.entity_id());
        cell.update(cx, |block, block_cx| {
            block.move_to(block.visible_len(), block_cx);
        });
    });
    redraw(cx);

    cx.simulate_keystrokes("ctrl-enter");
    redraw(cx);

    editor.update(cx, |editor, cx| {
        let visible = editor.document.visible_blocks();
        assert_eq!(visible.len(), 2);
        assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::Table);
        assert_eq!(visible[1].entity.read(cx).kind(), BlockKind::Paragraph);
        assert_eq!(visible[1].entity.read(cx).display_text(), "");
        assert_eq!(editor.active_entity_id, Some(visible[1].entity.entity_id()));
    });
}

#[gpui::test]
async fn ending_editor_pointer_selection_sessions_keeps_normal_selection(cx: &mut TestAppContext) {
    let editor =
        cx.new(|cx| Editor::from_markdown(cx, "```rust\nfn main() {}\n```".to_string(), None));

    editor.update(cx, |editor, cx| {
        let target = editor.document.visible_blocks()[0].entity.clone();
        target.update(cx, |block, _cx| {
            block.selected_range = 3..7;
            block.marked_range = Some(4..6);
            block.is_selecting = true;
        });
        editor.active_entity_id = Some(target.entity_id());

        assert!(editor.end_block_pointer_selection_sessions(cx));
        target.read_with(cx, |block, _cx| {
            assert!(!block.is_selecting);
            assert_eq!(block.selected_range, 3..7);
            assert_eq!(block.marked_range, Some(4..6));
        });

        assert!(!editor.end_block_pointer_selection_sessions(cx));
    });
}

#[gpui::test]
async fn toggle_view_mode_preserves_table_cell_position(cx: &mut TestAppContext) {
    let markdown = ["| Name | Value |", "| --- | --- |", "| alpha | beta |"].join("\n");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.update(cx, |editor, cx| {
        let table = editor.document.first_root().expect("table root").clone();
        let cell = table
            .read(cx)
            .table_runtime
            .as_ref()
            .expect("table runtime")
            .rows[0][1]
            .clone();
        cell.update(cx, |block, _cx| {
            block.selected_range = 2..2;
        });
        editor.active_entity_id = Some(cell.entity_id());

        editor.toggle_view_mode(cx);
        assert!(matches!(editor.view_mode, ViewMode::Source));

        editor.toggle_view_mode(cx);
        assert!(matches!(editor.view_mode, ViewMode::Rendered));
        let restored_table = editor
            .document
            .first_root()
            .expect("restored table")
            .clone();
        let restored_cell = restored_table
            .read(cx)
            .table_runtime
            .as_ref()
            .expect("restored runtime")
            .rows[0][1]
            .clone();
        assert_eq!(restored_cell.read(cx).display_text(), "beta");
        assert_eq!(restored_cell.read(cx).selected_range, 2..2);
        assert_eq!(editor.pending_focus, Some(restored_cell.entity_id()));
    });
}

#[gpui::test]
async fn toggle_view_mode_preserves_callout_table_cell_position(cx: &mut TestAppContext) {
    let markdown = [
        "> [!NOTE]",
        "> | Name | Value |",
        "> | --- | --- |",
        "> | alpha | beta |",
    ]
    .join("\n");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.update(cx, |editor, cx| {
        let callout = editor.document.first_root().expect("callout root").clone();
        let table = callout
            .read(cx)
            .children
            .iter()
            .find(|child| child.read(cx).kind() == BlockKind::Table)
            .expect("nested table child")
            .clone();
        let cell = table
            .read(cx)
            .table_runtime
            .as_ref()
            .expect("table runtime")
            .rows[0][1]
            .clone();
        cell.update(cx, |block, _cx| {
            block.selected_range = 2..2;
        });
        editor.active_entity_id = Some(cell.entity_id());

        editor.toggle_view_mode(cx);
        assert!(matches!(editor.view_mode, ViewMode::Source));

        editor.toggle_view_mode(cx);
        assert!(matches!(editor.view_mode, ViewMode::Rendered));
        let restored_callout = editor
            .document
            .first_root()
            .expect("restored callout")
            .clone();
        let restored_table = restored_callout
            .read(cx)
            .children
            .iter()
            .find(|child| child.read(cx).kind() == BlockKind::Table)
            .expect("restored nested table")
            .clone();
        let restored_cell = restored_table
            .read(cx)
            .table_runtime
            .as_ref()
            .expect("restored runtime")
            .rows[0][1]
            .clone();
        assert_eq!(restored_cell.read(cx).display_text(), "beta");
        assert_eq!(restored_cell.read(cx).selected_range, 2..2);
        assert_eq!(editor.pending_focus, Some(restored_cell.entity_id()));
    });
}

/// 「选择全文」那组用例的夹具：标题 + 中文段落 + 中文列表，三块以上而且正文全是多字节字符。
/// 报修是「全文选择不直观：Ctrl+A 第一次只选当前段落，750ms 内再按一次才选全文」，
/// 所以这里既要多块（一次按下要跨过块边界）也要 CJK（偏移按字符算，按字节会漂）。
const SELECT_DOCUMENT_DOC: &str = "# 标题\n\n第一段中文正文\n\n- 列表甲\n- 列表乙\n";

fn open_select_document_editor<'a>(
    cx: &'a mut TestAppContext,
) -> (gpui::Entity<Editor>, &'a mut VisualTestContext) {
    init_editor_test_app(cx);
    cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, SELECT_DOCUMENT_DOC.to_string(), None)
    })
}

/// 整篇选上的判据：跨块选区从第一块块首选到最后一块块尾，每一块的高亮铺满。
/// 断言的是选区端点与区间，不是像素——无窗口那套文本系统是等宽模拟。
fn assert_whole_document_selected(
    editor: &gpui::Entity<Editor>,
    cx: &mut VisualTestContext,
    pressed: &str,
) {
    editor.read_with(cx, |editor, cx| {
        let visible = editor.document.visible_blocks();
        assert!(
            visible.len() >= 3,
            "{pressed}：夹具该有三块以上，实测 {}",
            visible.len()
        );
        let Some(selection) = editor.cross_block_selection else {
            panic!("{pressed}：整篇没被选上，跨块选区是空的");
        };
        let last = visible.last().expect("夹具至少有最后一块");
        assert_eq!(
            selection.anchor.entity_id,
            visible[0].entity.entity_id(),
            "{pressed}：锚点该落在第一块"
        );
        assert_eq!(selection.anchor.offset, 0, "{pressed}：锚点该在第一块块首");
        assert_eq!(
            selection.focus.entity_id,
            last.entity.entity_id(),
            "{pressed}：落点该落在最后一块"
        );
        assert_eq!(
            selection.focus.offset,
            last.entity.read(cx).clean_visible_len(),
            "{pressed}：落点该在最后一块的块尾"
        );
        for visible in editor.document.visible_blocks() {
            let block = visible.entity.read(cx);
            let len = block.visible_len();
            if len > 0 {
                assert_eq!(
                    block.editor_selection_range,
                    Some(0..len),
                    "{pressed}：这一块的高亮该铺满（{}）",
                    block.display_text()
                );
            }
        }
    });
}

/// 一次按下就把整篇选上，不看 ⌘A 那条 750ms 循环的计时；也不去动那台计数器。
#[gpui::test]
async fn select_document_command_selects_the_whole_rendered_document_in_one_press(
    cx: &mut TestAppContext,
) {
    let (editor, cx) = open_select_document_editor(cx);
    redraw(cx);

    editor.update(cx, |editor, cx| {
        let block = editor.document.visible_blocks()[1].entity.clone();
        editor.focus_block(block.entity_id());
        block.update(cx, |block, block_cx| {
            block.move_to(3, block_cx);
        });
    });
    redraw(cx);

    cx.simulate_keystrokes("ctrl-shift-a");
    redraw(cx);

    assert_whole_document_selected(&editor, cx, "按下「选择全文」那一条");
    editor.read_with(cx, |editor, _cx| {
        assert!(
            editor.rendered_select_all_cycle.is_none(),
            "「选择全文」是一条独立命令，不该去写 ⌘A 那条循环的计数"
        );
    });
}

/// 命令注册表是菜单与命令面板共用的那一份（src/commands.rs）：这条命令要在表里，
/// 标签在本机语言下取得到，并从面板里真的执行得出整篇选择。
#[gpui::test]
async fn select_document_command_runs_from_the_command_palette(cx: &mut TestAppContext) {
    let (editor, cx) = open_select_document_editor(cx);
    redraw(cx);

    assert!(
        crate::commands::commands()
            .iter()
            .any(|spec| spec.id == "select_document"),
        "命令注册表里没有 `select_document`：命令面板与菜单都吃这一份，缺了就搜不到"
    );
    let label = cx.update(|_window, cx| {
        let strings = cx.global::<I18nManager>().strings();
        crate::commands::commands()
            .iter()
            .find(|spec| spec.id == "select_document")
            .map(|spec| spec.label(strings))
            .unwrap_or_default()
    });
    assert!(
        !label.trim().is_empty(),
        "「选择全文」在本机语言下没有标签，面板与菜单上会是一条空行"
    );

    cx.update(|window, cx| {
        window.activate_window();
        editor.update(cx, |editor, cx| editor.toggle_command_palette(window, cx));
    });
    redraw(cx);
    cx.simulate_input(&label);
    cx.simulate_keystrokes("return");
    redraw(cx);

    assert_whole_document_selected(&editor, cx, "从命令面板执行「选择全文」");
    assert!(
        editor.read_with(cx, |editor, _cx| editor.command_palette.is_none()),
        "执行完面板该收起"
    );
}

/// 源码模式只有那一条根块：同一条命令在那儿把整篇源文本选上，不要按了没反应。
#[gpui::test]
async fn select_document_command_selects_the_whole_source_buffer(cx: &mut TestAppContext) {
    let (editor, cx) = open_select_document_editor(cx);
    redraw(cx);

    editor.update(cx, |editor, cx| {
        editor.toggle_view_mode(cx);
        assert!(matches!(editor.view_mode, ViewMode::Source));
        let source = editor.document.visible_blocks()[0].entity.clone();
        editor.focus_block(source.entity_id());
        source.update(cx, |block, block_cx| {
            block.move_to(2, block_cx);
        });
    });
    redraw(cx);

    cx.simulate_keystrokes("ctrl-shift-a");
    redraw(cx);

    editor.read_with(cx, |editor, cx| {
        let source = editor.document.visible_blocks()[0].entity.read(cx);
        assert_eq!(
            source.selected_range,
            0..source.visible_len(),
            "源码模式下「选择全文」该把整篇源文本一次选上"
        );
    });
}

