//! 撤销栈存增量，不存全文副本。
//!
//! 旧实现每按一键就 `clone` 一份全文并整篇序列化来对比，撤销栈 200 深，10 MiB
//! 文档最坏吃掉 2 GB、单键 13 秒。缓冲区成为事实源之后，撤销组就是那几次
//! `TextBuffer::edit` 的逆操作：每条只存「被换掉的字节 + 它现在占的区间」。
//!
//! 顺带保住一件旧实现保不住的事：**撤销一次结构变更不该把未编辑的块洗掉**。
//! 旧路径撤销 = 拿全文快照重新解析整棵树 = 保存时整篇重新序列化；新路径撤销 =
//! 把区间换回原字节，别的块连字节都没被碰过。

use super::block_source_spans::{assert_spans_tile_the_content, root_block_spans, span_ranges};
use super::common::*;
use crate::editor::encoding;

/// 一个会被重新序列化的形状：填充过的表格 + 下划线强调。
const LOSSY_DOC: &str = concat!(
    "段落文字\n",
    "\n",
    "| 名称 | 数量 |\n",
    "| ---- | ---- |\n",
    "| 甲   | 1    |\n",
    "\n",
    "强调 __下划线__ 结尾\n",
);

#[gpui::test]
async fn undo_history_stores_deltas_not_document_copies(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    // 够大的文档：全文快照与增量的差别在这里必须是数量级。
    let source = format!(
        "开头段落\n\n{}末尾段落\n",
        "这是一段用于撑大小的中文行。\n".repeat(16_000)
    );
    assert!(
        source.len() > 512 * 1024,
        "夹具应该大于一半 MiB，实际 {}",
        source.len()
    );
    let size = source.len();
    let (editor, cx) = cx.add_window_view(|_window, cx| Editor::from_markdown(cx, source, None));
    redraw(cx);

    // 打字与拆块都声明了自己的区间：留下的必须是增量而不是整篇。
    cx.simulate_input("甲乙丙");
    redraw(cx);
    cx.dispatch_action(Newline);
    redraw(cx);

    let (bytes, groups) = editor.read_with(cx, |editor, _cx| {
        (editor.undo_history_byte_len(), editor.undo_history.len())
    });
    assert!(
        groups >= 2,
        "打字与拆块应各留下一条撤销记录，实际 {groups} 条"
    );
    assert!(
        bytes < 64 * 1024,
        "撤销栈存了 {bytes} 字节：文档 {size} 字节，说明条目里还存着全文副本"
    );
}

#[gpui::test]
async fn undoing_a_split_puts_the_exact_bytes_back(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let path = temp_markdown_path("undo-split");
    fs::write(&path, LOSSY_DOC.replace('\n', "\r\n")).expect("write fixture");
    let cleanup = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup);
    });

    let document = encoding::load_document(&path).expect("read fixture");
    let open_path = path.clone();
    let (editor, cx) = cx.add_window_view(move |_window, cx| {
        Editor::from_loaded_document(cx, document, Some(open_path))
    });
    redraw(cx);

    cx.simulate_input("写");
    redraw(cx);
    cx.dispatch_action(Newline);
    redraw(cx);
    editor.update(cx, |editor, _cx| editor.undo_document(_cx));
    redraw(cx);
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| editor.save_document(window, cx));
    });
    redraw(cx);
    redraw(cx);

    // 撤销掉的只是那一处改动：表格填充与 `__` 写法仍然是磁盘上的样子。
    let saved = fs::read_to_string(&path).expect("read saved file");
    assert_eq!(
        saved,
        LOSSY_DOC
            .replace("段落文字", "写段落文字")
            .replace('\n', "\r\n"),
        "撤销一次拆块之后，未编辑的块被重新序列化了：{saved:?}"
    );
}

