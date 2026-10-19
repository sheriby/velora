use super::super::{Editor, WorkspaceMenuAction, WorkspaceSelection};
use crate::components::UndoCaptureKind;
use gpui::{Entity, EntityInputHandler, Modifiers, MouseButton, TestAppContext, VisualTestContext};
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

fn tree_fixture(cx: &mut TestAppContext) -> (Entity<Editor>, &mut VisualTestContext, PathBuf) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let root = std::env::temp_dir().join(format!("velora-tree-name-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).expect("创建目录");
    let root = crate::file_url::canonical_path(root);
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
    cx.update(|window, cx| window.draw(cx).clear(cx));
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
    editor.read_with(cx, |editor, cx| {
        let input = editor.workspace.name_edit.as_ref().unwrap().input.read(cx);
        assert_eq!(input.value(), "untitled.md");
        assert_eq!(input.selected_range(), 0..8);
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
fn empty_tree_names_cancel_on_blur_and_keep_the_clicked_documents_focus(cx: &mut TestAppContext) {
    let (editor, cx, root) = tree_fixture(cx);
    editor.update(cx, |editor, cx| {
        editor.show_welcome = false;
        editor.replace_document_from_markdown("点击这段正文\n".into(), None, cx);
    });
    cx.update(|window, _| window.activate_window());
    redraw(cx);
    let mut uncancelled = Vec::new();
    for action in [
        WorkspaceMenuAction::NewFile,
        WorkspaceMenuAction::NewMarkdown,
        WorkspaceMenuAction::NewFolder,
    ] {
        for draft in ["", "   "] {
            cx.update(|window, cx| {
                editor.update(cx, |editor, cx| match action {
                    WorkspaceMenuAction::NewFile => {
                        editor.create_generic_workspace_file(window, cx)
                    }
                    WorkspaceMenuAction::NewMarkdown => {
                        editor.prompt_create_workspace_file(window, cx)
                    }
                    WorkspaceMenuAction::NewFolder => {
                        editor.prompt_create_workspace_folder(window, cx)
                    }
                    _ => unreachable!(),
                });
            });
            redraw(cx);
            cx.simulate_keystrokes(select_all_key());
            cx.simulate_keystrokes("backspace");
            if !draft.is_empty() {
                cx.simulate_input(draft);
            }
            redraw(cx);
            let body = editor.read_with(cx, |editor, cx| {
                editor
                    .document
                    .visible_blocks()
                    .first()
                    .expect("正文")
                    .entity
                    .read(cx)
                    .last_bounds
                    .expect("正文几何")
            });
            cx.simulate_click(body.center(), Modifiers::none());
            redraw(cx);
            if editor.read_with(cx, |editor, _| editor.workspace.name_edit.is_some()) {
                uncancelled.push(format!("{action:?}/{draft:?}"));
            }
            cx.update(|window, cx| {
                editor.read_with(cx, |editor, cx| {
                    assert!(
                        editor
                            .document
                            .visible_blocks()
                            .first()
                            .expect("正文")
                            .entity
                            .read(cx)
                            .focus_handle
                            .is_focused(window),
                        "取消空草稿不能抢走正文焦点"
                    );
                });
                editor.update(cx, |editor, cx| {
                    editor.cancel_workspace_name_edit(window, cx)
                });
            });
            redraw(cx);
        }
    }
    assert_eq!(
        fs::read_dir(root).expect("查看目录").count(),
        0,
        "取消不能创建磁盘条目"
    );
    assert!(
        uncancelled.is_empty(),
        "空名称失焦后仍残留输入框或红字：{uncancelled:?}"
    );
}

#[gpui::test]
fn valid_tree_names_still_confirm_on_blur(cx: &mut TestAppContext) {
    let (editor, cx, root) = tree_fixture(cx);
    cx.update(|window, cx| {
        window.activate_window();
        editor.update(cx, |editor, cx| {
            editor.create_generic_workspace_file(window, cx)
        });
    });
    redraw(cx);
    cx.simulate_input("新笔记.md");
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| editor.focus_workspace_tree(window, cx));
    });
    redraw(cx);
    assert!(
        root.join("新笔记.md").is_file(),
        "有效名称失焦后仍应完成创建"
    );
    assert!(editor.read_with(cx, |editor, _| editor.workspace.name_edit.is_none()));
}

#[gpui::test]
fn empty_tree_names_keep_validation_on_enter(cx: &mut TestAppContext) {
    let (editor, cx, root) = tree_fixture(cx);
    cx.update(|window, cx| {
        window.activate_window();
        editor.update(cx, |editor, cx| {
            editor.create_generic_workspace_file(window, cx)
        });
    });
    redraw(cx);
    cx.simulate_keystrokes("enter");
    redraw(cx);
    assert!(
        editor.read_with(cx, |editor, _| editor
            .workspace
            .name_edit
            .as_ref()
            .is_some_and(|edit| edit.error.is_some() && !edit.pending)),
        "主动提交空名称仍应提示校验错误"
    );
    assert_eq!(fs::read_dir(root).expect("查看目录").count(), 0);
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

/// 重命名「正在编辑的文档」之后的落点必须是原样新路径：`remap_moved_path`
/// 走 `destination.join("")` 时 Rust 会补一个尾分隔符（`…/new.md/`），此后
/// 自动保存的外部修改检查读这个路径直接 ENOTDIR（os error 20），冲突弹窗
/// 弹起、`file_error` 置位，整个文件树换成「无法读取工作区」。
#[gpui::test]
async fn renaming_the_open_document_keeps_its_path_readable(cx: &mut TestAppContext) {
    let (editor, cx, root) = tree_fixture(cx);
    let original = root.join("original.md");
    fs::write(&original, "# 磁盘原文\n").expect("写夹具");
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(original.clone(), window, cx);
            editor.workspace.selected = Some(WorkspaceSelection::File(original.clone()));
            editor.refresh_workspace_tree(cx);
        });
    });
    redraw(cx);
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| editor.prompt_rename_selected(window, cx))
    });
    redraw(cx);
    cx.simulate_input("重命名");
    cx.simulate_keystrokes("enter");
    redraw(cx);
    let renamed = root.join("重命名.md");
    assert!(renamed.exists(), "夹具：改名应已落盘");

    // 重命名之后再编辑（报修的动作序）：等自动保存跑完一轮。
    editor.update(cx, |editor, cx| {
        let block = editor.document.root_blocks()[0].clone();
        block.update(cx, |block, cx| {
            block.prepare_undo_capture(UndoCaptureKind::CoalescibleText, cx);
            block.replace_text_in_visible_range(0..0, "abs", None, false, cx);
        });
    });
    cx.executor().advance_clock(Duration::from_secs(2));
    cx.run_until_parked();

    editor.read_with(cx, |editor, _cx| {
        assert!(
            !editor.has_external_autosave_conflict(),
            "改名后的路径必须能读盘，自动保存不得被误判成外部修改"
        );
        assert!(
            editor.workspace.file_error.is_none(),
            "文件树不得进错误态：{:?}",
            editor.workspace.file_error
        );
    });
    assert!(
        fs::read_to_string(&renamed)
            .expect("读取自动保存结果")
            .contains("abs"),
        "自动保存应把编辑落到改名后的文件"
    );
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
            crate::file_url::canonical_path(&copied),
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
    use gpui_kit::test::TestWindowExt as _;
    let (editor, cx, root) = tree_fixture(cx);
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.prompt_create_workspace_file(window, cx)
        })
    });
    redraw(cx);
    let input = editor.read_with(cx, |editor, _| {
        editor.workspace.name_edit.as_ref().unwrap().input.clone()
    });
    input.update_in(cx, |input, window, cx| {
        input.replace_and_mark_text_in_range(None, "中文", None, window, cx)
    });
    cx.update(|window, cx| window.press("enter", cx));
    redraw(cx);
    assert_eq!(fs::read_dir(&root).expect("查看目录").count(), 0);
    input.update_in(cx, |input, window, cx| input.unmark_text(window, cx));
    cx.update(|window, cx| window.press("enter", cx));
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
        .debug_bounds("menu-item-workspace-context-action-CopyFileName")
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
    let root = crate::file_url::canonical_path(root);
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
    cx.update(|window, cx| window.draw(cx).clear(cx));
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

