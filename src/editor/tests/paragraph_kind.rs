//! 「段落」菜单与 ⌘1…⌘6 那一条入口：换块种类只动被波及的那几行字节，
//! 块与块之间的空行接缝跟着变，一步撤销回到原样。
//!
//! 标题与正文是一档，无序 / 有序 / 任务列表是另一档：后者的记号跟着同族邻项写，
//! 序号接所在列表组，带子块的父项只在列表这一族内部换。
//! 引用是第三档：整段文字存在这一块自己的标题里，跨行的引用只许换成正文。

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

/// 菜单那一行的置灰口径（与 `apply` 走同一条判断）。
fn available(editor: &Entity<Editor>, target: BlockKindTarget, cx: &mut VisualTestContext) -> bool {
    editor.read_with(cx, |editor, cx| {
        editor.block_kind_target_is_available(target, cx)
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

/// 标注（`> [!note]`）带头部与子块，这一笔不动它。
#[gpui::test]
async fn a_callout_is_left_alone(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(
            cx,
            "> [!note] 标注甲\n>\n> 引用乙\n\n正文丙\n".to_string(),
            None,
        )
    });
    redraw(cx);

    let before = buffer_text(&editor, cx);
    put_caret(&editor, 0, 3, cx);
    assert!(
        !apply(&editor, BlockKindTarget::Heading(2), cx),
        "标注有头部与子块，这一笔不该动它"
    );
    assert!(
        !apply(&editor, BlockKindTarget::Paragraph, cx),
        "标注换成正文要把子块安置进根序列，这一笔不该动它"
    );
    assert_eq!(buffer_text(&editor, cx), before, "被拒的换种类动了字节");
}

#[gpui::test]
async fn structural_blocks_are_left_alone(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "正文甲\n\n```text\n代码乙\n```\n".to_string(), None)
    });
    redraw(cx);

    let before = buffer_text(&editor, cx);
    let code_target = visible_block(&editor, 1, cx);
    editor.update(cx, |editor, _cx| {
        editor.focus_block(code_target.entity_id());
    });
    redraw(cx);
    assert!(
        !apply(&editor, BlockKindTarget::Heading(2), cx),
        "代码块是原子的结构块，不该被换成标题"
    );
    assert!(
        !apply(&editor, BlockKindTarget::Quote, cx),
        "代码块也不该被换成引用"
    );
    assert_eq!(buffer_text(&editor, cx), before, "被拒的换种类动了字节");
}

// ── 引用那一档 ──────────────────────────────────────────────────────────────

#[gpui::test]
async fn a_paragraph_becomes_a_quote_and_the_second_press_cancels_it(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx
        .add_window_view(|_window, cx| Editor::from_markdown(cx, TWO_PARAGRAPHS.to_string(), None));
    redraw(cx);

    put_caret(&editor, 0, 3, cx);
    assert!(apply(&editor, BlockKindTarget::Quote, cx));
    redraw(cx);

    assert_eq!(
        buffer_text(&editor, cx),
        "> 标题甲\n\n正文乙\n",
        "记号只补在这一行开头，别处字节原样"
    );
    assert_eq!(
        kinds(&editor, cx),
        vec!["Quote".to_string(), "Paragraph".to_string()],
        "块树与写回的字节不一致"
    );

    assert!(apply(&editor, BlockKindTarget::Quote, cx));
    redraw(cx);
    assert_eq!(
        buffer_text(&editor, cx),
        TWO_PARAGRAPHS,
        "已经在引用里再点一次该取消引用，逐字节回到原样"
    );
}

/// 引用里那一段跨两行：换成正文只去掉每行的 `> `，两行仍是同一块。
#[gpui::test]
async fn a_multi_line_quote_switches_to_plain_text_without_splitting(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "> 甲引用\n> 乙引用\n\n正文丙\n".to_string(), None)
    });
    redraw(cx);

    put_caret(&editor, 0, 3, cx);
    assert!(apply(&editor, BlockKindTarget::Paragraph, cx));
    redraw(cx);

    let text = buffer_text(&editor, cx);
    assert_eq!(
        text, "甲引用\n乙引用\n\n正文丙\n",
        "两块引用之间的接缝与第三块都原样，只去掉这一块的行记号"
    );
    assert_eq!(
        kinds(&editor, cx),
        vec!["Paragraph".to_string(), "Paragraph".to_string()],
        "跨两行的引用换完该还是一块（多出来那根就说明按行拆了）"
    );

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
}

