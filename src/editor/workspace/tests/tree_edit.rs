use super::super::{Editor, OverlayInputKind, WorkspaceMenuAction, WorkspaceSelection};
use gpui::{Entity, EntityInputHandler, Modifiers, MouseButton, TestAppContext, VisualTestContext};
use std::fs;
use std::path::PathBuf;

fn tree_fixture(cx: &mut TestAppContext) -> (Entity<Editor>, &mut VisualTestContext, PathBuf) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let root = std::env::temp_dir().join(format!("velora-tree-name-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).expect("创建目录");
    let root = fs::canonicalize(root).expect("规范化路径");
    cx.on_quit({
        let root = root.clone();
        move || fs::remove_dir_all(root).expect("删除夹具")
    });
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
    editor.update(cx, |editor, cx| editor.set_workspace_root(root.clone(), cx));
    redraw(cx);
    (editor, cx, root)
}

fn redraw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear());
    cx.run_until_parked();
}

fn select_all_key() -> &'static str {
    if cfg!(target_os = "macos") {
        "cmd-a"
    } else {
        "ctrl-a"
    }
}

#[test]
fn tree_names_allow_extensions_and_unicode_but_reject_move_paths() {
    for name in ["笔记.md", "script.py", ".env", "README", "目录 📝"] {
        assert!(super::super::tree_edit::valid_workspace_name(name));
    }
    for name in [
        "",
        "   ",
        ".",
        "..",
        "../note.md",
        "nested/note.md",
        "nested\\note.md",
        "a\0b",
    ] {
        assert!(
            !super::super::tree_edit::valid_workspace_name(name),
            "不能通过名称移动到 {name:?}"
        );
    }
}

#[gpui::test]
async fn generic_file_creation_keeps_the_typed_extension(cx: &mut TestAppContext) {
    let (editor, cx, root) = tree_fixture(cx);
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.create_generic_workspace_file(window, cx)
        })
    });
    redraw(cx);
    cx.simulate_input("设置.json");
    cx.simulate_keystrokes("enter");
    redraw(cx);
    assert_eq!(fs::read(root.join("设置.json")).expect("新建普通文件"), b"");
    assert!(!root.join("设置.json.md").exists());
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(editor.file_path, Some(root.join("设置.json")));
        assert!(editor.code_document);
        assert!(editor.workspace.name_edit.is_none());
    });
}

#[gpui::test]
async fn markdown_creation_selects_the_stem_and_allows_changing_the_suffix(
    cx: &mut TestAppContext,
) {
    let (editor, cx, root) = tree_fixture(cx);
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.prompt_create_workspace_file(window, cx)
        })
    });
    redraw(cx);
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(editor.input_text(OverlayInputKind::TreeName), "untitled.md");
        assert_eq!(editor.input_selection(OverlayInputKind::TreeName), 0..8);
    });
    cx.simulate_input("笔记");
    cx.simulate_keystrokes("enter");
    redraw(cx);
    assert!(root.join("笔记.md").is_file());
    editor.update(cx, |editor, _cx| {
        editor.workspace.selected = Some(WorkspaceSelection::Directory(root.clone()))
    });
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.prompt_create_workspace_file(window, cx)
        })
    });
    redraw(cx);
    cx.simulate_keystrokes(select_all_key());
    cx.simulate_input("main.py");
    cx.simulate_keystrokes("enter");
    redraw(cx);
    assert!(root.join("main.py").is_file());
    assert!(!root.join("main.py.md").exists());
}

#[gpui::test]
async fn tree_name_escape_cancels_without_creating_files(cx: &mut TestAppContext) {
    let (editor, cx, root) = tree_fixture(cx);
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.prompt_create_workspace_file(window, cx)
        })
    });
    redraw(cx);
    cx.simulate_input("取消 📝");
    cx.simulate_keystrokes("escape");
    redraw(cx);
    assert_eq!(fs::read_dir(root).expect("查看目录").count(), 0);
    assert!(editor.read_with(cx, |editor, _cx| editor.workspace.name_edit.is_none()));
}

#[gpui::test]
async fn tree_rename_keeps_unsaved_document_and_changes_only_the_name(cx: &mut TestAppContext) {
    let (editor, cx, root) = tree_fixture(cx);
    let original = root.join("original.md");
    fs::write(&original, "# 磁盘原文\n").expect("写夹具");
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(original.clone(), window, cx);
            editor.replace_document_from_markdown(
                "# 未保存内容\n".into(),
                Some(original.clone()),
                cx,
            );
            editor.document_dirty = true;
            editor.workspace.selected = Some(WorkspaceSelection::File(original.clone()));
            editor.refresh_workspace_tree(cx);
        })
    });
    redraw(cx);
    let unsaved = editor.read_with(cx, |editor, _cx| editor.document_text_for_save());
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| editor.prompt_rename_selected(window, cx))
    });
    redraw(cx);
    assert!(cx.debug_bounds("workspace-name-input").is_some());
    cx.simulate_input("重命名");
    cx.simulate_keystrokes("enter");
    redraw(cx);
    let renamed = root.join("重命名.md");
    assert!(!original.exists());
    assert!(renamed.exists());
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(editor.file_path.as_ref(), Some(&renamed));
        assert_eq!(editor.document_text_for_save(), unsaved);
        assert!(editor.document_dirty);
        assert_eq!(editor.workspace.active_document.as_ref(), Some(&renamed));
        assert!(
            editor
                .workspace
                .open_documents
                .iter()
                .any(|tab| tab.path == renamed)
        );
    });
}

