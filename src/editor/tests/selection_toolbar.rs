//! 选中后浮出的那条工具栏：出现与消失的时机、点按钮改字节且不抢焦点、贴顶翻转、
//! 与右键菜单互斥。

use super::common::*;
use crate::components::Block;
use gpui::{px, Entity, Modifiers, MouseButton, Point};

const TWO_PARAGRAPHS: &str = "alpha one\n\nbeta two\n";

const TOOLBAR_BUTTONS: [&str; 8] = [
    "toolbar-heading",
    "toolbar-bold",
    "toolbar-italic",
    "toolbar-underline",
    "toolbar-strikethrough",
    "toolbar-code",
    "toolbar-highlight",
    "toolbar-link",
];

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

fn block_bounds(
    editor: &Entity<Editor>,
    index: usize,
    cx: &mut VisualTestContext,
) -> gpui::Bounds<gpui::Pixels> {
    visible_block(editor, index, cx).read_with(cx, |block, _cx| {
        block
            .last_bounds
            .unwrap_or_else(|| panic!("第 {index} 块该有布局边界"))
    })
}

fn toolbar_bounds(cx: &mut VisualTestContext) -> Option<gpui::Bounds<gpui::Pixels>> {
    cx.debug_bounds("editor-selection-toolbar")
}

fn button_bounds(name: &'static str, cx: &mut VisualTestContext) -> gpui::Bounds<gpui::Pixels> {
    cx.debug_bounds(name)
        .unwrap_or_else(|| panic!("工具栏里没渲染出 {name}"))
}

fn center_of(bounds: gpui::Bounds<gpui::Pixels>) -> Point<gpui::Pixels> {
    gpui::point(
        bounds.left() + bounds.size.width * 0.5,
        bounds.top() + bounds.size.height * 0.5,
    )
}

fn click_element(name: &'static str, cx: &mut VisualTestContext) {
    let center = center_of(button_bounds(name, cx));
    cx.simulate_click(center, Modifiers::none());
    redraw(cx);
}

fn select_head_of_first_block(editor: &Entity<Editor>, cx: &mut VisualTestContext) {
    let block = visible_block(editor, 0, cx);
    editor.update(cx, |editor, _cx| editor.focus_block(block.entity_id()));
    block.update(cx, |block, _cx| block.selected_range = 0..5);
    redraw(cx);
}

/// 从第 from 块「距左缘 from_offset 像素」那一点拖到第 to 块同样量法的那一点。
/// 按像素而不是按块宽比例：块宽是整个正文列宽，文字只占左边一小段，比例拖到
/// 中间就已经落在文字之外，两次落点会收成同一个光标。
fn drag_selection(
    editor: &Entity<Editor>,
    from: (usize, f32),
    to: (usize, f32),
    cx: &mut VisualTestContext,
) {
    let mut point_in = |index: usize, offset: f32| {
        let bounds = block_bounds(editor, index, cx);
        gpui::point(
            bounds.left() + px(offset),
            bounds.top() + bounds.size.height * 0.5,
        )
    };
    let start = point_in(from.0, from.1);
    let end = point_in(to.0, to.1);
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
    redraw(cx);
    cx.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::none());
    redraw(cx);
    cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::none());
    redraw(cx);
}

/// 测试里按 f32 写坐标更顺手，这一层把两个数变成像素点。
fn point(x: f32, y: f32) -> Point<gpui::Pixels> {
    gpui::point(px(x), px(y))
}

fn toolbar_is_hidden(cx: &mut VisualTestContext) -> bool {
    toolbar_bounds(cx).is_none()
}

