use super::common::*;

#[gpui::test]
async fn ctrl_s_saves_rendered_mode_edit_to_existing_file(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let path = temp_markdown_path("ctrl-s-rendered-save");
    fs::write(&path, "alpha").expect("write initial markdown");
    let cleanup_path = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup_path);
    });

    let (editor, cx) = cx.add_window_view({
        let path = path.clone();
        move |_window, cx| Editor::from_markdown(cx, "alpha".to_string(), Some(path))
    });

    cx.simulate_input("!");
    redraw(cx);
    let expected = editor.read_with(cx, |editor, cx| {
        assert!(editor.document_dirty);
        assert!(!editor.pending_save);
        editor.document.markdown_text(cx)
    });
    assert_ne!(expected, "alpha");

    cx.simulate_keystrokes("ctrl-s");
    redraw(cx);

    assert_eq!(
        fs::read_to_string(&path).expect("read saved markdown"),
        expected
    );
    editor.read_with(cx, |editor, _cx| {
        assert!(!editor.document_dirty);
        assert!(!editor.pending_save);
    });
}

#[gpui::test]
async fn dirty_saved_document_is_autosaved(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let path = temp_markdown_path("autosave");
    fs::write(&path, "alpha").expect("write initial markdown");
    let cleanup_path = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup_path);
    });

    let (editor, cx) = cx.add_window_view({
        let path = path.clone();
        move |_window, cx| Editor::from_markdown(cx, "alpha".to_string(), Some(path))
    });
    let recovery_id = editor.read_with(cx, |editor, _cx| editor.recovery_id);
    cx.on_quit(move || {
        let _ = crate::config::remove_recovery_snapshot(recovery_id);
    });
    editor.update(cx, |editor, cx| {
        let first = editor.document.first_root().expect("first block").clone();
        first.update(cx, |block, _cx| {
            block
                .record
                .set_title(InlineTextTree::plain("autosaved text".to_string()));
            block.sync_render_cache();
        });
        editor.mark_dirty(cx);
    });

    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();

    assert_eq!(
        fs::read_to_string(&path).expect("read autosaved markdown"),
        "autosaved text"
    );
    editor.read_with(cx, |editor, _cx| assert!(!editor.document_dirty));
}

#[gpui::test]
async fn chinese_ime_composition_commit_save_and_undo_keep_text(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let path = temp_markdown_path("chinese-ime-commit");
    fs::write(&path, "note").expect("write initial markdown");
    let cleanup_path = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(cleanup_path);
    });

    let (editor, cx) = cx.add_window_view({
        let path = path.clone();
        move |_window, cx| Editor::from_markdown(cx, "note".to_string(), Some(path))
    });
    let recovery_id = editor.read_with(cx, |editor, _cx| editor.recovery_id);
    cx.on_quit(move || {
        let _ = crate::config::remove_recovery_snapshot(recovery_id);
    });
    let block = editor.read_with(cx, |editor, _cx| {
        editor.document.first_root().expect("paragraph").clone()
    });
    block.update(cx, |block, _cx| block.selected_range = 4..4);

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
    assert_eq!(
        block.read_with(cx, |block, _cx| block.marked_range.clone()),
        Some(4..9)
    );
    redraw(cx);
    editor.update(cx, |editor, _cx| {
        let entry = editor
            .undo_history
            .last_mut()
            .expect("composition should capture its original source");
        assert_eq!(entry.kind, UndoCaptureKind::ImeComposition);
        entry.timestamp = Instant::now() - Duration::from_secs(2);
    });
    let block_bounds = block.read_with(cx, |block, _cx| {
        block
            .last_bounds
            .expect("composing text should be laid out")
    });
    cx.update(|window, cx| {
        block.update(cx, |block, block_cx| {
            assert_eq!(
                <Block as EntityInputHandler>::marked_text_range(block, window, block_cx),
                Some(4..9)
            );
            assert!(
                <Block as EntityInputHandler>::bounds_for_range(
                    block,
                    4..9,
                    block_bounds,
                    window,
                    block_cx,
                )
                .is_some()
            );
        });
    });
    editor.update(cx, |editor, cx| editor.request_save_document(cx));
    redraw(cx);
    assert_eq!(
        fs::read_to_string(&path).expect("composition should not be saved"),
        "note"
    );
    editor.read_with(cx, |editor, _cx| assert!(editor.pending_save));

    cx.update(|window, cx| {
        block.update(cx, |block, block_cx| {
            <Block as EntityInputHandler>::replace_text_in_range(
                block, None, "你好", window, block_cx,
            );
        });
    });
    assert_eq!(
        block.read_with(cx, |block, _cx| block.display_text().to_string()),
        "note你好"
    );
    assert_eq!(
        block.read_with(cx, |block, _cx| block.marked_range.clone()),
        None
    );
    redraw(cx);
    editor.read_with(cx, |editor, _cx| assert!(!editor.pending_save));
    assert_eq!(
        fs::read_to_string(&path).expect("read saved Markdown"),
        "note你好"
    );
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(editor.undo_history.len(), 1);
    });

    editor.update(cx, |editor, cx| editor.undo_document(cx));
    assert_eq!(
        editor.read_with(cx, |editor, cx| editor.document.markdown_text(cx)),
        "note"
    );
}

