use std::sync::Arc;

use crate::components::markdown::inline::{
    InlineLinkHit,
    InlineTextTree,
};
use crate::components::markdown::link::parse_link_reference_definitions;
use crate::components::{
    Block, BlockKind, BlockRecord,
};
use gpui::{
    AppContext, EntityInputHandler,
    TestAppContext,
};



#[gpui::test]
async fn projected_reference_target_stays_link_hit_testable(cx: &mut TestAppContext) {
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

    let target_offset = block.update(cx, |block, _cx| {
        block.selected_range = 0..0;
        block.sync_inline_projection_for_focus(true);
        block
            .display_text()
            .find("ref-link")
            .expect("projection should expose reference target")
    });

    assert_eq!(
        block.read_with(cx, |block, _cx| block
            .inline_link_hit_at(target_offset)
            .cloned()),
        Some(InlineLinkHit {
            prompt_target: "ref-link".to_string(),
            open_target: "https://example.com".to_string(),
        })
    );
}

#[gpui::test]
async fn projected_reference_syntax_maps_full_delimiter_range_back_to_markdown(
    cx: &mut TestAppContext,
) {
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

    let display_len = block.update(cx, |block, _cx| {
        block.selected_range = 0..0;
        block.sync_inline_projection_for_focus(true);
        block.display_text().len()
    });

    assert_eq!(
        block.read_with(cx, |block, _cx| {
            block.current_range_to_markdown_range(0..display_len)
        }),
        0.."[reference link][ref-link]".len()
    );
}

#[gpui::test]
async fn editing_link_destination_inside_projection_preserves_link(cx: &mut TestAppContext) {
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("a [link](https://example.com) b"),
            ),
        )
    });

    block.update(cx, |block, cx| {
        block.selected_range = 2..2;
        block.sync_inline_projection_for_focus(true);
        let expanded = block.display_text().to_string();
        let insert_at = expanded
            .find("example.com")
            .expect("expanded link should expose its destination");
        block.replace_text_in_visible_range(insert_at..insert_at, "docs.", None, false, cx);
    });

    assert_eq!(
        block.read_with(cx, |block, _cx| block.record.title.serialize_markdown()),
        "a [link](https://docs.example.com) b"
    );
    assert_eq!(
        block.read_with(cx, |block, _cx| block.display_text().to_string()),
        "a [link](https://docs.example.com) b"
    );
    assert_eq!(
        block.read_with(cx, |block, _cx| block.inline_link_at(3).map(str::to_string)),
        Some("https://docs.example.com".to_string())
    );
}

#[gpui::test]
async fn typing_after_inline_link_preserves_link(cx: &mut TestAppContext) {
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("[Link](https://example.com/) x"),
            ),
        )
    });

    block.update(cx, |block, cx| {
        // Place the caret past the link (in the trailing text) so the edit does
        // not touch the link's projected run, then type. This is the case that
        // previously re-parsed from collapsed text and dropped the link.
        block.selected_range = 0..0;
        block.sync_inline_projection_for_focus(true);
        let end = block.display_text().len();
        block.selected_range = end..end;
        block.sync_inline_projection_for_focus(true);
        block.replace_text_in_visible_range(end..end, "y", None, false, cx);
    });

    assert_eq!(
        block.read_with(cx, |block, _cx| block.record.title.serialize_markdown()),
        "[Link](https://example.com/) xy"
    );
    assert_eq!(
        block.read_with(cx, |block, _cx| block.inline_link_at(0).map(str::to_string)),
        Some("https://example.com/".to_string())
    );
}

#[gpui::test]
async fn deleting_adjacent_text_preserves_reference_style_link_syntax(cx: &mut TestAppContext) {
    let block = cx.new(|cx| {
        let mut block = Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("[ref][ref-link]a"),
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

    block.update(cx, |block, cx| {
        block.replace_text_in_visible_range(3..4, "", None, false, cx);
    });

    assert_eq!(
        block.read_with(cx, |block, _cx| block.record.title.serialize_markdown()),
        "[ref][ref-link]"
    );
    assert_eq!(
        block.read_with(cx, |block, _cx| block.display_text().to_string()),
        "ref"
    );
}

#[gpui::test]
async fn deleting_adjacent_text_preserves_autolink_syntax(cx: &mut TestAppContext) {
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("<ref2>a"),
            ),
        )
    });

    block.update(cx, |block, cx| {
        block.replace_text_in_visible_range(4..5, "", None, false, cx);
    });

    assert_eq!(
        block.read_with(cx, |block, _cx| block.record.title.serialize_markdown()),
        "<ref2>"
    );
    assert_eq!(
        block.read_with(cx, |block, _cx| block.display_text().to_string()),
        "ref2"
    );
}

