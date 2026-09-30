use super::common::*;

#[gpui::test]
async fn export_html_writes_rendered_document_without_changing_editor_state(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);

    let export_path = temp_export_path("rendered-export-html", "html");
    let cleanup_path = export_path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup_path);
    });

    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "# Title\n\nbody".to_string(), None)
    });

    editor.update(cx, |editor, cx| {
        editor.mark_dirty(cx);
        assert!(editor.document_dirty);
        assert!(editor.file_path.is_none());
        editor
            .export_document_to_path(ExportFormat::Html, &export_path, cx)
            .expect("html export should write");
        assert!(editor.document_dirty);
        assert!(editor.file_path.is_none());
    });

    let html = fs::read_to_string(&export_path).expect("read exported html");
    assert!(html.contains("<h1>Title</h1>"));
    assert!(html.contains("<p>body</p>"));
}

#[gpui::test]
async fn export_png_writes_long_image_without_changing_editor_state(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let export_path = temp_export_path("rendered-export-png", "png");
    let cleanup_path = export_path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup_path);
    });

    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "# Title\n\nbody".to_string(), None)
    });

    let export_result = editor.update(cx, |editor, cx| {
        editor.mark_dirty(cx);
        let result = editor.export_document_to_path(ExportFormat::Png, &export_path, cx);
        assert!(editor.document_dirty);
        assert!(editor.file_path.is_none());
        result
    });

    match export_result {
        Ok(()) => {
            let png = fs::read(&export_path).expect("read exported png");
            assert!(png.starts_with(&[0x89, b'P', b'N', b'G']));
        }
        // Chrome 缺失时只要求给出可行动的错误，磁盘上不产生半成品。
        Err(err) => {
            let message = err.to_string();
            assert!(
                message.contains("Chromium") || message.contains("Chrome"),
                "unexpected PNG export error: {message}"
            );
            assert!(!export_path.exists());
        }
    }
}

#[gpui::test]
async fn export_html_uses_source_mode_raw_text(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let export_path = temp_export_path("source-export-html", "html");
    let cleanup_path = export_path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup_path);
    });

    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "rendered".to_string(), None));

    editor.update(cx, |editor, cx| {
        editor.toggle_view_mode(cx);
        let source_block = editor
            .document
            .first_root()
            .expect("source mode should keep one root block")
            .clone();
        source_block.update(cx, |block, _cx| {
            block.record.set_title(InlineTextTree::plain(
                "# Source\n\n<!--\n<strong>visible</strong>\n-->".to_string(),
            ));
            block.sync_render_cache();
        });
        editor
            .export_document_to_path(ExportFormat::Html, &export_path, cx)
            .expect("source html export should write");
    });

    let html = fs::read_to_string(&export_path).expect("read exported html");
    assert!(html.contains("<h1>Source</h1>"));
    assert!(html.contains("class=\"vlt-comment\""));
    assert!(html.contains("&lt;strong&gt;visible&lt;/strong&gt;"));
}

#[gpui::test]
async fn dropped_markdown_replaces_clean_editor_in_current_window(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let dropped_path = temp_markdown_path("drop-clean-replace");
    fs::write(
        &dropped_path,
        "# Dropped\n\n| A | B |\n| --- | --- |\n| 1 | 2 |\n",
    )
    .expect("write dropped markdown");
    let cleanup_path = dropped_path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup_path);
    });

    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "old".to_string(), None));

    editor.update(cx, |editor, cx| {
        editor.toggle_view_mode(cx);
        assert!(editor.view_mode == ViewMode::Source);
    });

    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.request_dropped_markdown_replace(dropped_path.clone(), window, cx);
        });
    });
    redraw(cx);

    editor.read_with(cx, |editor, cx| {
        assert_eq!(editor.file_path.as_ref(), Some(&dropped_path));
        assert!(editor.view_mode == ViewMode::Rendered);
        assert!(!editor.document_dirty);
        assert!(!editor.show_drop_replace_dialog);
        assert_eq!(editor.document.root_count(), 2);
        assert_eq!(
            editor
                .document
                .root_blocks()
                .last()
                .expect("table block")
                .read(cx)
                .kind(),
            BlockKind::Table
        );
        assert!(editor.document.markdown_text(cx).contains("# Dropped"));
    });

    let fallback_source = "!!! note\n  keep this text";
    editor.update(cx, |editor, cx| {
        editor.replace_document_from_markdown(fallback_source.into(), None, cx);
        assert!(matches!(editor.view_mode, ViewMode::Source));
        assert!(editor.source_mode_fallback_required);
        assert_eq!(editor.document.raw_source_text(cx), fallback_source);
    });
    redraw(cx);
    assert_eq!(cx.cx.windows().len(), 1);
}