/// 撤销一次粘贴 = 把插进去的那段字节拿掉。粘贴在缓冲区里只是一次插入，所以
/// 撤销组里连被替换的字节都没有，未编辑的块更不会被动到。
#[gpui::test]
async fn undoing_a_multiline_paste_puts_the_exact_bytes_back(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    // 够大的文档：整篇快照与增量的差别在这里必须是数量级。
    let source = format!(
        "段落文字\n\n{}| 名称 | 数量 |\n| ---- | ---- |\n| 甲   | 1    |\n\n强调 __下划线__ 结尾\n",
        "这是一段不参与改动的中文行。\n".repeat(16_000)
    );
    assert!(source.len() > 512 * 1024, "夹具应该大于一半 MiB");
    let path = temp_markdown_path("undo-paste");
    fs::write(&path, source.replace('\n', "\r\n")).expect("write fixture");
    let original_disk = fs::read_to_string(&path).expect("read fixture");
    let cleanup = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup);
    });

    let document = encoding::load_document(&path).expect("read fixture");
    let open_path = path.clone();
    let (editor, cx) = cx.add_window_view(move |_window, cx| {
        Editor::from_loaded_document(cx, document, Some(open_path))
    });
    redraw(cx);

    let before_len = editor.read_with(cx, |editor, _cx| editor.buffer.byte_len());
    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| {
            let block = editor.document.visible_blocks()[0].entity.clone();
            editor.on_block_event(
                block,
                &BlockEvent::RequestPasteMultiline {
                    leading: InlineTextTree::plain(String::new()),
                    lines: vec!["粘贴一".to_string(), "粘贴二".to_string()],
                    trailing: InlineTextTree::plain("段落文字".to_string()),
                    split_physical_lines: true,
                },
                cx,
            );
        });
    });
    redraw(cx);
    let inserted_len =
        editor.read_with(cx, |editor, _cx| editor.buffer.byte_len()) - before_len;
    assert!(inserted_len > 0, "粘贴没进缓冲区");

    let stored = editor.read_with(cx, |editor, _cx| editor.undo_history_byte_len());
    assert!(
        stored < inserted_len,
        "撤销粘贴记了 {stored} 字节，比粘进去的 {inserted_len} 字节还多：又在存整篇副本"
    );

    editor.update(cx, |editor, _cx| editor.undo_document(_cx));
    redraw(cx);
    cx.simulate_keystrokes("ctrl-s");
    redraw(cx);

    let saved = fs::read_to_string(&path).expect("read saved file");
    assert_eq!(
        saved, original_disk,
        "撤销一次粘贴之后，磁盘上的字节不再是原文"
    );
}

#[gpui::test]
async fn undo_and_redo_round_trip_the_document_text(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, LOSSY_DOC.to_string(), None));
    redraw(cx);
    let original = editor.read_with(cx, |editor, _cx| editor.buffer.text());

    cx.simulate_input("甲乙丙");
    redraw(cx);
    cx.dispatch_action(Newline);
    redraw(cx);
    let edited = editor.read_with(cx, |editor, _cx| editor.buffer.text());
    assert_ne!(edited, original);

    editor.update(cx, |editor, _cx| editor.undo_document(_cx));
    editor.update(cx, |editor, _cx| editor.undo_document(_cx));
    redraw(cx);
    assert_eq!(
        editor.read_with(cx, |editor, _cx| editor.buffer.text()),
        original,
        "连撤两步没回到原文"
    );

    editor.update(cx, |editor, _cx| editor.redo_document(_cx));
    editor.update(cx, |editor, _cx| editor.redo_document(_cx));
    redraw(cx);
    assert_eq!(
        editor.read_with(cx, |editor, _cx| editor.buffer.text()),
        edited,
        "连重做两步没回到改动后的文本"
    );
}

/// 一次组合输入 = 一个撤销组：组合与提交的两条增量必须留在同一个组里。
///
/// 增量的区间记的是**写入当时**缓冲区里的字节位置。撤销要一步退到组合开始前，
/// 就得按提交、组合的反序依次重放；漏掉提交那条，第一条逆操作的区间就和当前
/// 缓冲区错位（轻则退不干净，重则端点落在多字节字符中间）。
#[gpui::test]
async fn an_ime_composition_undoes_and_redoes_as_one_step(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "笔记内容\n".to_string(), None));
    redraw(cx);
    let block = editor.read_with(cx, |editor, _cx| {
        editor.document.first_root().expect("paragraph").clone()
    });
    block.update(cx, |block, _cx| block.selected_range = 12..12);

    cx.update(|window, cx| {
        block.update(cx, |block, block_cx| {
            <Block as EntityInputHandler>::replace_and_mark_text_in_range(
                block,
                None,
                "nihao",
                Some(5..5),
                window,
                block_cx,
            );
        });
    });
    redraw(cx);
    cx.update(|window, cx| {
        block.update(cx, |block, block_cx| {
            <Block as EntityInputHandler>::replace_text_in_range(
                block,
                None,
                "你好啊",
                window,
                block_cx,
            );
        });
    });
    redraw(cx);

    let composed = editor.read_with(cx, |editor, _cx| editor.buffer.text());
    assert_eq!(composed, "笔记内容你好啊\n");
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(
            editor.undo_history.len(),
            1,
            "一次组合输入应该在撤销栈里只留一个组"
        );
    });

    editor.update(cx, |editor, cx| editor.undo_document(cx));
    assert_eq!(
        editor.read_with(cx, |editor, _cx| editor.buffer.text()),
        "笔记内容\n",
        "撤销一次组合输入没退到组合开始前"
    );

    editor.update(cx, |editor, cx| editor.redo_document(cx));
    assert_eq!(
        editor.read_with(cx, |editor, _cx| editor.buffer.text()),
        composed,
        "重做没回到提交后的文本"
    );
}