#[gpui::test]
async fn link_projection_preserves_cursor_inside_destination_after_rebuild(
    cx: &mut TestAppContext,
) {
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("a [link](https://example.com) b"),
            ),
        )
    });

    block.update(cx, |block, cx| {
        block.selected_range = 2..2;
        block.sync_inline_projection_for_focus(true);
        let expanded = block.display_text().to_string();
        let destination_offset = expanded
            .find("example.com")
            .expect("expanded link should expose destination text");
        block.move_to_with_preferred_x(destination_offset, None, cx);
        block.sync_inline_projection_for_focus(true);
    });

    let destination_offset = block.read_with(cx, |block, _cx| {
        block
            .display_text()
            .find("example.com")
            .expect("expanded link should expose destination text")
    });
    assert_eq!(
        block.read_with(cx, |block, _cx| block.cursor_offset()),
        destination_offset
    );
}

#[gpui::test]
async fn link_projection_preserves_selection_inside_destination_after_rebuild(
    cx: &mut TestAppContext,
) {
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("a [link](https://example.com) b"),
            ),
        )
    });

    let selected_range = block.update(cx, |block, _cx| {
        block.selected_range = 2..2;
        block.sync_inline_projection_for_focus(true);
        let expanded = block.display_text().to_string();
        let destination_offset = expanded
            .find("example.com")
            .expect("expanded link should expose destination text");
        let selected_range = destination_offset..destination_offset + "example".len();
        block.selected_range = selected_range.clone();
        block.selection_reversed = false;
        block.sync_inline_projection_for_focus(true);
        selected_range
    });

    assert_eq!(
        block.read_with(cx, |block, _cx| block.selected_range.clone()),
        selected_range
    );
}

#[gpui::test]
async fn link_middle_delimiter_click_snaps_to_destination_start(cx: &mut TestAppContext) {
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("a [link](https://example.com) b"),
            ),
        )
    });

    let destination_offset = block.update(cx, |block, cx| {
        block.selected_range = 2..2;
        block.sync_inline_projection_for_focus(true);
        let expanded = block.display_text().to_string();
        let middle = expanded
            .find("](")
            .expect("expanded link should expose middle delimiter");
        let destination_offset = expanded
            .find("https://")
            .expect("expanded link should expose destination start");
        let click_target = block.pointer_target_offset(middle + 1);
        block.move_to_with_preferred_x(click_target, None, cx);
        block.sync_inline_projection_for_focus(true);
        destination_offset
    });

    assert_eq!(
        block.read_with(cx, |block, _cx| block.cursor_offset()),
        destination_offset
    );
}

#[gpui::test]
async fn reversed_selection_survives_projection_focus_refresh(cx: &mut TestAppContext) {
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("alpha beta"),
            ),
        )
    });

    block.update(cx, |block, _cx| {
        block.selected_range = 1..7;
        block.selection_reversed = true;
        block.sync_inline_projection_for_focus(true);
    });

    assert_eq!(
        block.read_with(cx, |block, _cx| block.selected_range.clone()),
        1..7
    );
    assert!(block.read_with(cx, |block, _cx| block.selection_reversed));
}

#[gpui::test]
async fn reversed_selection_survives_render_cache_refresh(cx: &mut TestAppContext) {
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("alpha beta"),
            ),
        )
    });

    block.update(cx, |block, _cx| {
        block.selected_range = 1..7;
        block.selection_reversed = true;
        block.sync_render_cache();
    });

    assert_eq!(
        block.read_with(cx, |block, _cx| block.selected_range.clone()),
        1..7
    );
    assert!(block.read_with(cx, |block, _cx| block.selection_reversed));
}

#[gpui::test]
async fn reversed_selection_survives_clear_inline_projection(cx: &mut TestAppContext) {
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("`code`"),
            ),
        )
    });

    block.update(cx, |block, _cx| {
        block.selected_range = 2..2;
        block.sync_inline_projection_for_focus(true);
        block.selected_range = 1..5;
        block.selection_reversed = true;
        block.clear_inline_projection();
    });

    assert_eq!(
        block.read_with(cx, |block, _cx| block.display_text().to_string()),
        "code"
    );
    assert_eq!(
        block.read_with(cx, |block, _cx| block.selected_range.clone()),
        0..4
    );
    assert!(block.read_with(cx, |block, _cx| block.selection_reversed));
}

