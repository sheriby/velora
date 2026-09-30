use super::common::*;

#[test]
fn about_dialog_body_lines_use_velora_brand_and_repository_link() {
    let strings = I18nStrings::zh_cn();
    let lines = Editor::about_dialog_body_lines(&strings);

    assert_eq!(lines[0], format!("Velora {}", env!("CARGO_PKG_VERSION")));
    assert_eq!(
        lines[2],
        format!("项目仓库: {}", crate::editor::render::ABOUT_GITHUB_URL)
    );
    assert_eq!(lines[3], "第三方来源与许可信息见项目文档。");
}

#[gpui::test]
async fn about_github_link_uses_gpui_url_opening(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::editor::render::open_about_github_url(cx);
    });

    assert_eq!(
        cx.opened_url(),
        Some(crate::editor::render::ABOUT_GITHUB_URL.to_string())
    );
}

/// 窗口 frame 用例需要独占配置目录：关闭/退出窗口的用例都会往 config.toml
/// 写 frame，共用进程级目录时并行执行会互相覆盖（实测会让断言读到别的用例的 frame）。
fn isolated_window_frame_config(test_name: &str) -> (PathBuf, crate::config::TestConfigRootGuard) {
    let root = std::env::temp_dir().join(format!(
        "velora-{test_name}-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let guard = crate::config::override_test_config_root(&root);
    (root, guard)
}

#[gpui::test]
async fn quitting_the_app_remembers_each_window_frame(cx: &mut TestAppContext) {
    let (root, _root_guard) = isolated_window_frame_config("window-frame-quit");
    // roadmap A2：⌘Q 也必须记住窗口位置与大小。此前只有关闭单窗口才落盘，
    // 「调完位置直接退出」会把调整丢掉（用户报修）。先放一个哨兵 frame，
    // 用来区分「退出路径没写盘」与「写盘写对了」。
    init_editor_test_app(cx);
    crate::config::store_window_frame(crate::config::WindowFrame {
        x: 11,
        y: 13,
        width: 1111,
        height: 777,
    })
    .expect("seed sentinel frame");

    let (_editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
    cx.update(|window, _cx| window.resize(gpui::size(px(1200.0), px(820.0))));
    redraw(cx);

    // 走用户真实路径：⌘Q 在窗口内派发，窗口正处于借用状态。
    cx.dispatch_action(QuitApplication);
    cx.run_until_parked();

    let stored = crate::config::saved_window_frame()
        .expect("read window frame")
        .expect("quitting should store the window frame");
    assert_ne!(
        (stored.width, stored.height),
        (1111, 777),
        "退出路径没有落盘：读到的还是哨兵 frame"
    );
    assert_eq!(
        (stored.width, stored.height),
        (1200, 820),
        "退出时应记住退出前的窗口尺寸"
    );
    let _ = fs::remove_dir_all(root);
}

#[gpui::test]
async fn platform_close_remembers_the_window_frame(cx: &mut TestAppContext) {
    let (root, _root_guard) = isolated_window_frame_config("window-frame-platform-close");
    // 平台自己发起的关闭（macOS 红灯）不经过应用内任何关闭入口，
    // 只有 on_window_should_close 能在窗口还活着时落盘（用户报修场景）。
    init_editor_test_app(cx);
    crate::config::store_window_frame(crate::config::WindowFrame {
        x: 3,
        y: 5,
        width: 999,
        height: 666,
    })
    .expect("seed sentinel frame");

    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
    cx.update(|window, _cx| window.resize(gpui::size(px(1280.0), px(860.0))));
    redraw(cx);

    let allowed = cx.update(|window, cx| {
        editor
            .clone()
            .update(cx, |editor, cx| editor.on_window_should_close(window, cx))
    });
    assert!(allowed, "干净文档应允许平台关闭窗口");

    let stored = crate::config::saved_window_frame()
        .expect("read window frame")
        .expect("平台关闭路径应落盘窗口 frame");
    assert_eq!(
        (stored.width, stored.height),
        (1280, 860),
        "平台关闭前应记住当时的窗口尺寸"
    );
    let _ = fs::remove_dir_all(root);
}

#[gpui::test]
async fn resizing_the_window_records_the_frame_without_quitting(cx: &mut TestAppContext) {
    let (root, _root_guard) = isolated_window_frame_config("window-frame-resize");
    // 窗口位置/大小不能只在关窗/退出时落盘：强杀进程、平台关闭回调缺位、或调完
    // 窗口程序就崩，最后一次调整就丢了。窗口一动（bounds 变化）就该记住。
    init_editor_test_app(cx);
    cx.update(|cx| crate::config::EditorSettings::init(cx, true));
    crate::config::store_window_frame(crate::config::WindowFrame {
        x: 10,
        y: 20,
        width: 900,
        height: 600,
    })
    .expect("seed frame");

    // 走真实开窗路径（open_editor_window 里装监听），不关窗、不退出。
    let editor = cx.update(|cx| crate::app_menu::open_editor_window(cx, String::new(), None));
    cx.run_until_parked();
    cx.simulate_window_resize(editor.into(), gpui::size(px(1320.0), px(880.0)));
    // 防抖窗口过后才落盘。
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(700));
    cx.run_until_parked();

    let stored = crate::config::saved_window_frame()
        .expect("read window frame")
        .expect("窗口刚被缩放，frame 就应该已经落盘");
    assert_eq!(
        (stored.width, stored.height),
        (1320, 880),
        "缩放窗口后应立刻记住新尺寸，不依赖退出路径"
    );
    let _ = fs::remove_dir_all(root);
}

#[gpui::test]
async fn window_open_position_setting_controls_how_windows_open(cx: &mut TestAppContext) {
    let (root, _root_guard) = isolated_window_frame_config("window-open-position");
    // 锁定设置语义：「记住上次位置」恢复 frame 的位置+大小；「居中打开」只把
    // 位置居中，大小仍用记住的 frame；「默认窗口尺寸」只在没有记住 frame 时生效。
    init_editor_test_app(cx);
    crate::config::store_window_frame(crate::config::WindowFrame {
        x: 40,
        y: 60,
        width: 1000,
        height: 700,
    })
    .expect("seed frame");
    cx.update(|cx| crate::config::EditorSettings::init(cx, true));

    cx.update(|cx| {
        crate::config::EditorSettings::set_window_open_position(
            cx,
            crate::config::WindowOpenPosition::Remember,
        );
    });
    let remembered = cx.update(|cx| crate::app_menu::open_editor_window(cx, String::new(), None));
    cx.run_until_parked();
    assert_eq!(
        windowed_rect(&remembered, cx),
        (40, 60, 1000, 700),
        "打开位置=记住上次位置 时应恢复记住的 frame"
    );

    cx.update(|cx| {
        crate::config::EditorSettings::set_window_open_position(
            cx,
            crate::config::WindowOpenPosition::Center,
        );
    });
    let centered = cx.update(|cx| crate::app_menu::open_editor_window(cx, String::new(), None));
    cx.run_until_parked();
    // 测试平台主屏 1920×1080，记住的 frame 1000×700 → 居中原点 (460, 190)。
    assert_eq!(
        windowed_rect(&centered, cx),
        (460, 190, 1000, 700),
        "打开位置=居中打开 时只居中位置，大小用记住的 frame"
    );
    let _ = fs::remove_dir_all(root);

    // 没有记住 frame 时，「居中打开」才用「默认窗口尺寸」（1080×720 → (420, 180)）。
    let (empty_root, _empty_root_guard) = isolated_window_frame_config("window-open-position-empty");
    let fallback = cx.update(|cx| crate::app_menu::open_editor_window(cx, String::new(), None));
    cx.run_until_parked();
    assert_eq!(
        windowed_rect(&fallback, cx),
        (420, 180, 1080, 720),
        "没有记住的 frame 时按默认窗口尺寸居中"
    );
    let _ = fs::remove_dir_all(empty_root);
}

#[gpui::test]
async fn window_frame_from_a_missing_display_keeps_its_size(cx: &mut TestAppContext) {
    let (root, _root_guard) = isolated_window_frame_config("window-frame-missing-display");
    // 副屏拔掉/分辨率变小后，记住的 frame 中心点落在任何显示器之外。gpui 的
    // Windows 后端遇到这种 frame 会把整块 bounds（连大小）换成显示器默认值——
    // 表现就是「无论上次多大，打开永远默认大小」。应用层必须把它挪回一块屏上，
    // 且尺寸照旧。
    init_editor_test_app(cx);
    cx.update(|cx| crate::config::EditorSettings::init(cx, true));
    crate::config::store_window_frame(crate::config::WindowFrame {
        x: 2600,
        y: 300,
        width: 1400,
        height: 900,
    })
    .expect("seed frame");

    let handle = cx.update(|cx| crate::app_menu::open_editor_window(cx, String::new(), None));
    cx.run_until_parked();

    // 测试主屏 1920×1080：窗口能整块放下 → 搬进屏内 (520, 180)，尺寸不变。
    assert_eq!(
        windowed_rect(&handle, cx),
        (520, 180, 1400, 900),
        "frame 不在任何显示器上时应挪回主屏，并保留记住的尺寸"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn every_editor_window_removal_remembers_the_frame() {
    // 用户报修：调整窗口位置/大小后关闭、下一次启动又回到旧位置。
    // 窗口移除统一走 Editor::close_editor_window（先落盘再移除），
    // 这条守卫挡住「新写一条关闭路径时忘了记 frame」。
    assert_eq!(
        include_str!("../close.rs")
            .matches("window.remove_window()")
            .count(),
        1,
        "窗口移除应只在 Editor::close_editor_window 里发生"
    );
    for (name, source) in [
        ("persistence.rs", include_str!("../persistence.rs")),
        ("workspace.rs", include_str!("../workspace.rs")),
        ("window_state.rs", include_str!("../window_state.rs")),
        ("events.rs", include_str!("../events.rs")),
        ("file_drop.rs", include_str!("../file_drop.rs")),
        ("render.rs", include_str!("../render.rs")),
    ] {
        assert_eq!(
            source.matches("remove_window()").count(),
            0,
            "{name} 里移除窗口应改用 Editor::close_editor_window"
        );
    }
}

#[test]
fn every_svg_icon_sets_its_own_text_color() {
    // 状态栏的源码切换按钮换成 svg 图标后整块看不见：因为
    // gpui 的 svg 元素只读自身 style.text.color，父容器 text_color 不继承
    // （docs/architecture/overview.md §GPUI 限制）：漏设 ⇒ 一个像素都不画。
    for (name, source) in [
        ("status_bar.rs", include_str!("../status_bar.rs")),
        ("workspace.rs", include_str!("../workspace.rs")),
        ("window_chrome.rs", include_str!("../../window_chrome.rs")),
        (
            "components/block/render.rs",
            include_str!("../../components/block/render.rs"),
        ),
    ] {
        for (index, chain) in source.split("svg()").skip(1).enumerate() {
            let chain = &chain[..chain.find(';').unwrap_or(chain.len())];
            assert!(
                chain.contains(".text_color("),
                "{name} 第 {} 处 svg() 没有自己的 .text_color：\
                 gpui 不从父容器继承文字色，图标会整块不渲染",
                index + 1
            );
        }
    }
}

#[gpui::test]
async fn close_window_menu_action_closes_only_active_editor_window(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let (_first_editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "first".to_string(), None));
    let first_window = activate_visual_window(cx);

    let (_second_editor, cx) = cx
        .cx
        .add_window_view(|_window, cx| Editor::from_markdown(cx, "second".to_string(), None));
    let second_window = activate_visual_window(cx);

    assert_ne!(first_window.window_id(), second_window.window_id());
    assert_eq!(cx.cx.windows().len(), 2);

    cx.cx.update(|cx| {
        crate::app_menu::dispatch_menu_action(&CloseWindow, cx);
    });
    cx.run_until_parked();

    let remaining = cx.cx.windows();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].window_id(), first_window.window_id());
    assert_ne!(remaining[0].window_id(), second_window.window_id());
}

#[gpui::test]
async fn app_menu_opened_windows_activate_and_close_independently(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let first_window =
        cx.update(|cx| crate::app_menu::open_editor_window(cx, "first".to_string(), None));
    cx.run_until_parked();
    let second_window =
        cx.update(|cx| crate::app_menu::open_editor_window(cx, "second".to_string(), None));
    cx.run_until_parked();

    let active_window = cx.update(|cx| cx.active_window().expect("window should be active"));
    assert_eq!(active_window.window_id(), second_window.window_id());
    assert_ne!(first_window.window_id(), second_window.window_id());
    assert_eq!(cx.update(|cx| cx.windows().len()), 2);

    assert!(
        second_window
            .update(cx, |editor, _window, _cx| editor.close_guard_installed)
            .expect("second editor window should be open")
    );

    cx.update(|cx| {
        crate::app_menu::dispatch_menu_action(&CloseWindow, cx);
    });
    cx.run_until_parked();

    let remaining = cx.update(|cx| cx.windows());
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].window_id(), first_window.window_id());

    cx.update(|cx| {
        crate::app_menu::dispatch_menu_action(&CloseWindow, cx);
    });
    cx.run_until_parked();

    assert!(cx.update(|cx| cx.windows().is_empty()));
}

