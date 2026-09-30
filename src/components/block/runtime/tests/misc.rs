
use crate::components::markdown::inline::InlineTextTree;
use crate::components::{
    Block, BlockKind, BlockRecord, DeleteBack, TableCellPosition,
};
use gpui::{
    AppContext, EntityInputHandler,
    TestAppContext,
};



#[gpui::test]
async fn editing_link_anchor_in_math_block_matches_plain_paragraph(cx: &mut TestAppContext) {
    // A block mixing inline math with a link is "source preserving", which used
    // to route its link edits through the markdown-space path. That path assumed
    // the anchor label began right after `[`, so the anchor's own emphasis
    // markers shifted the mapping and edits landed on the wrong character. Inline
    // links now edit through the link projection in every block, so deleting a
    // revealed anchor delimiter touches the delimiter, not a label character.
    let cx = cx.add_empty_window();
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("$x^2$ [**bold**](https://e.com)"),
            ),
        )
    });

    cx.update(|window, cx| {
        block.update(cx, |block, cx| {
            block.move_to("$x^2$ bo".len(), cx);
            block.sync_inline_projection_for_focus(true);

            // Caret just past the revealed opening `**` of the bold anchor.
            let projected = block.display_text().to_string();
            assert_eq!(projected, "$x^2$ [**bold**](https://e.com)");
            let after_open = projected.find("[**").unwrap() + "[**".len();
            block.selected_range = after_open..after_open;

            block.on_delete_back(&DeleteBack, window, cx);

            let markdown = block.record.title.serialize_markdown();
            assert!(
                markdown.starts_with("$x^2$ "),
                "math source preserved: {markdown:?}"
            );
            assert!(
                markdown.contains("bold"),
                "anchor label must stay intact, only the delimiter is edited: {markdown:?}"
            );
        });
    });
}

#[gpui::test]
async fn completing_link_in_math_block_places_caret_after_closing_paren(cx: &mut TestAppContext) {
    // A block mixing math with a link edits in markdown space. Typing the closing
    // `)` completes the link, and the caret must land just past it (like a plain
    // paragraph) rather than inside the anchor before `]`.
    let cx = cx.add_empty_window();
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("$x$ [link](google.com"),
            ),
        )
    });

    cx.update(|window, cx| {
        block.update(cx, |block, cx| {
            block.move_to(block.visible_len(), cx);
            block.sync_inline_projection_for_focus(true);
            block.replace_text_in_range(None, ")", window, cx);
            block.sync_inline_projection_for_focus(true);

            assert_eq!(
                block.record.title.serialize_markdown(),
                "$x$ [link](google.com)"
            );
            assert_eq!(block.display_text(), "$x$ [link](google.com)");
            let end = block.visible_len();
            assert_eq!(block.selected_range, end..end);
        });
    });
}

#[gpui::test]
async fn rtl_selection_across_trailing_link_keeps_block_end_anchor(cx: &mut TestAppContext) {
    // A link sitting at the very end of a block that also contains inline math
    // stays expanded while the projection is rebuilt on every render. Dragging a
    // selection right-to-left from the block end across the link used to collapse
    // the anchor onto the closing `]` of the anchor text, because the trailing
    // `](url)` delimiters all share one clean offset and the remap snapped back
    // to the inner cursor position. The anchor must stay at the block end.
    let cx = cx.add_empty_window();
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("$x$ [link](google.com)"),
            ),
        )
    });

    block.update(cx, |block, cx| {
        block.move_to(block.visible_len(), cx);
        block.sync_inline_projection_for_focus(true);
        let end = block.visible_len();
        assert_eq!(block.display_text(), "$x$ [link](google.com)");

        // Start an RTL selection at the block end and drag the head left,
        // re-syncing the projection after each move like the render loop does.
        block.move_to(end, cx);
        block.sync_inline_projection_for_focus(true);
        for target in (0..end).rev() {
            block.select_to(target, cx);
            block.sync_inline_projection_for_focus(true);
            assert_eq!(
                block.selected_range,
                target..end,
                "RTL selection anchor must stay at the block end (head {target})"
            );
            assert!(block.selection_reversed);
        }
    });
}

