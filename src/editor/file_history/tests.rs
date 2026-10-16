use super::Editor;
use gpui::{Entity, TestAppContext, VisualTestContext};
use std::path::PathBuf;

fn fixture(
    cx: &mut TestAppContext,
    check: impl FnOnce(Entity<Editor>, &mut VisualTestContext, PathBuf),
) {
    let root = std::env::temp_dir().join(format!("velora-history-ui-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("夹具");
    let _config = crate::config::override_test_config_root(root.join("config"));
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
        crate::config::EditorSettings::init(cx, true);
        crate::config::EditorSettings::set_autosave_in_memory(false, cx);
    });
    let path = root.join("notes.md");
    std::fs::write(&path, "当前内容\n").expect("当前文件");
    crate::config::record_file_history(&path, "历史内容\n").expect("历史");
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
    editor.update_in(cx, |editor, window, cx| {
        editor.open_workspace_file(path.clone(), window, cx)
    });
    redraw(cx);
    check(editor, cx, path);
    cx.cx
        .on_quit(move || std::fs::remove_dir_all(root).expect("清理夹具"));
}

fn redraw(cx: &mut VisualTestContext) {
    for _ in 0..2 {
        cx.update(|window, cx| window.draw(cx).clear());
        cx.run_until_parked();
    }
}

#[gpui::test]
fn choosing_a_history_entry_only_previews_it_until_restore_is_confirmed(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, path| {
        editor.update(cx, |editor, cx| editor.open_file_history(cx));
        redraw(cx);
        let row = cx.debug_bounds("file-history-entry-0").expect("版本行");
        cx.simulate_mouse_down(
            row.center(),
            gpui::MouseButton::Left,
            gpui::Modifiers::none(),
        );
        cx.simulate_mouse_up(
            row.center(),
            gpui::MouseButton::Left,
            gpui::Modifiers::none(),
        );
        redraw(cx);
        assert_eq!(
            editor.read_with(cx, |editor, _| editor.buffer.text()),
            "当前内容\n",
            "选择历史不能替换正文"
        );
        assert_eq!(
            std::fs::read_to_string(path).expect("磁盘内容"),
            "当前内容\n"
        );
        assert!(
            cx.debug_bounds("file-history-preview").is_some(),
            "历史内容预览"
        );
        assert!(
            cx.debug_bounds("file-history-compare").is_some(),
            "对比入口"
        );
    });
}

#[gpui::test]
fn restoring_history_requires_confirmation_and_preserves_a_safety_checkpoint(
    cx: &mut TestAppContext,
) {
    fixture(cx, |editor, cx, path| {
        editor.update(cx, |editor, cx| {
            editor.open_file_history(cx);
            editor.restore_file_history_version(0, cx);
        });
        assert!(
            editor.read_with(cx, |editor, _| editor.modal_is_open()),
            "恢复前必须确认"
        );
        assert_eq!(
            editor.read_with(cx, |editor, _| editor.buffer.text()),
            "当前内容\n"
        );
        editor.update_in(cx, |editor, window, cx| editor.dismiss_modal(0, window, cx));
        redraw(cx);
        assert_eq!(
            editor.read_with(cx, |editor, _| editor.buffer.text()),
            "历史内容\n"
        );
        assert!(
            crate::config::list_file_history(&path)
                .iter()
                .any(|version| std::fs::read_to_string(version)
                    .expect("备份")
                    .contains("当前内容")),
            "恢复前内容必须有历史备份"
        );
        editor.update(cx, |editor, cx| editor.undo_document(cx));
        redraw(cx);
        assert_eq!(
            editor.read_with(cx, |editor, _| editor.buffer.text()),
            "当前内容\n"
        );
    });
}

#[gpui::test]
fn automatic_saves_also_create_file_history(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, path| {
        cx.cx.update(|cx| {
            crate::config::EditorSettings::set_autosave_in_memory(true, cx);
            crate::config::EditorSettings::set_file_history_in_memory(
                cx,
                crate::config::file_history::HistoryPolicy {
                    interval_minutes: 0,
                    ..Default::default()
                },
            );
        });
        editor.update(cx, |editor, cx| {
            editor.buffer.edit(0..0, "自动保存");
            editor.document_dirty = true;
            editor.schedule_autosave(cx);
        });
        cx.run_until_parked();
        cx.executor()
            .advance_clock(std::time::Duration::from_secs(2));
        cx.run_until_parked();
        assert!(
            crate::config::list_file_history(&path)
                .iter()
                .any(|version| std::fs::read_to_string(version)
                    .expect("版本")
                    .contains("自动保存")),
            "自动保存也要接入文件历史"
        );
    });
}