#[gpui::test]
async fn app_menu_opened_file_window_reinstalls_close_guard_after_registration(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);

    let opened_path = temp_markdown_path("app-menu-opened-file-window-close");
    fs::write(&opened_path, "opened from file").expect("write opened markdown");

    let first_window =
        cx.update(|cx| crate::app_menu::open_editor_window(cx, "first".to_string(), None));
    cx.run_until_parked();
    let second_window = cx.update(|cx| {
        crate::app_menu::open_editor_window(
            cx,
            fs::read_to_string(&opened_path).expect("read opened markdown"),
            Some(opened_path.clone()),
        )
    });
    cx.run_until_parked();

    let active_window = cx.update(|cx| cx.active_window().expect("window should be active"));
    assert_eq!(active_window.window_id(), second_window.window_id());
    assert_ne!(first_window.window_id(), second_window.window_id());

    second_window
        .update(cx, |editor, window, cx| {
            assert!(editor.close_guard_installed);
            assert!(editor.on_window_should_close(window, cx));
        })
        .expect("second editor window should be open");

    cx.update(|cx| {
        crate::app_menu::dispatch_menu_action(&CloseWindow, cx);
    });
    cx.run_until_parked();

    let remaining = cx.update(|cx| cx.windows());
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].window_id(), first_window.window_id());
    assert_ne!(remaining[0].window_id(), second_window.window_id());

    let _ = fs::remove_file(opened_path);
}