/// 跨两行的引用换不出「标题」这一档：换成正文可以，换标题会把那行换行写成另一块。
#[gpui::test]
async fn a_multi_line_quote_refuses_a_heading_and_the_row_is_greyed(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "> 甲引用\n> 乙引用\n".to_string(), None)
    });
    redraw(cx);

    put_caret(&editor, 0, 3, cx);
    assert!(
        !apply(&editor, BlockKindTarget::Heading(2), cx),
        "跨行的引用换标题会把第二行留在原地当另一块"
    );
    assert_eq!(
        buffer_text(&editor, cx),
        "> 甲引用\n> 乙引用\n",
        "被拒的换种类动了字节"
    );
    assert!(
        !available(&editor, BlockKindTarget::Heading(2), cx),
        "菜单里「二级标题」这一行该跟着置灰"
    );
    assert!(
        available(&editor, BlockKindTarget::Paragraph, cx),
        "同一处选区里「正文」这一行该能点"
    );
}

#[gpui::test]
async fn a_single_line_quote_becomes_a_heading_without_touching_the_neighbour(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "> 标题甲\n\n正文乙\n".to_string(), None)
    });
    redraw(cx);

    put_caret(&editor, 0, 3, cx);
    assert!(apply(&editor, BlockKindTarget::Heading(2), cx));
    redraw(cx);

    assert_eq!(
        buffer_text(&editor, cx),
        "## 标题甲\n\n正文乙\n",
        "引用换成标题：行记号 `> ` 换成 `## `，接缝与邻块原样"
    );
    assert_eq!(
        kinds(&editor, cx),
        vec!["Heading { level: 2 }".to_string(), "Paragraph".to_string()],
        "块树与写回的字节不一致"
    );
}

/// 列表项换引用：`- ` 换成 `> `，同组另一项不动。
#[gpui::test]
async fn a_list_item_becomes_a_quote(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "- 项甲\n- 项乙\n".to_string(), None)
    });
    redraw(cx);

    put_caret(&editor, 1, 3, cx);
    assert!(apply(&editor, BlockKindTarget::Quote, cx));
    redraw(cx);

    assert_eq!(
        buffer_text(&editor, cx),
        "- 项甲\n\n> 项乙\n",
        "列表项换成引用，接缝上要多空一行；第一项那行字节原样"
    );
    assert_eq!(
        kinds(&editor, cx),
        vec!["BulletedListItem".to_string(), "Quote".to_string()],
        "块树与写回的字节不一致"
    );
}

/// 选区盖住两段正文：两段各成一块引用，字节之间要留那行空行——不留的话
/// `> 甲一` 与 `> 乙二` 会被读回成同一块两行的引用。
#[gpui::test]
async fn a_selection_over_two_paragraphs_makes_two_quotes(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "甲一\n\n乙二\n".to_string(), None)
    });
    redraw(cx);

    let (first, last) = (visible_block(&editor, 0, cx), visible_block(&editor, 1, cx));
    editor.update(cx, |editor, _cx| {
        editor.cross_block_selection = Some(crate::editor::CrossBlockSelection {
            anchor: crate::editor::CrossBlockSelectionEndpoint {
                entity_id: first.entity_id(),
                offset: 0,
            },
            focus: crate::editor::CrossBlockSelectionEndpoint {
                entity_id: last.entity_id(),
                offset: 2,
            },
        });
    });

    assert!(apply(&editor, BlockKindTarget::Quote, cx));
    redraw(cx);

    let text = buffer_text(&editor, cx);
    assert_eq!(text, "> 甲一\n\n> 乙二\n", "两块引用之间那行空行不能少");
    assert_eq!(
        kinds(&editor, cx),
        vec!["Quote".to_string(), "Quote".to_string()],
        "块树与写回的字节不一致"
    );

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
        vec!["Quote".to_string(), "Quote".to_string()],
        "写下去的字节读不回两块引用"
    );

    editor.update(cx, |editor, cx| editor.undo_document(cx));
    redraw(cx);
    assert_eq!(
        buffer_text(&editor, cx),
        "甲一\n\n乙二\n",
        "一次撤销要把两块一起退回"
    );
}

// ── 列表那一档 ──────────────────────────────────────────────────────────────

