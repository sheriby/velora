use super::common::*;

#[gpui::test]
async fn native_about_menu_action_opens_dialog_after_window_event(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
    editor.update(cx, |editor, cx| {
        editor.show_welcome = true;
        cx.notify();
    });
    redraw(cx);
    cx.update(|window, cx| {
        window.activate_window();
        crate::app_menu::dispatch_menu_action(&crate::components::ShowAbout, cx);
    });
    cx.run_until_parked();
    assert!(
        editor.read_with(cx, |editor, _| matches!(
            editor.info_dialog,
            Some(crate::editor::InfoDialogKind::About)
        )),
        "原生菜单事件结束后应显示关于弹层"
    );
}

#[gpui::test]
async fn native_update_menu_action_opens_dialog_after_window_event(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    cx.update(|cx| {
        let mut settings = crate::config::preferences::UpdatePreferences::default();
        settings.check_on_startup = false;
        crate::config::EditorSettings::set_updates_in_memory(cx, settings);
    });
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
    editor.update(cx, |editor, _| editor.update_check_in_progress = true);
    redraw(cx);
    let dispatched = cx.update(|window, cx| {
        window.activate_window();
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::app_menu::dispatch_menu_action(&crate::components::CheckForUpdates, cx);
        }))
        .is_ok()
    });
    assert!(dispatched, "原生菜单派发期间不能重读已借出的活动窗口");
    cx.run_until_parked();
    assert!(editor.read_with(cx, |editor, _| matches!(
        editor.info_dialog,
        Some(crate::editor::InfoDialogKind::CheckForUpdates)
    )), "事件结束后应显示检查更新弹层");
}

struct HamburgerMenuHarness {
    editor: gpui::Entity<Editor>,
    menus: Vec<gpui::OwnedMenu>,
}

impl gpui::Render for HamburgerMenuHarness {
    fn render(
        &mut self,
        _window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        use gpui::{ParentElement, Styled};
        let theme = cx.global::<ThemeManager>().current().clone();
        let panel = self.editor.update(cx, |editor, cx| {
            editor.render_hamburger_menu_panel(&theme, cx, &self.menus, 36.0)
        });
        gpui::div().size_full().children(panel)
    }
}

#[gpui::test]
async fn windows_hamburger_menu_root_actions_open_settings_and_update_dialog(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    let editor = cx.new(|cx| Editor::from_markdown(cx, String::new(), None));
    editor.update(cx, |editor, _| {
        editor.hamburger_menu_open = true;
        editor.update_check_in_progress = true;
    });
    let menus = vec![gpui::OwnedMenu {
        name: "File".into(),
        items: Vec::new(),
    }];
    let (_, cx) = cx.add_window_view(|_, _| HamburgerMenuHarness {
        editor: editor.clone(),
        menus,
    });
    redraw(cx);
    let preferences = cx.debug_bounds("app-hamburger-preferences");
    let updates = cx.debug_bounds("app-hamburger-check_updates");
    assert!(
        preferences.is_some() && updates.is_some(),
        "偏好设置和检查更新应直接显示在一级列表"
    );
    cx.simulate_click(updates.unwrap().center(), Modifiers::none());
    redraw(cx);
    assert!(editor.read_with(cx, |editor, _| matches!(
        editor.info_dialog,
        Some(crate::editor::InfoDialogKind::CheckForUpdates)
    )));
    editor.update(cx, |editor, cx| {
        assert!(!editor.hamburger_menu_open, "执行操作应关闭菜单");
        editor.hamburger_menu_open = true;
        cx.notify();
    });
    redraw(cx);
    let preferences = cx
        .debug_bounds("app-hamburger-preferences")
        .expect("偏好设置入口");
    cx.simulate_click(preferences.center(), Modifiers::none());
    cx.run_until_parked();
    assert!(
        cx.update(|_, cx| cx.windows().iter().any(|window| window
            .downcast::<crate::config::preferences::PreferencesWindow>()
            .is_some())),
        "一级偏好设置按钮应打开设置窗口"
    );
}