#[gpui::test]
async fn app_menu_opened_dirty_file_window_prompts_only_that_window(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let opened_path = temp_markdown_path("app-menu-opened-dirty-file-window-close");
    fs::write(&opened_path, "opened from file").expect("write opened markdown");

    let first_window =
        cx.update(|cx| crate::app_menu::open_editor_window(cx, "first".to_string(), None));
    let second_window = cx.update(|cx| {
        crate::app_menu::open_editor_window(
            cx,
            fs::read_to_string(&opened_path).expect("read opened markdown"),
            Some(opened_path.clone()),
        )
    });
    cx.run_until_parked();

    second_window
        .update(cx, |editor, window, cx| {
            editor.mark_dirty(cx);
            assert!(!editor.on_window_should_close(window, cx));
        })
        .expect("second editor window should be open");

    first_window
        .update(cx, |editor, _window, _cx| {
            assert!(!editor.show_unsaved_changes_dialog);
        })
        .expect("first editor window should be open");
    second_window
        .update(cx, |editor, _window, _cx| {
            assert!(editor.show_unsaved_changes_dialog);
        })
        .expect("second editor window should be open");

    let _ = fs::remove_file(opened_path);
}

#[gpui::test]
async fn app_menu_opened_dirty_window_close_guard_prompts_only_that_window(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);

    let first_window =
        cx.update(|cx| crate::app_menu::open_editor_window(cx, "first".to_string(), None));
    let second_window =
        cx.update(|cx| crate::app_menu::open_editor_window(cx, "second".to_string(), None));
    cx.run_until_parked();

    second_window
        .update(cx, |editor, window, cx| {
            editor.mark_dirty(cx);
            assert!(!editor.on_window_should_close(window, cx));
        })
        .expect("second editor window should be open");

    first_window
        .update(cx, |editor, _window, _cx| {
            assert!(!editor.show_unsaved_changes_dialog);
        })
        .expect("first editor window should be open");
    second_window
        .update(cx, |editor, _window, _cx| {
            assert!(editor.show_unsaved_changes_dialog);
        })
        .expect("second editor window should be open");
}

