use super::super::Editor;
use gpui::{
    TestAppContext,
};
use std::fs;
use std::time::Duration;


#[gpui::test]
async fn external_file_events_refresh_the_workspace_tree(cx: &mut TestAppContext) {
    // 审查发现：watcher 只转发 Modify/Create 且从不刷新文件树，外部新建/
    // 删除/改名的文件在树、⌘P、工作区搜索的文件列表里永远是旧的。
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let root = std::env::temp_dir().join(format!(
        "velora-watcher-tree-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&root).unwrap();
    // macOS 上 /var 是 /private/var 的符号链接：set_workspace_root 会把根
    // canonicalize，树里的路径全是 /private/var/...；测试断言用的路径必须
    // 与之同源，否则 path == 断言永远失败。
    let root = std::fs::canonicalize(&root).unwrap_or(root);
    let existing = root.join("a.md");
    fs::write(&existing, "# a\n").unwrap();
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

    // 外部新建：树/文件列表要出现它。
    let added = root.join("b.md");
    fs::write(&added, "# b\n").unwrap();
    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| editor.on_watched_path_changed(&added, cx));
    });
    cx.executor().advance_clock(Duration::from_millis(400));
    cx.run_until_parked();
    editor.read_with(cx, |editor, _| {
        assert!(
            editor.workspace_text_files().iter().any(|path| path == &added),
            "外部新建的文件应出现在工作区文件列表"
        );
    });

    // 外部删除：树/文件列表不能再列出它。
    fs::remove_file(&existing).unwrap();
    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| editor.on_watched_path_changed(&existing, cx));
    });
    cx.executor().advance_clock(Duration::from_millis(400));
    cx.run_until_parked();
    editor.read_with(cx, |editor, _| {
        assert!(
            !editor.workspace_text_files().iter().any(|path| path == &existing),
            "外部删除的文件不应再出现在工作区文件列表"
        );
    });
}

#[gpui::test]
async fn opening_a_single_file_starts_the_workspace_watcher(cx: &mut TestAppContext) {
    // 审查发现：只有打开文件夹才会启动 watcher；只打开一个文件时外部修改
    // 永远不会重载（D3 静默失效）。
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let root = std::env::temp_dir().join(format!(
        "velora-single-file-watch-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&root).unwrap();
    let path = root.join("note.md");
    fs::write(&path, "# note\n").unwrap();
    cx.on_quit({
        let root = root.clone();
        move || {
            let _ = fs::remove_dir_all(root);
        }
    });

    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
    editor.update(cx, |editor, cx| {
        editor.replace_document_from_markdown("# note\n".into(), Some(path.clone()), cx);
    });
    cx.run_until_parked();
    editor.read_with(cx, |editor, _| {
        assert!(editor.workspace.root.is_some(), "打开单文件应隐含工作区根");
        // 测试里不起真实 OS watcher（fd 限制），断言“决定监听哪根”的接缝；
        // 实际的 notify 监听由真实运行验证。
        assert_eq!(
            editor.watched_workspace_root.as_deref(),
            editor.workspace.root.as_deref(),
            "隐含根也必须进入监听状态"
        );
    });
}

#[gpui::test]
async fn backlinks_panel_picks_up_an_external_link_to_the_active_document(
    cx: &mut TestAppContext,
) {
    // 审查发现：反链/标签面板只按 document_revision 失效，别的文件在外部
    // 新增 [[链接]] 时面板一直显示旧结果。
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let root = std::env::temp_dir().join(format!(
        "velora-backlinks-external-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&root).unwrap();
    // 与索引/树产出的 canonical 路径对齐（macOS /var → /private/var）。
    let root = std::fs::canonicalize(&root).unwrap_or(root);
    let active = root.join("a.md");
    let other = root.join("b.md");
    fs::write(&active, "# A\n").unwrap();
    fs::write(&other, "# B\n").unwrap();
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
            editor.open_workspace_file(active.clone(), window, cx);
        });
    });
    cx.run_until_parked();
    editor.update(cx, |editor, cx| editor.refresh_link_panels(cx));
    editor.read_with(cx, |editor, _| {
        assert!(editor.link_panels.backlinks.is_empty(), "前置：还没有反链");
    });

    fs::write(&other, "# B\n\n[[a]]\n").unwrap();
    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| editor.on_watched_path_changed(&other, cx));
    });
    cx.executor().advance_clock(Duration::from_millis(400));
    cx.run_until_parked();
    editor.update(cx, |editor, cx| editor.refresh_link_panels(cx));
    editor.read_with(cx, |editor, _| {
        assert!(
            editor
                .link_panels
                .backlinks
                .iter()
                .any(|path| path == &other),
            "外部新增的 [[a]] 必须出现在反链面板"
        );
    });
}

