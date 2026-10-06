use super::common::*;

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