#[gpui::test]
async fn quit_application_allows_clean_editor_windows_to_quit(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let (first_editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "first".to_string(), None));
    let _first_window = activate_visual_window(cx);

    let (second_editor, cx) = cx
        .cx
        .add_window_view(|_window, cx| Editor::from_markdown(cx, "second".to_string(), None));
    let _second_window = activate_visual_window(cx);

    assert_eq!(cx.cx.windows().len(), 2);

    cx.cx.update(|cx| {
        crate::app_menu::dispatch_menu_action(&QuitApplication, cx);
    });
    cx.run_until_parked();

    first_editor.read_with(cx, |editor, _cx| {
        assert!(!editor.show_unsaved_changes_dialog);
    });
    second_editor.read_with(cx, |editor, _cx| {
        assert!(!editor.show_unsaved_changes_dialog);
    });
}

#[gpui::test]
async fn quit_application_prompts_dirty_editor_without_quitting(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let (first_editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "first".to_string(), None));
    let first_window = activate_visual_window(cx);

    let (second_editor, cx) = cx
        .cx
        .add_window_view(|_window, cx| Editor::from_markdown(cx, "second".to_string(), None));
    let second_window = activate_visual_window(cx);

    second_editor.update(cx, |editor, cx| editor.mark_dirty(cx));
    assert_eq!(cx.cx.windows().len(), 2);

    cx.cx.update(|cx| {
        crate::app_menu::dispatch_menu_action(&QuitApplication, cx);
    });
    cx.run_until_parked();

    let open_windows = cx.cx.windows();
    assert_eq!(open_windows.len(), 2);
    assert!(
        open_windows
            .iter()
            .any(|window| window.window_id() == first_window.window_id())
    );
    assert!(
        open_windows
            .iter()
            .any(|window| window.window_id() == second_window.window_id())
    );
    first_editor.read_with(cx, |editor, _cx| {
        assert!(!editor.show_unsaved_changes_dialog);
    });
    second_editor.read_with(cx, |editor, _cx| {
        assert!(editor.show_unsaved_changes_dialog);
    });
}

