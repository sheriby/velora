//! 命令面板新增的两组（「编辑」两条与「格式」十条）逐条从面板执行一遍、比缓冲区。
//!
//! 面板的输入框把窗口焦点借走之后，块那一层的处理者不在派发路径里，这一族命令因此在
//! 编辑器层收口（判定见 `Editor::block_focus_is_live`）；从面板执行正好把这条收口走满。
//! 系统菜单栏那五组的可达性另有 `window_menu::every_menu_bar_command_has_a_handler`
//! 用菜单启用判定把守，同一个判定在这里用不上：实测 `Newline`、`BoldSelection` 都报
//! 不可用，而同一窗口里真的派发却写得出字节。

use super::common::*;
use crate::commands::commands;
use crate::components::Block;
use gpui::{ClipboardItem, Entity, VisualTestContext};

const DOC: &str = "alpha one\n\nbeta two\n";
const STYLED: &str = "alpha **one** beta\n";
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

fn select(
    editor: &Entity<Editor>,
    index: usize,
    range: std::ops::Range<usize>,
    cx: &mut VisualTestContext,
) {
    let block = visible_block(editor, index, cx);
    editor.update(cx, |editor, _cx| editor.focus_block(block.entity_id()));
    block.update(cx, |block, _cx| block.selected_range = range);
    redraw(cx);
}

fn set_clipboard(text: &str, cx: &mut VisualTestContext) {
    let text = text.to_string();
    cx.update(|_window, cx| cx.write_to_clipboard(ClipboardItem::new_string(text)));
}

fn clipboard_text(cx: &mut VisualTestContext) -> Option<String> {
    cx.update(|_window, cx| cx.read_from_clipboard().and_then(|item| item.text()))
}

fn open_editor<'a>(
    text: &'static str,
    cx: &'a mut TestAppContext,
) -> (Entity<Editor>, &'a mut VisualTestContext) {
    cx.add_window_view(|_window, cx| Editor::from_markdown(cx, text.to_string(), None))
}

/// 打开面板：先激活窗口（浮层要接住键盘），再走与 ⇧⌘P 同一条入口。
fn open_palette(editor: &Entity<Editor>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        window.activate_window();
        editor.update(cx, |editor, cx| editor.toggle_command_palette(window, cx));
    });
    redraw(cx);
}

/// 注册表里这条命令排第几：面板没输入查询时列的就是这份顺序，
/// 按下标走到那一条再回车，用例不依赖界面语言。
fn registry_index(id: &str) -> usize {
    commands()
        .iter()
        .position(|spec| spec.id == id)
        .unwrap_or_else(|| panic!("命令注册表里没有 {id}"))
}

#[gpui::test]
async fn the_palette_runs_the_inline_format_commands(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    for (id, document, selection, expected) in [
        (
            "bold_selection",
            DOC,
            0..5usize,
            "**alpha** one\n\nbeta two\n",
        ),
        ("italic_selection", DOC, 0..5, "*alpha* one\n\nbeta two\n"),
        (
            "underline_selection",
            DOC,
            0..5,
            "<u>alpha</u> one\n\nbeta two\n",
        ),
        (
            "strikethrough_selection",
            DOC,
            0..5,
            "~~alpha~~ one\n\nbeta two\n",
        ),
        ("code_selection", DOC, 0..5, "`alpha` one\n\nbeta two\n"),
        (
            "highlight_selection",
            DOC,
            0..5,
            "==alpha== one\n\nbeta two\n",
        ),
        (
            "superscript_selection",
            DOC,
            0..5,
            "<sup>alpha</sup> one\n\nbeta two\n",
        ),
        (
            "subscript_selection",
            DOC,
            0..5,
            "<sub>alpha</sub> one\n\nbeta two\n",
        ),
        ("link_selection", DOC, 0..5, "[alpha]() one\n\nbeta two\n"),
        ("clear_format_selection", STYLED, 6..9, "alpha one beta\n"),
    ] {
        let (editor, cx) = open_editor(document, cx);
        redraw(cx);
        select(&editor, 0, selection.clone(), cx);

        open_palette(&editor, cx);
        for _ in 0..registry_index(id) {
            cx.simulate_keystrokes("down");
        }
        cx.simulate_keystrokes("return");
        redraw(cx);

        assert_eq!(
            buffer_text(&editor, cx),
            expected,
            "{id} 从命令面板执行之后缓冲区不对"
        );
        assert!(
            editor.read_with(cx, |editor, _| editor.command_palette.is_none()),
            "{id} 执行完面板该收起"
        );
    }
}

