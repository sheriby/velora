//! 正文右键菜单：从真实的右键事件出发，验五段行都渲染出来、置灰口径、
//! 二级面板的展开与落点，以及点一行确实改缓冲区且撤销一步能复原。

use super::common::*;
use crate::components::{Block, InlineFormat, install_keybindings};
use crate::editor::context_menu::{
    document_menu_shortcut, DocumentMenuCommand, DocumentMenuRow, DocumentSubmenu,
};
use gpui::{point, px, Entity, Modifiers, MouseButton, Size};

const TWO_PARAGRAPHS: &str = "alpha one\n\nbeta two\n";

/// 主菜单十三行加四条分节：行 id 与 `document_menu_rows` 里给的一致。
const MAIN_ROWS: [&str; 13] = [
    "undo",
    "redo",
    "cut",
    "copy",
    "paste",
    "paste-as-plain-text",
    "select-all",
    "copy-as-markdown",
    "copy-as-html",
    "format",
    "paragraph",
    "insert",
    "toggle-source-view",
];

const FORMAT_ROWS: [&str; 10] = [
    "bold",
    "italic",
    "underline",
    "strikethrough",
    "code",
    "highlight",
    "superscript",
    "subscript",
    "link",
    "clear-format",
];

/// 菜单里的「全选」与 ⌘A 是同一条循环：第一次选当前这一块，紧接着再来一次选整篇。
#[gpui::test]
async fn the_select_all_row_follows_the_same_cycle_as_the_key(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx
        .add_window_view(|_window, cx| Editor::from_markdown(cx, TWO_PARAGRAPHS.to_string(), None));
    redraw(cx);

    right_click(&editor, 0, cx);
    click_row("select-all", cx);
    let block = visible_block(&editor, 0, cx);
    assert_eq!(
        block.read_with(cx, |block, _| block.selected_range.clone()),
        0..9,
        "第一次该把「alpha one」这九个字选上"
    );
    assert!(
        editor.read_with(cx, |editor, _| editor.cross_block_selection.is_none()),
        "第一次不该直接跳到整篇"
    );

    right_click(&editor, 0, cx);
    click_row("select-all", cx);
    assert!(
        editor.read_with(cx, |editor, _| editor.cross_block_selection.is_some()),
        "紧接着再来一次该选整篇"
    );
}

/// 「拷贝为 HTML」给的是渲染过的那份，且不动文档字节。
#[gpui::test]
async fn the_copy_as_html_row_puts_rendered_html_on_the_clipboard(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let document = "前 **加粗** 后\n";
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, document.to_string(), None));
    redraw(cx);

    right_click(&editor, 0, cx);
    click_row("copy-as-html", cx);
    let html = cx
        .update(|_window, cx| cx.read_from_clipboard().and_then(|item| item.text()))
        .expect("剪贴板里该有那份 HTML");
    assert!(
        html.contains("<strong>加粗</strong>"),
        "拷贝为 HTML 给的该是渲染过的那份：{html}"
    );
    assert_eq!(buffer_text(&editor, cx), document, "拷贝不该改文档");
}

/// 「段落」那一档：六个标题级别、正文、列表的三种、引用与代码块。
const PARAGRAPH_ROWS: [&str; 12] = [
    "heading-1",
    "heading-2",
    "heading-3",
    "heading-4",
    "heading-5",
    "heading-6",
    "normal-text",
    "bullet-list",
    "numbered-list",
    "task-list",
    "quote",
    "code-block",
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

/// 块在屏幕上的中心点：右键事件按这个坐标派发，走的是渲染层那条命中路径。
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
    right_click_at(position, cx);
}

fn right_click_at(position: gpui::Point<gpui::Pixels>, cx: &mut VisualTestContext) {
    cx.simulate_mouse_down(position, MouseButton::Right, Modifiers::none());
    cx.simulate_mouse_up(position, MouseButton::Right, Modifiers::none());
    redraw(cx);
}

/// `debug_bounds` 只收 `&'static str`，选择器按行名现拼，就用仓里既有的 `Box::leak` 写法。
fn item_selector(name: &'static str) -> &'static str {
    Box::leak(format!("menu-item-{name}").into_boxed_str())
}

fn row_bounds(name: &'static str, cx: &mut VisualTestContext) -> gpui::Bounds<gpui::Pixels> {
    cx.debug_bounds(item_selector(name))
        .unwrap_or_else(|| panic!("菜单里没渲染出 {name} 这一行"))
}

fn shortcut_selector(label: &str) -> &'static str {
    Box::leak(format!("menu-shortcut-{label}").into_boxed_str())
}