#[gpui::test]
async fn windows_fallback_close_window_dispatch_closes_target_editor_window(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);

    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "target".to_string(), None));
    let target_window = activate_visual_window(cx);

    cx.update(|window, cx| {
        let editor = editor.downgrade();
        crate::app_menu::dispatch_menu_action_for_editor(&CloseWindow, &editor, window, cx);
    });
    cx.run_until_parked();

    assert!(
        cx.cx
            .windows()
            .iter()
            .all(|window| window.window_id() != target_window.window_id())
    );
}

#[gpui::test]
async fn window_close_action_closes_current_editor_before_global_menu_route(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);

    let (_first_editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "first".to_string(), None));
    let first_window = activate_visual_window(cx);

    let (_second_editor, cx) = cx
        .cx
        .add_window_view(|_window, cx| Editor::from_markdown(cx, "second".to_string(), None));
    let second_window = activate_visual_window(cx);

    cx.dispatch_action(CloseWindow);
    cx.run_until_parked();

    let remaining = cx.cx.windows();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].window_id(), first_window.window_id());
    assert_ne!(remaining[0].window_id(), second_window.window_id());
}

#[gpui::test]
async fn hamburger_menu_toggles_and_closes_with_the_item_panel(cx: &mut TestAppContext) {
    let editor = cx.new(|cx| Editor::from_markdown(cx, "alpha".to_string(), None));

    editor.update(cx, |editor, cx| {
        // 点一下：列表打开，条目面板先不显示。
        editor.toggle_hamburger_menu(cx);
        assert!(editor.hamburger_menu_open);
        assert_eq!(editor.menu_bar_open, None);

        // 划过（或点）某一项：列表留着，它的条目面板打开。
        editor.open_hamburger_menu_item(1, cx);
        assert!(editor.hamburger_menu_open);
        assert_eq!(editor.menu_bar_open, Some(1));

        // 再点一下按钮：列表与条目面板一起关。
        editor.toggle_hamburger_menu(cx);
        assert!(!editor.hamburger_menu_open);
        assert_eq!(editor.menu_bar_open, None);
    });
}

