use super::common::*;
use gpui::point;
use gpui::{Div, IntoElement, Render, Stateful, Styled};

#[gpui::test]
async fn review_indented_table_insertion_preserves_outer_padding(cx: &mut TestAppContext) {
    // 外层管道符之外的缩进不是单元格，结构编辑不能因此退回整表重拼。
    let source = "   | A    | B    |  \n   |:-----|-----:|  \n   | a\\|b | c    |  ";
    let editor = cx.new(|cx| Editor::from_markdown(cx, source.into(), None));
    editor.update(cx, |editor, cx| {
        let table = editor.document.first_root().unwrap().clone();
        assert_eq!(table.read(cx).kind(), BlockKind::Table);
        editor.append_table_row(&table, cx);
        assert!(editor.buffer.text().starts_with(source));
    });
}

#[gpui::test]
async fn review_table_structure_edits_preserve_escaped_pipes_and_padding(cx: &mut TestAppContext) {
    // 转义竖线曾触发整表重拼，连未编辑行的列宽与对齐写法也被改掉。
    let source = "| A    | B     |\n|:-----|------:|\n| a\\|b | 中文   |\n| c\\|d | e     |";
    for operation in 0..3 {
        let editor = cx.new(|cx| Editor::from_markdown(cx, source.into(), None));
        editor.update(cx, |editor, cx| {
            let table = editor.document.first_root().unwrap().clone();
            match operation {
                0 => {
                    editor.append_table_row(&table, cx);
                    assert!(editor.buffer.text().starts_with(source));
                }
                1 => {
                    editor.delete_table_row(&table, 0, cx);
                    assert_eq!(editor.buffer.text(), "| A    | B     |\n|:-----|------:|\n| c\\|d | e     |");
                }
                _ => {
                    editor.delete_table_header_row(&table, cx);
                    assert_eq!(editor.buffer.text(), "| a\\|b | 中文   |\n|:-----|------:|\n| c\\|d | e     |");
                }
            }
        });
    }
}

#[gpui::test]
async fn code_and_table_panels_use_95_percent_of_the_visible_content_width(
    cx: &mut TestAppContext,
) {
    // 代码块和表格原来铺满整列，比正文文字更宽；95% 必须按实际正文容器计算。
    init_editor_test_app(cx);
    for (markdown, is_table) in [
        ("```rust\nlet value = 1;\n```", false),
        (
            "| 标题甲 | 标题乙 |\n| --- | --- |\n| 内容甲 | 内容乙 |",
            true,
        ),
    ] {
        let (editor, cx) =
            cx.add_window_view(|_, cx| Editor::from_markdown(cx, markdown.into(), None));
        for (width, sidebar_open) in [(1400.0, true), (2400.0, false), (600.0, true)] {
            editor.update(cx, |editor, cx| {
                editor.workspace.is_open = sidebar_open;
                cx.notify();
            });
            cx.simulate_resize(gpui::size(px(width), px(1000.0)));
            redraw(cx);
            redraw(cx);
            redraw(cx);
            let (viewport, expected_width) = editor.read_with(cx, |editor, cx| {
                let viewport = editor.scroll_handle.bounds();
                let dimensions = &cx.global::<ThemeManager>().current().dimensions;
                let column =
                    Editor::writing_column_width(f32::from(viewport.size.width), dimensions, cx);
                (viewport, (column - dimensions.block_padding_x * 2.0) * 0.95)
            });
            let (left, right) = if is_table {
                let first = cx
                    .debug_bounds("table-column-indicator-0")
                    .expect("首列表头边界");
                let last = cx
                    .debug_bounds("table-column-indicator-1")
                    .expect("末列表头边界");
                (first.left(), last.right())
            } else {
                let panel = cx.debug_bounds("block-shell").expect("代码块面板");
                (panel.left(), panel.right())
            };
            assert!(
                (f32::from(right - left) - expected_width).abs() < 1.0,
                "窗口 {width}、侧栏 {sidebar_open}、表格 {is_table}：面板应宽 {expected_width}，实际 {:?}",
                right - left
            );
            assert!(left >= viewport.left() && right <= viewport.right());
            assert!(((left + right) * 0.5 - viewport.center().x).abs() < px(1.0));
        }
    }
}

#[gpui::test]
async fn table_cells_have_rounded_corners_accent_headers_and_a_subtle_hover(
    cx: &mut TestAppContext,
) {
    // 表格四个外角要随行列位置圆滑收边，表头用主题强调色加粗，悬停正文格只轻微染色。
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| {
        Editor::from_markdown(cx, "| 甲 | 乙 |\n| --- | --- |\n| 丙 | 丁 |".into(), None)
    });
    redraw(cx);
    redraw(cx);
    let (header, last_header, body, last_body) = editor.read_with(cx, |editor, cx| {
        let table = editor.document.first_root().expect("表格").read(cx);
        let runtime = table.table_runtime.as_ref().expect("表格运行时");
        (
            runtime.header[0].clone(),
            runtime.header[1].clone(),
            runtime.rows[0][0].clone(),
            runtime.rows[0][1].clone(),
        )
    });
    let style_for = |cell: &gpui::Entity<Block>, cx: &mut VisualTestContext| {
        cell.update_in(cx, |block, window, cx| {
            let mut element = block.render(window, cx).into_any_element();
            element
                .downcast_mut::<Stateful<Div>>()
                .expect("单元格渲染根元素")
                .style()
                .clone()
        })
    };
    let header_style = style_for(&header, cx);
    let accent = cx.update(|_, cx| cx.global::<ThemeManager>().current().colors.text_link);
    let header_text = &header_style.text;
    assert_eq!(header_text.color, Some(accent), "表头应使用主题强调色");
    assert_eq!(header_text.font_weight, Some(gpui::FontWeight::BOLD));
    assert_eq!(header_style.corner_radii.top_left, Some(px(10.0).into()));
    assert_eq!(header_style.corner_radii.bottom_left, Some(px(0.0).into()));
    let last_header_style = style_for(&last_header, cx);
    assert_eq!(
        last_header_style.corner_radii.top_right,
        Some(px(10.0).into())
    );
    assert_eq!(
        last_header_style.corner_radii.bottom_right,
        Some(px(0.0).into())
    );
    let before = style_for(&body, cx);
    assert_eq!(before.corner_radii.bottom_left, Some(px(10.0).into()));
    assert_eq!(before.corner_radii.top_left, Some(px(0.0).into()));
    let last_body_style = style_for(&last_body, cx);
    assert_eq!(
        last_body_style.corner_radii.bottom_right,
        Some(px(10.0).into())
    );
    let bounds = body.read_with(cx, |block, _| block.last_bounds.expect("正文格文字边界"));
    cx.simulate_mouse_move(bounds.center(), gpui::MouseButton::Left, Modifiers::none());
    redraw(cx);
    let after = style_for(&body, cx);
    assert_ne!(
        after.background, before.background,
        "悬停单元格应有淡淡的背景变化"
    );
    assert_eq!(
        after.text.color,
        before.text.color
    );
    cx.simulate_mouse_move(
        gpui::point(px(2.0), px(2.0)),
        gpui::MouseButton::Left,
        Modifiers::none(),
    );
    redraw(cx);
    assert_eq!(
        style_for(&body, cx).background,
        before.background,
        "离开单元格后应恢复背景"
    );
}