#[gpui::test]
async fn a_paragraph_becomes_a_bulleted_item_and_undoes_as_a_step(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx
        .add_window_view(|_window, cx| Editor::from_markdown(cx, TWO_PARAGRAPHS.to_string(), None));
    redraw(cx);

    put_caret(&editor, 0, 3, cx);
    assert!(apply(&editor, BlockKindTarget::BulletList, cx));
    redraw(cx);

    assert_eq!(
        buffer_text(&editor, cx),
        "- 标题甲\n\n正文乙\n",
        "记号只加在这一行开头，别的字节原样"
    );
    assert_eq!(
        kinds(&editor, cx),
        vec!["BulletedListItem".to_string(), "Paragraph".to_string()],
        "块树与写回的字节不一致"
    );

    editor.update(cx, |editor, cx| editor.undo_document(cx));
    redraw(cx);
    assert_eq!(buffer_text(&editor, cx), TWO_PARAGRAPHS, "一步撤销回到原样");
    assert_eq!(
        kinds(&editor, cx),
        vec!["Paragraph".to_string(), "Paragraph".to_string()],
        "撤销只退回了字节，块树还是列表项"
    );
}

#[gpui::test]
async fn applying_the_same_bullet_again_gives_a_plain_paragraph(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx
        .add_window_view(|_window, cx| Editor::from_markdown(cx, TWO_PARAGRAPHS.to_string(), None));
    redraw(cx);

    put_caret(&editor, 0, 3, cx);
    assert!(apply(&editor, BlockKindTarget::BulletList, cx));
    assert!(apply(&editor, BlockKindTarget::BulletList, cx));
    redraw(cx);

    assert_eq!(
        buffer_text(&editor, cx),
        TWO_PARAGRAPHS,
        "已经是无序项再点一次该取消列表，逐字节回到原样"
    );
}

#[gpui::test]
async fn a_paragraph_becomes_a_numbered_item_and_the_second_press_cancels_it(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    let (editor, cx) = cx
        .add_window_view(|_window, cx| Editor::from_markdown(cx, TWO_PARAGRAPHS.to_string(), None));
    redraw(cx);

    put_caret(&editor, 0, 3, cx);
    assert!(apply(&editor, BlockKindTarget::NumberedList, cx));
    redraw(cx);

    assert_eq!(buffer_text(&editor, cx), "1. 标题甲\n\n正文乙\n");
    assert_eq!(
        kinds(&editor, cx),
        vec!["NumberedListItem".to_string(), "Paragraph".to_string()],
        "块树与写回的字节不一致"
    );

    assert!(apply(&editor, BlockKindTarget::NumberedList, cx));
    redraw(cx);
    assert_eq!(
        buffer_text(&editor, cx),
        TWO_PARAGRAPHS,
        "已经是有序项再点一次该取消列表"
    );
}

/// 紧跟在已有列表组下面的一段正文换成有序项，序号要接上上面那组，不能从 1 重来。
#[gpui::test]
async fn a_numbered_item_continues_the_group_above_it(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "1. 甲一\n2. 乙二\n\n正文丙\n".to_string(), None)
    });
    redraw(cx);

    put_caret(&editor, 2, 3, cx);
    assert!(apply(&editor, BlockKindTarget::NumberedList, cx));
    redraw(cx);

    assert_eq!(
        buffer_text(&editor, cx),
        "1. 甲一\n2. 乙二\n3. 正文丙\n",
        "上面那组写到 2，新项该接 3 并进同一组（紧排列表不留空行）；上面两行的字节不动"
    );
    assert_eq!(
        kinds(&editor, cx),
        vec![
            "NumberedListItem".to_string(),
            "NumberedListItem".to_string(),
            "NumberedListItem".to_string()
        ],
        "块树与写回的字节不一致"
    );
}

/// 无序项的记号跟着同族邻项写：邻项用的是 `+`，新项不能被打回默认的 `-`。
#[gpui::test]
async fn the_new_bullet_takes_the_marker_the_neighbour_is_already_using(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "- 项甲\n+ 项乙\n\n正文丙\n".to_string(), None)
    });
    redraw(cx);

    put_caret(&editor, 2, 3, cx);
    assert!(apply(&editor, BlockKindTarget::BulletList, cx));
    redraw(cx);

    assert_eq!(
        buffer_text(&editor, cx),
        "- 项甲\n+ 项乙\n+ 正文丙\n",
        "挨着的项写的是 `+`，新项跟着它，并进同一组（紧排列表不留空行）；上面两行的字节原样"
    );
}