fn versions(path: &std::path::Path) -> Vec<crate::config::file_history::HistoryVersion> {
    crate::config::file_history::list_versions(
        path,
        Default::default(),
        std::time::SystemTime::now(),
    )
    .expect("历史列表")
}

fn edit(editor: &Entity<Editor>, cx: &mut VisualTestContext, text: &str) {
    editor.update(cx, |editor, cx| {
        let selection = editor.capture_source_selection_snapshot(cx);
        let edit = editor.buffer.edit(0..0, text);
        editor.undo_history.push(super::HistoryEntry {
            edits: vec![edit],
            selection,
            timestamp: std::time::Instant::now(),
            kind: crate::components::UndoCaptureKind::NonCoalescible,
        });
        editor.rebuild_document_from_buffer(cx);
        editor.mark_dirty_written_back(cx);
    });
}

#[gpui::test]
fn history_comparison_is_readonly_and_tracks_current_unsaved_content(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, _| {
        editor.update(cx, |editor, cx| editor.open_file_history(cx));
        redraw(cx);
        let compare = cx.debug_bounds("file-history-compare").expect("对比按钮");
        cx.simulate_mouse_down(
            compare.center(),
            gpui::MouseButton::Left,
            gpui::Modifiers::none(),
        );
        cx.simulate_mouse_up(
            compare.center(),
            gpui::MouseButton::Left,
            gpui::Modifiers::none(),
        );
        redraw(cx);
        editor.read_with(cx, |editor, _| {
            let overlay = editor.file_history_overlay.as_ref().expect("历史面板");
            assert!(overlay.compare);
            assert!(
                overlay
                    .lines
                    .iter()
                    .any(|line| line.kind == super::diff::DiffKind::Added
                        && line.text.contains("当前内容"))
            );
            assert!(
                overlay
                    .lines
                    .iter()
                    .any(|line| line.kind == super::diff::DiffKind::Removed
                        && line.text.contains("历史内容"))
            );
            assert!(!editor.document_dirty);
        });
        edit(&editor, cx, "新输入");
        redraw(cx);
        editor.read_with(cx, |editor, _| {
            assert!(
                editor
                    .file_history_overlay
                    .as_ref()
                    .expect("历史面板")
                    .lines
                    .iter()
                    .any(|line| line.text.contains("新输入"))
            )
        });
    });
}

#[gpui::test]
fn cancelling_history_restore_keeps_content_and_history_untouched(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, path| {
        let before = versions(&path).len();
        editor.update(cx, |editor, cx| {
            editor.open_file_history(cx);
            editor.restore_file_history_version(0, cx);
        });
        editor.update_in(cx, |editor, window, cx| editor.dismiss_modal(1, window, cx));
        assert_eq!(
            editor.read_with(cx, |editor, _| editor.buffer.text()),
            "当前内容\n"
        );
        assert!(editor.read_with(cx, |editor, _| editor.file_history_is_open()));
        assert_eq!(versions(&path).len(), before);
    });
}

#[gpui::test]
fn restoring_history_preserves_older_undo_steps_and_latest_unsaved_bytes(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, path| {
        edit(&editor, cx, "修改");
        editor.update(cx, |editor, cx| {
            editor.open_file_history(cx);
            editor.restore_file_history_version(0, cx);
        });
        edit(&editor, cx, "确认前");
        editor.update_in(cx, |editor, window, cx| editor.dismiss_modal(0, window, cx));
        let backup = versions(&path)
            .into_iter()
            .find(|version| {
                version.source == crate::config::file_history::HistorySource::BeforeRestore
            })
            .expect("恢复前检查点");
        assert_eq!(
            std::fs::read_to_string(backup.path).expect("备份"),
            "确认前修改当前内容\n"
        );
        for expected in ["确认前修改当前内容\n", "修改当前内容\n", "当前内容\n"]
        {
            editor.update(cx, |editor, cx| editor.undo_document(cx));
            assert_eq!(
                editor.read_with(cx, |editor, _| editor.buffer.text()),
                expected
            );
        }
        assert_eq!(
            std::fs::read_to_string(path).expect("磁盘"),
            "当前内容\n",
            "恢复先作为可撤销修改，不直接覆盘"
        );
    });
}

