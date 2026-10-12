use super::common::*;
use crate::editor::{TableTextPosition, TableTextSelection};
use crate::components::TableCellPosition;
use gpui::{Bounds, Entity};

#[gpui::test]
async fn core_repair_formula_cell_return_drag_keeps_the_atomic_selection(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let source = "alpha\n\n| text | $q$ |\n| --- | --- |\n| one | two |";
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, source.into(), None));
    redraw(cx);
    redraw(cx);
    let start = cell_bounds(&editor, cx, (0, 1)).center();
    cx.simulate_mouse_down(start, gpui::MouseButton::Left, Modifiers::none());
    redraw(cx);
    let outside = cell_text_start(cell_bounds(&editor, cx, (0, 0)));
    cx.simulate_mouse_move(outside, Some(gpui::MouseButton::Left), Modifiers::none());
    redraw(cx);
    let back = cell_text_start(cell_bounds(&editor, cx, (0, 1)));
    cx.simulate_mouse_move(back, Some(gpui::MouseButton::Left), Modifiers::none());
    cx.simulate_mouse_up(back, gpui::MouseButton::Left, Modifiers::none());
    redraw(cx);
    assert_eq!(editor.read_with(cx, |editor, cx| editor.selected_visible_text(cx)).as_deref(), Some("$q$"));
    cx.simulate_input("X");
    redraw(cx);
    assert_eq!(editor.read_with(cx, |editor, _| editor.buffer.text()), source.replace("$q$", "X"));
    editor.update(cx, |editor, cx| editor.undo_document(cx));
    assert_eq!(editor.read_with(cx, |editor, _| editor.buffer.text()), source);
}

#[gpui::test]
async fn core_repair_new_table_cells_route_input_from_an_empty_selection_anchor(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let source = format!("first\n\n| ab | cd |\n| --- | --- |\n| ef | gh |\n\n{}", (0..400).map(|index| format!("paragraph {index}\n\n")).collect::<String>());
    let cx = cx.add_empty_window();
    let editor = cx.new(|cx| Editor::from_markdown_with_chunk_budget(cx, source.clone(), None, 1));
    let cell = editor.update(cx, |editor, cx| {
        assert!(editor.materialize_next_pending_chunk(cx));
        assert!(editor.document.pending_tail().is_some());
        let table = editor.document.root_blocks().iter().find(|block| block.read(cx).kind() == BlockKind::Table).unwrap().clone();
        let cell = table.read(cx).table_runtime.as_ref().unwrap().header[0].clone();
        editor.select_table_text_range(table.entity_id(),
            TableTextPosition { cell: TableCellPosition { row: 0, column: 0 }, offset: 2, whole_cell: false },
            TableTextPosition { cell: TableCellPosition { row: 0, column: 1 }, offset: 1, whole_cell: false }, cx);
        editor.focus_block(cell.entity_id());
        cell
    });
    cell.update_in(cx, |cell, window, cx| cell.replace_text_in_range(None, "X", window, cx));
    redraw(cx);
    assert_eq!(editor.read_with(cx, |editor, _| editor.buffer.text()), source.replacen("| ab | cd |", "| ab | Xd |", 1));
}

#[gpui::test]
async fn core_repair_cross_cell_replacement_keeps_table_structure_and_input_target(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let source = "| ab | cd |\n| --- | --- |\n| ef | gh |";
    let mut mismatches = Vec::new();
    for (input, expected) in [("|", "| a\\| | d |\n| --- | --- |\n| ef | gh |"), ("X\nY", "| aX Y | d |\n| --- | --- |\n| ef | gh |"), ("中", "| a中 | d |\n| --- | --- |\n| ef | gh |")] {
        let (editor, window_cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, source.into(), None));
        redraw(window_cx);
        editor.update(window_cx, |editor, cx| {
            let table = editor.document.first_root().unwrap().clone();
            editor.select_table_text_range(table.entity_id(),
                TableTextPosition { cell: TableCellPosition { row: 0, column: 0 }, offset: 1, whole_cell: false },
                TableTextPosition { cell: TableCellPosition { row: 0, column: 1 }, offset: 1, whole_cell: false }, cx);
            let cell = table.read(cx).table_runtime.as_ref().unwrap().header[1].clone();
            editor.focus_block(cell.entity_id());
            assert!(editor.replace_cross_block_selection_with_text(input, None, false, UndoCaptureKind::NonCoalescible, cx));
        });
        redraw(window_cx);
        let actual = editor.read_with(window_cx, |editor, _| editor.buffer.text());
        if actual != expected { mismatches.push((input, actual, expected.to_string())); }
        window_cx.simulate_input("Z");
        redraw(window_cx);
        let actual = editor.read_with(window_cx, |editor, _| editor.buffer.text());
        let expected = expected.replacen(" | d |", "Z | d |", 1);
        if actual != expected { mismatches.push((input, actual, expected)); }
    }
    assert!(mismatches.is_empty(), "{mismatches:?}");
}

#[gpui::test]
async fn core_repair_cross_cell_preedit_keeps_relative_selection_and_composition(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let source = "| ab | cd |\n| --- | --- |\n| ef | gh |";
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, source.into(), None));
    redraw(cx);
    editor.update(cx, |editor, cx| {
        let table = editor.document.first_root().unwrap().clone();
        editor.select_table_text_range(table.entity_id(),
            TableTextPosition { cell: TableCellPosition { row: 0, column: 0 }, offset: 1, whole_cell: false },
            TableTextPosition { cell: TableCellPosition { row: 0, column: 1 }, offset: 1, whole_cell: false }, cx);
        assert!(editor.replace_cross_block_selection_with_text("ni", Some(1..1), true, UndoCaptureKind::ImeComposition, cx));
    });
    redraw(cx);
    let cell = editor.read_with(cx, |editor, cx| {
        let cell = editor.current_edit_target_from_state(cx).unwrap();
        assert_eq!(cell.read(cx).record.title.visible_text(), "ani");
        assert_eq!(cell.read(cx).selection_clean_range(), 2..2);
        assert_eq!(cell.read(cx).marked_range, Some(1..3));
        assert!(editor.has_marked_document_text(cx));
        cell
    });
    cell.update_in(cx, |cell, window, cx| cell.replace_text_in_range(None, "中", window, cx));
    redraw(cx);
    assert_eq!(editor.read_with(cx, |editor, _| editor.buffer.text()), "| a中 | d |\n| --- | --- |\n| ef | gh |");
    editor.update(cx, |editor, cx| editor.undo_document(cx));
    assert_eq!(editor.read_with(cx, |editor, _| editor.buffer.text()), source);
}

#[gpui::test]
async fn core_repair_formula_cell_drag_anchor_survives_focus_projection(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "alpha\n\n| text | $q$ |\n| --- | --- |\n| one | two |".into(), None));
    redraw(cx);
    redraw(cx);
    let start = cell_bounds(&editor, cx, (0, 1)).center();
    cx.simulate_mouse_down(start, gpui::MouseButton::Left, Modifiers::none());
    redraw(cx);
    let end = cell_text_start(cell_bounds(&editor, cx, (0, 0)));
    cx.simulate_mouse_move(end, Some(gpui::MouseButton::Left), Modifiers::none());
    redraw(cx);
    cx.simulate_mouse_up(end, gpui::MouseButton::Left, Modifiers::none());
    let text = editor.read_with(cx, |editor, cx| editor.selected_visible_text(cx)).unwrap();
    assert!(text.contains("$q$"), "聚焦后锚点仍需保留按下时的整格选择：{text:?}");
}

#[gpui::test]
async fn core_repair_offscreen_table_bounds_cannot_claim_a_mounted_cell(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let mut source = "| old |\n| --- |\n| cell |\n\n".to_string();
    source.push_str(&(0..160).map(|index| format!("paragraph {index}\n\n")).collect::<String>());
    source.push_str("| new |\n| --- |\n| cell |");
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, source, None));
    cx.run_until_parked();
    redraw(cx);
    redraw(cx);
    let (old, current) = editor.read_with(cx, |editor, _| (editor.document.first_root().unwrap().clone(), editor.document.root_blocks().last().unwrap().clone()));
    editor.update(cx, |editor, cx| {
        let cell = current.read(cx).table_runtime.as_ref().unwrap().header[0].clone();
        editor.focus_block(cell.entity_id());
        editor.request_active_block_scroll_into_view(cx);
    });
    for _ in 0..4 {
        editor.update(cx, |editor, _| {
            editor.pending_scroll_active_block_into_view = false;
            editor.pending_scroll_recheck_after_layout = false;
            editor.scroll_handle.set_offset(gpui::point(px(0.0), -editor.scroll_handle.max_offset().height));
        });
        redraw(cx);
    }
    assert!(!editor.read_with(cx, |editor, _| editor.entity_is_mounted(old.entity_id())), "旧表格需离开挂载集合");
    let (new_cell, point) = editor.read_with(cx, |_, cx| {
        let cell = current.read(cx).table_runtime.as_ref().unwrap().header[0].clone();
        let bounds = cell.read(cx).last_bounds.unwrap();
        (cell, bounds.center())
    });
    let old_cells = editor.read_with(cx, |_, cx| {
        let runtime = old.read(cx).table_runtime.as_ref().unwrap().clone();
        runtime.header.iter().chain(runtime.rows.iter().flatten()).cloned().collect::<Vec<_>>()
    });
    let new_bounds = new_cell.read_with(cx, |cell, _| cell.last_bounds.unwrap());
    for cell in old_cells { cell.update(cx, |cell, _| cell.last_bounds = Some(new_bounds)); }
    new_cell.update(cx, |cell, _| cell.last_bounds = None);
    assert!(editor.read_with(cx, |editor, cx| editor.table_cell_at_point(point, cx)).is_none(), "未挂载的旧格子不能参与命中");
    new_cell.update(cx, |cell, _| cell.last_bounds = Some(new_bounds));
    assert_eq!(editor.read_with(cx, |editor, cx| editor.table_cell_at_point(point, cx)).map(|hit| hit.0), Some(current.entity_id()));
}

