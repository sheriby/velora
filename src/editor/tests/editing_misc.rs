use super::common::*;

#[gpui::test]
async fn undo_reverts_recent_rendered_typing(cx: &mut TestAppContext) {
    let editor = cx.new(|cx| Editor::from_markdown(cx, "alpha".to_string(), None));

    editor.update(cx, |editor, cx| {
        let block = editor.document.first_root().expect("root").clone();
        editor.active_entity_id = Some(block.entity_id());
        block.update(cx, |block, cx| {
            block.prepare_undo_capture(crate::components::UndoCaptureKind::CoalescibleText, cx);
            block.replace_text_in_visible_range(5..5, " beta", None, false, cx);
        });
    });

    editor.update(cx, |editor, cx| {
        assert_eq!(editor.document.markdown_text(cx), "alpha beta");
        assert_eq!(editor.undo_history.len(), 1);
        editor.undo_document(cx);
        assert_eq!(editor.document.markdown_text(cx), "alpha");
    });
}

#[gpui::test]
async fn toc_block_renders_entries_and_jumps_to_heading(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let markdown = "# Title\n\n[TOC]\n\n## Section\n\nbody";
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, markdown.into(), None));
    redraw(cx);

    let (toc_block, section_heading) = editor.update(cx, |editor, _cx| {
        let visible = editor.document.visible_blocks().to_vec();
        assert_eq!(visible.len(), 4); // Title, TOC, Section, body
        (visible[1].entity.clone(), visible[2].entity.clone())
    });
    editor.read_with(cx, |_editor, cx| {
        let entries = &toc_block.read(cx).toc_entries;
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].title, "Title");
        assert_eq!(entries[1].title, "Section");
        assert_eq!(entries[1].line, 4);
    });

    // 目录条目按层级渲染并可点击。
    let bounds = cx
        .debug_bounds("toc-entry-1")
        .expect("second TOC entry is rendered");
    cx.simulate_click(bounds.center(), Modifiers::none());
    redraw(cx);

    editor.read_with(cx, |editor, cx| {
        assert_eq!(
            editor.active_entity_id,
            Some(section_heading.entity_id()),
            "clicking a TOC entry focuses the target heading"
        );
        // 光标落在 `## Section` 这一行上（"# Title\n\n[TOC]\n\n" 共 16 字节，
        // 渲染态把整行选区收敛到标题文字末尾，即第 26 字节）。
        let caret = editor.capture_source_selection_snapshot(cx).range.start;
        assert!(
            (16..=26).contains(&caret),
            "caret should land on the heading line, got {caret}"
        );
    });
}

#[gpui::test]
async fn manual_external_change_policy_keeps_buffer_until_user_reload(cx: &mut TestAppContext) {
    // roadmap H2：外部变更策略 manual 时不自动重载，auto 时重载。
    init_editor_test_app(cx);
    let path = temp_markdown_path("external-policy");
    fs::write(&path, "disk v1").expect("seed");
    let cleanup = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup);
    });

    let (editor, cx) = cx.add_window_view({
        let path = path.clone();
        move |_window, cx| Editor::from_markdown(cx, "disk v1".into(), Some(path))
    });

    cx.update(|_window, cx| {
        // 安装 EditorSettings 全局（否则 setter 只落盘、不改内存）。
        crate::config::EditorSettings::init(cx, true);
        crate::config::EditorSettings::set_external_change_policy(
            cx,
            crate::config::ExternalChangePolicy::Manual,
        );
    });
    fs::write(&path, "disk v2").expect("external write");
    editor.update(cx, |editor, cx| {
        editor.reload_externally_changed_document(&path, cx);
    });
    // 重载读盘在后台 executor 上（P175）：manual 策略在排任务前就返回，
    // auto 策略要泵完后台任务才落地。
    cx.run_until_parked();
    editor.read_with(cx, |editor, cx| {
        assert_eq!(
            editor.document.markdown_text(cx),
            "disk v1",
            "manual policy must not reload externally changed documents"
        );
    });

    cx.update(|_window, cx| {
        crate::config::EditorSettings::set_external_change_policy(
            cx,
            crate::config::ExternalChangePolicy::Auto,
        );
    });
    editor.update(cx, |editor, cx| {
        editor.reload_externally_changed_document(&path, cx);
    });
    cx.run_until_parked();
    editor.read_with(cx, |editor, cx| {
        assert_eq!(
            editor.document.markdown_text(cx),
            "disk v2",
            "auto policy reloads unedited documents"
        );
    });
    // 复原默认策略，避免影响同进程其他用例读取配置。
    cx.update(|_window, cx| {
        crate::config::EditorSettings::set_external_change_policy(
            cx,
            crate::config::ExternalChangePolicy::Auto,
        );
    });
}

