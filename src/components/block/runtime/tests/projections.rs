
use crate::components::markdown::inline::{
    InlineStyle,
    InlineTextTree,
};
use crate::components::{
    Block, BlockKind, BlockRecord, Newline,
};
use gpui::{
    AppContext,
    TestAppContext,
};



#[gpui::test]
async fn enter_inside_projected_inline_code_inserts_hard_line_without_splitting(
    cx: &mut TestAppContext,
) {
    let cx = cx.add_empty_window();
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("`line 1\nline 2`"),
            ),
        )
    });

    block.update(cx, |block, cx| {
        let offset = "line 1\n".len();
        block.selected_range = offset..offset;
        block.sync_inline_projection_for_focus(true);
        cx.notify();
    });

    cx.update(|window, cx| {
        block.update(cx, |block, block_cx| {
            block.on_newline(&Newline, window, block_cx);
        });
    });

    block.read_with(cx, |block, _cx| {
        let text = "line 1\n\nline 2";
        assert_eq!(block.kind(), BlockKind::Paragraph);
        assert_eq!(block.record.title.visible_text(), text);
        assert!(
            block
                .record
                .title
                .render_cache()
                .spans()
                .iter()
                .any(|span| span.style.code && span.range == (0..text.len()))
        );
    });
}

#[gpui::test]
async fn enter_outside_inline_code_still_splits_paragraph(cx: &mut TestAppContext) {
    let cx = cx.add_empty_window();
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("alpha beta"),
            ),
        )
    });

    block.update(cx, |block, cx| {
        block.selected_range = "alpha".len().."alpha".len();
        cx.notify();
    });

    cx.update(|window, cx| {
        block.update(cx, |block, block_cx| {
            block.on_newline(&Newline, window, block_cx);
        });
    });

    block.read_with(cx, |block, _cx| {
        assert_eq!(block.kind(), BlockKind::Paragraph);
        assert_eq!(block.display_text(), "alpha");
        assert_eq!(block.selected_range, "alpha".len().."alpha".len());
    });
}

#[gpui::test]
async fn enter_inside_comment_block_inserts_hard_line_without_splitting(cx: &mut TestAppContext) {
    let cx = cx.add_empty_window();
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::comment("<!--\n**not bold** [not link](https://example.com)\n-->"),
        )
    });

    block.update(cx, |block, cx| {
        let offset = "<!--\n".len();
        block.selected_range = offset..offset;
        cx.notify();
    });

    cx.update(|window, cx| {
        block.update(cx, |block, block_cx| {
            block.on_newline(&Newline, window, block_cx);
        });
    });

    block.read_with(cx, |block, _cx| {
        assert_eq!(block.kind(), BlockKind::Comment);
        assert_eq!(
            block.display_text(),
            "<!--\n\n**not bold** [not link](https://example.com)\n-->"
        );
        assert_eq!(block.inline_spans().len(), 1);
        assert_eq!(block.inline_spans()[0].range, 0..block.display_text().len());
        assert_eq!(block.inline_spans()[0].style, InlineStyle::default());
    });
}

#[gpui::test]
async fn paragraph_shortcut_creates_task_item_directly(cx: &mut TestAppContext) {
    let block = cx.new(|cx| Block::with_record(cx, BlockRecord::paragraph(String::new())));

    block.update(cx, |block, cx| {
        block.apply_title_edit(
            InlineTextTree::plain("- [x] task"),
            10,
            None,
            None,
            None,
            false,
            cx,
        );
    });

    let kind = block.read_with(cx, |block, _cx| block.kind());
    let text = block.read_with(cx, |block, _cx| block.display_text().to_string());
    assert_eq!(kind, BlockKind::TaskListItem { checked: true });
    assert_eq!(text, "task");
}

#[gpui::test]
async fn paragraph_shortcut_creates_parenthesized_numbered_list_directly(cx: &mut TestAppContext) {
    let block = cx.new(|cx| Block::with_record(cx, BlockRecord::paragraph(String::new())));

    block.update(cx, |block, cx| {
        block.apply_title_edit(
            InlineTextTree::plain("1) item"),
            7,
            None,
            None,
            None,
            false,
            cx,
        );
    });

    let kind = block.read_with(cx, |block, _cx| block.kind());
    let text = block.read_with(cx, |block, _cx| block.display_text().to_string());
    assert_eq!(kind, BlockKind::NumberedListItem);
    assert_eq!(text, "item");
}

