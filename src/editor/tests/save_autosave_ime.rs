use super::common::*;

#[gpui::test]
#[cfg(unix)]
async fn core_repair_autosave_preserves_symlink_targets(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let real = temp_markdown_path("autosave-link-real");
    let link = temp_markdown_path("autosave-link");
    fs::write(&real, "old").unwrap();
    std::os::unix::fs::symlink(&real, &link).unwrap();
    cx.on_quit({
        let (real, link) = (real.clone(), link.clone());
        move || { fs::remove_file(link).unwrap(); fs::remove_file(real).unwrap(); }
    });
    let document = crate::editor::encoding::load_document(&link).unwrap();
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_loaded_document(cx, document, Some(link.clone())));
    editor.update(cx, |editor, cx| {
        let block = editor.document.first_root().unwrap().clone();
        block.update(cx, |block, _| {
            block.record.set_title(InlineTextTree::plain("saved"));
            block.sync_render_cache();
        });
        editor.mark_dirty(cx);
    });
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    assert!(fs::symlink_metadata(&link).unwrap().is_symlink());
    assert_eq!(fs::read_to_string(&real).unwrap(), "saved");
}

#[gpui::test]
#[cfg(unix)]
async fn core_repair_autosave_preserves_private_permissions(cx: &mut TestAppContext) {
    use std::os::unix::fs::PermissionsExt;
    init_editor_test_app(cx);
    let path = temp_markdown_path("autosave-private");
    fs::write(&path, "old").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    cx.on_quit({ let path = path.clone(); move || fs::remove_file(path).unwrap() });
    let document = crate::editor::encoding::load_document(&path).unwrap();
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_loaded_document(cx, document, Some(path.clone())));
    editor.update(cx, |editor, cx| {
        let block = editor.document.first_root().unwrap().clone();
        block.update(cx, |block, _| {
            block.record.set_title(InlineTextTree::plain("saved"));
            block.sync_render_cache();
        });
        editor.mark_dirty(cx);
    });
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    assert_eq!(fs::read_to_string(&path).unwrap(), "saved");
    assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
}

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
#[cfg(unix)]
async fn core_repair_save_and_close_preserves_symlink_targets(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let real = temp_markdown_path("save-close-link-real");
    let link = temp_markdown_path("save-close-link");
    fs::write(&real, "old").unwrap();
    std::os::unix::fs::symlink(&real, &link).unwrap();
    cx.on_quit({
        let (real, link) = (real.clone(), link.clone());
        move || { fs::remove_file(link).unwrap(); fs::remove_file(real).unwrap(); }
    });
    let document = crate::editor::encoding::load_document(&link).unwrap();
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_loaded_document(cx, document, Some(link.clone())));
    edit_first_block(&editor, cx, "saved");
    editor.update_in(cx, |editor, window, cx| editor.save_dirty_workspace_documents_and_close(window, cx));
    cx.run_until_parked();
    assert!(fs::symlink_metadata(&link).unwrap().is_symlink());
    assert_eq!(fs::read_to_string(&real).unwrap(), "saved");
}

#[gpui::test]
#[cfg(unix)]
async fn core_repair_save_and_close_preserves_private_permissions(cx: &mut TestAppContext) {
    use std::os::unix::fs::PermissionsExt;
    init_editor_test_app(cx);
    let path = temp_markdown_path("save-close-private");
    fs::write(&path, "old").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    cx.on_quit({ let path = path.clone(); move || fs::remove_file(path).unwrap() });
    let document = crate::editor::encoding::load_document(&path).unwrap();
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_loaded_document(cx, document, Some(path.clone())));
    edit_first_block(&editor, cx, "saved");
    editor.update_in(cx, |editor, window, cx| editor.save_dirty_workspace_documents_and_close(window, cx));
    cx.run_until_parked();
    assert_eq!(fs::read_to_string(&path).unwrap(), "saved");
    assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
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