#[gpui::test]
async fn dismissing_from_body_closes_the_hamburger_list(cx: &mut TestAppContext) {
    let editor = cx.new(|cx| Editor::from_markdown(cx, "alpha".to_string(), None));

    editor.update(cx, |editor, cx| {
        editor.toggle_hamburger_menu(cx);
        assert!(editor.hamburger_menu_open);

        // 点正文（或 Esc）时，列表也要一起关——只开列表、没开条目面板也算打开。
        editor.dismiss_menu_bar_from_body(cx);
        assert!(!editor.hamburger_menu_open);
    });
}

#[gpui::test]
async fn dismissing_menu_bar_from_body_clears_open_state(cx: &mut TestAppContext) {
    let editor = cx.new(|cx| Editor::from_markdown(cx, "alpha".to_string(), None));

    editor.update(cx, |editor, cx| {
        editor.open_menu_bar(0, cx);
        editor.set_menu_bar_hovered(true, cx);
        editor.set_menu_panel_hovered(true, cx);
        assert_eq!(editor.menu_bar_open, Some(0));

        editor.dismiss_menu_bar_from_body(cx);
        assert_eq!(editor.menu_bar_open, None);
        assert!(!editor.menu_bar_hovered);
        assert!(!editor.menu_panel_hovered);
        assert!(!editor.menu_submenu_panel_hovered);
        assert!(editor.menu_close_task.is_none());
    });
}

#[gpui::test]
async fn submenu_panel_hover_keeps_in_window_menu_open(cx: &mut TestAppContext) {
    let editor = cx.new(|cx| Editor::from_markdown(cx, "alpha".to_string(), None));

    editor.update(cx, |editor, cx| {
        editor.open_menu_bar(0, cx);
        editor.open_menu_submenu(2, cx);
        editor.set_menu_submenu_panel_hovered(true, cx);
        editor.set_menu_panel_hovered(false, cx);
        editor.set_menu_bar_hovered(false, cx);

        assert_eq!(editor.menu_bar_open, Some(0));
        assert_eq!(editor.menu_submenu_open, Some(2));
        assert!(editor.menu_submenu_panel_hovered);
        assert!(editor.menu_close_task.is_none());

        editor.set_menu_submenu_panel_hovered(false, cx);
        assert!(editor.menu_close_task.is_some());

        editor.close_menu_bar(cx);
    });
}