#[gpui::test]
async fn header_only_single_cell_table_keeps_all_four_corners_round(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, "| 唯一表头 |\n| --- |".into(), None));
    redraw(cx);
    redraw(cx);
    let cell = editor.read_with(cx, |editor, cx| {
        editor
            .document
            .first_root()
            .expect("仅表头的表格")
            .read(cx)
            .table_runtime
            .as_ref()
            .expect("表格运行时")
            .header[0]
            .clone()
    });
    cell.update_in(cx, |block, window, cx| {
        let mut element = block.render(window, cx).into_any_element();
        let style = element
            .downcast_mut::<Stateful<Div>>()
            .expect("单元格根元素")
            .style();
        for corner in [
            style.corner_radii.top_left,
            style.corner_radii.top_right,
            style.corner_radii.bottom_left,
            style.corner_radii.bottom_right,
        ] {
            assert_eq!(corner, Some(px(10.0).into()), "单格表格应保留四个外圆角");
        }
    });
}

#[gpui::test]
async fn parsed_table_runtime_installs_column_alignment_on_cells(cx: &mut TestAppContext) {
    let markdown = [
        "| Left | Center | Right |",
        "| :--- | :---: | ---: |",
        "| a | b | c |",
    ]
    .join("\n");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.read_with(cx, |editor, cx| {
        let table = editor.document.first_root().expect("table root").clone();
        assert_eq!(table.read(cx).kind(), BlockKind::Table);
        let runtime = table
            .read(cx)
            .table_runtime
            .as_ref()
            .expect("table runtime");
        assert_eq!(
            runtime.header[0].read(cx).table_cell_alignment(),
            Some(TableColumnAlignment::Left)
        );
        assert_eq!(
            runtime.header[1].read(cx).table_cell_alignment(),
            Some(TableColumnAlignment::Center)
        );
        assert_eq!(
            runtime.rows[0][2].read(cx).table_cell_alignment(),
            Some(TableColumnAlignment::Right)
        );
    });
}

#[gpui::test]
async fn short_delimiter_dashes_still_render_as_tables(cx: &mut TestAppContext) {
    // 回归：`|:--|:--:|` 表头和 `htmd` 产出的 `| ---- | -- |` 分隔行曾被判为
    // “不是表格”，整段降级成纯文本。
    let markdown = [
        "| 分组 | 总数 | 保留 | 存疑 | 剔除 |",
        "|:--|:--:|:--:|:--:|:--:|",
        "| 1a 产物分 >0.8 | 1 | 1 | 0 | 0 |",
        "| **合计** | **3** | **2** | **0** | **1** |",
        "",
        "| 源文件 | 行数 | 动作 |",
        "| ---- | --- | -- |",
        "| `a.md` | 160 | 增强 |",
    ]
    .join("\n");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.read_with(cx, |editor, cx| {
        assert_eq!(editor.document.root_count(), 2);
        let roots = editor.document.root_blocks();
        for root in roots {
            assert_eq!(root.read(cx).kind(), BlockKind::Table);
        }
        let record = roots[0]
            .read(cx)
            .record
            .table
            .as_ref()
            .expect("table record");
        assert_eq!(record.alignments.len(), 5);
        assert_eq!(record.rows.len(), 2);
    });
}

#[gpui::test]
async fn append_column_updates_table_and_focuses_new_header_cell(cx: &mut TestAppContext) {
    let markdown = ["| A | B |", "| --- | ---: |", "| 1 | 2 |"].join("\n");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.update(cx, |editor, cx| {
        let table = editor.document.first_root().expect("table root").clone();
        editor.append_table_column(&table, cx);

        let record = table
            .read(cx)
            .record
            .table
            .as_ref()
            .expect("table record after append");
        assert_eq!(record.header.len(), 3);
        assert_eq!(record.rows[0].len(), 3);
        assert_eq!(
            record.alignments,
            vec![
                TableColumnAlignment::Default,
                TableColumnAlignment::Right,
                TableColumnAlignment::Right,
            ]
        );

        let runtime = table
            .read(cx)
            .table_runtime
            .as_ref()
            .expect("rebuilt runtime");
        let focused = runtime.header[2].entity_id();
        assert_eq!(editor.pending_focus, Some(focused));
    });
}

