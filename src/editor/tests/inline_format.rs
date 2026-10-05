//! 行内格式（加粗/斜体/下划线/删除线/行内代码/上标/下标/标记文本）在**编辑器层**的那一条入口。
//!
//! 选区横跨多个块时，行内样式只能成立在自己的块里（markdown 的行内语法本来就不跨块），
//! 所以这条入口按可见块切成几段分别处理，并且只开一个撤销组。快捷键、后面的选中工具栏
//! 和右键菜单共用它，三者行为必须一致。

use super::common::*;
use crate::components::{BoldSelection, InlineFormat};
use gpui::Entity;

const TWO_PARAGRAPHS: &str = "alpha one\n\nbeta two\n";
const THREE_PARAGRAPHS: &str = "alpha one\n\nbeta two\n\ngamma three\n";

fn visible_block(
    editor: &Entity<Editor>,
    index: usize,
    cx: &mut VisualTestContext,
) -> Entity<Block> {
    editor.read_with(cx, |editor, _cx| {
        editor.document.visible_blocks()[index].entity.clone()
    })
}

/// 让某个块成为编辑目标：走真实的 `focus_block` 通道，再抽一帧把焦点落进渲染树。
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
    visible_block(editor, index, cx).update(cx, |block, _cx| block.selected_range = range);
}

fn buffer_text(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |editor, _cx| editor.buffer.text())
}

fn toggle(editor: &Entity<Editor>, format: InlineFormat, cx: &mut VisualTestContext) -> bool {
    editor.update(cx, |editor, cx| {
        editor.toggle_inline_format_on_selection(format, cx)
    })
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

#[gpui::test]
async fn bold_on_a_single_block_selection_writes_markers_into_the_buffer(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx
        .add_window_view(|_window, cx| Editor::from_markdown(cx, TWO_PARAGRAPHS.to_string(), None));
    redraw(cx);

    focus_block_at(&editor, 0, cx);
    select(&editor, 0, 0..5, cx);
    cx.dispatch_action(BoldSelection);
    redraw(cx);

    assert_eq!(
        buffer_text(&editor, cx),
        "**alpha** one\n\nbeta two\n",
        "加粗没写回缓冲区，或者顺带改动了选区之外的字节"
    );
}

#[gpui::test]
async fn toggling_bold_twice_gives_the_original_bytes_back(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "alpha\n\nbeta two\n".to_string(), None)
    });
    redraw(cx);

    let whole = visible_block(&editor, 0, cx).read_with(cx, |block, _cx| block.visible_len());
    focus_block_at(&editor, 0, cx);
    select(&editor, 0, 0..whole, cx);
    cx.dispatch_action(BoldSelection);
    redraw(cx);
    let formatted = buffer_text(&editor, cx);
    assert!(
        formatted.starts_with("**alpha**"),
        "第一次没加上标记：{formatted:?}"
    );

    let whole = visible_block(&editor, 0, cx).read_with(cx, |block, _cx| block.visible_len());
    select(&editor, 0, 0..whole, cx);
    cx.dispatch_action(BoldSelection);
    redraw(cx);
    assert_eq!(
        buffer_text(&editor, cx),
        "alpha\n\nbeta two\n",
        "同一段加粗文字再按一次应当取消加粗，逐字节回到原样"
    );
}

#[gpui::test]
async fn each_inline_format_writes_its_own_markers(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    for (label, format, expected) in [
        ("删除线", InlineFormat::Strikethrough, "~~alpha~~ one"),
        ("上标", InlineFormat::Superscript, "<sup>alpha</sup> one"),
        ("下标", InlineFormat::Subscript, "<sub>alpha</sub> one"),
        ("标记文本", InlineFormat::Highlight, "==alpha== one"),
    ] {
        let (editor, cx) = cx.add_window_view(|_window, cx| {
            Editor::from_markdown(cx, TWO_PARAGRAPHS.to_string(), None)
        });
        redraw(cx);

        focus_block_at(&editor, 0, cx);
        select(&editor, 0, 0..5, cx);
        assert!(toggle(&editor, format, cx), "{label}报没改到内容");
        redraw(cx);

        assert_eq!(
            buffer_text(&editor, cx),
            format!("{expected}\n\nbeta two\n"),
            "{label}写回的记号不对"
        );
    }
}

/// 选区所在的那一块里本来就有斜体标记：屏幕上「alpha one beta」的字母位置要和写回
/// 的字节位置对上，否则加粗会盖到 `*` 上或偏出两三个字母。
#[gpui::test]
async fn a_selection_in_a_block_that_already_has_markers_lands_on_the_right_letters(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "alpha *one* beta\n".to_string(), None)
    });
    redraw(cx);

    let block = visible_block(&editor, 0, cx);
    let visible = block.read_with(cx, |block, _cx| block.display_text().to_string());
    let at = visible.find("beta").expect("可见文本里应当有 beta");
    focus_block_at(&editor, 0, cx);
    select(&editor, 0, at..at + 4, cx);
    assert!(toggle(&editor, InlineFormat::Bold, cx));
    redraw(cx);

    assert_eq!(
        buffer_text(&editor, cx),
        "alpha *one* **beta**\n",
        "屏幕上第 {at} 个字母起这 4 个字母被加粗了，写法却对不上：可见文本坐标没换算到树内坐标"
    );
}

#[gpui::test]
async fn a_selection_spanning_three_blocks_formats_each_one_and_undoes_as_a_step(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, THREE_PARAGRAPHS.to_string(), None)
    });
    redraw(cx);

    // 首块只盖住后半、末块只盖住前半、中间那块整个被选中。
    cross_block(&editor, (0, 6), (2, 5), cx);
    assert!(
        toggle(&editor, InlineFormat::Bold, cx),
        "跨块选区报没改到内容"
    );
    redraw(cx);

    assert_eq!(
        buffer_text(&editor, cx),
        "alpha **one**\n\n**beta two**\n\n**gamma** three\n",
        "跨块选区应当每块各加自己的标记、中间那块整段加上。只有焦点块那段被改到，说明还在读它自己的 selected_range"
    );

    editor.update(cx, |editor, cx| editor.undo_document(cx));
    redraw(cx);
    assert_eq!(
        buffer_text(&editor, cx),
        THREE_PARAGRAPHS,
        "一次撤销应当把三块一起退回：逐块各开撤销组会要求按三次"
    );
}

#[gpui::test]
async fn a_collapsed_selection_formats_nothing_and_reports_unsupported(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx
        .add_window_view(|_window, cx| Editor::from_markdown(cx, TWO_PARAGRAPHS.to_string(), None));
    redraw(cx);

    focus_block_at(&editor, 0, cx);
    select(&editor, 0, 2..2, cx);
    assert!(!toggle(&editor, InlineFormat::Bold, cx), "空选区不该报成功");
    assert_eq!(buffer_text(&editor, cx), TWO_PARAGRAPHS, "空选区动了字节");
    assert_eq!(
        visible_block(&editor, 0, cx).read_with(cx, |block, _cx| block.selected_range.clone()),
        2..2,
        "被拒的格式操作还把光标位置挪了"
    );

    // 跨块选区的两端重在同一点：同样算空。
    cross_block(&editor, (1, 4), (1, 4), cx);
    assert!(!toggle(&editor, InlineFormat::Italic, cx));
    assert_eq!(
        buffer_text(&editor, cx),
        TWO_PARAGRAPHS,
        "塌成一点的跨块选区动了字节"
    );
}