#[gpui::test]
async fn bullet_shortcut_upgrades_to_task_item_after_box_prefix(cx: &mut TestAppContext) {
    let block = cx.new(|cx| Block::with_record(cx, BlockRecord::paragraph(String::new())));

    block.update(cx, |block, cx| {
        block.apply_title_edit(InlineTextTree::plain("- "), 2, None, None, None, false, cx);
    });
    let kind = block.read_with(cx, |block, _cx| block.kind());
    assert_eq!(kind, BlockKind::BulletedListItem);

    block.update(cx, |block, cx| {
        block.apply_title_edit(
            InlineTextTree::plain("[ ] "),
            4,
            None,
            None,
            None,
            false,
            cx,
        );
    });

    let kind = block.read_with(cx, |block, _cx| block.kind());
    let text = block.read_with(cx, |block, _cx| block.display_text().to_string());
    assert_eq!(kind, BlockKind::TaskListItem { checked: false });
    assert_eq!(text, "");
}

#[gpui::test]
async fn inline_code_projection_only_expands_touched_span(cx: &mut TestAppContext) {
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("a `code` b"),
            ),
        )
    });

    block.update(cx, |block, _cx| {
        block.selected_range = 0..0;
        block.sync_inline_projection_for_focus(true);
    });
    assert_eq!(
        block.read_with(cx, |block, _cx| block.display_text().to_string()),
        "a code b"
    );

    block.update(cx, |block, _cx| {
        block.selected_range = 2..2;
        block.sync_inline_projection_for_focus(true);
    });
    assert_eq!(
        block.read_with(cx, |block, _cx| block.display_text().to_string()),
        "a `code` b"
    );

    block.update(cx, |block, _cx| {
        block.selected_range = 9..9;
        block.sync_inline_projection_for_focus(true);
    });
    assert_eq!(
        block.read_with(cx, |block, _cx| block.display_text().to_string()),
        "a code b"
    );
}

#[gpui::test]
async fn inline_code_projection_expands_only_the_selected_code_span(cx: &mut TestAppContext) {
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("`one` and `two`"),
            ),
        )
    });

    block.update(cx, |block, _cx| {
        block.selected_range = 1..1;
        block.sync_inline_projection_for_focus(true);
    });
    assert_eq!(
        block.read_with(cx, |block, _cx| block.display_text().to_string()),
        "`one` and two"
    );

    block.update(cx, |block, _cx| {
        block.selected_range = 10..10;
        block.sync_inline_projection_for_focus(true);
    });
    assert_eq!(
        block.read_with(cx, |block, _cx| block.display_text().to_string()),
        "one and `two`"
    );
}

#[gpui::test]
async fn bold_projection_only_expands_touched_span(cx: &mut TestAppContext) {
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("a **bold** b"),
            ),
        )
    });

    block.update(cx, |block, _cx| {
        block.selected_range = 0..0;
        block.sync_inline_projection_for_focus(true);
    });
    assert_eq!(
        block.read_with(cx, |block, _cx| block.display_text().to_string()),
        "a bold b"
    );

    block.update(cx, |block, _cx| {
        block.selected_range = 2..2;
        block.sync_inline_projection_for_focus(true);
    });
    assert_eq!(
        block.read_with(cx, |block, _cx| block.display_text().to_string()),
        "a **bold** b"
    );
}

#[gpui::test]
async fn bold_projection_expands_only_the_selected_bold_span(cx: &mut TestAppContext) {
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("**one** and **two**"),
            ),
        )
    });

    block.update(cx, |block, _cx| {
        block.selected_range = 1..1;
        block.sync_inline_projection_for_focus(true);
    });
    assert_eq!(
        block.read_with(cx, |block, _cx| block.display_text().to_string()),
        "**one** and two"
    );

    block.update(cx, |block, _cx| {
        block.clear_inline_projection();
        block.selected_range = "one and ".len().."one and ".len();
        block.sync_inline_projection_for_focus(true);
    });
    assert_eq!(
        block.read_with(cx, |block, _cx| block.display_text().to_string()),
        "one and **two**"
    );
}

