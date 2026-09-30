
use super::super::projection::{
    expanded_display_cursor_offset_for_clean, expanded_display_offset_for_clean,
};
use crate::components::markdown::inline::{
    InlineFragment, InlineInsertionAttributes, InlineScript, InlineStyle,
    InlineTextTree,
};
use crate::components::{
    Block, BlockKind, BlockRecord, IndentBlock, Newline,
};
use gpui::{
    AppContext,
    TestAppContext,
};



#[gpui::test]
async fn tab_inserts_character_in_paragraph(cx: &mut TestAppContext) {
    let cx = cx.add_empty_window();
    let block = cx.new(|cx| Block::with_record(cx, BlockRecord::paragraph("ab")));

    cx.update(|window, cx| {
        block.update(cx, |block, block_cx| {
            block.move_to(1, block_cx);
            block.on_indent_block(&IndentBlock, window, block_cx);
        });
    });

    block.read_with(cx, |block, _cx| {
        assert_eq!(block.display_text(), "a    b");
        assert_eq!(block.selected_range, 5..5);
    });
}

#[gpui::test]
async fn tab_inserts_character_in_code_block(cx: &mut TestAppContext) {
    let cx = cx.add_empty_window();
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::with_plain_text(BlockKind::CodeBlock { language: None }, "ab"),
        )
    });

    cx.update(|window, cx| {
        block.update(cx, |block, block_cx| {
            block.move_to(1, block_cx);
            block.on_indent_block(&IndentBlock, window, block_cx);
        });
    });

    block.read_with(cx, |block, _cx| {
        assert_eq!(block.display_text(), "a    b");
        assert_eq!(block.selected_range, 5..5);
    });
}

#[test]
fn expanded_code_cursor_offset_stays_before_closing_backtick() {
    let fragments = vec![InlineFragment {
        text: "123".to_string(),
        style: InlineStyle {
            code: true,
            ..InlineStyle::default()
        },
        html_style: None,
        link: None,
        footnote: None,
        math: None,
    }];

    assert_eq!(expanded_display_offset_for_clean(&fragments, 0), 1);
    assert_eq!(expanded_display_offset_for_clean(&fragments, 3), 5);
    assert_eq!(expanded_display_cursor_offset_for_clean(&fragments, 0), 1);
    assert_eq!(expanded_display_cursor_offset_for_clean(&fragments, 3), 4);
}

#[test]
fn expanded_code_cursor_offset_keeps_plain_text_boundaries() {
    let fragments = vec![
        InlineFragment {
            text: "a".to_string(),
            style: InlineStyle::default(),
            html_style: None,
            link: None,
            footnote: None,
            math: None,
        },
        InlineFragment {
            text: "bc".to_string(),
            style: InlineStyle {
                code: true,
                ..InlineStyle::default()
            },
            html_style: None,
            link: None,
            footnote: None,
            math: None,
        },
    ];

    assert_eq!(expanded_display_cursor_offset_for_clean(&fragments, 1), 1);
    assert_eq!(expanded_display_cursor_offset_for_clean(&fragments, 3), 4);
}

#[test]
fn typing_inside_manual_backticks_keeps_cursor_inside_code_span() {
    let tree = InlineTextTree::plain("``");
    let result = tree.replace_visible_range(1..1, "1", InlineInsertionAttributes::default());

    assert_eq!(result.tree.visible_text(), "1");
    assert_eq!(
        result.tree.fragments,
        vec![InlineFragment {
            text: "1".to_string(),
            style: InlineStyle {
                code: true,
                ..InlineStyle::default()
            },
            html_style: None,
            link: None,
            footnote: None,
            math: None,
        }]
    );

    let clean_cursor = result.map_offset(2);
    assert_eq!(clean_cursor, 1);
    assert_eq!(
        expanded_display_cursor_offset_for_clean(&result.tree.fragments, clean_cursor),
        2
    );
}

#[gpui::test]
async fn enter_inside_multiline_inline_code_inserts_hard_line_without_splitting(
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
        assert_eq!(block.display_text(), text);
        assert_eq!(block.selected_range, "line 1\n\n".len().."line 1\n\n".len());
        assert!(
            block
                .inline_spans()
                .iter()
                .any(|span| { span.style.code && span.range == (0..text.len()) })
        );
    });
}

#[gpui::test]
async fn inline_math_focus_stays_rendered_rich_and_keeps_links(cx: &mut TestAppContext) {
    let cx = cx.add_empty_window();
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("**bold** $x^2$ [repo](https://example.com)"),
            ),
        )
    });

    block.update(cx, |block, cx| {
        // The math source is shown inline (`$x^2$`) while bold and the link stay
        // collapsed; the block never falls back to raw Markdown editing.
        assert_eq!(block.display_text(), "bold $x^2$ repo");

        // Focusing with the caret inside the math keeps the rendered-rich
        // projection rather than dumping the whole block to raw source, so the
        // link in the same block keeps its link attribute.
        let caret = "bold $".len();
        block.move_to(caret, cx);
        block.sync_inline_projection_for_focus(true);
        assert!(!block.uses_raw_text_editing());
        assert!(block.record.title.has_mixed_inline_visuals());
        assert!(block.record.title.has_inline_links());
        assert!(
            block.inline_spans().iter().any(|span| span.link.is_some()),
            "link must stay styled while editing the math in the same block"
        );
        assert_eq!(
            block.record.title.serialize_markdown(),
            "**bold** $x^2$ [repo](https://example.com)"
        );
    });
}

#[gpui::test]
async fn script_spans_focus_stay_rendered_rich(cx: &mut TestAppContext) {
    let cx = cx.add_empty_window();
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
        assert_eq!(block.display_text(), "x2 and H2O");
        assert_eq!(block.inline_spans()[0].style.script, InlineScript::Normal);
        assert_eq!(
            block.inline_spans()[1].style.script,
            InlineScript::Superscript
        );
        assert!(!block.uses_raw_text_editing());
        assert_eq!(block.display_text(), "x2 and H2O");
        assert_eq!(block.record.title.serialize_markdown(), "x^2^ and H~2~O");
    });
}

#[gpui::test]
async fn link_anchor_emphasis_delimiters_are_revealed_when_caret_inside(cx: &mut TestAppContext) {
    let cx = cx.add_empty_window();
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("[**bold**](https://example.com)"),
            ),
        )
    });

    block.update(cx, |block, cx| {
        // Collapsed, only the styled anchor text is shown.
        assert_eq!(block.display_text(), "bold");

        // With the caret inside the bold anchor text, the projection reveals both
        // the link syntax and the anchor's own `**` emphasis markers, so they can
        // be edited instead of staying invisible.
        block.move_to(2, cx);
        block.sync_inline_projection_for_focus(true);
        assert_eq!(block.display_text(), "[**bold**](https://example.com)");
    });
}

#[gpui::test]
async fn mermaid_block_uses_raw_text_editing(cx: &mut TestAppContext) {
    let cx = cx.add_empty_window();
    let markdown = "```mermaid\nflowchart LR\nA --> B\n```";
    let block = cx.new(|cx| Block::with_record(cx, BlockRecord::mermaid(markdown)));

    block.update(cx, |block, _cx| {
        assert_eq!(block.kind(), BlockKind::MermaidBlock);
        assert!(block.uses_raw_text_editing());
        assert_eq!(block.display_text(), markdown);
        assert_eq!(block.record.markdown_line(0, None), markdown);
    });
}

