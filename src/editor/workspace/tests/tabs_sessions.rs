use super::super::{Editor, WorkspaceOpenMode, WorkspaceSearchScope};
use crate::components::Block;
use gpui::{
    EntityInputHandler, Modifiers, ScrollDelta, ScrollWheelEvent,
    TestAppContext, TouchPhase, point, px,
};
use std::fs;
use std::time::Duration;


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

    // 编辑过的预览标签当场转固定，切走不再被替换（用户需求：动过就不是临时窗口）。
    // 走真实的打字路径（`finish_dirty` 是唯一的转正点），不手改 `tab.dirty`——旧写法
    // 直接置位绕过了转正，把「预览可以带脏」这件不存在的事钉成了规矩。
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file_in_mode(paths[3].clone(), preview, window, cx);
        });
    });
    cx.run_until_parked();
    let block = editor.read_with(cx, |editor, _| {
        editor.document.first_root().expect("a block").clone()
    });
    cx.update(|window, cx| {
        block.update(cx, |block, cx| {
            block.selected_range = 0..0;
            <Block as EntityInputHandler>::replace_text_in_range(block, None, "X", window, cx);
        });
    });
    cx.run_until_parked();
    editor.read_with(cx, |editor, _| {
        let tab = editor
            .workspace
            .open_documents
            .iter()
            .find(|tab| tab.path == paths[3])
            .expect("编辑的这篇应有标签");
        assert!(editor.document_dirty, "前置：打字要留下未保存修改");
        assert!(
            !tab.preview,
            "第一次改动就要把预览标签转成固定，否则切走会被当成干净预览销毁"
        );
    });
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file_in_mode(paths[4].clone(), preview, window, cx);
        });
    });
    editor.read_with(cx, |editor, _| {
        let tabs = &editor.workspace.open_documents;
        assert!(
            tabs.iter().any(|tab| tab.path == paths[3]),
            "已修改（已转固定）的标签切走后应保留"
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

/// 「点开看看」的入口都开成预览标签：搜索结果、⌘P、正文链接连着点也只占一个标签位，
/// 不再越点越多（用户报修）。改过的那一篇在 `finish_dirty` 就地转固定，不会被下一个
/// 预览替换掉。
#[gpui::test]
async fn browsing_entries_keep_a_single_preview_tab(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let root = std::env::temp_dir().join(format!(
        "velora-preview-browsing-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&root).unwrap();
    let root = fs::canonicalize(&root).unwrap_or(root);
    let home = root.join("home.md");
    let alpha = root.join("alpha.md");
    let beta = root.join("beta.md");
    let gamma = root.join("gamma.md");
    for path in [&alpha, &beta, &gamma] {
        fs::write(path, "# 标题\n\n针脚 内容。\n").unwrap();
    }
    fs::write(&home, "# 首页\n\n见 [beta](beta.md)。\n").unwrap();
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
            editor.open_workspace_file(home.clone(), window, cx);
        });
    });
    cx.run_until_parked();
    let tabs_of = |editor: &gpui::Entity<Editor>, cx: &mut gpui::VisualTestContext| {
        editor.read_with(cx, |editor, _| {
            editor
                .workspace
                .open_documents
                .iter()
                .map(|tab| (tab.path.clone(), tab.preview))
                .collect::<Vec<_>>()
        })
    };

    // 1) 工作区搜索：先点 alpha 的命中，再点 beta 的命中——预览位始终只有一个。
    editor.update(cx, |editor, cx| {
        editor.workspace.search_query = "针脚".into();
        editor.workspace.search_scope = WorkspaceSearchScope::Workspace;
        editor.schedule_workspace_search(cx);
    });
    cx.executor().advance_clock(Duration::from_millis(400));
    cx.run_until_parked();
    let (alpha_hit, beta_hit) = editor.read_with(cx, |editor, _| {
        let index = |name: &str| {
            editor
                .workspace
                .search_results
                .iter()
                .position(|hit| {
                    hit.path.file_name().map(|file| file == name).unwrap_or(false)
                        && hit.line == Some(3)
                })
                .expect("a workspace hit")
        };
        (index("alpha.md"), index("beta.md"))
    });
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| editor.open_search_hit(alpha_hit, window, cx));
    });
    cx.run_until_parked();
    assert_eq!(
        tabs_of(&editor, cx),
        vec![(home.clone(), false), (alpha.clone(), true)],
        "搜索结果应开成预览标签，且固定标签留着"
    );
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| editor.open_search_hit(beta_hit, window, cx));
    });
    cx.run_until_parked();
    assert_eq!(
        tabs_of(&editor, cx),
        vec![(home.clone(), false), (beta.clone(), true)],
        "点第二条搜索命中要替换掉那个预览标签，而不是再加一个"
    );

    // 2) ⌘P 回车打开：仍只占那一个预览位。
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| editor.toggle_quick_open(window, cx));
    });
    editor.update_in(cx, |editor, window, cx| {
        editor.replace_text_in_range(None, "gamma.md", window, cx);
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(
        tabs_of(&editor, cx),
        vec![(home.clone(), false), (gamma.clone(), true)],
        "⌘P 打开的也应是预览标签"
    );

    // 3) 正文里点本地链接：同样替换那个预览位。
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(home.clone(), window, cx);
        });
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_link_target("beta.md".into(), window, cx);
        });
    });
    cx.run_until_parked();
    assert_eq!(
        tabs_of(&editor, cx),
        vec![(home.clone(), false), (beta.clone(), true)],
        "正文链接点开的是预览标签；点开 home 也不该把预览位算进去"
    );

    // 4) 预览标签被编辑 → 当场转固定；再点开别的，它留着，预览位换到新那篇。
    let block = editor.read_with(cx, |editor, _| {
        editor.document.first_root().expect("a block").clone()
    });
    cx.update(|window, cx| {
        block.update(cx, |block, cx| {
            block.selected_range = 0..0;
            <Block as EntityInputHandler>::replace_text_in_range(block, None, "X", window, cx);
        });
    });
    cx.run_until_parked();
    editor.read_with(cx, |editor, _| {
        let tab = editor
            .workspace
            .open_documents
            .iter()
            .find(|tab| tab.path == beta)
            .expect("beta 标签");
        assert!(!tab.preview, "编辑过的预览标签要当场转固定");
    });
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file_in_mode(
                alpha.clone(),
                WorkspaceOpenMode::Preview,
                window,
                cx,
            );
        });
    });
    cx.run_until_parked();
    assert_eq!(
        tabs_of(&editor, cx),
        vec![(home.clone(), false), (beta.clone(), false), (alpha.clone(), true)],
        "转过固定的那篇要留下，预览位只跟着当前这篇"
    );
}

