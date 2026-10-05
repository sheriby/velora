//! 「段落」菜单与 ⌘1…⌘6 那一条入口：换块种类只动被波及的那几行字节，
//! 块与块之间的空行接缝跟着变，一步撤销回到原样。

use super::common::*;
use crate::components::{Heading2, ParagraphText};
use crate::editor::paragraph_ops::BlockKindTarget;
use gpui::Entity;

const TWO_PARAGRAPHS: &str = "标题甲\n\n正文乙\n";

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

/// 光标落到第 `index` 块的第 `offset` 个字符处（走真实的 `focus_block` 通道）。
fn put_caret(editor: &Entity<Editor>, index: usize, offset: usize, cx: &mut VisualTestContext) {
    let block = visible_block(editor, index, cx);
    editor.update(cx, |editor, _cx| editor.focus_block(block.entity_id()));
    block.update(cx, |block, _cx| {
        block.selected_range = offset..offset;
    });
    redraw(cx);
}

fn apply(editor: &Entity<Editor>, target: BlockKindTarget, cx: &mut VisualTestContext) -> bool {
    editor.update(cx, |editor, cx| {
        editor.apply_block_kind_to_selection(target, cx)
    })
}

fn kinds(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> Vec<String> {
    editor.read_with(cx, |editor, cx| {
        editor
            .document
            .visible_blocks()
            .iter()
            .map(|visible| format!("{:?}", visible.entity.read(cx).kind()))
            .collect()
    })
}

#[gpui::test]
async fn heading_level_two_writes_the_marker_and_leaves_the_sibling_block_alone(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    let (editor, cx) = cx
        .add_window_view(|_window, cx| Editor::from_markdown(cx, TWO_PARAGRAPHS.to_string(), None));
    redraw(cx);

    put_caret(&editor, 0, 3, cx);
    cx.dispatch_action(Heading2);
    redraw(cx);

    assert_eq!(
        buffer_text(&editor, cx),
        "## 标题甲\n\n正文乙\n",
        "标题号只该动这一行，别的字节原样"
    );
    assert_eq!(
        kinds(&editor, cx),
        vec!["Heading { level: 2 }".to_string(), "Paragraph".to_string()],
        "块树与写回的字节不一致"
    );
}

#[gpui::test]
async fn applying_the_same_heading_level_again_gives_a_plain_paragraph(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx
        .add_window_view(|_window, cx| Editor::from_markdown(cx, TWO_PARAGRAPHS.to_string(), None));
    redraw(cx);

    put_caret(&editor, 0, 3, cx);
    assert!(apply(&editor, BlockKindTarget::Heading(2), cx));
    assert!(apply(&editor, BlockKindTarget::Heading(2), cx));
    redraw(cx);

    assert_eq!(
        buffer_text(&editor, cx),
        TWO_PARAGRAPHS,
        "再点一次同一级别该退回普通段落，逐字节回到原样"
    );
}

#[gpui::test]
async fn paragraph_command_switches_a_heading_back_without_touching_the_neighbour(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "### 标题甲\n\n正文乙\n".to_string(), None)
    });
    redraw(cx);

    put_caret(&editor, 0, 6, cx);
    cx.dispatch_action(ParagraphText);
    redraw(cx);

    assert_eq!(buffer_text(&editor, cx), "标题甲\n\n正文乙\n");
}

#[gpui::test]
async fn a_selection_spanning_roots_changes_each_one_and_undoes_as_a_step(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "甲一\n\n乙二\n\n丙三\n".to_string(), None)
    });
    redraw(cx);

    let (first, last) = (visible_block(&editor, 0, cx), visible_block(&editor, 2, cx));
    editor.update(cx, |editor, _cx| {
        editor.cross_block_selection = Some(crate::editor::CrossBlockSelection {
            anchor: crate::editor::CrossBlockSelectionEndpoint {
                entity_id: first.entity_id(),
                offset: 0,
            },
            focus: crate::editor::CrossBlockSelectionEndpoint {
                entity_id: last.entity_id(),
                offset: 6,
            },
        });
    });

    assert!(apply(&editor, BlockKindTarget::Heading(2), cx));
    redraw(cx);
    assert_eq!(
        buffer_text(&editor, cx),
        "## 甲一\n\n## 乙二\n\n## 丙三\n",
        "选区盖到三块就该三块都换，中间那块不能漏"
    );

    editor.update(cx, |editor, cx| editor.undo_document(cx));
    redraw(cx);
    assert_eq!(
        buffer_text(&editor, cx),
        "甲一\n\n乙二\n\n丙三\n",
        "一次撤销要把三块一起退回：逐块各开撤销组会要求按三次"
    );
}