#[gpui::test]
async fn autosave_waits_until_ime_composition_is_committed(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let path = temp_markdown_path("ime-autosave");
    fs::write(&path, "note").expect("write initial markdown");
    let cleanup_path = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(cleanup_path);
    });

    let (editor, cx) = cx.add_window_view({
        let path = path.clone();
        move |_window, cx| Editor::from_markdown(cx, "note".to_string(), Some(path))
    });
    let block = editor.read_with(cx, |editor, _cx| {
        editor.document.first_root().expect("paragraph").clone()
    });
    block.update(cx, |block, _cx| block.selected_range = 4..4);
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

    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    assert_eq!(
        fs::read_to_string(&path).expect("read Markdown during composition"),
        "note"
    );

    cx.update(|window, cx| {
        block.update(cx, |block, block_cx| {
            <Block as EntityInputHandler>::replace_text_in_range(
                block, None, "你好", window, block_cx,
            );
        });
    });
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    assert_eq!(
        fs::read_to_string(&path).expect("read committed Markdown"),
        "note你好"
    );
}

#[gpui::test]
async fn external_autosave_conflict_survives_workspace_rescan(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let path = temp_markdown_path("autosave-conflict-rescan");
    fs::write(&path, "alpha").expect("write initial markdown");
    let cleanup_path = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(cleanup_path);
    });

    let (editor, cx) = cx.add_window_view({
        let path = path.clone();
        move |_window, cx| Editor::from_markdown(cx, "alpha".to_string(), Some(path))
    });
    let recovery_id = editor.read_with(cx, |editor, _cx| editor.recovery_id);
    cx.on_quit(move || {
        let _ = crate::config::remove_recovery_snapshot(recovery_id);
    });
    editor.update(cx, |editor, cx| {
        let first = editor.document.first_root().expect("first block").clone();
        first.update(cx, |block, _cx| {
            block
                .record
                .set_title(InlineTextTree::plain("our edits".to_string()));
            block.sync_render_cache();
        });
        editor.mark_dirty(cx);
    });
    fs::write(&path, "external edits").expect("write external changes");

    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    editor.read_with(cx, |editor, _cx| {
        assert!(editor.has_external_autosave_conflict());
    });

    // 工作区重新扫描会清空普通错误提示，但外部修改冲突必须保留，
    // 否则自动保存会重新放开并覆盖磁盘上的新内容。
    editor.update(cx, |editor, cx| {
        let root = path.parent().expect("temp dir").to_path_buf();
        editor.set_workspace_root(root, cx);
    });
    cx.run_until_parked();
    editor.read_with(cx, |editor, _cx| {
        assert!(
            editor.has_external_autosave_conflict(),
            "conflict flag must survive a workspace rescan"
        );
    });

    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    assert_eq!(
        fs::read_to_string(&path).expect("read external file"),
        "external edits",
        "autosave must not overwrite the externally modified file"
    );
}

