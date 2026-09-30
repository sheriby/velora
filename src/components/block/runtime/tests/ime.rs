use std::ops::Range;

use crate::components::markdown::inline::InlineTextTree;
use crate::components::{
    Block, BlockKind, BlockRecord, TableCellPosition,
};
use gpui::{
    AppContext, EntityInputHandler,
    TestAppContext,
};

fn assert_only_code_range(block: &Block, expected: Range<usize>) {
    let code_ranges = block
        .inline_spans()
        .iter()
        .filter(|span| span.style.code)
        .map(|span| span.range.clone())
        .collect::<Vec<_>>();
    assert_eq!(code_ranges, vec![expected]);
}


#[gpui::test]
async fn source_document_mode_enables_line_numbers(cx: &mut TestAppContext) {
    let block = cx.new(|cx| {
        let mut block = Block::with_record(
            cx,
            BlockRecord::new(BlockKind::Paragraph, InlineTextTree::plain("a\nb")),
        );
        block.set_source_document_mode();
        block
    });

    block.read_with(cx, |block, _cx| {
        assert!(block.is_source_raw_mode());
        assert!(block.show_source_line_numbers());
    });
}

#[gpui::test]
async fn source_raw_mode_does_not_enable_line_numbers(cx: &mut TestAppContext) {
    let block = cx.new(|cx| {
        let mut block = Block::with_record(
            cx,
            BlockRecord::new(BlockKind::Paragraph, InlineTextTree::plain("raw")),
        );
        block.set_source_raw_mode();
        block
    });

    block.read_with(cx, |block, _cx| {
        assert!(block.is_source_raw_mode());
        assert!(!block.show_source_line_numbers());
    });
}

#[gpui::test]
async fn ime_replace_and_mark_text_replaces_right_to_left_selection_in_table_cell(
    cx: &mut TestAppContext,
) {
    let cx = cx.add_empty_window();
    let block = cx.new(|cx| {
        let mut block = Block::with_record(
            cx,
            BlockRecord::new(BlockKind::Paragraph, InlineTextTree::from_markdown("alpha")),
        );
        block.set_table_cell_mode(
            TableCellPosition { row: 0, column: 0 },
            crate::components::TableColumnAlignment::Left,
        );
        block
    });

    block.update(cx, |block, _cx| {
        block.selected_range = 1..4;
        block.selection_reversed = true;
    });

    cx.update(|window, cx| {
        block.update(cx, |block, block_cx| {
            <Block as EntityInputHandler>::replace_and_mark_text_in_range(
                block,
                None,
                "XY",
                Some(0..1),
                window,
                block_cx,
            );
        });
    });

    assert_eq!(
        block.read_with(cx, |block, _cx| block.display_text().to_string()),
        "aXYa"
    );
    assert_eq!(
        block.read_with(cx, |block, _cx| block.selected_range.clone()),
        1..2
    );
    assert_eq!(
        block.read_with(cx, |block, _cx| block.marked_range.clone()),
        Some(1..3)
    );
    assert!(!block.read_with(cx, |block, _cx| block.selection_reversed));
}

#[gpui::test]
async fn ime_commit_inside_inline_code_preserves_code_style(cx: &mut TestAppContext) {
    let cx = cx.add_empty_window();
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("aaa`hello world`aaa"),
            ),
        )
    });

    block.update(cx, |block, _cx| {
        let cursor = "aaahello".len();
        block.selected_range = cursor..cursor;
    });

    cx.update(|window, cx| {
        block.update(cx, |block, block_cx| {
            <Block as EntityInputHandler>::replace_and_mark_text_in_range(
                block,
                None,
                "ni",
                Some(2..2),
                window,
                block_cx,
            );
            <Block as EntityInputHandler>::replace_text_in_range(
                block, None, "你", window, block_cx,
            );
        });
    });

    block.read_with(cx, |block, _cx| {
        assert_eq!(block.display_text(), "aaahello你 worldaaa");
        assert_eq!(
            block.record.title.serialize_markdown(),
            "aaa`hello你 world`aaa"
        );
        assert_only_code_range(block, "aaa".len().."aaahello你 world".len());
    });
}