#[gpui::test]
async fn dragging_a_selection_pops_the_toolbar_after_the_button_is_released(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    let (editor, cx) = cx
        .add_window_view(|_window, cx| Editor::from_markdown(cx, TWO_PARAGRAPHS.to_string(), None));
    redraw(cx);

    let start = {
        let bounds = block_bounds(&editor, 0, cx);
        gpui::point(bounds.left() + px(10.0), bounds.center().y)
    };
    let end = {
        let bounds = block_bounds(&editor, 1, cx);
        gpui::point(bounds.left() + px(40.0), bounds.center().y)
    };
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
    redraw(cx);
    cx.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::none());
    redraw(cx);
    assert!(
        toolbar_is_hidden(cx),
        "还在拖动选区的时候就该浮出工具栏，会挡住正在拖的那段文字"
    );

    cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::none());
    redraw(cx);
    assert!(
        editor.read_with(cx, |editor, _| editor.cross_block_selection.is_some()),
        "这段拖动应当成立为跨块选区"
    );
    let _ = toolbar_bounds(cx).expect("抬手之后工具栏该浮出来");
    for name in TOOLBAR_BUTTONS {
        let bounds = button_bounds(name, cx);
        assert!(
            f32::from(bounds.size.width) > 0.0 && f32::from(bounds.size.height) > 0.0,
            "{name} 没有尺寸"
        );
    }
}

#[gpui::test]
async fn clicking_bold_in_the_toolbar_edits_the_buffer_and_keeps_the_selection(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    let (editor, cx) = cx
        .add_window_view(|_window, cx| Editor::from_markdown(cx, TWO_PARAGRAPHS.to_string(), None));
    redraw(cx);

    select_head_of_first_block(&editor, cx);
    let _ = toolbar_bounds(cx).expect("有选区就该有工具栏");
    click_element("toolbar-bold", cx);

    assert_eq!(
        buffer_text(&editor, cx),
        "**alpha** one\n\nbeta two\n",
        "工具栏的「加粗」没写回缓冲区"
    );
    let focused = editor.read_with(cx, |editor, _| editor.active_entity_id);
    assert_eq!(
        focused,
        Some(visible_block(&editor, 0, cx).entity_id()),
        "点工具栏把编辑目标的焦点抢走了"
    );
    let selected =
        visible_block(&editor, 0, cx).read_with(cx, |block, _cx| block.selected_range.clone());
    assert!(!selected.is_empty(), "点完按钮选区被收掉了：{selected:?}");

    editor.update(cx, |editor, cx| editor.undo_document(cx));
    redraw(cx);
    assert_eq!(
        buffer_text(&editor, cx),
        TWO_PARAGRAPHS,
        "工具栏这次改动撤销一步没复原"
    );
}

#[gpui::test]
async fn a_collapsed_selection_hides_the_toolbar(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx
        .add_window_view(|_window, cx| Editor::from_markdown(cx, TWO_PARAGRAPHS.to_string(), None));
    redraw(cx);

    select_head_of_first_block(&editor, cx);
    assert!(toolbar_bounds(cx).is_some(), "有选区时没浮出工具栏");

    let caret = {
        let bounds = block_bounds(&editor, 0, cx);
        gpui::point(
            bounds.left() + px(30.0),
            bounds.top() + bounds.size.height * 0.5,
        )
    };
    cx.simulate_click(caret, Modifiers::none());
    redraw(cx);
    assert!(
        visible_block(&editor, 0, cx).read_with(cx, |block, _cx| block.selected_range.is_empty()),
        "按下之后应当只剩光标"
    );
    assert!(toolbar_is_hidden(cx), "选区塌成光标后工具栏该收掉");
}

#[gpui::test]
async fn the_heading_menu_lists_six_levels_and_plain_text(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx
        .add_window_view(|_window, cx| Editor::from_markdown(cx, TWO_PARAGRAPHS.to_string(), None));
    redraw(cx);

    select_head_of_first_block(&editor, cx);
    click_element("toolbar-heading", cx);
    let menu = cx
        .debug_bounds("editor-toolbar-heading-menu")
        .expect("点「标题」该展开档位列表");
    let toolbar = toolbar_bounds(cx).expect("工具栏还在");
    assert!(
        f32::from(menu.top()) >= f32::from(toolbar.bottom())
            || f32::from(menu.bottom()) <= f32::from(toolbar.top()),
        "档位列表该贴在工具栏的上下某一侧，不该盖住它"
    );

    click_element("menu-item-heading-2", cx);
    assert_eq!(
        buffer_text(&editor, cx),
        "## alpha one\n\nbeta two\n",
        "档位列表里的「二级标题」没把这一段转成标题"
    );
    assert!(
        cx.debug_bounds("editor-toolbar-heading-menu").is_none(),
        "选完档位之后列表该收起"
    );

    editor.update(cx, |editor, cx| editor.undo_document(cx));
    redraw(cx);
    assert_eq!(buffer_text(&editor, cx), TWO_PARAGRAPHS);
}