#[gpui::test]
async fn autosave_does_not_overwrite_external_file_changes(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let path = temp_markdown_path("autosave-external-change");
    fs::write(&path, "alpha").expect("write initial markdown");
    let cleanup_path = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(cleanup_path);
    });

    let (editor, cx) = cx.add_window_view({
        let path = path.clone();
        move |_window, cx| Editor::from_markdown(cx, "alpha".to_string(), Some(path))
    });
    let recovery_id = editor.read_with(cx, |editor, _cx| editor.recovery_id);
    cx.on_quit(move || {
        let _ = crate::config::remove_recovery_snapshot(recovery_id);
    });
    editor.update(cx, |editor, cx| {
        let first = editor.document.first_root().expect("first block").clone();
        first.update(cx, |block, _cx| {
            block
                .record
                .set_title(InlineTextTree::plain("our edits".to_string()));
            block.sync_render_cache();
        });
        editor.mark_dirty(cx);
    });
    fs::write(&path, "external edits").expect("write external changes");

    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();

    assert_eq!(
        fs::read_to_string(&path).expect("read external file"),
        "external edits"
    );
    editor.read_with(cx, |editor, _cx| {
        assert!(editor.document_dirty);
        assert!(editor.has_external_autosave_conflict());
    });
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            assert!(!editor.save_to_existing_path(&path, window, cx));
        });
    });
    assert_eq!(
        fs::read_to_string(&path).expect("read external file after manual save"),
        "external edits"
    );
}

#[gpui::test]
async fn autosave_flushes_a_dirty_tab_after_switching_away(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let first_path = temp_markdown_path("autosave-tab-first");
    let second_path = temp_markdown_path("autosave-tab-second");
    fs::write(&first_path, "alpha").expect("write first document");
    fs::write(&second_path, "beta").expect("write second document");
    let cleanup_first = first_path.clone();
    let cleanup_second = second_path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(cleanup_first);
        let _ = fs::remove_file(cleanup_second);
    });

    let (editor, cx) = cx.add_window_view({
        let first_path = first_path.clone();
        move |_window, cx| Editor::from_markdown(cx, "alpha".to_string(), Some(first_path))
    });
    let first_recovery_id = editor.read_with(cx, |editor, _cx| editor.recovery_id);
    editor.update(cx, |editor, cx| {
        let first = editor.document.first_root().expect("first block").clone();
        first.update(cx, |block, _cx| {
            block
                .record
                .set_title(InlineTextTree::plain("edited alpha".to_string()));
            block.sync_render_cache();
        });
        editor.mark_dirty(cx);
    });

    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(second_path.clone(), window, cx);
        });
    });
    editor.update(cx, |editor, cx| {
        let second = editor.document.first_root().expect("second block").clone();
        second.update(cx, |block, _cx| {
            block
                .record
                .set_title(InlineTextTree::plain("edited beta".to_string()));
            block.sync_render_cache();
        });
        editor.mark_dirty(cx);
    });
    let second_recovery_id = editor.read_with(cx, |editor, _cx| editor.recovery_id);
    cx.on_quit(move || {
        let _ = crate::config::remove_recovery_snapshot(first_recovery_id);
        let _ = crate::config::remove_recovery_snapshot(second_recovery_id);
    });

    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();

    assert_eq!(
        fs::read_to_string(&first_path).expect("read first autosaved document"),
        "edited alpha"
    );
    assert_eq!(
        fs::read_to_string(&second_path).expect("read second autosaved document"),
        "edited beta"
    );
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(editor.file_path.as_ref(), Some(&second_path));
        assert!(!editor.document_dirty);
        assert!(!editor.has_dirty_workspace_documents());
    });
}

#[gpui::test]
async fn save_and_close_writes_every_dirty_workspace_tab(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let first_path = temp_markdown_path("save-close-tab-first");
    let second_path = temp_markdown_path("save-close-tab-second");
    fs::write(&first_path, "alpha").expect("write first document");
    fs::write(&second_path, "beta").expect("write second document");
    let cleanup_first = first_path.clone();
    let cleanup_second = second_path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(cleanup_first);
        let _ = fs::remove_file(cleanup_second);
    });

    let (editor, cx) = cx.add_window_view({
        let first_path = first_path.clone();
        move |_window, cx| Editor::from_markdown(cx, "alpha".to_string(), Some(first_path))
    });
    let first_recovery_id = editor.read_with(cx, |editor, _cx| editor.recovery_id);
    editor.update(cx, |editor, cx| {
        let first = editor.document.first_root().expect("first block").clone();
        first.update(cx, |block, _cx| {
            block
                .record
                .set_title(InlineTextTree::plain("edited alpha".to_string()));
            block.sync_render_cache();
        });
        editor.mark_dirty(cx);
    });
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(second_path.clone(), window, cx);
        });
    });
    let second_recovery_id = editor.read_with(cx, |editor, _cx| editor.recovery_id);
    cx.on_quit(move || {
        let _ = crate::config::remove_recovery_snapshot(first_recovery_id);
        let _ = crate::config::remove_recovery_snapshot(second_recovery_id);
    });
    editor.update(cx, |editor, cx| {
        let second = editor.document.first_root().expect("second block").clone();
        second.update(cx, |block, _cx| {
            block
                .record
                .set_title(InlineTextTree::plain("edited beta".to_string()));
            block.sync_render_cache();
        });
        editor.mark_dirty(cx);
    });

    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.save_dirty_workspace_documents_and_close(window, cx);
        });
    });
    cx.run_until_parked();

    assert_eq!(
        fs::read_to_string(&first_path).expect("read first"),
        "edited alpha"
    );
    assert_eq!(
        fs::read_to_string(&second_path).expect("read second"),
        "edited beta"
    );
    assert_eq!(cx.cx.windows().len(), 0);
}