/// 一轮自动保存对同一个目标文件只读两遍盘：一遍在后台取落盘形状，一遍在
/// rename 之前兜底校验。中间那次 verify 的结论紧接着被 `commit_checked`
/// 重做一遍，属于白读——大文档每轮多付一整篇解码加哈希。
#[gpui::test]
async fn one_autosave_round_verifies_the_target_file_twice(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let path = temp_markdown_path("autosave-verify-reads");
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
    editor.read_with(cx, |editor, _cx| {
        editor
            .autosave_verify_reads
            .store(0, std::sync::atomic::Ordering::Relaxed);
    });

    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();

    assert_eq!(
        fs::read_to_string(&path).expect("read autosaved markdown"),
        "autosaved text"
    );
    let reads = editor.read_with(cx, |editor, _cx| {
        editor
            .autosave_verify_reads
            .load(std::sync::atomic::Ordering::Relaxed)
    });
    assert_eq!(
        reads, 2,
        "一轮自动保存对目标文件做了 {reads} 次读盘校验，只该有「取形状」与「rename 前兜底」两次"
    );
}

/// 没有输入法组合时，调度自动保存不该逐块问「你有没有组合中的文本」。
///
/// 守卫 `has_marked_document_text` 在每键的 `schedule_autosave` 开头跑：逐块
/// 读实体扫全部可见块加全部表格格子。组合态是窗口级的少数事件，正常打字时的
/// 判定必须是 O(1)。
#[gpui::test]
async fn scheduling_a_save_does_not_scan_every_block_for_composition_state(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    let path = temp_markdown_path("autosave-composition-guard");
    let markdown = (0..200)
        .map(|index| format!("第 {index} 段正文。\n\n"))
        .collect::<String>();
    fs::write(&path, &markdown).expect("write initial markdown");
    let cleanup_path = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup_path);
    });
    let (editor, cx) = cx.add_window_view({
        let path = path.clone();
        move |_window, cx| Editor::from_markdown(cx, markdown.clone(), Some(path))
    });
    redraw(cx);
    let blocks = editor.read_with(cx, |editor, _cx| editor.document.visible_blocks().len());
    assert!(blocks > 100, "夹具得有足够多的块才测得出全量扫描：{blocks}");

    editor.update(cx, |editor, cx| {
        editor.autosave_guard_block_reads.set(0);
        for _ in 0..3 {
            editor.schedule_autosave(cx);
        }
    });
    let reads = editor.read_with(cx, |editor, _cx| editor.autosave_guard_block_reads.get());
    assert_eq!(
        reads, 0,
        "没有组合文本时三次调度读了 {reads} 个块实体：每键都在为 IME 守卫扫全篇"
    );
}

/// ⌘S 的整条链路不再跑在调用线程上：返回那一刻目标文件还没被替换，读盘校验与
/// 临时文件写入在后台，完成后才回主线程提交。
#[gpui::test]
async fn manual_save_defers_io_to_the_background(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let path = temp_markdown_path("manual-save-background");
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
                .set_title(InlineTextTree::plain("beta".to_string()));
            block.sync_render_cache();
        });
        editor.mark_dirty(cx);
    });
    editor.update_in(cx, |editor, window, cx| {
        assert!(editor.save_to_existing_path(&path, window, cx));
    });
    assert_eq!(
        fs::read_to_string(&path).expect("read before the background round"),
        "alpha",
        "保存受理就同步写完了：整条 IO 还在调用线程上"
    );
    cx.run_until_parked();
    assert_eq!(
        fs::read_to_string(&path).expect("read after the background round"),
        "beta"
    );
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
    cx.run_until_parked();
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            // 保存受理后版本校验在后台做：冲突在完成回调里上报，文件保持外部内容。
            assert!(editor.save_to_existing_path(&path, window, cx));
        });
    });
    cx.run_until_parked();
    assert_eq!(
        fs::read_to_string(&path).expect("read external file after manual save"),
        "external edits"
    );
}