#[gpui::test]
async fn typing_destination_into_empty_link_parens_keeps_caret_inside(cx: &mut TestAppContext) {
    // Fixes an edge case where batched auto-pair macro `()+Left` caused first character typed
    // into `()` of link to snap the caret past `)` with rest of URL landing outside the link.
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("[GitHub]"),
            ),
        )
    });

    block.update(cx, |block, cx| {
        // Auto-pair the `()` after the label, then drop the caret between them.
        block.selected_range = 8..8;
        block.sync_inline_projection_for_focus(true);
        block.replace_text_in_visible_range(8..8, "()", None, false, cx);
        block.sync_inline_projection_for_focus(true);
        let between = block.display_text().find(')').expect("closing paren");
        block.selected_range = between..between;
        block.sync_inline_projection_for_focus(true);
        for ch in "https://github.com".chars() {
            let at = block.selected_range.clone();
            block.replace_text_in_visible_range(at, &ch.to_string(), None, false, cx);
            block.sync_inline_projection_for_focus(true);
        }
    });

    block.read_with(cx, |block, _cx| {
        assert_eq!(
            block.record.title.serialize_markdown(),
            "[GitHub](https://github.com)"
        );
        assert_eq!(
            block.inline_link_at(1).map(str::to_string),
            Some("https://github.com".to_string())
        );
        // Caret stays inside `()`, just before the closing `)`.
        let close = block.display_text().find(')').expect("closing paren");
        assert_eq!(block.selected_range, close..close);
    });
}

#[gpui::test]
async fn table_header_cells_center_by_default_and_respect_explicit_alignment(
    cx: &mut TestAppContext,
) {
    // 表头默认居中；显式对齐语法（:--- / :---: / ---:）优先。
    use crate::components::TableColumnAlignment as Align;

    let cx = cx.add_empty_window();
    let make_cell = |cx: &mut gpui::VisualTestContext, row: usize, alignment| {
        cx.new(|cx| {
            let mut block = Block::with_record(
                cx,
                BlockRecord::new(BlockKind::Paragraph, InlineTextTree::plain("单元格")),
            );
            block.set_table_cell_mode(
                TableCellPosition { row, column: 0 },
                alignment,
            );
            block
        })
    };
    let header_default = make_cell(cx, 0, Align::Default);
    let body_default = make_cell(cx, 1, Align::Default);
    let header_explicit_left = make_cell(cx, 0, Align::Left);
    let header_explicit_center = make_cell(cx, 0, Align::Center);
    let header_explicit_right = make_cell(cx, 0, Align::Right);

    header_default.read_with(cx, |block, _| {
        assert_eq!(
            block.text_align(),
            gpui::TextAlign::Center,
            "无对齐语法的表头应默认居中"
        );
    });
    body_default.read_with(cx, |block, _| {
        assert_eq!(block.text_align(), gpui::TextAlign::Left, "数据行默认左对齐");
    });
    header_explicit_left.read_with(cx, |block, _| {
        assert_eq!(
            block.text_align(),
            gpui::TextAlign::Left,
            "显式 :--- 的表头应保持左对齐"
        );
    });
    header_explicit_center.read_with(cx, |block, _| {
        assert_eq!(block.text_align(), gpui::TextAlign::Center);
    });
    header_explicit_right.read_with(cx, |block, _| {
        assert_eq!(block.text_align(), gpui::TextAlign::Right);
    });
}

#[gpui::test]
async fn double_click_selects_word_at_offset(cx: &mut TestAppContext) {
    // 双击选词：拉丁词、CJK 分组、标点分段、行尾兜底。
    let cx = cx.add_empty_window();
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::plain("hello 世界, world"),
            ),
        )
    });

    // "hello 世界, world" 字节布局（UAX#29：CJK 单字成段）：
    // hello=0..5, 世=6..9, 界=9..12, 逗号=12..13, world=14..19。
    let mut select_at = |offset: usize| {
        block.update(cx, |block, block_cx| {
            block.select_word_at(offset, block_cx);
            block.selected_range.clone()
        })
    };

    assert_eq!(select_at(1), 0..5, "点 hello 中间应选中整个 hello");
    assert_eq!(select_at(6), 6..9, "点 CJK 字符应选中该字");
    assert_eq!(select_at(10), 9..12, "第二个 CJK 字同理");
    assert_eq!(select_at(12), 12..13, "点标点应选中标点段");
    assert_eq!(select_at(17), 14..19, "点 world 中间应选中 world");
    assert_eq!(select_at(19), 14..19, "点文本末尾应选中最后一个词");
    assert_eq!(select_at(1_000), 14..19, "超界偏移应收敛到最后一个词");
}
