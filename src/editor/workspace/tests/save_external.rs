use super::super::{
    Editor, WorkspaceSelection,
};
use crate::components::UndoCaptureKind;
use gpui::{
    TestAppContext, point, px,
};
use std::fs;
use std::time::Duration;


#[gpui::test]
async fn manual_save_then_typing_does_not_report_an_external_change(cx: &mut TestAppContext) {
// 用户报修（审查发现）：⌘S 只同步 Editor::file_version，工作区标签还留着旧版本号；
// 再敲一个字，自动保存拿旧版本去校验刚写的盘上内容，误报「检测到外部修改」
// 并停掉整个会话的自动保存。
cx.update(|cx| {
    crate::i18n::I18nManager::init(cx);
    crate::theme::ThemeManager::init(cx);
    crate::components::init(cx);
});
let root = std::env::temp_dir().join(format!(
    "velora-manual-save-{}",
    uuid::Uuid::new_v4()
));
fs::create_dir_all(&root).unwrap();
let path = root.join("a.md");
fs::write(&path, "alpha\n").expect("write initial markdown");
cx.on_quit({
    let root = root.clone();
    move || {
        let _ = fs::remove_dir_all(root);
    }
});

let (editor, cx) =
    cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
cx.update(|window, cx| {
    editor.update(cx, |editor, cx| {
        editor.set_workspace_root(root.clone(), cx);
        editor.open_workspace_file(path.clone(), window, cx);
    });
});
cx.run_until_parked();

// 编辑 → 手动保存。
editor.update(cx, |editor, cx| {
    let block = editor.document.root_blocks()[0].clone();
    block.update(cx, |block, cx| {
        block.prepare_undo_capture(UndoCaptureKind::CoalescibleText, cx);
        block.replace_text_in_visible_range(0..0, "one ", None, false, cx);
    });
});
cx.run_until_parked();
cx.update(|window, cx| {
    editor.update(cx, |editor, cx| editor.save_document(window, cx));
});
cx.run_until_parked();
editor.read_with(cx, |editor, _cx| {
    let tab = editor
        .workspace
        .open_documents
        .iter()
        .find(|tab| tab.path == path)
        .expect("文件应从树里打开为标签");
    assert_eq!(
        tab.file_version,
        editor.file_version.expect("手动保存后编辑器版本应已更新"),
        "手动保存后标签的 file_version 必须同步"
    );
});

// 再编辑一个字，等自动保存落地。
editor.update(cx, |editor, cx| {
    let block = editor.document.root_blocks()[0].clone();
    block.update(cx, |block, cx| {
        block.prepare_undo_capture(UndoCaptureKind::CoalescibleText, cx);
        block.replace_text_in_visible_range(0..0, "two ", None, false, cx);
    });
});
cx.executor().advance_clock(Duration::from_secs(2));
cx.run_until_parked();
editor.read_with(cx, |editor, _cx| {
    assert!(
        !editor.has_external_autosave_conflict(),
        "自己的手动保存不能被当成外部修改"
    );
});
assert!(
    fs::read_to_string(&path)
        .expect("read autosaved file")
        .starts_with("two "),
    "自动保存应把新编辑落盘"
);
}

