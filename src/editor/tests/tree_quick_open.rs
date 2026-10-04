use super::common::*;

#[gpui::test]
async fn workspace_tree_scan_is_async_and_applies_result(cx: &mut TestAppContext) {
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
    let expected =
        std::fs::canonicalize(root.join("nested").join("note.md")).expect("canonicalize");
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(editor.workspace_text_files(), vec![expected.clone()]);
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
        // 第一次扫描尚未落地就切到新根：旧结果必须被丢弃。
        editor.set_workspace_root(root_b.clone(), cx);
    });

    cx.run_until_parked();
    let expected_b = std::fs::canonicalize(root_b.join("b.md")).expect("canonicalize");
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(editor.workspace_text_files(), vec![expected_b.clone()]);
    });
    let _ = std::fs::remove_dir_all(root_a);
    let _ = std::fs::remove_dir_all(root_b);
}

#[gpui::test]
async fn reopening_workspace_root_rescans_after_tree_is_cleared(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let root = std::env::temp_dir().join(format!("velora-tree-reopen-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("create root");
    std::fs::write(root.join("a.md"), "# a\n").expect("write");

    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
    editor.update(cx, |editor, cx| {
        editor.set_workspace_root(root.clone(), cx);
    });
    cx.run_until_parked();
    let expected = std::fs::canonicalize(root.join("a.md")).expect("canonicalize");
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(editor.workspace_text_files(), vec![expected.clone()]);
    });

    // 再次打开同一文件夹：旧树先清空，随后必须重新扫描出结果，
    // 不能因为"该根已扫描过"而卡在空树（roadmap D9 缓存标记）。
    editor.update(cx, |editor, cx| {
        editor.set_workspace_root(root.clone(), cx);
        assert!(editor.workspace_text_files().is_empty());
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
            .apply_heading_fold_filter(editor.document.visible_blocks().to_vec(), cx)
            .iter()
            .map(|visible| visible.entity.read(cx).display_text().to_string())
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
    cx.simulate_keystrokes("cmd-a");
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

        // 用户报修：跳转后选区落在「上一块末尾两个字 + 标题开头两个字」，
        // 即映射把标题行起点解析进了前一个块——绝不允许产生跨块选区。
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
    // 深度累积漂移（用户报修：光标落在「沟通风格」的沟和通之间）。
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