#[test]
fn permanent_delete_removes_file_and_directory() {
    // roadmap H2：永久删除策略的落盘行为。
    let root = std::env::temp_dir().join(format!("velora-permadelete-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(root.join("nested")).expect("create nested");
    let file = root.join("gone.md");
    fs::write(&file, "bye").expect("write file");

    crate::editor::workspace::permanent_delete(&file, false).expect("delete file");
    assert!(!file.exists());
    crate::editor::workspace::permanent_delete(&root.join("nested"), true).expect("delete dir");
    assert!(!root.join("nested").exists());
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
async fn recovery_snapshot_merges_into_open_session_tab(cx: &mut TestAppContext) {
    // roadmap E10：会话已打开同一文件时，恢复快照并入标签而不是另开窗口。
    init_editor_test_app(cx);
    let path = temp_markdown_path("merge-recovery");
    fs::write(&path, "disk version\n").expect("seed file");
    let cleanup = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup);
    });

    let (editor, cx) = cx.add_window_view({
        let path = path.clone();
        move |_window, cx| Editor::from_markdown(cx, "disk version\n".into(), Some(path))
    });
    let snapshot_id = uuid::Uuid::new_v4();
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            // 注册为会话标签（模拟 A4 会话恢复已打开该文件）。
            editor.snapshot_current_document(cx);
            assert!(
                editor
                    .workspace_open_document_paths()
                    .iter()
                    .any(|open| open == &path)
            );

            assert!(editor.merge_recovery_snapshot(
                &path,
                "unsaved edits\n",
                snapshot_id,
                window,
                cx,
            ));
            assert_eq!(editor.document.markdown_text(cx).trim_end(), "unsaved edits");
            assert!(editor.document_dirty);
            assert_eq!(editor.recovery_id, snapshot_id);
            let (dirty, tab_recovery_id, markdown) = editor
                .workspace_tab_state_for_test(&path)
                .expect("tab");
            assert!(dirty);
            assert_eq!(tab_recovery_id, snapshot_id);
            assert_eq!(markdown.trim_end(), "unsaved edits");
        });
    });
    // 磁盘仍是旧内容，未保存内容留在内存标签里。
    assert_eq!(fs::read_to_string(&path).unwrap(), "disk version\n");
}

