use super::common::*;

#[gpui::test]
async fn welcome_page_renders_and_dismisses_into_a_new_document(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
    editor.update(cx, |editor, cx| {
        editor.show_welcome = true;
        cx.notify();
    });
    // The welcome overlay must render without panicking.
    cx.update(|window, cx| {
        window.draw(cx).clear();
    });

    // 新建文档 dismisses the page and leaves an editable empty document.
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.on_welcome_new_document(&gpui::ClickEvent::default(), window, cx);
        });
        window.draw(cx).clear();
    });
    editor.read_with(cx, |editor, _cx| {
        assert!(!editor.show_welcome);
    });
}

#[gpui::test]
async fn welcome_page_hides_once_a_document_opens(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let root = std::env::temp_dir().join(format!("velora-welcome-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("create root");
    let doc = root.join("welcome-sample.md");
    std::fs::write(&doc, "# Welcome\n\nBody text.\n").expect("write doc");

    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
    editor.update(cx, |editor, cx| {
        editor.show_welcome = true;
        editor.set_workspace_root(root.clone(), cx);
        cx.notify();
    });
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(doc.clone(), window, cx);
        });
        window.draw(cx).clear();
    });
    editor.read_with(cx, |editor, _cx| {
        assert!(!editor.show_welcome);
        assert_eq!(editor.file_path.as_deref(), Some(doc.as_path()));
    });
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
async fn status_bar_breadcrumb_renders_without_panicking(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let root = std::env::temp_dir().join(format!("velora-crumb-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("create root");
    let doc = root.join("nested").join("note.md");
    std::fs::create_dir_all(doc.parent().unwrap()).expect("create nested");
    std::fs::write(&doc, "# note\n").expect("write");

    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.set_workspace_root(root.clone(), cx);
            editor.open_workspace_file(doc.clone(), window, cx);
        });
    });
    // 渲染含面包屑的状态栏不应 panic。
    cx.update(|window, cx| {
        window.draw(cx).clear();
    });
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(editor.file_path.as_deref(), Some(doc.as_path()));
        // set_workspace_root canonicalizes, so compare against the
        // canonicalized root (/var ↔ /private/var on macOS).
        let canonical = std::fs::canonicalize(&root).expect("canonicalize");
        assert_eq!(editor.workspace_root_path(), Some(canonical.as_path()));
    });
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
async fn many_tabs_never_slide_under_the_window_controls(cx: &mut TestAppContext) {
    // 用户报修：标签开多了，第一个标签一直在左移，最后压到红绿灯下面。
    // 根因是标题行的红绿灯预留位（和应用绘制的窗口按钮）没有 flex_shrink_0，
    // 整行溢出时被 taffy 挤扁。
    init_editor_test_app(cx);
    let root = temp_markdown_path("tab-strip-overflow");
    fs::create_dir_all(&root).unwrap();
    let mut paths = Vec::new();
    for index in 0..12 {
        let path = root.join(format!("doc-with-a-much-longer-name-{index:02}.md"));
        fs::write(&path, format!("# doc {index}\n")).unwrap();
        paths.push(path);
    }
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
    cx.update(|window, _cx| window.resize(gpui::size(px(1615.0), px(900.0))));
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.set_workspace_root(root.clone(), cx);
        });
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(paths[0].clone(), window, cx);
        });
    });
    redraw(cx);
    let first_with_one_tab = cx
        .debug_bounds("document-tab-0")
        .expect("单个标签应渲染在标题行里")
        .origin
        .x;

    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            for path in &paths[1..] {
                editor.open_workspace_file(path.clone(), window, cx);
            }
        });
    });
    redraw(cx);
    let first_with_many = cx.debug_bounds("document-tab-0").expect("首标签仍应存在");
    assert_eq!(
        first_with_many.origin.x, first_with_one_tab,
        "加满标签后第一个标签不该左移（{first_with_one_tab} → {:?}）",
        first_with_many.origin.x
    );
    #[cfg(target_os = "macos")]
    assert!(
        first_with_many.origin.x >= px(84.0),
        "第一个标签不该进入 macOS 红绿灯预留区（84px），实测 {:?}",
        first_with_many.origin.x
    );
    let max_offset = editor.read_with(cx, |editor, _| {
        editor.workspace.tabs_scroll_handle.max_offset()
    });
    assert!(
        max_offset.width > px(0.0),
        "溢出的标签条应该可以横向滚动，实测 max_offset {max_offset:?}"
    );
}

#[gpui::test]
async fn heading_fold_chevron_renders_and_click_toggles_fold(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let markdown = "## Section\n\nalpha\n\n## Empty";
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, markdown.into(), None)
    });
    redraw(cx);

    // 含章节内容的标题在左侧留白渲染 chevron，空章节标题不渲染。
    let bounds = cx
        .debug_bounds("heading-fold-chevron")
        .expect("foldable heading should render a fold chevron");
    assert!(bounds.size.width > px(0.0) && bounds.size.height > px(0.0));

    let (heading, empty_heading) = editor.update(cx, |editor, _cx| {
        let visible = editor.document.visible_blocks().to_vec();
        assert_eq!(visible.len(), 3); // Section, alpha, Empty
        (visible[0].entity.clone(), visible[2].entity.clone())
    });
    assert!(heading.read_with(cx, |block, _cx| block.foldable));
    assert!(!empty_heading.read_with(cx, |block, _cx| block.foldable));

    // 点击 chevron 中心：章节折叠，块级 mouse-down 不改变光标。
    cx.simulate_click(bounds.center(), Modifiers::none());
    redraw(cx);

    editor.update(cx, |editor, cx| {
        assert!(heading.read(cx).folded);
        let filtered = editor
            .apply_heading_fold_filter(cx)
            .iter()
            .map(|&index| {
                editor.document.visible_blocks()[index as usize]
                    .entity
                    .read(cx)
                    .display_text()
                    .to_string()
            })
            .collect::<Vec<_>>();
        assert_eq!(filtered, vec!["Section".to_string(), "Empty".to_string()]);
    });
}
