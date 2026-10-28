use super::super::Editor;
use crate::components::{BlockKind, Delete};
use gpui::{AppContext, TestAppContext};

#[gpui::test]
async fn forward_delete_at_block_end_merges_the_next_block(cx: &mut TestAppContext) {
    let cx = cx.add_empty_window();
    let editor = cx.new(|cx| Editor::from_markdown(cx, "alpha\n\nbeta".to_string(), None));

    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            let first = editor.document.visible_blocks()[0].entity.clone();
            first.update(cx, |block, block_cx| {
                block.move_to(block.visible_len(), block_cx);
                block.on_delete(&Delete, window, block_cx);
            });
        });
    });

    editor.update(cx, |editor, cx| {
        let visible = editor.document.visible_blocks();
        assert_eq!(visible.len(), 1, "块尾前向删除没有合并下一块");
        assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::Paragraph);
        assert_eq!(visible[0].entity.read(cx).display_text(), "alphabeta");
    });
}

#[gpui::test]
async fn forward_delete_before_a_table_removes_the_table_without_eating_its_data(
    cx: &mut TestAppContext,
) {
    let cx = cx.add_empty_window();
    let editor = cx.new(|cx| {
        Editor::from_markdown(cx, "alpha\n\n| a |\n| --- |\n| 1 |".to_string(), None)
    });

    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            let first = editor.document.visible_blocks()[0].entity.clone();
            first.update(cx, |block, block_cx| {
                block.move_to(block.visible_len(), block_cx);
                block.on_delete(&Delete, window, block_cx);
            });
        });
    });

    editor.update(cx, |editor, cx| {
        let visible = editor.document.visible_blocks();
        assert_eq!(visible.len(), 1, "表格没有被前向删除收掉");
        assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::Paragraph);
        assert_eq!(visible[0].entity.read(cx).display_text(), "alpha");
    });
}
