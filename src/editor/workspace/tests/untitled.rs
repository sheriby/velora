use super::super::{Editor, WorkspaceOpenMode};
use gpui::{Entity, TestAppContext, VisualTestContext};
use std::path::PathBuf;

fn fixture(
    cx: &mut TestAppContext,
    check: impl FnOnce(Entity<Editor>, &mut VisualTestContext, PathBuf),
) {
    let root = std::env::temp_dir().join(format!("velora-untitled-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("夹具目录");
    let root = crate::file_url::canonical_path(root);
    let _config = crate::config::override_test_config_root(root.join("config"));
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::config::EditorSettings::init(cx, true);
        crate::components::init(cx);
    });
    let path = root.join("notes.md");
    std::fs::write(&path, "原文\n").expect("原文夹具");
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
    editor.update_in(cx, |editor, window, cx| {
        editor.set_workspace_root(root.clone(), cx);
        editor.open_workspace_file(path.clone(), window, cx);
    });
    redraw(cx);
    check(editor, cx, path);
    cx.cx
        .on_quit(move || std::fs::remove_dir_all(root).expect("清理夹具"));
}

fn redraw(cx: &mut VisualTestContext) {
    for _ in 0..2 {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
    }
}

#[test]
fn new_document_command_replaces_the_new_window_entry() {
    let commands = crate::commands::commands();
    assert!(
        commands.iter().all(|command| command.id != "new_window"),
        "移除新建窗口命令"
    );
    let command = commands
        .iter()
        .find(|command| command.id == "new_document")
        .expect("新建文档命令");
    assert_eq!(
        command.label(&crate::i18n::I18nStrings::zh_cn()),
        "新建文档"
    );
}

#[gpui::test]
fn new_documents_are_independent_tabs_with_recoverable_drafts(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, path| {
        cx.cx
            .update(|cx| crate::config::EditorSettings::set_autosave_in_memory(false, cx));
        editor.update_in(cx, |editor, window, cx| {
            editor.new_workspace_document(window, cx)
        });
        redraw(cx);
        assert_eq!(cx.cx.windows().len(), 1, "新建文档不另开窗口");
        assert_eq!(
            editor.read_with(cx, |editor, _| editor.workspace.open_documents.len()),
            2,
            "保留原文件并新建标签"
        );
        let first = editor
            .read_with(cx, |editor, _| editor.workspace.active_document.clone())
            .expect("第一个未命名标签");
        cx.simulate_input("第一份草稿");
        editor.update_in(cx, |editor, window, cx| {
            editor.new_workspace_document(window, cx)
        });
        redraw(cx);
        let second = editor
            .read_with(cx, |editor, _| editor.workspace.active_document.clone())
            .expect("第二个未命名标签");
        assert_ne!(first, second);
        cx.simulate_input("第二份草稿");
        editor.update_in(cx, |editor, window, cx| {
            editor.open_workspace_file_in_mode(
                first.clone(),
                WorkspaceOpenMode::Activate,
                window,
                cx,
            );
        });
        redraw(cx);
        assert_eq!(
            editor.read_with(cx, |editor, _| editor.buffer.text()),
            "第一份草稿"
        );
        assert!(editor.read_with(cx, |editor, _| editor.file_path.is_none()));
        editor.update_in(cx, |editor, window, cx| {
            editor.open_workspace_file(path.clone(), window, cx)
        });
        redraw(cx);
        cx.executor()
            .advance_clock(std::time::Duration::from_secs(2));
        cx.run_until_parked();
        let snapshots = crate::config::read_recovery_snapshots().expect("读取草稿");
        for content in ["第一份草稿", "第二份草稿"] {
            assert!(
                snapshots
                    .iter()
                    .any(|snapshot| snapshot.source_path.is_none() && snapshot.markdown == content),
                "后台未命名标签也要有独立临时文件：{content}"
            );
        }
        assert_eq!(std::fs::read_to_string(path).expect("磁盘原文"), "原文\n");
        assert_eq!(
            editor.read_with(cx, |editor, _| editor.workspace.open_documents.len()),
            3
        );
    });
}