#[gpui::test]
async fn tree_name_collision_and_invalid_names_do_not_overwrite(cx: &mut TestAppContext) {
    let (editor, cx, root) = tree_fixture(cx);
    let existing = root.join("existing.md");
    fs::write(&existing, "保留原文").expect("写既有文件");
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.prompt_create_workspace_file(window, cx)
        })
    });
    redraw(cx);
    cx.simulate_input("existing");
    cx.simulate_keystrokes("enter");
    redraw(cx);
    assert_eq!(fs::read_to_string(&existing).expect("读取文件"), "保留原文");
    assert!(editor.read_with(cx, |editor, _cx| {
        editor
            .workspace
            .name_edit
            .as_ref()
            .is_some_and(|edit| edit.error.is_some() && !edit.pending)
    }));
    cx.simulate_keystrokes(select_all_key());
    cx.simulate_input("../escaped.md");
    cx.simulate_keystrokes("enter");
    redraw(cx);
    assert!(editor.read_with(cx, |editor, _cx| {
        editor
            .workspace
            .name_edit
            .as_ref()
            .is_some_and(|edit| edit.error.is_some())
    }));
    assert_eq!(fs::read_dir(&root).expect("查看目录").count(), 1);
    cx.simulate_keystrokes("escape");
    redraw(cx);
    let source = root.join("source.md");
    fs::write(&source, "待重命名的原文").expect("创建源文件");
    editor.update(cx, |editor, cx| {
        editor.workspace.selected = Some(WorkspaceSelection::File(source.clone()));
        editor.refresh_workspace_tree(cx);
    });
    redraw(cx);
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| editor.prompt_rename_selected(window, cx))
    });
    redraw(cx);
    cx.simulate_input("existing");
    cx.simulate_keystrokes("enter");
    redraw(cx);
    assert_eq!(
        fs::read_to_string(&source).expect("源文件不应消失"),
        "待重命名的原文"
    );
    assert_eq!(
        fs::read_to_string(&existing).expect("目标文件不应被覆盖"),
        "保留原文"
    );
    assert!(editor.read_with(cx, |editor, _cx| {
        editor
            .workspace
            .name_edit
            .as_ref()
            .is_some_and(|edit| edit.error.is_some())
    }));
}

#[gpui::test]
async fn copy_tree_paths_and_filename_uses_the_selected_target(cx: &mut TestAppContext) {
    let (editor, cx, root) = tree_fixture(cx);
    let nested = root.join("子目录");
    fs::create_dir(&nested).expect("创建目录");
    let target = nested.join("笔记.md");
    fs::write(&target, "内容").expect("写文件");
    for (action, expected) in [
        (
            WorkspaceMenuAction::CopyRelativePath,
            PathBuf::from("子目录")
                .join("笔记.md")
                .to_string_lossy()
                .into_owned(),
        ),
        (WorkspaceMenuAction::CopyFileName, "笔记.md".to_string()),
    ] {
        cx.update(|_window, cx| {
            editor.update(cx, |editor, cx| {
                editor.workspace.selected = Some(WorkspaceSelection::File(target.clone()));
                editor.copy_workspace_path_text(action, cx);
            });
            assert_eq!(
                cx.read_from_clipboard().and_then(|item| item.text()),
                Some(expected)
            );
        });
    }
    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| {
            editor.copy_workspace_path_text(WorkspaceMenuAction::CopyAbsolutePath, cx)
        });
        let copied = cx
            .read_from_clipboard()
            .and_then(|item| item.text())
            .expect("绝对路径");
        assert_eq!(
            fs::canonicalize(&copied).expect("复制的路径应可直接使用"),
            target
        );
        #[cfg(windows)]
        assert!(
            !copied.starts_with(r"\\?\"),
            "复制路径不应带 Windows 内部设备前缀"
        );
    });
}

#[gpui::test]
async fn composing_a_chinese_tree_name_does_not_confirm_until_unmarked(cx: &mut TestAppContext) {
    let (editor, cx, root) = tree_fixture(cx);
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.prompt_create_workspace_file(window, cx)
        })
    });
    redraw(cx);
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.replace_and_mark_text_in_range(None, "中文", None, window, cx);
            editor.confirm_workspace_name_edit(window, cx);
        })
    });
    redraw(cx);
    assert_eq!(fs::read_dir(&root).expect("查看目录").count(), 0);
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.unmark_text(window, cx);
            editor.confirm_workspace_name_edit(window, cx);
        })
    });
    redraw(cx);
    assert!(root.join("中文.md").is_file());
}

