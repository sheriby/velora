use super::common::*;

#[gpui::test]
async fn core_repair_same_text_ime_commit_separates_following_typing(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "note".into(), None));
    redraw(cx);
    let block = editor.read_with(cx, |editor, _| editor.document.first_root().unwrap().clone());
    block.update(cx, |block, _| block.selected_range = 4..4);
    block.update_in(cx, |block, window, cx| block.replace_and_mark_text_in_range(None, "中", Some(1..1), window, cx));
    redraw(cx);
    block.update_in(cx, |block, window, cx| block.replace_text_in_range(None, "中", window, cx));
    redraw(cx);
    block.update_in(cx, |block, window, cx| block.replace_text_in_range(None, "X", window, cx));
    redraw(cx);
    assert_eq!(editor.read_with(cx, |editor, _| editor.buffer.text()), "note中X");
    editor.update(cx, |editor, cx| editor.undo_document(cx));
    assert_eq!(editor.read_with(cx, |editor, _| editor.buffer.text()), "note中");
    editor.update(cx, |editor, cx| editor.undo_document(cx));
    assert_eq!(editor.read_with(cx, |editor, _| editor.buffer.text()), "note");
}

fn composition_end_autosaves(cx: &mut TestAppContext, unmark: bool) {
    init_editor_test_app(cx);
    let path = temp_markdown_path("same-ime-save");
    fs::write(&path, "note").unwrap();
    cx.on_quit({ let path = path.clone(); move || fs::remove_file(path).unwrap() });
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "note".into(), Some(path.clone())));
    let recovery = editor.read_with(cx, |editor, _| editor.recovery_id);
    cx.cx.on_quit(move || crate::config::remove_recovery_snapshot(recovery).unwrap());
    let block = editor.read_with(cx, |editor, _| editor.document.first_root().unwrap().clone());
    block.update(cx, |block, _| block.selected_range = 4..4);
    block.update_in(cx, |block, window, cx| block.replace_and_mark_text_in_range(None, "中", Some(1..1), window, cx));
    redraw(cx);
    if unmark {
        block.update_in(cx, |block, window, cx| block.unmark_text(window, cx));
    } else {
        block.update_in(cx, |block, window, cx| block.replace_text_in_range(None, "中", window, cx));
    }
    redraw(cx);
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    assert_eq!(fs::read_to_string(path).unwrap(), "note中");
}

#[gpui::test]
async fn core_repair_same_text_ime_commit_resumes_autosave(cx: &mut TestAppContext) {
    composition_end_autosaves(cx, false);
}

#[gpui::test]
async fn core_repair_unmark_resumes_autosave(cx: &mut TestAppContext) {
    composition_end_autosaves(cx, true);
}

#[gpui::test]
async fn core_repair_language_same_text_commit_resumes_autosave(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let source = "```rust\ncode\n```";
    let path = temp_markdown_path("language-ime-save");
    fs::write(&path, source).unwrap();
    cx.on_quit({ let path = path.clone(); move || fs::remove_file(path).unwrap() });
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, source.into(), Some(path.clone())));
    let recovery = editor.read_with(cx, |editor, _| editor.recovery_id);
    cx.cx.on_quit(move || crate::config::remove_recovery_snapshot(recovery).unwrap());
    let block = editor.read_with(cx, |editor, _| editor.document.first_root().unwrap().clone());
    block.update(cx, |block, cx| {
        let length = block.visible_len();
        block.replace_text_in_visible_range(0..length, "edited", None, false, cx);
    });
    block.update_in(cx, |block, window, cx| {
        block.code_language_focus_handle.focus(window);
        block.replace_and_mark_text_in_range(Some(0..4), "rust", Some(4..4), window, cx);
    });
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    assert_eq!(fs::read_to_string(&path).unwrap(), source);
    block.update_in(cx, |block, window, cx| block.replace_text_in_range(None, "rust", window, cx));
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    assert_eq!(fs::read_to_string(path).unwrap(), "```rust\nedited\n```");
}

enum SaveAttempt { Auto, Manual, Close }

fn table_preedit_is_not_saved(cx: &mut TestAppContext, attempt: SaveAttempt) {
    init_editor_test_app(cx);
    let source = "| a | b |\n| --- | --- |\n| c | d |";
    let path = temp_markdown_path("table-ime-save");
    fs::write(&path, source).unwrap();
    cx.on_quit({ let path = path.clone(); move || fs::remove_file(path).unwrap() });
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, source.into(), Some(path.clone())));
    let recovery = editor.read_with(cx, |editor, _| editor.recovery_id);
    cx.cx.on_quit(move || crate::config::remove_recovery_snapshot(recovery).unwrap());
    let cell = editor.read_with(cx, |editor, cx| {
        editor.document.first_root().unwrap().read(cx).table_runtime.as_ref().unwrap().header[0].clone()
    });
    cell.update_in(cx, |block, window, cx| block.replace_and_mark_text_in_range(None, "n", Some(1..1), window, cx));
    match attempt {
        SaveAttempt::Auto => cx.executor().advance_clock(Duration::from_secs(1)),
        SaveAttempt::Manual => { editor.update_in(cx, |editor, window, cx| editor.save_to_existing_path(&path, window, cx)); }
        SaveAttempt::Close => editor.update_in(cx, |editor, window, cx| editor.save_dirty_workspace_documents_and_close(window, cx)),
    }
    cx.run_until_parked();
    assert_eq!(fs::read_to_string(path).unwrap(), source);
    assert!(!cx.cx.windows().is_empty());
}

#[gpui::test]
async fn core_repair_table_preedit_blocks_autosave(cx: &mut TestAppContext) {
    table_preedit_is_not_saved(cx, SaveAttempt::Auto);
}

#[gpui::test]
async fn core_repair_table_preedit_blocks_manual_save(cx: &mut TestAppContext) {
    table_preedit_is_not_saved(cx, SaveAttempt::Manual);
}

#[gpui::test]
async fn core_repair_table_preedit_blocks_save_and_close(cx: &mut TestAppContext) {
    table_preedit_is_not_saved(cx, SaveAttempt::Close);
}