/// 自动保存读不到盘上内容时，只能有一个框，而且必须是那个能解除冲突的框。
///
/// 能在 HEAD 复现的触发条件是
/// 「这一篇的文件此刻读不出来」——被别的程序独占、被同步盘移走、被改名、被删掉。
/// 这类失败在 `verify_file_version` 里长成「无法读取文件以检查外部修改：…」，与
/// 「检测到外部修改」是同一件事的两面（都不能写，都要靠重载解除），失败上报那一支
/// 两个前缀都认；可最后决定要不要再弹一个框的那道判断只认后一个前缀，于是冲突框之上
/// 又叠了一个「保存失败」框。`show_modal` 是**替换**语义：用户丢掉的是「重载 / 另存为」
/// 这个唯一的解除入口，只剩一块「好」，而自动保存仍然停着——看上去就像「自动保存
/// 自己失败了」。
#[gpui::test]
async fn an_unreadable_file_at_autosave_shows_the_conflict_modal_only(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);

    let path = temp_markdown_path("autosave-unreadable-file");
    fs::write(&path, "alpha\n").expect("write initial markdown");
    let cleanup_path = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup_path);
    });

    let (editor, cx) = cx.add_window_view({
        let path = path.clone();
        move |_window, cx| Editor::from_markdown(cx, "alpha\n".to_string(), Some(path))
    });
    let recovery_id = editor.read_with(cx, |editor, _cx| editor.recovery_id);
    cx.on_quit(move || {
        let _ = crate::config::remove_recovery_snapshot(recovery_id);
    });
    edit_first_block(&editor, cx, "our edits");

    // 文件读不出来了（这里用最省事的一种：它已经不在原地）。
    fs::remove_file(&path).expect("remove the document file");

    cx.executor().advance_clock(Duration::from_secs(2));
    cx.run_until_parked();

    let strings = cx.update(|_window, cx| cx.global::<I18nManager>().strings().clone());
    editor.read_with(cx, |editor, _cx| {
        assert!(
            editor.has_external_autosave_conflict(),
            "读不到盘上内容时不能继续写，冲突要记下来"
        );
        let spec = editor.modal_spec().expect("冲突要有能解除它的框");
        assert_eq!(
            spec.title, strings.external_change_title,
            "只能有一个框，而且是「外部修改」那个（带重载 / 另存为），实测 {:?}",
            spec.title
        );
        assert_eq!(
            spec.buttons.len(),
            3,
            "「重载 / 另存为 / 继续编辑」三个入口不能被「保存失败」框顶掉，实测 {:?}",
            spec.buttons
        );
    });
}

/// 反过来：真写不进去的时候，「保存失败」框还得照旧出——修上面的误叠不能把提示一起修没。
///
/// 这里手动把 `file_version` 清掉跳过盘上比对（冲突判定就不参与了），再把文档所在目录
/// 整个挪走：临时文件创建不了，失败纯属写盘。三个平台都成立（不靠 chmod，也不靠 Windows
/// 的独占句柄）。
#[gpui::test]
async fn a_real_write_failure_still_reports_save_failed(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let root = std::env::temp_dir().join(format!("velora-write-fail-{}", temp_fixture_token()));
    fs::create_dir_all(&root).expect("create document dir");
    let path = root.join("note.md");
    fs::write(&path, "alpha\n").expect("write initial markdown");
    cx.on_quit({
        let root = root.clone();
        let path = path.clone();
        move || {
            // 目录被用例挪到 `moved` 去了，两个名字都要试一次。
            let _ = fs::remove_dir_all(&root);
            let _ = fs::remove_dir_all(moved_document_dir(&root));
            let _ = fs::remove_file(&path);
        }
    });

    let (editor, cx) = cx.add_window_view({
        let path = path.clone();
        move |_window, cx| Editor::from_markdown(cx, "alpha\n".to_string(), Some(path))
    });
    edit_first_block(&editor, cx, "our edits");

    editor.update(cx, |editor, _cx| editor.file_version = None);
    fs::rename(&root, moved_document_dir(&root)).expect("move the directory away");

    let accepted = editor.update_in(cx, |editor, window, cx| {
        editor.save_to_existing_path(&path, window, cx)
    });
    assert!(accepted, "保存受理后由后台完成读盘与写临时文件");
    cx.run_until_parked();
    assert!(
        !path.exists(),
        "目录已被挪走，写不进去的文件不该凭空出现"
    );

    let strings = cx.update(|_window, cx| cx.global::<I18nManager>().strings().clone());
    editor.read_with(cx, |editor, _cx| {
        let spec = editor.modal_spec().expect("写盘失败要有提示");
        assert_eq!(
            spec.title, strings.save_failed_title,
            "真的写不进去就该报「保存失败」，实测 {:?}",
            spec.title
        );
        assert!(
            !editor.has_external_autosave_conflict(),
            "写盘失败不是外部修改，不该把自动保存整体会话停掉"
        );
    });
    assert!(
        fs::read_to_string(&path).is_err(),
        "失败的保存不能在原地留下半个文件"
    );
}