#[gpui::test]
async fn recovered_document_is_opened_as_a_dirty_copy(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let recovery_id = uuid::Uuid::new_v4();
    let source_path = temp_markdown_path("recovered-source");
    let (editor, cx) = cx.add_window_view({
        let source_path = source_path.clone();
        move |_window, cx| {
            Editor::from_recovery(
                cx,
                crate::config::RecoverySnapshot {
                    id: recovery_id,
                    source_path: Some(source_path),
                    markdown: "recovered text".to_string(),
                },
            )
        }
    });

    editor.read_with(cx, |editor, cx| {
        assert!(editor.document_dirty);
        assert!(editor.is_recovered_document);
        assert_eq!(editor.recovery_id, recovery_id);
        assert_eq!(editor.file_path, None);
        assert_eq!(editor.document.markdown_text(cx), "recovered text");
    });
}

/// 标签缓存存的是文档文本，不是「从块树重拼一遍」的结果。
///
/// 会话标签（`WorkspaceDocumentTab.markdown`）是自动保存/恢复快照的**内容来源**：切标签、
/// 退出前存脏标签、写快照都取它。它取块树序列化的话，写进快照的正文就已经被洗过一遍
/// （下划线强调变成星号、表格列宽被重新对齐），恢复出来的是另一份文档。缓冲区才是事实源，
/// 所以标签必须存缓冲区文本。
#[gpui::test]
async fn the_workspace_tab_cache_holds_the_buffer_text_not_a_reserialization(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);

    const FIXTURE: &str = "# 标题\n\n_下划线_ 强调\n\n| 名称   | 数量 |\n|:-------|-----:|\n| 苹果   |    3 |\n";
    let path = temp_markdown_path("tab-cache-holds-buffer-text");
    fs::write(&path, FIXTURE).expect("seed fixture");
    let cleanup = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup);
    });

    let (editor, cx) = cx.add_window_view({
        let path = path.clone();
        move |_window, cx| Editor::from_markdown(cx, FIXTURE.to_string(), Some(path))
    });
    redraw(cx);

    editor.read_with(cx, |editor, _cx| {
        assert!(!editor.document_dirty, "干净文档才该等于磁盘原文");
        assert!(editor.buffer.text().contains("_下划线_"));
    });

    editor.update(cx, |editor, cx| editor.snapshot_current_document(cx));

    let (_, _, tab_markdown) = editor
        .read_with(cx, |editor, _cx| {
            editor.workspace_tab_state_for_test(&path).expect("tab")
        });
    let buffer_text = editor.read_with(cx, |editor, _cx| editor.buffer.text());
    assert_eq!(
        tab_markdown, buffer_text,
        "标签缓存被重新序列化过：{:?}\n缓冲区：{:?}",
        tab_markdown, buffer_text
    );
}