#[gpui::test]
async fn autosave_conflict_reports_the_file_that_actually_changed(cx: &mut TestAppContext) {
// 审查发现：两个脏标签、其中一个被外部改动时，冲突路径总是记成第一个
// 有路径的文档，冲突标记清不掉、自动保存一直停摆。
cx.update(|cx| {
    crate::i18n::I18nManager::init(cx);
    crate::theme::ThemeManager::init(cx);
    crate::components::init(cx);
});
let root = std::env::temp_dir().join(format!(
    "velora-autosave-conflict-{}",
    uuid::Uuid::new_v4()
));
fs::create_dir_all(&root).unwrap();
let first_path = root.join("a.md");
let second_path = root.join("b.md");
fs::write(&first_path, "alpha\n").unwrap();
fs::write(&second_path, "beta\n").unwrap();
cx.on_quit({
    let root = root.clone();
    move || {
        let _ = fs::remove_dir_all(root);
    }
});

let (editor, cx) =
    cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
cx.update(|window, cx| {
    editor.update(cx, |editor, cx| {
        editor.set_workspace_root(root.clone(), cx);
        editor.open_workspace_file(first_path.clone(), window, cx);
    });
});
cx.run_until_parked();
let edit_first_block = |editor: &mut Editor, text: &str, cx: &mut gpui::Context<Editor>| {
    let block = editor.document.root_blocks()[0].clone();
    block.update(cx, |block, cx| {
        block.prepare_undo_capture(UndoCaptureKind::CoalescibleText, cx);
        block.replace_text_in_visible_range(0..0, text, None, false, cx);
    });
};
editor.update(cx, |editor, cx| edit_first_block(editor, "one ", cx));
cx.run_until_parked();
// 打开第二个文件（a 的未保存内容留在它的标签里），也改一笔。
cx.update(|window, cx| {
    editor.update(cx, |editor, cx| {
        editor.open_workspace_file(second_path.clone(), window, cx);
    });
});
cx.run_until_parked();
editor.update(cx, |editor, cx| edit_first_block(editor, "two ", cx));
cx.run_until_parked();
// 外部改动 b.md。
fs::write(&second_path, "external beta\n").expect("write external changes");

cx.executor().advance_clock(Duration::from_secs(2));
cx.run_until_parked();
editor.read_with(cx, |editor, _cx| {
    let (conflict_path, _) = editor
        .workspace
        .external_change_conflict
        .clone()
        .expect("应记录外部修改冲突");
    assert_eq!(
        conflict_path, second_path,
        "冲突必须记在真正被外部改动的文件上（而不是第一个有路径的标签）"
    );
});
}


#[gpui::test]
async fn workspace_context_menu_keeps_the_right_clicked_directory(cx: &mut TestAppContext) {
    // 用户报修：右键目录后，侧栏每帧把选中项改回活动文件，菜单动作
    // （新建/粘贴/重命名/删除…）会作用到活动文件上。
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let root = std::env::temp_dir().join(format!(
        "velora-context-menu-{}",
        uuid::Uuid::new_v4()
    ));
    let drafts = root.join("drafts");
    fs::create_dir_all(&drafts).unwrap();
    let note = root.join("a.md");
    fs::write(&note, "# a\n").unwrap();
    cx.on_quit({
        let root = root.clone();
        move || {
            let _ = fs::remove_dir_all(root);
        }
    });

    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.set_workspace_root(root.clone(), cx);
            editor.workspace.is_open = true;
            editor.open_workspace_file(note.clone(), window, cx);
        });
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear());
    editor.read_with(cx, |editor, _| {
        assert_eq!(
            editor.workspace.selected,
            Some(WorkspaceSelection::File(note.clone())),
            "打开文件后树应跟随活动文件"
        );
    });

    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_context_menu(
                point(px(40.0), px(120.0)),
                Some(WorkspaceSelection::Directory(drafts.clone())),
                cx,
            );
        });
    });
    // 菜单渲染帧：以前会把选中项改回活动文件。
    cx.update(|window, cx| window.draw(cx).clear());
    editor.read_with(cx, |editor, _| {
        assert_eq!(
            editor.workspace.selected,
            Some(WorkspaceSelection::Directory(drafts.clone())),
            "右键目录后，选中目标不能被活动文件顶掉"
        );
        assert_eq!(
            editor.selected_workspace_directory(),
            Some(drafts.clone()),
            "新建/粘贴的目标目录应是右键的那个目录"
        );
    });
}


