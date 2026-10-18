use super::common::*;

#[gpui::test]
async fn workspace_tree_and_quick_open_load_independently(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let root = std::env::temp_dir().join(format!("velora-async-tree-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(root.join("nested")).expect("create nested");
    std::fs::write(root.join("nested").join("note.md"), "# note\n").expect("write");

    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
    editor.update(cx, |editor, cx| {
        editor.set_workspace_root(root.clone(), cx);
        // 扫描不再同步发生在调用栈内（roadmap D9）。
        assert!(editor.workspace_text_files().is_empty());
    });

    cx.run_until_parked();
    editor.read_with(cx, |editor, _| {
        assert!(editor.workspace_text_files().is_empty(), "只加载文件树不应遍历全局文件");
    });
    editor.update_in(cx, |editor, window, cx| editor.toggle_quick_open(window, cx));
    cx.run_until_parked();
    let expected =
        crate::file_url::canonical_path(&root.join("nested").join("note.md"));
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(editor.workspace_text_files(), vec![expected.clone()]);
        assert_eq!(editor.quick_open.as_ref().expect("快速打开").results, vec![expected.clone()],
            "首次快速打开应在名单完成后刷新，包含未展开目录中的文件");
    });
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
async fn workspace_tree_scan_discards_stale_root_results(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let root_a = std::env::temp_dir().join(format!("velora-tree-a-{}", uuid::Uuid::new_v4()));
    let root_b = std::env::temp_dir().join(format!("velora-tree-b-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root_a).expect("create a");
    std::fs::create_dir_all(&root_b).expect("create b");
    std::fs::write(root_a.join("a.md"), "# a\n").expect("write a");
    std::fs::write(root_b.join("b.md"), "# b\n").expect("write b");

    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
    editor.update(cx, |editor, cx| {
        editor.set_workspace_root(root_a.clone(), cx);
        editor.ensure_workspace_files_walk(cx);
        // 第一次扫描尚未落地就切到新根：旧结果必须被丢弃。
        editor.set_workspace_root(root_b.clone(), cx);
        editor.ensure_workspace_files_walk(cx);
    });

    cx.run_until_parked();
    let expected_b = crate::file_url::canonical_path(&root_b.join("b.md"));
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(editor.workspace_text_files(), vec![expected_b.clone()]);
    });
    let _ = std::fs::remove_dir_all(root_a);
    let _ = std::fs::remove_dir_all(root_b);
}

#[gpui::test]
async fn reopening_workspace_root_reuses_the_loaded_file_list(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let root = std::env::temp_dir().join(format!("velora-tree-reopen-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("create root");
    std::fs::write(root.join("a.md"), "# a\n").expect("write");

    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
    editor.update(cx, |editor, cx| {
        editor.set_workspace_root(root.clone(), cx);
        editor.ensure_workspace_files_walk(cx);
    });
    cx.run_until_parked();
    let expected = crate::file_url::canonical_path(&root.join("a.md"));
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(editor.workspace_text_files(), vec![expected.clone()]);
    });

    // 同一工作区继续复用已经加载的名单，不闪空也不重新遍历目录。
    editor.update(cx, |editor, cx| {
        editor.set_workspace_root(root.clone(), cx);
        editor.ensure_workspace_files_walk(cx);
        assert_eq!(editor.workspace_text_files(), vec![expected.clone()]);
    });
    cx.run_until_parked();
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(editor.workspace_text_files(), vec![expected.clone()]);
    });
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
async fn heading_fold_chevron_toggle_hides_section_and_refocuses_heading(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    let source = "## Section\n\nalpha\n\n## Next\n\ngamma";
    let editor = cx.new(|cx| Editor::from_markdown(cx, source.into(), None));

    let (heading, paragraph) = editor.update(cx, |editor, _cx| {
        let visible = editor.document.visible_blocks().to_vec();
        (visible[0].entity.clone(), visible[1].entity.clone())
    });
    // 光标停留在章节内的 alpha 段落上。
    editor.update(cx, |editor, _cx| {
        editor.active_entity_id = Some(paragraph.entity_id());
    });

    // chevron 点击：块发出折叠请求，编辑器统一翻转折叠状态。
    heading.update(cx, |_block, cx| cx.emit(BlockEvent::RequestToggleFold));

    editor.update(cx, |editor, cx| {
        assert!(heading.read(cx).folded);
        // 折叠后光标所在段落被隐藏，焦点回到标题，避免输入静默丢失。
        assert_eq!(editor.pending_focus, Some(heading.entity_id()));
        assert_eq!(editor.active_entity_id, Some(heading.entity_id()));
        let filtered = editor
            .apply_heading_fold_filter(cx)
            .iter()
            .map(|&index| {
                editor.document.visible_blocks()[index as usize]
                    .entity
                    .read(cx)
                    .display_text()
                    .to_string()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            filtered,
            vec!["Section".to_string(), "Next".to_string(), "gamma".to_string()]
        );
    });
}

/// 断言快捷切换器只剩一个结果，且文件名匹配（工作区根会被规范化成 /private 前缀）。
fn assert_quick_open_result(results: &[PathBuf], expected_name: &str) {
    assert_eq!(results.len(), 1, "expected one match, got {results:?}");
    assert_eq!(
        results[0].file_name().and_then(|name| name.to_str()),
        Some(expected_name)
    );
}

/// ⌘P 面板的落点：偏上居中、叠在正文上——不能把正文整块换成灰板
/// （以前浮层顶替 content_area，后面的文档全没了，只剩遮罩的灰色）。
#[gpui::test]
async fn quick_open_panel_floats_top_centered_over_the_document(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "正文一行。".into(), None));
    redraw(cx);
    let before = cx
        .debug_bounds("editor-scroll")
        .expect("夹具：面板未开时正文的滚动容器应该在");

    editor.update_in(cx, |editor, window, cx| editor.toggle_quick_open(window, cx));
    redraw(cx);

    let after = cx
        .debug_bounds("editor-scroll")
        .expect("⌘P 面板开着时正文应仍渲染在面板后面");
    assert_eq!(
        before, after,
        "面板是叠上去的，正文几何不应被挤动：{before:?} -> {after:?}"
    );
    let overlay = cx.debug_bounds("quick-open-overlay").expect("遮罩应已绘制");
    let panel = cx.debug_bounds("quick-open-panel").expect("面板应已绘制");
    let overlay_center: f32 = (overlay.origin.x + overlay.size.width / 2.0).into();
    let panel_center: f32 = (panel.origin.x + panel.size.width / 2.0).into();
    assert!(
        (panel_center - overlay_center).abs() <= 1.0,
        "面板应水平居中：panel={panel:?} overlay={overlay:?}"
    );
    let top_offset: f32 = (panel.origin.y - overlay.origin.y).into();
    assert!(
        (top_offset - 64.0).abs() <= 1.0,
        "面板应偏上（距浮层顶 64）：{top_offset}"
    );
}

