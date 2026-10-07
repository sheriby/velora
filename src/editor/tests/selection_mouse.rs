use super::common::*;
use crate::editor::{TableTextPosition, TableTextSelection};
use crate::components::TableCellPosition;
use gpui::{Bounds, Entity};

#[gpui::test]
async fn selection_word_count_stays_cheap_on_a_long_document(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    // 状态栏每帧都要算选中词数。旧实现走 O(整篇) 的 markdown 序列化 +
    // source mapping 重建（600 块文档实测 38ms/次），长文档拖动选择卡死。
    let markdown = (0..300)
        .map(|index| {
            format!(
                "## 第 {index} 节标题\n\n这是第 {index} 段中文正文，足够长以便换行，含标点与英文 mixed text。\n"
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let (editor, cx) = cx.add_window_view(|_window, cx| Editor::from_markdown(cx, markdown, None));
    redraw(cx);

    editor.update(cx, |editor, cx| {
        let visible = editor.document.visible_blocks().to_vec();
        let first = visible[0].entity.entity_id();
        let last = visible[visible.len() - 1].entity.entity_id();
        editor.cross_block_selection = Some(crate::editor::CrossBlockSelection {
            anchor: crate::editor::CrossBlockSelectionEndpoint {
                entity_id: first,
                offset: 0,
            },
            focus: crate::editor::CrossBlockSelectionEndpoint {
                entity_id: last,
                offset: usize::MAX,
            },
        });

        let text = editor.selected_visible_text(cx).expect("selection text");
        assert!(text.contains("第 0 节标题"), "选中文本应包含首块内容");

        let calls = 20;
        let start = Instant::now();
        for _ in 0..calls {
            let _ = editor.selected_visible_text(cx);
        }
        let per_call = start.elapsed() / calls;
        println!(
            "[measure] selected_visible_text 单次 = {per_call:?}（可见块 {} 个）",
            visible.len()
        );
        assert!(
            per_call < Duration::from_millis(5),
            "状态栏选词路径又变回 O(整篇) 了：{per_call:?}（可见块 {} 个）",
            visible.len()
        );
    });
}

#[gpui::test]
async fn dragging_inside_a_rendered_code_block_does_not_panic(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    // 用户报修：在一篇长 markdown（含多个 fence）里，把鼠标放到 ``​`text`` 代码块内部
    // 按住拖动选中时 coredump。这里用同构文档真实模拟。
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
    // 用户报修：想去选另一段的文字，拖多少次都没反应，必须先单击那一段把光标
    // 落下去，第二次才拖得动。落点在没有聚焦的块上时只放光标、不起选区。
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

#[gpui::test]
async fn dragging_the_left_button_across_cjk_blocks_keeps_selecting(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    // 用户报修：左键按住拖动选择文本时又卡又容易崩。拖动路径上每次 mouse_move
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
        .map(|cell| cell.read(cx).editor_selection_range.clone())
        .collect()
}

/// 报修：按住拖动选不上表格里的文字，只能选出一个格子里的文字。
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
            },
            focus: TableTextPosition {
                cell: TableCellPosition { row: 1, column: 1 },
                offset: 1,
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
            .map(|visible| visible.entity.read(cx).editor_selection_range.clone())
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
