use super::common::*;

#[gpui::test]
async fn core_repair_undo_restores_the_selection_from_before_the_block_edit(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    let mut mismatches = Vec::new();
    for (source, range, reversed, inserted) in [
        ("abc", 1..1, false, "X"),
        ("abc", 1..2, false, "中"),
        ("abc", 1..2, true, "中"),
        ("a <strong>b</strong> Z", 4..4, false, "中"),
        ("a <strong>b</strong> Z", 2..3, true, "中"),
        ("a ` b ` Z", 2..3, true, "中"),
        ("```rust\nabc\n```", 1..2, true, "中"),
        ("```rust\nabc\ndef\n```", 1..5, true, "中"),
        ("> ```rust\n> abc\n> def\n> ```", 1..5, true, "中"),
        ("abc\ndef", 1..5, true, "中"),
        ("> abc\n> def", 1..5, true, "中"),
        ("[^1]: abc\n    def", 1..5, true, "中"),
        (
            "[^1]:\n    ```rust\n    abc\n    def\n    ```",
            1..5,
            true,
            "中",
        ),
        ("[^1]: # abc", 1..2, true, "中"),
        ("[^1]: > abc", 1..2, true, "中"),
        ("[^1]: ```rust\n    abc\n    ```", 1..2, true, "中"),
        ("#", 1..1, false, " "),
    ] {
        let (editor, window_cx) =
            cx.add_window_view(|_, cx| Editor::from_markdown(cx, source.into(), None));
        redraw(window_cx);
        let block = editor.read_with(window_cx, |editor, cx| {
            editor
                .document
                .visible_blocks()
                .iter()
                .find(|entry| entry.entity.read(cx).clean_visible_len() >= range.end)
                .unwrap()
                .entity
                .clone()
        });
        block.update_in(window_cx, |block, window, cx| {
            block.selected_range = block.clean_range_to_display_range(range.clone());
            block.selection_reversed = reversed;
            block.replace_text_in_range(None, inserted, window, cx);
        });
        redraw(window_cx);
        editor.update(window_cx, |editor, cx| editor.undo_document(cx));
        redraw(window_cx);
        let actual = editor.read_with(window_cx, |editor, cx| {
            assert_eq!(editor.buffer.text(), source);
            let block = editor.current_edit_target_from_state(cx).unwrap().read(cx);
            (block.selection_clean_range(), block.selection_reversed)
        });
        if actual != (range.clone(), reversed) {
            mismatches.push((source, range, reversed, actual));
        }
    }
    assert!(mismatches.is_empty(), "{mismatches:?}");
}


/// 撤销组快照的「每一行让开几字节」必须与块重建同一口径。
///
/// `history::prepare_block_undo_capture` 当时把 `source_mapping::measured_block_line_prefixes`
/// 抄了一份自己记账：续行只剥 `quote_depth` 层（少剥了块自己那层引用记号），量不出
/// 前缀时 `saturating_sub` 钳零而不是放弃。快照落点于是漂出这一段——撤销回来的
/// 选区（以及「撤销后接着打字」的落点）与编辑发生的地方对不上。
/// 撤销组快照的「每一行让开几字节」必须与块重建同一口径。
///
/// `history::prepare_block_undo_capture` 当时把 `source_mapping::measured_block_line_prefixes`
/// 抄了一份自己记账：续行只剥 `quote_depth` 层（少剥了块自己那层引用记号），量不出
/// 前缀时 `saturating_sub` 钳零而不是放弃。快照落点于是漂出这一段——撤销回来的
/// 选区（以及「撤销后接着打字」的落点）与编辑发生的地方对不上。
#[gpui::test]
async fn review_undo_snapshot_lands_on_the_edit_in_container_blocks(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let mut mismatches = Vec::new();
    for (source, needle, inserted) in [
        ("> 引用一\n> 引用二", "引用二", "中"),
        ("> 引用一\n>\n> 引用二丙丁", "引用二丙丁", "中"),
        ("> 引用一\n> 引用二", "引用一", "中"),
        ("- 甲\n    - 乙丙丁", "乙丙丁", "中"),
        ("- 甲\n  乙丙丁", "乙丙丁", "中"),
        ("- 甲\n\n  续段丙丁", "续段丙丁", "中"),
    ] {
        let (editor, window_cx) =
            cx.add_window_view(|_, cx| Editor::from_markdown(cx, source.into(), None));
        redraw(window_cx);
        let (block, range) = editor.read_with(window_cx, |editor, cx| {
            let block = editor
                .document
                .visible_blocks()
                .iter()
                .map(|visible| visible.entity.clone())
                .find(|block| block.read(cx).display_text().contains(needle))
                .expect("夹具里该有这一段");
            let offset = block.read(cx).display_text().find(needle).unwrap();
            let range = offset..offset + needle.len();
            (block, range)
        });
        block.update_in(window_cx, |block, window, cx| {
            block.selected_range = block.clean_range_to_display_range(range.clone());
            block.replace_text_in_range(None, inserted, window, cx);
        });
        redraw(window_cx);
        editor.update(window_cx, |editor, cx| editor.undo_document(cx));
        redraw(window_cx);
        let actual = editor.read_with(window_cx, |editor, cx| {
            assert_eq!(editor.buffer.text(), source);
            editor
                .current_edit_target_from_state(cx)
                .unwrap()
                .read(cx)
                .selection_clean_range()
        });
        if actual != range {
            mismatches.push((source, needle, range, actual));
        }
    }
    assert!(mismatches.is_empty(), "{mismatches:?}");
}