/// 工具栏「标题」下拉与右键菜单的「段落」那一档同源：列表那三行也在这里，
/// 点一行写回的是同一段字节。
#[gpui::test]
async fn the_paragraph_menu_offers_the_same_list_rows_as_the_context_menu(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx
        .add_window_view(|_window, cx| Editor::from_markdown(cx, TWO_PARAGRAPHS.to_string(), None));
    redraw(cx);

    select_head_of_first_block(&editor, cx);
    click_element("toolbar-heading", cx);
    for name in [
        "menu-item-heading-6",
        "menu-item-normal-text",
        "menu-item-bullet-list",
        "menu-item-numbered-list",
        "menu-item-task-list",
        "menu-item-quote",
        "menu-item-code-block",
    ] {
        assert!(cx.debug_bounds(name).is_some(), "档位列表里没渲染出 {name}");
    }

    click_element("menu-item-numbered-list", cx);
    assert_eq!(
        buffer_text(&editor, cx),
        "1. alpha one\n\nbeta two\n",
        "工具栏里的「有序列表」没把这一段转成有序项"
    );

    editor.update(cx, |editor, cx| editor.undo_document(cx));
    redraw(cx);
    assert_eq!(
        buffer_text(&editor, cx),
        TWO_PARAGRAPHS,
        "一次撤销该整步退回"
    );
}

/// 代码块上弹不出正文右键菜单，退回正文那一步由工具栏这一档给：选中代码块里的文字，
/// 下拉里的「代码块」这一行点下去就收掉围栏。
#[gpui::test]
async fn the_paragraph_row_unfences_a_code_block(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "```\n代码甲\n```\n\n正文乙\n".to_string(), None)
    });
    redraw(cx);

    let block = visible_block(&editor, 0, cx);
    editor.update(cx, |editor, _cx| editor.focus_block(block.entity_id()));
    block.update(cx, |block, _cx| block.selected_range = 0..3);
    redraw(cx);
    click_element("toolbar-heading", cx);
    click_element("menu-item-code-block", cx);

    assert_eq!(
        buffer_text(&editor, cx),
        "代码甲\n\n正文乙\n",
        "工具栏里的「代码块」没把这一段从围栏里放出来"
    );
}

#[gpui::test]
async fn the_toolbar_sits_above_the_selection(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx
        .add_window_view(|_window, cx| Editor::from_markdown(cx, TWO_PARAGRAPHS.to_string(), None));
    redraw(cx);

    drag_selection(&editor, (0, 4.0), (1, 4.0), cx);
    let selection = block_bounds(&editor, 0, cx);
    let toolbar = toolbar_bounds(cx).expect("拖完该有工具栏");
    assert!(
        f32::from(toolbar.bottom()) <= f32::from(selection.top()),
        "空间够时工具栏该在选区上方：{} vs {}",
        f32::from(toolbar.bottom()),
        f32::from(selection.top())
    );
    // 面板中心该落在被选中的那段文字覆盖的横向范围里。
    let center = f32::from(toolbar.center().x);
    let selection_left = f32::from(block_bounds(&editor, 0, cx).left());
    let selection_right = f32::from(block_bounds(&editor, 1, cx).right());
    assert!(
        center >= selection_left && center <= selection_right,
        "工具栏横向不该甩到选区之外：中心 {center} 不在 [{selection_left}, {selection_right}] 里"
    );
}

/// 选区整段不在视口里就不该再画工具栏：`last_bounds` 可能停在滚动前的位置。
#[test]
fn an_offscreen_selection_hides_the_toolbar() {
    let viewport = gpui::size(px(600.0), px(400.0));
    let box_at = |x: f32, y: f32| gpui::Bounds::new(point(x, y), gpui::size(px(200.0), px(20.0)));
    assert!(
        Editor::selection_is_on_screen(box_at(100.0, 100.0), viewport),
        "视口里的选区被判成了看不见"
    );
    assert!(!Editor::selection_is_on_screen(
        box_at(100.0, 460.0),
        viewport
    ));
    assert!(!Editor::selection_is_on_screen(
        box_at(100.0, -40.0),
        viewport
    ));
    assert!(!Editor::selection_is_on_screen(
        box_at(700.0, 100.0),
        viewport
    ));
    // 只露一角也算看得见：工具栏还有得锚。
    assert!(Editor::selection_is_on_screen(
        box_at(590.0, 395.0),
        viewport
    ));
}