#[gpui::test]
async fn tree_copy_then_paste_duplicates_file_into_selected_folder(cx: &mut TestAppContext) {
    // roadmap D6：树右键 复制 → 目标目录 粘贴 生成副本。
    init_editor_test_app(cx);
    let root = std::env::temp_dir().join(format!("velora-tree-paste-{}", uuid::Uuid::new_v4()));
    let sub = root.join("notes");
    std::fs::create_dir_all(&sub).expect("create sub");
    let doc = root.join("alpha.md");
    std::fs::write(&doc, "# Alpha\n").expect("write doc");

    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, String::new(), None)
    });
    editor.update(cx, |editor, cx| {
        editor.set_workspace_root(root.clone(), cx);
        editor.select_workspace_path_for_test(doc.clone(), cx);
        editor.copy_selected_workspace_file(cx);
        assert_eq!(editor.tree_clipboard.as_deref(), Some(doc.as_path()));
        editor.select_workspace_path_for_test(sub.clone(), cx);
        editor.paste_into_workspace_tree(cx);
    });

    let pasted = sub.join("alpha copy.md");
    assert!(pasted.exists(), "paste should create the copy in the folder");
    assert_eq!(std::fs::read_to_string(&pasted).unwrap(), "# Alpha\n");
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
async fn tree_paste_writes_clipboard_image_with_date_hash_name(cx: &mut TestAppContext) {
    // roadmap D6：剪贴板图片粘贴到树，沿用 B10 命名模板。
    init_editor_test_app(cx);
    let root = std::env::temp_dir().join(format!("velora-tree-img-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("create root");

    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, String::new(), None)
    });
    editor.update(cx, |editor, cx| {
        editor.set_workspace_root(root.clone(), cx);
        editor.tree_clipboard = None;
        cx.write_to_clipboard(gpui::ClipboardItem::new_image(&gpui::Image::from_bytes(
            gpui::ImageFormat::Png,
            vec![0x89, b'P', b'N', b'G'],
        )));
        editor.select_workspace_path_for_test(root.clone(), cx);
        editor.paste_into_workspace_tree(cx);
    });

    let entries: Vec<_> = std::fs::read_dir(&root)
        .expect("read root")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    let image_name = entries
        .iter()
        .find(|name| name.ends_with(".png"))
        .unwrap_or_else(|| panic!("expected a pasted png, got {entries:?}"));
    let stem = image_name.trim_end_matches(".png");
    let parts: Vec<&str> = stem.rsplitn(2, '-').collect();
    assert_eq!(parts[0].len(), 8, "hash suffix: {stem}");
    assert!(parts[0].chars().all(|ch| ch.is_ascii_hexdigit()));
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
async fn long_paragraph_renders_as_plain_source(cx: &mut TestAppContext) {
    // roadmap B12：单块超阈值时渲染态降级为源码文本。
    init_editor_test_app(cx);
    let long_line = "x".repeat(crate::components::LONG_BLOCK_SOURCE_LIMIT + 1);
    let markdown = format!("# Title\n\n{long_line}\n\nshort");
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, markdown, None));
    redraw(cx);

    assert!(
        cx.debug_bounds("block-long-source").is_some(),
        "over-limit block falls back to the plain source element"
    );
    let blocks = editor.read_with(cx, |editor, _cx| editor.document.visible_blocks().to_vec());
    let long_block = &blocks[1].entity;
    assert!(
        long_block.read_with(cx, |block, _cx| block.exceeds_long_block_source_limit()),
        "the over-limit block reports the degraded state"
    );
    assert!(
        !blocks[2]
            .entity
            .read_with(cx, |block, _cx| block.exceeds_long_block_source_limit()),
        "short blocks keep the rich render path"
    );
}

#[gpui::test]
async fn code_block_copy_button_copies_code_to_clipboard(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let markdown = "正文先获得焦点。\n\n```rust\nlet x = 1;\n```";
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, markdown.into(), None));
    redraw(cx);

    let bounds = cx
        .debug_bounds("code-copy-button")
        .expect("未聚焦代码块也应显示复制按钮");
    let header = cx.debug_bounds("code-block-header").expect("代码块顶栏");
    assert!(header.contains(&bounds.center()), "复制按钮应在顶部同一行");
    assert!(bounds.center().x > header.center().x, "复制按钮应在右侧");
    // 复制按钮是标准按钮：自绘 div 只有 hover 底色，辅助技术读不出这是「复制」。
    {
        use gpui_kit::test::TestWindowExt as _;
        cx.update(|window, _| {
            let button = window.try_find("code-copy-button").expect("复制按钮");
            assert_eq!(button.role(), Some(gpui::Role::Button), "复制必须是标准按钮");
            assert!(
                button.label().is_some_and(|label| !label.is_empty()),
                "复制按钮必须有可访问名称"
            );
        });
    }
    cx.simulate_click(bounds.center(), Modifiers::none());
    redraw(cx);

    let clipboard = cx.update(|_window, cx| {
        cx.read_from_clipboard()
            .and_then(|item| item.text())
            .map(|text| text.to_string())
    });
    assert_eq!(clipboard.as_deref(), Some("let x = 1;"));
    editor.read_with(cx, |editor, _cx| {
        // 点击复制不顺带移动光标/选中文字。
        let block = &editor.document.visible_blocks()[1].entity;
        assert!(block.read(_cx).selected_range.is_empty());
    });
}