/// 某一行的快捷键文字（没有键位时是 None）。
fn shortcut_of(command: DocumentMenuCommand, cx: &mut VisualTestContext) -> Option<String> {
    cx.update(|_window, cx| document_menu_shortcut(command, cx))
        .map(|label| label.to_string())
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

fn select_head_of_first_block(editor: &Entity<Editor>, cx: &mut VisualTestContext) {
    let block = visible_block(editor, 0, cx);
    editor.update(cx, |editor, _cx| editor.focus_block(block.entity_id()));
    block.update(cx, |block, _cx| block.selected_range = 0..5);
    redraw(cx);
}

fn enabled_of(rows: &[(&'static str, bool)], name: &str) -> bool {
    rows.iter()
        .find(|(row, _)| *row == name)
        .unwrap_or_else(|| panic!("{name} 行不见了"))
        .1
}

fn menu_is_open(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> bool {
    editor.read_with(cx, |editor, _| editor.context_menu.is_some())
}

fn enabled_rows(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> Vec<(&'static str, bool)> {
    editor.read_with(cx, |editor, cx| {
        editor
            .document_menu_rows(cx)
            .into_iter()
            .filter_map(|row| match row {
                DocumentMenuRow::Item { name, enabled, .. } => Some((name, enabled)),
                _ => None,
            })
            .collect()
    })
}

fn submenu_enabled_rows(
    editor: &Entity<Editor>,
    submenu: DocumentSubmenu,
    cx: &mut VisualTestContext,
) -> Vec<(&'static str, bool)> {
    editor.read_with(cx, |editor, cx| {
        editor
            .document_submenu_rows(submenu, cx)
            .into_iter()
            .filter_map(|row| match row {
                DocumentMenuRow::Item { name, enabled, .. } => Some((name, enabled)),
                DocumentMenuRow::Submenu { .. } | DocumentMenuRow::Separator => None,
            })
            .collect()
    })
}

#[gpui::test]
async fn right_click_inside_a_block_opens_the_document_menu(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx
        .add_window_view(|_window, cx| Editor::from_markdown(cx, TWO_PARAGRAPHS.to_string(), None));
    redraw(cx);

    right_click(&editor, 0, cx);

    for name in MAIN_ROWS {
        let bounds = row_bounds(name, cx);
        assert!(f32::from(bounds.size.height) > 0.0, "{name} 这一行没有高度");
    }
    // 行的先后顺序就是菜单的分段顺序：撤销在最上，视图切换在最下。
    let undo_top = f32::from(row_bounds("undo", cx).top());
    let redo_top = f32::from(row_bounds("redo", cx).top());
    let paste_top = f32::from(row_bounds("paste", cx).top());
    let view_top = f32::from(row_bounds("toggle-source-view", cx).top());
    assert!(undo_top < redo_top && redo_top < paste_top && paste_top < view_top);
    assert!(menu_is_open(&editor, cx));
}

#[gpui::test]
async fn hovering_the_format_row_opens_its_submenu_beside_the_parent_row(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx
        .add_window_view(|_window, cx| Editor::from_markdown(cx, TWO_PARAGRAPHS.to_string(), None));
    redraw(cx);

    right_click(&editor, 0, cx);
    let parent = row_bounds("format", cx);
    let center = point(
        parent.left() + parent.size.width * 0.5,
        parent.top() + parent.size.height * 0.5,
    );
    cx.simulate_mouse_move(center, Option::<MouseButton>::None, Modifiers::none());
    redraw(cx);

    for name in FORMAT_ROWS {
        row_bounds(name, cx);
    }
    let submenu_left = f32::from(row_bounds("bold", cx).left());
    let panel_right = f32::from(row_bounds("undo", cx).right());
    assert!(
        submenu_left > panel_right,
        "二级面板该开在主面板右侧：主面板右边 {panel_right}，二级左边 {submenu_left}"
    );
    let padding = crate::theme::Theme::default_theme()
        .dimensions
        .menu_panel_padding;
    assert_eq!(
        f32::from(row_bounds("bold", cx).top()) - padding,
        f32::from(parent.top()),
        "二级面板的顶部该与父行对齐（行比面板顶低一个内边距）"
    );
    assert_eq!(
        editor.read_with(cx, |editor, _| match editor.context_menu.as_ref() {
            Some(crate::editor::context_menu::ContextMenuState::Document {
                open_submenu, ..
            }) => *open_submenu,
            _ => None,
        }),
        Some(DocumentSubmenu::Format)
    );
}

#[gpui::test]
async fn clicking_bold_in_the_submenu_edits_the_buffer_and_one_undo_restores_it(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    let (editor, cx) = cx
        .add_window_view(|_window, cx| Editor::from_markdown(cx, TWO_PARAGRAPHS.to_string(), None));
    redraw(cx);

    select_head_of_first_block(&editor, cx);
    right_click(&editor, 0, cx);
    editor.update(cx, |editor, cx| {
        editor.set_document_menu_hover(true, Some(DocumentSubmenu::Format), cx)
    });
    redraw(cx);

    click_row("bold", cx);
    assert_eq!(
        buffer_text(&editor, cx),
        "**alpha** one\n\nbeta two\n",
        "菜单里的「加粗」没写回缓冲区"
    );
    assert!(
        !menu_is_open(&editor, cx),
        "点完一行菜单该收起，实际状态 {:?}",
        editor.read_with(cx, |editor, _| {
            match editor.context_menu.as_ref() {
                Some(crate::editor::context_menu::ContextMenuState::Document {
                    position,
                    open_submenu,
                    ..
                }) => (f32::from(position.x), f32::from(position.y), *open_submenu),
                other => {
                    let _ = other;
                    (-1.0, -1.0, None)
                }
            }
        })
    );

    editor.update(cx, |editor, cx| editor.undo_document(cx));
    redraw(cx);
    assert_eq!(
        buffer_text(&editor, cx),
        TWO_PARAGRAPHS,
        "一次撤销该把菜单里这次改动整体退回"
    );
}

#[gpui::test]
async fn clicking_a_paragraph_row_turns_the_block_into_a_heading(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx
        .add_window_view(|_window, cx| Editor::from_markdown(cx, TWO_PARAGRAPHS.to_string(), None));
    redraw(cx);

    select_head_of_first_block(&editor, cx);
    right_click(&editor, 0, cx);
    editor.update(cx, |editor, cx| {
        editor.set_document_menu_hover(true, Some(DocumentSubmenu::Paragraph), cx)
    });
    redraw(cx);

    click_row("heading-2", cx);
    assert_eq!(
        buffer_text(&editor, cx),
        "## alpha one\n\nbeta two\n",
        "菜单里的「二级标题」没把这一段转成标题"
    );

    editor.update(cx, |editor, cx| editor.undo_document(cx));
    redraw(cx);
    assert_eq!(buffer_text(&editor, cx), TWO_PARAGRAPHS);
}

/// 段落菜单里的列表那三行与快捷键共用一条入口：写回的字节、撤销的步数都要对得上。
#[gpui::test]
async fn clicking_a_list_row_in_the_submenu_writes_the_marker(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx
        .add_window_view(|_window, cx| Editor::from_markdown(cx, TWO_PARAGRAPHS.to_string(), None));
    redraw(cx);

    select_head_of_first_block(&editor, cx);
    right_click(&editor, 0, cx);
    editor.update(cx, |editor, cx| {
        editor.set_document_menu_hover(true, Some(DocumentSubmenu::Paragraph), cx)
    });
    redraw(cx);

    click_row("task-list", cx);
    assert_eq!(
        buffer_text(&editor, cx),
        "- [ ] alpha one\n\nbeta two\n",
        "菜单里的「任务列表」没把这一段转成任务项"
    );

    editor.update(cx, |editor, cx| editor.undo_document(cx));
    redraw(cx);
    assert_eq!(
        buffer_text(&editor, cx),
        TWO_PARAGRAPHS,
        "一次撤销该整步退回"
    );
}

/// 已经是无序项时「无序列表」这一行还是可点的，点它是取消记号，不是没反应。
#[gpui::test]
async fn clicking_the_bullet_row_on_an_item_cancels_the_marker(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "- alpha one\n\nbeta two\n".to_string(), None)
    });
    redraw(cx);

    select_head_of_first_block(&editor, cx);
    right_click(&editor, 0, cx);
    editor.update(cx, |editor, cx| {
        editor.set_document_menu_hover(true, Some(DocumentSubmenu::Paragraph), cx)
    });
    redraw(cx);

    let paragraphs = submenu_enabled_rows(&editor, DocumentSubmenu::Paragraph, cx);
    assert!(
        enabled_of(&paragraphs, "bullet-list"),
        "这一行点下去是取消记号，不该置灰：{paragraphs:?}"
    );
    click_row("bullet-list", cx);
    assert_eq!(
        buffer_text(&editor, cx),
        "alpha one\n\nbeta two\n",
        "「无序列表」没把这一项退回正文"
    );
}

/// 「段落」档里的「代码块」这一行：点下去补一对围栏。退回正文那一半走同一条入口，
/// 但代码块上弹不出这套菜单（那条口径在 `a_right_click_on_a_code_block_does_not_open_the_document_menu`），
/// 所以那一步在这里按编辑器层入口验。
#[gpui::test]
async fn clicking_the_code_block_row_fences_the_block(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx
        .add_window_view(|_window, cx| Editor::from_markdown(cx, TWO_PARAGRAPHS.to_string(), None));
    redraw(cx);

    select_head_of_first_block(&editor, cx);
    right_click(&editor, 0, cx);
    editor.update(cx, |editor, cx| {
        editor.set_document_menu_hover(true, Some(DocumentSubmenu::Paragraph), cx)
    });
    redraw(cx);

    click_row("code-block", cx);
    assert_eq!(
        buffer_text(&editor, cx),
        "```\nalpha one\n```\n\nbeta two\n",
        "菜单里的「代码块」没把这一段包进围栏"
    );

    editor.update(cx, |editor, cx| {
        editor.apply_block_kind_to_selection(
            crate::editor::paragraph_ops::BlockKindTarget::CodeBlock,
            cx,
        )
    });
    redraw(cx);
    assert_eq!(
        buffer_text(&editor, cx),
        "alpha one\n\nbeta two\n",
        "同一处入口再点一次该退回正文，连那对围栏一起收掉"
    );
}

/// 段落菜单里的「引用」这一行：写回行记号，再点一次取消。
#[gpui::test]
async fn clicking_the_quote_row_wraps_and_unwraps_the_block(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx
        .add_window_view(|_window, cx| Editor::from_markdown(cx, TWO_PARAGRAPHS.to_string(), None));
    redraw(cx);

    select_head_of_first_block(&editor, cx);
    right_click(&editor, 0, cx);
    editor.update(cx, |editor, cx| {
        editor.set_document_menu_hover(true, Some(DocumentSubmenu::Paragraph), cx)
    });
    redraw(cx);

    click_row("quote", cx);
    assert_eq!(
        buffer_text(&editor, cx),
        "> alpha one\n\nbeta two\n",
        "菜单里的「引用」没把这一段包起来"
    );

    right_click(&editor, 0, cx);
    editor.update(cx, |editor, cx| {
        editor.set_document_menu_hover(true, Some(DocumentSubmenu::Paragraph), cx)
    });
    redraw(cx);
    let paragraphs = submenu_enabled_rows(&editor, DocumentSubmenu::Paragraph, cx);
    assert!(
        enabled_of(&paragraphs, "quote"),
        "这一行点下去是取消引用，不该置灰：{paragraphs:?}"
    );
    click_row("quote", cx);
    assert_eq!(
        buffer_text(&editor, cx),
        TWO_PARAGRAPHS,
        "已经在引用里再点一次该取消引用"
    );

    editor.update(cx, |editor, cx| editor.undo_document(cx));
    redraw(cx);
    assert_eq!(
        buffer_text(&editor, cx),
        "> alpha one\n\nbeta two\n",
        "一步撤销该退回上一次"
    );
}

/// 没有选区时剪切/拷贝/格式那些项做不了，但行要留在原位：藏起来会让菜单高度跳，
/// 用户也看不出「这一项存在，只是现在不能点」。
#[gpui::test]
async fn rows_that_cannot_run_stay_in_place_but_greyed(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx
        .add_window_view(|_window, cx| Editor::from_markdown(cx, TWO_PARAGRAPHS.to_string(), None));
    redraw(cx);

    right_click(&editor, 0, cx);
    let without_selection = enabled_rows(&editor, cx);
    let geometry_without = MAIN_ROWS
        .iter()
        .map(|name| f32::from(row_bounds(name, cx).top()))
        .collect::<Vec<_>>();
    for name in ["cut", "copy"] {
        assert!(
            !enabled_of(&without_selection, name),
            "{name} 在无选区时还是可点的：{without_selection:?}"
        );
    }

    select_head_of_first_block(&editor, cx);
    right_click(&editor, 0, cx);
    let with_selection = enabled_rows(&editor, cx);
    for name in ["cut", "copy"] {
        assert!(
            enabled_of(&with_selection, name),
            "{name} 在有选区时还置着：{with_selection:?}"
        );
    }
    // 撤销/重做看历史、粘贴看剪贴板，这三行不随选区变。
    for name in ["undo", "redo", "paste"] {
        assert_eq!(
            enabled_of(&with_selection, name),
            enabled_of(&without_selection, name),
            "{name} 这一行的可用性不该跟着选区变"
        );
    }
    let geometry_with = MAIN_ROWS
        .iter()
        .map(|name| f32::from(row_bounds(name, cx).top()))
        .collect::<Vec<_>>();
    assert_eq!(
        geometry_without, geometry_with,
        "置灰只是变色，行序与行高不能变"
    );

    // 二级面板里同样的口径：格式那九行（八种行内样式与链接）都跟着选区走。
    let formats = submenu_enabled_rows(&editor, DocumentSubmenu::Format, cx);
    assert_eq!(formats.len(), FORMAT_ROWS.len());
    assert!(formats.iter().all(|(_, enabled)| *enabled));
    // 「段落」那一档看的是「这一块换得动吗」：这一段本来就是正文，「正文」这一行点不动，
    // 标题与列表那几行仍然能换。
    let paragraphs = submenu_enabled_rows(&editor, DocumentSubmenu::Paragraph, cx);
    assert_eq!(
        paragraphs.len(),
        PARAGRAPH_ROWS.len(),
        "段落那一档的行数变了，测试里的行名清单要跟着补"
    );
    assert!(
        !enabled_of(&paragraphs, "normal-text"),
        "光标已经在正文里，这一行还置着才对：{paragraphs:?}"
    );
    assert!(
        paragraphs
            .iter()
            .filter(|(name, _)| *name != "normal-text")
            .all(|(_, enabled)| *enabled),
        "标题与列表那几行都该换得动：{paragraphs:?}"
    );
}

/// 菜单只能从「可插入表格的块」上弹出：代码块与表格单元格仍然不给这个菜单，
/// 这条口径在扩菜单之前就有，不能被顺手放宽。
#[gpui::test]
async fn a_right_click_on_a_code_block_does_not_open_the_document_menu(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "alpha one\n\n```\ncode\n```\n".to_string(), None)
    });
    redraw(cx);

    let code = visible_block(&editor, 1, cx);
    code.read_with(cx, |block, _cx| {
        assert!(matches!(
            block.kind(),
            crate::components::BlockKind::CodeBlock { .. }
        ))
    });
    right_click(&editor, 1, cx);
    assert!(!menu_is_open(&editor, cx), "代码块上弹不出正文菜单");

    right_click(&editor, 0, cx);
    assert!(menu_is_open(&editor, cx), "段落上还是该能弹");
}