#[gpui::test]
fn new_document_shortcut_creates_a_tab_in_the_current_window(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, _| {
        cx.simulate_keystrokes(if cfg!(target_os = "macos") {
            "cmd-n"
        } else {
            "ctrl-n"
        });
        redraw(cx);
        assert_eq!(cx.cx.windows().len(), 1);
        assert_eq!(
            editor.read_with(cx, |editor, _| editor.workspace.open_documents.len()),
            2
        );
        assert!(editor.read_with(cx, |editor, _| editor.file_path.is_none()));
    });
}

#[gpui::test]
fn quitting_saves_each_untitled_tab_to_its_own_selected_path(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, path| {
        for content in ["第一份", "第二份"] {
            editor.update_in(cx, |editor, window, cx| {
                editor.new_workspace_document(window, cx)
            });
            redraw(cx);
            cx.simulate_input(content);
        }
        editor.update_in(cx, |editor, window, cx| {
            editor.open_workspace_file(path.clone(), window, cx);
            assert!(!editor.on_window_should_close(window, cx));
            editor.on_save_and_close(&gpui::ClickEvent::default(), window, cx);
        });
        for index in 0..2 {
            redraw(cx);
            assert!(
                cx.cx.did_prompt_for_new_path(),
                "每份未命名文档都需要选择保存路径"
            );
            let saved = path.with_file_name(format!("saved-{index}.md"));
            cx.cx.simulate_new_path_selection(|_| Some(saved));
            cx.run_until_parked();
        }
        let mut saved = [0, 1].map(|index| {
            std::fs::read_to_string(path.with_file_name(format!("saved-{index}.md")))
                .expect("正式文件")
        });
        saved.sort();
        assert_eq!(saved, ["第一份", "第二份"]);
        assert!(cx.cx.windows().is_empty());
    });
}

#[gpui::test]
fn closing_an_untitled_tab_prompts_for_a_path_and_preserves_the_window(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, path| {
        editor.update_in(cx, |editor, window, cx| {
            editor.new_workspace_document(window, cx)
        });
        redraw(cx);
        cx.simulate_input("保存后关闭标签");
        editor.update_in(cx, |editor, window, cx| editor.close_active_tab(window, cx));
        editor.update_in(cx, |editor, window, cx| editor.dismiss_modal(0, window, cx));
        cx.run_until_parked();
        assert!(
            cx.cx.did_prompt_for_new_path(),
            "不能把未命名标签身份当成磁盘路径保存"
        );
        let saved = path.with_file_name("closed.md");
        cx.cx.simulate_new_path_selection(|_| Some(saved.clone()));
        cx.run_until_parked();
        redraw(cx);
        assert_eq!(
            std::fs::read_to_string(saved).expect("正式文件"),
            "保存后关闭标签"
        );
        assert_eq!(cx.cx.windows().len(), 1);
        assert_eq!(
            editor.read_with(cx, |editor, _| editor.workspace.open_documents.len()),
            1
        );
        assert_eq!(
            editor.read_with(cx, |editor, _| editor.file_path.clone()),
            Some(path)
        );
    });
}

#[gpui::test]
fn new_document_menu_targets_the_active_editor_group(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, path| {
        editor.update_in(cx, |editor, window, cx| {
            editor.split_active_document(super::super::split::SplitDirection::Right, window, cx)
        });
        redraw(cx);
        let groups = editor.read_with(cx, |editor, _| editor.split_groups());
        cx.update(|window, cx| {
            crate::app_menu::dispatch_menu_action_for_editor(
                &crate::components::NewDocument,
                &editor.downgrade(),
                window,
                cx,
            )
        });
        redraw(cx);
        assert_eq!(cx.cx.windows().len(), 1);
        assert_eq!(
            editor.read_with(cx, |editor, _| editor.split_groups().len()),
            2
        );
        let left = groups.first().expect("左组");
        let right = groups.get(1).expect("活动右组");
        assert_eq!(
            left.read_with(cx, |editor, _| editor.workspace.open_documents.len()),
            1
        );
        assert_eq!(
            left.read_with(cx, |editor, _| editor.file_path.clone()),
            Some(path)
        );
        assert_eq!(
            right.read_with(cx, |editor, _| editor.workspace.open_documents.len()),
            2
        );
        assert!(right.read_with(cx, |editor, _| editor.file_path.is_none()));
    });
}