#[gpui::test]
async fn core_repair_untitled_callout_header_does_not_swallow_body_hit(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "> [!NOTE]\n> first\n>\n> second".into(), None));
    redraw(cx);
    redraw(cx);
    let body = editor.read_with(cx, |editor, _| editor.document.visible_blocks().last().unwrap().entity.clone());
    let point = body.read_with(cx, |block, _| block.last_bounds.unwrap().center());
    cx.simulate_mouse_down(point, gpui::MouseButton::Left, Modifiers::none());
    assert_eq!(editor.read_with(cx, |editor, _| editor.cross_block_drag.unwrap().anchor.entity_id), body.entity_id());
    cx.simulate_mouse_up(point, gpui::MouseButton::Left, Modifiers::none());
}

#[gpui::test]
async fn core_repair_mixed_paragraph_does_not_keep_text_layout_after_blur(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "first\n\ntext $x$ end".into(), None));
    redraw(cx);
    redraw(cx);
    let (first, mixed) = editor.read_with(cx, |editor, _| (editor.document.root_blocks()[0].clone(), editor.document.root_blocks()[1].clone()));
    editor.update(cx, |editor, _| editor.focus_block(mixed.entity_id()));
    redraw(cx);
    redraw(cx);
    assert!(mixed.read_with(cx, |block, _| block.has_text_layout()));
    editor.update(cx, |editor, _| editor.focus_block(first.entity_id()));
    redraw(cx);
    redraw(cx);
    assert!(!mixed.read_with(cx, |block, _| block.has_text_layout()), "混合视觉呈现不能使用聚焦期间的旧文字布局");
}

#[gpui::test]
async fn core_repair_shift_click_returning_to_the_anchor_keeps_the_original_anchor(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "abc\n\ndef\n\nghi".into(), None));
    redraw(cx);
    redraw(cx);
    let first = editor.read_with(cx, |editor, cx| editor.document.visible_blocks()[0].entity.read(cx).last_bounds.unwrap());
    let start = gpui::point(first.left() + px(1.0), first.center().y);
    cx.simulate_mouse_down(start, gpui::MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(start, gpui::MouseButton::Left, Modifiers::none());
    redraw(cx);
    let mut shift = Modifiers::none();
    shift.shift = true;
    let third = editor.read_with(cx, |editor, cx| editor.document.visible_blocks()[2].entity.read(cx).last_bounds.unwrap());
    let end = gpui::point(third.left() + px(1.0), third.center().y);
    cx.simulate_mouse_down(end, gpui::MouseButton::Left, shift);
    cx.simulate_mouse_up(end, gpui::MouseButton::Left, shift);
    redraw(cx);
    assert!(editor.read_with(cx, |editor, _| editor.cross_block_selection.is_some()));
    let first = editor.read_with(cx, |editor, cx| editor.document.visible_blocks()[0].entity.read(cx).last_bounds.unwrap());
    let back = gpui::point(first.right() - px(1.0), first.center().y);
    cx.simulate_mouse_down(back, gpui::MouseButton::Left, shift);
    cx.simulate_mouse_up(back, gpui::MouseButton::Left, shift);
    redraw(cx);
    assert_eq!(editor.read_with(cx, |editor, cx| editor.selected_visible_text(cx)).as_deref(), Some("abc"));
    cx.simulate_input("X");
    redraw(cx);
    assert_eq!(editor.read_with(cx, |editor, _| editor.buffer.text()), "X\n\ndef\n\nghi");
}

#[gpui::test]
async fn core_repair_cross_cell_undo_restores_the_logical_text_selection(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let source = "| ab | cd |\n| --- | --- |\n| ef | gh |";
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, source.into(), None));
    redraw(cx);
    editor.update(cx, |editor, cx| {
        let table = editor.document.first_root().unwrap().clone();
        editor.select_table_text_range(table.entity_id(),
            TableTextPosition { cell: TableCellPosition { row: 0, column: 0 }, offset: 1, whole_cell: false },
            TableTextPosition { cell: TableCellPosition { row: 0, column: 1 }, offset: 1, whole_cell: false }, cx,
        );
        let cell = table.read(cx).table_runtime.as_ref().unwrap().header[1].clone();
        editor.focus_block(cell.entity_id());
        assert_eq!(editor.table_text_selection_text(cx).as_deref(), Some("b\tc"));
        assert!(editor.replace_table_text_selection_with_text("", None, false, UndoCaptureKind::NonCoalescible, cx));
    });
    redraw(cx);
    editor.update(cx, |editor, cx| editor.undo_document(cx));
    redraw(cx);
    assert_eq!(editor.read_with(cx, |editor, _| editor.buffer.text()), source);
    assert_eq!(editor.read_with(cx, |editor, cx| editor.table_text_selection_text(cx)).as_deref(), Some("b\tc"));
    cx.dispatch_action(crate::components::Copy);
    assert_eq!(cx.update(|_, cx| cx.read_from_clipboard().and_then(|item| item.text())).as_deref(), Some("b\tc"));
}

#[gpui::test]
async fn core_repair_cross_cell_selection_survives_a_source_view_round_trip(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let source = "| ab | cd |\n| --- | --- |\n| ef | gh |";
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, source.into(), None));
    redraw(cx);
    editor.update(cx, |editor, cx| {
        let table = editor.document.first_root().unwrap().clone();
        editor.select_table_text_range(table.entity_id(),
            TableTextPosition { cell: TableCellPosition { row: 0, column: 1 }, offset: 1, whole_cell: false },
            TableTextPosition { cell: TableCellPosition { row: 0, column: 0 }, offset: 1, whole_cell: false }, cx,
        );
        editor.toggle_view_mode(cx);
    });
    redraw(cx);
    editor.update(cx, |editor, cx| editor.toggle_view_mode(cx));
    redraw(cx);
    editor.read_with(cx, |editor, cx| {
        assert_eq!(editor.table_text_selection_text(cx).as_deref(), Some("b\tc"));
        let selection = editor.table_text_selection.unwrap();
        assert_eq!(selection.anchor.cell.column, 1);
        assert_eq!(selection.focus.cell.column, 0);
    });
}

#[gpui::test]
async fn review_arrow_clears_cross_cell_selection_before_delete(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(
        cx, "| ab | cd |\n| --- | --- |\n| ef | gh |".into(), None,
    ));
    redraw(cx);
    cx.update(|window, cx| editor.update(cx, |editor, cx| {
        let table = editor.document.first_root().unwrap().clone();
        let cell = table.read(cx).table_runtime.as_ref().unwrap().header[1].clone();
        cell.update(cx, |cell, cx| { cell.move_to(2, cx); cell.focus_handle.focus(window); });
        editor.active_entity_id = Some(cell.entity_id());
        editor.table_text_selection = Some(TableTextSelection {
            table_block_id: table.entity_id(),
            anchor: TableTextPosition { cell: TableCellPosition { row: 0, column: 0 }, offset: 0, whole_cell: false },
            focus: TableTextPosition { cell: TableCellPosition { row: 0, column: 1 }, offset: 2, whole_cell: false },
        });
        editor.sync_table_text_selection_visuals(cx);
    }));
    redraw(cx);
    cx.dispatch_action(crate::components::MoveLeft);
    redraw(cx);
    assert!(editor.read_with(cx, |editor, _| editor.table_text_selection.is_none()));
    cx.dispatch_action(crate::components::DeleteBack);
    redraw(cx);
    assert_eq!(editor.read_with(cx, |editor, _| editor.buffer.text()), "| ab | d |\n| --- | --- |\n| ef | gh |");
}

#[gpui::test]
async fn review_cut_keeps_clipboard_when_table_deletion_has_no_mapping(cx: &mut TestAppContext) {
    // 映射不可用时剪切应保留文档与剪贴板，旧路径先复制再忽略删除失败。
    init_editor_test_app(cx);
    let source = "| ab | cd |\n| --- | --- |\n| ef | gh |";
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, source.into(), None));
    cx.update(|window, cx| editor.update(cx, |editor, cx| {
        let table = editor.document.first_root().unwrap().clone();
        editor.table_text_selection = Some(TableTextSelection {
            table_block_id: table.entity_id(),
            anchor: TableTextPosition { cell: TableCellPosition { row: 0, column: 0 }, offset: 0, whole_cell: false },
            focus: TableTextPosition { cell: TableCellPosition { row: 0, column: 1 }, offset: 2, whole_cell: false },
        });
        editor.document.clear_source_span(table.entity_id());
        cx.write_to_clipboard(gpui::ClipboardItem::new_string("保留".into()));
        editor.on_cut_capture(&crate::components::Cut, window, cx);
        assert_eq!(editor.buffer.text(), source);
        assert_eq!(cx.read_from_clipboard().and_then(|item| item.text()).as_deref(), Some("保留"));
    }));
}