/// 没有文件路径的文档，自动保存写的恢复快照也必须是一字不差的缓冲区文本。
///
/// 快照是「用户还没保存过的正文」唯一的落盘形态：它若取块树序列化，恢复出来的文档
/// 已经不是用户打开的那份（下划线变星号、表格列宽被重新对齐），而且这份损失连撤销
/// 都找不回来。这里只编辑标题一行，其余块的原文必须原样进快照。
#[gpui::test]
async fn an_untitled_autosave_snapshot_holds_the_buffer_text(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    const FIXTURE: &str = "# 标题\n\n_下划线_ 强调\n\n| 名称   | 数量 |\n|:-------|-----:|\n| 苹果   |    3 |\n";
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, FIXTURE.to_string(), None));
    let recovery_id = editor.read_with(cx, |editor, _cx| editor.recovery_id);
    cx.on_quit(move || {
        let _ = crate::config::remove_recovery_snapshot(recovery_id);
    });
    redraw(cx);

    cx.simulate_input("起草 ");
    redraw(cx);
    editor.read_with(cx, |editor, _cx| assert!(editor.document_dirty));

    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();

    let snapshot = crate::config::read_recovery_snapshots()
        .expect("read recovery snapshots")
        .into_iter()
        .find(|snapshot| snapshot.id == recovery_id)
        .expect("autosave must write a recovery snapshot for the untitled document");
    let buffer_text = editor.read_with(cx, |editor, _cx| editor.buffer.text());
    assert_eq!(snapshot.markdown, buffer_text);
    assert!(
        snapshot.markdown.contains("_下划线_") && snapshot.markdown.contains("| 苹果   |    3 |"),
        "快照里的正文被洗过：{:?}",
        snapshot.markdown
    );
}

/// 磁盘上只是行尾不同（CRLF ↔ LF），不算外部修改，干净文档不许被重新导入。
///
/// 标签缓存与缓冲区同为 LF 文本，而读盘拿到的字符串保留原行尾：逐字比较会让每次监听
/// 事件都判定「文件被改了」，把用户刚保存的文件再导入一遍——块实体全换、折叠与滚动
/// 状态随之丢失。
#[gpui::test]
async fn a_clean_document_is_not_reimported_when_only_the_line_ending_differs(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);

    const FIXTURE: &str = "# 标题\n\n_下划线_ 强调\n";
    let path = temp_markdown_path("watcher-crlf-no-reload");
    fs::write(&path, FIXTURE.replace('\n', "\r\n")).expect("seed CRLF fixture");
    let cleanup = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup);
    });

    let (editor, cx) = cx.add_window_view({
        let path = path.clone();
        move |_window, cx| Editor::from_markdown(cx, FIXTURE.to_string(), Some(path))
    });
    redraw(cx);

    let first_block = editor.read_with(cx, |editor, _cx| {
        editor.document.first_root().cloned().expect("first root")
    });
    editor.update(cx, |editor, cx| {
        editor.reload_externally_changed_document(&path, cx);
    });
    editor.read_with(cx, |editor, _cx| {
        assert!(
            !editor.document_dirty,
            "行尾差异被当成了未保存的改动"
        );
        assert_eq!(
            editor.document.first_root().cloned().expect("first root"),
            first_block,
            "内容没变却重新导入了整篇文档"
        );
    });
}

#[gpui::test]
async fn workspace_tabs_restore_unsaved_markdown_state(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let first_path = temp_markdown_path("workspace-tab-first");
    let second_path = temp_markdown_path("workspace-tab-second");
    fs::write(&first_path, "alpha").expect("write first document");
    fs::write(&second_path, "beta").expect("write second document");
    let cleanup_first = first_path.clone();
    let cleanup_second = second_path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup_first);
        let _ = fs::remove_file(&cleanup_second);
    });

    let (editor, cx) = cx.add_window_view({
        let first_path = first_path.clone();
        move |_window, cx| Editor::from_markdown(cx, "alpha".to_string(), Some(first_path))
    });
    editor.update(cx, |editor, cx| {
        let first = editor.document.first_root().expect("first block").clone();
        first.update(cx, |block, _cx| {
            block
                .record
                .set_title(InlineTextTree::plain("edited alpha".to_string()));
            block.sync_render_cache();
        });
        editor.mark_dirty(cx);
    });

    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(second_path.clone(), window, cx);
        });
    });
    editor.read_with(cx, |editor, cx| {
        assert_eq!(editor.file_path.as_ref(), Some(&second_path));
        assert_eq!(editor.document.markdown_text(cx), "beta");
        assert!(!editor.document_dirty);
    });

    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(first_path.clone(), window, cx);
        });
    });
    editor.read_with(cx, |editor, cx| {
        assert_eq!(editor.file_path.as_ref(), Some(&first_path));
        assert_eq!(editor.document.markdown_text(cx), "edited alpha");
        assert!(editor.document_dirty);
    });
}

