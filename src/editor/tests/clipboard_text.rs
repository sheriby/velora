//! 「粘贴为纯文本」与「拷贝为 Markdown」：与「粘贴」「拷贝」并排的两条入口，
//! 差在内容来源——前者只用剪贴板的文本味道，后者取文件里的那几个字节而不是渲染后的字。
//!
//! 用例钉四件事：两条入口各自与对照那条的差异、剪贴板里到底进了什么、一步撤销回到原样、
//! 菜单里的两行与键位走的是同一个动作。

use super::common::*;
use crate::components::{Block, CopyAsMarkdown, Paste, PasteAsPlainText};
use gpui::{ClipboardItem, Entity, Modifiers, MouseButton, point};

const URL_DOC: &str = "选中文字\n\n别段\n";
const MARKDOWN_DOC: &str = "前 **加粗** 后\n\n另一段\n";
const URL: &str = "https://example.test";

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

fn put_caret(editor: &Entity<Editor>, index: usize, offset: usize, cx: &mut VisualTestContext) {
    select(editor, index, offset..offset, cx);
}

fn set_clipboard(text: &str, cx: &mut VisualTestContext) {
    let text = text.to_string();
    cx.update(|_window, cx| cx.write_to_clipboard(ClipboardItem::new_string(text)));
}

fn clipboard_text(cx: &mut VisualTestContext) -> Option<String> {
    cx.update(|_window, cx| cx.read_from_clipboard().and_then(|item| item.text()))
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

#[gpui::test]
async fn pasting_a_url_over_a_selection_builds_a_link(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = open_editor(URL_DOC, cx);
    redraw(cx);

    // 「选中文字」= 四个汉字 12 个字节，整段选中。
    select(&editor, 0, 0..12, cx);
    set_clipboard(URL, cx);
    cx.dispatch_action(Paste);
    redraw(cx);

    assert_eq!(
        buffer_text(&editor, cx),
        format!("[选中文字]({URL})\n\n别段\n"),
        "对照那条：选中文字后粘贴网址要包成链接（roadmap B5）"
    );
}

#[gpui::test]
async fn paste_as_plain_text_keeps_the_url_as_its_own_characters(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = open_editor(URL_DOC, cx);
    redraw(cx);

    select(&editor, 0, 0..12, cx);
    set_clipboard(URL, cx);
    cx.dispatch_action(PasteAsPlainText);
    redraw(cx);

    assert_eq!(
        buffer_text(&editor, cx),
        format!("{URL}\n\n别段\n"),
        "「粘贴为纯文本」不该把网址改写成链接，也不该动别的段"
    );

    undo(&editor, cx);
    assert_eq!(
        buffer_text(&editor, cx),
        URL_DOC,
        "一步撤销要放回原来那四个字"
    );
}

#[gpui::test]
async fn copy_as_markdown_takes_the_selection_source(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = open_editor(MARKDOWN_DOC, cx);
    redraw(cx);

    // 屏幕上的这一行是「前 加粗 后」，14 个字节；源码里那对 `**` 不进可见文本。
    select(&editor, 0, 0..14, cx);
    cx.dispatch_action(CopyAsMarkdown);
    redraw(cx);

    assert_eq!(
        clipboard_text(cx).as_deref(),
        Some("前 **加粗** 后"),
        "剪贴板里该是文件里的那几个字节，不是渲染后的字"
    );
}

#[gpui::test]
async fn copy_as_markdown_without_a_selection_takes_the_whole_document(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = open_editor(MARKDOWN_DOC, cx);
    redraw(cx);

    put_caret(&editor, 0, 0, cx);
    cx.dispatch_action(CopyAsMarkdown);
    redraw(cx);

    assert_eq!(
        clipboard_text(cx).as_deref(),
        Some(MARKDOWN_DOC),
        "没有选区时与「拷贝为 HTML」同一条口径：整篇源码进剪贴板"
    );
}

#[gpui::test]
async fn the_copy_as_markdown_row_in_the_menu_reaches_the_same_action(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = open_editor(MARKDOWN_DOC, cx);
    redraw(cx);

    select(&editor, 0, 0..14, cx);
    right_click(&editor, 0, cx);
    for name in [
        "cut",
        "copy",
        "paste",
        "paste-as-plain-text",
        "copy-as-markdown",
    ] {
        row_bounds(name, cx);
    }

    click_row("copy-as-markdown", cx);
    assert_eq!(
        clipboard_text(cx).as_deref(),
        Some("前 **加粗** 后"),
        "菜单里那一行的「拷贝为 Markdown」要与键位同一条"
    );
    assert_eq!(
        buffer_text(&editor, cx),
        MARKDOWN_DOC,
        "拷贝只动剪贴板，文档一个字节都不该改"
    );
}

#[gpui::test]
async fn the_paste_as_plain_text_row_in_the_menu_reaches_the_same_action(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = open_editor(URL_DOC, cx);
    redraw(cx);

    set_clipboard("纯文本 **记号**", cx);
    select(&editor, 0, 0..12, cx);
    right_click(&editor, 0, cx);
    click_row("paste-as-plain-text", cx);

    assert_eq!(
        buffer_text(&editor, cx),
        "纯文本 **记号**\n\n别段\n",
        "菜单里那一行的「粘贴为纯文本」要与键位同一条：落的还是那几个字"
    );
}