/// 贴边右键：菜单得整个留在视口里，不能有一半跑到窗口外面。
#[gpui::test]
async fn the_menu_is_pulled_back_inside_the_viewport(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    // 段落多一些，正文铺满小视口，右下角那一点必定落在编辑区里。
    let text = (1..=10)
        .map(|index| format!("第 {index} 段正文\n"))
        .collect::<Vec<_>>()
        .join("\n");
    let (editor, cx) = cx.add_window_view(|_window, cx| Editor::from_markdown(cx, text, None));
    redraw(cx);

    cx.simulate_resize(Size {
        width: px(420.0),
        height: px(600.0),
    });
    redraw(cx);
    let viewport = cx.update(|window, _cx| window.viewport_size());
    // 最后一段的中心已经在视口下沿附近：菜单原样摆会越界，落点得往回收。
    let position = block_center(&editor, 9, cx);
    assert!(
        f32::from(position.y) + 100.0 > f32::from(viewport.height),
        "测试前提：这一段该靠近视口下沿，实测 y {:?}",
        f32::from(position.y)
    );
    right_click_at(position, cx);
    assert!(menu_is_open(&editor, cx), "靠下靠右的右键该弹出菜单");

    let last = row_bounds("toggle-source-view", cx);
    let viewport_height = f32::from(viewport.height);
    let viewport_width = f32::from(viewport.width);
    assert!(
        f32::from(last.bottom()) <= viewport_height,
        "最后一行压到了视口下沿之外：{} > {viewport_height}",
        f32::from(last.bottom())
    );
    assert!(
        f32::from(row_bounds("undo", cx).right()) <= viewport_width,
        "主面板越过了视口右沿"
    );

    // 二级面板同样：在主面板右侧放不下时改贴左侧。
    editor.update(cx, |editor, cx| {
        editor.set_document_menu_hover(true, Some(DocumentSubmenu::Paragraph), cx)
    });
    redraw(cx);
    let submenu_left = f32::from(row_bounds("heading-1", cx).left());
    let panel_left = f32::from(row_bounds("undo", cx).left());
    assert!(
        submenu_left < panel_left,
        "窄视口里二级面板该翻到主面板左侧：{submenu_left} vs {panel_left}"
    );
}