#[gpui::test]
async fn review_copy_inside_one_table_cell_uses_only_selected_text(cx: &mut TestAppContext) {
    // 格内选区曾不参与选区读取，复制为 Markdown 因而退回整篇文档。
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(
        cx, "| alpha 其他 |\n| --- |\n| 正文 |".into(), None,
    ));
    editor.update(cx, |editor, cx| {
        let cell = editor.document.first_root().unwrap().read(cx).table_runtime.as_ref().unwrap().header[0].clone();
        cell.update(cx, |cell, _| cell.selected_range = 0..5);
        editor.active_entity_id = Some(cell.entity_id());
        assert_eq!(editor.selected_visible_text(cx).as_deref(), Some("alpha"));
        assert_eq!(editor.selected_markdown_text(cx).as_deref(), Some("alpha"));
        assert!(editor.copy_as_markdown(cx));
        assert_eq!(cx.read_from_clipboard().and_then(|item| item.text()).as_deref(), Some("alpha"));
    });
}

#[gpui::test]
async fn dragging_inside_a_rendered_code_block_does_not_panic(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    // 在一篇长 markdown（含多个 fence）里，把鼠标放到 ``​`text`` 代码块内部
    // 按住拖动选中曾导致 coredump。这里用同构文档真实模拟。
    let markdown = r#"# 基于 opencode 插件的自定义上下文压缩

## 1. 背景与问题

随着任务复杂度上升，超长会话成为语料治理的核心痛点：单行超长、调试长尾。

## 5. 压缩会话的语料处理：按真实压缩点拆行

压缩解决了运行期问题，但采集侧必须处理「一行跨压缩点全量历史」的超长 + 失真问题。

segment 的重建规则（对齐真实重启上下文）：

```text
seg_1 = [system] + 原始消息序列 + 压缩响应（清洗为纯摘要：从 <stage_summary> 截取）

seg_k (k ≥ 2) = [system]
      + [user: 原始任务 + [RESUME NOTICE] + 上一段阶段摘要]
      + 保留尾部消息（tail_start_id … 压缩宿主消息）
      + 该段内的续作消息
```

配套细节：RESUME NOTICE 明确告知「此前已执行 N 轮、因上下文限制做过阶段压缩」。

```json
{
  "compaction": {
    "auto": true,
    "reserved": 10000,
    "preserve_recent_tokens": 15000
  }
}
```

## 8. 总结

方案核心是把压缩从「运行时兜底机制」升级为「可设计、可治理的一等公民」。
"#;
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, markdown.to_string(), None));
    redraw(cx);
    redraw(cx);

    // 找到装着 seg_1 的代码块。
    let (_code_block, bounds) = editor.read_with(cx, |editor, cx| {
        let visible = editor.document.visible_blocks().to_vec();
        for entry in visible {
            let block = entry.entity.clone();
            let state = block.read(cx);
            if state.display_text().contains("seg_1 = [system]") {
                let bounds = state
                    .last_bounds
                    .expect("the code block has layout bounds");
                return (block, bounds);
            }
        }
        panic!("the fenced block with seg_1 was not found");
    });

    // 在代码块内部做多组按住拖动：点进块→聚焦→按行往下/往上拖。
    for (start_ratio, step) in [
        (0.12_f32, 7.0_f32),
        (0.50, 7.0),
        (0.85, -7.0),
        (0.30, 3.0),
    ] {
        let start = gpui::point(
            bounds.left() + px(37.5),
            bounds.top() + bounds.size.height * start_ratio,
        );
        cx.simulate_mouse_down(start, gpui::MouseButton::Left, Modifiers::none());
        redraw(cx);
        let mut position = start;
        for _ in 0..12 {
            position = gpui::point(position.x + px(4.25), position.y + px(step));
            cx.simulate_mouse_move(position, gpui::MouseButton::Left, Modifiers::none());
            redraw(cx);
        }
        cx.simulate_mouse_up(position, gpui::MouseButton::Left, Modifiers::none());
        redraw(cx);
    }

    editor.read_with(cx, |editor, _cx| {
        assert!(
            editor.prev_mounted_run.is_some(),
            "拖动后编辑器仍然在渲染"
        );
    });
}

#[gpui::test]
async fn dragging_inside_a_paragraph_without_prior_focus_selects(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    // 想去选另一段的文字，直接拖动也必须起选区：落点在没聚焦的块上时曾只放
    // 光标、不起选区，要先单击那一段、第二次才拖得动。
    let markdown = "alpha one beta\n\nsecond paragraph here\n";
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, markdown.to_string(), None));
    redraw(cx);
    redraw(cx);

    // 先单击第一段：焦点明确落在第 0 块上，第二段保持未聚焦。
    let (first, first_center) = editor.read_with(cx, |editor, cx| {
        let block = editor.document.visible_blocks()[0].entity.clone();
        let bounds = block.read(cx).last_bounds.expect("第一段该有布局边界");
        (block.entity_id(), bounds.center())
    });
    cx.simulate_click(first_center, Modifiers::none());
    redraw(cx);

    let second = editor.read_with(cx, |editor, _cx| {
        editor.document.visible_blocks()[1].entity.clone()
    });
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(
            editor.active_entity_id,
            Some(first),
            "前置条件没搭好：单击第一段之后编辑目标不是第一段"
        );
    });

    let (start, end) = second.read_with(cx, |block, _cx| {
        let bounds = block.last_bounds.expect("第二段该有布局边界");
        let y = bounds.top() + bounds.size.height * 0.5;
        (
            gpui::point(bounds.left() + px(6.0), y),
            gpui::point(bounds.left() + px(72.0), y),
        )
    });
    cx.simulate_mouse_down(start, gpui::MouseButton::Left, Modifiers::none());
    redraw(cx);
    cx.simulate_mouse_move(end, gpui::MouseButton::Left, Modifiers::none());
    redraw(cx);
    cx.simulate_mouse_up(end, gpui::MouseButton::Left, Modifiers::none());
    redraw(cx);

    second.read_with(cx, |block, _cx| {
        assert!(
            !block.selected_range.is_empty(),
            "在没聚焦的段落里按下拖动该选出文字，实际选区是 {:?}",
            block.selected_range
        );
    });
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(
            editor.active_entity_id,
            Some(second.entity_id()),
            "拖完之后编辑目标该跟着换到被拖的那一段"
        );
    });
    assert!(
        cx.debug_bounds("editor-selection-toolbar").is_some(),
        "选出别段的文字之后，选中工具栏也该浮出来"
    );
}

#[gpui::test]
async fn mouse_over_a_block_whose_text_shrank_since_layout_does_not_panic(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    // 指针定位用上一帧的 last_layout 行序去索引当前文本的行范围。文本在布局之后
    // 变短（后台续建、外部修改、编辑）时 ranges[line_idx] 会越界 panic，而
    // panic = abort，就是用户看到的 coredump。
    let markdown = "# 标题\n\n第一段中文\n\n第二段中文\n\n第三段中文\n";
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        let editor = Editor::from_markdown(cx, markdown.to_string(), None);
        editor
    });
    // 源码模式：整篇就是一个多行块，而且它是聚焦的（未聚焦的代码块不挂文本元素，
    // last_layout 为空，跑不到越界分支）。
    editor.update(cx, |editor, cx| editor.toggle_view_mode(cx));
    redraw(cx);
    redraw(cx);

    let (block, bounds) = editor.read_with(cx, |editor, cx| {
        let block = editor.document.visible_blocks()[0].entity.clone();
        let state = block.read(cx);
        let lines = state
            .last_layout
            .as_ref()
            .map(|lines| lines.len())
            .unwrap_or(0);
        assert!(
            lines >= 4,
            "前置条件：源码模式下的多行布局，实际 last_layout = {lines} 行"
        );
        (
            block,
            state.last_bounds.expect("the source block has layout bounds"),
        )
    });

    // 布局之后把多行缩成一行，且不再重绘（真实运行时事件总是先于下一帧到达）。
    block.update(cx, |block, cx| {
        let len = block.visible_len();
        block.replace_text_in_visible_range(0..len, "只剩一行", None, false, cx);
    });

    // 指针仍落在旧布局的第二行高度上。
    let stale_line = gpui::point(bounds.left() + px(20.0), bounds.top() + px(30.0));
    let (offset, len) = block.read_with(cx, |block, _cx| {
        (block.index_for_mouse_position(stale_line), block.visible_len())
    });
    assert!(
        offset <= len,
        "指针偏移必须落在当前文本内：offset={offset}, len={len}"
    );
}

/// 缺陷：按住左键拖动能把选区拉过段界，但「点一下、按住 shift 再点另一段」在抬起时
/// 选区仍是空的——只有同段内的 shift 扩选（块自己的那条路）能用。
#[gpui::test]
async fn shift_click_selects_across_paragraphs(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "第一段正文\n\n第二段正文\n\n第三段正文".into(), None)
    });
    redraw(cx);
    redraw(cx);

    let first = editor.read_with(cx, |editor, cx| {
        editor.document.visible_blocks()[0]
            .entity
            .read(cx)
            .last_bounds
            .expect("段落该有布局边界")
    });
    let in_first = gpui::point(first.left() + px(1.0), first.center().y);
    cx.simulate_mouse_down(in_first, gpui::MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(in_first, gpui::MouseButton::Left, Modifiers::none());
    redraw(cx);

    let third = editor.read_with(cx, |editor, cx| {
        editor.document.visible_blocks()[2]
            .entity
            .read(cx)
            .last_bounds
            .expect("段落该有布局边界")
    });
    let mut shift = Modifiers::none();
    shift.shift = true;
    let in_third = gpui::point(third.left() + px(1.0), third.center().y);
    cx.simulate_mouse_down(in_third, gpui::MouseButton::Left, shift);
    cx.simulate_mouse_up(in_third, gpui::MouseButton::Left, shift);
    redraw(cx);

    editor.read_with(cx, |editor, cx| {
        let selection = editor
            .cross_block_selection
            .expect("跨段 shift+点应当成选区");
        let visible = editor.document.visible_blocks();
        assert_eq!(selection.anchor.entity_id, visible[0].entity.entity_id());
        assert_eq!(selection.focus.entity_id, visible[2].entity.entity_id());
        assert_eq!(selection.anchor.offset, 0, "锚点该停在第一段的该点处");
        let text = editor.selected_visible_text(cx).unwrap_or_default();
        assert!(text.contains("第一段正文"), "第一段该进选区：{text:?}");
        assert!(text.contains("第二段正文"), "中间那一段该进选区：{text:?}");
    });
}

