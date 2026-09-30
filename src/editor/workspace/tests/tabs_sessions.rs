use super::super::Editor;
use gpui::{
    Modifiers, ScrollDelta, ScrollWheelEvent,
    TestAppContext, TouchPhase, point, px,
};
use std::fs;


#[gpui::test]
async fn switching_workspace_drops_the_previous_workspaces_tabs(cx: &mut TestAppContext) {
    // 用户报修：切换工作区之后，顶栏还留着上一个工作区的标签。
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let root_a = std::env::temp_dir().join(format!("velora-switch-a-{}", uuid::Uuid::new_v4()));
    let root_b = std::env::temp_dir().join(format!("velora-switch-b-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root_a).unwrap();
    fs::create_dir_all(&root_b).unwrap();
    let alpha = root_a.join("alpha.md");
    let beta = root_a.join("beta.md");
    fs::write(&alpha, "# alpha\n").unwrap();
    fs::write(&beta, "# beta\n").unwrap();
    let gamma = root_b.join("gamma.md");
    fs::write(&gamma, "# gamma\n").unwrap();
    cx.on_quit({
        let (root_a, root_b) = (root_a.clone(), root_b.clone());
        move || {
            let _ = fs::remove_dir_all(root_a);
            let _ = fs::remove_dir_all(root_b);
        }
    });

    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.set_workspace_root(root_a.clone(), cx);
        });
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(alpha.clone(), window, cx);
            editor.open_workspace_file(beta.clone(), window, cx);
        });
    });
    editor.read_with(cx, |editor, _| {
        assert_eq!(editor.workspace.open_documents.len(), 2, "切换前应有两个标签");
    });

    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| {
            editor.set_workspace_root(root_b.clone(), cx);
        });
    });
    editor.read_with(cx, |editor, _| {
        assert!(
            editor.workspace.open_documents.is_empty(),
            "上一个工作区的标签必须全部收起，实测 {:?}",
            editor.workspace.open_documents.iter().map(|tab| tab.path.clone()).collect::<Vec<_>>()
        );
        assert!(editor.workspace.active_document.is_none());
        assert!(editor.show_welcome, "没有可留的标签时应回到欢迎页");
    });
}

#[gpui::test]
async fn single_click_previews_and_double_click_pins_tabs(cx: &mut TestAppContext) {
    // 用户需求：单击打开为预览标签——切换到其它文件时未修改的预览标签被
    // 替换、不再占据标签栏；双击打开固定常驻；已修改的预览不会被替换。
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let root = std::env::temp_dir().join(format!("velora-preview-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let paths: Vec<std::path::PathBuf> = ["a", "b", "c", "d", "e"]
        .iter()
        .map(|name| {
            let path = root.join(format!("{name}.md"));
            fs::write(&path, format!("# {name}\n")).unwrap();
            path
        })
        .collect();
    cx.on_quit({
        let root = root.clone();
        move || {
            let _ = fs::remove_dir_all(root);
        }
    });
    let preview = super::super::WorkspaceOpenMode::Preview;
    let pinned = super::super::WorkspaceOpenMode::Pinned;

    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.set_workspace_root(root.clone(), cx);
        });
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file_in_mode(paths[0].clone(), preview, window, cx);
        });
    });
    editor.read_with(cx, |editor, _| {
        let tabs = &editor.workspace.open_documents;
        assert_eq!(tabs.len(), 1);
        assert!(tabs[0].preview, "单击打开的应是预览标签");
    });

    // 单击另一个文件：旧的未修改预览标签被替换。
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file_in_mode(paths[1].clone(), preview, window, cx);
        });
    });
    editor.read_with(cx, |editor, _| {
        let tabs = &editor.workspace.open_documents;
        assert_eq!(tabs.len(), 1, "切走后未修改的预览标签应被替换");
        assert_eq!(tabs[0].path, paths[1]);
    });

    // 双击打开：固定，之后切走不再被替换。
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file_in_mode(paths[0].clone(), pinned, window, cx);
        });
    });
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file_in_mode(paths[2].clone(), preview, window, cx);
        });
    });
    editor.read_with(cx, |editor, _| {
        let tabs = &editor.workspace.open_documents;
        assert_eq!(tabs.len(), 2, "固定标签保留，预览标签只有当前一个");
        assert!(
            tabs.iter().any(|tab| tab.path == paths[0] && !tab.preview),
            "双击打开的标签应为固定"
        );
    });

    // 已修改的预览标签切走后保留。
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file_in_mode(paths[3].clone(), preview, window, cx);
            if let Some(tab) = editor
                .workspace
                .open_documents
                .iter_mut()
                .find(|tab| tab.path == paths[3])
            {
                tab.dirty = true;
            }
        });
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file_in_mode(paths[4].clone(), preview, window, cx);
        });
    });
    editor.read_with(cx, |editor, _| {
        let tabs = &editor.workspace.open_documents;
        assert!(
            tabs.iter().any(|tab| tab.path == paths[3]),
            "已修改的预览标签切走后应保留"
        );
        assert_eq!(tabs.len(), 3);
    });
}

