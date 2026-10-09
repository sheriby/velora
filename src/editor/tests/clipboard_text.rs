//! 「粘贴为纯文本」与「复制为 Markdown」：与「粘贴」「复制」并排的两条入口，
//! 差在内容来源——前者只用剪贴板的文本味道，后者取文件里的那几个字节而不是渲染后的字。
//!
//! 用例钉四件事：两条入口各自与对照那条的差异、剪贴板里到底进了什么、一步撤销回到原样、
//! 菜单里的两行与键位走的是同一个动作。

use super::common::*;
use crate::components::{Block, Copy, CopyAsMarkdown, Paste, PasteAsPlainText};
use gpui::{ClipboardItem, Entity, Modifiers, MouseButton, point};

const URL_DOC: &str = "选中文字\n\n别段\n";
const MARKDOWN_DOC: &str = "前 **加粗** 后\n\n另一段\n";
const URL: &str = "https://example.test";

#[gpui::test]
async fn copying_selected_code_uses_only_the_body_and_keeps_literal_backticks(
    cx: &mut TestAppContext,
) {
    // 代码正文全选走编辑器选区后，普通复制错误地把源码围栏和语言名也交给剪贴板。
    // 必须复制可见代码正文；显式复制为 Markdown 则继续取原始源码选区。
    init_editor_test_app(cx);
    for (fence, language, body) in [
        (
            "```",
            "bash",
            "cargo build\ncargo run                       # 空窗口启动\ncargo run /路径/到/工作区        # 以指定文件夹为工作区打开\ncargo test",
        ),
        (
            "````",
            "text",
            "    keep indentation\nliteral ``` stays\n中文 🦀",
        ),
    ] {
        let markdown = format!("前文\n\n{fence}{language}\n{body}\n{fence}\n\n后文");
        let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, markdown, None));
        redraw(cx);
        focus_block_at(&editor, 1, cx);
        cx.simulate_keystrokes(if cfg!(target_os = "macos") {
            "cmd-a"
        } else {
            "ctrl-a"
        });
        cx.dispatch_action(Copy);
        assert_eq!(
            clipboard_text(cx).as_deref(),
            Some(body),
            "代码全选复制不应加入围栏或语言名"
        );
        editor.update(cx, |editor, cx| {
            let code = editor.document.visible_blocks()[1].entity.entity_id();
            editor.cross_block_selection = Some(crate::editor::CrossBlockSelection {
                anchor: crate::editor::CrossBlockSelectionEndpoint {
                    entity_id: code,
                    offset: 0,
                },
                focus: crate::editor::CrossBlockSelectionEndpoint {
                    entity_id: code,
                    offset: body.len(),
                },
            });
            cx.notify();
        });
        cx.dispatch_action(Copy);
        assert_eq!(
            clipboard_text(cx).as_deref(),
            Some(body),
            "鼠标式选区复制代码也不应加入围栏"
        );
        if language == "bash" {
            cx.dispatch_action(CopyAsMarkdown);
            assert_eq!(
                clipboard_text(cx),
                Some(format!("{fence}{language}\n{body}")),
                "显式复制为 Markdown 仍应保留选区中的源码记号"
            );
        }
        let start = body.find("\n").expect("代码有多行") + 1;
        editor.update(cx, |editor, cx| {
            let code = editor.document.visible_blocks()[1].entity.entity_id();
            editor.cross_block_selection = Some(crate::editor::CrossBlockSelection {
                anchor: crate::editor::CrossBlockSelectionEndpoint {
                    entity_id: code,
                    offset: body.len(),
                },
                focus: crate::editor::CrossBlockSelectionEndpoint {
                    entity_id: code,
                    offset: start,
                },
            });
            cx.notify();
        });
        cx.dispatch_action(Copy);
        assert_eq!(
            clipboard_text(cx).as_deref(),
            body.get(start..),
            "反向选中部分代码也应原样复制"
        );
    }
}

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

/// 顶部图标条那一颗的边界与点击（剪切/复制/粘贴/粘贴为纯文本已经不在长行里了）。
fn quick_action_bounds(
    name: &'static str,
    cx: &mut VisualTestContext,
) -> gpui::Bounds<gpui::Pixels> {
    let selector: &'static str = Box::leak(format!("menu-quick-action-{name}").into_boxed_str());
    cx.debug_bounds(selector)
        .unwrap_or_else(|| panic!("图标条里没渲染出 {name} 那一颗"))
}

fn click_quick_action(name: &'static str, cx: &mut VisualTestContext) {
    let bounds = quick_action_bounds(name, cx);
    let center = point(
        bounds.left() + bounds.size.width * 0.5,
        bounds.top() + bounds.size.height * 0.5,
    );
    cx.simulate_click(center, Modifiers::none());
    redraw(cx);
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
        "没有选区时与「复制为 HTML」同一条口径：整篇源码进剪贴板"
    );
}

#[gpui::test]
async fn the_copy_as_markdown_row_in_the_menu_reaches_the_same_action(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = open_editor(MARKDOWN_DOC, cx);
    redraw(cx);

    select(&editor, 0, 0..14, cx);
    right_click(&editor, 0, cx);
    row_bounds("copy-as-markdown", cx);
    for name in ["cut", "copy", "paste", "paste-as-plain-text"] {
        quick_action_bounds(name, cx);
    }

    click_row("copy-as-markdown", cx);
    assert_eq!(
        clipboard_text(cx).as_deref(),
        Some("前 **加粗** 后"),
        "菜单里那一行的「复制为 Markdown」要与键位同一条"
    );
    assert_eq!(
        buffer_text(&editor, cx),
        MARKDOWN_DOC,
        "复制只动剪贴板，文档一个字节都不该改"
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
    click_quick_action("paste-as-plain-text", cx);

    assert_eq!(
        buffer_text(&editor, cx),
        "纯文本 **记号**\n\n别段\n",
        "菜单里那一行的「粘贴为纯文本」要与键位同一条：落的还是那几个字"
    );
}

/// 同一个窗口里连着走两轮「选区 → 右键 → 点菜单行」（方案 §7 的 R7）：每一轮的落点
/// 都按当轮生效的那份选区，第二轮不沿用第一轮的位置。选区要按块当前那份屏幕文本取——
/// 同一块在两种写法下屏幕文本的字节数不同（R8），写死一个数会让第二轮只盖住半句，
/// 看着像落点错了，其实是选区本来就只到那儿。
#[gpui::test]
async fn two_menu_rounds_land_on_the_selection_in_effect(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = open_editor(MARKDOWN_DOC, cx);
    redraw(cx);
    set_clipboard(URL, cx);

    for round in 0..2 {
        let visible_len =
            visible_block(&editor, 0, cx).read_with(cx, |block, _| block.visible_len());
        select(&editor, 0, 0..visible_len, cx);
        right_click(&editor, 0, cx);
        click_quick_action("paste-as-plain-text", cx);
        assert_eq!(
            buffer_text(&editor, cx),
            format!("{URL}\n\n另一段\n"),
            "第 {round} 轮把整块选中再点「粘贴为纯文本」，落的该只是那个网址"
        );
    }
}