#[gpui::test]
fn tree_creation_context_menus_only_offer_actions_for_the_clicked_target(cx: &mut TestAppContext) {
    let (editor, cx, root) = tree_fixture(cx);
    let directory = root.join("目录");
    fs::create_dir(&directory).expect("创建目录");
    let file = directory.join("note.md");
    fs::write(&file, "正文").expect("写夹具");
    editor.update(cx, |editor, cx| {
        editor.workspace.selected = Some(WorkspaceSelection::File(file.clone()));
        editor.copy_selected_workspace_file(cx);
    });
    let creation = ["NewFile", "NewMarkdown", "NewFolder", "Paste"];
    let paths = [
        "Reveal",
        "CopyAbsolutePath",
        "CopyRelativePath",
        "CopyFileName",
    ];
    let mut file_actions = paths.to_vec();
    file_actions.extend(["Copy", "Rename", "Duplicate", "Delete"]);
    let mut directory_actions = creation.to_vec();
    directory_actions.extend(paths);
    directory_actions.extend(["Rename", "Delete"]);
    let mut root_actions = creation.to_vec();
    root_actions.extend(paths);
    let actions = [
        "NewFile",
        "NewMarkdown",
        "NewFolder",
        "Reveal",
        "CopyAbsolutePath",
        "CopyRelativePath",
        "CopyFileName",
        "Copy",
        "Paste",
        "Rename",
        "Duplicate",
        "Delete",
    ];
    let mut mismatches = Vec::new();
    for (label, selection, expected) in [
        (
            "文件",
            Some(WorkspaceSelection::File(file.clone())),
            file_actions,
        ),
        (
            "目录",
            Some(WorkspaceSelection::Directory(directory)),
            directory_actions,
        ),
        (
            "根目录",
            Some(WorkspaceSelection::Directory(root)),
            root_actions,
        ),
        ("空白", None, creation.to_vec()),
    ] {
        editor.update(cx, |editor, cx| {
            editor.workspace.selected = Some(WorkspaceSelection::File(file.clone()));
            editor.open_workspace_context_menu(
                gpui::point(gpui::px(60.0), gpui::px(80.0)),
                selection,
                cx,
            );
        });
        redraw(cx);
        for action in actions {
            let selector =
                Box::leak(format!("menu-item-workspace-context-action-{action}").into_boxed_str());
            if cx.debug_bounds(selector).is_some() != expected.contains(&action) {
                mismatches.push(format!("{label}: {action}"));
            }
        }
    }
    assert!(
        mismatches.is_empty(),
        "右键菜单包含不适用的操作或缺少操作：{mismatches:?}"
    );
}

#[gpui::test]
fn tree_paste_menu_only_shows_for_current_pasteable_content(cx: &mut TestAppContext) {
    let (editor, cx, root) = tree_fixture(cx);
    let source = root.join("source.md");
    let directory = root.join("目标目录");
    fs::create_dir(&directory).expect("创建目标目录");
    let mut mismatches = Vec::new();
    for (label, clipboard, copy_file, delete_source, expected) in [
        ("空剪贴板", None, false, false, false),
        (
            "空文本",
            Some(gpui::ClipboardItem::new_string(String::new())),
            false,
            false,
            false,
        ),
        (
            "普通文字",
            Some(gpui::ClipboardItem::new_string("文字".into())),
            false,
            false,
            false,
        ),
        ("已复制文件", None, true, false, true),
        (
            "复制后改为文字",
            Some(gpui::ClipboardItem::new_string("别的文字".into())),
            true,
            false,
            false,
        ),
        (
            "复制后清空",
            Some(gpui::ClipboardItem::new_string(String::new())),
            true,
            false,
            false,
        ),
        ("源文件已删除", None, true, true, false),
        (
            "空图片",
            Some(gpui::ClipboardItem::new_image(&gpui::Image::empty())),
            false,
            false,
            false,
        ),
    ] {
        fs::write(&source, "源文件").expect("写夹具");
        editor.update(cx, |editor, cx| {
            editor.tree_clipboard = None;
            editor.workspace.selected = Some(WorkspaceSelection::File(source.clone()));
            if copy_file {
                editor.copy_selected_workspace_file(cx);
            }
            if let Some(clipboard) = clipboard {
                cx.write_to_clipboard(clipboard);
            }
        });
        if delete_source {
            fs::remove_file(&source).expect("删除已复制的文件");
        }
        for selection in [Some(WorkspaceSelection::Directory(directory.clone())), None] {
            editor.update(cx, |editor, cx| {
                editor.open_workspace_context_menu(
                    gpui::point(gpui::px(60.0), gpui::px(80.0)),
                    selection.clone(),
                    cx,
                );
            });
            redraw(cx);
            let visible = cx
                .debug_bounds("menu-item-workspace-context-action-Paste")
                .is_some();
            if visible != expected {
                mismatches.push(format!("{label}/{selection:?}"));
            }
        }
    }
    assert!(
        mismatches.is_empty(),
        "粘贴入口没有反映剪贴板内容：{mismatches:?}"
    );
}