#[gpui::test]
async fn window_save_action_saves_current_editor_without_global_menu_route(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);

    let path = temp_markdown_path("window-action-save");
    fs::write(&path, "alpha").expect("write initial markdown");
    let cleanup_path = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup_path);
    });

    let (editor, cx) = cx.add_window_view({
        let path = path.clone();
        move |_window, cx| Editor::from_markdown(cx, "alpha".to_string(), Some(path))
    });

    cx.simulate_input(" action");
    redraw(cx);
    let expected = editor.read_with(cx, |editor, cx| {
        assert!(editor.document_dirty);
        editor.document.markdown_text(cx)
    });
    assert_ne!(expected, "alpha");

    cx.dispatch_action(SaveDocument);
    redraw(cx);

    assert_eq!(
        fs::read_to_string(&path).expect("read saved markdown"),
        expected
    );
    editor.read_with(cx, |editor, _cx| {
        assert!(!editor.document_dirty);
        assert!(!editor.pending_save);
    });
}

#[gpui::test]
async fn window_title_tracks_file_and_edited_state(cx: &mut TestAppContext) {
    // roadmap A7 的可自动验证面：标题（含「已编辑」前缀标记）由渲染帧同步到窗口，
    // 编辑后立刻带上标记、保存后去掉；锁屏期间的系统显示延迟属平台行为，留人工复核。
    init_editor_test_app(cx);

    let path = temp_markdown_path("window-title-sync");
    fs::write(&path, "alpha").expect("write initial markdown");
    let cleanup_path = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup_path);
    });

    let file_name = path
        .file_name()
        .expect("temp path has a file name")
        .to_string_lossy()
        .to_string();
    let clean_title = format!("Velora - {file_name}");
    let dirty_marker = cx.update(|cx| cx.global::<I18nManager>().strings().dirty_title_marker.clone());
    assert!(!dirty_marker.is_empty(), "脏标记文案不应为空");

    let (editor, cx) = cx.add_window_view({
        let path = path.clone();
        let markdown = "alpha".to_string();
        move |_window, cx| Editor::from_markdown(cx, markdown, Some(path))
    });
    redraw(cx);
    assert_eq!(cx.update(|window, _cx| window.window_title()), clean_title);

    cx.simulate_input(" x");
    redraw(cx);
    editor.read_with(cx, |editor, _cx| assert!(editor.document_dirty));
    assert_eq!(
        cx.update(|window, _cx| window.window_title()),
        format!("{dirty_marker} {clean_title}"),
        "编辑后窗口标题应带已编辑标记"
    );

    cx.dispatch_action(SaveDocument);
    redraw(cx);
    editor.read_with(cx, |editor, _cx| assert!(!editor.document_dirty));
    assert_eq!(
        cx.update(|window, _cx| window.window_title()),
        clean_title,
        "保存后窗口标题应去掉已编辑标记"
    );
}

/// 自动保存开关的落点：文档放在用例独占的目录里，这样「有没有留下 .velora-*.tmp」
/// 才只反映本篇的行为（`temp_markdown_path` 共用系统临时目录，并列用例会互相看到残留）。
fn isolated_doc_dir(cx: &mut TestAppContext, test_name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "velora-{test_name}-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&root).expect("create document dir");
    // macOS 上 /var ↔ /private/var 是两个名字：不 canonicalize，编辑器算出的路径与
    // 断言里的对不上，临时残留也就扫不准。
    let root = fs::canonicalize(&root).unwrap_or(root);
    let cleanup = root.clone();
    cx.on_quit(move || {
        let _ = fs::remove_dir_all(cleanup);
    });
    root
}

/// 目录里留下的 autosave 临时文件名（`.velora-*.tmp`）。
fn leftover_autosave_temps(dir: &std::path::Path) -> Vec<String> {
    fs::read_dir(dir)
        .expect("list document dir")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains(".velora-"))
        .collect()
}

fn set_autosave_switch(cx: &mut TestAppContext, autosave: bool) {
    cx.update(|cx| {
        crate::config::EditorSettings::init(cx, true);
        crate::config::EditorSettings::set_autosave_in_memory(autosave, cx);
    });
}

