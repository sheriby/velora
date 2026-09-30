use std::sync::Arc;

use crate::components::markdown::inline::{
    InlineLinkHit, InlineScript,
    InlineTextTree,
};
use crate::components::markdown::link::parse_link_reference_definitions;
use crate::components::{
    Block, BlockKind, BlockRecord,
};
use gpui::{
    AppContext,
    TestAppContext,
};



#[gpui::test]
async fn script_projection_marker_edit_unwraps_script_style(cx: &mut TestAppContext) {
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(BlockKind::Paragraph, InlineTextTree::from_markdown("x^2^")),
        )
    });

    block.update(cx, |block, cx| {
        block.selected_range = 1..1;
        block.sync_inline_projection_for_focus(true);
        assert_eq!(block.display_text(), "x^2^");
        block.replace_text_in_visible_range(1..2, "", None, false, cx);
    });

    block.read_with(cx, |block, _cx| {
        assert_eq!(block.display_text(), "x2");
        assert_eq!(block.record.title.serialize_markdown(), "x2");
        assert!(
            block
                .inline_spans()
                .iter()
                .all(|span| span.style.script == InlineScript::Normal)
        );
    });
}

#[gpui::test]
async fn subscript_projection_marker_edit_unwraps_script_style(cx: &mut TestAppContext) {
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(BlockKind::Paragraph, InlineTextTree::from_markdown("H~2~O")),
        )
    });

    block.update(cx, |block, cx| {
        block.selected_range = 1..1;
        block.sync_inline_projection_for_focus(true);
        assert_eq!(block.display_text(), "H~2~O");
        block.replace_text_in_visible_range(1..2, "", None, false, cx);
    });

    block.read_with(cx, |block, _cx| {
        assert_eq!(block.display_text(), "H2O");
        assert_eq!(block.record.title.serialize_markdown(), "H2O");
        assert!(
            block
                .record
                .title
                .render_cache()
                .spans()
                .iter()
                .all(|span| span.style.script == InlineScript::Normal)
        );
    });
}

#[gpui::test]
async fn script_projection_insertion_inside_span_preserves_script_style(cx: &mut TestAppContext) {
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(BlockKind::Paragraph, InlineTextTree::from_markdown("x^2^")),
        )
    });

    block.update(cx, |block, cx| {
        block.selected_range = 1..1;
        block.sync_inline_projection_for_focus(true);
        assert_eq!(block.display_text(), "x^2^");
        block.replace_text_in_visible_range(3..3, "3", None, false, cx);
    });

    block.read_with(cx, |block, _cx| {
        assert_eq!(block.display_text(), "x^23^");
        assert_eq!(block.record.title.serialize_markdown(), "x^23^");
        assert_eq!(
            block.record.title.render_cache().spans()[1].style.script,
            InlineScript::Superscript
        );
    });
}

#[gpui::test]
async fn inline_code_projection_right_escape_stays_outside_after_rebuild(cx: &mut TestAppContext) {
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("a `123` b"),
            ),
        )
    });

    block.update(cx, |block, _cx| {
        block.selected_range = 5..5;
        block.sync_inline_projection_for_focus(true);
    });
    assert_eq!(block.read_with(cx, |block, _cx| block.cursor_offset()), 6);

    block.update(cx, |block, _cx| {
        let (target, affinity) = block
            .projected_move_right_target(block.cursor_offset())
            .expect("inner end should jump to outer end");
        block.assign_collapsed_selection_offset(target, affinity, None);
        block.sync_inline_projection_for_focus(true);
    });
    assert_eq!(
        block.read_with(cx, |block, _cx| block.display_text().to_string()),
        "a `123` b"
    );
    assert_eq!(block.read_with(cx, |block, _cx| block.cursor_offset()), 7);

    block.update(cx, |block, _cx| {
        let target = block.next_boundary(block.cursor_offset());
        block.move_to_with_preferred_x(target, None, _cx);
    });
    assert_eq!(block.read_with(cx, |block, _cx| block.cursor_offset()), 8);
}