#[gpui::test]
async fn dragging_the_left_button_across_cjk_blocks_keeps_selecting(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    // 左键按住拖动选择文本必须流畅且不崩。拖动路径上每次 mouse_move
    // 都会重新定位端点并同步全部可见块的选中样式；中文文档还要跨 UTF-8 字符
    // 边界取偏移。这个用例真的模拟按住拖动。
    let markdown = (0..400)
        .map(|index| {
            format!(
                "## 第 {index} 节标题\n\n这是第 {index} 段中文正文，足够长以便换行，含标点与英文 mixed text。\n"
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let (editor, cx) = cx.add_window_view(|_window, cx| Editor::from_markdown(cx, markdown, None));
    redraw(cx);
    redraw(cx);

    let first_bounds = editor.read_with(cx, |editor, cx| {
        editor.document.visible_blocks()[0]
            .entity
            .read(cx)
            .last_bounds
            .expect("the first block has layout bounds")
    });

    let start = gpui::point(first_bounds.left() + px(37.5), first_bounds.center().y);
    cx.simulate_mouse_down(start, gpui::MouseButton::Left, Modifiers::none());
    redraw(cx);

    // 按住往下拖：每步都走一次 on_editor_mouse_move / 块内 select_to。
    let mut position = start;
    for _ in 0..60 {
        position = gpui::point(position.x + px(6.25), position.y + px(11.0));
        cx.simulate_mouse_move(position, gpui::MouseButton::Left, Modifiers::none());
        redraw(cx);
    }
    cx.simulate_mouse_up(position, gpui::MouseButton::Left, Modifiers::none());
    redraw(cx);

    editor.read_with(cx, |editor, _cx| {
        assert!(
            editor.cross_block_selection.is_some(),
            "按住拖动后应当有跨块选择"
        );
    });
}

/// 表格文档：一段正文 + 一张两列两行的表 + 一段正文（表里的字是 a b / c d）。
const TABLE_DOC: &str = "alpha\n\n| a | b |\n| --- | --- |\n| c | d |\n\ngamma";

/// 文档里那张表格块。
fn table_entity(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> Entity<Block> {
    editor.read_with(cx, |editor, cx| {
        editor
            .document
            .visible_blocks()
            .iter()
            .find(|visible| visible.entity.read(cx).kind() == BlockKind::Table)
            .expect("文档里该有一张表")
            .entity
            .clone()
    })
}

/// 表格的每一个格子：表头行在前，数据行随后。
fn table_cells(cx: &gpui::App, table: &Entity<Block>) -> Vec<Entity<Block>> {
    let runtime = table.read(cx).table_runtime.clone().expect("表格运行时");
    runtime
        .header
        .iter()
        .chain(runtime.rows.iter().flatten())
        .cloned()
        .collect()
}

/// `cell_position`（0 是表头行）那一格的当前布局边界。
///
/// 表格自己没有文本元素，几何全在格子上；`.last_bounds` 只有画过之后才有值。
fn cell_bounds(
    editor: &Entity<Editor>,
    cx: &mut VisualTestContext,
    cell_position: (usize, usize),
) -> Bounds<gpui::Pixels> {
    editor.read_with(cx, |editor, cx| cell_bounds_in(editor, cx, cell_position))
}

fn cell_bounds_in(
    editor: &Editor,
    cx: &gpui::App,
    cell_position: (usize, usize),
) -> Bounds<gpui::Pixels> {
    let table = editor
        .document
        .visible_blocks()
        .iter()
        .find(|visible| visible.entity.read(cx).kind() == BlockKind::Table)
        .expect("文档里该有一张表")
        .entity
        .clone();
    let runtime = table.read(cx).table_runtime.clone().expect("表格运行时");
    let (row, column) = cell_position;
    let cell = if row == 0 {
        runtime.header[column].clone()
    } else {
        runtime.rows[row - 1][column].clone()
    };
    cell.read(cx).last_bounds.expect("这一格该有布局边界")
}

/// 格子文字的左/右边缘：`index_for_mouse_position` 在格子的左端给 0、右端给格尾，
/// 用它拿确定的偏移，不去猜测试文本系统里一个字符有多宽。
fn cell_text_start(bounds: Bounds<gpui::Pixels>) -> gpui::Point<gpui::Pixels> {
    gpui::point(bounds.left() + px(1.0), bounds.center().y)
}

fn cell_text_end(bounds: Bounds<gpui::Pixels>) -> gpui::Point<gpui::Pixels> {
    gpui::point(bounds.right() - px(1.0), bounds.center().y)
}

/// 按下 → 拖过中间的落点 → 抬手。
fn drag_across(
    cx: &mut VisualTestContext,
    start: gpui::Point<gpui::Pixels>,
    waypoints: &[gpui::Point<gpui::Pixels>],
    end: gpui::Point<gpui::Pixels>,
) {
    cx.simulate_mouse_down(start, gpui::MouseButton::Left, Modifiers::none());
    redraw(cx);
    for waypoint in waypoints {
        cx.simulate_mouse_move(*waypoint, gpui::MouseButton::Left, Modifiers::none());
        redraw(cx);
    }
    cx.simulate_mouse_move(end, gpui::MouseButton::Left, Modifiers::none());
    redraw(cx);
    cx.simulate_mouse_up(end, gpui::MouseButton::Left, Modifiers::none());
    redraw(cx);
}

/// 每一格上画着的选中高亮（没高亮就是 `None`）。
fn cell_selection_ranges(
    cx: &gpui::App,
    table: &Entity<Block>,
) -> Vec<Option<std::ops::Range<usize>>> {
    table_cells(cx, table)
        .into_iter()
        .map(|cell| cell.read(cx).editor_selection_clean_range.clone())
        .collect()
}

/// 按住拖动必须能选上表格里的文字，而不是只选出一个格子里的文字。
///
/// 现象：从 a 拖到 d，抬手后只有按下那一格自己的文字带着选中色，别的格子毫无反应。
/// 根因：表格块没有自己的文本元素（格子才是文本），`last_bounds` 一直是空，落点换算
/// 看不见这张表——表里任何一点都算到别的块的端点上，与按下时的端点同块同偏移，
/// `on_editor_mouse_move` 的同块早退把选区一直压着不建。
///
/// 口径：表格就当成一片连着的文本，从 a 拖到 d 就是 a b c d 四个字选中；复制出去
/// 同行的格用制表符接、行与行之间换行。
#[gpui::test]
async fn dragging_across_table_cells_selects_that_text(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, TABLE_DOC.to_string(), None)
    });
    redraw(cx);
    redraw(cx);

    let table = table_entity(&editor, cx);
    let start = cell_text_start(cell_bounds(&editor, cx, (0, 0)));
    let end = cell_text_end(cell_bounds(&editor, cx, (1, 1)));
    drag_across(cx, start, &[], end);

    editor.read_with(cx, |editor, cx| {
        let selection = editor
            .table_text_selection
            .expect("从 a 拖到 d 应当选出这几个字");
        assert_eq!(selection.table_block_id, table.entity_id());
        assert_eq!((selection.anchor.cell.row, selection.anchor.cell.column), (0, 0));
        assert_eq!((selection.focus.cell.row, selection.focus.cell.column), (1, 1));
        assert!(
            editor.cross_block_selection.is_none(),
            "跨格文本选区不该同时挂着跨块选区：{:?}",
            editor.cross_block_selection
        );
        assert_eq!(
            editor.table_text_selection_text(cx).as_deref(),
            Some("a\tb\nc\td"),
            "a b c d 四个字都该在选区里，同行用制表符接、换行分行"
        );
        assert_eq!(
            cell_selection_ranges(cx, &table),
            vec![
                Some(0..1),
                Some(0..1),
                Some(0..1),
                Some(0..1)
            ],
            "四个格子都该画出选中高亮"
        );
    });
}

/// 「选了多少就是多少」：只选到半格时，起点格与落点格只高亮选到的那一段。
#[gpui::test]
async fn cross_cell_selection_keeps_text_granularity(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let markdown = "| ab | cd |\n| --- | --- |\n| ef | gh |";
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, markdown.to_string(), None)
    });
    redraw(cx);

    editor.update(cx, |editor, cx| {
        let table = editor.document.visible_blocks()[0].entity.clone();
        editor.table_text_selection = Some(TableTextSelection {
            table_block_id: table.entity_id(),
            // 从第一格的第二个字（b）选到最后一格的第一个字（g）。
            anchor: TableTextPosition {
                cell: TableCellPosition { row: 0, column: 0 },
                offset: 1,
                whole_cell: false,
            },
            focus: TableTextPosition {
                cell: TableCellPosition { row: 1, column: 1 },
                offset: 1,
                whole_cell: false,
            },
        });
        editor.sync_table_text_selection_visuals(cx);
        assert_eq!(
            editor.table_text_selection_text(cx).as_deref(),
            Some("b\tcd\nef\tg"),
            "半格的端点只带走选到的那几个字"
        );
        assert_eq!(
            cell_selection_ranges(cx, &table),
            vec![Some(1..2), Some(0..2), Some(0..2), Some(0..1)],
            "起点格从起点选到格尾、落点格从格首选到落点、中间的整格"
        );
    });
}