#[gpui::test]
async fn switching_workspace_keeps_inner_tabs_and_reopens_one(cx: &mut TestAppContext) {
    // 新根目录是旧根的子目录：子目录里的标签要留下，且活动标签被收起时
    // 下一帧补开剩下的那个。
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let root = std::env::temp_dir().join(format!("velora-switch-inner-{}", uuid::Uuid::new_v4()));
    let inner = root.join("sub");
    fs::create_dir_all(&inner).unwrap();
    let outer = root.join("outer.md");
    let inside = inner.join("inside.md");
    fs::write(&outer, "# outer\n").unwrap();
    fs::write(&inside, "# inside\n").unwrap();
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
        });
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(inside.clone(), window, cx);
            editor.open_workspace_file(outer.clone(), window, cx);
        });
    });
    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| {
            editor.set_workspace_root(inner.clone(), cx);
        });
    });
    editor.read_with(cx, |editor, _| {
        let kept = editor
            .workspace
            .open_documents
            .iter()
            .map(|tab| tab.path.clone())
            .collect::<Vec<_>>();
        assert_eq!(kept, vec![inside.clone()], "只应留下新根目录内的标签");
    });
    cx.update(|window, cx| window.draw(cx).clear());
    editor.read_with(cx, |editor, _| {
        // 活动标签被收起后，补开剩下的那个（延后一帧，那时才拿得到 Window）。
        assert_eq!(
            editor.file_path.as_deref(),
            Some(inside.as_path()),
            "应补开新根目录内剩下的标签，file_path={:?} active={:?}",
            editor.file_path,
            editor.workspace.active_document
        );
        assert!(editor.pending_workspace_tab_activation.is_none());
        assert!(!editor.show_welcome, "还有标签时不该回到欢迎页");
    });
}

#[gpui::test]
async fn switching_workspace_saves_dirty_stale_tabs_instead_of_losing_them(
    cx: &mut TestAppContext,
) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let root_a = std::env::temp_dir().join(format!("velora-switch-dirty-a-{}", uuid::Uuid::new_v4()));
    let root_b = std::env::temp_dir().join(format!("velora-switch-dirty-b-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root_a).unwrap();
    fs::create_dir_all(&root_b).unwrap();
    let doc = root_a.join("notes.md");
    fs::write(&doc, "# 原文\n").unwrap();
    cx.on_quit({
        let (root_a, root_b) = (root_a.clone(), root_b.clone());
        move || {
            let _ = fs::remove_dir_all(root_a);
            let _ = fs::remove_dir_all(root_b);
        }
    });

    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.set_workspace_root(root_a.clone(), cx);
        });
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(doc.clone(), window, cx);
        });
    });
    // 造一个未保存的脏标签。
    editor.update(cx, |editor, _cx| {
        editor.workspace.open_documents[0].dirty = true;
        editor.workspace.open_documents[0].markdown = "# 未保存的修改\n".into();
    });
    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| {
            editor.set_workspace_root(root_b.clone(), cx);
        });
    });
    editor.read_with(cx, |editor, _| {
        assert!(editor.workspace.open_documents.is_empty(), "脏标签也应被收起");
    });
    assert_eq!(
        fs::read_to_string(&doc).unwrap(),
        "# 未保存的修改\n",
        "切换工作区不能吞掉未保存的内容"
    );
}

#[gpui::test]
async fn clicking_a_file_keeps_the_tree_scroll_offset(cx: &mut TestAppContext) {
    // 用户报修：长文件树滚到下面后点一个文件，树会刷新并自动置顶。
    // 根因是打开文件时把已扫描的树清成 None，重扫落地前的那一帧侧栏只剩
    // 「…」（内容高度 ≈30px），gpui 的 div 会把记住的偏移按新的 scroll_max
    // 夹到 0 并写回，重扫完成后偏移已经没了。
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let root = std::env::temp_dir().join(format!(
        "velora-tree-scroll-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&root).unwrap();
    for index in 0..80 {
        fs::write(
            root.join(format!("note-{index:02}.md")),
            format!("# note {index}\n"),
        )
        .unwrap();
    }
    cx.on_quit({
        let root = root.clone();
        move || {
            let _ = fs::remove_dir_all(root);
        }
    });

    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
    editor.update(cx, |editor, cx| {
        editor.set_workspace_root(root.clone(), cx);
        editor.workspace.is_open = true;
    });
    cx.run_until_parked();
    // 压矮窗口，让 80 行的树必须滚动才有下方内容。
    cx.update(|window, _cx| window.resize(gpui::size(px(320.0), px(260.0))));
    cx.update(|window, cx| window.draw(cx).clear());

    cx.simulate_event(ScrollWheelEvent {
        position: point(px(60.0), px(200.0)),
        delta: ScrollDelta::Pixels(point(px(0.0), px(-600.0))),
        modifiers: Modifiers::default(),
        touch_phase: TouchPhase::default(),
    });
    cx.update(|window, cx| window.draw(cx).clear());
    let scrolled =
        editor.read_with(cx, |editor, _| editor.workspace.tree_scroll_handle.offset().y);
    assert!(
        scrolled < px(0.0),
        "滚轮应把长树滚下去（gpui 的偏移向下为负），实测 {scrolled:?}"
    );

    let target = root.join("note-70.md");
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(target.clone(), window, cx);
        });
    });
    // 重扫落地前的那一帧：树不能被丢掉。
    cx.update(|window, cx| window.draw(cx).clear());
    editor.read_with(cx, |editor, _| {
        assert!(
            editor.workspace.file_tree.is_some(),
            "同一根目录下点开文件不应丢掉已扫描的树"
        );
    });
    let after_click =
        editor.read_with(cx, |editor, _| editor.workspace.tree_scroll_handle.offset().y);
    assert_eq!(after_click, scrolled, "点文件那一刻滚动位置不应被重置");

    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear());
    let settled =
        editor.read_with(cx, |editor, _| editor.workspace.tree_scroll_handle.offset().y);
    assert_eq!(settled, scrolled, "重扫落地后滚动位置仍应保持");
}