#[gpui::test]
async fn core_repair_typing_after_undo_replaces_the_original_selection(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "abc".into(), None));
    redraw(cx);
    let block = editor.read_with(cx, |editor, _| {
        editor.document.first_root().unwrap().clone()
    });
    block.update_in(cx, |block, window, cx| {
        block.selected_range = 1..2;
        block.replace_text_in_range(None, "中", window, cx);
    });
    redraw(cx);
    editor.update(cx, |editor, cx| editor.undo_document(cx));
    redraw(cx);
    cx.simulate_input("X");
    redraw(cx);
    assert_eq!(
        editor.read_with(cx, |editor, _| editor.buffer.text()),
        "aXc"
    );
}

#[gpui::test]
async fn core_repair_undo_code_fence_conversion_restores_the_original_caret(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, "```rust".into(), None));
    redraw(cx);
    let block = editor.read_with(cx, |editor, _| {
        editor.document.first_root().unwrap().clone()
    });
    block.update(cx, |block, cx| block.move_to(block.visible_len(), cx));
    cx.dispatch_action(Newline);
    redraw(cx);
    editor.update(cx, |editor, cx| editor.undo_document(cx));
    redraw(cx);
    editor.read_with(cx, |editor, cx| {
        assert_eq!(editor.buffer.text(), "```rust");
        assert_eq!(
            editor
                .current_edit_target_from_state(cx)
                .unwrap()
                .read(cx)
                .selection_clean_range(),
            7..7
        );
    });
}

#[gpui::test]
async fn core_repair_source_mode_undo_restores_the_pre_edit_selection(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, "abc\ndef".into(), None));
    editor.update(cx, |editor, cx| editor.toggle_view_mode(cx));
    redraw(cx);
    let block = editor.read_with(cx, |editor, _| {
        editor.document.first_root().unwrap().clone()
    });
    block.update_in(cx, |block, window, cx| {
        block.selected_range = 1..5;
        block.selection_reversed = true;
        block.replace_text_in_range(None, "中", window, cx);
    });
    redraw(cx);
    editor.update(cx, |editor, cx| editor.undo_document(cx));
    redraw(cx);
    editor.read_with(cx, |editor, cx| {
        assert_eq!(editor.buffer.text(), "abc\ndef");
        let block = editor.current_edit_target_from_state(cx).unwrap().read(cx);
        assert_eq!(block.selected_range, 1..5);
        assert!(block.selection_reversed);
    });
}

#[gpui::test]
async fn core_repair_cell_undo_restores_the_pre_edit_selection(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let source = "| abc |\n| --- |\n| def |";
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, source.into(), None));
    redraw(cx);
    let cell = editor.update(cx, |editor, cx| {
        let cell = editor
            .document
            .first_root()
            .unwrap()
            .read(cx)
            .table_runtime
            .as_ref()
            .unwrap()
            .header[0]
            .clone();
        editor.focus_block(cell.entity_id());
        cell
    });
    redraw(cx);
    cell.update_in(cx, |block, window, cx| {
        block.selected_range = 1..2;
        block.selection_reversed = true;
        block.replace_text_in_range(None, "中", window, cx);
    });
    redraw(cx);
    editor.update(cx, |editor, cx| editor.undo_document(cx));
    redraw(cx);
    editor.read_with(cx, |editor, cx| {
        assert_eq!(editor.buffer.text(), source);
        let block = editor.current_edit_target_from_state(cx).unwrap().read(cx);
        assert_eq!(block.selection_clean_range(), 1..2);
        assert!(block.selection_reversed);
    });
}

#[gpui::test]
async fn core_repair_keyboard_delete_undo_restores_the_caret_before_range_expansion(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    let mut mismatches = Vec::new();
    for (source, cursor, command) in [
        ("abc", 1, 0),
        ("abc", 1, 1),
        ("abc def", 3, 2),
        ("abc def", 0, 3),
    ] {
        let (editor, cx) =
            cx.add_window_view(|_, cx| Editor::from_markdown(cx, source.into(), None));
        redraw(cx);
        let block = editor.read_with(cx, |editor, _| {
            editor.document.first_root().unwrap().clone()
        });
        block.update_in(cx, |block, window, cx| {
            block.move_to(cursor, cx);
            match command {
                0 => block.on_delete_back(&DeleteBack, window, cx),
                1 => block.on_delete(&Delete, window, cx),
                2 => block.on_word_delete_back(&crate::components::WordDeleteBack, window, cx),
                _ => {
                    block.on_word_delete_forward(&crate::components::WordDeleteForward, window, cx)
                }
            }
        });
        redraw(cx);
        editor.update(cx, |editor, cx| editor.undo_document(cx));
        redraw(cx);
        let actual = editor.read_with(cx, |editor, cx| {
            assert_eq!(editor.buffer.text(), source);
            editor
                .current_edit_target_from_state(cx)
                .unwrap()
                .read(cx)
                .selection_clean_range()
        });
        if actual != (cursor..cursor) {
            mismatches.push((source, cursor, command, actual));
        }
    }
    assert!(mismatches.is_empty(), "{mismatches:?}");
}