#[gpui::test]
fn history_restore_with_one_entry_limit_reads_target_before_backing_up(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, path| {
        cx.cx.update(|cx| {
            crate::config::EditorSettings::set_file_history_in_memory(
                cx,
                crate::config::file_history::HistoryPolicy {
                    max_entries: 1,
                    ..Default::default()
                },
            )
        });
        editor.update(cx, |editor, cx| {
            editor.open_file_history(cx);
            editor.restore_file_history_version(0, cx);
        });
        editor.update_in(cx, |editor, window, cx| editor.dismiss_modal(0, window, cx));
        assert_eq!(
            editor.read_with(cx, |editor, _| editor.buffer.text()),
            "历史内容\n"
        );
        let versions = versions(&path);
        assert_eq!(versions.len(), 1);
        assert_eq!(
            std::fs::read_to_string(&versions[0].path).expect("检查点"),
            "当前内容\n"
        );
    });
}

#[gpui::test]
fn failed_history_backup_never_replaces_the_current_document(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, path| {
        editor.update(cx, |editor, cx| {
            editor.open_file_history(cx);
            editor.restore_file_history_version(0, cx);
        });
        let metadata = versions(&path).remove(0).path.with_extension("json");
        std::fs::write(metadata, "损坏的元数据").expect("模拟历史备份失败");
        editor.update_in(cx, |editor, window, cx| editor.dismiss_modal(0, window, cx));
        assert_eq!(
            editor.read_with(cx, |editor, _| editor.buffer.text()),
            "当前内容\n"
        );
        assert!(
            editor.read_with(cx, |editor, _| editor.modal_is_open()),
            "错误必须可见"
        );
    });
}

#[gpui::test]
fn history_restore_cannot_apply_to_a_different_document(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, path| {
        editor.update(cx, |editor, cx| {
            editor.open_file_history(cx);
            editor.restore_file_history_version(0, cx);
        });
        let other = path.with_file_name("other.md");
        std::fs::write(&other, "另一篇\n").expect("另一篇文档");
        editor.update_in(cx, |editor, window, cx| {
            editor.open_workspace_file(other, window, cx)
        });
        editor.update_in(cx, |editor, window, cx| editor.dismiss_modal(0, window, cx));
        assert_eq!(
            editor.read_with(cx, |editor, _| editor.buffer.text()),
            "另一篇\n"
        );
    });
}

#[gpui::test]
fn manual_save_marks_a_checkpoint_even_after_autosave(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, path| {
        editor.update_in(cx, |editor, window, cx| {
            assert!(editor.save_to_existing_path(&path, window, cx))
        });
        let newest = versions(&path).remove(0);
        assert_eq!(
            newest.source,
            crate::config::file_history::HistorySource::Manual
        );
        assert_eq!(
            std::fs::read_to_string(newest.path).expect("手动检查点"),
            "当前内容\n"
        );
    });
}

#[gpui::test]
fn save_as_captures_existing_target_before_overwriting_it(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, path| {
        let target = path.with_file_name("saved.md");
        std::fs::write(&target, "目标原文\r\n").expect("目标");
        editor.update(cx, |editor, cx| {
            let identity = editor.document_save_identity();
            let text = editor.buffer.text();
            let bytes = editor.buffer.file_bytes();
            editor
                .complete_save_request(target.clone(), identity, text, bytes, cx)
                .expect("另存为");
        });
        let versions = versions(&target);
        assert!(versions.iter().any(|version| version.source
            == crate::config::file_history::HistorySource::Baseline
            && std::fs::read(&version.path).expect("原始字节")
                == b"\xe7\x9b\xae\xe6\xa0\x87\xe5\x8e\x9f\xe6\x96\x87\r\n"));
        assert!(versions.iter().any(|version| version.source == crate::config::file_history::HistorySource::Manual));
    });
}

#[gpui::test]
fn closing_dirty_background_tabs_records_their_saved_contents(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, path| {
        edit(&editor, cx, "后台");
        let other = path.with_file_name("other.md");
        std::fs::write(&other, "另一篇\n").expect("另一篇");
        editor.update_in(cx, |editor, window, cx| {
            editor.pin_active_preview_tab();
            editor.open_workspace_file(other, window, cx);
            editor.close_workspace_tabs(std::slice::from_ref(&path), window, cx);
            assert!(editor.modal_is_open());
            editor.dismiss_modal(0, window, cx);
        });
        let newest = versions(&path).remove(0);
        assert_eq!(
            newest.source,
            crate::config::file_history::HistorySource::Exit
        );
        assert_eq!(
            std::fs::read_to_string(newest.path).expect("关闭保存的版本"),
            "后台当前内容\n"
        );
    });
}