#[gpui::test]
fn tree_paste_does_not_use_a_file_after_the_clipboard_is_replaced(cx: &mut TestAppContext) {
    let (editor, cx, root) = tree_fixture(cx);
    let source = root.join("source.md");
    let directory = root.join("目标目录");
    fs::write(&source, "源文件").expect("写夹具");
    fs::create_dir(&directory).expect("创建目标目录");
    editor.update(cx, |editor, cx| {
        editor.workspace.selected = Some(WorkspaceSelection::File(source));
        editor.copy_selected_workspace_file(cx);
        cx.write_to_clipboard(gpui::ClipboardItem::new_string("普通文字".into()));
        editor.workspace.selected = Some(WorkspaceSelection::Directory(directory.clone()));
        editor.paste_into_workspace_tree(cx);
    });
    assert_eq!(
        fs::read_dir(directory).expect("读取目标目录").count(),
        0,
        "不能粘贴已被普通文字取代的旧文件"
    );
    assert!(
        editor.read_with(cx, |editor, _| editor.workspace.file_error.is_none()),
        "没有可粘贴内容不应让文件树进入错误态"
    );
}

#[gpui::test]
fn tree_paste_menu_uses_the_current_image_instead_of_a_previously_copied_file(
    cx: &mut TestAppContext,
) {
    let (editor, cx, root) = tree_fixture(cx);
    let source = root.join("source.md");
    let directory = root.join("目标目录");
    fs::write(&source, "源文件").expect("写夹具");
    fs::create_dir(&directory).expect("创建目标目录");
    let image = gpui::Image::from_bytes(gpui::ImageFormat::Png, vec![0x89, b'P', b'N', b'G']);
    editor.update(cx, |editor, cx| {
        editor.workspace.selected = Some(WorkspaceSelection::File(source));
        editor.copy_selected_workspace_file(cx);
        cx.write_to_clipboard(gpui::ClipboardItem::new_image(&image));
        editor.open_workspace_context_menu(
            gpui::point(gpui::px(60.0), gpui::px(80.0)),
            Some(WorkspaceSelection::Directory(directory.clone())),
            cx,
        );
    });
    redraw(cx);
    let paste = cx
        .debug_bounds("menu-item-workspace-context-action-Paste")
        .expect("有图片时应显示粘贴");
    cx.simulate_click(paste.center(), Modifiers::none());
    redraw(cx);
    let entries = fs::read_dir(&directory)
        .expect("读取粘贴目录")
        .map(|entry| entry.expect("目录条目").path())
        .collect::<Vec<_>>();
    assert_eq!(entries.len(), 1, "应只粘贴当前图片");
    let pasted = entries.first().expect("粘贴产物");
    assert_eq!(
        pasted.extension().and_then(|extension| extension.to_str()),
        Some("png"),
        "不能把旧文件当作当前图片粘贴"
    );
    assert_eq!(fs::read(pasted).expect("读取图片"), image.bytes);
}

#[gpui::test]
fn tree_creation_inputs_appear_after_subfolders_and_before_the_first_file(cx: &mut TestAppContext) {
    let (editor, cx, root) = tree_fixture(cx);
    let directory = root.join("目录-a");
    let nested = directory.join("子目录");
    fs::create_dir_all(&nested).expect("创建嵌套目录");
    fs::create_dir(root.join("目录-b")).expect("创建另一个目录");
    fs::write(nested.join("child.md"), "正文").expect("写嵌套文件");
    for parent in [&root, &directory] {
        for name in ["first.md", "second.md"] {
            fs::write(parent.join(name), "正文").expect("写夹具");
        }
    }
    editor.update(cx, |editor, cx| {
        editor
            .workspace
            .expanded
            .insert(super::super::file_node_id(&directory));
        editor
            .workspace
            .expanded
            .insert(super::super::file_node_id(&nested));
        editor.refresh_workspace_tree(cx);
    });
    redraw(cx);
    redraw(cx);
    let mut misplaced = Vec::new();
    for (selection, parent) in [
        (None, root.clone()),
        (
            Some(WorkspaceSelection::Directory(directory.clone())),
            directory.clone(),
        ),
    ] {
        for action in ["NewFile", "NewMarkdown", "NewFolder"] {
            editor.update(cx, |editor, cx| {
                editor.workspace.selected =
                    Some(WorkspaceSelection::File(directory.join("second.md")));
                editor.open_workspace_context_menu(
                    gpui::point(gpui::px(60.0), gpui::px(80.0)),
                    selection.clone(),
                    cx,
                );
            });
            redraw(cx);
            let selector =
                Box::leak(format!("menu-item-workspace-context-action-{action}").into_boxed_str());
            let item = cx.debug_bounds(selector).expect("新建菜单项");
            cx.simulate_click(item.center(), Modifiers::none());
            redraw(cx);
            editor.read_with(cx, |editor, _| {
                use super::super::render_panel::WorkspaceTreeRow;
                assert_eq!(
                    editor
                        .workspace
                        .name_edit
                        .as_ref()
                        .expect("名称编辑")
                        .directory,
                    parent
                );
                let rows = editor.workspace_files_rows();
                let input = rows
                    .iter()
                    .position(|row| matches!(row, WorkspaceTreeRow::NameEdit(_)))
                    .expect("输入行");
                let first_file = rows
                    .iter()
                    .position(|row| {
                        matches!(row, WorkspaceTreeRow::Node(node, _) if !node.kind_dir()
                        && super::super::tree_node_path(node).parent() == Some(parent.as_path()))
                    })
                    .expect("目录中的第一个文件");
                if input + 1 != first_file {
                    misplaced.push(format!("{}: {action}", parent.display()));
                }
            });
            assert!(
                cx.debug_bounds("workspace-name-input").is_some(),
                "名称输入框应可见"
            );
            cx.update(|window, cx| {
                editor.update(cx, |editor, cx| {
                    editor.cancel_workspace_name_edit(window, cx)
                });
            });
            redraw(cx);
        }
    }
    assert!(
        misplaced.is_empty(),
        "输入框应位于子目录之后、第一个文件之前：{misplaced:?}"
    );
}