#[gpui::test]
async fn code_block_copy_button_preserves_literal_backticks(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let markdown = "正文\n\n````text\n    keep indentation\nliteral ``` stays\n中文也保留\n````";
    let (_editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, markdown.into(), None)
    });
    redraw(cx);
    let button = cx.debug_bounds("code-copy-button").expect("复制按钮常驻");
    cx.simulate_click(button.center(), Modifiers::none());
    let clipboard = cx.update(|_window, cx| cx.read_from_clipboard().and_then(|item| item.text()));
    assert_eq!(clipboard.as_deref(), Some("    keep indentation\nliteral ``` stays\n中文也保留"));
}

#[gpui::test]
async fn undo_after_view_mode_switch_keeps_text(cx: &mut TestAppContext) {
    let editor = cx.new(|cx| Editor::from_markdown(cx, "alpha".to_string(), None));

    editor.update(cx, |editor, cx| {
        let block = editor.document.first_root().expect("root").clone();
        editor.active_entity_id = Some(block.entity_id());
        block.update(cx, |block, cx| {
            block.prepare_undo_capture(crate::components::UndoCaptureKind::CoalescibleText, cx);
            block.replace_text_in_visible_range(5..5, " beta", None, false, cx);
        });
    });

    // 源码/渲染模式来回切换后，撤销历史仍应完整可用（roadmap B11）。
    editor.update(cx, |editor, cx| {
        assert_eq!(editor.document.markdown_text(cx), "alpha beta");
        editor.toggle_view_mode(cx);
        assert!(matches!(editor.view_mode, ViewMode::Source));
        editor.undo_document(cx);
        assert_eq!(editor.document.markdown_text(cx), "alpha");
        editor.toggle_view_mode(cx);
        assert!(matches!(editor.view_mode, ViewMode::Rendered));
        assert_eq!(editor.document.markdown_text(cx), "alpha");
        editor.redo_document(cx);
        assert_eq!(editor.document.markdown_text(cx), "alpha beta");
    });
}

#[gpui::test]
async fn undo_first_edit_in_a_paren_numbered_item_restores_content(cx: &mut TestAppContext) {
    let editor = cx.new(|cx| Editor::from_markdown(cx, "1) first".to_string(), None));

    editor.update(cx, |editor, cx| {
        let block = editor.document.first_root().expect("list root").clone();
        editor.active_entity_id = Some(block.entity_id());
        block.update(cx, |block, cx| {
            block.prepare_undo_capture(crate::components::UndoCaptureKind::CoalescibleText, cx);
            block.replace_text_in_visible_range(5..5, "!", None, false, cx);
        });
    });

    editor.update(cx, |editor, cx| {
        assert_eq!(editor.document.markdown_text(cx), "1) first!");
        editor.undo_document(cx);
        assert_eq!(editor.document.markdown_text(cx), "1) first");
    });
}