/// 文字粒度：起点停在某一格的字尾、落点停在另一格的字首时，这两格的字不算选中。
///
/// 报修筑到「格子粒度」时这一条会红：那时只要碰到一格就把整格选上，a 和 d 也会跟着带走。
#[gpui::test]
async fn dragging_between_cell_edges_keeps_the_text_granularity(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, TABLE_DOC.to_string(), None)
    });
    redraw(cx);
    redraw(cx);

    let table = table_entity(&editor, cx);
    let start = cell_text_end(cell_bounds(&editor, cx, (0, 0)));
    let end = cell_text_start(cell_bounds(&editor, cx, (1, 1)));
    drag_across(cx, start, &[], end);

    editor.read_with(cx, |editor, cx| {
        assert_eq!(
            editor.table_text_selection_text(cx).as_deref(),
            Some("b\nc"),
            "从 a 的字尾拉到 d 的字首，只带走 b 与 c"
        );
        assert_eq!(
            cell_selection_ranges(cx, &table),
            vec![None, Some(0..1), Some(0..1), None],
            "边界那两格只算格内真正的选中段：a 与 d 都不算"
        );
    });
}

/// 一格之内的拖动仍然是这一格自己的块内选区：不能因为落点在表格里就把整片格子选上。
#[gpui::test]
async fn dragging_inside_one_table_cell_keeps_the_selection_local(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, TABLE_DOC.to_string(), None)
    });
    redraw(cx);
    redraw(cx);

    let table = table_entity(&editor, cx);
    let cell = editor.read_with(cx, |editor, cx| {
        let table_block = editor.document.visible_blocks()[1].entity.clone();
        table_block
            .read(cx)
            .table_runtime
            .as_ref()
            .expect("表格运行时")
            .rows[0][0]
            .clone()
    });
    let bounds = cell_bounds(&editor, cx, (1, 0));

    drag_across(
        cx,
        cell_text_start(bounds),
        &[],
        gpui::point(bounds.left() + px(24.0), bounds.center().y),
    );

    editor.read_with(cx, |editor, cx| {
        assert!(
            editor.table_text_selection.is_none(),
            "一格之内的拖动不该变成跨格选区：{:?}",
            editor.table_text_selection
        );
        assert!(
            editor.cross_block_selection.is_none(),
            "一格之内的拖动不该变成跨块选区"
        );
        assert!(
            !cell.read(cx).selected_range.is_empty(),
            "一格之内的拖动该由格子自己选出文字，实际选区是 {:?}",
            cell.read(cx).selected_range
        );
        assert_eq!(
            cell_selection_ranges(cx, &table),
            vec![None, None, None, None],
            "没有跨格选区时格子不该带选中高亮"
        );
    });
}

/// 点别处要能把跨格选区（连同格子上的高亮）收起。
#[gpui::test]
async fn clicking_elsewhere_clears_the_cross_cell_selection(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, TABLE_DOC.to_string(), None)
    });
    redraw(cx);
    redraw(cx);

    let table = table_entity(&editor, cx);
    let start = cell_text_start(cell_bounds(&editor, cx, (0, 0)));
    let end = cell_text_end(cell_bounds(&editor, cx, (1, 1)));
    drag_across(cx, start, &[], end);

    let paragraph = editor.read_with(cx, |editor, cx| {
        editor.document.visible_blocks()[0]
            .entity
            .read(cx)
            .last_bounds
            .expect("上面那一段该有布局边界")
    });
    let away = gpui::point(paragraph.left() + px(4.0), paragraph.center().y);
    cx.simulate_mouse_down(away, gpui::MouseButton::Left, Modifiers::none());
    redraw(cx);
    cx.simulate_mouse_up(away, gpui::MouseButton::Left, Modifiers::none());
    redraw(cx);

    editor.read_with(cx, |editor, cx| {
        assert!(
            editor.table_text_selection.is_none(),
            "点别处之后跨格选区该收起来：{:?}",
            editor.table_text_selection
        );
        assert_eq!(
            cell_selection_ranges(cx, &table),
            vec![None, None, None, None],
            "点别处之后格子上的高亮该收回去"
        );
    });
}

/// 删掉跨格选区：选中的那几段字没，表格结构与没选中的格子不动。
#[gpui::test]
async fn deleting_a_cross_cell_selection_clears_those_texts(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, TABLE_DOC.to_string(), None)
    });
    redraw(cx);
    redraw(cx);

    // 选整个表头行（a 与 b）。
    let start = cell_text_start(cell_bounds(&editor, cx, (0, 0)));
    let end = cell_text_end(cell_bounds(&editor, cx, (0, 1)));
    drag_across(cx, start, &[], end);
    editor.read_with(cx, |editor, cx| {
        assert_eq!(
            editor.table_text_selection_text(cx).as_deref(),
            Some("a\tb"),
            "前置条件：表头行那两个格子都在选区里"
        );
    });

    cx.dispatch_action(DeleteBack);
    redraw(cx);

    editor.read_with(cx, |editor, cx| {
        assert!(
            editor.table_text_selection.is_none(),
            "删完选区就该收起来：{:?}",
            editor.table_text_selection
        );
        assert_eq!(
            editor.document.markdown_text(cx),
            "alpha\n\n|  |  |\n| --- | --- |\n| c | d |\n\ngamma",
            "只该清掉表头行那两个字"
        );
    });

    editor.update(cx, |editor, cx| editor.undo_document(cx));
    redraw(cx);
    editor.read_with(cx, |editor, cx| {
        assert_eq!(
            editor.document.markdown_text(cx).trim(),
            TABLE_DOC,
            "一步撤销该把删掉的字原样放回来"
        );
    });
}

/// 直接敲字也要换掉跨格选区里的字（不能把这一次输入吞掉）。
#[gpui::test]
async fn typing_over_a_cross_cell_selection_replaces_it(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, TABLE_DOC.to_string(), None)
    });
    redraw(cx);
    redraw(cx);

    let start = cell_text_start(cell_bounds(&editor, cx, (0, 0)));
    let end = cell_text_end(cell_bounds(&editor, cx, (0, 1)));
    drag_across(cx, start, &[], end);
    editor.read_with(cx, |editor, _cx| {
        assert!(
            editor.table_text_selection.is_some(),
            "前置条件：跨格选区该建起来"
        );
    });

    cx.simulate_input("X");
    redraw(cx);

    editor.read_with(cx, |editor, cx| {
        assert!(
            editor.table_text_selection.is_none(),
            "打完字选区该收起来"
        );
        assert_eq!(
            editor.document.markdown_text(cx),
            "alpha\n\n| X |  |\n| --- | --- |\n| c | d |\n\ngamma",
            "敲的字该落在选区起点那一格，选中的字一起换掉"
        );
    });
}

/// 从表格里往外拖：整张表跟着走进跨块选区（格子上的高亮也一起亮）。
#[gpui::test]
async fn dragging_from_a_table_cell_out_of_the_table_keeps_the_table(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let markdown = "| a | b |\n| --- | --- |\n| c | d |\n\ngamma";
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, markdown.to_string(), None)
    });
    redraw(cx);
    redraw(cx);

    let table = table_entity(&editor, cx);
    let below_bounds = editor.read_with(cx, |editor, cx| {
        editor.document.visible_blocks()[1]
            .entity
            .read(cx)
            .last_bounds
            .expect("表格下面那一段该有布局边界")
    });

    let start = cell_text_start(cell_bounds(&editor, cx, (0, 0)));
    let last_cell_end = cell_text_end(cell_bounds(&editor, cx, (1, 1)));
    let end = gpui::point(below_bounds.right() - px(4.0), below_bounds.center().y);
    drag_across(cx, start, &[last_cell_end], end);

    editor.read_with(cx, |editor, cx| {
        let markdown = editor
            .cross_block_selected_markdown(cx)
            .expect("从表格拖到下面那一段应当有选区");
        assert!(markdown.contains("| a | b |"), "选中文本缺整张表：{markdown:?}");
        assert!(markdown.contains("| c | d |"), "选中文本缺数据行：{markdown:?}");
        assert!(markdown.contains("gamma"), "选中文本缺下面那一段：{markdown:?}");
        assert_eq!(
            cell_selection_ranges(cx, &table),
            vec![Some(0..1), Some(0..1), Some(0..1), Some(0..1)],
            "整张表进了选区，每一格都该亮"
        );
    });
}

/// 从表格上面那一段拖进表格：选区要能跨进表里（此前拖进表里就停住）。
#[gpui::test]
async fn dragging_from_a_paragraph_into_a_table_includes_the_table(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, TABLE_DOC.to_string(), None)
    });
    redraw(cx);
    redraw(cx);

    let table = table_entity(&editor, cx);
    let paragraph_bounds = editor.read_with(cx, |editor, cx| {
        editor.document.visible_blocks()[0]
            .entity
            .read(cx)
            .last_bounds
            .expect("表格上面那一段该有布局边界")
    });

    let start = gpui::point(paragraph_bounds.left() + px(1.0), paragraph_bounds.center().y);
    let end = cell_text_end(cell_bounds(&editor, cx, (1, 1)));
    drag_across(cx, start, &[], end);

    editor.read_with(cx, |editor, cx| {
        let markdown = editor
            .cross_block_selected_markdown(cx)
            .expect("从上面那一段拖进表格应当有选区");
        assert!(markdown.contains("alpha"), "选中文本缺上面那一段：{markdown:?}");
        assert!(markdown.contains("| a | b |"), "整张表该跟着进来：{markdown:?}");
        assert!(markdown.contains("| c | d |"), "整张表该跟着进来：{markdown:?}");
        assert_eq!(
            cell_selection_ranges(cx, &table),
            vec![Some(0..1), Some(0..1), Some(0..1), Some(0..1)],
            "整张表在选区里，每一格都该亮"
        );
    });
}

