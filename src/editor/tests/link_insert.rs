//! 「格式 → 链接」那一条入口：⌘K、右键菜单、选中工具栏三个入口写的字节必须一样。
//!
//! 这一笔只补链接的外壳 `[文字]()`，地址留给用户当场写，所以用例盯四件事：缓冲区只在
//! 选区那一处动、光标落在 `](` 之后（只有光标时落在 `[]` 中间）、块树不被拆开、一步撤销回到原样。

use super::common::*;
use crate::components::{Block, LinkSelection};
use crate::editor::context_menu::DocumentSubmenu;
use gpui::{Entity, Modifiers, MouseButton, point};

const TWO_PARAGRAPHS: &str = "alpha one\n\nbeta two\n";

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

fn kinds(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> Vec<String> {
    editor.read_with(cx, |editor, cx| {
        editor
            .document
            .root_blocks()
            .iter()
            .map(|block| format!("{:?}", block.read(cx).kind()))
            .collect()
    })
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

fn caret_of(
    editor: &Entity<Editor>,
    index: usize,
    cx: &mut VisualTestContext,
) -> std::ops::Range<usize> {
    visible_block(editor, index, cx).read_with(cx, |block, _cx| block.selected_range.clone())
}

fn focused_id(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> Option<gpui::EntityId> {
    editor.read_with(cx, |editor, _| editor.active_entity_id)
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

fn undo(editor: &Entity<Editor>, cx: &mut VisualTestContext) {
    editor.update(cx, |editor, cx| editor.undo_document(cx));
    redraw(cx);
}

fn wrap(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> bool {
    editor.update(cx, |editor, cx| editor.insert_link_on_selection(cx))
}

fn available(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> bool {
    editor.read_with(cx, |editor, cx| editor.link_insert_is_available(cx))
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

fn row_bounds(name: &'static str, cx: &mut VisualTestContext) -> gpui::Bounds<gpui::Pixels> {
    let selector: &'static str = Box::leak(format!("menu-item-{name}").into_boxed_str());
    cx.debug_bounds(selector)
        .unwrap_or_else(|| panic!("菜单里没渲染出 {name} 这一行"))
}

fn click_row(name: &'static str, cx: &mut VisualTestContext) {
    let bounds = row_bounds(name, cx);
    let center = point(
        bounds.left() + bounds.size.width * 0.5,
        bounds.top() + bounds.size.height * 0.5,
    );
    cx.simulate_click(center, Modifiers::none());
    redraw(cx);
}

fn open_format_submenu(editor: &Entity<Editor>, index: usize, cx: &mut VisualTestContext) {
    right_click(editor, index, cx);
    editor.update(cx, |editor, cx| {
        editor.set_document_menu_hover(true, Some(DocumentSubmenu::Format), cx)
    });
    redraw(cx);
}
#[gpui::test]
async fn wrapping_a_selection_writes_the_link_shell_and_parks_the_caret_in_the_url_slot(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    let (editor, cx) = open_editor(TWO_PARAGRAPHS, cx);
    redraw(cx);

    select(&editor, 0, 0..5, cx);
    assert!(wrap(&editor, cx));
    redraw(cx);

    assert_eq!(
        buffer_text(&editor, cx),
        "[alpha]() one\n\nbeta two\n",
        "链接的外壳没写回缓冲区，或者顺带动到了选区之外的字节"
    );
    // `[alpha]()` 的 `(` 在下标 7，光标该紧跟着它，用户接着写地址。
    assert_eq!(caret_of(&editor, 0, cx), 8..8, "光标没落在括号中间");
    assert_eq!(
        focused_id(&editor, cx),
        Some(visible_block(&editor, 0, cx).entity_id()),
        "写完焦点该留在改过的那一块"
    );
    assert_eq!(
        kinds(&editor, cx),
        ["Paragraph".to_string(), "Paragraph".to_string()],
        "包一层链接不该把块拆开或换种类"
    );

    undo(&editor, cx);
    assert_eq!(
        buffer_text(&editor, cx),
        TWO_PARAGRAPHS,
        "一步撤销要把外壳整个放回去"
    );
}

#[gpui::test]
async fn a_bare_caret_leaves_the_link_row_pointless(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = open_editor(TWO_PARAGRAPHS, cx);
    redraw(cx);

    select(&editor, 0, 0..0, cx);
    assert!(
        !available(&editor, cx),
        "只有光标时这一行该置灰：空的 `[]()` 在行内树里存不住"
    );
    assert!(!wrap(&editor, cx), "点不动就不该动字节");
    assert_eq!(buffer_text(&editor, cx), TWO_PARAGRAPHS);
    assert_eq!(
        editor.read_with(cx, |editor, cx| {
            editor
                .document_submenu_rows(DocumentSubmenu::Format, cx)
                .into_iter()
                .filter_map(|row| match row {
                    crate::editor::context_menu::DocumentMenuRow::Item {
                        name, enabled, ..
                    } => Some((name, enabled)),
                    _ => None,
                })
                .find(|(name, _)| *name == "link")
                .map(|(_, enabled)| enabled)
        }),
        Some(false),
        "菜单里那一行的置灰口径要与 `link_insert_is_available` 同源"
    );
}

#[gpui::test]
async fn a_selection_spanning_two_blocks_wraps_each_block_and_undoes_as_one_step(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    let (editor, cx) = open_editor(TWO_PARAGRAPHS, cx);
    redraw(cx);

    cross_block(&editor, (0, 0), (1, 4), cx);
    assert!(wrap(&editor, cx));
    redraw(cx);

    assert_eq!(
        buffer_text(&editor, cx),
        "[alpha one]()\n\n[beta]() two\n",
        "跨块要逐块各包一层：markdown 的行内语法本来就不跨块"
    );
    assert_eq!(
        caret_of(&editor, 0, cx),
        12..12,
        "光标该落在第一段写地址的那一处"
    );
    assert_eq!(
        focused_id(&editor, cx),
        Some(visible_block(&editor, 0, cx).entity_id()),
        "两段各改一次只算一步，焦点跟第一段"
    );

    undo(&editor, cx);
    assert_eq!(
        buffer_text(&editor, cx),
        TWO_PARAGRAPHS,
        "跨块包链接必须是一步撤销，两段同时放回去"
    );
}

#[gpui::test]
async fn wrapping_leaves_a_list_markers_writing_alone(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = open_editor("- alpha\n\n- beta\n", cx);
    redraw(cx);

    select(&editor, 0, 0..5, cx);
    assert!(wrap(&editor, cx));
    redraw(cx);

    assert_eq!(
        buffer_text(&editor, cx),
        "- [alpha]()\n\n- beta\n",
        "只在选区那五个字节上动，`- ` 记号与第二块原样不动"
    );
}

#[gpui::test]
async fn the_key_binding_reaches_the_same_entry(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = open_editor(TWO_PARAGRAPHS, cx);
    redraw(cx);

    select(&editor, 0, 6..9, cx);
    cx.dispatch_action(LinkSelection);
    redraw(cx);

    assert_eq!(
        buffer_text(&editor, cx),
        "alpha [one]()\n\nbeta two\n",
        "⌘K 走的该是编辑器层那一条入口，字节与菜单一致"
    );
    assert_eq!(caret_of(&editor, 0, cx), 12..12);
}

#[gpui::test]
async fn the_link_row_in_the_format_menu_reaches_the_same_entry(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = open_editor(TWO_PARAGRAPHS, cx);
    redraw(cx);

    select(&editor, 0, 0..5, cx);
    // 右键按块的中心派发：这一块的选区已经选好，菜单收起时选区不被改动。
    open_format_submenu(&editor, 0, cx);
    for name in [
        "bold",
        "italic",
        "underline",
        "strikethrough",
        "code",
        "highlight",
        "superscript",
        "subscript",
        "link",
    ] {
        row_bounds(name, cx);
    }

    click_row("link", cx);
    assert_eq!(
        buffer_text(&editor, cx),
        "[alpha]() one\n\nbeta two\n",
        "菜单里的「链接」没落到缓冲区"
    );
    assert!(
        !editor.read_with(cx, |editor, _| editor.context_menu.is_some()),
        "点完一行菜单该收起"
    );

    undo(&editor, cx);
    assert_eq!(buffer_text(&editor, cx), TWO_PARAGRAPHS);
}

#[gpui::test]
async fn the_toolbar_link_button_reaches_the_same_entry(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = open_editor(TWO_PARAGRAPHS, cx);
    redraw(cx);

    select(&editor, 0, 0..5, cx);
    let bounds = cx
        .debug_bounds("toolbar-link")
        .expect("工具栏里该有链接那颗按钮");
    let center = point(
        bounds.left() + bounds.size.width * 0.5,
        bounds.top() + bounds.size.height * 0.5,
    );
    cx.simulate_click(center, Modifiers::none());
    redraw(cx);

    assert_eq!(
        buffer_text(&editor, cx),
        "[alpha]() one\n\nbeta two\n",
        "工具栏那颗按钮写的字节要与菜单、⌘K 同一条"
    );
}
