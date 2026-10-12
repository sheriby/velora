use super::common::*;

fn staged_external_change(cx: &mut TestAppContext, save_mode: u8) {
    init_editor_test_app(cx);
    let path = temp_markdown_path("staged-conflict");
    fs::write(&path, "old").unwrap();
    cx.on_quit({ let path = path.clone(); move || fs::remove_file(path).unwrap() });
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "old".into(), Some(path.clone())));
    let recovery_id = editor.read_with(cx, |editor, _| editor.recovery_id);
    cx.cx.on_quit(move || crate::config::remove_recovery_snapshot(recovery_id).unwrap());
    editor.update(cx, |editor, cx| {
        let block = editor.document.first_root().unwrap().clone();
        block.update(cx, |block, _| {
            block.record.set_title(InlineTextTree::plain("ours"));
            block.sync_render_cache();
        });
        editor.mark_dirty(cx);
        editor.save_commit_hook = Some(Box::new({
            let path = path.clone();
            move |_, _| fs::write(path, "external").unwrap()
        }));
    });
    match save_mode {
        1 => editor.update_in(cx, |editor, window, cx| editor.save_dirty_workspace_documents_and_close(window, cx)),
        2 => { editor.update_in(cx, |editor, window, cx| editor.save_to_existing_path(&path, window, cx)); }
        _ => cx.executor().advance_clock(Duration::from_secs(1)),
    }
    cx.run_until_parked();
    assert_eq!(fs::read_to_string(path).unwrap(), "external");
    assert!(!cx.cx.windows().is_empty());
    editor.read_with(cx, |editor, _| {
        assert!(editor.document_dirty);
        assert!(editor.modal_is_open());
    });
}

#[gpui::test]
async fn core_repair_autosave_rechecks_external_changes_at_commit(cx: &mut TestAppContext) {
    staged_external_change(cx, 0);
}

#[gpui::test]
async fn core_repair_save_and_close_rechecks_external_changes_at_commit(cx: &mut TestAppContext) {
    staged_external_change(cx, 1);
}

#[gpui::test]
async fn core_repair_manual_save_rechecks_external_changes_at_commit(cx: &mut TestAppContext) {
    staged_external_change(cx, 2);
}

#[gpui::test]
async fn core_repair_save_as_rejects_a_snapshot_from_before_reload(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let path = temp_markdown_path("save-as-before-reload");
    let target = temp_markdown_path("save-as-after-reload");
    cx.on_quit({ let target = target.clone(); move || { if target.exists() { fs::remove_file(target).unwrap(); } } });
    let editor = cx.new(|cx| Editor::from_markdown(cx, "old".into(), Some(path.clone())));
    editor.update(cx, |editor, cx| {
        let identity = editor.document_save_identity();
        let bytes = editor.document_bytes_for_save();
        let text = editor.document_text_for_save();
        editor.replace_document_from_markdown("reloaded".into(), Some(path.clone()), cx);
        let result = editor.complete_save_request(target.clone(), identity, text, bytes, cx);
        assert!(result.is_err());
        assert_eq!(editor.file_path.as_ref(), Some(&path));
        assert_eq!(editor.buffer.text(), "reloaded");
        assert!(!target.exists());
    });
    if target.exists() { fs::remove_file(target).unwrap(); }
}

#[gpui::test]
async fn core_repair_save_as_accepts_the_current_snapshot(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let path = temp_markdown_path("save-as-current-source");
    let target = temp_markdown_path("save-as-current-target");
    fs::write(&path, "current\n").unwrap();
    cx.on_quit({
        let (path, target) = (path.clone(), target.clone());
        move || { fs::remove_file(path).unwrap(); if target.exists() { fs::remove_file(target).unwrap(); } }
    });
    let document = crate::editor::encoding::load_document(&path).unwrap();
    let editor = cx.new(|cx| Editor::from_loaded_document(cx, document, Some(path.clone())));
    editor.update(cx, |editor, cx| {
        let identity = editor.document_save_identity();
        let bytes = editor.document_bytes_for_save();
        let text = editor.document_text_for_save();
        editor.complete_save_request(target.clone(), identity, text, bytes, cx).unwrap();
        assert_eq!(editor.file_path.as_ref(), Some(&target));
        assert_eq!(fs::read(&target).unwrap(), editor.document_bytes_for_save());
        assert!(!editor.document_dirty);
    });
}