#[gpui::test]
async fn append_row_updates_table_and_focuses_first_cell_of_new_row(cx: &mut TestAppContext) {
    let markdown = ["| A | B |", "| --- | :---: |", "| 1 | 2 |"].join("\n");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.update(cx, |editor, cx| {
        let table = editor.document.first_root().expect("table root").clone();
        editor.append_table_row(&table, cx);

        let record = table
            .read(cx)
            .record
            .table
            .as_ref()
            .expect("table record after append");
        assert_eq!(record.rows.len(), 2);
        assert_eq!(record.rows[1].len(), 2);
        assert!(
            record.rows[1]
                .iter()
                .all(|cell| cell.serialize_markdown().is_empty())
        );

        let runtime = table
            .read(cx)
            .table_runtime
            .as_ref()
            .expect("rebuilt runtime");
        let focused = runtime.rows[1][0].entity_id();
        assert_eq!(editor.pending_focus, Some(focused));
    });
}

#[gpui::test]
async fn insert_table_row_below_header_writes_a_blank_line_after_the_delimiter(
    cx: &mut TestAppContext,
) {
    let markdown = ["| A | B |", "| --- | --- |", "| 1 | 2 |", "| 3 | 4 |"].join("\n");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.update(cx, |editor, cx| {
        let table = editor.document.first_root().expect("table root").clone();
        editor.insert_table_row(&table, 0, true, None, cx);

        assert_eq!(
            editor.buffer.text(),
            ["| A | B |", "| --- | --- |", "|   |   |", "| 1 | 2 |", "| 3 | 4 |"]
                .join("\n")
        );
        let record = table.read(cx).record.table.as_ref().expect("table record");
        assert_eq!(record.rows.len(), 3);
        assert!(
            record.rows[0]
                .iter()
                .all(|cell| cell.serialize_markdown().is_empty())
        );

        let runtime = table.read(cx).table_runtime.as_ref().expect("runtime");
        assert_eq!(editor.pending_focus, Some(runtime.rows[0][0].entity_id()));
    });
}

#[gpui::test]
async fn insert_table_row_above_a_body_row_keeps_the_other_lines_untouched(
    cx: &mut TestAppContext,
) {
    let markdown = ["| A | B |", "| --- | --- |", "| 1 | 2 |", "| 3 | 4 |"].join("\n");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.update(cx, |editor, cx| {
        let table = editor.document.first_root().expect("table root").clone();
        editor.insert_table_row(&table, 2, false, None, cx);

        assert_eq!(
            editor.buffer.text(),
            ["| A | B |", "| --- | --- |", "| 1 | 2 |", "|   |   |", "| 3 | 4 |"]
                .join("\n")
        );
        let record = table.read(cx).record.table.as_ref().expect("table record");
        assert_eq!(record.rows.len(), 3);
        assert_eq!(record.rows[0][0].serialize_markdown(), "1");
        assert_eq!(record.rows[2][0].serialize_markdown(), "3");
        assert!(
            record.rows[1]
                .iter()
                .all(|cell| cell.serialize_markdown().is_empty())
        );

        let runtime = table.read(cx).table_runtime.as_ref().expect("runtime");
        assert_eq!(editor.pending_focus, Some(runtime.rows[1][0].entity_id()));
    });
}

#[gpui::test]
async fn insert_table_column_left_copies_the_reference_alignment(cx: &mut TestAppContext) {
    let markdown = ["| A | B |", "| --- | ---: |", "| 1 | 2 |"].join("\n");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.update(cx, |editor, cx| {
        let table = editor.document.first_root().expect("table root").clone();
        editor.insert_table_column(&table, 1, false, None, cx);

        assert_eq!(
            editor.buffer.text(),
            ["| A |  | B |", "| --- | ---: | ---: |", "| 1 |  | 2 |"].join("\n")
        );
        let record = table.read(cx).record.table.as_ref().expect("table record");
        assert_eq!(
            record.alignments,
            vec![
                TableColumnAlignment::Default,
                TableColumnAlignment::Right,
                TableColumnAlignment::Right,
            ]
        );
        assert!(record.header[1].serialize_markdown().is_empty());

        let runtime = table.read(cx).table_runtime.as_ref().expect("runtime");
        assert_eq!(editor.pending_focus, Some(runtime.header[1].entity_id()));
    });
}

#[gpui::test]
async fn insert_table_column_right_of_the_last_column_appends_the_cell(cx: &mut TestAppContext) {
    let markdown = ["| A | B |", "| --- | ---: |", "| 1 | 2 |"].join("\n");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.update(cx, |editor, cx| {
        let table = editor.document.first_root().expect("table root").clone();
        editor.insert_table_column(&table, 1, true, None, cx);

        assert_eq!(
            editor.buffer.text(),
            ["| A | B |  |", "| --- | ---: | ---: |", "| 1 | 2 |  |"].join("\n")
        );
        let record = table.read(cx).record.table.as_ref().expect("table record");
        assert_eq!(record.column_count(), 3);
        assert_eq!(
            record.alignments,
            vec![
                TableColumnAlignment::Default,
                TableColumnAlignment::Right,
                TableColumnAlignment::Right,
            ]
        );

        let runtime = table.read(cx).table_runtime.as_ref().expect("runtime");
        assert_eq!(editor.pending_focus, Some(runtime.header[2].entity_id()));
    });
}

#[gpui::test]
async fn inserting_a_table_row_can_be_undone(cx: &mut TestAppContext) {
    let markdown = ["| A | B |", "| --- | --- |", "| 1 | 2 |"].join("\n");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown.clone(), None));

    editor.update(cx, |editor, cx| {
        let table = editor.document.first_root().expect("table root").clone();
        editor.insert_table_row(&table, 1, false, None, cx);
        assert_ne!(editor.buffer.text(), markdown);
        editor.undo_document(cx);
        assert_eq!(editor.buffer.text(), markdown);
    });
}

fn table_cell_bounds(
    editor: &gpui::Entity<Editor>,
    cx: &mut VisualTestContext,
    row: usize,
    column: usize,
) -> gpui::Bounds<gpui::Pixels> {
    editor.read_with(cx, |editor, cx| {
        let table = editor.document.first_root().expect("table root");
        let runtime = table
            .read(cx)
            .table_runtime
            .as_ref()
            .expect("table runtime");
        let cell = if row == 0 {
            runtime.header[column].clone()
        } else {
            runtime.rows[row - 1][column].clone()
        };
        cell.read(cx).last_bounds.expect("cell bounds")
    })
}