/// 源码模式里新打出来的行（回车新建的块）之间拖选，整段代码块必须能选上。
/// 实测：按下武装跨块拖拽的那一步被门在渲染态，源码模式里 cross_block_drag
/// 永远不建立，mouse_move 全程早退——拖过多少行都只有按下那一根块里有块内
/// 选区（用户看到的是「代码块选不中」）。
#[gpui::test]
async fn dragging_across_newly_typed_source_lines_selects_the_code_block(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "# 标题\n\n正文一段".to_string(), None)
    });
    redraw(cx);
    editor.update(cx, |editor, cx| editor.toggle_view_mode(cx));
    for _ in 0..3 {
        redraw(cx);
    }

    // 光标先挪到文末，再打三行：```python / print('hello') / ```
    {
        let last = editor.read_with(cx, |editor, _cx| {
            editor.document.root_blocks().last().cloned().expect("有根块")
        });
        cx.update(|_window, cx| {
            editor.update(cx, |editor, _cx| editor.focus_block(last.entity_id()));
        });
        last.update(cx, |block, cx| block.move_to(block.visible_len(), cx));
        redraw(cx);
    }
    let lines = ["```python", "print('hello')", "```"];
    for line in lines {
        cx.update(|window, cx| {
            let last = editor.read_with(cx, |editor, _cx| {
                editor.document.root_blocks().last().cloned().expect("有根块")
            });
            last.update(cx, |block, cx| {
                block.move_to(block.visible_len(), cx);
                block.on_newline(&Newline, window, cx);
            });
        });
        cx.simulate_input(line);
    }
    for _ in 0..3 {
        redraw(cx);
    }

    // 真实拖选：从 ```python 行首拖到最后一行 ``` 行尾
    let (start, end) = editor.read_with(cx, |editor, cx| {
        let blocks = editor.document.visible_blocks();
        let fence = blocks
            .iter()
            .find(|visible| visible.entity.read(cx).display_text() == "```python")
            .expect("找到 ```python 块")
            .entity
            .read(cx)
            .last_bounds
            .expect("有布局边界");
        let tail = blocks
            .iter()
            .filter(|visible| visible.entity.read(cx).display_text() == "```")
            .next_back()
            .expect("找到结尾 ``` 块")
            .entity
            .read(cx)
            .last_bounds
            .expect("有布局边界");
        (
            gpui::point(fence.left() + px(2.0), fence.center().y),
            gpui::point(tail.left() + px(30.0), tail.center().y),
        )
    });
    cx.simulate_mouse_down(start, gpui::MouseButton::Left, Modifiers::none());
    redraw(cx);
    cx.simulate_mouse_move(end, gpui::MouseButton::Left, Modifiers::none());
    redraw(cx);
    cx.simulate_mouse_up(end, gpui::MouseButton::Left, Modifiers::none());
    redraw(cx);

    editor.read_with(cx, |editor, cx| {
        assert!(
            editor.cross_block_selection.is_some(),
            "跨块拖拽结束应当有跨块选区"
        );
        let markdown = editor
            .cross_block_selected_markdown(cx)
            .expect("选区文本该取得到");
        for line in ["```python", "print('hello')", "```"] {
            assert!(
                markdown.contains(line),
                "选中文本缺 {line:?}：{markdown:?}"
            );
        }
        let blocks = editor.document.visible_blocks();
        let states: Vec<Option<std::ops::Range<usize>>> = blocks
            .iter()
            .map(|visible| visible.entity.read(cx).editor_selection_clean_range.clone())
            .collect();
        assert_eq!(
            states.len(),
            4,
            "前置：源码模式整篇一根块 + 三行代码块"
        );
        assert!(
            states[0].is_none(),
            "选区外的块不该亮：{:?}",
            states[0]
        );
        assert!(
            states[1..4].iter().all(|range| range.is_some()),
            "代码块三行都得亮：{states:?}"
        );
    });
}

/// 一段正文 + 一根公式 + 一段正文：公式块未聚焦时只画 KaTeX 图片、没有文字布局。
const MATH_DOC: &str = "alpha\n\n$$\nx = 1\n$$\n\ngamma";

/// 文档第 index 根可见块的上/下半区里的一个点。
fn block_half_point(
    editor: &Entity<Editor>,
    cx: &mut VisualTestContext,
    index: usize,
    half: f32,
) -> gpui::Point<gpui::Pixels> {
    let bounds = editor.read_with(cx, |editor, cx| {
        editor.document.visible_blocks()[index]
            .entity
            .read(cx)
            .last_bounds
            .expect("这一块该有布局边界")
    });
    gpui::point(bounds.left() + px(1.0), bounds.top() + bounds.size.height * half)
}

/// 缺陷：公式块未聚焦时没有文字布局，指针落在它这一段空间里给出的端点是块首，
/// 于是「从上一段往下拖进公式」选出来的只有上一段，公式一个字也进不来。
#[gpui::test]
async fn dragging_into_a_formula_selects_the_formula(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, MATH_DOC.to_string(), None)
    });
    redraw(cx);
    redraw(cx);

    // 公式块没有 last_bounds：它的位置由上下两段的边界夹出来。
    let (alpha_bottom, gamma_top) = editor.read_with(cx, |editor, cx| {
        let visible = editor.document.visible_blocks().to_vec();
        let alpha = visible[0].entity.read(cx).last_bounds.expect("alpha").bottom();
        let gamma = visible[2].entity.read(cx).last_bounds.expect("gamma").top();
        (alpha, gamma)
    });
    let in_formula = gpui::point(px(700.0), alpha_bottom + (gamma_top - alpha_bottom) * 0.25);

    let start = block_half_point(&editor, cx, 0, 0.5);
    cx.simulate_mouse_down(start, gpui::MouseButton::Left, Modifiers::none());
    redraw(cx);
    cx.simulate_mouse_move(in_formula, gpui::MouseButton::Left, Modifiers::none());
    redraw(cx);
    cx.simulate_mouse_up(in_formula, gpui::MouseButton::Left, Modifiers::none());
    redraw(cx);

    editor.read_with(cx, |editor, cx| {
        let text = editor
            .selected_visible_text(cx)
            .expect("拖进公式应当有跨块选区");
        assert!(text.contains("alpha"), "上一段该在选区里：{text:?}");
        assert!(text.contains("x = 1"), "公式该在选区里：{text:?}");
    });

    // 聚焦的公式块要铺出选中底色（KaTeX 是图片，没有文字布局可画高亮）。
    assert!(
        cx.debug_bounds("math-block-selection").is_some(),
        "选区里的公式块该铺选中底色"
    );
}

/// 反方向：从下一段往上拖进公式，公式同样要整块进选区。
#[gpui::test]
async fn dragging_up_into_a_formula_selects_the_formula(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, MATH_DOC.to_string(), None)
    });
    redraw(cx);
    redraw(cx);

    let (alpha_bottom, gamma_top) = editor.read_with(cx, |editor, cx| {
        let visible = editor.document.visible_blocks().to_vec();
        let alpha = visible[0].entity.read(cx).last_bounds.expect("alpha").bottom();
        let gamma = visible[2].entity.read(cx).last_bounds.expect("gamma").top();
        (alpha, gamma)
    });
    let in_formula = gpui::point(px(700.0), alpha_bottom + (gamma_top - alpha_bottom) * 0.75);

    let start = block_half_point(&editor, cx, 2, 0.5);
    cx.simulate_mouse_down(start, gpui::MouseButton::Left, Modifiers::none());
    redraw(cx);
    cx.simulate_mouse_move(in_formula, gpui::MouseButton::Left, Modifiers::none());
    redraw(cx);
    cx.simulate_mouse_up(in_formula, gpui::MouseButton::Left, Modifiers::none());
    redraw(cx);

    editor.read_with(cx, |editor, cx| {
        let text = editor
            .selected_visible_text(cx)
            .expect("往上拖进公式应当有跨块选区");
        assert!(text.contains("x = 1"), "公式该在选区里：{text:?}");
    });
    assert!(
        cx.debug_bounds("math-block-selection").is_some(),
        "选区里的公式块该铺选中底色"
    );
}

/// Shift+点落在公式上：公式要进选区，不能只选到上一段。
#[gpui::test]
async fn shift_click_on_a_formula_includes_the_formula(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, MATH_DOC.to_string(), None)
    });
    redraw(cx);
    redraw(cx);

    let (alpha_bottom, gamma_top) = editor.read_with(cx, |editor, cx| {
        let visible = editor.document.visible_blocks().to_vec();
        let alpha = visible[0].entity.read(cx).last_bounds.expect("alpha").bottom();
        let gamma = visible[2].entity.read(cx).last_bounds.expect("gamma").top();
        (alpha, gamma)
    });
    let in_formula = gpui::point(px(700.0), alpha_bottom + (gamma_top - alpha_bottom) * 0.75);

    let start = block_half_point(&editor, cx, 0, 0.5);
    let mut shift = Modifiers::none();
    shift.shift = true;
    cx.simulate_mouse_down(start, gpui::MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(start, gpui::MouseButton::Left, Modifiers::none());
    redraw(cx);
    cx.simulate_mouse_down(in_formula, gpui::MouseButton::Left, shift);
    cx.simulate_mouse_up(in_formula, gpui::MouseButton::Left, shift);
    redraw(cx);

    editor.read_with(cx, |editor, cx| {
        let text = editor
            .selected_visible_text(cx)
            .expect("Shift+点在公式上应当有跨块选区");
        assert!(text.contains("alpha"), "上一段该在选区里：{text:?}");
        assert!(text.contains("x = 1"), "公式该在选区里：{text:?}");
    });
}