#[gpui::test]
async fn inline_code_projection_left_escape_stays_outside_after_rebuild(cx: &mut TestAppContext) {
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("a `123` b"),
            ),
        )
    });

    block.update(cx, |block, _cx| {
        block.selected_range = 2..2;
        block.sync_inline_projection_for_focus(true);
    });
    assert_eq!(block.read_with(cx, |block, _cx| block.cursor_offset()), 3);

    block.update(cx, |block, _cx| {
        let (target, affinity) = block
            .projected_move_left_target(block.cursor_offset())
            .expect("inner start should jump to outer start");
        block.assign_collapsed_selection_offset(target, affinity, None);
        block.sync_inline_projection_for_focus(true);
    });
    assert_eq!(block.read_with(cx, |block, _cx| block.cursor_offset()), 2);

    block.update(cx, |block, _cx| {
        let target = block.previous_boundary(block.cursor_offset());
        block.move_to_with_preferred_x(target, None, _cx);
    });
    assert_eq!(block.read_with(cx, |block, _cx| block.cursor_offset()), 1);
}

#[gpui::test]
async fn strikethrough_projection_right_escape_stays_outside_after_rebuild(
    cx: &mut TestAppContext,
) {
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("a ~~123~~ b"),
            ),
        )
    });

    block.update(cx, |block, _cx| {
        block.selected_range = 5..5;
        block.sync_inline_projection_for_focus(true);
    });
    assert_eq!(block.read_with(cx, |block, _cx| block.cursor_offset()), 7);

    block.update(cx, |block, _cx| {
        let (target, affinity) = block
            .projected_move_right_target(block.cursor_offset())
            .expect("inner end should jump to outer end");
        block.assign_collapsed_selection_offset(target, affinity, None);
        block.sync_inline_projection_for_focus(true);
    });
    assert_eq!(block.read_with(cx, |block, _cx| block.cursor_offset()), 9);

    block.update(cx, |block, _cx| {
        let target = block.next_boundary(block.cursor_offset());
        block.move_to_with_preferred_x(target, None, _cx);
    });
    assert_eq!(block.read_with(cx, |block, _cx| block.cursor_offset()), 10);
}

#[gpui::test]
async fn strikethrough_projection_left_escape_stays_outside_after_rebuild(cx: &mut TestAppContext) {
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("a ~~bc~~ d"),
            ),
        )
    });

    block.update(cx, |block, _cx| {
        block.selected_range = 2..2;
        block.sync_inline_projection_for_focus(true);
    });
    assert_eq!(block.read_with(cx, |block, _cx| block.cursor_offset()), 4);

    block.update(cx, |block, _cx| {
        let (target, affinity) = block
            .projected_move_left_target(block.cursor_offset())
            .expect("expected projected move left target");
        block.assign_collapsed_selection_offset(target, affinity, None);
        block.sync_inline_projection_for_focus(true);
    });
    assert_eq!(block.read_with(cx, |block, _cx| block.cursor_offset()), 2);

    block.update(cx, |block, _cx| {
        let target = block.previous_boundary(block.cursor_offset());
        block.move_to_with_preferred_x(target, None, _cx);
    });
    assert_eq!(block.read_with(cx, |block, _cx| block.cursor_offset()), 1);
}

#[gpui::test]
async fn word_start_boundaries_step_over_whole_words(cx: &mut TestAppContext) {
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("hello world foo"),
            ),
        )
    });

    block.read_with(cx, |block, _cx| {
        // Word starts are at offsets 0 ("hello"), 6 ("world"), 12 ("foo").
        assert_eq!(block.next_word_start(0), 6);
        assert_eq!(block.next_word_start(3), 6);
        assert_eq!(block.next_word_start(6), 12);
        assert_eq!(block.next_word_start(12), 15);

        assert_eq!(block.previous_word_start(15), 12);
        assert_eq!(block.previous_word_start(12), 6);
        assert_eq!(block.previous_word_start(7), 6);
        assert_eq!(block.previous_word_start(6), 0);
        assert_eq!(block.previous_word_start(0), 0);
    });
}