fn right_click_cell(cx: &mut VisualTestContext, bounds: gpui::Bounds<gpui::Pixels>) {
    cx.simulate_mouse_down(
        bounds.center(),
        gpui::MouseButton::Right,
        Modifiers::none(),
    );
    cx.simulate_mouse_up(
        bounds.center(),
        gpui::MouseButton::Right,
        Modifiers::none(),
    );
    redraw(cx);
}

#[gpui::test]
async fn right_clicking_a_table_cell_opens_the_cell_menu_and_inserts_below(
    cx: &mut TestAppContext,
) {
    use gpui_kit::test::TestWindowExt as _;
    init_editor_test_app(cx);
    let markdown = ["| A | B |", "| --- | --- |", "| 1 | 2 |", "| 3 | 4 |"].join("\n");
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, markdown.clone(), None));
    redraw(cx);
    redraw(cx);

    let bounds = table_cell_bounds(&editor, cx, 1, 0);
    right_click_cell(cx, bounds);
    editor.read_with(cx, |editor, _| match &editor.context_menu {
        Some(crate::editor::context_menu::ContextMenuState::TableCell { cell, .. }) => {
            assert_eq!((cell.row, cell.column), (1, 0), "菜单应记住被右击的格子");
        }
        _ => panic!("右键单元格应打开单元格菜单"),
    });

    // 菜单顺序：上方插行、下方插行、左侧插列、右侧插列……
    cx.update(|window, cx| {
        assert_eq!(
            window.try_find("popup-menu").and_then(|menu| menu.role()),
            Some(gpui::Role::Menu),
            "单元格菜单应使用 Kit 菜单语义"
        );
        for _ in 0..2 {
            window.press("down", cx);
        }
        window.press("enter", cx);
    });
    redraw(cx);
    assert_eq!(
        editor.read_with(cx, |editor, _| editor.buffer.text()),
        ["| A | B |", "| --- | --- |", "| 1 | 2 |", "|   |   |", "| 3 | 4 |"]
            .join("\n")
    );
}

#[gpui::test]
async fn header_cell_menu_cannot_insert_above_the_header(cx: &mut TestAppContext) {
    use gpui_kit::test::TestWindowExt as _;
    init_editor_test_app(cx);
    let markdown = ["| A | B |", "| --- | --- |", "| 1 | 2 |"].join("\n");
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, markdown.clone(), None));
    redraw(cx);
    redraw(cx);

    let bounds = table_cell_bounds(&editor, cx, 0, 0);
    right_click_cell(cx, bounds);
    editor.read_with(cx, |editor, _| match &editor.context_menu {
        Some(crate::editor::context_menu::ContextMenuState::TableCell { cell, .. }) => {
            assert_eq!(cell.row, 0, "菜单应记住表头格");
        }
        _ => panic!("右键单元格应打开单元格菜单"),
    });

    // 「上方插行」在表头格上是禁用项，键盘应落到第一项可用动作「下方插行」。
    cx.update(|window, cx| {
        window.press("down", cx);
        window.press("enter", cx);
    });
    redraw(cx);
    assert_eq!(
        editor.read_with(cx, |editor, _| editor.buffer.text()),
        ["| A | B |", "| --- | --- |", "|   |   |", "| 1 | 2 |"].join("\n")
    );
}

/// 仅剩表头的表上，单元格菜单的「删除行」与轴菜单同语义：没有数据行可升，
/// 删的就是整张表。以前这里静默什么都不做，两个菜单同一动作
/// 两种结果。
#[gpui::test]
async fn cell_menu_delete_row_on_a_header_only_table_removes_the_table(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, "| 唯一表头 |\n| --- |".into(), None));
    redraw(cx);
    redraw(cx);

    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            let table = editor.document.first_root().expect("表").clone();
            editor.context_menu =
                Some(crate::editor::context_menu::ContextMenuState::TableCell {
                    position: point(px(10.0), px(10.0)),
                    table_block_id: table.entity_id(),
                    cell: crate::components::TableCellPosition { row: 0, column: 0 },
                });
            editor.on_delete_table_cell_row(&ClickEvent::default(), window, cx);
            let root = editor
                .document
                .first_root()
                .expect("删表后应留一个空段落接住光标");
            assert_eq!(
                root.read(cx).kind(),
                BlockKind::Paragraph,
                "仅剩表头的表上「删除行」应删掉整张表"
            );
            assert!(root.read(cx).record.table.is_none());
        });
    });
}

/// 单元格菜单「在下方插入一行」，焦点落在「新行 × 右击的那一列」：
/// 用户的心智位置不丢，插完接着打字打的是右击那格的正下方。
#[gpui::test]
async fn cell_menu_insert_row_below_focuses_the_clicked_column(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let markdown = ["| A | B |", "| --- | --- |", "| 1 | 2 |"].join("\n");
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, markdown, None));
    redraw(cx);
    redraw(cx);

    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            let table = editor.document.first_root().expect("表").clone();
            editor.context_menu =
                Some(crate::editor::context_menu::ContextMenuState::TableCell {
                    position: point(px(10.0), px(10.0)),
                    table_block_id: table.entity_id(),
                    cell: crate::components::TableCellPosition { row: 1, column: 1 },
                });
            editor.on_insert_table_row_below(&ClickEvent::default(), window, cx);
            let focused = editor
                .active_entity_id
                .expect("插入后应有一个焦点格");
            let runtime = table.read(cx).table_runtime.as_ref().expect("运行时");
            assert_eq!(
                focused,
                runtime.rows[1][1].entity_id(),
                "插行后焦点应落在右击那一列的新行上"
            );
        });
    });
}

