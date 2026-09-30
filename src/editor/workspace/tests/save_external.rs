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

