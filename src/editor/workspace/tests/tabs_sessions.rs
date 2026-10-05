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

/// 标签之间来回切，每篇的阅读现场（视图模式 / 视口 / 光标）都得是自己那份：
/// 切走时记在标签上、切回来交还。同源缺陷——切换以前等于重开一篇，模式跳回
/// 渲染态、视口弹回顶部，与外部改动重载那笔是一个根。
#[gpui::test]
async fn switching_tabs_keeps_each_documents_reading_position(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let root = std::env::temp_dir().join(format!(
        "velora-tab-reading-position-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&root).unwrap();
    // macOS 的 /var → /private/var 符号链接：树里与标签的路径都按 canonical 走。
    let root = fs::canonicalize(&root).unwrap_or(root);
    let alpha = root.join("alpha.md");
    let beta = root.join("beta.md");
    let text = super::external_changes::long_markdown(400);
    // 落点要取在字符边界上：整篇是中文，任意字节位会把选区劈进多字节字符中间。
    let mut caret = 1234usize;
    while !text.is_char_boundary(caret) {
        caret -= 1;
    }
    fs::write(&alpha, &text).unwrap();
    fs::write(&beta, &text).unwrap();
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
        editor.workspace.is_open = false;
    });
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(alpha.clone(), window, cx)
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear());

    // alpha 的现场：源码模式、滚到中段、光标放在缓冲区中段的一个字符边界上。
    editor.update(cx, |editor, cx| editor.toggle_view_mode(cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear());
    editor.update(cx, |editor, cx| {
        editor.set_vertical_scroll_offset(px(-3000.0), cx);
        let block = editor.document.root_blocks()[0].clone();
        editor.active_entity_id = Some(block.entity_id());
        block.update(cx, |block, _cx| block.selected_range = caret..caret);
    });
    let alpha_view = editor.read_with(cx, |editor, cx| editor.capture_document_view(cx));
    assert!(
        matches!(alpha_view.view_mode, crate::editor::ViewMode::Source)
            && alpha_view.scroll_y < 0.0
            && alpha_view.selection.range == (caret..caret),
        "前置：alpha 应在源码模式、已滚开、光标在中段，实测 {alpha_view:?}"
    );

    // beta：本次会话第一次读它——渲染态、文档顶部，不许继承 alpha 的现场。
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(beta.clone(), window, cx)
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear());
    editor.read_with(cx, |editor, _| {
        assert_eq!(
            editor.view_mode,
            crate::editor::ViewMode::Rendered,
            "第一次打开 beta 不应带着 alpha 的源码模式"
        );
        assert_eq!(
            editor.scroll_handle.offset().y,
            px(0.0),
            "第一次打开 beta 应从文档顶部开始"
        );
    });

    // beta 自己的现场：滚一段。
    editor.update(cx, |editor, cx| {
        editor.set_vertical_scroll_offset(px(-1200.0), cx);
    });
    let beta_view = editor.read_with(cx, |editor, cx| editor.capture_document_view(cx));
    assert!(beta_view.scroll_y < 0.0, "前置：beta 应已滚开，实测 {beta_view:?}");

    // 切回 alpha：模式、视口、光标按它那份交还（跑几帧，夹取与校正都在这些帧里发生）。
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(alpha.clone(), window, cx)
        })
    });
    cx.run_until_parked();
    for _ in 0..4 {
        cx.update(|window, cx| window.draw(cx).clear());
    }
    let restored = editor.read_with(cx, |editor, cx| editor.capture_document_view(cx));
    assert_eq!(
        restored.view_mode, alpha_view.view_mode,
        "切回 alpha 把源码模式换掉了，实测 {restored:?}"
    );
    assert_eq!(
        restored.scroll_y, alpha_view.scroll_y,
        "切回 alpha 的视口不对：应 {}，实测 {}",
        alpha_view.scroll_y, restored.scroll_y
    );
    assert_eq!(
        restored.selection.range, alpha_view.selection.range,
        "切回 alpha 的光标不对：应 {:?}，实测 {:?}",
        alpha_view.selection.range, restored.selection.range
    );

    // 再切回 beta：它那一份没被 alpha 盖掉。
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(beta.clone(), window, cx)
        })
    });
    cx.run_until_parked();
    for _ in 0..4 {
        cx.update(|window, cx| window.draw(cx).clear());
    }
    let restored = editor.read_with(cx, |editor, cx| editor.capture_document_view(cx));
    assert_eq!(
        restored, beta_view,
        "切回 beta 没有还原它自己的阅读现场（现场必须是按篇存的）"
    );
}