/// 单元格菜单「在左侧插入一列」，焦点落在「右击的那一行 × 新列」：
/// 第一下输入不许污染表头。
#[gpui::test]
async fn cell_menu_insert_column_focuses_the_clicked_row(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let markdown = ["| A | B |", "| --- | --- |", "| 1 | 2 |"].join("\n");
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, markdown, None));
    redraw(cx);
    redraw(cx);

    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            let table = editor.document.first_root().expect("表").clone();
            editor.context_menu =
                Some(crate::editor::context_menu::ContextMenuState::TableCell {
                    position: point(px(10.0), px(10.0)),
                    table_block_id: table.entity_id(),
                    cell: crate::components::TableCellPosition { row: 1, column: 0 },
                });
            editor.on_insert_table_column_left(&ClickEvent::default(), window, cx);
            let focused = editor
                .active_entity_id
                .expect("插入后应有一个焦点格");
            let runtime = table.read(cx).table_runtime.as_ref().expect("运行时");
            assert_eq!(
                focused,
                runtime.rows[0][0].entity_id(),
                "插列后焦点应落在右击那一行的新列上，而不是表头"
            );
        });
    });
}

#[gpui::test]
async fn cell_menu_inserts_a_column_to_the_right(cx: &mut TestAppContext) {
    use gpui_kit::test::TestWindowExt as _;
    init_editor_test_app(cx);
    let markdown = ["| A | B |", "| --- | ---: |", "| 1 | 2 |"].join("\n");
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, markdown.clone(), None));
    redraw(cx);
    redraw(cx);

    let bounds = table_cell_bounds(&editor, cx, 1, 1);
    right_click_cell(cx, bounds);
    cx.update(|window, cx| {
        for _ in 0..4 {
            window.press("down", cx);
        }
        window.press("enter", cx);
    });
    redraw(cx);
    assert_eq!(
        editor.read_with(cx, |editor, _| editor.buffer.text()),
        ["| A | B |  |", "| --- | ---: | ---: |", "| 1 | 2 |  |"].join("\n")
    );
}

#[gpui::test]
async fn setting_column_alignment_updates_record_and_selection(cx: &mut TestAppContext) {
    let markdown = ["| A | B |", "| --- | --- |", "| 1 | 2 |"].join("\n");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.update(cx, |editor, cx| {
        let table = editor.document.first_root().expect("table root").clone();
        editor.set_table_column_alignment(&table, 1, TableColumnAlignment::Right, cx);

        let record = table.read(cx).record.table.as_ref().expect("table record");
        assert_eq!(
            record.alignments,
            vec![TableColumnAlignment::Default, TableColumnAlignment::Right]
        );
        assert_eq!(
            editor.table_axis_selection,
            Some(crate::editor::TableAxisSelection {
                table_block_id: table.entity_id(),
                kind: crate::components::TableAxisKind::Column,
                index: 1,
            })
        );
    });
}

#[gpui::test]
async fn moving_table_row_updates_focus_and_selection(cx: &mut TestAppContext) {
    let markdown = ["| A | B |", "| --- | --- |", "| 1 | 2 |", "| 3 | 4 |"].join("\n");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.update(cx, |editor, cx| {
        let table = editor.document.first_root().expect("table root").clone();
        // Visual row 2 is the second body row; move it up above the first.
        editor.move_table_row(&table, 2, -1, cx);

        let record = table.read(cx).record.table.as_ref().expect("table record");
        assert_eq!(record.rows[0][0].serialize_markdown(), "3");
        assert_eq!(
            editor.table_axis_selection,
            Some(crate::editor::TableAxisSelection {
                table_block_id: table.entity_id(),
                kind: crate::components::TableAxisKind::Row,
                index: 1,
            })
        );

        let runtime = table
            .read(cx)
            .table_runtime
            .as_ref()
            .expect("rebuilt runtime");
        assert_eq!(editor.pending_focus, Some(runtime.rows[0][0].entity_id()));
    });
}

#[gpui::test]
async fn moving_first_body_row_up_swaps_with_header(cx: &mut TestAppContext) {
    let markdown = ["| A | B |", "| --- | --- |", "| 1 | 2 |", "| 3 | 4 |"].join("\n");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.update(cx, |editor, cx| {
        let table = editor.document.first_root().expect("table root").clone();
        // Visual row 1 (first body row) moves up into the header position.
        editor.move_table_row(&table, 1, -1, cx);

        let record = table.read(cx).record.table.as_ref().expect("table record");
        assert_eq!(record.header[0].serialize_markdown(), "1");
        assert_eq!(record.rows[0][0].serialize_markdown(), "A");
        assert_eq!(
            editor.table_axis_selection,
            Some(crate::editor::TableAxisSelection {
                table_block_id: table.entity_id(),
                kind: crate::components::TableAxisKind::Row,
                index: 0,
            })
        );
    });
}

#[gpui::test]
async fn moving_header_row_down_swaps_with_first_body(cx: &mut TestAppContext) {
    let markdown = ["| A | B |", "| --- | --- |", "| 1 | 2 |", "| 3 | 4 |"].join("\n");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.update(cx, |editor, cx| {
        let table = editor.document.first_root().expect("table root").clone();
        // Visual row 0 (header) moves down, swapping with the first body row.
        editor.move_table_row(&table, 0, 1, cx);

        let record = table.read(cx).record.table.as_ref().expect("table record");
        assert_eq!(record.header[0].serialize_markdown(), "1");
        assert_eq!(record.rows[0][0].serialize_markdown(), "A");
        assert_eq!(
            editor.table_axis_selection,
            Some(crate::editor::TableAxisSelection {
                table_block_id: table.entity_id(),
                kind: crate::components::TableAxisKind::Row,
                index: 1,
            })
        );
    });
}