#[gpui::test]
async fn folders_are_created_and_renamed_inline_without_losing_open_children(
    cx: &mut TestAppContext,
) {
    let (editor, cx, root) = tree_fixture(cx);
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.prompt_create_workspace_folder(window, cx)
        })
    });
    redraw(cx);
    cx.simulate_input("资料");
    cx.simulate_keystrokes("enter");
    redraw(cx);
    let old_folder = root.join("资料");
    assert!(old_folder.is_dir());
    let document = old_folder.join("note.md");
    fs::write(&document, "原文").expect("创建子文档");
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(document, window, cx);
            editor.workspace.selected = Some(WorkspaceSelection::Directory(old_folder.clone()));
            editor.refresh_workspace_tree(cx);
        })
    });
    redraw(cx);
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| editor.prompt_rename_selected(window, cx))
    });
    redraw(cx);
    cx.simulate_input("归档");
    cx.simulate_keystrokes("enter");
    redraw(cx);
    let new_folder = root.join("归档");
    assert!(!old_folder.exists());
    assert_eq!(
        fs::read_to_string(new_folder.join("note.md")).expect("读取已重命名目录"),
        "原文"
    );
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(editor.file_path, Some(new_folder.join("note.md")));
        assert!(
            editor
                .workspace
                .open_documents
                .iter()
                .any(|tab| tab.path == new_folder.join("note.md"))
        );
    });
}

#[gpui::test]
async fn right_click_selects_the_target_and_menu_actions_keep_their_target(
    cx: &mut TestAppContext,
) {
    let (editor, cx, root) = tree_fixture(cx);
    let active = root.join("active.md");
    let target = root.join("target.unknown");
    fs::write(&active, "打开的文档").expect("创建活动文档");
    fs::write(&target, "目标").expect("创建目标文件");
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(active.clone(), window, cx);
            editor.refresh_workspace_tree(cx);
        })
    });
    redraw(cx);
    let selector = format!(
        "workspace-node-{}",
        super::super::stable_node_hash(&super::super::file_node_id(&target))
    );
    let selector: &'static str = Box::leak(selector.into_boxed_str());
    let row = cx.debug_bounds(selector).expect("目标文件行");
    cx.simulate_mouse_down(row.center(), MouseButton::Right, Modifiers::none());
    cx.simulate_mouse_up(row.center(), MouseButton::Right, Modifiers::none());
    redraw(cx);
    cx.update(|window, cx| {
        editor.read_with(cx, |editor, _cx| {
            assert_eq!(editor.selected_workspace_path(), Some(target.clone()));
            assert_eq!(editor.file_path.as_ref(), Some(&active));
            assert!(
                editor
                    .workspace
                    .tree_focus
                    .as_ref()
                    .is_some_and(|focus| focus.is_focused(window))
            );
        })
    });
    // 异步激活文档或其它状态更新不能改变已经打开的菜单所绑定的文件。
    editor.update(cx, |editor, _cx| {
        editor.workspace.selected = Some(WorkspaceSelection::File(active.clone()))
    });
    redraw(cx);
    let copy_name = cx
        .debug_bounds("menu-item-workspace-context-action-6")
        .expect("复制文件名菜单项");
    cx.simulate_click(copy_name.center(), Modifiers::none());
    redraw(cx);
    cx.update(|_window, cx| {
        assert_eq!(
            cx.read_from_clipboard().and_then(|item| item.text()),
            Some("target.unknown".to_string())
        );
    });
    assert_eq!(
        editor.read_with(cx, |editor, _cx| editor.selected_workspace_path()),
        Some(target)
    );
    cx.simulate_keystrokes("f2");
    redraw(cx);
    assert!(
        cx.debug_bounds("workspace-name-input").is_some(),
        "文件树焦点下 F2 应直接重命名"
    );
    cx.simulate_keystrokes("escape");
    redraw(cx);
    assert!(editor.read_with(cx, |editor, _cx| editor.workspace.name_edit.is_none()));
}

#[test]
fn tree_creation_and_rename_never_open_native_path_dialogs() {
    for source in [
        include_str!("../prompts.rs"),
        include_str!("../tree_edit.rs"),
    ] {
        assert!(
            !source.contains("prompt_for_new_path"),
            "文件树新建和重命名必须在树内输入"
        );
    }
}

#[gpui::test]
async fn markdown_creation_opens_an_input_inside_the_tree(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let root = std::env::temp_dir().join(format!("velora-tree-edit-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).expect("创建夹具目录");
    let root = fs::canonicalize(root).expect("规范化夹具路径");
    cx.on_quit({
        let root = root.clone();
        move || fs::remove_dir_all(root).expect("删除夹具目录")
    });
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
    editor.update(cx, |editor, cx| {
        editor.set_workspace_root(root.clone(), cx);
        editor.workspace.selected = Some(WorkspaceSelection::Directory(root.clone()));
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.prompt_create_workspace_file(window, cx)
        });
    });
    cx.update(|window, cx| window.draw(cx).clear());
    assert!(
        cx.debug_bounds("workspace-name-input").is_some(),
        "新建 Markdown 应在文件树内显示名称输入框"
    );
    assert_eq!(
        fs::read_dir(&root).expect("查看夹具目录").count(),
        0,
        "确认名称之前不创建磁盘文件"
    );
}