#[gpui::test]
async fn dropped_paths_pick_first_valid_markdown_file(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let text_path = temp_export_path("drop-ignore-non-markdown", "txt");
    let markdown_path = temp_export_path("drop-pick-markdown", "markdown");
    fs::write(&text_path, "plain").expect("write text");
    fs::write(&markdown_path, "markdown").expect("write markdown");
    let cleanup_text = text_path.clone();
    let cleanup_markdown = markdown_path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup_text);
        let _ = fs::remove_file(&cleanup_markdown);
    });

    assert_eq!(
        Editor::first_dropped_markdown_path(&[text_path, markdown_path.clone()]),
        Some(markdown_path)
    );
}

#[gpui::test]
async fn dropped_paths_pick_first_valid_image_file(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let text_path = temp_export_path("drop-ignore-non-image", "txt");
    let image_path = temp_export_path("drop-pick-image", "png");
    fs::write(&text_path, "plain").expect("write text");
    fs::write(&image_path, b"image bytes").expect("write image");
    let cleanup_text = text_path.clone();
    let cleanup_image = image_path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup_text);
        let _ = fs::remove_file(&cleanup_image);
    });

    assert_eq!(
        Editor::first_dropped_image_path(&[text_path, image_path.clone()]),
        Some(image_path)
    );
}

#[gpui::test]
async fn dirty_drop_waits_for_replace_decision_and_cancel_preserves_document(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);

    let dropped_path = temp_markdown_path("drop-dirty-cancel");
    fs::write(&dropped_path, "dropped").expect("write dropped markdown");
    let cleanup_path = dropped_path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup_path);
    });

    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "current".to_string(), None));
    editor.update(cx, |editor, cx| editor.mark_dirty(cx));

    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.request_dropped_markdown_replace(dropped_path, window, cx);
        });
    });
    redraw(cx);

    editor.read_with(cx, |editor, cx| {
        assert!(editor.document_dirty);
        assert!(editor.show_drop_replace_dialog);
        assert_eq!(editor.document.markdown_text(cx), "current");
        assert!(editor.pending_drop_replace_path.is_some());
    });

    editor.update(cx, |editor, cx| editor.cancel_drop_replace_dialog(cx));

    editor.read_with(cx, |editor, cx| {
        assert!(editor.document_dirty);
        assert!(!editor.show_drop_replace_dialog);
        assert!(editor.pending_drop_replace_path.is_none());
        assert_eq!(editor.document.markdown_text(cx), "current");
    });
}

#[gpui::test]
async fn dirty_drop_can_replace_without_saving(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let dropped_path = temp_markdown_path("drop-dirty-discard");
    fs::write(&dropped_path, "dropped").expect("write dropped markdown");
    let cleanup_path = dropped_path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup_path);
    });

    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "current".to_string(), None));
    editor.update(cx, |editor, cx| editor.mark_dirty(cx));

    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.request_dropped_markdown_replace(dropped_path.clone(), window, cx);
            editor.discard_pending_drop_replace(window, cx);
        });
    });
    redraw(cx);

    editor.read_with(cx, |editor, cx| {
        assert_eq!(editor.file_path.as_ref(), Some(&dropped_path));
        assert_eq!(editor.document.markdown_text(cx), "dropped");
        assert!(!editor.document_dirty);
        assert!(!editor.show_drop_replace_dialog);
    });
}

#[gpui::test]
async fn dirty_drop_saves_existing_document_before_replace(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let current_path = temp_markdown_path("drop-save-current");
    let dropped_path = temp_markdown_path("drop-save-replace");
    fs::write(&current_path, "original").expect("write current markdown");
    fs::write(&dropped_path, "dropped").expect("write dropped markdown");
    let cleanup_current = current_path.clone();
    let cleanup_dropped = dropped_path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup_current);
        let _ = fs::remove_file(&cleanup_dropped);
    });

    let (editor, cx) = cx.add_window_view({
        let current_path = current_path.clone();
        move |_window, cx| Editor::from_markdown(cx, "original".to_string(), Some(current_path))
    });

    editor.update(cx, |editor, cx| {
        let first = editor.document.first_root().expect("current root").clone();
        first.update(cx, |block, _cx| {
            block
                .record
                .set_title(InlineTextTree::plain("edited".to_string()));
            block.sync_render_cache();
        });
        editor.mark_dirty(cx);
    });

    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.request_dropped_markdown_replace(dropped_path.clone(), window, cx);
            editor.save_and_replace_pending_drop(window, cx);
        });
    });
    redraw(cx);

    assert_eq!(
        fs::read_to_string(&current_path).expect("read saved current"),
        "edited"
    );
    editor.read_with(cx, |editor, cx| {
        assert_eq!(editor.file_path.as_ref(), Some(&dropped_path));
        assert_eq!(editor.document.markdown_text(cx), "dropped");
        assert!(!editor.document_dirty);
        assert!(!editor.pending_drop_replace_after_save);
    });
}