#[gpui::test]
async fn bold_projection_expands_selected_range_and_html_strong(cx: &mut TestAppContext) {
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("a **bold** b"),
            ),
        )
    });

    block.update(cx, |block, _cx| {
        block.selected_range = 2..6;
        block.sync_inline_projection_for_focus(true);
    });
    assert_eq!(
        block.read_with(cx, |block, _cx| block.display_text().to_string()),
        "a **bold** b"
    );

    let html_block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("<strong>bold</strong>"),
            ),
        )
    });

    html_block.update(cx, |block, _cx| {
        block.selected_range = 0..0;
        block.sync_inline_projection_for_focus(true);
    });
    assert_eq!(
        html_block.read_with(cx, |block, _cx| block.display_text().to_string()),
        "**bold**"
    );
}

#[gpui::test]
async fn bold_projection_marker_edit_unwraps_bold_style(cx: &mut TestAppContext) {
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("**bold**"),
            ),
        )
    });

    block.update(cx, |block, cx| {
        block.selected_range = 0..0;
        block.sync_inline_projection_for_focus(true);
        assert_eq!(block.display_text(), "**bold**");
        block.replace_text_in_visible_range(0..2, "", None, false, cx);
    });

    block.read_with(cx, |block, _cx| {
        assert_eq!(block.display_text(), "bold");
        assert_eq!(block.record.title.serialize_markdown(), "bold");
        assert!(
            block
                .record
                .title
                .render_cache()
                .spans()
                .iter()
                .all(|span| !span.style.bold)
        );
    });
}

#[gpui::test]
async fn bold_projection_insertion_inside_span_preserves_bold_style(cx: &mut TestAppContext) {
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("**bold**"),
            ),
        )
    });

    block.update(cx, |block, cx| {
        block.selected_range = 0..0;
        block.sync_inline_projection_for_focus(true);
        assert_eq!(block.display_text(), "**bold**");
        block.replace_text_in_visible_range(3..3, "X", None, false, cx);
    });

    block.read_with(cx, |block, _cx| {
        assert_eq!(block.display_text(), "**bXold**");
        assert_eq!(block.record.title.serialize_markdown(), "**bXold**");
        assert!(block.record.title.render_cache().spans()[0].style.bold);
    });
}

#[gpui::test]
async fn italic_projection_only_expands_touched_span(cx: &mut TestAppContext) {
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("a *italic* b"),
            ),
        )
    });

    block.update(cx, |block, _cx| {
        block.selected_range = 0..0;
        block.sync_inline_projection_for_focus(true);
    });
    assert_eq!(
        block.read_with(cx, |block, _cx| block.display_text().to_string()),
        "a italic b"
    );

    block.update(cx, |block, _cx| {
        block.selected_range = 2..2;
        block.sync_inline_projection_for_focus(true);
    });
    assert_eq!(
        block.read_with(cx, |block, _cx| block.display_text().to_string()),
        "a *italic* b"
    );
}

#[gpui::test]
async fn italic_projection_marker_edit_unwraps_italic_style(cx: &mut TestAppContext) {
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(BlockKind::Paragraph, InlineTextTree::from_markdown("*it*")),
        )
    });

    block.update(cx, |block, cx| {
        block.selected_range = 0..0;
        block.sync_inline_projection_for_focus(true);
        assert_eq!(block.display_text(), "*it*");
        block.replace_text_in_visible_range(0..1, "", None, false, cx);
    });

    block.read_with(cx, |block, _cx| {
        assert_eq!(block.display_text(), "it");
        assert_eq!(block.record.title.serialize_markdown(), "it");
        assert!(
            block
                .record
                .title
                .render_cache()
                .spans()
                .iter()
                .all(|span| !span.style.italic)
        );
    });
}