/// 缺陷：块划出挂载窗口之后，它的 `last_bounds` 停在最后一次画下来的屏幕位置，
/// 没人清它。端点换算遍历全文档时，这些陈旧边界会被当成落点——长文档往下翻过几屏，
/// 文档开头的一块就把落点抢走，选区从上面拉下来一大片。
#[gpui::test]
#[ignore = "慢用例（>1s）：本地默认跳过，CI 跑"]
async fn a_press_ignores_bounds_left_behind_by_unmounted_blocks(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let markdown = (0..400)
        .map(|index| format!("## Section {index}\n\nParagraph body for section {index}.\n"))
        .collect::<Vec<_>>()
        .join("\n");
    let (editor, cx) = cx.add_window_view(|_window, cx| Editor::from_markdown(cx, markdown, None));
    redraw(cx);
    redraw(cx);

    // 滚轮往下翻过几屏。
    let wheel_at = editor.read_with(cx, |editor, _cx| editor.scroll_handle.bounds().center());
    for _ in 0..30 {
        cx.simulate_mouse_move(wheel_at, None, Modifiers::none());
        cx.simulate_event(gpui::ScrollWheelEvent {
            position: wheel_at,
            delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.0), px(-400.0))),
            modifiers: Modifiers::none(),
            touch_phase: gpui::TouchPhase::default(),
        });
        redraw(cx);
    }
    redraw(cx);
    redraw(cx);

    // 前置条件：挂载窗口已经移开文首，视口中央是别的块。
    // 窗口由上一帧的行区间与行计划换出来（生产代码里同一份判据）。
    let (mounted_start, mounted_end, center) = editor.read_with(cx, |editor, _cx| {
        let run = editor.prev_mounted_run.expect("挂载区");
        let plan = editor.rendered_row_plan.clone().expect("行计划");
        let start = plan.rows[run.row_start].visible_start;
        let end = plan.rows[run.row_end - 1].visible_end;
        (start, end, editor.scroll_handle.bounds().center())
    });
    assert!(
        mounted_start > 0,
        "前置条件：滚动之后挂载窗口该离开文首，实际从 {mounted_start} 开始"
    );

    // 造出「没挂载但留着边界」的那一块：把它的边界挪到视口中央，正是渲染留下的形状。
    // 写完不重绘——重绘就等于把真实状态摆回来，滚动留下的痕迹正是这样留在内存里的。
    let stale_index = mounted_start - 1;
    assert!(stale_index < mounted_start, "前置条件：拿的是一块没挂载的块");
    editor.update(cx, |editor, cx| {
        let stale = editor.document.visible_blocks()[stale_index].entity.clone();
        stale.update(cx, |block, _cx| {
            block.last_bounds = Some(Bounds::new(
                gpui::point(center.x - px(200.0), center.y - px(10.0)),
                gpui::size(px(400.0), px(20.0)),
            ));
        });
        assert!(
            editor.document.visible_blocks()[stale_index]
                .entity
                .read(cx)
                .last_bounds
                .is_some_and(|bounds| bounds.contains(&center)),
            "前置条件：陈旧边界该盖住视口中央"
        );
    });

    cx.simulate_mouse_down(center, gpui::MouseButton::Left, Modifiers::none());
    redraw(cx);
    let end = gpui::point(center.x, center.y + px(30.0));
    cx.simulate_mouse_move(end, gpui::MouseButton::Left, Modifiers::none());
    redraw(cx);
    cx.simulate_mouse_up(end, gpui::MouseButton::Left, Modifiers::none());
    redraw(cx);

    editor.read_with(cx, |editor, _cx| {
        let selection = editor
            .cross_block_selection
            .expect("视口里按下拖动该有跨块选区");
        let anchor_index = editor
            .document
            .visible_index_for_entity_id(selection.anchor.entity_id)
            .expect("锚点仍在文档里");
        assert_ne!(
            anchor_index, stale_index,
            "没挂载的那一块留着陈旧边界，不能拿它当落点"
        );
        assert!(
            anchor_index >= mounted_start && anchor_index < mounted_end,
            "锚点必须落在挂载窗口 {mounted_start}..{mounted_end} 里，实际 {anchor_index}"
        );
    });
}

/// 缺陷：格子里只有公式时（`$q$`）格子没有文字布局，`last_bounds` 也一直空着——
/// 表格按格选字认不出它，整格点不到；落在选区终点侧的那一格区间还会是空的。
#[gpui::test]
async fn dragging_across_table_cells_holding_formulas_selects_them(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(
            cx,
            "alpha\n\n| $q$ | 含义 |\n| --- | --- |\n| $U$ | key 集合 |\n\ngamma".to_string(),
            None,
        )
    });
    redraw(cx);
    redraw(cx);
    redraw(cx);

    let table = table_entity(&editor, cx);
    let start = cell_text_start(cell_bounds(&editor, cx, (0, 0)));
    let end = cell_text_end(cell_bounds(&editor, cx, (1, 1)));
    drag_across(&mut cx.clone(), start, &[], end);

    editor.read_with(cx, |editor, cx| {
        let ranges = cell_selection_ranges(cx, &table);
        assert!(
            ranges.iter().all(|range| range.is_some()),
            "四格都该带上选中区间，实际 {ranges:?}"
        );
        let text = editor.selected_visible_text(cx).expect("表格里该有选区");
        assert!(text.contains("$q$"), "只装公式的格子该整格选上：{text:?}");
        assert!(text.contains("$U$"), "只装公式的格子该整格选上：{text:?}");
        assert!(text.contains("key 集合"), "有文字的格子该选上：{text:?}");
    });
}

/// 缺陷：未聚焦、带行内公式的段落只画图片，没有 `last_bounds` 也没有文字布局，
/// 指针落点换算看不见它——从上一段拖过它就是选不上，越往下拖越选不上。
#[gpui::test]
async fn dragging_through_an_unfocused_paragraph_with_inline_math_selects_it(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(
            cx,
            "第一段纯文字\n\n$y$ 就是不切分时这一行的输出\n\n最后一段".to_string(),
            None,
        )
    });
    redraw(cx);
    redraw(cx);
    redraw(cx);

    let (start, end) = editor.read_with(cx, |editor, cx| {
        let visible = editor.document.visible_blocks().to_vec();
        let first = visible[0].entity.read(cx).last_bounds.expect("第一段");
        let last = visible[2].entity.read(cx).last_bounds.expect("最后一段");
        (
            gpui::point(first.left() + px(1.0), first.center().y),
            gpui::point(last.left() + px(20.0), last.center().y),
        )
    });
    drag_across(&mut cx.clone(), start, &[], end);

    editor.read_with(cx, |editor, cx| {
        let text = editor.selected_visible_text(cx).expect("该有跨块选区");
        assert!(
            text.contains("$y$ 就是不切分时这一行的输出"),
            "带行内公式的那一段该进选区：{text:?}"
        );
        assert!(text.contains("最后"), "拖到的那一段该进选区：{text:?}");
    });
}