/// 撤销会重建整棵树：块区间必须重新指向缓冲区，否则下一次写回落在错的字节上。
///
/// 这是「块树只是投影」在撤销路径上的续集——撤销之后文档还是那份文档，
/// 但每个块都是新实体，区间只能从缓冲区重新算。
#[gpui::test]
async fn undo_reattaches_root_spans_so_the_next_edit_lands_right(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, LOSSY_DOC.to_string(), None));
    redraw(cx);

    cx.simulate_input("写");
    redraw(cx);
    cx.dispatch_action(Newline);
    redraw(cx);
    editor.update(cx, |editor, cx| editor.undo_document(cx));
    redraw(cx);

    let (spans, buffer_text) = root_block_spans(&editor, cx);
    assert_spans_tile_the_content(&span_ranges(&spans), &buffer_text, "撤销之后");

    // 再改一次：只动那一处，未编辑的块（表格列宽、`__` 写法）依旧原样。
    cx.simulate_input("入");
    redraw(cx);
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(
            editor.buffer.text(),
            LOSSY_DOC.replace("段落文字", "写入段落文字"),
            "撤销之后的第二次编辑改动到了别处"
        );
    });
}

/// 撤销「单元格里回车插出来的空段落」只该把那个空行收回去。
///
/// 插入本身是一条 `AppliedEdit`，撤销就是它的逆操作；顺手整篇重新序列化会把
/// 表格的列宽填充和 `__下划线__` 写法一起洗掉，磁盘上就不再是原文。
#[gpui::test]
async fn undoing_a_table_cell_newline_removes_the_blank_line_it_added(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let path = temp_markdown_path("undo-table-enter");
    fs::write(&path, LOSSY_DOC.replace('\n', "\r\n")).expect("write fixture");
    let original_disk = fs::read_to_string(&path).expect("read fixture");
    let cleanup = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup);
    });

    let document = encoding::load_document(&path).expect("read fixture");
    let open_path = path.clone();
    let (editor, cx) = cx.add_window_view(move |_window, cx| {
        Editor::from_loaded_document(cx, document, Some(open_path))
    });
    redraw(cx);

    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| {
            let table = editor
                .document
                .root_blocks()
                .iter()
                .find(|root| root.read(cx).kind() == BlockKind::Table)
                .cloned()
                .expect("夹具里应有一张表");
            let cell = table
                .read(cx)
                .table_runtime
                .as_ref()
                .and_then(|runtime| {
                    runtime.cell(crate::components::TableCellPosition { row: 1, column: 0 })
                })
                .expect("夹具里的表应有数据行的单元格");
            editor.on_block_event(
                cell.clone(),
                &BlockEvent::RequestNewline {
                    trailing: InlineTextTree::plain(String::new()),
                    source_already_mutated: false,
                },
                cx,
            );
        });
    });
    redraw(cx);

    editor.read_with(cx, |editor, _cx| {
        assert!(
            editor.buffer.text().contains("| 甲   | 1    |\n\n\n强调"),
            "回车没在表后插出空段落：{:?}",
            editor.buffer.text()
        );
    });

    editor.update(cx, |editor, cx| editor.undo_document(cx));
    redraw(cx);
    cx.simulate_keystrokes("ctrl-s");
    redraw(cx);

    let saved = fs::read_to_string(&path).expect("read saved file");
    assert_eq!(
        saved, original_disk,
        "撤销一次表内回车之后，磁盘上的字节不再是原文"
    );
}

/// 撤销「删掉整张表」要把那几行表原样放回去。
///
/// 删除走的是区间收行（旧的几行连它们自己的换行一起收掉），撤销就是把它换回去
/// 的那次 `AppliedEdit`；顺手整篇重投影会把列宽填充和 `__下划线__` 写法洗掉。
#[gpui::test]
async fn undoing_a_dropped_table_puts_the_rows_back_byte_for_byte(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let path = temp_markdown_path("undo-drop-table");
    fs::write(&path, LOSSY_DOC.replace('\n', "\r\n")).expect("write fixture");
    let original_disk = fs::read_to_string(&path).expect("read fixture");
    let cleanup = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup);
    });

    let document = encoding::load_document(&path).expect("read fixture");
    let open_path = path.clone();
    let (editor, cx) = cx.add_window_view(move |_window, cx| {
        Editor::from_loaded_document(cx, document, Some(open_path))
    });
    redraw(cx);

    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| {
            let table = editor
                .document
                .root_blocks()
                .iter()
                .find(|root| root.read(cx).kind() == BlockKind::Table)
                .cloned()
                .expect("夹具里应有一张表");
            editor.remove_table_block(&table, cx);
        });
    });
    redraw(cx);
    editor.read_with(cx, |editor, _cx| {
        assert!(
            !editor.buffer.text().contains("名称"),
            "删表没生效：{:?}",
            editor.buffer.text()
        );
    });

    editor.update(cx, |editor, cx| editor.undo_document(cx));
    redraw(cx);
    cx.simulate_keystrokes("ctrl-s");
    redraw(cx);

    let saved = fs::read_to_string(&path).expect("read saved file");
    assert_eq!(
        saved, original_disk,
        "撤销一次删表之后，磁盘上的字节不再是原文"
    );
}