/// 视口压矮到只剩几行：工具栏放不下上半区时收到的仍是视口内，不露出一半。
#[gpui::test]
async fn a_short_viewport_keeps_the_toolbar_inside_it(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx
        .add_window_view(|_window, cx| Editor::from_markdown(cx, TWO_PARAGRAPHS.to_string(), None));
    cx.simulate_resize(gpui::size(px(600.0), px(150.0)));
    redraw(cx);
    redraw(cx);

    // 矮视口里第二行已经掉出窗口，拖动只在第一块里进行。
    drag_selection(&editor, (0, 4.0), (0, 30.0), cx);
    let toolbar = toolbar_bounds(cx).expect("矮视口里也该有工具栏");
    let viewport = cx.update(|window, _cx| window.viewport_size());
    assert!(
        f32::from(toolbar.bottom()) <= f32::from(viewport.height),
        "工具栏越过了视口下沿：{} > {}",
        f32::from(toolbar.bottom()),
        f32::from(viewport.height)
    );
    assert!(
        f32::from(toolbar.right()) <= f32::from(viewport.width),
        "工具栏越过了视口右沿"
    );
}

#[gpui::test]
async fn the_toolbar_yields_to_the_context_menu_and_to_source_mode(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx
        .add_window_view(|_window, cx| Editor::from_markdown(cx, TWO_PARAGRAPHS.to_string(), None));
    redraw(cx);

    select_head_of_first_block(&editor, cx);
    assert!(toolbar_bounds(cx).is_some(), "有选区时没浮出工具栏");

    let bounds = block_bounds(&editor, 0, cx);
    let position = gpui::point(
        bounds.left() + px(20.0),
        bounds.top() + bounds.size.height * 0.5,
    );
    cx.simulate_mouse_down(position, MouseButton::Right, Modifiers::none());
    cx.simulate_mouse_up(position, MouseButton::Right, Modifiers::none());
    redraw(cx);
    assert!(
        cx.debug_bounds("menu-item-undo").is_some(),
        "右键菜单该照常弹出"
    );
    assert!(toolbar_is_hidden(cx), "右键菜单开着时不该同时浮着工具栏");

    cx.dispatch_action(crate::components::ToggleViewMode);
    redraw(cx);
    assert!(
        editor.read_with(cx, |editor, _| editor.view_mode == ViewMode::Source),
        "切不到源码模式"
    );
    assert!(
        toolbar_is_hidden(cx),
        "源码模式里没有渲染块可锚，工具栏不该出现"
    );
}

/// 面板落点的三条规则：优先上方、上方放不下改下方、越界的边按视口收回。
#[test]
fn toolbar_origin_prefers_above_and_falls_back_below() {
    let size = gpui::size(px(200.0), px(34.0));
    let viewport = gpui::size(px(600.0), px(400.0));

    // 选区在中间：贴着选区上沿再往上 8px，水平居中。
    let selection = gpui::Bounds::new(point(100.0, 200.0), gpui::size(px(120.0), px(20.0)));
    let origin = Editor::toolbar_origin(selection, size, viewport);
    assert_eq!((f32::from(origin.x), f32::from(origin.y)), (60.0, 158.0));

    // 选区贴顶：上方放不下，改到选区下方 8px。
    let selection = gpui::Bounds::new(point(100.0, 20.0), gpui::size(px(120.0), px(20.0)));
    let origin = Editor::toolbar_origin(selection, size, viewport);
    assert_eq!(f32::from(origin.y), 48.0);

    // 选区贴右下角：左右与下沿都按视口收回，不留半截在外面。
    let selection = gpui::Bounds::new(point(520.0, 380.0), gpui::size(px(70.0), px(18.0)));
    let origin = Editor::toolbar_origin(selection, size, viewport);
    assert!(f32::from(origin.x) + 200.0 <= 600.0 - 6.0 + f32::EPSILON);
    assert!(f32::from(origin.y) + 34.0 <= 400.0 - 6.0 + f32::EPSILON);
    assert!(f32::from(origin.y) >= 6.0);
}