/// 「格式」那一档十行的快捷键那一列都要显示出来，其中标记文本与清除格式这两行是
/// FP9b 才补上键位的；段落与插入那一档还没有键位，留空但不换行宽。
#[gpui::test]
async fn rows_show_their_shortcut_column_when_a_binding_exists(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx
        .add_window_view(|_window, cx| Editor::from_markdown(cx, TWO_PARAGRAPHS.to_string(), None));
    redraw(cx);

    right_click(&editor, 0, cx);
    editor.update(cx, |editor, cx| {
        editor.set_document_menu_hover(true, Some(DocumentSubmenu::Format), cx)
    });
    redraw(cx);

    for format in [
        InlineFormat::Bold,
        InlineFormat::Italic,
        InlineFormat::Underline,
        InlineFormat::Strikethrough,
        InlineFormat::Code,
        InlineFormat::Highlight,
        InlineFormat::Superscript,
        InlineFormat::Subscript,
    ] {
        let label = shortcut_of(DocumentMenuCommand::Format(format), cx)
            .unwrap_or_else(|| panic!("{format:?} 这一行该有默认键位"));
        assert!(
            cx.debug_bounds(shortcut_selector(&label)).is_some(),
            "{label} 这一列没渲染出来"
        );
    }
    assert_eq!(
        shortcut_of(DocumentMenuCommand::Format(InlineFormat::Highlight), cx).as_deref(),
        Some("⌘⇧H"),
        "标记文本的键位要与方案里写的那一条一致"
    );
    assert_eq!(
        shortcut_of(DocumentMenuCommand::ClearFormat, cx).as_deref(),
        Some("⌘\\"),
        "清除格式的键位要与方案里写的那一条一致"
    );

    // 段落与插入那一档刻意不给键位：那一列留空，行序与行高都不受影响。
    for command in [
        DocumentMenuCommand::Heading(2),
        DocumentMenuCommand::NormalText,
        DocumentMenuCommand::BulletList,
        DocumentMenuCommand::Quote,
        DocumentMenuCommand::CodeBlock,
        DocumentMenuCommand::InsertTable,
        DocumentMenuCommand::InsertCodeBlock,
    ] {
        assert_eq!(
            shortcut_of(command, cx),
            None,
            "{command:?} 这一行还没有键位，不该凭空造一个"
        );
    }

    // 有键位的主菜单行也一样显示。
    for command in [
        DocumentMenuCommand::Undo,
        DocumentMenuCommand::Copy,
        DocumentMenuCommand::Paste,
        DocumentMenuCommand::SelectAll,
        DocumentMenuCommand::ToggleSourceView,
    ] {
        let label = shortcut_of(command, cx).expect("这几行都有默认键位");
        assert!(
            cx.debug_bounds(shortcut_selector(&label)).is_some(),
            "{label} 这一列没渲染出来"
        );
    }
    assert_eq!(
        shortcut_of(DocumentMenuCommand::SelectAll, cx).as_deref(),
        Some("⌘A"),
        "「全选」那一列显示的是与 ⌘A 同一条键位"
    );
    // 「拷贝为 HTML」的 ⌘⇧C 是写死的一份绑定，不在键位表里，这一列留空。
    assert_eq!(
        shortcut_of(DocumentMenuCommand::CopyAsHtml, cx),
        None,
        "没有表内键位的行不该凭空造一个"
    );
}