#[gpui::test]
async fn ime_commit_inside_projected_inline_code_preserves_code_style(cx: &mut TestAppContext) {
    let cx = cx.add_empty_window();
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("aaa`hello world`aaa"),
            ),
        )
    });

    block.update(cx, |block, _cx| {
        let cursor = "aaahello".len();
        block.selected_range = cursor..cursor;
        block.sync_inline_projection_for_focus(true);
        assert_eq!(block.display_text(), "aaa`hello world`aaa");
    });

    cx.update(|window, cx| {
        block.update(cx, |block, block_cx| {
            <Block as EntityInputHandler>::replace_and_mark_text_in_range(
                block,
                None,
                "ni",
                Some(2..2),
                window,
                block_cx,
            );
            <Block as EntityInputHandler>::replace_text_in_range(
                block, None, "你", window, block_cx,
            );
        });
    });

    block.update(cx, |block, _cx| {
        assert_eq!(
            block.record.title.serialize_markdown(),
            "aaa`hello你 world`aaa"
        );
        block.clear_inline_projection();
        assert_eq!(block.display_text(), "aaahello你 worldaaa");
        assert_only_code_range(block, "aaa".len().."aaahello你 world".len());
    });
}

#[gpui::test]
async fn replacing_selection_inside_inline_code_preserves_code_style(cx: &mut TestAppContext) {
    let cx = cx.add_empty_window();
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("aaa`hello world`aaa"),
            ),
        )
    });

    block.update(cx, |block, _cx| {
        let start = "aaahello ".len();
        let end = "aaahello world".len();
        block.selected_range = start..end;
    });

    cx.update(|window, cx| {
        block.update(cx, |block, block_cx| {
            <Block as EntityInputHandler>::replace_text_in_range(
                block, None, "你", window, block_cx,
            );
        });
    });

    block.read_with(cx, |block, _cx| {
        assert_eq!(block.display_text(), "aaahello 你aaa");
        assert_eq!(block.record.title.serialize_markdown(), "aaa`hello 你`aaa");
        assert_only_code_range(block, "aaa".len().."aaahello 你".len());
    });
}

#[gpui::test]
async fn replacing_selection_across_inline_code_boundary_stays_plain(cx: &mut TestAppContext) {
    let cx = cx.add_empty_window();
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("aaa`hello`bbb"),
            ),
        )
    });

    block.update(cx, |block, _cx| {
        block.selected_range = "aaahel".len().."aaahellobb".len();
    });

    cx.update(|window, cx| {
        block.update(cx, |block, block_cx| {
            <Block as EntityInputHandler>::replace_text_in_range(
                block, None, "你", window, block_cx,
            );
        });
    });

    block.read_with(cx, |block, _cx| {
        assert_eq!(block.display_text(), "aaahel你b");
        assert_eq!(block.record.title.serialize_markdown(), "aaa`hel`你b");
        assert_only_code_range(block, "aaa".len().."aaahel".len());
    });
}

#[test]
fn ime_utf16_ranges_keep_multilingual_boundaries() {
    let text = "中文😀かな";
    let emoji_utf8 = "中文".len().."中文😀".len();
    assert_eq!(Block::utf16_range_to_utf8_in(text, &(2..4)), emoji_utf8);
    assert_eq!(Block::utf8_range_to_utf16_in(text, &emoji_utf8), 2..4);
}

#[gpui::test]
async fn ime_replace_text_handles_cjk_and_emoji_utf16_ranges(cx: &mut TestAppContext) {
    let cx = cx.add_empty_window();
    let block = cx.new(|cx| {
        let mut block = Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::plain("中文😀かな".to_string()),
            ),
        );
        block.set_source_raw_mode();
        block
    });

    cx.update(|window, cx| {
        block.update(cx, |block, block_cx| {
            <Block as EntityInputHandler>::replace_text_in_range(
                block,
                Some(2..4),
                "語",
                window,
                block_cx,
            );
        });
    });

    assert_eq!(
        block.read_with(cx, |block, _cx| block.display_text().to_string()),
        "中文語かな"
    );
    assert_eq!(
        block.read_with(cx, |block, _cx| block.selected_range.clone()),
        "中文語".len().."中文語".len()
    );
}

#[gpui::test]
async fn ime_selection_ignores_editor_external_selection(cx: &mut TestAppContext) {
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
        block.selected_range = 1..1;
        block.editor_selection_range = Some(0..block.visible_len());
    });

    let selection = cx.update(|window, cx| {
        block.update(cx, |block, block_cx| {
            <Block as EntityInputHandler>::selected_text_range(block, false, window, block_cx)
                .expect("selection")
        })
    });

    assert_eq!(selection.range, 1..1);
    assert!(!selection.reversed);
}