#[gpui::test]
fn tree_creation_input_follows_all_folders_when_there_are_no_files(cx: &mut TestAppContext) {
    let (editor, cx, root) = tree_fixture(cx);
    for name in ["目录-a", "目录-b"] {
        fs::create_dir(root.join(name)).expect("创建子目录");
    }
    editor.update(cx, |editor, cx| editor.refresh_workspace_tree(cx));
    redraw(cx);
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.prompt_create_workspace_file(window, cx)
        });
    });
    redraw(cx);
    editor.read_with(cx, |editor, _| {
        use super::super::render_panel::WorkspaceTreeRow;
        let rows = editor.workspace_files_rows();
        assert!(
            matches!(rows.last(), Some(WorkspaceTreeRow::NameEdit(1))),
            "没有文件时输入框应在所有子目录之后"
        );
    });
}

#[gpui::test]
fn tree_creation_input_is_visible_after_large_directories_load(cx: &mut TestAppContext) {
    let (editor, cx, root) = tree_fixture(cx);
    let directory = root.join("目录-200");
    for index in 0..240 {
        fs::create_dir_all(root.join(format!("目录-{index:03}"))).expect("创建根目录夹具");
        fs::create_dir_all(directory.join(format!("子目录-{index:03}"))).expect("创建子目录夹具");
    }
    editor.update(cx, |editor, cx| editor.refresh_workspace_tree(cx));
    cx.update(|window, _| window.resize(gpui::size(gpui::px(800.0), gpui::px(320.0))));
    redraw(cx);
    let mut hidden = Vec::new();
    for parent in [&root, &directory] {
        editor.update(cx, |editor, _| {
            editor.workspace.selected = Some(WorkspaceSelection::Directory(parent.clone()));
        });
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.prompt_create_workspace_file(window, cx)
            });
        });
        redraw(cx);
        redraw(cx);
        let viewport =
            editor.read_with(cx, |editor, _| editor.workspace.tree_scroll_handle.bounds());
        match cx.debug_bounds("workspace-name-input") {
            Some(input) if viewport.contains(&input.center()) => {}
            _ => hidden.push(parent.clone()),
        }
        cx.simulate_input("新笔记");
        if hidden.is_empty() {
            editor.read_with(cx, |editor, cx| {
                assert_eq!(
                    editor.workspace.name_edit.as_ref().unwrap().input.read(cx).value(),
                    "新笔记.md"
                );
            });
        }
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.cancel_workspace_name_edit(window, cx)
            });
        });
        redraw(cx);
    }
    assert!(
        hidden.is_empty(),
        "长目录加载后名称输入框不可见：{hidden:?}"
    );
}

/// 文件树每帧只该建视口那一窗行。
///
/// 现象：树大一点（1500 个文件的面板，本机实测一帧建满整棵树 ≈ 200 ms）点一下文件要等
/// 两三帧才出结果，看起来就是「点的特别顿、有时候像完全没反应」「鼠标一悬停就卡」。
/// 根因：`render_workspace_nodes` 每帧把整棵树都走成元素；大纲面板早有按视口开窗
/// （当时使用手动开窗），文件树未接入虚拟化。
#[gpui::test]
async fn the_file_tree_renders_only_the_rows_in_the_viewport(cx: &mut TestAppContext) {
    let (editor, cx, root) = tree_fixture(cx);
    for index in 0..1500 {
        fs::write(root.join(format!("file-{index:04}.md")), "# 标题\n").expect("写夹具");
    }
    editor.update(cx, |editor, cx| editor.refresh_workspace_tree(cx));
    redraw(cx);
    redraw(cx);

    let (rows, total) = editor.read_with(cx, |editor, _| {
        (
            editor.panel_rows_rendered.get(),
            editor
                .workspace
                .file_tree
                .as_ref()
                .map_or(0, |tree| tree.children.len())
                + 1,
        )
    });
    assert_eq!(total, 1501, "前置：树里该有 1500 个文件加根那一行");
    assert!(rows > 0, "文件树一帧都没渲染，闸门测不到东西");
    assert!(
        rows <= 200,
        "一帧建了 {rows} 行文件树元素（共 {total} 行）：侧栏没有按视口裁剪"
    );

    // 滚动之后窗口要跟着走：滚到第 1000 行附近，那一行得落在窗口里。
    editor.update(cx, |editor, _| {
        editor
            .workspace
            .tree_scroll_handle
            // 负值才是真滚轮会产生的偏移（gpui 的内容偏移是已滚过距离的相反数）。
            .set_offset(gpui::point(gpui::px(0.0), gpui::px(-(6.0 + 1000.0 * 24.0))));
    });
    redraw(cx);
    let (first, rows) = editor.read_with(cx, |editor, _| {
        (
            editor.panel_first_row_rendered.get(),
            editor.panel_rows_rendered.get(),
        )
    });
    assert!(
        (first as usize) <= 1000 && 1000 < first as usize + rows as usize,
        "滚到第 1000 行后窗口是 {first}..{}：窗口没跟着滚动走",
        first as usize + rows as usize
    );
}