#[gpui::test]
async fn the_palette_runs_the_clipboard_commands(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    // 「粘贴为纯文本」：剪贴板里只有一个网址时，落的还是那几个字，
    // 不写成「选中文字」那一条链接。选区是字节偏移。
    let (editor, cx) = open_editor("选中文字\n\n别段\n", cx);
    redraw(cx);
    set_clipboard(URL, cx);
    select(&editor, 0, 0..12, cx);
    open_palette(&editor, cx);
    for _ in 0..registry_index("paste_as_plain_text") {
        cx.simulate_keystrokes("down");
    }
    cx.simulate_keystrokes("return");
    redraw(cx);
    assert_eq!(
        buffer_text(&editor, cx),
        "https://example.test\n\n别段\n",
        "「粘贴为纯文本」从面板执行要对齐键位那条"
    );

    // 「复制为 Markdown」：没有选区时给整篇源码，文件字节不动。
    let (editor, cx) = open_editor(MARKDOWN_DOC, cx);
    redraw(cx);
    open_palette(&editor, cx);
    for _ in 0..registry_index("copy_as_markdown") {
        cx.simulate_keystrokes("down");
    }
    cx.simulate_keystrokes("return");
    redraw(cx);
    assert_eq!(
        clipboard_text(cx).as_deref(),
        Some(MARKDOWN_DOC),
        "剪贴板里要带 `**`，那是文件里的那几个字节"
    );
    assert_eq!(buffer_text(&editor, cx), MARKDOWN_DOC, "复制不该改文档");
}
/// 面板上一条的标签随界面语言变，用例取当下那份标签当查询词，不写死中文或英文。
fn command_label(id: &str, cx: &mut VisualTestContext) -> String {
    let spec = commands()
        .iter()
        .find(|spec| spec.id == id)
        .unwrap_or_else(|| panic!("命令注册表里没有 {id}"));
    cx.update(|_window, cx| spec.label(cx.global::<crate::i18n::I18nManager>().strings()))
}

/// 打字筛出那一条再回车：执行的是筛后列表的第 0 行，不是注册表原顺序的那一行。
#[gpui::test]
async fn typing_then_enter_runs_the_row_the_query_left(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = open_editor(DOC, cx);
    redraw(cx);
    select(&editor, 0, 0..5, cx);
    open_palette(&editor, cx);
    let label = command_label("bold_selection", cx);
    cx.simulate_input(&label);
    cx.simulate_keystrokes("return");
    redraw(cx);
    assert_eq!(
        buffer_text(&editor, cx),
        "**alpha** one\n\nbeta two\n",
        "查询筛出「{label}」之后回车，该执行这一条"
    );
    assert!(
        editor.read_with(cx, |editor, _| editor.command_palette.is_none()),
        "执行完面板该收起"
    );
}

/// 查询没命中时回车：不执行、不收场，留着让人改查询词。
#[gpui::test]
async fn enter_with_no_matching_row_runs_nothing(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = open_editor(DOC, cx);
    redraw(cx);
    select(&editor, 0, 0..5, cx);
    open_palette(&editor, cx);
    cx.simulate_input("不可能命中任何一条的查询zzz");
    cx.simulate_keystrokes("return");
    redraw(cx);
    assert_eq!(buffer_text(&editor, cx), DOC, "没有命中就不该动文档");
    assert!(
        editor.read_with(cx, |editor, _| editor.command_palette.is_some()),
        "没执行成，面板该留着"
    );
}
