//! 「格式 → 清除格式」：剥掉选区里的行内样式记号，块级记号与链接不在这一档。
//!
//! 用例钉四件事：只动选区覆盖到的那一段（边界处把样式片段切开）、写下去的字节读回同样的
//! 结构、一次撤销回到原样、菜单那一行与编辑器层入口是同一条。

use super::common::*;
use crate::components::Block;
use crate::editor::context_menu::DocumentSubmenu;
use gpui::{Entity, Modifiers, MouseButton, point};

fn visible_block(
    editor: &Entity<Editor>,
    index: usize,
    cx: &mut VisualTestContext,
) -> Entity<Block> {
    editor.read_with(cx, |editor, _cx| {
        editor.document.visible_blocks()[index].entity.clone()
    })
}

fn buffer_text(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |editor, _cx| editor.buffer.text())
}

fn focus_block_at(editor: &Entity<Editor>, index: usize, cx: &mut VisualTestContext) {
    let block = visible_block(editor, index, cx);
    editor.update(cx, |editor, _cx| editor.focus_block(block.entity_id()));
    redraw(cx);
}

fn select(
    editor: &Entity<Editor>,
    index: usize,
    range: std::ops::Range<usize>,
    cx: &mut VisualTestContext,
) {
    focus_block_at(editor, index, cx);
    visible_block(editor, index, cx).update(cx, |block, _cx| block.selected_range = range);
    redraw(cx);
}

/// 把跨块选区摆成「第 from.0 块的 from.1 处 → 第 to.0 块的 to.1 处」。
fn cross_block(
    editor: &Entity<Editor>,
    from: (usize, usize),
    to: (usize, usize),
    cx: &mut VisualTestContext,
) {
    let start = visible_block(editor, from.0, cx);
    let end = visible_block(editor, to.0, cx);
    editor.update(cx, |editor, _cx| {
        editor.cross_block_selection = Some(crate::editor::CrossBlockSelection {
            anchor: crate::editor::CrossBlockSelectionEndpoint {
                entity_id: start.entity_id(),
                offset: from.1,
            },
            focus: crate::editor::CrossBlockSelectionEndpoint {
                entity_id: end.entity_id(),
                offset: to.1,
            },
        });
    });
}

fn clear(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> bool {
    editor.update(cx, |editor, cx| editor.clear_inline_format_on_selection(cx))
}

fn undo(editor: &Entity<Editor>, cx: &mut VisualTestContext) {
    editor.update(cx, |editor, cx| editor.undo_document(cx));
    redraw(cx);
}

fn open_editor<'a>(
    text: &'static str,
    cx: &'a mut TestAppContext,
) -> (Entity<Editor>, &'a mut VisualTestContext) {
    cx.add_window_view(|_window, cx| Editor::from_markdown(cx, text.to_string(), None))
}

fn block_center(
    editor: &Entity<Editor>,
    index: usize,
    cx: &mut VisualTestContext,
) -> gpui::Point<gpui::Pixels> {
    let bounds = visible_block(editor, index, cx).read_with(cx, |block, _cx| {
        block
            .last_bounds
            .unwrap_or_else(|| panic!("第 {index} 块该有布局边界"))
    });
    point(
        bounds.left() + bounds.size.width * 0.5,
        bounds.top() + bounds.size.height * 0.5,
    )
}

fn right_click(editor: &Entity<Editor>, index: usize, cx: &mut VisualTestContext) {
    let position = block_center(editor, index, cx);
    cx.simulate_mouse_down(position, MouseButton::Right, Modifiers::none());
    cx.simulate_mouse_up(position, MouseButton::Right, Modifiers::none());
    redraw(cx);
}

fn click_menu_row(name: &'static str, cx: &mut VisualTestContext) {
    let selector: &'static str = Box::leak(format!("menu-item-{name}").into_boxed_str());
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("菜单里没渲染出 {name} 这一行"));
    let center = point(
        bounds.left() + bounds.size.width * 0.5,
        bounds.top() + bounds.size.height * 0.5,
    );
    cx.simulate_click(center, Modifiers::none());
    redraw(cx);
}

#[gpui::test]
async fn clearing_format_strips_the_emphasis_markers(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = open_editor("before **bold** after\n", cx);
    redraw(cx);

    // 这一行屏幕上是「before bold after」（17 字节），整段选中。
    select(&editor, 0, 0..17, cx);
    assert!(clear(&editor, cx));
    redraw(cx);

    assert_eq!(
        buffer_text(&editor, cx),
        "before bold after\n",
        "粗体记号没剥掉，或者顺带动到了别的字"
    );

    undo(&editor, cx);
    assert_eq!(buffer_text(&editor, cx), "before **bold** after\n");
}

#[gpui::test]
async fn clearing_format_only_touches_the_selected_span(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = open_editor("**abc**\n", cx);
    redraw(cx);

    // 实测这一块的屏幕文本带着写法（`display_text` 就是 `**abc**`），与「文字 + 记号 + 文字」
    // 那种块不一样；所以选中间那个字在屏幕坐标上是 `3..4`。
    select(&editor, 0, 3..4, cx);
    assert!(clear(&editor, cx));
    redraw(cx);

    assert_eq!(
        buffer_text(&editor, cx),
        "**a**b**c**\n",
        "只该把选中的那个字从粗体里拿出来，两边仍是粗体"
    );
}