#[gpui::test]
fn workspace_context_menu_retargets_on_each_right_click(cx: &mut TestAppContext) {
    let (editor, cx, root) = tree_fixture(cx);
    let first = root.join("z.md");
    let second = root.join("a.md");
    let directory = root.join("目录");
    fs::create_dir(&directory).expect("创建目录");
    fs::write(&first, "正文 A").expect("写文件 A");
    fs::write(&second, "正文 B").expect("写文件 B");
    cx.update(|window, cx| {
        window.resize(gpui::size(gpui::px(900.0), gpui::px(650.0)));
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(first.clone(), window, cx);
            editor.refresh_workspace_tree(cx);
        });
    });
    redraw(cx);
    let row_position = |cx: &mut VisualTestContext, path: &PathBuf| {
        let selector = Box::leak(
            format!(
                "workspace-node-{}",
                super::super::stable_node_hash(&super::super::file_node_id(path))
            )
            .into_boxed_str(),
        );
        let bounds = cx.debug_bounds(selector).expect("文件树节点");
        gpui::point(bounds.left() + gpui::px(8.0), bounds.center().y)
    };
    let right_click = |cx: &mut VisualTestContext, position| {
        if let Some(menu) = cx.debug_bounds("workspace-context-panel") {
            assert!(!menu.contains(&position), "点击目标必须露在菜单面板外");
        }
        cx.simulate_mouse_down(position, MouseButton::Right, Modifiers::none());
        cx.simulate_mouse_up(position, MouseButton::Right, Modifiers::none());
        redraw(cx);
    };
    let mut position = row_position(cx, &first);
    position.x += gpui::px(100.0);
    right_click(cx, position);
    let position = row_position(cx, &second);
    right_click(cx, position);
    let mut failures = Vec::new();
    if editor.read_with(cx, |editor, _| {
        editor
            .workspace
            .context_menu
            .as_ref()
            .and_then(|menu| menu.target.clone())
    }) != Some(WorkspaceSelection::File(second.clone()))
    {
        failures.push("文件 A → 文件 B");
    }
    let copy_name = cx
        .debug_bounds("menu-item-workspace-context-action-CopyFileName")
        .expect("复制文件名");
    cx.simulate_click(copy_name.center(), Modifiers::none());
    redraw(cx);
    cx.update(|_, cx| {
        if cx
            .read_from_clipboard()
            .and_then(|item| item.text())
            .as_deref()
            != Some("a.md")
        {
            failures.push("菜单操作仍指向旧文件");
        }
    });
    let mut position = row_position(cx, &first);
    position.x += gpui::px(100.0);
    right_click(cx, position);
    let position = row_position(cx, &directory);
    right_click(cx, position);
    if editor.read_with(cx, |editor, _| {
        editor
            .workspace
            .context_menu
            .as_ref()
            .and_then(|menu| menu.target.clone())
    }) != Some(WorkspaceSelection::Directory(directory))
        || cx
            .debug_bounds("menu-item-workspace-context-action-NewFile")
            .is_none()
    {
        failures.push("文件 → 目录");
    }
    let panel = cx.debug_bounds("workspace-panel").expect("侧栏");
    right_click(
        cx,
        gpui::point(
            panel.left() + gpui::px(12.0),
            panel.bottom() - gpui::px(10.0),
        ),
    );
    if editor.read_with(cx, |editor, _| {
        editor
            .workspace
            .context_menu
            .as_ref()
            .and_then(|menu| menu.target.clone())
    }) != Some(WorkspaceSelection::WorkspaceRoot(root))
        || cx
            .debug_bounds("menu-item-workspace-context-action-CopyFileName")
            .is_some()
    {
        failures.push("目录 → 空白处");
    }
    assert_eq!(
        editor.read_with(cx, |editor, _| editor.file_path.clone()),
        Some(first),
        "右键不能切换正文"
    );
    assert!(failures.is_empty(), "连续右键没有更新目标：{failures:?}");
}

#[gpui::test]
fn workspace_context_menu_left_dismiss_does_not_click_the_file_underneath(cx: &mut TestAppContext) {
    let (editor, cx, root) = tree_fixture(cx);
    let first = root.join("z.md");
    let second = root.join("a.md");
    for path in [&first, &second] {
        fs::write(path, "正文").expect("写夹具");
    }
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(first.clone(), window, cx);
            editor.refresh_workspace_tree(cx);
        });
    });
    redraw(cx);
    let row_position = |cx: &mut VisualTestContext, path: &PathBuf| {
        let selector = Box::leak(
            format!(
                "workspace-node-{}",
                super::super::stable_node_hash(&super::super::file_node_id(path))
            )
            .into_boxed_str(),
        );
        let bounds = cx.debug_bounds(selector).expect("文件行");
        gpui::point(bounds.left() + gpui::px(8.0), bounds.center().y)
    };
    let mut position = row_position(cx, &first);
    position.x += gpui::px(100.0);
    cx.simulate_mouse_down(position, MouseButton::Right, Modifiers::none());
    cx.simulate_mouse_up(position, MouseButton::Right, Modifiers::none());
    redraw(cx);
    let position = row_position(cx, &second);
    assert!(
        !cx.debug_bounds("workspace-context-panel")
            .expect("菜单")
            .contains(&position)
    );
    cx.simulate_click(position, Modifiers::none());
    redraw(cx);
    editor.read_with(cx, |editor, _| {
        assert!(editor.workspace.context_menu.is_none(), "左键应关闭菜单");
        assert_eq!(
            editor.file_path,
            Some(first),
            "关闭菜单不能顺便打开下方文件"
        );
    });
}