#[gpui::test]
async fn column_selection_does_not_insert_a_blank_row(cx: &mut TestAppContext) {
    use crate::components::TableAxisKind;
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "| A | B |\n| --- | --- |\n| 1 | 2 |\n| 3 | 4 |".into(), None)
    });
    redraw(cx);
    let (table, header_before) = editor.read_with(cx, |editor, cx| {
        let table = editor.document.first_root().unwrap().clone();
        let header = table.read(cx).table_runtime.as_ref().unwrap().header[0].read(cx).last_bounds.unwrap();
        (table, header)
    });
    editor.update(cx, |editor, cx| {
        editor.select_table_axis(table.entity_id(), TableAxisKind::Column, 0, cx);
    });
    redraw(cx);
    let header_after = table.read_with(cx, |table, cx| {
        table.table_runtime.as_ref().unwrap().header[0].read(cx).last_bounds.unwrap()
    });
    assert_eq!(header_before, header_after, "选择列不能插入空白行或推动表头");
    let indicator = cx.debug_bounds("table-column-indicator-0").expect("表头中的列指示线");
    assert!(indicator.top() < header_after.top());
    assert_eq!(indicator.size.height, px(2.0));
    let second = cx.debug_bounds("table-column-indicator-1").unwrap();
    cx.simulate_click(second.center(), Modifiers::none());
    redraw(cx);
    editor.read_with(cx, |editor, _cx| {
        let selection = editor.table_axis_selection.unwrap();
        assert_eq!(selection.kind, TableAxisKind::Column);
        assert_eq!(selection.index, 1, "表头上沿仍应能直接选择整列");
    });
}

#[gpui::test]
async fn selecting_first_body_row_does_not_highlight_header(cx: &mut TestAppContext) {
    use crate::components::{TableAxisHighlight, TableAxisKind};
    let markdown = ["| A | B |", "| --- | --- |", "| 1 | 2 |", "| 3 | 4 |"].join("\n");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.update(cx, |editor, cx| {
        let table = editor.document.first_root().expect("table root").clone();
        // Visual row 1 is the first body row; the header (row 0) must stay clear.
        editor.select_table_axis(table.entity_id(), TableAxisKind::Row, 1, cx);

        let runtime = table.read(cx).table_runtime.clone().expect("runtime");
        for cell in &runtime.header {
            assert_eq!(
                cell.read(cx).table_axis_highlight,
                TableAxisHighlight::None,
                "header should not be highlighted"
            );
        }
        for cell in &runtime.rows[0] {
            assert_eq!(
                cell.read(cx).table_axis_highlight,
                TableAxisHighlight::Selected
            );
        }
        for cell in &runtime.rows[1] {
            assert_eq!(cell.read(cx).table_axis_highlight, TableAxisHighlight::None);
        }
    });
}

#[gpui::test]
async fn selecting_header_row_highlights_only_header(cx: &mut TestAppContext) {
    use crate::components::{TableAxisHighlight, TableAxisKind};
    let markdown = ["| A | B |", "| --- | --- |", "| 1 | 2 |"].join("\n");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.update(cx, |editor, cx| {
        let table = editor.document.first_root().expect("table root").clone();
        editor.select_table_axis(table.entity_id(), TableAxisKind::Row, 0, cx);

        let runtime = table.read(cx).table_runtime.clone().expect("runtime");
        for cell in &runtime.header {
            assert_eq!(
                cell.read(cx).table_axis_highlight,
                TableAxisHighlight::Selected
            );
        }
        for cell in &runtime.rows[0] {
            assert_eq!(cell.read(cx).table_axis_highlight, TableAxisHighlight::None);
        }
    });
}

#[gpui::test]
async fn body_row_preview_survives_stale_header_leave(cx: &mut TestAppContext) {
    use crate::components::TableAxisKind;
    let markdown = ["| A | B |", "| --- | --- |", "| 1 | 2 |"].join("\n");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.update(cx, |editor, cx| {
        let table = editor.document.first_root().expect("table root").clone();
        let id = table.entity_id();

        // Pointer crosses from the header handle down onto the first body row.
        // The body handle's enter arrives first, then the header handle's leave;
        // the stale leave must not clear the preview the pointer moved onto.
        editor.preview_table_axis(id, TableAxisKind::Row, 1, true, cx);
        editor.preview_table_axis(id, TableAxisKind::Row, 0, false, cx);
        assert_eq!(
            editor.table_axis_preview,
            Some(crate::editor::TableAxisSelection {
                table_block_id: id,
                kind: TableAxisKind::Row,
                index: 1,
            }),
            "body row preview must survive the header's stale leave"
        );

        // Leaving the body handle that owns the preview still clears it.
        editor.preview_table_axis(id, TableAxisKind::Row, 1, false, cx);
        assert_eq!(editor.table_axis_preview, None);
    });
}

#[gpui::test]
async fn deleting_table_column_moves_selection_to_nearest_survivor(cx: &mut TestAppContext) {
    let markdown = ["| A | B | C |", "| --- | --- | --- |", "| 1 | 2 | 3 |"].join("\n");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.update(cx, |editor, cx| {
        let table = editor.document.first_root().expect("table root").clone();
        editor.delete_table_column(&table, 2, cx);

        let record = table.read(cx).record.table.as_ref().expect("table record");
        assert_eq!(record.header.len(), 2);
        assert_eq!(
            editor.table_axis_selection,
            Some(crate::editor::TableAxisSelection {
                table_block_id: table.entity_id(),
                kind: crate::components::TableAxisKind::Column,
                index: 1,
            })
        );
    });
}

#[gpui::test]
async fn deleting_table_header_promotes_next_row(cx: &mut TestAppContext) {
    let markdown = ["| A | B |", "| --- | --- |", "| 1 | 2 |"].join("\n");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.update(cx, |editor, cx| {
        let table = editor.document.first_root().expect("table root").clone();
        editor.delete_table_header_row(&table, cx);

        let record = table.read(cx).record.table.as_ref().expect("table record");
        assert_eq!(record.header[0].serialize_markdown(), "1");
        assert_eq!(record.header[1].serialize_markdown(), "2");
        assert!(record.rows.is_empty());

        let runtime = table
            .read(cx)
            .table_runtime
            .as_ref()
            .expect("rebuilt runtime");
        assert_eq!(editor.pending_focus, Some(runtime.header[0].entity_id()));
    });
}