#[gpui::test]
async fn welcome_page_keeps_two_primary_buttons_and_dispatches_open_action(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    let actions = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    cx.update(|cx| {
        cx.on_action({
            let actions = actions.clone();
            move |_: &crate::components::OpenFile, _| actions.borrow_mut().push("file")
        });
    });
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
    editor.update(cx, |editor, cx| {
        editor.show_welcome = true;
        cx.notify();
    });
    redraw(cx);
    let file = cx.debug_bounds("welcome-open-file");
    assert!(file.is_some(), "欢迎页应提供打开入口");
    let new_document = cx
        .debug_bounds("welcome-new-document")
        .expect("新建文档入口");
    assert_eq!(
        file.unwrap().top(),
        new_document.top(),
        "保留两个主按钮并排的布局"
    );
    assert!(
        cx.debug_bounds("welcome-open-folder").is_none(),
        "欢迎页不增加第三个按钮"
    );
    cx.simulate_click(file.unwrap().center(), Modifiers::none());
    cx.run_until_parked();
    assert_eq!(
        *actions.borrow(),
        vec!["file"],
        "打开按钮应走既有原生打开动作"
    );
}

#[gpui::test]
async fn welcome_recent_folders_show_paths_even_for_system_temp_root(cx: &mut TestAppContext) {
    let root = temp_fixture_dir().join(format!("welcome-paths-{}", temp_fixture_token()));
    let _config = crate::config::override_test_config_root(root.join("config"));
    crate::config::record_recent_folder(&std::env::temp_dir()).expect("记录临时目录");
    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
    editor.update(cx, |editor, cx| {
        editor.show_welcome = true;
        cx.notify();
    });
    redraw(cx);
    assert!(
        cx.debug_bounds("welcome-recent-path-0").is_some(),
        "目录不能只显示 T 这样的末级名称，应保留记录并展示路径"
    );
    fs::remove_dir_all(root).expect("清理路径展示夹具");
}

#[gpui::test]
async fn welcome_recent_remove_only_forgets_history_without_opening_or_deleting(
    cx: &mut TestAppContext,
) {
    let root = temp_fixture_dir().join(format!("welcome-remove-{}", temp_fixture_token()));
    let _config = crate::config::override_test_config_root(root.join("config"));
    let kept = root.join("kept");
    let removed = root.join("removed");
    fs::create_dir_all(&kept).expect("建立保留目录");
    fs::create_dir_all(&removed).expect("建立移除目录");
    fs::write(removed.join("note.md"), "保留正文").expect("写入目录内容");
    crate::config::record_recent_folder(&kept).expect("记录保留目录");
    crate::config::record_recent_folder(&removed).expect("记录移除目录");
    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
    editor.update(cx, |editor, cx| {
        editor.show_welcome = true;
        cx.notify();
    });
    redraw(cx);
    let remove = cx.debug_bounds("welcome-recent-remove-0");
    assert!(remove.is_some(), "每条最近记录的右侧应有移除按钮");
    cx.simulate_click(remove.unwrap().center(), Modifiers::none());
    redraw(cx);
    assert_eq!(
        crate::config::read_recent_folders().expect("重新读取历史"),
        vec![kept.clone()]
    );
    assert_eq!(
        fs::read_to_string(removed.join("note.md")).expect("实际文件仍存在"),
        "保留正文"
    );
    editor.read_with(cx, |editor, _| {
        assert!(editor.show_welcome, "点击移除不能触发行的打开操作");
        assert!(editor.workspace.root.is_none());
        assert_eq!(editor.buffer.text(), "");
    });
    assert!(
        cx.debug_bounds("welcome-recent-remove-1").is_none(),
        "列表应立即刷新"
    );
    let remove_last = cx
        .debug_bounds("welcome-recent-remove-0")
        .expect("保留项也可移除");
    cx.simulate_click(remove_last.center(), Modifiers::none());
    redraw(cx);
    assert!(
        crate::config::read_recent_folders()
            .expect("读取清空历史")
            .is_empty()
    );
    assert!(
        kept.is_dir() && removed.is_dir(),
        "移除最后一条也不能删除目录"
    );
    assert!(cx.debug_bounds("welcome-recent-remove-0").is_none());
    fs::remove_dir_all(root).expect("清理历史移除夹具");
}

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