/// 「切换源码模式」这一行派发的是与 `⌘/` 同一个动作，两条路径不能各自长出一份行为。
#[gpui::test]
async fn the_view_row_toggles_the_same_mode_as_the_shortcut(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx
        .add_window_view(|_window, cx| Editor::from_markdown(cx, TWO_PARAGRAPHS.to_string(), None));
    redraw(cx);

    right_click(&editor, 0, cx);
    click_row("toggle-source-view", cx);
    let mode = editor.read_with(cx, |editor, _| editor.view_mode);
    assert_eq!(mode, ViewMode::Source, "菜单里那一行走的不是切换视图的动作");

    // 源码模式里没有渲染块可围绕，右键仍然弹不出这套菜单。
    right_click(&editor, 0, cx);
    assert!(!menu_is_open(&editor, cx), "源码模式不该弹正文菜单");
}

/// 右键不该把已选中的那段弄丢：菜单是围绕选区操作的。
#[gpui::test]
async fn right_click_keeps_the_selection_it_opened_with(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx
        .add_window_view(|_window, cx| Editor::from_markdown(cx, TWO_PARAGRAPHS.to_string(), None));
    redraw(cx);

    select_head_of_first_block(&editor, cx);
    right_click(&editor, 0, cx);

    let selected =
        visible_block(&editor, 0, cx).read_with(cx, |block, _cx| block.selected_range.clone());
    assert_eq!(selected, 0..5, "右键把块内选区改成了光标");
    assert_eq!(
        editor.read_with(cx, |editor, _| editor.active_entity_id),
        Some(visible_block(&editor, 0, cx).entity_id()),
        "右键把编辑目标焦点丢了"
    );

    // 空选区（只有光标）时格式那八行置灰，插入表格仍然可点。
    let block = visible_block(&editor, 0, cx);
    block.update(cx, |block, _cx| block.selected_range = 2..2);
    right_click(&editor, 0, cx);
    let formats = submenu_enabled_rows(&editor, DocumentSubmenu::Format, cx);
    assert!(
        formats.iter().all(|(_, enabled)| !*enabled),
        "只有光标时格式项该置灰：{formats:?}"
    );
    assert_eq!(
        buffer_text(&editor, cx),
        TWO_PARAGRAPHS,
        "只是弹菜单不该改字节"
    );
}