/// 组合期间那颗回车属于输入法提交候选词，不该拿去确认文件（与命令面板同一条判据）。
///
/// 探针直接喂 `on_quick_open_key_down`：⌘P 的按键要经窗口焦点路由，夹具里结果列表
/// 为空时确认本身是早退，走 `simulate_keystrokes` 会两头都「通过」而钉不住行为。
#[gpui::test]
async fn quick_open_ignores_enter_during_ime_composition(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let root = std::env::temp_dir().join(format!(
        "velora-quick-open-ime-enter-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).expect("create root");
    let root = std::fs::canonicalize(&root).unwrap_or(root);
    std::fs::write(root.join("alpha.md"), "# Alpha\n").expect("write alpha");

    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
    editor.update(cx, |editor, cx| editor.set_workspace_root(root.clone(), cx));
    editor.update_in(cx, |editor, window, cx| {
        editor.toggle_quick_open(window, cx)
    });
    redraw(cx);
    cx.simulate_input("alpha");

    let enter = gpui::KeyDownEvent {
        prefer_character_input: false,
        keystroke: gpui::Keystroke::parse("enter").expect("valid enter keystroke"),
        is_held: false,
    };
    let opened_before = editor.read_with(cx, |editor, _| editor.file_path.clone());

    editor.update_in(cx, |editor, window, cx| {
        let state = editor.quick_open.as_mut().expect("面板应开着");
        state.marked_range = Some(5..5);
        assert!(
            !state.results.is_empty(),
            "前置：候选列表要有条目，否则确认本身就是早退"
        );
        editor.on_quick_open_key_down(&enter, window, cx);
    });
    redraw(cx);
    assert!(
        editor.read_with(cx, |editor, _| editor.quick_open.is_some()),
        "组合期间这颗回车该只交还给输入法"
    );
    assert_eq!(
        editor.read_with(cx, |editor, _| editor.file_path.clone()),
        opened_before,
        "组合期间不该把高亮那条文件打开"
    );

    // 对照：组合落定之后同一颗回车照旧确认。
    editor.update_in(cx, |editor, window, cx| {
        editor
            .quick_open
            .as_mut()
            .expect("面板应还开着")
            .marked_range = None;
        editor.on_quick_open_key_down(&enter, window, cx);
    });
    redraw(cx);
    assert!(
        editor.read_with(cx, |editor, _| editor.quick_open.is_none()),
        "组合结束后该确认并打开高亮的那条"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
async fn quick_open_accepts_ime_text_for_non_ascii_file_names(cx: &mut TestAppContext) {
    // roadmap E9：⌘P 输入接 EntityInputHandler，中文文件名可直接用输入法拼写。
    init_editor_test_app(cx);
    let root = std::env::temp_dir().join(format!("velora-quick-open-ime-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("create root");
    let notes = root.join("笔记.md");
    std::fs::write(&notes, "# 笔记\n").expect("write notes");
    std::fs::write(root.join("alpha.md"), "# Alpha\n").expect("write alpha");

    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
    editor.update(cx, |editor, cx| editor.set_workspace_root(root.clone(), cx));
    editor.update_in(cx, |editor, window, cx| {
        editor.toggle_quick_open(window, cx)
    });
    redraw(cx);

    // 输入法提交路径：key_char → replace_text_in_range（同 macOS insertText）。
    cx.simulate_input("笔记");

    editor.read_with(cx, |editor, _cx| {
        let state = editor.quick_open.as_ref().expect("quick open stays open");
        assert_eq!(state.query, "笔记");
        assert_eq!(state.selected_range, 6..6);
        assert_eq!(state.marked_range, None);
        assert_quick_open_result(&state.results, "笔记.md");
    });

    // 纯 ASCII 也必须只插入一次（输入处理器接管后不再走手动按键插入）。
    cx.simulate_keystrokes(if cfg!(target_os = "macos") { "cmd-a" } else { "ctrl-a" });
    cx.simulate_input("alpha");
    editor.read_with(cx, |editor, _cx| {
        let state = editor.quick_open.as_ref().expect("quick open stays open");
        assert_eq!(state.query, "alpha");
        assert_eq!(state.selected_range, 5..5);
        assert_quick_open_result(&state.results, "alpha.md");
    });

    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
async fn quick_open_composition_commit_backspace_and_escape_edit_the_query(
    cx: &mut TestAppContext,
) {
    // roadmap E9：组合期标记由输入法接管，提交覆盖组合串；退格按字素删除。
    init_editor_test_app(cx);
    let root = std::env::temp_dir().join(format!("velora-quick-open-ime-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("create root");
    let notes = root.join("笔记.md");
    std::fs::write(&notes, "# 笔记\n").expect("write notes");

    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
    editor.update(cx, |editor, cx| editor.set_workspace_root(root.clone(), cx));
    editor.update_in(cx, |editor, window, cx| {
        editor.toggle_quick_open(window, cx)
    });
    redraw(cx);

    // 拼音组合中：marked_range 覆盖组合串，结果先按拼音过滤。
    editor.update_in(cx, |editor, window, cx| {
        editor.replace_and_mark_text_in_range(None, "biji", Some(0..4), window, cx);
    });
    editor.read_with(cx, |editor, _cx| {
        let state = editor.quick_open.as_ref().expect("quick open stays open");
        assert_eq!(state.query, "biji");
        assert_eq!(state.marked_range, Some(0..4));
    });

    // 提交：输入法用候选词覆盖组合串，并刷新结果。
    editor.update_in(cx, |editor, window, cx| {
        editor.replace_text_in_range(None, "笔记", window, cx);
    });
    editor.read_with(cx, |editor, _cx| {
        let state = editor.quick_open.as_ref().expect("quick open stays open");
        assert_eq!(state.query, "笔记");
        assert_eq!(state.marked_range, None);
        assert_quick_open_result(&state.results, "笔记.md");
    });

    // 退格删掉整个汉字（按字素，而不是按字节）。
    cx.simulate_keystrokes("backspace");
    editor.read_with(cx, |editor, _cx| {
        let state = editor.quick_open.as_ref().expect("quick open stays open");
        assert_eq!(state.query, "笔");
        assert_eq!(state.selected_range, 3..3);
    });

    // escape 关闭面板并清空查询。
    cx.simulate_keystrokes("escape");
    editor.read_with(cx, |editor, _cx| {
        assert!(editor.quick_open.is_none());
    });

    let _ = std::fs::remove_dir_all(root);
}


#[gpui::test]
async fn outline_jump_lands_inside_the_target_heading(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    // 复刻用户文档形态：front matter + 表格 + 分隔线之后的长文档深处标题。
    let source = concat!(
        "---\n",
        "name: ascend-kernel-developer\n",
        "---\n",
        "\n",
        "## System Prompt\n",
        "\n",
        "intro line\n",
        "\n",
        "| 项目 | 说明 |\n",
        "| --- | --- |\n",
        "| Phase 4 最大迭代 | 3 次，禁止超出 |\n",
        "| 语言 | 中文 |\n",
        "\n",
        "---\n",
        "\n",
        "## 沟通风格\n",
        "\n",
        "- 专业、技术、简洁\n",
    );
    let (editor, cx) = cx.add_window_view({
        let source = source.to_string();
        move |_window, cx| Editor::from_markdown(cx, source, None)
    });

    editor.update(cx, |editor, cx| {
        let buffer_text = editor.buffer.text();
        let line = buffer_text
            .lines()
            .position(|text| text == "## 沟通风格")
            .expect("target heading line");
        editor.open_outline_node(format!("outline-{line}"), line, cx);

        // 跳转后选区不得跨块：曾落在「上一块末尾两个字 + 标题开头两个字」，
        // 即映射把标题行起点解析进了前一个块。
        assert!(
            editor.cross_block_selection.is_none(),
            "大纲跳转到单行标题不应产生跨块选区"
        );
        let active_id = editor.active_entity_id.expect("outline jump focuses a block");
        let target = editor
            .document
            .visible_blocks()
            .into_iter()
            .find(|visible| visible.entity.entity_id() == active_id)
            .expect("active block is visible")
            .entity
            .clone();
        let block = target.read(cx);
        assert_eq!(
            block.kind(),
            BlockKind::Heading { level: 2 },
            "焦点块应是目标标题"
        );
        assert!(
            block.selected_range.start <= block.visible_len()
                && block.selected_range.end <= block.visible_len(),
            "选区应完全落在标题块内，实际 {:?}（可见长度 {}）",
            block.selected_range,
            block.visible_len()
        );
    });
}

#[gpui::test]
async fn source_mappings_align_with_the_real_document_text(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    // 复刻 ascend-kernel-developer.md 的真实构造：代码围栏/列表项后直接跟
    // `---`（无空行）、嵌套 front matter、双空行、表格。映射的
    // full_source_range 必须逐字节对齐原文，否则大纲跳转/搜索定位随文档
    // 深度累积漂移（曾表现为光标落在「沟通风格」的沟和通之间）。
    let source = concat!(
        "---\n",
        "name: ascend-kernel-developer\n",
        "argument-hint: >\n",
        "  输入格式: 生成算子\n",
        "---\n",
        "\n",
        "# System Prompt\n",
        "\n",
        "body text\n",
        "\n",
        "## 工作流\n",
        "\n",
        "```\n",
        "Phase 0: 参数确认\n",
        "Phase 1: 环境准备\n",
        "```\n",
        "---\n",
        "\n",
        "## 关键限制\n",
        "\n",
        "- 必须融合成单个算子\n",
        "- 禁止 torch 算子\n",
        "---\n",
        "\n",
        "## Phase 0: 参数确认\n",
        "\n",
        "| 参数 | 说明 |\n",
        "|------|------|\n",
        "| `npu` | 设备 ID |\n",
        "\n",
        "\n",
        "## 沟通风格\n",
        "\n",
        "- 专业、技术、简洁\n",
    );
    let (editor, cx) = cx.add_window_view({
        let source = source.to_string();
        move |_window, cx| Editor::from_markdown(cx, source, None)
    });

    editor.update(cx, |editor, cx| {
        let source = editor.buffer.text();
        for mapping in editor.build_source_target_mappings(cx) {
            let block = mapping.entity.read(cx);
            let range = &mapping.full_source_range;
            assert!(
                range.end <= source.len() && source.is_char_boundary(range.start),
                "块 {:?} 的映射范围 {:?} 越界/不合法",
                block.kind(),
                range
            );
            if let BlockKind::Heading { level } = block.kind() {
                let slice = &source[range.clone()];
                let expected_prefix =
                    format!("{}{} ", "  ".repeat(0), "#".repeat(level.clone() as usize));
                assert!(
                    slice.starts_with(&expected_prefix),
                    "标题映射未对齐原文：映射切到 {slice:?}，应为 {expected_prefix:?} 开头"
                );
                let title = block.record.title.visible_text().to_string();
                assert!(
                    slice.ends_with(&title),
                    "标题映射未对齐原文：映射切到 {slice:?}，应以标题 {title:?} 结尾"
                );
            }
        }
    });
}


/// 点大纲里的标题，展开的必须是那一行真正的标题块，而且不该为此重拼整篇映射。
///
/// 「哪一块含这一行的字节」本来就写在块的 `source_span` 里。这里额外用一个 Setext
/// 标题当靶子：它在文件里占两行、按块树重新序列化只占一行，任何按重新序列化算的行号
/// 都会指错块。整篇 source mapping 重建是 O(文档)（1 MiB 实测一次 227ms），大纲跟随
/// 滚动每帧都在调用链上，所以这条同时是个成本闸门。
#[gpui::test]
async fn clicking_an_outline_heading_keeps_that_heading_highlighted_after_scrolling(
    cx: &mut TestAppContext,
) {
    // README 中点 Look & feel 后正文已跳转，大纲高亮却被滚动跟随改回 Velora。
    // 跳转会把标题居中，点击选中必须在后续绘制中保留，不能按视口顶部覆盖。
    init_editor_test_app(cx);
    let source = include_str!("../../../README.md");
    let target_line = source
        .lines()
        .position(|line| line == "### Look & feel")
        .expect("README 中的目标标题");
    let target_id = format!("outline:{target_line}");
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, source.to_string(), None));
    cx.simulate_resize(gpui::size(px(1400.0), px(1000.0)));
    editor.update(cx, |editor, cx| {
        editor.workspace.is_open = true;
        editor.workspace.active_tab = crate::editor::workspace::WorkspaceTab::Outline;
        editor.sync_workspace_outline(cx);
        cx.notify();
    });
    redraw(cx);
    redraw(cx);
    let selector = format!(
        "workspace-node-{}",
        crate::editor::workspace::stable_node_hash(&target_id)
    );
    let row = cx
        .debug_bounds(Box::leak(selector.into_boxed_str()))
        .expect("Look & feel 大纲行");
    cx.simulate_click(row.center(), Modifiers::none());
    for _ in 0..6 {
        redraw(cx);
    }
    editor.read_with(cx, |editor, cx| {
        assert_eq!(
            editor.workspace.selected,
            Some(crate::editor::workspace::WorkspaceSelection::Outline(
                target_id.clone()
            )),
            "点击大纲并完成滚动后应保持目标行高亮"
        );
        let active = editor
            .active_entity_id
            .and_then(|id| editor.focusable_entity_by_id(id))
            .expect("跳转目标块");
        assert_eq!(active.read(cx).display_text(), "Look & feel");
        let bounds = active.read(cx).last_bounds.expect("目标标题的布局边界");
        let viewport = editor.scroll_handle.bounds();
        assert!(bounds.top() >= viewport.top() && bounds.bottom() <= viewport.bottom());
    });
    editor.update(cx, |editor, cx| {
        let heading = editor
            .heading_block_at_source_line(target_line, cx)
            .expect("目标标题");
        let bounds = heading.read(cx).last_bounds.expect("目标标题边界");
        let viewport = editor.scroll_handle.bounds();
        let target = editor.scroll_handle.offset().y + viewport.top() + px(48.0) - bounds.top();
        editor.set_vertical_scroll_offset(target, cx);
    });
    for _ in 0..3 {
        redraw(cx);
    }
    editor.read_with(cx, |editor, _| {
        assert_eq!(
            editor.workspace.selected,
            Some(crate::editor::workspace::WorkspaceSelection::Outline(target_id.clone())),
            "手动把 Look & feel 滚到顶部后，不能读到未挂载的 Velora 旧边界"
        );
    });
    editor.update(cx, |editor, cx| editor.set_vertical_scroll_offset(px(0.0), cx));
    for _ in 0..3 {
        redraw(cx);
    }
    editor.read_with(cx, |editor, _| {
        assert_eq!(
            editor.workspace.selected,
            Some(crate::editor::workspace::WorkspaceSelection::Outline(
                "outline:0".to_string()
            )),
            "手动滚回顶部后应恢复大纲跟随，选中 Velora"
        );
    });
    cx.simulate_click(row.center(), Modifiers::none());
    for _ in 0..6 {
        redraw(cx);
    }
    editor.read_with(cx, |editor, _| {
        assert_eq!(
            editor.workspace.selected,
            Some(crate::editor::workspace::WorkspaceSelection::Outline(target_id)),
            "手动滚动后再次点击仍应保持目标行高亮"
        );
    });
    let position = editor.read_with(cx, |editor, _| editor.scroll_handle.bounds().center());
    cx.simulate_event(gpui::ScrollWheelEvent {
        position,
        delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.0), px(10000.0))),
        modifiers: Modifiers::none(),
        touch_phase: gpui::TouchPhase::Moved,
    });
    for _ in 0..3 {
        redraw(cx);
    }
    editor.read_with(cx, |editor, _| {
        assert_eq!(editor.scroll_handle.offset().y, px(0.0));
        assert_eq!(
            editor.workspace.selected,
            Some(crate::editor::workspace::WorkspaceSelection::Outline(
                "outline:0".to_string()
            )),
            "滚轮回到顶部也应恢复大纲跟随"
        );
    });
}

#[gpui::test]
async fn clicking_an_outline_heading_unfolds_it_without_a_document_wide_mapping(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    let source = concat!(
        "章一\n",
        "=====\n",
        "\n",
        "正文甲\n",
        "\n",
        "## 章二\n",
        "\n",
        "正文乙\n",
    );
    let (editor, cx) = cx.add_window_view({
        let source = source.to_string();
        move |_window, cx| Editor::from_markdown(cx, source, None)
    });
    redraw(cx);

    editor.update(cx, |editor, cx| {
        for block in editor.document.root_blocks() {
            block.update(cx, |block, _cx| {
                if matches!(block.kind(), BlockKind::Heading { .. }) {
                    block.folded = true;
                }
            });
        }
    });
    redraw(cx);

    let builds_before = editor.read_with(cx, |editor, _| editor.source_mapping_full_builds.get());
    editor.update(cx, |editor, cx| {
        // 大纲条目记的是文件里的行号：Setext 标题就在第 0 行。
        editor.open_outline_node("outline-0".to_string(), 0, cx);
    });
    redraw(cx);

    let flags = editor.read_with(cx, |editor, cx| {
        editor
            .document
            .root_blocks()
            .iter()
            .map(|block| {
                block.read_with(cx, |block, _cx| {
                    let level = match block.kind() {
                        BlockKind::Heading { level } => level as usize,
                        _ => 0,
                    };
                    (level, block.folded)
                })
            })
            .collect::<Vec<_>>()
    });
    assert_eq!(
        flags,
        vec![(1, false), (0, false), (2, true), (0, false)],
        "点第 0 行只该展开那个一级 Setext 标题，另一个标题仍在折叠"
    );

    let builds_after = editor.read_with(cx, |editor, _| editor.source_mapping_full_builds.get());
    assert_eq!(
        builds_after - builds_before,
        0,
        "点一次大纲标题重拼了整篇 source mapping"
    );
}

/// 大纲跟随滚动：选中视口上方那一个标题，而且不为此整篇重拼映射。
///
/// 这条挂在每帧的绘制上（`render/paint.rs`）。块在缓冲区里的起点本来就记在块上，
/// 行号也问缓冲区就行；旧实现却为「哪一块含这个字节」把整篇 source mapping 重拼一遍，
/// 还把全文复制出来扫一遍换行符建 `Vec<usize>`（10 MiB 文档 ≈ 百万条 ≈ 8 MB），按
/// revision 缓存——于是每次编辑之后的第一帧都要付这一趟。这条路径此前没有测试。
#[gpui::test]
async fn scrolling_with_the_outline_open_follows_the_heading_above_the_viewport(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(
            cx,
            "# 章一\n\n正文甲\n\n# 章二\n\n正文乙\n".to_string(),
            None,
        )
    });
    redraw(cx);

    editor.update(cx, |editor, cx| {
        editor.workspace.is_open = true;
        editor.workspace.active_tab = crate::editor::workspace::WorkspaceTab::Outline;
        editor.sync_workspace_outline(cx);
        let target = editor
            .document
            .visible_blocks()
            .iter()
            .find(|entry| entry.entity.read(cx).display_text() == "正文乙")
            .expect("第二节正文")
            .entity
            .entity_id();
        let plan = editor.rendered_row_plan.as_ref().expect("已绘制的行计划");
        let row = plan.rows.iter().position(|row| row.first_id == target)
            .expect("正文对应的行");
        let run = editor.prev_mounted_run.expect("已挂载的行");
        let bounds = editor
            .scroll_handle
            .bounds_for_item(run.child_base + row - run.row_start)
            .expect("正文行的容器边界");
        // 把第二节正文移到跟随取样线，以真实行布局验证 source_span 行号。
        editor.scroll_handle.set_offset(gpui::point(px(0.0), px(48.0) - bounds.top()));
    });

    let builds_before = editor.read_with(cx, |editor, _| editor.source_mapping_full_builds.get());
    editor.update(cx, |editor, cx| {
        editor.sync_outline_follow_scroll(gpui::px(0.0), cx);
    });
    let builds_after = editor.read_with(cx, |editor, _| editor.source_mapping_full_builds.get());

    editor.read_with(cx, |editor, _cx| {
        assert_eq!(
            editor.workspace.selected,
            Some(crate::editor::workspace::WorkspaceSelection::Outline(
                "outline:4".to_string()
            )),
            "滚动跟随应选中视口上方那一个标题（文件里第 5 行的 `# 章二`）"
        );
    });
    assert_eq!(
        builds_after - builds_before,
        0,
        "滚动跟随一次重拼了整篇 source mapping"
    );
}

/// 工作区根会一路流进「打开最近」、会话文件与菜单标签：Windows 上 `canonicalize`
/// 给的 `\\?\C:\…` 抄到那三处就是用户看不懂的一列（报修：打开最近全是它）。
/// 落进工作区状态的必须是用户认得的那条写法。
#[gpui::test]
async fn workspace_root_is_kept_in_the_form_the_user_recognizes(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let root = std::env::temp_dir().join(format!("velora-root-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("create root");

    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
    editor.update(cx, |editor, cx| {
        editor.set_workspace_root(root.clone(), cx);
        let stored = editor
            .workspace
            .root
            .clone()
            .expect("工作区根要落位");
        assert!(
            !stored.to_string_lossy().starts_with(r"\\?\"),
            "工作区根不该是 verbatim 写法：{}",
            stored.display()
        );
        // 与「打开最近」同一口径：两边剥完之后是同一条路径，不会出现同一目录两行。
        assert_eq!(
            stored,
            crate::file_url::strip_verbatim_prefix(
                &crate::file_url::canonical_path(&root)
            )
        );
    });
    let _ = std::fs::remove_dir_all(root);
}

/// 「打开最近」点一条工作区：就该就地换工作区（同一个窗口里侧栏跟着换）。
#[gpui::test]
async fn a_recent_workspace_entry_switches_the_workspace(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let root_a = std::env::temp_dir().join(format!("velora-recent-a-{}", uuid::Uuid::new_v4()));
    let root_b = std::env::temp_dir().join(format!("velora-recent-b-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root_a).expect("create a");
    std::fs::create_dir_all(&root_b).expect("create b");
    std::fs::write(root_a.join("a.md"), "# a\n").expect("write a");
    std::fs::write(root_b.join("b.md"), "# b\n").expect("write b");

    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
    editor.update(cx, |editor, cx| editor.set_workspace_root(root_a.clone(), cx));
    cx.run_until_parked();
    assert_eq!(
        editor.read_with(cx, |editor, _| editor.workspace.root.clone()),
        Some(crate::file_url::canonical_path(&root_a))
    );

    // 菜单条目携带的是标签字符串（全路径），点下去就走这条派发。
    let label = crate::file_url::canonical_path(&root_b)
        .to_string_lossy()
        .into_owned();
    cx.update(|window, cx| {
        crate::app_menu::dispatch_menu_action_for_editor(
            &crate::components::OpenRecentFile { path: label },
            &editor.downgrade(),
            window,
            cx,
        )
    });
    editor.update(cx, |editor, cx| { editor.ensure_workspace_files_walk(cx); });
    cx.run_until_parked();

    editor.read_with(cx, |editor, _| {
        assert_eq!(
            editor.workspace.root,
            Some(crate::file_url::canonical_path(&root_b)),
            "点最近工作区应当就地换根"
        );
        assert!(
            !editor.workspace_text_files().is_empty(),
            "换了根要重新列出文件：{:?}",
            editor.workspace_text_files()
        );
    });

    let _ = std::fs::remove_dir_all(root_a);
    let _ = std::fs::remove_dir_all(root_b);
}

/// 同一条派发路上的两条分支：点最近**文件**要开成标签，点已经不在的条目要给出提示。
/// 两者以前都栽在同一个坑里——点击处理器里按窗口句柄再 update，窗口已被 take 出表，
/// `let _ =` 把「window not found」吞掉，点了什么都不发生。
#[gpui::test]
async fn a_recent_entry_opens_a_file_and_reports_a_missing_one(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let root = std::env::temp_dir().join(format!("velora-recent-file-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("create root");
    let note = root.join("note.md");
    std::fs::write(&note, "# note\n").expect("write note");
    let missing = root.join("gone.md");

    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
    editor.update(cx, |editor, cx| editor.set_workspace_root(root.clone(), cx));
    cx.run_until_parked();

    let note_label = crate::file_url::canonical_path(&note)
        .to_string_lossy()
        .into_owned();
    cx.update(|window, cx| {
        crate::app_menu::dispatch_menu_action_for_editor(
            &crate::components::OpenRecentFile { path: note_label.clone() },
            &editor.downgrade(),
            window,
            cx,
        )
    });
    cx.run_until_parked();
    editor.read_with(cx, |editor, _| {
        assert_eq!(
            editor.file_path.as_deref(),
            Some(crate::file_url::canonical_path(&note).as_path()),
            "点最近文件应当开成标签"
        );
    });

    cx.update(|window, cx| {
        crate::app_menu::dispatch_menu_action_for_editor(
            &crate::components::OpenRecentFile {
                path: missing.to_string_lossy().into_owned(),
            },
            &editor.downgrade(),
            window,
            cx,
        )
    });
    cx.run_until_parked();
    assert!(
        editor.read_with(cx, |editor, _| editor.modal_is_open()),
        "条目已经不在时要给出提示，不能静默什么都不做"
    );

    let _ = std::fs::remove_dir_all(root);
}

fn check_long_outline_follow(cx: &mut TestAppContext, headings: usize, zoom: i64) {
    use crate::editor::workspace::{WorkspaceSelection, WorkspaceTab, stable_node_hash};
    let root = temp_fixture_dir().join(format!("outline-follow-{}", temp_fixture_token()));
    fs::create_dir_all(&root).unwrap();
    let _config = crate::config::override_test_config_root(&root);
    cx.on_quit(move || fs::remove_dir_all(root).unwrap());
    init_editor_test_app(cx);
    cx.update(|cx| {
        crate::config::EditorSettings::install_test_settings(cx, false);
        crate::config::EditorSettings::set_zoom_percent(cx, zoom);
    });
    let source = (0..headings)
        .map(|index| format!("# 第 {index} 节\n\n这一节的正文内容。\n\n"))
        .collect::<String>();
    let (editor, visual) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, source, None));
    visual.simulate_resize(gpui::size(px(1200.0), px(720.0)));
    editor.update(visual, |editor, cx| {
        editor.workspace.is_open = true;
        editor.workspace.active_tab = WorkspaceTab::Outline;
        editor.sync_workspace_outline(cx);
        cx.notify();
    });
    for _ in 0..4 {
        redraw(visual);
    }
    let body = editor.read_with(visual, |editor, _| editor.scroll_handle.bounds().center());
    let scroll = |visual: &mut VisualTestContext, position, delta| {
        visual.simulate_event(gpui::ScrollWheelEvent {
            position,
            delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.0), px(delta))),
            modifiers: Modifiers::none(),
            touch_phase: gpui::TouchPhase::Moved,
        });
        for _ in 0..6 {
            redraw(visual);
        }
    };
    scroll(visual, body, -250.0);
    assert_eq!(
        editor.read_with(visual, |editor, _| editor
            .workspace
            .tree_scroll_handle
            .offset()
            .y),
        px(0.0),
        "当前标题仍可见时不应移动大纲"
    );
    let distance = editor.read_with(visual, |editor, _| {
        f32::from(editor.scroll_handle.max_offset().y) * 0.7
    });
    assert!(distance > 1000.0, "正文应足够长");
    scroll(visual, body, -distance);
    let (selected, sidebar, body_offset) = editor.read_with(visual, |editor, _| {
        let Some(WorkspaceSelection::Outline(selected)) = &editor.workspace.selected else {
            panic!("应跟随正文选中标题")
        };
        (
            selected.clone(),
            editor.workspace.tree_scroll_handle.bounds(),
            editor.scroll_handle.offset().y,
        )
    });
    assert_ne!(selected, "outline:0");
    let selector = format!("workspace-node-{}", stable_node_hash(&selected));
    let row = visual
        .debug_bounds(Box::leak(selector.into_boxed_str()))
        .expect("正文当前标题对应的大纲行应已渲染");
    assert!(
        row.top() >= sidebar.top() && row.bottom() <= sidebar.bottom(),
        "{headings} 条大纲、{zoom}% 缩放：当前标题应滚入可视区域，行 {row:?}，视口 {sidebar:?}"
    );
    scroll(visual, sidebar.center(), 1000000.0);
    assert_eq!(
        editor.read_with(visual, |editor, _| editor
            .workspace
            .tree_scroll_handle
            .offset()
            .y),
        px(0.0),
        "手动浏览大纲时不能被当前标题反复拉回"
    );
    assert_eq!(
        editor.read_with(visual, |editor, _| editor.scroll_handle.offset().y),
        body_offset
    );
    scroll(visual, sidebar.center(), -1000000.0);
    assert!(
        editor.read_with(visual, |editor, _| editor
            .workspace
            .tree_scroll_handle
            .offset()
            .y)
            < px(0.0)
    );
    scroll(visual, body, 1000000.0);
    editor.read_with(visual, |editor, _| {
        assert_eq!(
            editor.workspace.selected,
            Some(WorkspaceSelection::Outline("outline:0".into()))
        );
        assert_eq!(
            editor.workspace.tree_scroll_handle.offset().y,
            px(0.0),
            "正文回到开头时大纲也应把当前标题滚入可视区域"
        );
    });
}

#[gpui::test]
async fn long_outline_scrolls_active_heading_into_view_without_overriding_manual_scroll(
    cx: &mut TestAppContext,
) {
    check_long_outline_follow(cx, 80, 100);
}

#[gpui::test]
async fn virtual_outline_scrolls_active_heading_into_view_without_overriding_manual_scroll(
    cx: &mut TestAppContext,
) {
    check_long_outline_follow(cx, 260, 150);
}

#[gpui::test]
async fn overscrolling_virtual_outline_keeps_its_render_window_in_range(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, visual) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, "# 标题\n".into(), None));
    for _ in 0..2 {
        redraw(visual);
    }
    let range = editor.update(visual, |editor, cx| {
        let previous = editor.workspace.tree_scroll_handle.offset();
        editor
            .workspace
            .tree_scroll_handle
            .set_offset(gpui::point(px(0.0), px(-1000000.0)));
        let theme = cx.global::<ThemeManager>().derived_theme(cx);
        let range = editor.workspace_list_window(260, &theme);
        editor.workspace.tree_scroll_handle.set_offset(previous);
        range
    });
    assert!(
        range.start <= range.end && range.end <= 260,
        "快速滚动后的列表范围不能越界：{range:?}"
    );
    assert_eq!(range.end, 260, "大幅下滚后仍应包含末尾条目");
}

#[gpui::test]
async fn outline_arrows_toggle_nested_headings_without_navigating(cx: &mut TestAppContext) {
    use crate::editor::workspace::{WorkspaceSelection, WorkspaceTab, stable_node_hash};
    init_editor_test_app(cx);
    let source =
        "# 章一\n\n正文甲\n\n## 子节\n\n正文乙\n\n### 孙节\n\n正文丙\n\n# 章二\n\n正文丁\n";
    let (editor, visual) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, source.into(), None));
    editor.update(visual, |editor, cx| {
        editor.workspace.is_open = true;
        editor.workspace.active_tab = WorkspaceTab::Outline;
        editor.sync_workspace_outline(cx);
        cx.notify();
    });
    for _ in 0..3 {
        redraw(visual);
    }
    let row = |visual: &mut VisualTestContext, line| {
        let selector = format!(
            "workspace-node-{}",
            stable_node_hash(&format!("outline:{line}"))
        );
        visual.debug_bounds(Box::leak(selector.into_boxed_str()))
    };
    let next = row(visual, 12).expect("第二章的大纲行");
    visual.simulate_click(next.center(), Modifiers::none());
    for _ in 0..3 {
        redraw(visual);
    }
    let selected = Some(WorkspaceSelection::Outline("outline:12".into()));
    assert_eq!(
        editor.read_with(visual, |editor, _| editor.workspace.selected.clone()),
        selected
    );
    let body_offset = editor.read_with(visual, |editor, _| editor.scroll_handle.offset());
    let click_arrow = |visual: &mut VisualTestContext, line, depth: f32| {
        let bounds = row(visual, line).expect("有下级标题的大纲行");
        visual.simulate_click(
            gpui::point(bounds.left() + px(14.0 + depth * 16.0), bounds.center().y),
            Modifiers::none(),
        );
        for _ in 0..3 {
            redraw(visual);
        }
    };
    click_arrow(visual, 4, 1.0);
    assert!(
        !editor.read_with(visual, |editor, _| editor
            .workspace
            .expanded
            .contains("outline:4")),
        "点击子节箭头应折叠，而不是按整行点击重新展开"
    );
    assert!(row(visual, 4).is_some());
    assert!(row(visual, 8).is_none(), "折叠子节后孙节应隐藏");
    click_arrow(visual, 0, 0.0);
    assert!(!editor.read_with(visual, |editor, _| {
        editor.workspace.expanded.contains("outline:0")
    }));
    assert!(row(visual, 4).is_none(), "折叠章后子节应隐藏");
    click_arrow(visual, 0, 0.0);
    assert!(row(visual, 4).is_some(), "重新展开章后子节应恢复");
    assert!(row(visual, 8).is_none(), "父级展开不能重置子节的折叠状态");
    click_arrow(visual, 4, 1.0);
    assert!(row(visual, 8).is_some(), "再次展开子节后孙节应恢复");
    editor.read_with(visual, |editor, cx| {
        assert_eq!(
            editor.workspace.selected, selected,
            "点击折叠箭头不应改变当前标题"
        );
        assert_eq!(
            editor.scroll_handle.offset(),
            body_offset,
            "折叠大纲不应跳转正文"
        );
        assert_eq!(editor.buffer.text(), source);
        assert!(
            editor
                .document
                .root_blocks()
                .iter()
                .all(|block| !block.read(cx).folded),
            "大纲折叠不应折叠正文"
        );
    });
}