fn moved_document_dir(root: &std::path::Path) -> PathBuf {
    root.with_extension("moved")
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
    let root = std::env::temp_dir().join(format!("velora-{test_name}-{}", temp_fixture_token()));
    fs::create_dir_all(&root).expect("create document dir");
    // macOS 上 /var ↔ /private/var 是两个名字：不 canonicalize，编辑器算出的路径与
    // 断言里的对不上，临时残留也就扫不准。
    let root = crate::file_url::canonical_path(&root);
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

/// 一篇撞上外部修改冲突，不能把整个窗口其余文档的自动保存与恢复快照一起冻结
/// 快照不覆盖磁盘、无覆盖风险，冲突只该挡冲突那一篇的磁盘提交。
#[gpui::test]
async fn autosave_keeps_saving_other_documents_during_an_external_change_conflict(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);

    let first_path = temp_markdown_path("conflict-freeze-first");
    let second_path = temp_markdown_path("conflict-freeze-second");
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
    cx.on_quit(move || {
        let _ = crate::config::remove_recovery_snapshot(first_recovery_id);
    });
    edit_first_block(&editor, cx, "edited alpha");
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(second_path.clone(), window, cx);
        });
    });
    edit_first_block(&editor, cx, "edited beta");

    // 外部把第一篇改掉，让自动保存先撞上一处冲突。
    fs::write(&first_path, "external alpha").expect("write external changes");
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    editor.read_with(cx, |editor, _cx| {
        assert!(
            editor.has_external_autosave_conflict(),
            "外部修改要先撞出一处冲突"
        );
    });
    assert_eq!(
        fs::read_to_string(&first_path).expect("read first document"),
        "external alpha",
        "冲突的那一篇不能被覆盖"
    );

    // 冲突未解除时继续在第二篇打字：第二篇的快照与写盘必须照常走。
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(second_path.clone(), window, cx);
        });
    });
    edit_first_block(&editor, cx, "second round beta");
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();

    assert_eq!(
        fs::read_to_string(&second_path).expect("read second document"),
        "second round beta",
        "一篇冲突不能冻结其余文档的自动保存"
    );
    assert_eq!(
        fs::read_to_string(&first_path).expect("read first document again"),
        "external alpha",
        "冲突在解除前必须继续拦着那一篇"
    );
    editor.read_with(cx, |editor, _cx| {
        assert!(editor.has_external_autosave_conflict());
    });
}

/// 防抖到点发现输入法组合还挂着：本轮取消后必须重排下一轮，否则用户在组合态
/// 停手离开的话，恢复快照与文件写全部停更到下一次编辑事件为止。
#[gpui::test]
async fn autosave_reschedules_when_composition_is_still_open_at_the_debounce(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);

    let path = temp_markdown_path("ime-autosave-reschedule");
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
    // 先留一处未保存修改，让一轮自动保存排上（此刻还没有组合态）。
    edit_first_block(&editor, cx, "pending");
    let block = editor.read_with(cx, |editor, _cx| {
        editor.document.first_root().expect("paragraph").clone()
    });
    block.update(cx, |block, _cx| block.selected_range = 7..7);
    // 防抖窗口内进入输入法组合：到点的那一轮会发现组合还挂着。
    cx.update(|window, cx| {
        block.update(cx, |block, block_cx| {
            <Block as EntityInputHandler>::replace_and_mark_text_in_range(
                block,
                None,
                "zh",
                Some(7..7),
                window,
                block_cx,
            );
        });
    });

    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    editor.read_with(cx, |editor, _cx| {
        assert!(
            editor.autosave_task.is_some(),
            "组合挂着时被顶掉的那一轮必须重排下一轮"
        );
    });
    assert_eq!(
        fs::read_to_string(&path).expect("read markdown during composition"),
        "alpha",
        "组合态下不该写盘"
    );
}