/// 菜单开着时，鼠标压在菜单面板上，面板底下那一行文件不该还算悬停。
///
/// 现象：右键菜单弹出来以后，鼠标在菜单上面移动，菜单下面的文件跟着亮
/// 悬停底色。根因：菜单面板没有 `occlude`，GPUI 的命中测试
/// （`Window::hit_test`）只在遇到 `HitboxBehavior::BlockMouse` 时才停，于是菜单底下那一
/// 层的行也进了命中集合，`is_hovered` 跟着为真（点击也会一起漏过去）。
#[gpui::test]
async fn the_workspace_context_menu_stops_hover_at_its_own_surface(cx: &mut TestAppContext) {
    let (editor, cx, root) = tree_fixture(cx);
    for name in ["a.md", "b.md", "c.md"] {
        fs::write(root.join(name), "# 标题\n\n正文\n").expect("写夹具");
    }
    editor.update(cx, |editor, cx| editor.refresh_workspace_tree(cx));
    redraw(cx);

    let row_selector = |path: &PathBuf| -> &'static str {
        let selector = format!(
            "workspace-node-{}",
            super::super::stable_node_hash(&super::super::file_node_id(path))
        );
        Box::leak(selector.into_boxed_str())
    };
    let anchor = cx
        .debug_bounds(row_selector(&root.join("a.md")))
        .expect("锚点行");
    cx.simulate_mouse_down(anchor.center(), MouseButton::Right, Modifiers::none());
    cx.simulate_mouse_up(anchor.center(), MouseButton::Right, Modifiers::none());
    redraw(cx);

    let panel = cx.debug_bounds("workspace-context-panel").expect("菜单面板");
    // 落点：既压在菜单面板里，也压在某一行文件上（菜单从右键位置往下铺，会盖住下面几行）。
    let under = ["b.md", "c.md"]
        .into_iter()
        .find_map(|name| {
            let selector = row_selector(&root.join(name));
            let bounds = cx.debug_bounds(selector)?;
            let probe = gpui::point(panel.left() + panel.size.width * 0.5, bounds.center().y);
            (panel.contains(&probe) && bounds.contains(&probe)).then_some((selector, probe))
        })
        .expect("前提：菜单该盖住下面某一行");
    cx.simulate_mouse_move(under.1, None, Modifiers::none());
    redraw(cx);
    assert_eq!(
        cx.debug_hovered(under.0),
        Some(false),
        "鼠标压在菜单面板上时，菜单底下那一行还算是悬停"
    );
}

fn tree_drop_node_bounds(
    cx: &mut VisualTestContext,
    path: &std::path::Path,
) -> gpui::Bounds<gpui::Pixels> {
    let selector = format!(
        "workspace-node-{}",
        super::super::stable_node_hash(&super::super::file_node_id(path))
    );
    cx.debug_bounds(Box::leak(selector.into_boxed_str()))
        .expect("文件树节点")
}

fn drag_tree_entry_to(
    cx: &mut VisualTestContext,
    source: &std::path::Path,
    destination: gpui::Point<gpui::Pixels>,
) {
    let row = tree_drop_node_bounds(cx, source);
    let start = gpui::point(row.left() + gpui::px(65.0), row.center().y);
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(
        start + gpui::point(gpui::px(15.0), gpui::px(0.0)),
        MouseButton::Left,
        Modifiers::none(),
    );
    cx.simulate_mouse_move(destination, MouseButton::Left, Modifiers::none());
    redraw(cx);
    assert!(
        cx.cx.read(|cx| cx.has_active_drag()),
        "文件和目录都应能开始拖动"
    );
    cx.simulate_mouse_up(destination, MouseButton::Left, Modifiers::none());
    redraw(cx);
    redraw(cx);
}

fn drop_external_tree_paths(
    cx: &mut VisualTestContext,
    paths: Vec<PathBuf>,
    position: gpui::Point<gpui::Pixels>,
) {
    cx.simulate_event(gpui::FileDropEvent::Entered {
        position,
        paths: gpui::ExternalPaths(paths.into_iter().collect()),
    });
    redraw(cx);
    cx.simulate_event(gpui::FileDropEvent::Submit { position });
    redraw(cx);
    redraw(cx);
}

#[gpui::test]
fn tree_drop_moves_a_file_and_keeps_its_unsaved_document(cx: &mut TestAppContext) {
    let (editor, cx, root) = tree_fixture(cx);
    let source = root.join("note.md");
    let folder = root.join("folder");
    fs::write(&source, "原文\n").unwrap();
    fs::create_dir(&folder).unwrap();
    editor.update_in(cx, |editor, window, cx| {
        editor.open_workspace_file(source.clone(), window, cx);
        editor.replace_document_from_markdown("未保存的修改\n".into(), Some(source.clone()), cx);
        editor.mark_dirty(cx);
        editor.refresh_workspace_tree(cx);
    });
    redraw(cx);
    let recovery_id = editor.read_with(cx, |editor, _| editor.recovery_id);
    let target = tree_drop_node_bounds(cx, &folder).center();
    drag_tree_entry_to(cx, &source, target);
    assert!(!source.exists(), "拖入目录后源路径应移动");
    assert_eq!(
        fs::read_to_string(folder.join("note.md")).unwrap(),
        "原文\n"
    );
    editor.read_with(cx, |editor, _| {
        assert_eq!(editor.file_path, Some(folder.join("note.md")));
        assert_eq!(editor.buffer.text(), "未保存的修改\n");
        assert!(editor.document_dirty);
        assert_eq!(editor.recovery_id, recovery_id);
    });
}

#[gpui::test]
fn tree_drop_moves_a_directory_with_its_children(cx: &mut TestAppContext) {
    let (editor, cx, root) = tree_fixture(cx);
    let source = root.join("source");
    let folder = root.join("target");
    fs::create_dir_all(source.join("nested")).unwrap();
    fs::create_dir(&folder).unwrap();
    fs::write(source.join("nested/note.md"), "正文").unwrap();
    editor.update(cx, |editor, cx| editor.refresh_workspace_tree(cx));
    redraw(cx);
    let target = tree_drop_node_bounds(cx, &folder).center();
    drag_tree_entry_to(cx, &source, target);
    assert!(!source.exists(), "目录应可拖入另一目录");
    assert_eq!(
        fs::read_to_string(folder.join("source/nested/note.md")).unwrap(),
        "正文"
    );
}