#[gpui::test]
fn cancelling_an_untitled_tab_save_keeps_it_open_for_a_later_save(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, path| {
        editor.update_in(cx, |editor, window, cx| {
            editor.new_workspace_document(window, cx)
        });
        redraw(cx);
        cx.simulate_input("取消保存的草稿");
        editor.update_in(cx, |editor, window, cx| {
            editor.close_active_tab(window, cx);
            editor.dismiss_modal(0, window, cx);
        });
        cx.run_until_parked();
        cx.cx.simulate_new_path_selection(|_| None);
        cx.run_until_parked();
        assert!(editor.read_with(cx, |editor, _| editor.document_dirty));
        assert_eq!(
            editor.read_with(cx, |editor, _| editor.workspace.open_documents.len()),
            2
        );
        editor.update_in(cx, |editor, window, cx| editor.save_document(window, cx));
        cx.run_until_parked();
        let saved = path.with_file_name("kept.md");
        cx.cx.simulate_new_path_selection(|_| Some(saved.clone()));
        cx.run_until_parked();
        redraw(cx);
        assert_eq!(
            editor.read_with(cx, |editor, _| editor.workspace.open_documents.len()),
            2,
            "取消关闭后普通保存不能偷偷关闭标签"
        );
        assert_eq!(
            std::fs::read_to_string(saved).expect("正式文件"),
            "取消保存的草稿"
        );
    });
}

#[gpui::test]
fn restarting_restores_untitled_tabs_without_opening_duplicate_recovery_windows(
    cx: &mut TestAppContext,
) {
    for without_workspace in [false, true] {
        fixture(cx, |editor, cx, _| {
            for content in ["恢复第一份", "恢复第二份"] {
                editor.update_in(cx, |editor, window, cx| {
                    editor.new_workspace_document(window, cx)
                });
                redraw(cx);
                cx.simulate_input(content);
            }
            editor.update(cx, |editor, cx| {
                if without_workspace {
                    editor.workspace.root = None;
                    editor
                        .workspace
                        .open_documents
                        .retain(|tab| super::super::untitled_document_id(&tab.path).is_some());
                }
                editor.persist_session(cx);
            });
            cx.run_until_parked();
            cx.executor()
                .advance_clock(std::time::Duration::from_secs(2));
            cx.run_until_parked();
            cx.update(|window, _| window.remove_window());
            let restored = cx.cx.update(|cx| {
                assert!(
                    crate::app_menu::restore_last_session(cx),
                    "未命名标签在尚未选择工作区目录时也要恢复"
                );
                crate::restore_recovery_windows(cx, &std::sync::atomic::AtomicBool::new(false));
                assert_eq!(
                    cx.windows().len(),
                    1,
                    "会话与草稿恢复不能把同一文档打开两次"
                );
                crate::ui::window_content::<Editor>(cx.windows().first().expect("恢复窗口"), cx)
                    .expect("编辑器窗口")
            });
            for _ in 0..4 {
                cx.executor()
                    .advance_clock(std::time::Duration::from_millis(16));
                cx.run_until_parked();
            }
            let (drafts, current) = cx.cx.update(|cx| {
                restored
                    .update(cx, |editor, _, _| {
                        let drafts = editor
                            .workspace
                            .open_documents
                            .iter()
                            .filter(|tab| super::super::untitled_document_id(&tab.path).is_some())
                            .map(|tab| tab.markdown.clone())
                            .collect::<Vec<_>>();
                        (drafts, editor.buffer.text())
                    })
                    .expect("恢复后的编辑器")
            });
            assert_eq!(drafts.len(), 2);
            assert!(drafts.iter().any(|text| text == "恢复第一份"));
            assert!(drafts.iter().any(|text| text == "恢复第二份"));
            assert_eq!(current, "恢复第二份", "恢复上次活动标签");
            cx.cx.update(|cx| {
                restored
                    .update(cx, |_, window, _| window.remove_window())
                    .expect("关闭恢复窗口")
            });
        });
    }
}

