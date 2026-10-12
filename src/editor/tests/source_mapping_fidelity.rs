use super::common::*;

#[gpui::test]
async fn core_repair_inline_offsets_use_the_original_markdown(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let mut mismatches = Vec::new();
    for source in [
        "a <strong>b</strong> Z",
        "a ` b ` Z",
        "a <br />Z",
        "a \\*b\\* Z",
        "> a <strong>b</strong> Z",
        "甲 <strong>乙</strong> Z",
        "# a <strong>b</strong> Z #",
        "- a <strong>b</strong> Z",
        "> a <strong>b</strong>\n> Z",
        "a ` b\nc ` Z",
        "a [<strong>b</strong>](https://example.com) Z",
        "a [b][ref] Z\n\n[ref]: https://example.com",
        "a [^1] Z\n\n[^1]: footnote",
        "> [!NOTE] a <strong>b</strong> Z\n> body",
        "[^1]: a <strong>b</strong> Z",
    ] {
        let editor = cx.new(|cx| Editor::from_markdown(cx, source.into(), None));
        editor.read_with(cx, |editor, cx| {
            let target = editor
                .document
                .visible_blocks()
                .iter()
                .find(|entry| entry.entity.read(cx).display_text().contains('Z'))
                .unwrap()
                .entity
                .clone();
            let offset = target.read(cx).display_text().rfind('Z').unwrap();
            let actual = editor.caret_source_offset(target.entity_id(), offset, cx);
            if actual != source.rfind('Z') {
                mismatches.push((source, actual, source.rfind('Z')));
            }
        });
    }
    assert!(mismatches.is_empty(), "{mismatches:?}");
}

#[gpui::test]
async fn core_repair_heading_content_end_stays_before_the_original_closing_marker(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    let source = "# a <strong>b</strong> #";
    let editor = cx.new(|cx| Editor::from_markdown(cx, source.into(), None));
    editor.read_with(cx, |editor, cx| {
        let heading = editor.document.first_root().unwrap();
        assert_eq!(
            editor.caret_source_offset(heading.entity_id(), heading.read(cx).visible_len(), cx),
            source.rfind(" #")
        );
    });
}

#[gpui::test]
async fn core_repair_table_inline_offsets_use_the_original_markdown(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let source = "| a <strong>b</strong> Z |\n| --- |\n| c |";
    let editor = cx.new(|cx| Editor::from_markdown(cx, source.into(), None));
    editor.read_with(cx, |editor, cx| {
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
        let offset = cell.read(cx).display_text().rfind('Z').unwrap();
        assert_eq!(
            editor.caret_source_offset(cell.entity_id(), offset, cx),
            source.rfind('Z')
        );
    });
}

#[gpui::test]
async fn core_repair_multiline_paste_after_html_markup_preserves_the_original_bytes(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    let source = "a <strong>b</strong> Z";
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, source.into(), None));
    redraw(cx);
    let block = editor.read_with(cx, |editor, _| {
        editor.document.first_root().unwrap().clone()
    });
    cx.update(|window, cx| {
        cx.write_to_clipboard(gpui::ClipboardItem::new_string("X\nY".into()));
        block.update(cx, |block, cx| {
            let clean = block.record.title.visible_text().rfind('Z').unwrap();
            block.move_to(block.clean_to_current_cursor_offset(clean), cx);
            block.on_paste_as_plain_text(&crate::components::PasteAsPlainText, window, cx);
        });
    });
    redraw(cx);
    assert_eq!(
        editor.read_with(cx, |editor, _| editor.buffer.text()),
        "a <strong>b</strong> X\n\nYZ"
    );
}

#[gpui::test]
async fn core_repair_typing_after_html_markup_preserves_the_original_bytes(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    let source = "a <strong>b</strong> Z";
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, source.into(), None));
    redraw(cx);
    let block = editor.read_with(cx, |editor, _| {
        editor.document.first_root().unwrap().clone()
    });
    block.update_in(cx, |block, window, cx| {
        let clean = block.record.title.visible_text().rfind('Z').unwrap();
        block.move_to(block.clean_to_current_cursor_offset(clean), cx);
        block.replace_text_in_range(None, "中", window, cx);
    });
    redraw(cx);
    assert_eq!(
        editor.read_with(cx, |editor, _| editor.buffer.text()),
        "a <strong>b</strong> 中Z"
    );
}

#[gpui::test]
async fn core_repair_cross_block_copy_and_replace_use_the_original_html_boundary(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    let source = "a <strong>b</strong> Z\n\ntail";
    let editor = cx.new(|cx| Editor::from_markdown(cx, source.into(), None));
    editor.update(cx, |editor, cx| {
        let first = editor.document.first_root().unwrap().clone();
        let last = editor.document.root_blocks().last().unwrap().clone();
        editor.cross_block_selection = Some(crate::editor::CrossBlockSelection {
            anchor: crate::editor::CrossBlockSelectionEndpoint {
                entity_id: first.entity_id(),
                offset: first
                    .read(cx)
                    .record
                    .title
                    .visible_text()
                    .rfind('Z')
                    .unwrap(),
            },
            focus: crate::editor::CrossBlockSelectionEndpoint {
                entity_id: last.entity_id(),
                offset: last.read(cx).clean_visible_len(),
            },
        });
        assert_eq!(
            editor.cross_block_selected_markdown(cx).as_deref(),
            Some("Z\n\ntail")
        );
        assert!(editor.replace_cross_block_selection_with_text(
            "X",
            None,
            false,
            UndoCaptureKind::NonCoalescible,
            cx
        ));
        assert_eq!(editor.buffer.text(), "a <strong>b</strong> X");
        editor.undo_document(cx);
        assert_eq!(editor.buffer.text(), source);
    });
}