#[gpui::test]
async fn deleting_last_body_row_leaves_header_only_table(cx: &mut TestAppContext) {
    let markdown = ["| A | B |", "| --- | --- |", "| 1 | 2 |"].join("\n");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.update(cx, |editor, cx| {
        let table = editor.document.first_root().expect("table root").clone();
        // Deleting the only body row used to be blocked; now it leaves a
        // header-only table behind.
        editor.delete_table_row(&table, 0, cx);

        let record = table.read(cx).record.table.as_ref().expect("table record");
        assert!(record.rows.is_empty());
        assert_eq!(record.header[0].serialize_markdown(), "A");
        assert_eq!(editor.document.root_count(), 1);
        assert_eq!(table.read(cx).kind(), BlockKind::Table);
    });
}

#[gpui::test]
async fn removing_table_block_replaces_it_with_empty_paragraph(cx: &mut TestAppContext) {
    let markdown = [
        "intro",
        "",
        "| A | B |",
        "| --- | --- |",
        "| 1 | 2 |",
        "",
        "outro",
    ]
    .join("\n");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.update(cx, |editor, cx| {
        let table = editor.document.root_blocks()[1].clone();
        assert_eq!(table.read(cx).kind(), BlockKind::Table);
        editor.remove_table_block(&table, cx);

        let roots = editor.document.root_blocks();
        assert_eq!(roots.len(), 3);
        assert_eq!(roots[0].read(cx).display_text(), "intro");
        assert_eq!(roots[1].read(cx).kind(), BlockKind::Paragraph);
        assert_eq!(roots[1].read(cx).display_text(), "");
        assert_eq!(roots[2].read(cx).display_text(), "outro");
        assert_eq!(editor.pending_focus, Some(roots[1].entity_id()));
    });
}

#[gpui::test]
async fn removing_the_only_table_leaves_one_empty_paragraph(cx: &mut TestAppContext) {
    let markdown = ["| A | B |", "| --- | --- |", "| 1 | 2 |"].join("\n");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.update(cx, |editor, cx| {
        let table = editor.document.first_root().expect("table root").clone();
        editor.remove_table_block(&table, cx);

        let roots = editor.document.root_blocks();
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].read(cx).kind(), BlockKind::Paragraph);
        assert_eq!(roots[0].read(cx).display_text(), "");
    });
}


/// 表格轴高亮同步只写真变化（评审报告维度四建议 7）：旧实现对所有可见表 ×
/// 所有格无条件 update + notify，一次预览/选列 = 表块 + 全部格子都写一遍。
/// 写入前比旧值后，一次选列只剩目标表块与那一列的格子。
#[gpui::test]
async fn selecting_a_table_column_writes_only_that_column(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let markdown = ["| A | B | C |", "| --- | --- | --- |", "| 1 | 2 | 3 |", "| 4 | 5 | 6 |"].join("\n");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    let before = crate::editor::table_edit::probe::axis_writes();
    editor.update(cx, |editor, cx| {
        let table = editor.document.first_root().expect("table root").clone();
        editor.preview_table_axis(table.entity_id(), crate::components::TableAxisKind::Column, 1, true, cx);
        editor.select_table_axis(table.entity_id(), crate::components::TableAxisKind::Column, 1, cx);
    });
    let writes = crate::editor::table_edit::probe::axis_writes() - before;
    // 预览一趟 + 选中一趟：每趟目标表块 1 次 + 目标列 3 格（表头 + 两行正文）。
    // 旧形状是每趟 10 个实体全写（两趟 20 次）。
    assert_eq!(
        writes, 8,
        "一次预览 + 一次选列只许写目标表与那一列（实际 {writes} 次）"
    );
}

/// 单元格即使采用不同长度的换行标签，也要能把正文位置换算回原文。
#[gpui::test]
async fn table_cell_mappings_account_for_content_length(cx: &mut TestAppContext) {
    let markdown = "| a<br />b | 平文 |\n| --- | --- |\n";
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown.to_string(), None));
    editor.update(cx, |editor, cx| {
        let mappings = editor.source_mappings_in_range(&(0..markdown.len()), cx);
        let table = editor.document.first_root().expect("table root").clone();
        let header = table
            .read(cx)
            .table_runtime
            .as_ref()
            .expect("table runtime")
            .header
            .clone();
        let line_break_cell = &header[0];
        let plain = &header[1];
        assert_eq!(
            line_break_cell.read(cx).display_text(),
            "a\nb",
            "模型中的换行只有一字节"
        );
        let mapping_for = |cell: &gpui::Entity<crate::components::Block>| {
            mappings
                .iter()
                .find(|mapping| mapping.entity.entity_id() == cell.entity_id())
                .map(|mapping| mapping.content_to_source.len())
        };
        assert_eq!(
            editor.caret_source_offset(line_break_cell.entity_id(), 2, cx),
            markdown.rfind('b'),
            "标签后面的字符应落在原文 b 之前"
        );
        assert_eq!(
            mapping_for(plain),
            Some(7),
            "对得上号的格仍要推映射（平文 6 字节 → 0..=6 共 7 项）"
        );
    });
}


#[gpui::test]
fn native_object_context_menu_table_supports_keyboard_alignment_and_undo(cx: &mut TestAppContext) {
    use gpui_kit::test::TestWindowExt as _;
    init_editor_test_app(cx);
    let original = "| A | B |\n| --- | --- |\n| 1 | 2 |\n";
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, original.into(), None));
    redraw(cx);
    editor.update(cx, |editor, cx| {
        let table = editor.document.first_root().unwrap().entity_id();
        editor.open_table_axis_context_menu(
            gpui::point(px(80.0), px(80.0)),
            crate::editor::TableAxisSelection {
                table_block_id: table,
                kind: crate::components::TableAxisKind::Column,
                index: 1,
            },
            cx,
        );
    });
    redraw(cx);
    cx.update(|window, cx| {
        assert_eq!(
            window.try_find("popup-menu").and_then(|menu| menu.role()),
            Some(gpui::Role::Menu),
            "表格轴菜单应使用 Kit 菜单语义"
        );
        for _ in 0..3 {
            window.press("down", cx);
        }
        window.press("enter", cx);
    });
    redraw(cx);
    editor.read_with(cx, |editor, cx| {
        let table = editor
            .document
            .first_root()
            .unwrap()
            .read(cx)
            .record
            .table
            .as_ref()
            .unwrap();
        assert_eq!(table.alignments.get(1), Some(&TableColumnAlignment::Right));
    });
    editor.update(cx, |editor, cx| editor.undo_document(cx));
    redraw(cx);
    assert_eq!(
        editor.read_with(cx, |editor, _| editor.buffer.text()),
        original
    );
}