/// 状态栏自定义按钮配的 `action_id` 必须真的接上派发：点一下等价于按那条命令的键位。
///
/// 现象：`config.toml` 里配了 `{ id=…, label=…, action_id="select_document" }`，重启后
/// 状态栏出现该按钮（`.cursor_pointer()` 明示可点），点击毫无反应。根因：607f692 把按钮
/// 接进了运行时镜像与渲染，派发那一半没接——`action_id` 这个字段在产品代码里零消费。
#[gpui::test]
async fn status_bar_custom_button_dispatches_its_registry_action(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "alpha one\n\nbeta two\n".to_string(), None)
    });
    redraw(cx);

    let dispatched = editor.update_in(cx, |editor, window, cx| {
        editor.dispatch_status_bar_command("select_document", window, cx)
    });
    redraw(cx);

    assert!(dispatched, "注册表里有这条 id，按钮就该派发它");
    let selected = editor.read_with(cx, |editor, cx| editor.selected_visible_text(cx));
    assert!(
        editor.read_with(cx, |editor, _| editor.cross_block_selection.is_some()),
        "派发之后该有跨块选区"
    );
    // 选了多少由选区那一层判断（块间隙不重复算空行），这里只认「两块都在选区里」。
    let selected = selected.expect("全选应当返回可见文本");
    assert!(
        selected.contains("alpha one") && selected.contains("beta two"),
        "两块正文都该在选区里：{selected:?}"
    );

    // 对照：乱配的 id 判为查不到，不派发也不崩。
    let unknown = editor.update_in(cx, |editor, window, cx| {
        editor.dispatch_status_bar_command("no_such_command", window, cx)
    });
    assert!(!unknown, "查不到的 action_id 不该被当成派发成功");
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
        let canonical = crate::file_url::canonical_path(&root);
        assert_eq!(editor.workspace_root_path(), Some(canonical.as_path()));
    });
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
async fn many_tabs_never_slide_under_the_window_controls(cx: &mut TestAppContext) {
    // 标签开多了也不许让第一个标签左移、压到红绿灯下面：标题行的红绿灯预留位
    // （和应用绘制的窗口按钮）必须 flex_shrink_0，否则整行溢出时会被 taffy 挤扁。
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

#[gpui::test]
fn titlebar_blank_space_after_tabs_can_start_a_window_drag(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let root = temp_markdown_path("titlebar-blank-drag");
    fs::create_dir_all(&root).unwrap();
    cx.on_quit({
        let root = root.clone();
        move || fs::remove_dir_all(root).unwrap()
    });
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
    cx.simulate_resize(gpui::size(px(1200.0), px(800.0)));
    editor.update_in(cx, |editor, window, cx| {
        for index in 0..3 {
            let path = root.join(format!("document-with-a-long-visible-title-{index}.md"));
            fs::write(&path, "正文\n").unwrap();
            editor.open_workspace_file(path, window, cx);
        }
    });
    redraw(cx);
    redraw(cx);
    for grouped in [false, true] {
        if grouped {
            editor.update_in(cx, |editor, window, cx| {
                editor.split_active_document(
                    crate::editor::workspace::split::SplitDirection::Right,
                    window,
                    cx,
                );
            });
            redraw(cx);
            redraw(cx);
            let groups = editor.read_with(cx, |editor, _| editor.split_groups());
            let left = groups.first().expect("左组").clone();
            let right = groups.get(1).expect("右组").clone();
            let active = right.read_with(cx, |editor, _| editor.file_path.clone().unwrap());
            editor.update_in(cx, |editor, window, cx| {
                editor.transfer_split_tab(
                    right.downgrade(),
                    left.entity_id(),
                    active,
                    None,
                    false,
                    window,
                    cx,
                );
            });
            redraw(cx);
            redraw(cx);
            assert_eq!(
                editor.read_with(cx, |editor, _| editor.split_groups().len()),
                1
            );
        }
        let last = cx.debug_bounds("document-tab-2").expect("最后一个标签");
        let filler = cx
            .debug_bounds("window-titlebar-drag-filler")
            .expect("标题栏拖动区");
        let point = gpui::point(last.right() + px(16.0), filler.center().y);
        assert!(point.x < filler.right(), "夹具必须留下标签后的空白");
        cx.simulate_mouse_move(point, None, Modifiers::none());
        assert_eq!(
            cx.hit_test_window_control(),
            Some(gpui::WindowControlArea::Drag),
            "标签后的空白应全部可拖窗，最后标签 {last:?}，右侧拖动区 {filler:?}"
        );
        #[cfg(target_os = "macos")]
        {
            let before = cx.window_move_requests();
            cx.simulate_mouse_down(point, gpui::MouseButton::Left, Modifiers::none());
            assert_eq!(
                cx.window_move_requests(),
                before + 1,
                "macOS 空白处应实际发起拖窗"
            );
            cx.simulate_mouse_up(point, gpui::MouseButton::Left, Modifiers::none());
        }
    }
}

#[gpui::test]
fn titlebar_keeps_a_drag_area_when_tabs_overflow(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let root = temp_markdown_path("titlebar-reserved-drag");
    fs::create_dir_all(&root).unwrap();
    cx.on_quit({
        let root = root.clone();
        move || fs::remove_dir_all(root).unwrap()
    });
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
    cx.simulate_resize(gpui::size(px(1000.0), px(800.0)));
    editor.update_in(cx, |editor, window, cx| {
        for index in 0..14 {
            let path = root.join(format!("long-document-name-{index}.md"));
            fs::write(&path, "正文\n").unwrap();
            editor.open_workspace_file(path, window, cx);
        }
    });
    redraw(cx);
    redraw(cx);
    let filler = cx
        .debug_bounds("window-titlebar-drag-filler")
        .expect("固定拖动区");
    assert!(
        filler.size.width >= px(40.0),
        "标签占满仍须留下至少 40px 拖动区，实际 {filler:?}"
    );
    assert!(filler.right() <= px(1000.0), "固定拖动区不能溢出窗口");
    cx.simulate_mouse_move(filler.center(), None, Modifiers::none());
    assert_eq!(
        cx.hit_test_window_control(),
        Some(gpui::WindowControlArea::Drag)
    );
    assert!(
        editor.read_with(cx, |editor, _| editor
            .workspace
            .tabs_scroll_handle
            .max_offset()
            .width
            > px(0.0)),
        "其余标签应继续横向滚动"
    );
}

#[gpui::test]
fn titlebar_blank_space_between_groups_can_start_a_window_drag(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let path = temp_markdown_path("titlebar-group-gap-drag");
    fs::write(&path, "正文\n").unwrap();
    cx.on_quit({
        let path = path.clone();
        move || fs::remove_file(path).unwrap()
    });
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
    cx.simulate_resize(gpui::size(px(1200.0), px(800.0)));
    editor.update_in(cx, |editor, window, cx| {
        editor.open_workspace_file(path, window, cx);
        editor.split_active_document(
            crate::editor::workspace::split::SplitDirection::Right,
            window,
            cx,
        );
    });
    redraw(cx);
    redraw(cx);
    let gap = cx
        .debug_bounds("tab-group-gap-1")
        .expect("标签分组之间的空白");
    cx.simulate_mouse_move(gap.center(), None, Modifiers::none());
    assert_eq!(
        cx.hit_test_window_control(),
        Some(gpui::WindowControlArea::Drag),
        "分组之间的空白也应属于拖窗区域"
    );
    #[cfg(target_os = "macos")]
    {
        let before = cx.window_move_requests();
        cx.simulate_mouse_down(gap.center(), gpui::MouseButton::Left, Modifiers::none());
        assert_eq!(cx.window_move_requests(), before + 1);
        cx.simulate_mouse_up(gap.center(), gpui::MouseButton::Left, Modifiers::none());
    }
}