#[gpui::test]
fn workspace_pruning_records_saved_dirty_tabs(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, path| {
        edit(&editor, cx, "离开工作区");
        editor.update(cx, |editor, cx| {
            editor.snapshot_current_document(cx);
            assert!(
                editor.prune_workspace_tabs_outside_root(&path.with_file_name("elsewhere"), cx)
            );
        });
        let newest = versions(&path).remove(0);
        assert_eq!(
            newest.source,
            crate::config::file_history::HistorySource::Exit
        );
        assert_eq!(
            std::fs::read_to_string(newest.path).expect("工作区保存的版本"),
            "离开工作区当前内容\n"
        );
    });
}

#[gpui::test]
fn save_and_close_records_all_dirty_documents(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, path| {
        edit(&editor, cx, "第一篇");
        let other = path.with_file_name("other.md");
        std::fs::write(&other, "另一篇\n").expect("另一篇");
        editor.update_in(cx, |editor, window, cx| {
            editor.pin_active_preview_tab();
            editor.open_workspace_file(other.clone(), window, cx);
        });
        edit(&editor, cx, "第二篇");
        editor.update_in(cx, |editor, window, cx| {
            editor.save_dirty_workspace_documents_and_close(window, cx)
        });
        cx.run_until_parked();
        for (path, expected) in [(path, "第一篇当前内容\n"), (other, "第二篇另一篇\n")]
        {
            let newest = versions(&path).remove(0);
            assert_eq!(
                newest.source,
                crate::config::file_history::HistorySource::Exit
            );
            assert_eq!(
                std::fs::read_to_string(newest.path).expect("退出检查点"),
                expected
            );
        }
    });
}

#[gpui::test]
fn history_restore_waits_until_ime_composition_is_finished(cx: &mut TestAppContext) {
    use gpui::EntityInputHandler;
    fixture(cx, |editor, cx, _| {
        let block = editor.read_with(cx, |editor, _| {
            editor.document.first_root().expect("正文块").clone()
        });
        block.update_in(cx, |block, window, cx| {
            block.replace_and_mark_text_in_range(None, "输入", Some(2..2), window, cx)
        });
        cx.run_until_parked();
        let before = editor.read_with(cx, |editor, _| editor.buffer.text());
        editor.update(cx, |editor, cx| {
            editor.open_file_history(cx);
            editor.restore_file_history_version(0, cx);
        });
        editor.update_in(cx, |editor, window, cx| editor.dismiss_modal(0, window, cx));
        assert_eq!(
            editor.read_with(cx, |editor, _| editor.buffer.text()),
            before,
            "输入法组合未完成不能替换正文"
        );
        assert!(
            editor.read_with(cx, |editor, _| editor.modal_is_open()),
            "应提示先完成输入"
        );
    });
}

#[gpui::test]
fn history_code_lines_follow_code_font_preferences_and_zoom(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, _| {
        cx.cx.update(|cx| {
            let mut preferences =
                crate::config::load_or_create_app_preferences().expect("配置");
            preferences.fonts.code_family = "Menlo".into();
            preferences.fonts.code_size = 20;
            preferences.zoom_percent = 150;
            let dirs = crate::config::VeloraConfigDirs::from_system().expect("目录");
            crate::config::preferences::save_app_preferences_with_dirs(&preferences, &dirs)
                .expect("保存字体配置");
            cx.remove_global::<crate::config::EditorSettings>();
            crate::config::EditorSettings::init(cx, true);
        });
        editor.update(cx, |editor, cx| editor.open_file_history(cx));
        redraw(cx);
        for compare in [false, true] {
            if compare {
                let button = cx.debug_bounds("file-history-compare").expect("对比按钮");
                cx.simulate_click(button.center(), gpui::Modifiers::none());
                redraw(cx);
            }
            let line = cx
                .debug_bounds("file-history-code-line-0")
                .expect("历史代码行");
            assert_eq!(
                f32::from(line.size.height),
                45.0,
                "预览与对比应采用代码字号及缩放后的行高"
            );
        }
    });
}