/// 外部改动冲突要有解除入口（用户需求：两个动作——重载 / 另存为）。
/// 这一条盯住「重载」：盘上的那版读回来，本地编辑一个字节都不写进盘，
/// 而放弃掉的内容得留在恢复快照里；「继续编辑」（Esc 那一位）什么都不动。
#[gpui::test]
async fn the_conflict_modal_reload_reads_the_disk_version_back(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let root = std::env::temp_dir().join(format!(
        "velora-conflict-reload-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&root).unwrap();
    let root = fs::canonicalize(&root).unwrap_or(root);
    let path = root.join("note.md");
    fs::write(&path, "alpha\n").unwrap();
    let cleanup = root.clone();
    cx.on_quit(move || {
        let _ = fs::remove_dir_all(cleanup);
    });

    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.set_workspace_root(root.clone(), cx);
            editor.open_workspace_file(path.clone(), window, cx);
        });
    });
    cx.run_until_parked();
    let edit = |editor: &mut Editor, text: &str, cx: &mut gpui::Context<Editor>| {
        let block = editor.document.root_blocks()[0].clone();
        block.update(cx, |block, cx| {
            block.prepare_undo_capture(UndoCaptureKind::CoalescibleText, cx);
            block.replace_text_in_visible_range(0..0, text, None, false, cx);
        });
    };
    editor.update(cx, |editor, cx| edit(editor, "our edit ", cx));
    cx.run_until_parked();
    let recovery_id = editor.read_with(cx, |editor, _| editor.recovery_id);
    cx.on_quit(move || {
        let _ = crate::config::remove_recovery_snapshot(recovery_id);
    });
    fs::write(&path, "external version\n").expect("external write");
    cx.executor().advance_clock(Duration::from_secs(2));
    cx.run_until_parked();

    editor.read_with(cx, |editor, _| {
        assert!(
            editor.modal_is_open(),
            "冲突被记下就要给解除入口，不能只留一条红字"
        );
    });

    // 「继续编辑」（cancel 位）：冲突、脏标记、盘上内容都保持原样。
    editor.update_in(cx, |editor, window, cx| editor.dismiss_modal(2, window, cx));
    editor.read_with(cx, |editor, _cx| {
        assert!(!editor.modal_is_open());
        assert!(
            editor.workspace.external_change_conflict.is_some(),
            "选「继续编辑」不该把冲突当成已解决"
        );
        assert!(editor.document_dirty);
    });
    assert_eq!(fs::read_to_string(&path).unwrap(), "external version\n");

    // 「重载」：再走同一个入口，选第 0 位。
    editor.update(cx, |editor, cx| {
        editor.show_external_change_conflict_modal(path.clone(), cx)
    });
    editor.update_in(cx, |editor, window, cx| editor.dismiss_modal(0, window, cx));
    cx.run_until_parked();

    editor.read_with(cx, |editor, cx| {
        assert_eq!(
            editor.document.markdown_text(cx),
            "external version",
            "重载要把盘上那一版交回来"
        );
        assert!(
            !editor.document_dirty,
            "内容与磁盘一致了就不该再算未保存（圆点得灭）"
        );
        assert!(
            editor.workspace.external_change_conflict.is_none(),
            "重载之后冲突就该解除"
        );
        let tab = editor
            .workspace
            .open_documents
            .iter()
            .find(|tab| tab.path == path)
            .expect("note.md 标签");
        assert!(!tab.dirty, "标签的脏标记也要跟上");
    });
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "external version\n",
        "重载不能顺手把盘上内容再写一遍"
    );
    let recovery_dir = crate::config::VeloraConfigDirs::from_system()
        .expect("config dirs")
        .recovery_dir();
    let stashed = fs::read_to_string(recovery_dir.join(format!("{recovery_id}.json")))
        .expect("按「重载」放弃的编辑要有恢复快照");
    // 这一份也可能只是自动保存 tick 写的（它在前一步就存过同样的内容），所以
    // stash 那一步的牙齿在 `..._save_as_...` 那条用例里——那里自动保存没 tick 过。
    assert!(
        stashed.contains("our edit"),
        "放弃掉的那笔编辑要留在恢复快照里，实测 {stashed:?}"
    );
}