// The gap bridge and the submenu panel overlap, so moving the cursor from the
// bridge onto the submenu emits `bridge: false` and `panel: true` in the same
// gesture. With both regions sharing one hover flag the stale `bridge: false`
// could win and tear the menu down, which made reaching the recent-files list
// fail intermittently. Track the two regions independently so the handoff
// always keeps the menu open, regardless of event order.
#[gpui::test]
async fn submenu_survives_bridge_to_panel_hover_handoff(cx: &mut TestAppContext) {
    let editor = cx.new(|cx| Editor::from_markdown(cx, "alpha".to_string(), None));

    editor.update(cx, |editor, cx| {
        editor.open_menu_bar(0, cx);
        editor.open_menu_submenu(3, cx);

        // Crossing the gap: only the bridge is hovered.
        editor.set_menu_panel_hovered(false, cx);
        editor.set_menu_bar_hovered(false, cx);
        editor.set_menu_submenu_bridge_hovered(true, cx);
        assert!(editor.menu_close_task.is_none());

        // Handoff into the submenu panel. The bridge reporting `false` after
        // the panel is already hovered must not schedule a close.
        editor.set_menu_submenu_panel_hovered(true, cx);
        editor.set_menu_submenu_bridge_hovered(false, cx);

        assert_eq!(editor.menu_bar_open, Some(0));
        assert_eq!(editor.menu_submenu_open, Some(3));
        assert!(editor.menu_submenu_panel_hovered);
        assert!(
            editor.menu_close_task.is_none(),
            "menu must stay open across the bridge-to-panel handoff"
        );

        editor.close_menu_bar(cx);
    });
}

#[gpui::test]
async fn app_source_never_uses_native_prompts(_cx: &mut TestAppContext) {
    // 用户要求：整个软件禁止系统原生弹窗。这条守卫挡住以后新增 `window.prompt`。
    let mut offenders = Vec::new();
    collect_prompt_offenders(std::path::Path::new("src"), &mut offenders);
    assert!(
        offenders.is_empty(),
        "src/ 里不应再出现系统原生弹窗调用 window.prompt：{offenders:?}"
    );
}

fn collect_prompt_offenders(dir: &std::path::Path, offenders: &mut Vec<String>) {
    for entry in fs::read_dir(dir).expect("source dir should be readable") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            collect_prompt_offenders(&path, offenders);
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }
        let source = fs::read_to_string(&path).expect("source file should be readable");
        for (index, line) in source.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with("//") {
                continue;
            }
            // `prompt_for_*` 是原生文件/目录选择器，不属于消息框。
            if line.contains(".prompt(") && !line.contains("prompt_for_") {
                offenders.push(format!("{}:{}", path.display(), index + 1));
            }
        }
    }
}

#[gpui::test]
async fn every_registered_command_has_a_handler(cx: &mut TestAppContext) {
    // roadmap H5：注册表里的每条命令都必须有处理者——菜单项与命令面板条目
    // 都经 Action 派发，没有处理者的命令会变成「点了没反应」（菜单里还会变灰）。
    // 这里用菜单启用判定 is_action_available 逐条把守。
    init_editor_test_app(cx);
    cx.update(|cx| crate::app_menu::init(cx));
    let (_editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, String::new(), None)
    });
    redraw(cx);

    let mut missing = Vec::new();
    for spec in crate::commands::commands() {
        let action = spec.boxed_action();
        if !cx.update(|_window, cx| cx.is_action_available(action.as_ref())) {
            missing.push(spec.id);
        }
    }

    assert!(missing.is_empty(), "以下命令没有处理者：{missing:?}");
}