/// 闸门（方案 §6.2.4）：撤销栈打满 200 步，内存必须停在 8 MB 以内。
///
/// 旧实现每步存一份全文快照，栈深 200 × 文档大小——10 MiB 文档最坏 2 GB。存增量
/// 之后每组只有「被换掉的字节 + 它现在占的区间」。这里用勾任务框当步子：它是
/// `NonCoalescible`（打字那组会在 1 秒合并窗口里并起来，真实时钟下测试没法拉开），
/// 而且每步只改 `[ ]`↔`[x]` 那几个字节。
#[gpui::test]
async fn two_hundred_undo_steps_stay_within_the_memory_budget(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let source = format!(
        "{}\n",
        (0..200)
            .map(|index| format!("- [ ] 任务 {index}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    assert!(source.len() > 2048, "夹具太小测不出整篇副本：{}", source.len());
    let source_len = source.len();
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, source, None));
    redraw(cx);

    for _ in 0..200 {
        let task = editor.read_with(cx, |editor, cx| {
            editor
                .document
                .root_blocks()
                .iter()
                .find(|root| matches!(root.read(cx).kind(), BlockKind::TaskListItem { .. }))
                .cloned()
                .expect("夹具里应有任务项")
        });
        dispatch_block_event(&editor, task, crate::components::BlockEvent::ToggleTaskChecked, cx);
        redraw(cx);
    }

    let stored = editor.read_with(cx, |editor, _cx| editor.undo_history_byte_len());
    let groups = editor.read_with(cx, |editor, _cx| editor.undo_history.len());
    eprintln!("[measure] {groups} 组撤销记了 {stored} 字节，文档 {source_len} 字节");
    assert_eq!(groups, 200, "撤销栈没打满，测不到最坏情况：{groups}");
    assert!(
        stored <= 8 * 1024 * 1024,
        "200 步撤销记了 {stored} 字节，超出 8 MB 预算：又在存整篇副本"
    );
    // 绝对上限：整篇副本的话这里是 200 × 文档大小，这条会先炸。
    assert!(
        stored <= 64 * 1024,
        "200 步撤销记了 {stored} 字节，文档才 {source_len} 字节：增量存大了"
    );
}

/// 撤销负载不该跟着文档大小长。
///
/// 同一串动作在小文档和大文档上记的字节必须差不多：增量的成本只与「改了哪些字节」
/// 有关。整篇副本做不到——文档大一倍，撤销栈就大一倍。
#[gpui::test]
async fn undo_memory_does_not_scale_with_document_size(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let stored_for = |cx: &mut TestAppContext, tasks: usize| -> usize {
        let source = format!(
            "{}\n",
            (0..tasks)
                .map(|index| format!("- [ ] 任务 {index}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
        let (editor, cx) =
            cx.add_window_view(|_window, cx| Editor::from_markdown(cx, source, None));
        redraw(cx);
        for _ in 0..12 {
            let task = editor.read_with(cx, |editor, cx| {
                editor
                    .document
                    .root_blocks()
                    .iter()
                    .find(|root| matches!(root.read(cx).kind(), BlockKind::TaskListItem { .. }))
                    .cloned()
                    .expect("夹具里应有任务项")
            });
            dispatch_block_event(&editor, task, crate::components::BlockEvent::ToggleTaskChecked, cx);
            redraw(cx);
        }
        editor.read_with(cx, |editor, _cx| editor.undo_history_byte_len())
    };

    let small = stored_for(cx, 20);
    let large = stored_for(cx, 4_000);
    eprintln!("[measure] 撤销负载：小文档 {small} 字节，200 倍任务数的文档 {large} 字节");
    assert_eq!(
        small, large,
        "文档大了 200 倍，撤销负载从 {small} 涨到 {large} 字节：又在按文档大小记账"
    );
}

fn dispatch_block_event(
    editor: &gpui::Entity<Editor>,
    block: gpui::Entity<crate::components::Block>,
    event: crate::components::BlockEvent,
    cx: &mut gpui::VisualTestContext,
) {
    editor.update(cx, |editor, cx| {
        editor.on_block_event(block, &event, cx);
    });
}