#[gpui::test]
async fn consecutive_text_edits_within_window_coalesce_into_one_undo(cx: &mut TestAppContext) {
    let editor = cx.new(|cx| Editor::from_markdown(cx, "a".to_string(), None));

    editor.update(cx, |editor, cx| {
        let block = editor.document.first_root().expect("root").clone();
        editor.active_entity_id = Some(block.entity_id());

        block.update(cx, |block, cx| {
            block.prepare_undo_capture(crate::components::UndoCaptureKind::CoalescibleText, cx);
            block.replace_text_in_visible_range(1..1, "b", None, false, cx);
        });
        block.update(cx, |block, cx| {
            block.prepare_undo_capture(crate::components::UndoCaptureKind::CoalescibleText, cx);
            block.replace_text_in_visible_range(2..2, "c", None, false, cx);
        });
    });

    editor.update(cx, |editor, cx| {
        assert_eq!(editor.document.markdown_text(cx), "abc");
        assert_eq!(editor.undo_history.len(), 1);

        editor.undo_document(cx);
        assert_eq!(editor.document.markdown_text(cx), "a");
    });
}

#[gpui::test]
async fn redo_restores_text_reverted_by_undo(cx: &mut TestAppContext) {
    let editor = cx.new(|cx| Editor::from_markdown(cx, "alpha".to_string(), None));

    editor.update(cx, |editor, cx| {
        let block = editor.document.first_root().expect("root").clone();
        editor.active_entity_id = Some(block.entity_id());
        block.update(cx, |block, cx| {
            block.prepare_undo_capture(crate::components::UndoCaptureKind::CoalescibleText, cx);
            block.replace_text_in_visible_range(5..5, " beta", None, false, cx);
        });
    });

    editor.update(cx, |editor, cx| {
        editor.undo_document(cx);
        assert_eq!(editor.document.markdown_text(cx), "alpha");
        assert_eq!(editor.redo_history.len(), 1);

        editor.redo_document(cx);
        assert_eq!(editor.document.markdown_text(cx), "alpha beta");
        assert!(editor.redo_history.is_empty());
    });
}

#[gpui::test]
async fn fresh_edit_clears_pending_redo_history(cx: &mut TestAppContext) {
    let editor = cx.new(|cx| Editor::from_markdown(cx, "alpha".to_string(), None));

    editor.update(cx, |editor, cx| {
        let block = editor.document.first_root().expect("root").clone();
        editor.active_entity_id = Some(block.entity_id());
        block.update(cx, |block, cx| {
            block.prepare_undo_capture(crate::components::UndoCaptureKind::CoalescibleText, cx);
            block.replace_text_in_visible_range(5..5, " beta", None, false, cx);
        });
    });

    editor.update(cx, |editor, cx| {
        editor.undo_document(cx);
        assert_eq!(editor.redo_history.len(), 1);

        // A new edit invalidates the redo stack so it cannot revive stale text.
        let block = editor.document.first_root().expect("root").clone();
        block.update(cx, |block, cx| {
            block.prepare_undo_capture(crate::components::UndoCaptureKind::CoalescibleText, cx);
            block.replace_text_in_visible_range(5..5, " gamma", None, false, cx);
        });
    });

    editor.update(cx, |editor, cx| {
        editor.finalize_pending_undo_capture(cx);
        assert!(editor.redo_history.is_empty());

        editor.redo_document(cx);
        assert_eq!(editor.document.markdown_text(cx), "alpha gamma");
    });
}