#[gpui::test]
async fn a_task_item_gains_the_checkbox_and_a_second_press_drops_it(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx
        .add_window_view(|_window, cx| Editor::from_markdown(cx, TWO_PARAGRAPHS.to_string(), None));
    redraw(cx);

    put_caret(&editor, 0, 3, cx);
    assert!(apply(&editor, BlockKindTarget::TaskList, cx));
    redraw(cx);

    assert_eq!(buffer_text(&editor, cx), "- [ ] 标题甲\n\n正文乙\n");
    assert_eq!(
        kinds(&editor, cx),
        vec![
            "TaskListItem { checked: false }".to_string(),
            "Paragraph".to_string()
        ],
        "块树与写回的字节不一致"
    );

    // 再点一次是去掉复选框，回到普通无序项——不是取消整个列表。
    assert!(apply(&editor, BlockKindTarget::TaskList, cx));
    redraw(cx);
    assert_eq!(
        buffer_text(&editor, cx),
        "- 标题甲\n\n正文乙\n",
        "任务项再点一次该退回无序项"
    );
    assert_eq!(
        kinds(&editor, cx),
        vec!["BulletedListItem".to_string(), "Paragraph".to_string()],
        "块树与写回的字节不一致"
    );
}

/// 列表组中间换一项：只重写那一行，两端的项一个字都不动；写下去的字节读回同一个结构。
#[gpui::test]
async fn the_middle_item_of_a_group_can_switch_family_without_disturbing_the_ends(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "- 项甲\n- 项乙\n- 项丙\n".to_string(), None)
    });
    redraw(cx);

    put_caret(&editor, 1, 3, cx);
    assert!(apply(&editor, BlockKindTarget::NumberedList, cx));
    redraw(cx);

    let text = buffer_text(&editor, cx);
    assert_eq!(
        text, "- 项甲\n1. 项乙\n- 项丙\n",
        "换的是中间那一行，两端的项与行序都不该跟着动"
    );
    assert_eq!(
        kinds(&editor, cx),
        vec![
            "BulletedListItem".to_string(),
            "NumberedListItem".to_string(),
            "BulletedListItem".to_string()
        ],
        "块树与写回的字节不一致"
    );

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
}

/// 选区盖住三段正文：三块各加自己的记号，一次撤销一起退回。
#[gpui::test]
async fn a_selection_over_three_paragraphs_makes_three_items_in_one_undo_step(
    cx: &mut TestAppContext,
) {
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
                offset: 2,
            },
        });
    });

    assert!(apply(&editor, BlockKindTarget::NumberedList, cx));
    redraw(cx);
    assert_eq!(
        buffer_text(&editor, cx),
        "1. 甲一\n2. 乙二\n3. 丙三\n",
        "三块各接自己的序号，并进同一组（模型对紧排列表的写法不留空行）"
    );

    editor.update(cx, |editor, cx| editor.undo_document(cx));
    redraw(cx);
    assert_eq!(
        buffer_text(&editor, cx),
        "甲一\n\n乙二\n\n丙三\n",
        "一次撤销要把三块一起退回"
    );
}

/// 带子块的父项：换进另一族列表项可以（子块的缩进层数不变），换成标题要拒
/// （序列化会把子块写成和父块平齐的行，块树还是父子、文件已经是两个根）。
#[gpui::test]
async fn a_list_item_with_children_only_switches_within_the_list_family(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "- 项甲\n  - 内乙\n\n正文丙\n".to_string(), None)
    });
    redraw(cx);

    put_caret(&editor, 0, 3, cx);
    assert!(
        !apply(&editor, BlockKindTarget::Heading(2), cx),
        "父项换成标题会把子块提成根块，这一笔不该动它"
    );
    assert_eq!(
        buffer_text(&editor, cx),
        "- 项甲\n  - 内乙\n\n正文丙\n",
        "被拒的换种类动了字节"
    );

    assert!(
        apply(&editor, BlockKindTarget::TaskList, cx),
        "父项换任务项该放行"
    );
    redraw(cx);
    let text = buffer_text(&editor, cx);
    assert_eq!(
        text, "- [ ] 项甲\n  - 内乙\n\n正文丙\n",
        "只动父项那一行的记号，子块与它自己的缩进原样"
    );
    assert_eq!(
        kinds(&editor, cx),
        vec![
            "TaskListItem { checked: false }".to_string(),
            "BulletedListItem".to_string(),
            "Paragraph".to_string()
        ],
        "块树与写回的字节不一致"
    );

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
}