fn edit_first_block(editor: &gpui::Entity<Editor>, cx: &mut VisualTestContext, text: &str) {
    editor.update(cx, |editor, cx| {
        let first = editor.document.first_root().expect("first block").clone();
        first.update(cx, |block, _cx| {
            block
                .record
                .set_title(InlineTextTree::plain(text.to_string()));
            block.sync_render_cache();
        });
        editor.mark_dirty(cx);
    });
}

#[gpui::test]
async fn autosave_off_leaves_the_file_alone_but_still_notices_external_edits(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    set_autosave_switch(cx, false);

    let root = isolated_doc_dir(cx, "autosave-off");
    let path = root.join("note.md");
    fs::write(&path, "alpha").expect("write initial markdown");

    let (editor, cx) = cx.add_window_view({
        let path = path.clone();
        move |_window, cx| Editor::from_markdown(cx, "alpha".to_string(), Some(path))
    });
    let recovery_id = editor.read_with(cx, |editor, _cx| editor.recovery_id);
    cx.on_quit(move || {
        let _ = crate::config::remove_recovery_snapshot(recovery_id);
    });

    edit_first_block(&editor, cx, "our edits");
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();

    assert_eq!(
        fs::read_to_string(&path).expect("read file"),
        "alpha",
        "关掉自动保存后，编辑不该写进真文件"
    );
    let leftovers = leftover_autosave_temps(&root);
    assert!(
        leftovers.is_empty(),
        "关掉自动保存不该在同一目录留下临时文件，实测 {leftovers:?}"
    );
    editor.read_with(cx, |editor, _cx| {
        assert!(
            editor.document_dirty,
            "没落盘的编辑要一直算未保存（标签上的圆点得亮着）"
        );
        assert!(
            !editor.has_external_autosave_conflict(),
            "文件没被动过就不该报冲突"
        );
    });

    // 关掉的只是「写真文件」这一半：恢复快照照旧落盘，崩了不能丢字。
    // 按 `<id>.json` 那条路径点名查，不用 `read_recovery_snapshots()` 扫目录——
    // 并列用例会在同一时刻删自己那份快照，整目录扫一遍会 Err 掉（实测过一次假红）。
    let recovery_dir = crate::config::VeloraConfigDirs::from_system()
        .expect("config dirs")
        .recovery_dir();
    assert!(
        recovery_dir.join(format!("{recovery_id}.json")).is_file(),
        "关掉自动保存后这一篇仍要有恢复快照"
    );

    // 检测不跟着写入一起关掉：外部改动照样在下一次防抖 tick 上被认出来。
    fs::write(&path, "external edits").expect("external write");
    edit_first_block(&editor, cx, "our edits again");
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    editor.read_with(cx, |editor, _cx| {
        assert!(
            editor.has_external_autosave_conflict(),
            "关掉自动保存也要发现外部改动，否则用户的编辑会被后来的写入静默覆盖"
        );
    });
    assert_eq!(
        fs::read_to_string(&path).expect("read file"),
        "external edits",
        "报冲突也不能把外部改动写掉"
    );
}

#[gpui::test]
async fn autosave_on_writes_the_edited_file_and_clears_the_dirty_flag(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    set_autosave_switch(cx, true);

    let root = isolated_doc_dir(cx, "autosave-on");
    let path = root.join("note.md");
    fs::write(&path, "alpha").expect("write initial markdown");

    let (editor, cx) = cx.add_window_view({
        let path = path.clone();
        move |_window, cx| Editor::from_markdown(cx, "alpha".to_string(), Some(path))
    });
    let recovery_id = editor.read_with(cx, |editor, _cx| editor.recovery_id);
    cx.on_quit(move || {
        let _ = crate::config::remove_recovery_snapshot(recovery_id);
    });

    edit_first_block(&editor, cx, "our edits");
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();

    assert_eq!(
        fs::read_to_string(&path).expect("read file"),
        "our edits",
        "开关开着时防抖到点就要落盘（与关掉那一对，差的只有这个开关）"
    );
    editor.read_with(cx, |editor, _cx| {
        assert!(!editor.document_dirty, "落盘之后不该再算未保存");
    });
    let leftovers = leftover_autosave_temps(&root);
    assert!(
        leftovers.is_empty(),
        "rename 之后不该留下临时文件，实测 {leftovers:?}"
    );
}