#[gpui::test]
async fn a_kind_change_leaves_another_block_s_original_writing_alone(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    // 第一块是 Setext 写法：模型的序列化只会写 ATX，所以整篇重投影会把它改掉。
    // 换种类走的是区段写回，没碰的那一块必须一个字节都不动。
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "标题甲\n===\n\n正文乙\n".to_string(), None)
    });
    redraw(cx);

    put_caret(&editor, 1, 3, cx);
    assert!(apply(&editor, BlockKindTarget::Heading(2), cx));
    redraw(cx);

    assert_eq!(
        buffer_text(&editor, cx),
        "标题甲\n===\n\n## 正文乙\n",
        "写回把没改过的那一块也按模型的写法重排了：这条命令只该动被波及的那一段"
    );
}

#[gpui::test]
async fn turning_a_list_item_into_a_heading_keeps_the_sibling_item_a_list(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "- 项甲\n- 项乙\n\n正文丙\n".to_string(), None)
    });
    redraw(cx);

    put_caret(&editor, 1, 3, cx);
    assert!(apply(&editor, BlockKindTarget::Heading(2), cx));
    redraw(cx);

    let text = buffer_text(&editor, cx);
    assert_eq!(
        text, "- 项甲\n\n## 项乙\n\n正文丙\n",
        "列表项换成标题，接缝上要多空一行；除这两行以外别的字节原样"
    );
    assert_eq!(
        kinds(&editor, cx),
        vec![
            "BulletedListItem".to_string(),
            "Heading { level: 2 }".to_string(),
            "Paragraph".to_string()
        ],
        "写回的字节与块树的种类对不上"
    );

    // 写下去的字节重新读一遍，种类必须一模一样——ATX 标题能打断列表，
    // 所以少那行空行不影响「树与文件一致」。
    let reread = cx.new(|cx| Editor::from_markdown(cx, text, None));
    let reread_kinds = reread.read_with(cx, |editor, cx| {
        editor
            .document
            .visible_blocks()
            .iter()
            .map(|visible| format!("{:?}", visible.entity.read(cx).kind()))
            .collect::<Vec<_>>()
    });
    assert_eq!(
        reread_kinds,
        kinds(&editor, cx),
        "写下去的字节读不回同一个结构"
    );

    editor.update(cx, |editor, cx| editor.undo_document(cx));
    redraw(cx);
    assert_eq!(
        buffer_text(&editor, cx),
        "- 项甲\n- 项乙\n\n正文丙\n",
        "撤销要把改掉的这一行整个放回去"
    );
}

#[gpui::test]
async fn containers_and_structural_blocks_are_left_alone(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "> 引用甲\n\n```text\n代码乙\n```\n".to_string(), None)
    });
    redraw(cx);

    let before = buffer_text(&editor, cx);
    put_caret(&editor, 0, 3, cx);
    assert!(
        !apply(&editor, BlockKindTarget::Heading(2), cx),
        "引用是容器，本笔不该动它"
    );
    let quote_target = visible_block(&editor, 1, cx);
    editor.update(cx, |editor, _cx| {
        editor.focus_block(quote_target.entity_id());
    });
    redraw(cx);
    assert!(
        !apply(&editor, BlockKindTarget::Heading(2), cx),
        "代码块是原子的结构块，不该被换成标题"
    );
    assert_eq!(buffer_text(&editor, cx), before, "被拒的换种类动了字节");
}
