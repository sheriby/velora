//! 正文最后一行下方那一大片空白点不了：在那儿点下去光标不动，鼠标也不是竖线。

use super::common::*;

/// 报修 1：最后一行下方的空白里按下，光标要落到文末，接着就能打字。
///
/// 现象：点下去什么都没发生。根因：正文块自己的命中测试只看块内，块底以下的空间
/// 没有块接手，编辑器也只在按下时清了一下菜单栏，光标不动。
#[gpui::test]
async fn clicking_below_the_last_line_puts_the_caret_at_the_document_end(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    let markdown = "短的正文一段\n";
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, markdown.to_string(), None));
    redraw(cx);
    redraw(cx);

    let (last_id, bounds) = editor.read_with(cx, |editor, cx| {
        let block = editor
            .document
            .visible_blocks()
            .last()
            .expect("文档有一块")
            .entity
            .clone();
        (
            block.entity_id(),
            block.read(cx).last_bounds.expect("正文块有布局"),
        )
    });

    // 用户口径：最后一行往下大约半个屏幕的那片空白。
    let click = gpui::point(bounds.center().x, bounds.bottom() + px(300.0));
    cx.simulate_mouse_down(click, gpui::MouseButton::Left, Modifiers::none());
    redraw(cx);
    cx.simulate_mouse_up(click, gpui::MouseButton::Left, Modifiers::none());
    redraw(cx);

    editor.read_with(cx, |editor, cx| {
        let block = editor
            .document
            .block_entity_by_id(last_id)
            .expect("最后一块还在");
        let state = block.read(cx);
        assert_eq!(
            state.cursor_offset(),
            state.display_text().len(),
            "点末尾下方的空白，光标要落到文末（文本 {:?}）",
            state.display_text()
        );
        assert_eq!(
            editor.active_entity_id,
            Some(last_id),
            "编辑目标要停在最后一块"
        );
    });

    cx.simulate_input("尾巴");
    redraw(cx);
    editor.read_with(cx, |editor, cx| {
        assert_eq!(
            editor
                .document
                .block_entity_by_id(last_id)
                .expect("最后一块还在")
                .read(cx)
                .display_text(),
            "短的正文一段尾巴",
            "点完要能直接接在文末打字"
        );
    });
}