#[gpui::test]
fn native_object_context_menu_table_retargeting_returns_focus_to_the_document(
    cx: &mut TestAppContext,
) {
    use gpui_kit::test::TestWindowExt as _;
    init_editor_test_app(cx);
    for original in [
        "正文\n\n| A | B |\n| --- | --- |\n| 1 | 2 |\n",
        "| A | B |\n| --- | --- |\n| 1 | 2 |\n",
    ] {
        let (editor, cx) =
            cx.add_window_view(|_, cx| Editor::from_markdown(cx, original.into(), None));
        redraw(cx);
        redraw(cx);
        let previous_focus = cx
            .update(|window, cx| window.focused(cx))
            .expect("正文焦点");
        for index in [0, 1] {
            editor.update(cx, |editor, cx| {
                let table = editor
                    .document
                    .root_blocks()
                    .iter()
                    .find(|block| block.read(cx).kind() == BlockKind::Table)
                    .unwrap()
                    .entity_id();
                editor.open_table_axis_context_menu(
                    gpui::point(px(80.0), px(80.0)),
                    crate::editor::TableAxisSelection {
                        table_block_id: table,
                        kind: crate::components::TableAxisKind::Column,
                        index,
                    },
                    cx,
                );
            });
            redraw(cx);
        }
        cx.update(|window, cx| window.press("escape", cx));
        redraw(cx);
        cx.update(|window, cx| {
            editor.read_with(cx, |editor, _| {
                assert!(
                    previous_focus.is_focused(window),
                    "更换菜单目标后 Esc 应回到原来的正文焦点"
                );
                assert!(editor.context_menu.is_none());
                assert_eq!(editor.buffer.text(), original);
            })
        });
    }
}

/// 表格块自身的 padding/缩进区也该走块级命中链：点在这里应聚焦表格（落进
/// 格子），而不是落回上一段——表格臂曾另起裸 div，丢掉 render_shell 的
/// track_focus 与鼠标监听。
#[gpui::test]
async fn clicking_table_block_padding_focuses_the_table(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let markdown = ["内文", "", "| A | B |", "| --- | --- |", "| 1 | 2 |", "", "后记"].join("\n");
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, markdown.clone(), None));
    redraw(cx);
    redraw(cx);
    let (header, table_id, cell_ids) = editor.read_with(cx, |editor, cx| {
        let table = editor
            .document
            .root_blocks()
            .iter()
            .find(|block| block.read(cx).kind() == BlockKind::Table)
            .cloned()
            .expect("table root");
        let runtime = table.read(cx).table_runtime.clone().expect("table runtime");
        let bounds = runtime.header[0].read(cx).last_bounds.expect("header bounds");
        let ids: Vec<gpui::EntityId> = runtime
            .header
            .iter()
            .chain(runtime.rows.iter().flatten())
            .map(|cell| cell.entity_id())
            .collect();
        (bounds, table.entity_id(), ids)
    });
    let shell = cx
        .debug_bounds("table-block-shell")
        .expect("表格块的焦点壳");
    // 表格宿主按 95% 宽居中：壳左缘与宿主左缘之间留着一块块自己的 padding。
    let point = gpui::point(shell.left() + px(8.0), header.center().y);
    cx.simulate_click(point, Modifiers::none());
    redraw(cx);
    editor.read_with(cx, |editor, _| {
        let active = editor.active_entity_id.expect("点击后应有活动块");
        assert!(
            active == table_id || cell_ids.contains(&active),
            "点表格 padding 应聚焦表格/格子，实际落在 {active:?}"
        );
    });
}

/// 表头文字上沿那一条不该变成「选列」的隐形热区：列轴激活条要贴在自己
/// 预留的窄条里，按在表头文字上应进入文字编辑/拖选。
#[gpui::test]
async fn clicking_the_header_top_edge_edits_text_instead_of_selecting_the_column(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    let markdown = "| A | B |\n| --- | --- |\n| 1 | 2 |";
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, markdown.into(), None));
    redraw(cx);
    redraw(cx);
    let cell = table_cell_bounds(&editor, cx, 0, 0);
    let point = gpui::point(cell.center().x, cell.top() + px(2.0));
    cx.simulate_click(point, Modifiers::none());
    redraw(cx);
    editor.read_with(cx, |editor, cx| {
        let table = editor
            .document
            .root_blocks()
            .iter()
            .find(|block| block.read(cx).kind() == BlockKind::Table)
            .cloned()
            .expect("table root");
        assert!(
            table.read(cx).table_axis_selection.is_none(),
            "表头文字上沿的按下不该触发列选择"
        );
    });
}
/// 行轴选择带不能再伸出表格左边：祖先一旦裁切，整条带会消失。
/// 现在它贴在行内左侧（格内留白里），行的布局尺寸不变。
#[gpui::test]
async fn row_axis_bands_stay_inside_the_table(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| {
        Editor::from_markdown(cx, "| a | b |\n| --- | --- |\n| c | d |\n".into(), None)
    });
    redraw(cx);
    redraw(cx);
    for (row_selector, band_selector) in [
        ("table-header-row", "table-header-axis-band"),
        ("table-body-row", "table-body-axis-band"),
    ] {
        let row = cx.debug_bounds(row_selector).expect("行应有布局");
        let band = cx.debug_bounds(band_selector).expect("选择带应有布局");
        assert!(
            band.left() >= row.left() - gpui::px(0.5),
            "{band_selector} 伸到了行外：{band:?} / {row:?}"
        );
        assert!(
            band.right() <= row.right() + gpui::px(0.5),
            "{band_selector} 超出行的右边界"
        );
    }
}

