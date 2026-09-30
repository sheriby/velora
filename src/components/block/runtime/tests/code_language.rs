
use crate::components::markdown::code_highlight::CodeLanguageKey;
use crate::components::markdown::inline::InlineTextTree;
use crate::components::{
    Block, BlockKind, BlockRecord,
};
use gpui::{
    AppContext, EntityInputHandler,
    TestAppContext,
};



#[gpui::test]
async fn code_block_cache_builds_rust_highlight_spans(cx: &mut TestAppContext) {
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::CodeBlock {
                    language: Some("rust".into()),
                },
                InlineTextTree::plain("fn main() {\n    let value: i32 = 42;\n}\n"),
            ),
        )
    });

    let highlight = block
        .read_with(cx, |block, _cx| block.code_highlight_result().cloned())
        .expect("code block should cache a highlight result");
    assert_eq!(highlight.language, CodeLanguageKey::Rust);
    assert!(!highlight.spans.is_empty());
}

#[gpui::test]
async fn code_block_cache_updates_when_language_changes(cx: &mut TestAppContext) {
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::CodeBlock {
                    language: Some("rust".into()),
                },
                InlineTextTree::plain("fn main() {\n    let value = 42;\n}\n"),
            ),
        )
    });

    block.update(cx, |block, _cx| {
        block.record.kind = BlockKind::CodeBlock {
            language: Some("text".into()),
        };
        block.sync_render_cache();
    });

    let highlight = block
        .read_with(cx, |block, _cx| block.code_highlight_result().cloned())
        .expect("known plain fallback should still cache a result");
    assert_eq!(highlight.language, CodeLanguageKey::PlainText);
    assert!(highlight.spans.is_empty());
}

#[gpui::test]
async fn code_block_language_setter_updates_highlight_without_changing_content(
    cx: &mut TestAppContext,
) {
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::CodeBlock {
                    language: Some("rust".into()),
                },
                InlineTextTree::plain("print('hello')"),
            ),
        )
    });

    block.update(cx, |block, cx| {
        let range = 0..block.code_language_text().len();
        block.replace_code_language_text_in_range(range, "python", None, false, cx);
    });

    block.read_with(cx, |block, _cx| {
        assert_eq!(block.code_language_text(), "python");
        assert_eq!(block.display_text(), "print('hello')");
        assert_eq!(
            block
                .code_highlight_result()
                .expect("python should highlight")
                .language,
            CodeLanguageKey::Python
        );
    });
}

#[gpui::test]
async fn code_block_language_accepts_unknown_language_as_plain_rendering(cx: &mut TestAppContext) {
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::CodeBlock {
                    language: Some("rust".into()),
                },
                InlineTextTree::plain("fn main() {}"),
            ),
        )
    });

    block.update(cx, |block, cx| {
        let range = 0..block.code_language_text().len();
        block.replace_code_language_text_in_range(range, "unknown-lang", None, false, cx);
    });

    block.read_with(cx, |block, _cx| {
        assert_eq!(block.code_language_text(), "unknown-lang");
        assert!(block.code_highlight_result().is_none());
    });
}

#[gpui::test]
async fn code_language_input_uses_ime_path_without_touching_code_content(cx: &mut TestAppContext) {
    let cx = cx.add_empty_window();
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::CodeBlock {
                    language: Some("rust".into()),
                },
                InlineTextTree::plain("fn main() {}"),
            ),
        )
    });

    cx.update(|window, cx| {
        block.update(cx, |block, block_cx| {
            block.code_language_focus_handle.focus(window);
            block.code_language_selected_range = 0..block.code_language_text().len();
            block.selected_range = 3..3;
            <Block as EntityInputHandler>::replace_text_in_range(
                block, None, "python", window, block_cx,
            );
        });
    });

    block.read_with(cx, |block, _cx| {
        assert_eq!(block.code_language_text(), "python");
        assert_eq!(block.display_text(), "fn main() {}");
        assert_eq!(block.selected_range, 3..3);
        assert_eq!(block.code_language_selected_range, 6..6);
    });
}

#[gpui::test]
async fn code_language_input_handles_utf16_ranges(cx: &mut TestAppContext) {
    let cx = cx.add_empty_window();
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::CodeBlock {
                    language: Some("zh😀kana".into()),
                },
                InlineTextTree::plain("body"),
            ),
        )
    });

    cx.update(|window, cx| {
        block.update(cx, |block, block_cx| {
            block.code_language_focus_handle.focus(window);
            <Block as EntityInputHandler>::replace_text_in_range(
                block,
                Some(2..4),
                "py",
                window,
                block_cx,
            );
        });
    });

    block.read_with(cx, |block, _cx| {
        assert_eq!(block.code_language_text(), "zhpykana");
        assert_eq!(block.display_text(), "body");
    });
}

#[gpui::test]
async fn code_language_input_clears_language_when_empty(cx: &mut TestAppContext) {
    let cx = cx.add_empty_window();
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::CodeBlock {
                    language: Some("rust".into()),
                },
                InlineTextTree::plain("body"),
            ),
        )
    });

    cx.update(|window, cx| {
        block.update(cx, |block, block_cx| {
            block.code_language_focus_handle.focus(window);
            block.code_language_selected_range = 0..block.code_language_text().len();
            <Block as EntityInputHandler>::replace_text_in_range(block, None, "", window, block_cx);
        });
    });

    block.read_with(cx, |block, _cx| {
        assert_eq!(block.code_language_text(), "");
        assert!(matches!(
            block.kind(),
            BlockKind::CodeBlock { language: None }
        ));
        assert!(block.code_highlight_result().is_none());
    });
}

