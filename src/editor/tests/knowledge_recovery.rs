use super::common::*;
use super::loading_chunks::chunk_boundary_source;

#[gpui::test]
async fn wikilink_creates_missing_file_at_workspace_root(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let root = std::env::temp_dir().join(format!("velora-wikilink-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("create root");
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
    editor.update(cx, |editor, cx| {
        editor.set_workspace_root(root.clone(), cx);
    });
    let created = root.join("fresh note.md");
    let created_for_assert = created.clone();
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_wikilink("fresh note".into(), window, cx);
        });
        window.draw(cx).clear();
    });
    // The open path is canonicalized by set_workspace_root, so compare
    // against the canonical form of the created file.
    let canonical_created = std::fs::canonicalize(&created_for_assert)
        .expect("canonicalize created");
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(editor.file_path, Some(canonical_created.clone()));
    });
    assert!(created.is_file(), "wikilink target should be created");
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
async fn wikilink_opens_existing_workspace_file(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let root = std::env::temp_dir().join(format!("velora-wikilink-open-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("create root");
    let existing = root.join("target.md");
    std::fs::write(&existing, "# target\n").expect("write");

    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
    editor.update(cx, |editor, cx| {
        editor.set_workspace_root(root.clone(), cx);
    });
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_wikilink("target".into(), window, cx);
        });
        window.draw(cx).clear();
    });
    let canonical_existing = std::fs::canonicalize(&existing).expect("canonicalize existing");
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(editor.file_path.as_deref(), Some(canonical_existing.as_path()));
    });
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
async fn crash_recovery_drill_snapshot_restore_save(cx: &mut TestAppContext) {
    // roadmap G6：「写快照 → 崩溃 → 恢复 → 保存」全链路演练。
    init_editor_test_app(cx);

    let source_path = temp_markdown_path("crash-drill");
    fs::write(&source_path, "saved before crash").expect("seed file");
    let cleanup_source = source_path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup_source);
    });

    // Phase 1 — 编辑后写入恢复快照（与自动保存使用同一持久化函数）。
    let (editor, cx) = cx.add_window_view({
        let path = source_path.clone();
        move |_window, cx| Editor::from_markdown(cx, "saved before crash".into(), Some(path))
    });
    let recovery_id = editor.read_with(cx, |editor, _cx| editor.recovery_id);
    let unsaved = "edited after save, not yet on disk";
    editor.update(cx, |editor, cx| {
        let block = editor.document.first_root().expect("block").clone();
        block.update(cx, |block, _cx| {
            block
                .record
                .set_title(InlineTextTree::plain(unsaved.to_string()));
            block.sync_render_cache();
        });
        editor.mark_dirty(cx);
    });
    crate::config::save_recovery_snapshot(&crate::config::RecoverySnapshot {
        id: recovery_id,
        source_path: Some(source_path.clone()),
        markdown: unsaved.into(),
    })
    .expect("write snapshot");
    assert!(crate::config::read_recovery_snapshots()
        .expect("read snapshots")
        .iter()
        .any(|snapshot| snapshot.id == recovery_id));

    // Phase 2 — 崩溃：直接丢弃编辑器实体（不走任何关闭流程）。磁盘文件
    // 仍是旧内容，只有恢复快照里有未保存编辑。
    drop(editor);
    assert_eq!(
        fs::read_to_string(&source_path).expect("read disk"),
        "saved before crash"
    );

    // Phase 3 — 恢复：from_recovery 打开未保存内容（dirty 副本）。
    let (editor, cx) = cx.add_window_view({
        let snapshot_path = source_path.clone();
        move |_window, cx| {
            Editor::from_recovery(
                cx,
                crate::config::RecoverySnapshot {
                    id: recovery_id,
                    source_path: Some(snapshot_path),
                    markdown: unsaved.into(),
                },
            )
        }
    });
    editor.read_with(cx, |editor, cx| {
        assert!(editor.is_recovered_document);
        assert_eq!(editor.document.markdown_text(cx), unsaved);
    });

    // Phase 4 — 保存：磁盘内容更新，恢复快照被清理。
    // 真实应用中恢复文档经对话框另存到源路径；演练直接调用同一保存内核。
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            assert!(editor.save_to_existing_path(&source_path, window, cx));
        });
    });
    assert_eq!(
        fs::read_to_string(&source_path).expect("read after save"),
        unsaved
    );
    assert!(!crate::config::read_recovery_snapshots()
        .expect("read snapshots")
        .iter()
        .any(|snapshot| snapshot.id == recovery_id));
}

#[gpui::test]
async fn tmp_debug_merge_state(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let source = chunk_boundary_source();
    let path = std::env::temp_dir().join(format!("velora-chunk-dbg-{}.log", std::process::id()));
    fs::write(&path, &source).expect("write chunk fixture");

    let editor =
        cx.update(|cx| crate::app_menu::open_editor_window(cx, source.clone(), Some(path)));
    cx.run_until_parked();
    editor
        .read_with(cx, |editor, _cx| {
            for (index, visible) in editor.document.visible_blocks().iter().enumerate() {
                let text = visible.entity.read(_cx).display_text();
                println!("before block[{index}] id={:?} len={}", visible.entity.entity_id(), text.len());
            }
        })
        .expect("open");
    editor
        .update(cx, |editor, window, cx| {
            let blocks = editor.document.flatten_visible_blocks();
            let second = blocks[1].entity.clone();
            second.update(cx, |block, cx| {
                block.selected_range = 0..0;
                block.on_delete_back(&DeleteBack, window, cx);
            });
        })
        .expect("edit");
    editor
        .read_with(cx, |editor, _cx| {
            let blocks = editor.document.visible_blocks();
            println!("pre-park: {} blocks", blocks.len());
            for (index, visible) in blocks.iter().enumerate() {
                let text = visible.entity.read(_cx).display_text();
                println!("pre-park block[{index}] id={:?} len={}", visible.entity.entity_id(), text.len());
            }
        })
        .expect("read");
    cx.run_until_parked();
    editor
        .read_with(cx, |editor, _cx| {
            let blocks = editor.document.visible_blocks();
            println!("after merge: {} blocks", blocks.len());
            for (index, visible) in blocks.iter().enumerate() {
                let text = visible.entity.read(_cx).display_text();
                println!("block[{index}] id={:?} len={}", visible.entity.entity_id(), text.len());
            }
        })
        .expect("read");
}