#[gpui::test]
async fn typing_closing_italic_marker_places_caret_after_marker(cx: &mut TestAppContext) {
    // `*italic` is literal until the closing `*` is typed; afterwards the caret
    // must land *after* the closing marker so further typing stays plain.
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("*italic"),
            ),
        )
    });

    block.update(cx, |block, cx| {
        block.selected_range = 7..7;
        block.sync_inline_projection_for_focus(true);
        assert_eq!(block.display_text(), "*italic");
        block.replace_text_in_visible_range(7..7, "*", None, false, cx);
    });

    block.read_with(cx, |block, _cx| {
        assert_eq!(block.display_text(), "*italic*");
        assert_eq!(block.cursor_offset(), "*italic*".len());
        assert_eq!(
            block.collapsed_caret_affinity,
            super::super::super::CollapsedCaretAffinity::OuterEnd
        );
    });
}

#[gpui::test]
async fn typing_closing_bold_marker_places_caret_after_marker(cx: &mut TestAppContext) {
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("**bold*"),
            ),
        )
    });

    block.update(cx, |block, cx| {
        block.selected_range = 7..7;
        block.sync_inline_projection_for_focus(true);
        assert_eq!(block.display_text(), "**bold*");
        block.replace_text_in_visible_range(7..7, "*", None, false, cx);
    });

    block.read_with(cx, |block, _cx| {
        assert_eq!(block.display_text(), "**bold**");
        assert_eq!(block.cursor_offset(), "**bold**".len());
        assert_eq!(
            block.collapsed_caret_affinity,
            super::super::super::CollapsedCaretAffinity::OuterEnd
        );
    });
}

#[gpui::test]
async fn typing_inside_span_keeps_default_affinity(cx: &mut TestAppContext) {
    // Inserting an ordinary character inside a bold span must not jump the
    // caret outside the span.
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("**bold**"),
            ),
        )
    });

    block.update(cx, |block, cx| {
        block.selected_range = 0..0;
        block.sync_inline_projection_for_focus(true);
        assert_eq!(block.display_text(), "**bold**");
        // Insert "X" inside the bold word (display offset 3 = after "**b").
        block.replace_text_in_visible_range(3..3, "X", None, false, cx);
    });

    block.read_with(cx, |block, _cx| {
        assert_eq!(block.display_text(), "**bXold**");
        assert_eq!(
            block.collapsed_caret_affinity,
            super::super::super::CollapsedCaretAffinity::Default
        );
    });
}

#[gpui::test]
async fn typing_bold_markers_char_by_char_produces_bold_not_italic(cx: &mut TestAppContext) {
    // Typing `**bold**` one character at a time must yield bold, not italic.
    // The clean parse is committed on each keystroke, so the intermediate
    // `**bold*` must not collapse to a literal `*` plus an italic `bold`.
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(BlockKind::Paragraph, InlineTextTree::from_markdown("")),
        )
    });

    block.update(cx, |block, cx| {
        block.selected_range = 0..0;
        block.sync_inline_projection_for_focus(true);
        for ch in "**bold**".chars() {
            let caret = block.cursor_offset();
            block.replace_text_in_visible_range(caret..caret, &ch.to_string(), None, false, cx);
        }
    });

    block.read_with(cx, |block, _cx| {
        assert_eq!(block.record.title.visible_text(), "bold");
        assert_eq!(block.record.title.serialize_markdown(), "**bold**");
        assert!(
            block
                .record
                .title
                .render_cache()
                .spans()
                .iter()
                .all(|span| span.style.bold && !span.style.italic),
            "typed `**bold**` must be bold, not italic"
        );
    });
}

#[gpui::test]
async fn typing_after_closing_italic_marker_inserts_plain_text(cx: &mut TestAppContext) {
    // After typing `*italic*` the caret sits after the closing `*`, so further
    // typing must be plain text rather than being absorbed back into the span.
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(BlockKind::Paragraph, InlineTextTree::from_markdown("")),
        )
    });

    block.update(cx, |block, cx| {
        block.selected_range = 0..0;
        block.sync_inline_projection_for_focus(true);
        for ch in "*italic* x".chars() {
            let caret = block.cursor_offset();
            block.replace_text_in_visible_range(caret..caret, &ch.to_string(), None, false, cx);
        }
    });

    block.read_with(cx, |block, _cx| {
        assert_eq!(block.record.title.visible_text(), "italic x");
        assert_eq!(block.record.title.serialize_markdown(), "*italic* x");
        // The trailing " x" must be a plain (non-italic) fragment.
        let trailing_is_italic = block
            .record
            .title
            .fragments
            .iter()
            .any(|fragment| fragment.text.contains('x') && fragment.style.italic);
        assert!(
            !trailing_is_italic,
            "text after closing `*` must not be italic"
        );
    });
}