/// 未保存的圆点要标在**活动**标签上（用户需求：像 VS Code 那样在标签上点出「这篇改过
/// 还没落盘」）。此前 `(dirty && !active)` 把正在编辑的那篇排除在外，反而是最需要标记
/// 的一个没有标记。打字即亮，自动保存落盘即灭。
#[gpui::test]
async fn the_dirty_dot_marks_the_active_tab(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let root = std::env::temp_dir().join(format!(
        "velora-dirty-dot-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&root).unwrap();
    let root = fs::canonicalize(&root).unwrap_or(root);
    let alpha = root.join("alpha.md");
    fs::write(&alpha, "# alpha\n").unwrap();
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
    });
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(alpha.clone(), window, cx);
        });
    });
    cx.update(|window, cx| window.draw(cx).clear());
    assert!(
        cx.debug_bounds("document-tab-dirty-0").is_none(),
        "干净的标签不该点"
    );

    // 打一个字：脏了，而自动保存的防抖还没到点。
    let block = editor.read_with(cx, |editor, _| {
        editor.document.first_root().expect("a block").clone()
    });
    cx.update(|window, cx| {
        block.update(cx, |block, cx| {
            block.selected_range = 0..0;
            <Block as EntityInputHandler>::replace_text_in_range(block, None, "X", window, cx);
        });
    });
    editor.read_with(cx, |editor, _| {
        assert!(editor.document_dirty, "前置：打字要留下未保存修改");
    });
    cx.update(|window, cx| window.draw(cx).clear());
    assert!(
        cx.debug_bounds("document-tab-dirty-0").is_some(),
        "活动标签改了字要在标签上点出圆点"
    );

    // 自动保存落盘：圆点该跟着灭掉。
    cx.executor().advance_clock(Duration::from_millis(1_200));
    cx.run_until_parked();
    editor.read_with(cx, |editor, _| {
        assert!(!editor.document_dirty, "前置：防抖到点后要已落盘");
    });
    cx.update(|window, cx| window.draw(cx).clear());
    assert!(
        cx.debug_bounds("document-tab-dirty-0").is_none(),
        "落盘之后圆点要消失"
    );
    assert_eq!(
        fs::read_to_string(&alpha).unwrap(),
        // 渲染态下标题的 content 起点在 `# ` 之后：块内偏移 0 就是那个「a」。
        "# Xalpha\n",
        "圆点灭掉要对应真落盘，不是标记被擦掉"
    );
}

#[gpui::test]
async fn the_close_tab_command_closes_the_active_tab(cx: &mut TestAppContext) {
    // 用户要求：cmd/ctrl-w 关当前标签页（浏览器肌肉记忆）。此前 ctrl-w 挂在
    // 「切换侧边栏」上，误按就把侧边栏收起来（用户报修「经常错误触发」）。
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let root = std::env::temp_dir().join(format!("velora-close-tab-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let alpha = root.join("alpha.md");
    let beta = root.join("beta.md");
    fs::write(&alpha, "# alpha\n").unwrap();
    fs::write(&beta, "# beta\n").unwrap();
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
            editor.open_workspace_file(alpha.clone(), window, cx);
            editor.open_workspace_file(beta.clone(), window, cx);
        });
    });
    editor.read_with(cx, |editor, _| {
        assert_eq!(editor.workspace.open_documents.len(), 2, "前置：应有两个标签");
        assert_eq!(editor.file_path.as_deref(), Some(beta.as_path()), "前置：活动页是 beta");
    });

    // 命令派发（键位与菜单都走这一条）：活动页 beta 关掉，alpha 留着。
    cx.dispatch_action(crate::components::CloseTab);
    cx.update(|window, cx| window.draw(cx).clear());
    editor.read_with(cx, |editor, _| {
        let open = editor.workspace.open_documents.clone();
        assert_eq!(open.len(), 1, "应只剩一个标签，实测 {open:?}");
        assert_eq!(
            editor.file_path.as_deref(),
            Some(alpha.as_path()),
            "关掉的应是活动页 beta，留下的是 alpha"
        );
    });

    // 最后一个标签也照样关（和浏览器一致），回到欢迎页；窗口不关。
    cx.dispatch_action(crate::components::CloseTab);
    cx.update(|window, cx| window.draw(cx).clear());
    editor.read_with(cx, |editor, _| {
        assert!(editor.workspace.open_documents.is_empty(), "最后一个标签也该关掉");
        assert!(editor.show_welcome, "没有标签页时应回到欢迎页");
    });

    // 已经没页可关时是空操作，不该报错也不该关窗口。
    cx.dispatch_action(crate::components::CloseTab);
    cx.update(|window, cx| window.draw(cx).clear());
    editor.read_with(cx, |editor, _| {
        assert!(editor.workspace.open_documents.is_empty());
        assert!(editor.file_path.is_none());
    });
}
