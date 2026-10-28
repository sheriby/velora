//! 事件总线热路径的性能闸门：点击与结构/导航事件不许为「找一块」
//! 克隆整篇可见块表。克隆一块要付一次 Entity 引用计数与 LeakDetector 登记，
//! 10 MiB 档整篇一次实测 17ms；这些动作要么最高频（点击、回车），要么在大文档
//! 上按块数线性付钱。计数器是 `DocumentTree::visible_blocks_cloned` 与
//! `Editor::focus_transfer_block_updates`。

use super::super::Editor;
use crate::components::{BlockEvent, InlineTextTree, Newline};
use gpui::{AppContext, TestAppContext};

fn plain_document(line_count: usize) -> String {
    (0..line_count)
        .map(|index| format!("第 {index} 段。"))
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// 焦点转移（RequestFocus，鼠标点进未聚焦块时块自己发）只许刷新参与活动样式
/// 切换的前后两块：旧活动目标摘输入态，新块亮起来。逐块 notify 整篇可见块是
/// 每次点击一次的 O(文档) 实体写风暴，外加一次整篇克隆。
#[gpui::test]
async fn focus_transfer_notifies_only_the_previous_and_new_active_block(
    cx: &mut TestAppContext,
) {
    let cx = cx.add_empty_window();
    let editor = cx.new(|cx| Editor::from_markdown(cx, plain_document(12), None));

    // 打开文档后的活动块是第一块：先把它钉成旧活动目标。
    let first_id = editor.read_with(cx, |editor, _cx| {
        editor.document.visible_blocks()[0].entity.entity_id()
    });
    editor.update(cx, |editor, _cx| editor.focus_block(first_id));

    let before = editor.read_with(cx, |editor, _cx| {
        (
            editor.document.visible_blocks_cloned.get(),
            editor.focus_transfer_block_updates.get(),
        )
    });

    let target_id = editor.update(cx, |editor, cx| {
        let target = editor.document.visible_blocks()[2].entity.clone();
        let target_id = target.entity_id();
        target.update(cx, |_block, cx| cx.emit(BlockEvent::RequestFocus));
        target_id
    });

    let (flattened, refreshed, active) = editor.read_with(cx, |editor, _cx| {
        (
            editor.document.visible_blocks_cloned.get(),
            editor.focus_transfer_block_updates.get(),
            editor.active_entity_id,
        )
    });
    assert_eq!(active, Some(target_id), "焦点没落到被点的块上");
    assert_eq!(
        refreshed - before.1,
        2,
        "活动样式切换只该刷新前后两块，实测 {} 块",
        refreshed - before.1
    );
    assert_eq!(
        flattened - before.0,
        0,
        "焦点转移克隆了整篇可见块表（旧实现逐块 notify 前先 flatten）"
    );
}

/// 回车是最高频的结构事件，而它根本不用可见序号：不许为它克隆整篇可见块表。
#[gpui::test]
async fn enter_does_not_clone_the_visible_block_list(cx: &mut TestAppContext) {
    let cx = cx.add_empty_window();
    let editor = cx.new(|cx| Editor::from_markdown(cx, plain_document(40), None));

    let before = editor.read_with(cx, |editor, _cx| {
        editor.document.visible_blocks_cloned.get()
    });
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            let block = editor.document.visible_blocks()[0].entity.clone();
            block.update(cx, |block, block_cx| {
                block.move_to(block.visible_len(), block_cx);
                block.on_newline(&Newline, window, block_cx);
            });
        });
    });

    let (flattened, visible_len) = editor.read_with(cx, |editor, _cx| {
        (
            editor.document.visible_blocks_cloned.get(),
            editor.document.visible_blocks().len(),
        )
    });
    assert_eq!(
        flattened - before,
        0,
        "回车克隆了整篇可见块表（flatten 结果只有 merge/delete/outdent 分支用）"
    );
    assert_eq!(visible_len, 41, "回车没拆出新块，这条闸门测错了东西");
}

/// 合并（Backspace 块首）仍然要找「前一块」：惰性查邻居之后行为必须不变，
/// 且不再为找它克隆整篇。
#[gpui::test]
async fn merge_into_prev_still_takes_the_correct_previous_block(cx: &mut TestAppContext) {
    let cx = cx.add_empty_window();
    let editor = cx.new(|cx| Editor::from_markdown(cx, plain_document(3), None));

    let before = editor.read_with(cx, |editor, _cx| {
        editor.document.visible_blocks_cloned.get()
    });
    let first_id = editor.read_with(cx, |editor, _cx| {
        editor.document.visible_blocks()[0].entity.entity_id()
    });
    editor.update(cx, |editor, cx| {
        let block = editor.document.visible_blocks()[1].entity.clone();
        block.update(cx, |_block, cx| {
            cx.emit(BlockEvent::RequestMergeIntoPrev {
                content: InlineTextTree::plain("第二段。"),
            });
        });
    });

    let (flattened, visible_len, active, first_text) = editor.read_with(cx, |editor, cx| {
        (
            editor.document.visible_blocks_cloned.get(),
            editor.document.visible_blocks().len(),
            editor.active_entity_id,
            editor.document.visible_blocks()[0].entity.read(cx).display_text().to_string(),
        )
    });
    assert_eq!(
        flattened - before,
        0,
        "合并克隆了整篇可见块表（只需要前一块那一个实体）"
    );
    assert_eq!(visible_len, 2, "合并没删掉被吸收的块");
    assert_eq!(active, Some(first_id), "合并后焦点没落回前一块");
    assert_eq!(first_text, "第 0 段。第二段。", "内容没并进前一块");
}

/// 表格边缘的垂直导航（移出表格去相邻块）同样只为找一块：不许整篇克隆。
#[gpui::test]
async fn leaving_a_table_vertically_does_not_clone_the_visible_block_list(
    cx: &mut TestAppContext,
) {
    let cx = cx.add_empty_window();
    let markdown = "| 甲 | 乙 |\n| --- | --- |\n| 丙 | 丁 |\n\n表格下面那一段。\n";
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown.to_string(), None));

    let before = editor.read_with(cx, |editor, _cx| {
        editor.document.visible_blocks_cloned.get()
    });
    editor.update(cx, |editor, cx| {
        let binding = editor
            .table_cells
            .values()
            .find(|binding| binding.position == crate::components::TableCellPosition { row: 1, column: 0 })
            .cloned()
            .expect("夹具该有 (1,0) 格子");
        binding
            .cell
            .update(cx, |_cell, cx| cx.emit(BlockEvent::RequestTableCellMoveVertical { delta: 1 }));
    });

    let (flattened, active) = editor.read_with(cx, |editor, _cx| {
        (
            editor.document.visible_blocks_cloned.get(),
            editor.active_entity_id,
        )
    });
    assert_eq!(
        flattened - before,
        0,
        "移出表格克隆了整篇可见块表（只需要表的上/下一个块）"
    );
    let active_text = editor.read_with(cx, |editor, cx| {
        active
            .and_then(|id| editor.focusable_entity_by_id(id))
            .map(|block| block.read(cx).display_text().to_string())
    });
    assert_eq!(
        active_text.as_deref(),
        Some("表格下面那一段。"),
        "从最后一行往下没落到表格下面的块"
    );
}
