
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

fn approximate_geometry_line(text: &str) -> gpui::WrappedLine {
    // 模拟几何层返回字符内部的近似字节偏移，只验证边界收敛。
    let mut line = gpui::WrappedLine::default();
    *std::ops::DerefMut::deref_mut(&mut line) = std::sync::Arc::new(gpui::WrappedLineLayout {
        unwrapped_layout: std::sync::Arc::new(gpui::LineLayout { len: 1, ..Default::default() }),
        wrap_boundaries: Default::default(),
        wrap_width: None,
    });
    line.text = text.to_owned().into();
    line
}

#[gpui::test]
async fn review_vertical_entry_clamps_geometry_to_character_boundary(cx: &mut TestAppContext) {
    let block = cx.new(|cx| Block::with_record(cx, BlockRecord::new(BlockKind::Paragraph, InlineTextTree::plain("中文"))));
    block.update(cx, |block, _| {
        block.last_line_height = px(16.0);
        block.last_layout = Some(std::sync::Arc::new(vec![approximate_geometry_line("中文")]));
        assert_eq!(block.entry_offset_for_vertical_focus(false, Some(px(8.0))), 0);
    });
}

#[gpui::test]
async fn review_vertical_motion_clamps_geometry_to_character_boundary(cx: &mut TestAppContext) {
    let block = cx.new(|cx| Block::with_record(cx, BlockRecord::new(BlockKind::Paragraph, InlineTextTree::plain("a\n中文"))));
    block.update(cx, |block, cx| {
        block.last_line_height = px(16.0);
        block.last_layout = Some(std::sync::Arc::new(vec![approximate_geometry_line("a"), approximate_geometry_line("中文")]));
        assert!(block.move_cursor_vertically(1, px(8.0), cx));
        assert_eq!(block.cursor_offset(), 2);
    });
}

#[gpui::test]
async fn review_code_language_click_clamps_geometry_to_character_boundary(cx: &mut TestAppContext) {
    let block = cx.new(|cx| Block::with_record(cx, BlockRecord::new(
        BlockKind::CodeBlock { language: Some("中文".into()) }, InlineTextTree::plain("代码"),
    )));
    block.update(cx, |block, _| {
        block.code_language_last_bounds = Some(gpui::Bounds::new(point(px(0.0), px(0.0)), gpui::size(px(16.0), px(16.0))));
        block.code_language_last_layout = Some(gpui::ShapedLine::default().with_len(1));
        assert_eq!(block.code_language_index_for_mouse_position(point(px(8.0), px(8.0))), 0);
    });
}

#[gpui::test]
async fn review_input_point_clamps_geometry_before_utf16_conversion(cx: &mut TestAppContext) {
    cx.update(|cx| { I18nManager::init(cx); ThemeManager::init(cx); });
    let (block, cx) = cx.add_window_view(|_, cx| Block::with_record(
        cx, BlockRecord::new(BlockKind::Paragraph, InlineTextTree::plain("a\n中文")),
    ));
    cx.update(|window, cx| block.update(cx, |block, cx| {
        block.last_line_height = px(16.0);
        block.last_bounds = Some(gpui::Bounds::new(point(px(0.0), px(0.0)), gpui::size(px(16.0), px(32.0))));
        block.last_layout = Some(std::sync::Arc::new(vec![approximate_geometry_line("a"), approximate_geometry_line("中文")]));
        assert_eq!(gpui::EntityInputHandler::character_index_for_point(block, point(px(8.0), px(24.0)), window, cx), Some(2));
    }));
}



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

