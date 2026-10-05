//! 源码模式（按 512 行切成投影块）的跨块选区落地。
//!
//! 阶段 3d：旧写法把选区**钳进起点所在的那一根**块，跨块的选区在后面的块上
//! 一个字都不显示；同时 `snapshot.range.start - chunk_start` 在被
//! `source_mappings_in_range` 顺带返回的**区间外相邻块**上是下溢的。

use super::common::*;
use crate::editor::UndoSelectionSnapshot;
use gpui::App;

const CHUNK_LINES: usize = 512;

/// `n` 行 `line 1..n` 的定长文本，返回文本与「第 `chunk` 块起点」的字节偏移。
fn numbered_lines(n: usize) -> String {
    (1..=n).map(|i| format!("line {i}\n")).collect()
}

/// 第 `chunk` 根投影块（0 基）在文本里的起始字节 = 前 `chunk * CHUNK_LINES` 行的总长。
fn chunk_start(text: &str, chunk: usize) -> usize {
    text.split_inclusive('\n')
        .take(chunk * CHUNK_LINES)
        .map(|line| line.len())
        .sum()
}

fn selected_ranges(editor: &Editor, cx: &App) -> Vec<std::ops::Range<usize>> {
    editor
        .document
        .root_blocks()
        .iter()
        .map(|block| block.read(cx).selected_range.clone())
        .filter(|range| !range.is_empty())
        .collect()
}

#[gpui::test]
async fn a_source_selection_spanning_a_chunk_boundary_lands_on_both_chunks(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    let text = numbered_lines(1100);
    let boundary = chunk_start(&text, 1);
    let (editor, cx) = cx.add_window_view(|_, cx| {
        Editor::from_markdown(cx, text.clone(), None)
    });
    cx.run_until_parked();
    editor.update(cx, |editor, cx| {
        editor.toggle_view_mode(cx);
    });
    cx.run_until_parked();
    editor.read_with(cx, |editor, _cx| {
        assert!(matches!(editor.view_mode, ViewMode::Source));
        assert_eq!(
            editor.document.root_count(),
            3,
            "用例前提：1100 行按 512 行切成三根投影块"
        );
    });

    // 跨过第一道块界：前 3 字节属于第 1 块，后 3 字节属于第 2 块。
    editor.update(cx, |editor, cx| {
        editor.apply_selection_snapshot_in_current_mode(
            &UndoSelectionSnapshot {
                range: boundary - 3..boundary + 3,
                reversed: false,
            },
            cx,
        );
    });
    editor.read_with(cx, |editor, cx| {
        let selected = selected_ranges(editor, cx);
        assert_eq!(
            selected.len(),
            2,
            "跨块选区必须在两根投影块上都有内容（改前只落在起点那一根）：{selected:?}"
        );
        // 实测投影块的源码区间是「不含自己结尾换行」的：0..4499、4500..9132，
        // 字节 4499（第 512 行的换行）谁都不显示。所以选 6 个字节、两段加起来
        // 是 5 个——缺的那一个正是块与块之间那道换行，它不在任何一块上。
        assert_eq!(
            selected,
            vec![(boundary - 3)..(boundary - 1), 0..3],
            "第一块拿结尾那 2 字节，第二块从头拿 3 字节"
        );
        assert_eq!(
            editor.active_entity_id,
            Some(editor.document.root_blocks()[0].entity_id()),
            "焦点与活动块仍然归起点所在的那一根"
        );
    });
}

#[gpui::test]
async fn a_smaller_source_selection_clears_the_stale_chunks(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let text = numbered_lines(1100);
    let boundary = chunk_start(&text, 1);
    let tail_start = chunk_start(&text, 2);
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, text.clone(), None));
    cx.run_until_parked();
    editor.update(cx, |editor, cx| {
        editor.toggle_view_mode(cx);
    });
    cx.run_until_parked();

    // 先跨块选一次，再选一个只落在第三块里的小选区。
    editor.update(cx, |editor, cx| {
        editor.apply_selection_snapshot_in_current_mode(
            &UndoSelectionSnapshot {
                range: boundary - 3..boundary + 3,
                reversed: false,
            },
            cx,
        );
    });
    editor.update(cx, |editor, cx| {
        editor.apply_selection_snapshot_in_current_mode(
            &UndoSelectionSnapshot {
                range: tail_start + 1..tail_start + 4,
                reversed: false,
            },
            cx,
        );
    });
    editor.read_with(cx, |editor, cx| {
        let selected = selected_ranges(editor, cx);
        assert_eq!(
            selected.len(),
            1,
            "上一次跨块选区留在前两块的残留必须清掉：{selected:?}"
        );
        assert_eq!(selected[0].len(), 3);
    });
}