/// 关窗保存撞上外部修改：要走带「重载 / 另存为」解除入口的冲突记录，不该
/// 双路上报（侧栏红字 + 闭包外再弹一块纯展示模态）。
#[gpui::test]
async fn close_save_failure_records_the_conflict_instead_of_double_reporting(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);

    let first_path = temp_markdown_path("close-conflict-first");
    let second_path = temp_markdown_path("close-conflict-second");
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
    cx.on_quit(move || {
        let _ = crate::config::remove_recovery_snapshot(first_recovery_id);
    });
    edit_first_block(&editor, cx, "edited alpha");
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(second_path.clone(), window, cx);
        });
    });
    edit_first_block(&editor, cx, "edited beta");

    // 外部把第一篇改掉：关窗保存会在版本校验上撞冲突。
    fs::write(&first_path, "external alpha").expect("write external changes");
    editor.update_in(cx, |editor, window, cx| {
        editor.save_dirty_workspace_documents_and_close(window, cx);
    });
    cx.run_until_parked();

    editor.read_with(cx, |editor, _cx| {
        assert!(
            editor.has_external_autosave_conflict(),
            "关窗保存撞上外部修改要走冲突入口，而不是普通「保存失败」"
        );
        assert!(
            editor.show_unsaved_changes_dialog,
            "没保存成不能静默继续关窗"
        );
        assert!(!editor.pending_close_after_save);
    });
    assert_eq!(
        fs::read_to_string(&first_path).expect("read first document"),
        "external alpha",
        "被拦下的保存不能动盘上那一份"
    );
}

/// 另存为落到一篇已被外部改动的已打开标签上：要走冲突入口（重载 / 另存为），
/// 不能只弹一块没有解除入口的「保存失败」。
#[gpui::test]
async fn save_as_onto_an_externally_changed_open_tab_offers_the_conflict_entry(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);

    let alpha_path = temp_markdown_path("save-as-conflict-alpha");
    let beta_path = temp_markdown_path("save-as-conflict-beta");
    fs::write(&alpha_path, "alpha").expect("write alpha");
    fs::write(&beta_path, "beta").expect("write beta");
    let cleanup_alpha = alpha_path.clone();
    let cleanup_beta = beta_path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(cleanup_alpha);
        let _ = fs::remove_file(cleanup_beta);
    });

    let (editor, cx) = cx.add_window_view({
        let alpha_path = alpha_path.clone();
        move |_window, cx| Editor::from_markdown(cx, "alpha".to_string(), Some(alpha_path))
    });
    let alpha_recovery = editor.read_with(cx, |editor, _cx| editor.recovery_id);
    cx.on_quit(move || {
        let _ = crate::config::remove_recovery_snapshot(alpha_recovery);
    });

    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(beta_path.clone(), window, cx);
        });
    });
    // beta 标签还干净时，外部把 beta 改掉。
    fs::write(&beta_path, "beta 被外部改了").expect("write external beta");
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(alpha_path.clone(), window, cx);
        });
    });

    editor.update_in(cx, |editor, window, cx| {
        editor.save_document_as(window, cx);
    });
    cx.run_until_parked();
    assert!(cx.did_prompt_for_new_path(), "另存为要弹路径选择");
    cx.simulate_new_path_selection(|_| Some(beta_path.clone()));
    cx.run_until_parked();

    editor.read_with(cx, |editor, _cx| {
        assert!(
            editor.has_external_autosave_conflict(),
            "另存为撞上目标被外部改动要走冲突入口"
        );
    });
    assert_eq!(
        fs::read_to_string(&beta_path).expect("read beta"),
        "beta 被外部改了",
        "被拦下的另存为不能动盘上那一份"
    );
}

/// 监听溢出（need_rescan）之后，打开中的干净文档也要补一次磁盘比对：
/// 树是新的、正文停在旧版上而用户毫无感知，是丢事件窗口里最隐蔽的损失。
#[gpui::test]
async fn workspace_rescan_rechecks_open_clean_documents(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let path = temp_markdown_path("rescan-recheck");
    fs::write(&path, "alpha").expect("write initial markdown");
    let cleanup_path = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(cleanup_path);
    });

    let (editor, cx) = cx.add_window_view({
        let path = path.clone();
        move |_window, cx| Editor::from_markdown(cx, "alpha".to_string(), Some(path))
    });
    // 丢失窗口里外部改了这篇干净文档：正常的事件流没了，只剩 Rescan。
    fs::write(&path, "外部改了").expect("write external changes");

    editor.update(cx, |editor, cx| editor.on_workspace_watch_rescan(cx));
    cx.run_until_parked();

    editor.read_with(cx, |editor, cx| {
        assert_eq!(
            editor.document.markdown_text(cx),
            "外部改了",
            "重扫要把打开中干净文档的正文补齐"
        );
    });
}
