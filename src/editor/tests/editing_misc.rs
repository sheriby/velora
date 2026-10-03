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

#[test]
fn copy_as_html_item_carries_html_flavor() {
    // roadmap F2 增强：纯文本 flavor 保留 HTML 源码，同时附带 text/html flavor。
    let html = "<h1>Title</h1>".to_string();
    let item = crate::editor::workspace::copy_as_html_clipboard_item(html.clone());
    assert_eq!(item.html(), Some(html.as_str()));
}

#[gpui::test]
async fn copy_as_html_writes_html_source_to_clipboard(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "# Title\n\nbody".into(), None)
    });
    editor.update(cx, |editor, cx| editor.copy_as_html(cx));
    let text = cx.update(|_window, cx| {
        cx.read_from_clipboard().and_then(|item| item.text())
    });
    let text = text.expect("clipboard should hold HTML source");
    assert!(text.contains("<h1"), "expected rendered HTML, got: {text}");
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