#[gpui::test]
async fn a_source_selection_past_the_end_of_the_file_clamps_instead_of_panicking(
    cx: &mut TestAppContext,
) {
    // 端点越界（撤销快照里留着文件已变短的区间）不能 panic，也不能把选区
    // 甩到别的块上去。
    init_editor_test_app(cx);
    let text = numbered_lines(1100);
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, text.clone(), None));
    cx.run_until_parked();
    editor.update(cx, |editor, cx| {
        editor.toggle_view_mode(cx);
    });
    cx.run_until_parked();
    let far = text.len() + 5_000;
    editor.update(cx, |editor, cx| {
        editor.apply_selection_snapshot_in_current_mode(
            &UndoSelectionSnapshot {
                range: text.len()..far,
                reversed: true,
            },
            cx,
        );
    });
    editor.read_with(cx, |editor, cx| {
        for range in selected_ranges(editor, cx) {
            assert!(
                range.start <= range.end,
                "钳完的选区不许反向：{range:?}"
            );
        }
    });
}

/// 快照里的偏移属于**上一份**内容：文档插过或删过字节之后，那个字节位可能正落在
/// 多字节字符中间，而选区落地的整条链路（行号、块区间换算）处处按字符边界走，
/// `TextBuffer::line_of` 会直接断言失败。两种视图都取整到边界，不许 panic。
#[gpui::test]
async fn a_snapshot_offset_inside_a_multibyte_character_clamps_instead_of_panicking(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    let text = "第一段内容。\n\n第二段内容。\n".to_string();
    // 「段」占三个字节，取它中间那一字节作偏移——正是外部改动把内容挪了一位之后
    // 旧偏移会变成的样子。
    let split = text.find('段').expect("fixture has 段") + 1;
    assert!(
        !text.is_char_boundary(split),
        "用例前提：{split} 应落在多字节字符中间"
    );

    for source_mode in [false, true] {
        let (editor, cx) =
            cx.add_window_view(|_, cx| Editor::from_markdown(cx, text.clone(), None));
        cx.run_until_parked();
        if source_mode {
            editor.update(cx, |editor, cx| editor.toggle_view_mode(cx));
            cx.run_until_parked();
        }
        editor.update(cx, |editor, cx| {
            editor.apply_selection_snapshot_in_current_mode(
                &UndoSelectionSnapshot {
                    range: split..split,
                    reversed: false,
                },
                cx,
            );
        });
        editor.read_with(cx, |editor, cx| {
            let landed = editor.capture_source_selection_snapshot(cx).range.start;
            assert!(
                editor.buffer.is_char_boundary(landed),
                "{}模式下落点 {landed} 仍不在字符边界上",
                if source_mode { "源码" } else { "渲染" }
            );
        });
    }
}

/// 源码模式取选区快照必须按**缓冲区偏移**记账：光标落在第 2 块之后，读回来的
/// 却还是「第一块的本地偏移」，等于把落点说成文件开头。撤销、切视图、外部改动
/// 重载都以这份快照为锚，取错一位就全错。
#[gpui::test]
async fn a_source_caret_in_a_later_chunk_captures_its_buffer_offset(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let text = numbered_lines(1100);
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, text.clone(), None));
    cx.run_until_parked();
    editor.update(cx, |editor, cx| {
        editor.toggle_view_mode(cx);
    });
    cx.run_until_parked();
    editor.read_with(cx, |editor, _cx| {
        assert!(matches!(editor.view_mode, ViewMode::Source));
        assert_eq!(
            editor.document.root_count(),
            3,
            "用例前提：1100 行按 512 行切成三根投影块"
        );
    });

    let caret = chunk_start(&text, 2) + 5;
    editor.update(cx, |editor, cx| {
        let block = editor.document.root_blocks()[2].clone();
        editor.active_entity_id = Some(block.entity_id());
        block.update(cx, |block, _cx| block.selected_range = 5..5);
    });
    let snapshot = editor.read_with(cx, |editor, cx| {
        editor.capture_source_selection_snapshot(cx)
    });
    assert_eq!(
        snapshot.range,
        caret..caret,
        "光标在第 3 块，快照要给出缓冲区偏移 {caret}，实测 {:?}",
        snapshot.range
    );
}