#[gpui::test]
async fn typing_a_toc_marker_fills_its_entries_on_that_frame(cx: &mut TestAppContext) {
    // 折叠过滤读的是「同步可见列表那一趟记下的 `[TOC]` 形状」，不再逐块读实体。
    // 这条测试钉住它的前提：段落的文本一变，那一趟必跟着重跑，所以刚把
    // `待填` 改成 `[TOC]` 的这一帧就拿到条目；改回去的这一帧条目又被清掉。
    init_editor_test_app(cx);
    let markdown = "# Title\n\n待填\n\n## Section";
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, markdown.into(), None));
    redraw(cx);

    let paragraph = editor.update(cx, |editor, _cx| {
        let visible = editor.document.visible_blocks().to_vec();
        assert_eq!(visible.len(), 3, "夹具该是 Title / 待填 / Section 三块");
        visible[1].entity.clone()
    });

    let retype = |text: &'static str, cx: &mut gpui::VisualTestContext| {
        editor.update(cx, |editor, cx| {
            editor.focus_block(paragraph.entity_id());
            let len = paragraph.read_with(cx, |block, _cx| block.visible_len());
            paragraph.update(cx, |block, block_cx| {
                block.replace_text_in_visible_range(0..len, text, None, false, block_cx);
            });
        });
        redraw(cx);
    };

    retype("[TOC]", cx);
    paragraph.read_with(cx, |block, _cx| {
        let titles = block
            .toc_entries
            .iter()
            .map(|entry| entry.title.clone())
            .collect::<Vec<_>>();
        assert_eq!(
            titles,
            vec!["Title".to_string(), "Section".to_string()],
            "改成 `[TOC]` 的这一帧就该有目录条目"
        );
    });

    retype("待填", cx);
    assert!(
        paragraph.read_with(cx, |block, _cx| block.toc_entries.is_empty()),
        "文本不再是 `[TOC]`，条目要跟着清掉，否则块上挂着过期目录"
    );
}

#[gpui::test]
async fn markdown_source_mode_highlights_with_monospace_pipeline(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    // markdown 源码模式必须提供等宽字体与基础语法高亮（对标
    // VS Code 的 markdown 源码视图）。切到源码后：分块走高亮管线（markdown
    // 语法 span）、行号标志打开（等宽字体与行号对齐都靠它）。
    let markdown = "# 标题\n\n正文 `code` **粗体**。\n";
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, markdown.into(), None));
    editor.update(cx, |editor, cx| {
        editor.toggle_view_mode(cx);
        assert!(matches!(editor.view_mode, ViewMode::Source));
    });

    editor.read_with(cx, |editor, cx| {
        let roots = editor.document.root_blocks();
        // 源码分块按 512 行切，4 行的文档就一块（整篇在里面）。
        assert_eq!(roots.len(), 1);
        for block in roots {
            let block = block.read(cx);
            assert!(
                block.show_source_line_numbers(),
                "源码分块都应带行号标志（等宽字体与行号绘制的开关）"
            );
        }
        let highlight = roots[0]
            .read(cx)
            .code_highlight_result()
            .expect("markdown 源码分块应有高亮结果");
        assert!(
            !highlight.spans.is_empty(),
            "标题/#、code span、粗体记号应产出高亮 span"
        );
    });
}


/// 代码块里选中多行按 Tab：逐行加缩进，不能拿四个空格把选区整段替换掉。
#[gpui::test]
async fn tab_in_a_code_block_indents_every_selected_line(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| {
        Editor::from_markdown(cx, "```\na\nb\nc\n```\n".to_string(), None)
    });
    redraw(cx);
    let code = editor.read_with(cx, |editor, _cx| {
        editor.document.root_blocks()[0].clone()
    });
    code.update_in(cx, |block, window, cx| {
        block.selected_range = 0..5;
        block.selection_reversed = false;
        block.on_indent_block(&crate::components::IndentBlock, window, cx);
        assert_eq!(block.display_text(), "    a\n    b\n    c");
        assert_eq!(block.selected_range, 4..17, "选区没有跟着缩进右移");
    });
}

/// 代码块里 Shift+Tab：逐行去掉至多四个空格的缩进。
#[gpui::test]
async fn shift_tab_in_a_code_block_outdents_every_selected_line(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| {
        Editor::from_markdown(cx, "```\n    a\n    b\n```\n".to_string(), None)
    });
    redraw(cx);
    let code = editor.read_with(cx, |editor, _cx| {
        editor.document.root_blocks()[0].clone()
    });
    code.update_in(cx, |block, window, cx| {
        let len = block.display_text().len();
        block.selected_range = 0..len;
        block.selection_reversed = false;
        block.on_outdent_block(&crate::components::OutdentBlock, window, cx);
        assert_eq!(block.display_text(), "a\nb");
        assert_eq!(block.selected_range, 0..3, "选区没有跟着缩进左移");
    });
}