/// 报修原文（`fixtures/regressions/15-long-doc-math-selection.md`，512 行、含公式与表格）
/// 上的三处拖动：表格里的公式、公式下面那一段、从第 4 章往下拖。
///
/// 现象：① 表格里只装公式的格子选不中；② 从 2.2 往 2.3 拖，公式下面那一段选不上，
/// 越往下拖越选不上；③ 从第 4 章往下拖，文档前面按不到的内容也被选中。
#[gpui::test]
#[ignore = "慢用例（>1s）：本地默认跳过，CI 跑"]
async fn report_long_document_selection_gestures(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let markdown = std::fs::read_to_string("fixtures/regressions/15-long-doc-math-selection.md")
        .expect("报修原文");
    let (editor, cx) = cx.add_window_view(|_window, cx| Editor::from_markdown(cx, markdown, None));
    redraw(cx);
    redraw(cx);

    // 轮子往下滚，直到某一块的边界落进视口中间，返回它是否找到。
    let wheel_at = editor.read_with(cx, |editor, _cx| editor.scroll_handle.bounds().center());
    let visible_with_room = |cx: &mut VisualTestContext, index: usize| {
        for _ in 0..60 {
            let visible = editor.read_with(cx, |editor, cx| {
                let viewport = editor.scroll_handle.bounds();
                editor.document.visible_blocks()[index]
                    .entity
                    .read(cx)
                    .last_bounds
                    .is_some_and(|bounds| {
                        bounds.top() > viewport.top() + px(80.0)
                            && bounds.bottom() < viewport.bottom() - px(160.0)
                    })
            });
            if visible {
                return true;
            }
            cx.simulate_mouse_move(wheel_at, None, Modifiers::none());
            cx.simulate_event(gpui::ScrollWheelEvent {
                position: wheel_at,
                delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.0), px(-300.0))),
                modifiers: Modifiers::none(),
                touch_phase: gpui::TouchPhase::default(),
            });
            redraw(cx);
            redraw(cx);
        }
        false
    };
    let index_of = |cx: &mut VisualTestContext, needle: &str| {
        editor.read_with(cx, |editor, cx| {
            editor
                .document
                .visible_blocks()
                .iter()
                .position(|visible| visible.entity.read(cx).display_text().contains(needle))
                .unwrap_or_else(|| panic!("文档里找不到 {needle}"))
        })
    };

    // ① 2.1 那张表：格子里装的是公式。
    let section_21 = index_of(&mut cx.clone(), "2.1 基本量");
    assert!(visible_with_room(&mut cx.clone(), section_21), "2.1 该滚进视口");
    let (start, end) = editor.read_with(cx, |editor, cx| {
        let viewport_bottom = editor.scroll_handle.bounds().bottom();
        let table = editor
            .document
            .visible_blocks()
            .iter()
            .enumerate()
            .filter(|(index, _)| *index >= section_21)
            .find(|(_, visible)| visible.entity.read(cx).kind() == BlockKind::Table)
            .expect("2.1 下面该有表格")
            .1
            .entity
            .clone();
        let in_view: Vec<Bounds<gpui::Pixels>> = table_cells(cx, &table)
            .into_iter()
            .filter_map(|cell| cell.read(cx).last_bounds)
            .filter(|bounds| bounds.bottom() < viewport_bottom - px(20.0))
            .collect();
        let first = in_view.first().copied().expect("视口里该有格子");
        let last = in_view.last().copied().expect("视口里该有格子");
        (
            gpui::point(first.left() + px(1.0), first.center().y),
            gpui::point(last.right() - px(1.0), last.center().y),
        )
    });
    drag_across(&mut cx.clone(), start, &[], end);
    editor.read_with(cx, |editor, cx| {
        let text = editor.selected_visible_text(cx).expect("表格里该有选区");
        assert!(text.contains("$q$"), "只装公式的格子该选上：{text:?}");
        assert!(
            text.contains("该行的 query 向量"),
            "公式与文字混着的格子该选上：{text:?}"
        );
    });

    // ② 从 2.2 往 2.3 拖：中间的公式与公式下面那一段都要进来。
    let section_22 = index_of(&mut cx.clone(), "2.2 真值");
    assert!(visible_with_room(&mut cx.clone(), section_22), "2.2 该滚进视口");
    let (start, end) = editor.read_with(cx, |editor, cx| {
        let bounds = editor.document.visible_blocks()[section_22]
            .entity
            .read(cx)
            .last_bounds
            .expect("2.2 该有边界");
        let start = gpui::point(bounds.left() + px(1.0), bounds.center().y);
        let bottom = f32::from(editor.scroll_handle.bounds().bottom()) - 40.0;
        let end_y = (f32::from(start.y) + 260.0).min(bottom);
        (start, gpui::point(start.x, px(end_y)))
    });
    drag_across(&mut cx.clone(), start, &[], end);
    editor.read_with(cx, |editor, cx| {
        let text = editor.selected_visible_text(cx).expect("2.2 往下拖该有选区");
        assert!(text.contains("2.2 真值"), "起点那一段该在选区里：{text:?}");
        assert!(text.contains("m_{all}"), "中间的公式该在选区里：{text:?}");
        assert!(
            text.contains("$y$ 就是"),
            "公式下面那一段该在选区里：{text:?}"
        );
    });

    // ③ 从第 4 章往下拖：不能把文档前面的内容也拉进来。
    let chapter_4 = index_of(&mut cx.clone(), "第 4 章");
    for _ in 0..60 {
        redraw(cx);
        redraw(cx);
        let visible = editor.read_with(cx, |editor, cx| {
            editor.document.visible_blocks()[chapter_4]
                .entity
                .read(cx)
                .last_bounds
                .is_some_and(|bounds| bounds.top() > editor.scroll_handle.bounds().top())
        });
        if visible {
            break;
        }
        cx.simulate_mouse_move(wheel_at, None, Modifiers::none());
        cx.simulate_event(gpui::ScrollWheelEvent {
            position: wheel_at,
            delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.0), px(300.0))),
            modifiers: Modifiers::none(),
            touch_phase: gpui::TouchPhase::default(),
        });
    }
    let (start, end) = editor.read_with(cx, |editor, cx| {
        let bounds = editor.document.visible_blocks()[chapter_4]
            .entity
            .read(cx)
            .last_bounds
            .expect("第 4 章该有边界");
        let start = gpui::point(bounds.left() + px(30.0), bounds.center().y);
        let bottom = f32::from(editor.scroll_handle.bounds().bottom()) - 40.0;
        let end_y = (f32::from(start.y) + 480.0).min(bottom);
        (start, gpui::point(start.x, px(end_y)))
    });
    drag_across(&mut cx.clone(), start, &[], end);
    editor.read_with(cx, |editor, cx| {
        let text = editor.selected_visible_text(cx).expect("第 4 章往下拖该有选区");
        assert!(text.contains("反向推导"), "起点那一段该在选区里：{text:?}");
        assert!(
            !text.contains("符号定义"),
            "文档前面的内容不该被拉进来：{text:?}"
        );
    });
}

/// 守门：不管一行装的是哪种块（正文、标题、列表、引用、代码块、表格、公式、分隔线、
/// 图片），只要它挂载着，指针落在这一行里就必须解析到这一行自己的块。
///
/// 这一层过去靠块自己画下来的 `last_bounds`，于是两类毛病反复出现：只画图片的块没有
/// 那个字段（点不到），划出挂载窗口的块留着旧值（点到几屏之前）。这条用例长文档 + 真
/// 滚动，把每一行都点一遍——新加一种块类型时它会先红。
#[gpui::test]
#[ignore = "慢用例（>1s）：本地默认跳过，CI 跑"]
async fn every_mounted_row_resolves_to_its_own_block(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let markdown = (0..40)
        .map(|index| {
            format!(
                "## 第 {index} 节\n\n\
                 正文段落 {index}，行内公式 $a_{index}+b$ 与行内代码 `let x = {index};`。\n\n\
                 {{{{=x_{index}=}}}} 这是分隔线\n\n\
                 > 引用 {index}\n\n\
                 - 列表项 {index}\n\n\
                 ```rust\nlet y = {index};\n```\n\n\
                 | 列 A | 列 B |\n| --- | --- |\n| $c_{index}$ | 文字 {index} |\n\n\
                 $$m_{index}=\\max_j s_j$$\n\n\
                 ![图 {index}](missing-{index}.png)\n"
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let (editor, cx) = cx.add_window_view(|_window, cx| Editor::from_markdown(cx, markdown, None));
    redraw(cx);
    redraw(cx);

    let wheel_at = editor.read_with(cx, |editor, _cx| editor.scroll_handle.bounds().center());
    for round in 0..3 {
        if round > 0 {
            for _ in 0..3 {
                cx.simulate_mouse_move(wheel_at, None, Modifiers::none());
                cx.simulate_event(gpui::ScrollWheelEvent {
                    position: wheel_at,
                    delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.0), px(-900.0))),
                    modifiers: Modifiers::none(),
                    touch_phase: gpui::TouchPhase::default(),
                });
                redraw(cx);
                redraw(cx);
            }
        }

        // 从后往前点：按下会把那一块聚焦、它下面的行会挪位置，先点下面的行，
        // 已经点过的不会被我自己的按下搅动。每一行都在按下前重取几何。
        let row_range = editor.read_with(cx, |editor, _cx| {
            let run = editor.prev_mounted_run.expect("挂载区");
            (run.row_start, run.row_end)
        });
        let row_geometry = |cx: &mut VisualTestContext, row: usize| {
            editor.read_with(cx, |editor, _cx| {
                let run = editor.prev_mounted_run?;
                if row < run.row_start || row >= run.row_end {
                    return None;
                }
                let plan = editor.rendered_row_plan.clone()?;
                let scroll = editor.scroll_handle.offset();
                let viewport = editor.scroll_handle.bounds();
                let item = editor
                    .scroll_handle
                    .bounds_for_item(run.child_base + row - run.row_start)?;
                let top = f32::from(item.top()) + f32::from(scroll.y);
                let bottom = f32::from(item.bottom()) + f32::from(scroll.y);
                if top < f32::from(viewport.top()) || bottom > f32::from(viewport.bottom()) {
                    return None;
                }
                let plan_row = &plan.rows[row];
                let ids: Vec<gpui::EntityId> = (plan_row.visible_start..plan_row.visible_end)
                    .map(|index| editor.document.visible_blocks()[index].entity.entity_id())
                    .collect();
                Some((px((top + bottom) / 2.0), ids))
            })
        };
        // 每轮均匀取十几行：一行一行点下来太慢，而覆盖靠的是「每种块都被点到」。
        let span = (row_range.1 - row_range.0).max(1);
        let stride = (span / 14).max(1);
        let mut pressed = 0;
        for row in (row_range.0..row_range.1).rev().step_by(stride) {
            let Some((y, ids)) = row_geometry(&mut cx.clone(), row) else {
                continue;
            };
            let point = gpui::point(px(700.0), y);
            cx.simulate_mouse_down(point, gpui::MouseButton::Left, Modifiers::none());
            redraw(cx);
            let anchored = editor
                .read_with(cx, |editor, _cx| editor.cross_block_drag.map(|drag| drag.anchor.entity_id));
            cx.simulate_mouse_up(point, gpui::MouseButton::Left, Modifiers::none());
            redraw(cx);
            let anchored = anchored.unwrap_or_else(|| panic!("第 {round} 轮 y={y:?} 没有武装锚点"));
            assert!(
                ids.contains(&anchored),
                "第 {round} 轮 y={y:?} 该落在这一行的块里，实际锚点在别的块：{:?}",
                editor.read_with(cx, |editor, cx| {
                    let mut parts: Vec<String> = Vec::new();
                    for id in std::iter::once(anchored).chain(ids.iter().copied()) {
                        if let Some(block) = editor.document.block_entity_by_id(id) {
                            let block = block.read(cx);
                            parts.push(format!(
                                "{:?} {:?}",
                                block.kind(),
                                block.display_text().chars().take(20).collect::<String>()
                            ));
                        }
                    }
                    parts.join(" | ")
                })
            );
            pressed += 1;
        }
        assert!(pressed > 0, "第 {round} 轮该点到行");
    }
}