/// 偏好页改过键位之后，菜单那一列写的是改成的那颗键，而不是默认键：
/// 显示的那一份与真正绑上去的那一份同源（`install_keybindings` 一处写下）。
#[gpui::test]
async fn the_shortcut_column_shows_the_users_own_binding(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let mut config = std::collections::BTreeMap::new();
    config.insert("bold_selection".to_string(), vec!["cmd-alt-b".to_string()]);
    cx.update(|cx| install_keybindings(cx, &config));
    let (editor, cx) = cx
        .add_window_view(|_window, cx| Editor::from_markdown(cx, TWO_PARAGRAPHS.to_string(), None));
    redraw(cx);

    right_click(&editor, 0, cx);
    editor.update(cx, |editor, cx| {
        editor.set_document_menu_hover(true, Some(DocumentSubmenu::Format), cx)
    });
    redraw(cx);

    let label = shortcut_of(DocumentMenuCommand::Format(InlineFormat::Bold), cx)
        .expect("加粗这一行总有键位可显示");
    assert_eq!(label, "⌥⌘B", "菜单那一列要写用户自己定的那颗键");
    assert!(
        cx.debug_bounds(shortcut_selector("⌥⌘B")).is_some(),
        "改过的键位没渲染进菜单那一列"
    );
    assert!(
        cx.debug_bounds(shortcut_selector("⌘B")).is_none(),
        "默认键 ⌘B 不该还挂在屏幕上"
    );
}