#[gpui::test]
async fn reversed_selection_inside_link_destination_survives_focus_refresh(
    cx: &mut TestAppContext,
) {
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("a [link](https://example.com) b"),
            ),
        )
    });

    let expected = block.update(cx, |block, _cx| {
        block.selected_range = 2..2;
        block.sync_inline_projection_for_focus(true);
        let expanded = block.display_text().to_string();
        let destination_offset = expanded
            .find("example.com")
            .expect("expanded link should expose destination text");
        let expected = destination_offset..destination_offset + "example".len();
        block.selected_range = expected.clone();
        block.selection_reversed = true;
        block.sync_inline_projection_for_focus(true);
        expected
    });

    assert_eq!(
        block.read_with(cx, |block, _cx| block.selected_range.clone()),
        expected
    );
    assert!(block.read_with(cx, |block, _cx| block.selection_reversed));
}

#[gpui::test]
async fn ime_selected_text_range_reports_reversed_for_right_to_left_selection(
    cx: &mut TestAppContext,
) {
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

    block.update(cx, |block, _cx| {
        block.selected_range = 1..7;
        block.selection_reversed = true;
    });

    let selection = cx.update(|window, cx| {
        block.update(cx, |block, block_cx| {
            <Block as EntityInputHandler>::selected_text_range(block, false, window, block_cx)
                .expect("selection")
        })
    });

    assert_eq!(selection.range, 1..7);
    assert!(selection.reversed);
}

#[gpui::test]
async fn ime_replace_text_replaces_right_to_left_selection_in_source_raw_mode(
    cx: &mut TestAppContext,
) {
    let cx = cx.add_empty_window();
    let block = cx.new(|cx| {
        let mut block = Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("alpha beta"),
            ),
        );
        block.set_source_raw_mode();
        block
    });

    block.update(cx, |block, _cx| {
        block.selected_range = 1..7;
        block.selection_reversed = true;
    });

    cx.update(|window, cx| {
        block.update(cx, |block, block_cx| {
            <Block as EntityInputHandler>::replace_text_in_range(
                block, None, "Z", window, block_cx,
            );
        });
    });

    assert_eq!(
        block.read_with(cx, |block, _cx| block.display_text().to_string()),
        "aZeta"
    );
    assert_eq!(
        block.read_with(cx, |block, _cx| block.selected_range.clone()),
        2..2
    );
    assert!(!block.read_with(cx, |block, _cx| block.selection_reversed));
}

#[gpui::test]
async fn pasting_url_over_selection_makes_link(cx: &mut TestAppContext) {
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

    block.update(cx, |block, _cx| {
        block.selected_range = 0..5; // "alpha"
    });

    cx.update(|window, cx| {
        block.update(cx, |block, block_cx| {
            block.paste_url_as_link("https://example.test", window, block_cx);
        });
    });

    block.read_with(cx, |block, _cx| {
        assert_eq!(
            block.display_text().to_string(),
            "[alpha](https://example.test) beta"
        );
    });
}

#[gpui::test]
async fn typed_wrap_symbol_wraps_selection(cx: &mut TestAppContext) {
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

    block.update(cx, |block, _cx| {
        block.selected_range = 0..5; // "alpha"
    });

    cx.update(|window, cx| {
        block.update(cx, |block, block_cx| {
            <Block as EntityInputHandler>::replace_text_in_range(
                block, None, "*", window, block_cx,
            );
        });
    });

    block.read_with(cx, |block, _cx| {
        assert_eq!(block.display_text().to_string(), "*alpha* beta");
        // The selection stays on the wrapped text so continued typing replaces
        // the inner content, not the markers.
        assert_eq!(block.selected_range.clone(), 1..6);
    });
}

#[gpui::test]
async fn typed_paren_wraps_selection_with_matched_close(cx: &mut TestAppContext) {
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

    block.update(cx, |block, _cx| {
        block.selected_range = 6..10; // "beta"
    });

    cx.update(|window, cx| {
        block.update(cx, |block, block_cx| {
            <Block as EntityInputHandler>::replace_text_in_range(
                block, None, "(", window, block_cx,
            );
        });
    });

    block.read_with(cx, |block, _cx| {
        assert_eq!(block.display_text().to_string(), "alpha (beta)");
        assert_eq!(block.selected_range.clone(), 7..11);
    });
}

#[gpui::test]
async fn wrap_symbol_with_empty_selection_still_replaces(cx: &mut TestAppContext) {
    let cx = cx.add_empty_window();
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("alpha"),
            ),
        )
    });

    block.update(cx, |block, _cx| {
        block.selected_range = 2..2;
    });

    cx.update(|window, cx| {
        block.update(cx, |block, block_cx| {
            <Block as EntityInputHandler>::replace_text_in_range(
                block, None, "*", window, block_cx,
            );
        });
    });

    assert_eq!(
        block.read_with(cx, |block, _cx| block.display_text().to_string()),
        "al*pha"
    );
}