#[gpui::test]
fn untitled_recovery_uses_the_latest_shared_buffer_when_another_group_hides_it(
    cx: &mut TestAppContext,
) {
    fixture(cx, |editor, cx, path| {
        editor.update_in(cx, |editor, window, cx| {
            editor.new_workspace_document(window, cx)
        });
        redraw(cx);
        cx.simulate_input("共享草稿");
        let id = editor.read_with(cx, |editor, _| editor.recovery_id);
        editor.update_in(cx, |editor, window, cx| {
            editor.split_active_document(super::super::split::SplitDirection::Right, window, cx)
        });
        redraw(cx);
        let groups = editor.read_with(cx, |editor, _| editor.split_groups());
        let left = groups.first().expect("左组").clone();
        groups
            .get(1)
            .expect("右组")
            .update_in(cx, |editor, window, cx| {
                editor.open_workspace_file(path, window, cx)
            });
        editor.update(cx, |editor, cx| {
            editor.activate_split_group(left.entity_id(), cx)
        });
        left.update(cx, |editor, cx| {
            editor.buffer.edit(0..0, "最新");
            editor.document_dirty = true;
            cx.notify();
        });
        redraw(cx);
        cx.executor()
            .advance_clock(std::time::Duration::from_secs(2));
        cx.run_until_parked();
        let snapshots = crate::config::read_recovery_snapshots().expect("草稿");
        assert_eq!(
            snapshots
                .iter()
                .find(|snapshot| snapshot.id == id)
                .expect("共享草稿快照")
                .markdown,
            "最新共享草稿"
        );
    });
}

#[gpui::test]
fn untitled_recovery_does_not_resurrect_a_tab_closed_during_autosave(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, path| {
        editor.update_in(cx, |editor, window, cx| {
            editor.new_workspace_document(window, cx)
        });
        redraw(cx);
        cx.simulate_input("已丢弃草稿");
        let id = editor.read_with(cx, |editor, _| editor.recovery_id);
        editor.update_in(cx, |editor, window, cx| {
            editor.open_workspace_file(path, window, cx)
        });
        editor.update(cx, |editor, _| {
            editor.save_commit_hook = Some(Box::new(move |editor, _| {
                editor
                    .workspace
                    .open_documents
                    .retain(|tab| tab.recovery_id != id);
            }));
        });
        cx.executor()
            .advance_clock(std::time::Duration::from_secs(2));
        cx.run_until_parked();
        assert!(
            !crate::config::read_recovery_snapshots()
                .expect("草稿")
                .iter()
                .any(|snapshot| snapshot.id == id),
            "异步写入完成后也要清理已经关闭的草稿"
        );
    });
}

#[gpui::test]
fn saving_an_untitled_document_renames_the_tab_and_keeps_other_drafts(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, path| {
        editor.update_in(cx, |editor, window, cx| {
            editor.new_workspace_document(window, cx)
        });
        redraw(cx);
        assert_eq!(
            editor.read_with(cx, |editor, _| editor.workspace.open_documents.len()),
            2,
            "必须新建标签"
        );
        cx.simulate_input("需要保存的草稿");
        let recovery_id = editor.read_with(cx, |editor, _| editor.recovery_id);
        editor.update_in(cx, |editor, window, cx| editor.save_document(window, cx));
        cx.run_until_parked();
        assert!(cx.cx.did_prompt_for_new_path(), "首次保存必须选择正式路径");
        let saved = path.with_file_name("saved.md");
        cx.cx.simulate_new_path_selection(|_| Some(saved.clone()));
        cx.run_until_parked();
        redraw(cx);
        editor.read_with(cx, |editor, _| {
            assert_eq!(editor.file_path.as_ref(), Some(&saved));
            assert_eq!(
                editor.workspace.open_documents.len(),
                2,
                "原标签就地转成正式文件"
            );
            assert!(!editor.document_dirty);
        });
        assert_eq!(
            std::fs::read_to_string(saved).expect("正式文件"),
            "需要保存的草稿"
        );
        assert!(
            !crate::config::read_recovery_snapshots()
                .expect("草稿目录")
                .iter()
                .any(|snapshot| snapshot.id == recovery_id)
        );
    });
}
