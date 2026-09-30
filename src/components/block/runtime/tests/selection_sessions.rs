
use crate::components::markdown::inline::InlineTextTree;
use crate::components::{
    Block, BlockKind, BlockRecord,
};
use crate::i18n::I18nManager;
use crate::theme::ThemeManager;
use gpui::{
    AppContext, Modifiers, MouseButton, MouseMoveEvent, MouseUpEvent,
    TestAppContext, point, px,
};



#[gpui::test]
async fn ending_pointer_selection_session_preserves_text_state(cx: &mut TestAppContext) {
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

    block.update(cx, |block, _cx| {
        block.is_selecting = true;
        block.code_language_is_selecting = true;
        block.selected_range = 3..7;
        block.marked_range = Some(4..6);
        block.code_language_selected_range = 1..3;
        block.code_language_marked_range = Some(1..2);

        assert!(block.end_pointer_selection_session());
        assert!(!block.is_selecting);
        assert!(!block.code_language_is_selecting);
        assert_eq!(block.selected_range, 3..7);
        assert_eq!(block.marked_range, Some(4..6));
        assert_eq!(block.code_language_selected_range, 1..3);
        assert_eq!(block.code_language_marked_range, Some(1..2));

        assert!(!block.end_pointer_selection_session());
    });
}

#[gpui::test]
async fn non_dragging_mouse_move_ends_stale_text_selection(cx: &mut TestAppContext) {
    cx.update(|cx| {
        I18nManager::init(cx);
        ThemeManager::init(cx);
    });
    let (block, cx) = cx.add_window_view(|_window, cx| {
        Block::with_record(
            cx,
            BlockRecord::new(BlockKind::Paragraph, InlineTextTree::plain("hello world")),
        )
    });

    let event = MouseMoveEvent {
        position: point(px(8.0), px(8.0)),
        pressed_button: None,
        modifiers: Modifiers::default(),
    };
    cx.update(|window, cx| {
        block.update(cx, |block, cx| {
            block.is_selecting = true;
            block.selected_range = 3..7;
            block.marked_range = Some(4..6);

            block.on_mouse_move(&event, window, cx);

            assert!(!block.is_selecting);
            assert_eq!(block.selected_range, 3..7);
            assert_eq!(block.marked_range, Some(4..6));
        });
    });
}

#[gpui::test]
async fn dragging_mouse_move_keeps_text_selection_session_active(cx: &mut TestAppContext) {
    cx.update(|cx| {
        I18nManager::init(cx);
        ThemeManager::init(cx);
    });
    let (block, cx) = cx.add_window_view(|_window, cx| {
        Block::with_record(
            cx,
            BlockRecord::new(BlockKind::Paragraph, InlineTextTree::plain("hello world")),
        )
    });

    let event = MouseMoveEvent {
        position: point(px(8.0), px(8.0)),
        pressed_button: Some(MouseButton::Left),
        modifiers: Modifiers::default(),
    };
    cx.update(|window, cx| {
        block.update(cx, |block, cx| {
            block.is_selecting = true;
            block.on_mouse_move(&event, window, cx);
            assert!(block.is_selecting);
        });
    });
}

#[gpui::test]
async fn non_dragging_mouse_move_ends_stale_code_language_selection(cx: &mut TestAppContext) {
    cx.update(|cx| {
        I18nManager::init(cx);
        ThemeManager::init(cx);
    });
    let (block, cx) = cx.add_window_view(|_window, cx| {
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

    let event = MouseMoveEvent {
        position: point(px(8.0), px(8.0)),
        pressed_button: None,
        modifiers: Modifiers::default(),
    };
    cx.update(|window, cx| {
        block.update(cx, |block, cx| {
            block.code_language_is_selecting = true;
            block.code_language_selected_range = 1..3;
            block.code_language_marked_range = Some(1..2);

            block.on_code_language_mouse_move(&event, window, cx);

            assert!(!block.code_language_is_selecting);
            assert_eq!(block.code_language_selected_range, 1..3);
            assert_eq!(block.code_language_marked_range, Some(1..2));
        });
    });
}

#[gpui::test]
async fn code_language_mouse_up_out_ends_selection_without_clearing_text_state(
    cx: &mut TestAppContext,
) {
    cx.update(|cx| {
        I18nManager::init(cx);
        ThemeManager::init(cx);
    });
    let (block, cx) = cx.add_window_view(|_window, cx| {
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

    let event = MouseUpEvent {
        position: point(px(200.0), px(200.0)),
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
        click_count: 1,
    };
    cx.update(|window, cx| {
        block.update(cx, |block, cx| {
            block.is_selecting = true;
            block.code_language_is_selecting = true;
            block.selected_range = 3..7;
            block.marked_range = Some(4..6);
            block.code_language_selected_range = 1..3;
            block.code_language_marked_range = Some(1..2);

            block.on_code_language_mouse_up_out(&event, window, cx);

            assert!(block.is_selecting);
            assert!(!block.code_language_is_selecting);
            assert_eq!(block.selected_range, 3..7);
            assert_eq!(block.marked_range, Some(4..6));
            assert_eq!(block.code_language_selected_range, 1..3);
            assert_eq!(block.code_language_marked_range, Some(1..2));
        });
    });
}

#[gpui::test]
async fn code_block_without_language_keeps_plain_rendering(cx: &mut TestAppContext) {
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::CodeBlock { language: None },
                InlineTextTree::plain("no highlighting here"),
            ),
        )
    });

    assert!(block.read_with(cx, |block, _cx| block.code_highlight_result().is_none()));
}