#[gpui::test]
async fn clearing_format_strips_code_and_highlight_markers(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = open_editor("code `x` and mark ==y==\n", cx);
    redraw(cx);

    // 屏幕上这一行是「code x and mark y」（17 字节）。
    select(&editor, 0, 0..17, cx);
    assert!(clear(&editor, cx));
    redraw(cx);

    assert_eq!(
        buffer_text(&editor, cx),
        "code x and mark y\n",
        "行内代码与标记文本的成对记号都该剥掉"
    );
}

#[gpui::test]
async fn clearing_format_leaves_a_link_alone(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    const DOC: &str = "see [text](https://example.test) here\n";
    let (editor, cx) = open_editor(DOC, cx);
    redraw(cx);

    // 链接的可见文字是 `text`，整行在屏幕上是「see text here」（13 字节）。
    select(&editor, 0, 0..13, cx);
    assert!(
        !clear(&editor, cx),
        "链接是结构不是样式，这一段里没有可剥的样式记号"
    );
    assert_eq!(buffer_text(&editor, cx), DOC);
}

#[gpui::test]
async fn clearing_format_across_two_blocks_is_one_undo_step(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = open_editor("**ab**\n\n**cd**\n", cx);
    redraw(cx);

    cross_block(&editor, (0, 0), (1, 2), cx);
    assert!(clear(&editor, cx));
    redraw(cx);

    assert_eq!(
        buffer_text(&editor, cx),
        "ab\n\ncd\n",
        "跨块要逐块都剥掉，写法之外一个字节不动"
    );

    undo(&editor, cx);
    assert_eq!(
        buffer_text(&editor, cx),
        "**ab**\n\n**cd**\n",
        "两块各剥一次只算一步撤销"
    );
}

#[gpui::test]
async fn the_clear_format_row_in_the_format_menu_reaches_the_same_entry(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = open_editor("before **bold** after\n", cx);
    redraw(cx);

    select(&editor, 0, 0..17, cx);
    right_click(&editor, 0, cx);
    editor.update(cx, |editor, cx| {
        editor.set_document_menu_hover(true, Some(DocumentSubmenu::Format), cx)
    });
    redraw(cx);

    click_menu_row("clear-format", cx);
    assert_eq!(
        buffer_text(&editor, cx),
        "before bold after\n",
        "菜单里的「清除格式」没剥掉记号"
    );
    assert!(
        !editor.read_with(cx, |editor, _| editor.context_menu.is_some()),
        "点完一行菜单该收起"
    );
}

/// 「清除格式」这一档现在点得动吗：判定要跟着选区里实际挂着的东西走。
///
/// 工具栏那颗格子与右键菜单那一行读的是同一条 `Editor::clear_format_is_available`，
/// 三档口径分开钉：裸字（没东西可剥，灰）、带粗体（亮）、只有链接（灰——链接是结构
/// 不是样式，与 `clearing_format_leaves_a_link_alone` 那条写回口径一字不差）。
/// 跨块的那一段两头都问，只要有一头挂着样式就该亮。
#[gpui::test]
async fn clear_format_availability_follows_the_selection(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = open_editor(
        "裸字一段\n\n**带粗体的一段**\n\n[链接文字](https://example.test)\n",
        cx,
    );
    redraw(cx);
    let available = |editor: &Entity<Editor>, cx: &mut VisualTestContext| {
        editor.read_with(cx, |editor, cx| editor.clear_format_is_available(cx))
    };
    let whole = |editor: &Entity<Editor>, index: usize, cx: &mut VisualTestContext| {
        let len = visible_block(editor, index, cx).read_with(cx, |block, _| block.visible_len());
        select(editor, index, 0..len, cx);
    };

    whole(&editor, 0, cx);
    assert!(
        !available(&editor, cx),
        "选中的是没样式的裸字，这一档该点不动（灰着才对，别让人点了没反应）"
    );

    whole(&editor, 1, cx);
    assert!(available(&editor, cx), "选中了带粗体那一段，这一档该点得动");

    whole(&editor, 2, cx);
    assert!(
        !available(&editor, cx),
        "只选中一个链接：链接不在「清除格式」这一档的口径里，该点不动"
    );

    // 裸字第 0 块 → 带粗体第 1 块：跨块里有一块挂着样式就该亮。
    let second_len = visible_block(&editor, 1, cx).read_with(cx, |block, _| block.visible_len());
    cross_block(&editor, (0, 0), (1, second_len), cx);
    assert!(
        available(&editor, cx),
        "跨块选区覆盖到了粗体那一段，该点得动"
    );
    cross_block(&editor, (0, 0), (2, 2), cx);
    assert!(
        available(&editor, cx),
        "跨块的另一头（第 2 块）只有链接，第 1 块仍带粗体，也该点得动"
    );
}