#[gpui::test]
async fn inline_link_projection_only_expands_touched_span(cx: &mut TestAppContext) {
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("a [link](https://example.com) b"),
            ),
        )
    });

    block.update(cx, |block, _cx| {
        block.selected_range = 0..0;
        block.sync_inline_projection_for_focus(true);
    });
    assert_eq!(
        block.read_with(cx, |block, _cx| block.display_text().to_string()),
        "a link b"
    );

    block.update(cx, |block, _cx| {
        block.selected_range = 2..2;
        block.sync_inline_projection_for_focus(true);
    });
    assert_eq!(
        block.read_with(cx, |block, _cx| block.display_text().to_string()),
        "a [link](https://example.com) b"
    );
}

#[gpui::test]
async fn reference_style_link_resolves_and_expands_preserving_raw_syntax(cx: &mut TestAppContext) {
    let block = cx.new(|cx| {
        let mut block = Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("[reference link][ref-link]"),
            ),
        );
        block.set_runtime_context(
            None,
            Arc::default(),
            Arc::new(parse_link_reference_definitions(
                "[ref-link]: https://example.com",
            )),
            Arc::default(),
        );
        block
    });

    assert_eq!(
        block.read_with(cx, |block, _cx| block.display_text().to_string()),
        "reference link"
    );
    assert_eq!(
        block.read_with(cx, |block, _cx| block.inline_link_at(0).map(str::to_string)),
        Some("https://example.com".to_string())
    );

    block.update(cx, |block, _cx| {
        block.selected_range = 0..0;
        block.sync_inline_projection_for_focus(true);
    });

    assert_eq!(
        block.read_with(cx, |block, _cx| block.display_text().to_string()),
        "[reference link][ref-link]"
    );
    assert_eq!(
        block.read_with(cx, |block, _cx| block.record.title.serialize_markdown()),
        "[reference link][ref-link]"
    );
}

#[gpui::test]
async fn reference_style_link_hit_exposes_raw_prompt_and_resolved_open_target(
    cx: &mut TestAppContext,
) {
    let block = cx.new(|cx| {
        let mut block = Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("[reference link][ref-links]"),
            ),
        );
        block.set_runtime_context(
            None,
            Arc::default(),
            Arc::new(parse_link_reference_definitions(
                "[ref-links]: https://example.com",
            )),
            Arc::default(),
        );
        block
    });

    assert_eq!(
        block.read_with(cx, |block, _cx| block.inline_link_hit_at(0).cloned()),
        Some(InlineLinkHit {
            prompt_target: "ref-links".to_string(),
            open_target: "https://example.com".to_string(),
        })
    );
}

#[gpui::test]
async fn inline_link_with_title_expands_title_but_opens_destination(cx: &mut TestAppContext) {
    let markdown = "[ABC](https://abc.com \"https://abc.com\")";
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown(markdown),
            ),
        )
    });

    assert_eq!(
        block.read_with(cx, |block, _cx| block.inline_link_hit_at(0).cloned()),
        Some(InlineLinkHit {
            prompt_target: "https://abc.com".to_string(),
            open_target: "https://abc.com".to_string(),
        })
    );

    block.update(cx, |block, _cx| {
        block.selected_range = 0..0;
        block.sync_inline_projection_for_focus(true);
    });

    assert_eq!(
        block.read_with(cx, |block, _cx| block.display_text().to_string()),
        markdown
    );
    assert_eq!(
        block.read_with(cx, |block, _cx| block.record.title.serialize_markdown()),
        markdown
    );
}

#[gpui::test]
async fn autolink_expands_with_angle_brackets_when_touched(cx: &mut TestAppContext) {
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("<https://example.com>"),
            ),
        )
    });

    assert_eq!(
        block.read_with(cx, |block, _cx| block.display_text().to_string()),
        "https://example.com"
    );

    block.update(cx, |block, _cx| {
        block.selected_range = 0..0;
        block.sync_inline_projection_for_focus(true);
    });

    assert_eq!(
        block.read_with(cx, |block, _cx| block.display_text().to_string()),
        "<https://example.com>"
    );
}