/// 「另存为」这一位：当前编辑内容留着并去要一个新路径，不碰冲突那篇文件。
#[gpui::test]
async fn the_conflict_modal_save_as_keeps_the_edits_and_asks_for_a_new_path(
    cx: &mut TestAppContext,
) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let root = std::env::temp_dir().join(format!(
        "velora-conflict-save-as-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&root).unwrap();
    let root = fs::canonicalize(&root).unwrap_or(root);
    let path = root.join("note.md");
    fs::write(&path, "alpha\n").unwrap();
    let cleanup = root.clone();
    cx.on_quit(move || {
        let _ = fs::remove_dir_all(cleanup);
    });

    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.set_workspace_root(root.clone(), cx);
            editor.open_workspace_file(path.clone(), window, cx);
        });
    });
    cx.run_until_parked();
    editor.update(cx, |editor, cx| {
        let block = editor.document.root_blocks()[0].clone();
        block.update(cx, |block, cx| {
            block.prepare_undo_capture(UndoCaptureKind::CoalescibleText, cx);
            block.replace_text_in_visible_range(0..0, "our edit ", None, false, cx);
        });
    });
    cx.run_until_parked();
    fs::write(&path, "external version\n").expect("external write");
    // 这次不放钟：让冲突由手动保存撞出来，于是自动保存一次快照都还没写过——
    // 「重载」那一侧的 stash 才有东西可证。
    let saved = editor.update_in(cx, |editor, window, cx| {
        editor.save_to_existing_path(&path, window, cx)
    });
    assert!(!saved, "冲突时手动保存必须拒绝写盘");
    editor.read_with(cx, |editor, _| {
        assert!(
            editor.modal_is_open(),
            "⌘S 撞上外部改动也要给同一个解除入口"
        );
    });

    // `pending_save_as` 只活到下一帧：绘制时 `sync_pending_save_as` 就把它取走换成
    // 新路径面板，所以这一条要在同一次 update 里看。
    editor.update_in(cx, |editor, window, cx| {
        editor.dismiss_modal(1, window, cx);
        assert!(
            editor.pending_save_as,
            "「另存为」要走到取新路径的那条路上"
        );
        assert!(
            editor.document_dirty,
            "另存为之后本地编辑仍然是未保存状态"
        );
    });
    cx.run_until_parked();
    editor.read_with(cx, |editor, cx| {
        assert!(
            editor.document.markdown_text(cx).contains("our edit"),
            "另存为不该把本地编辑换掉"
        );
    });
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "external version\n",
        "另存为不能动那篇被外部改过的文件"
    );

    // 同一场冲突改选「重载」：放弃掉的那笔编辑得进恢复快照。本用例里自动保存一次
    // 都没 tick 过，所以这条证的正是「重载」自己那一步 stash，而不是顺带复用。
    let recovery_id = editor.read_with(cx, |editor, _| editor.recovery_id);
    cx.on_quit(move || {
        let _ = crate::config::remove_recovery_snapshot(recovery_id);
    });
    let snapshot_path = crate::config::VeloraConfigDirs::from_system()
        .expect("config dirs")
        .recovery_dir()
        .join(format!("{recovery_id}.json"));
    editor.update(cx, |editor, cx| {
        editor.show_external_change_conflict_modal(path.clone(), cx)
    });
    editor.update_in(cx, |editor, window, cx| editor.dismiss_modal(0, window, cx));
    cx.run_until_parked();
    let stashed = fs::read_to_string(&snapshot_path).expect("放弃的编辑要有恢复快照");
    assert!(
        stashed.contains("our edit"),
        "按「重载」放弃掉的内容要落到恢复快照里，实测 {stashed:?}"
    );
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "external version\n",
        "存恢复快照不是写正文文件"
    );
}