/// 三击选中整块（段落）；双击仍然是选词。
#[gpui::test]
async fn triple_click_selects_the_whole_block(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| {
        Editor::from_markdown(cx, "alpha beta gamma".into(), None)
    });
    redraw(cx);
    let block = editor.read_with(cx, |editor, _cx| {
        editor.document.first_root().expect("正文块").clone()
    });
    editor.update(cx, |editor, _cx| editor.focus_block(block.entity_id()));
    redraw(cx);
    let position = block.read_with(cx, |block, _cx| {
        let bounds = block.last_bounds.expect("正文块应有几何");
        gpui::point(bounds.left() + gpui::px(2.0), bounds.center().y)
    });
    let down = |count: usize| gpui::MouseDownEvent {
        position,
        button: gpui::MouseButton::Left,
        modifiers: gpui::Modifiers::default(),
        click_count: count,
        first_mouse: count == 1,
    };
    editor.update_in(cx, |_editor, window, cx| {
        block.update(cx, |block, block_cx| {
            block.on_mouse_down(&down(3), window, block_cx);
        });
    });
    redraw(cx);
    block.read_with(cx, |block, _cx| {
        assert_eq!(
            block.selected_range,
            0..block.display_text().len(),
            "三击没有选中整块"
        );
    });
}

/// 段落变空并失焦后，上一帧的几何必须作废：否则点击下方区域会被陈旧
/// 边界认领进这个看不见的空块。
#[gpui::test]
async fn an_empty_unfocused_paragraph_drops_its_stale_geometry(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| {
        Editor::from_markdown(cx, "alpha\n\nbeta".into(), None)
    });
    redraw(cx);
    let (first, second) = editor.read_with(cx, |editor, _cx| {
        let blocks = editor.document.root_blocks();
        (blocks[0].clone(), blocks[1].clone())
    });
    assert!(
        first.read_with(cx, |block, _cx| block.last_bounds.is_some()),
        "夹具：第一块应先有几何"
    );
    editor.update(cx, |editor, cx| {
        first.update(cx, |block, _cx| {
            block.record.set_title(crate::components::InlineTextTree::plain(String::new()));
            block.sync_render_cache();
        });
        editor.focus_block(second.entity_id());
        editor.mark_dirty(cx);
    });
    redraw(cx);
    redraw(cx);
    first.read_with(cx, |block, _cx| {
        assert!(
            block.last_bounds.is_none() && block.last_layout.is_none(),
            "空段落还留着上一帧的几何：{:?}",
            block.last_bounds
        );
    });
}

/// `[TOC]` 块聚焦后要回到普通段落文本：以前聚焦仍只画目录行，块里没有文本
/// 元素——看不到光标、打字没有可见回显，也没法把 `[TOC]` 改成别的文字。
#[gpui::test]
async fn focused_toc_block_falls_back_to_editable_text(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let markdown = "# Title\n\n[TOC]\n\nbody";
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, markdown.into(), None));
    redraw(cx);
    let toc_block = editor.read_with(cx, |editor, _| editor.document.root_blocks()[1].clone());
    editor.update(cx, |editor, _cx| editor.focus_block(toc_block.entity_id()));
    redraw(cx);
    redraw(cx);
    editor.read_with(cx, |_editor, cx| {
        let block = toc_block.read(cx);
        assert!(
            block.active_range_or_cursor_bounds().is_some(),
            "聚焦的目录块要有光标几何（文本元素在）"
        );
    });

    cx.simulate_input("x");
    redraw(cx);
    editor.read_with(cx, |editor, cx| {
        assert!(
            editor.document.markdown_text(cx).contains("x[TOC]"),
            "聚焦的目录块应能直接编辑：{:?}",
            editor.document.markdown_text(cx)
        );
    });
}