#[gpui::test]
fn tree_drop_imports_external_files_into_the_actual_target_instead_of_opening_them(
    cx: &mut TestAppContext,
) {
    let (editor, cx, root) = tree_fixture(cx);
    let outside = root.with_file_name(format!("velora-drop-external-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&outside).unwrap();
    cx.cx.on_quit({
        let outside = outside.clone();
        move || fs::remove_dir_all(outside).unwrap()
    });
    let first = outside.join("first.rs");
    let second = outside.join("second.md");
    fs::write(&first, "fn main() {}\n").unwrap();
    fs::write(&second, "外部文件\n").unwrap();
    let folder = root.join("target");
    fs::create_dir(&folder).unwrap();
    let active = root.join("active.md");
    fs::write(&active, "当前正文\n").unwrap();
    editor.update_in(cx, |editor, window, cx| {
        editor.open_workspace_file(active.clone(), window, cx);
        editor.workspace.selected = Some(WorkspaceSelection::Directory(root.clone()));
        editor.refresh_workspace_tree(cx);
    });
    redraw(cx);
    let target = tree_drop_node_bounds(cx, &folder).center();
    drop_external_tree_paths(cx, vec![first.clone(), second.clone()], target);
    assert_eq!(
        fs::read_to_string(folder.join("first.rs")).ok().as_deref(),
        Some("fn main() {}\n"),
        "外部文件应复制进落点目录"
    );
    assert_eq!(
        fs::read_to_string(folder.join("second.md")).unwrap(),
        "外部文件\n"
    );
    assert!(first.exists() && second.exists(), "外部拖入保留源文件");
    editor.read_with(cx, |editor, _| {
        assert_eq!(editor.file_path, Some(active));
        assert_eq!(editor.buffer.text(), "当前正文\n");
    });
}

#[gpui::test]
fn tree_drop_opens_all_external_files_in_the_editor_without_inserting_images(
    cx: &mut TestAppContext,
) {
    let (editor, cx, root) = tree_fixture(cx);
    let active = root.join("active.md");
    let other = root.join("other.rs");
    let image = root.join("pixel.png");
    fs::write(&active, "当前正文\n").unwrap();
    fs::write(&other, "fn main() {}\n").unwrap();
    fs::write(&image, [137, 80, 78, 71, 13, 10, 26, 10]).unwrap();
    editor.update_in(cx, |editor, window, cx| {
        editor.open_workspace_file(active.clone(), window, cx);
        editor.refresh_workspace_tree(cx);
    });
    redraw(cx);
    let target = cx.debug_bounds("editor-scroll").expect("正文落点").center();
    drop_external_tree_paths(cx, vec![other.clone(), image.clone()], target);
    editor.read_with(cx, |editor, _| {
        assert_eq!(editor.file_path, Some(image));
        assert!(editor.image_preview.is_some(), "图片拖入正文应打开图片预览");
        assert!(
            editor
                .workspace
                .open_documents
                .iter()
                .any(|tab| tab.path == other),
            "多文件都应打开为固定标签"
        );
        assert!(
            editor
                .workspace
                .open_documents
                .iter()
                .any(|tab| tab.path == active)
        );
    });
    assert_eq!(fs::read_to_string(active).unwrap(), "当前正文\n");
}

#[gpui::test]
fn tree_drop_rejects_collisions_for_moves_and_external_copies(cx: &mut TestAppContext) {
    for external in [false, true] {
        let (editor, cx, root) = tree_fixture(cx);
        let source = root.join("note.md");
        let folder = root.join("target");
        fs::create_dir(&folder).unwrap();
        fs::write(&source, "源文件").unwrap();
        fs::write(folder.join("note.md"), "目标原文").unwrap();
        editor.update(cx, |editor, cx| editor.refresh_workspace_tree(cx));
        redraw(cx);
        let target = tree_drop_node_bounds(cx, &folder).center();
        if external {
            drop_external_tree_paths(cx, vec![source.clone()], target);
        } else {
            drag_tree_entry_to(cx, &source, target);
        }
        assert_eq!(fs::read_to_string(&source).unwrap(), "源文件");
        assert_eq!(
            fs::read_to_string(folder.join("note.md")).unwrap(),
            "目标原文"
        );
        assert!(
            editor.read_with(cx, |editor, _| editor.modal_is_open()),
            "同名冲突应显示应用内错误"
        );
    }
}

#[gpui::test]
fn tree_drop_rejects_moving_a_directory_into_its_descendant(cx: &mut TestAppContext) {
    let (editor, cx, root) = tree_fixture(cx);
    let source = root.join("source");
    let child = source.join("nested");
    fs::create_dir_all(&child).unwrap();
    fs::write(source.join("note.md"), "保留").unwrap();
    editor.update(cx, |editor, cx| {
        editor
            .workspace
            .expanded
            .insert(super::super::file_node_id(&source));
        editor.refresh_workspace_tree(cx);
    });
    redraw(cx);
    redraw(cx);
    let target = tree_drop_node_bounds(cx, &child).center();
    drag_tree_entry_to(cx, &source, target);
    assert_eq!(fs::read_to_string(source.join("note.md")).unwrap(), "保留");
    assert!(!child.join("source").exists());
    assert!(editor.read_with(cx, |editor, _| editor.modal_is_open()));
}

#[gpui::test]
fn tree_drop_uses_a_files_parent_and_the_root_for_blank_space(cx: &mut TestAppContext) {
    let (editor, cx, root) = tree_fixture(cx);
    let folder = root.join("target");
    fs::create_dir(&folder).unwrap();
    let first = root.join("first.md");
    let second = folder.join("second.md");
    let anchor = folder.join("anchor.txt");
    fs::write(&first, "first").unwrap();
    fs::write(&second, "second").unwrap();
    fs::write(&anchor, "anchor").unwrap();
    editor.update(cx, |editor, cx| {
        editor
            .workspace
            .expanded
            .insert(super::super::file_node_id(&folder));
        editor.refresh_workspace_tree(cx);
    });
    redraw(cx);
    redraw(cx);
    let target = tree_drop_node_bounds(cx, &anchor).center();
    drag_tree_entry_to(cx, &first, target);
    assert_eq!(
        fs::read_to_string(folder.join("first.md")).unwrap(),
        "first"
    );
    assert!(!first.exists());
    let panel = cx.debug_bounds("workspace-panel").expect("文件树面板");
    let target = gpui::point(panel.center().x, panel.bottom() - gpui::px(20.0));
    drag_tree_entry_to(cx, &second, target);
    assert_eq!(
        fs::read_to_string(root.join("second.md")).unwrap(),
        "second"
    );
    assert!(!second.exists());
}

#[gpui::test]
fn tree_drop_preserves_dirty_untitled_content_when_opening_an_external_file(
    cx: &mut TestAppContext,
) {
    let (editor, cx, root) = tree_fixture(cx);
    let target = root.join("open.rs");
    fs::write(&target, "fn main() {}\n").unwrap();
    editor.update(cx, |editor, cx| {
        editor.show_welcome = false;
        editor.replace_document_from_markdown("未命名的未保存正文\n".into(), None, cx);
        editor.mark_dirty(cx);
    });
    redraw(cx);
    let point = cx.debug_bounds("editor-scroll").unwrap().center();
    drop_external_tree_paths(cx, vec![target.clone()], point);
    editor.read_with(cx, |editor, _| {
        assert_eq!(editor.file_path, Some(target));
        assert!(!editor.show_drop_replace_dialog);
        assert!(
            editor.workspace.open_documents.iter().any(|tab| {
                super::super::untitled_document_id(&tab.path).is_some()
                    && tab.dirty
                    && tab.markdown == "未命名的未保存正文\n"
            }),
            "拖入应打开新标签，并保留未命名文档"
        );
    });
}

#[gpui::test]
fn tree_drop_imports_external_directories_without_decoding_their_files(cx: &mut TestAppContext) {
    let (editor, cx, root) = tree_fixture(cx);
    let outside = root.with_file_name(format!("velora-drop-directory-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(outside.join("nested/empty")).unwrap();
    cx.cx.on_quit({
        let outside = outside.clone();
        move || fs::remove_dir_all(outside).unwrap()
    });
    fs::write(outside.join("nested/raw.bin"), [0, 255, 0, 128]).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink("..", outside.join("nested/cycle")).unwrap();
    let point = tree_drop_node_bounds(cx, &root).center();
    drop_external_tree_paths(cx, vec![outside.clone()], point);
    let imported = root.join(outside.file_name().unwrap());
    assert!(imported.join("nested/empty").is_dir());
    assert_eq!(
        fs::read(imported.join("nested/raw.bin")).unwrap(),
        [0, 255, 0, 128]
    );
    assert!(outside.join("nested/raw.bin").is_file());
    #[cfg(unix)]
    assert_eq!(
        fs::read_link(imported.join("nested/cycle")).unwrap(),
        PathBuf::from("..")
    );
    assert!(editor.read_with(cx, |editor, _| editor.file_path.is_none()));
}

#[gpui::test]
fn tree_drop_directory_moves_update_every_split_group_and_keep_shared_edits(
    cx: &mut TestAppContext,
) {
    let (editor, cx, root) = tree_fixture(cx);
    let source = root.join("source");
    let target = root.join("target");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&target).unwrap();
    let path = source.join("note.md");
    fs::write(&path, "原文\n").unwrap();
    editor.update_in(cx, |editor, window, cx| {
        editor.open_workspace_file(path.clone(), window, cx);
        editor.set_workspace_root(root.clone(), cx);
        editor.refresh_workspace_tree(cx);
        editor.split_active_document(super::super::split::SplitDirection::Right, window, cx);
    });
    redraw(cx);
    redraw(cx);
    let groups = editor.read_with(cx, |editor, _| editor.split_groups());
    assert_eq!(groups.len(), 2);
    groups.first().unwrap().update(cx, |editor, cx| {
        editor.buffer.edit(0..0, "未保存");
        editor.document_dirty = true;
        cx.notify();
    });
    redraw(cx);
    let point = tree_drop_node_bounds(cx, &target).center();
    drag_tree_entry_to(cx, &source, point);
    redraw(cx);
    let destination = target.join("source/note.md");
    assert!(destination.is_file());
    assert!(!path.exists());
    for group in &groups {
        group.read_with(cx, |editor, _| {
            assert_eq!(editor.file_path, Some(destination.clone()));
            assert_eq!(editor.buffer.text(), "未保存原文\n");
            assert!(editor.document_dirty);
            assert!(
                editor
                    .workspace
                    .open_documents
                    .iter()
                    .all(|tab| tab.path != path)
            );
        });
    }
    let active = editor.read_with(cx, |editor, _| editor.active_split_editor().unwrap());
    active.update_in(cx, |editor, window, cx| {
        assert!(editor.save_to_existing_path(&destination, window, cx));
    });
    redraw(cx);
    assert_eq!(fs::read_to_string(&destination).unwrap(), "未保存原文\n");
    assert!(!path.exists(), "保存不能重新创建移动前的目录和文件");
}

#[gpui::test]
fn tree_name_input_exposes_native_text_semantics_and_undo(cx: &mut TestAppContext) {
    use gpui_kit::test::TestWindowExt as _;
    let (editor, visual, root) = tree_fixture(cx);
    editor.update_in(visual, |editor, window, cx| {
        window.activate_window();
        editor.create_generic_workspace_file(window, cx);
    });
    redraw(visual);
    assert!(
        visual.debug_bounds("workspace-name-input").is_some(),
        "名称输入已挂载"
    );
    visual.update(|window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window
                .try_find("workspace-name-input")
                .and_then(|field| field.role()),
            Some(gpui::Role::TextInput),
            "树内名称应提供标准文本输入语义"
        );
        window.input("笔记 📝.md", cx);
        window.press("secondary-a", cx);
        window.press("secondary-x", cx);
        window.press("secondary-z", cx);
    });
    redraw(visual);
    visual.update(|window, cx| window.press("enter", cx));
    redraw(visual);
    assert!(
        root.join("笔记 📝.md").is_file(),
        "撤销剪切后应恢复完整名称"
    );
    assert!(editor.read_with(visual, |editor, _| editor.buffer.text().is_empty()));
}