#[gpui::test]
async fn typing_after_closing_bold_marker_inserts_plain_text(cx: &mut TestAppContext) {
    // Same as above for bold: typing past the closing `**` must be plain.
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(BlockKind::Paragraph, InlineTextTree::from_markdown("")),
        )
    });

    block.update(cx, |block, cx| {
        block.selected_range = 0..0;
        block.sync_inline_projection_for_focus(true);
        for ch in "**bold** more".chars() {
            let caret = block.cursor_offset();
            block.replace_text_in_visible_range(caret..caret, &ch.to_string(), None, false, cx);
        }
    });

    block.read_with(cx, |block, _cx| {
        assert_eq!(block.record.title.visible_text(), "bold more");
        assert_eq!(block.record.title.serialize_markdown(), "**bold** more");
        let trailing_is_bold = block
            .record
            .title
            .fragments
            .iter()
            .any(|fragment| fragment.text.contains("more") && fragment.style.bold);
        assert!(
            !trailing_is_bold,
            "text after closing `**` must not be bold"
        );
    });
}

#[gpui::test]
async fn strikethrough_projection_only_expands_touched_span(cx: &mut TestAppContext) {
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("a ~~gone~~ b"),
            ),
        )
    });

    block.update(cx, |block, _cx| {
        block.selected_range = 0..0;
        block.sync_inline_projection_for_focus(true);
    });
    assert_eq!(
        block.read_with(cx, |block, _cx| block.display_text().to_string()),
        "a gone b"
    );

    block.update(cx, |block, _cx| {
        block.selected_range = 2..2;
        block.sync_inline_projection_for_focus(true);
    });
    assert_eq!(
        block.read_with(cx, |block, _cx| block.display_text().to_string()),
        "a ~~gone~~ b"
    );
}

#[gpui::test]
async fn script_projection_expands_only_touched_span(cx: &mut TestAppContext) {
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("x^2^ and H~2~O"),
            ),
        )
    });

    block.update(cx, |block, _cx| {
        block.selected_range = 0..0;
        block.sync_inline_projection_for_focus(true);
    });
    assert_eq!(
        block.read_with(cx, |block, _cx| block.display_text().to_string()),
        "x2 and H2O"
    );

    block.update(cx, |block, _cx| {
        block.selected_range = 1..1;
        block.sync_inline_projection_for_focus(true);
    });
    assert_eq!(
        block.read_with(cx, |block, _cx| block.display_text().to_string()),
        "x^2^ and H2O"
    );

    block.update(cx, |block, _cx| {
        block.clear_inline_projection();
        block.selected_range = "x2 and H".len().."x2 and H".len();
        block.sync_inline_projection_for_focus(true);
    });
    assert_eq!(
        block.read_with(cx, |block, _cx| block.display_text().to_string()),
        "x2 and H~2~O"
    );
}

#[gpui::test]
async fn standalone_script_projection_uses_html_marker_fallback(cx: &mut TestAppContext) {
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("<sup>2</sup> and <sub>n</sub>"),
            ),
        )
    });

    block.update(cx, |block, _cx| {
        block.selected_range = 0..0;
        block.sync_inline_projection_for_focus(true);
    });
    assert_eq!(
        block.read_with(cx, |block, _cx| block.display_text().to_string()),
        "<sup>2</sup> and n"
    );

    block.update(cx, |block, _cx| {
        block.clear_inline_projection();
        block.selected_range = "2 and ".len().."2 and ".len();
        block.sync_inline_projection_for_focus(true);
    });
    assert_eq!(
        block.read_with(cx, |block, _cx| block.display_text().to_string()),
        "2 and <sub>n</sub>"
    );
}

