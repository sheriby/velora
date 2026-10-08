use super::super::{Editor, WorkspaceTab};
use gpui::{Modifiers, ScrollDelta, ScrollWheelEvent, TestAppContext, TouchPhase, point, px};
use std::fs;

fn init_sidebar_test_app(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
}

fn sidebar_is_visible(cx: &mut gpui::VisualTestContext) -> (bool, bool) {
    (
        cx.debug_bounds("activity-files").is_some(),
        cx.debug_bounds("workspace-panel").is_some(),
    )
}

#[gpui::test]
async fn the_sidebar_toggles_whole_and_leaves_no_mouse_trap_behind(cx: &mut TestAppContext) {
    // 用户报修：收起侧边栏经常误触，误触后整条（含图标列）都没了，只能靠贴左边
    // 缘的浮层找回来。现行契约：收起就是整条不占布局、正文满宽，唤出只有命令
    // 那一条；没有贴边浮层，也没有常驻的图标列（用户明确不要常驻入口）。
    init_sidebar_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
    cx.update(|window, cx| window.draw(cx).clear());

    for (id, tab) in [
        ("activity-files", WorkspaceTab::Files),
        ("activity-search", WorkspaceTab::Search),
        ("activity-outline", WorkspaceTab::Outline),
    ] {
        // 默认收起：图标列与面板都不占位。
        assert_eq!(
            sidebar_is_visible(cx),
            (false, false),
            "默认收起状态不该有图标列或面板"
        );

        // 命令唤出：图标列 + 面板一起出现，并切到这一页。
        editor.update(cx, |editor, cx| {
            editor.set_workspace_tab(tab, cx);
            editor.workspace.is_open = true;
        });
        cx.update(|window, cx| window.draw(cx).clear());
        assert_eq!(
            sidebar_is_visible(cx),
            (true, true),
            "唤出后图标列与面板都该占位"
        );
        editor.read_with(cx, |editor, _| {
            assert_eq!(editor.workspace.active_tab, tab, "应切到对应页");
        });

        // 图标列上点同一页签：收起整条（含图标列），不留半个入口在布局里。
        let rail_button = cx.debug_bounds(id).expect("展开时图标列应可点");
        cx.simulate_click(rail_button.center(), Modifiers::none());
        cx.update(|window, cx| window.draw(cx).clear());
        editor.read_with(cx, |editor, _| {
            assert!(!editor.workspace.is_open, "再点 {id} 应收起侧边栏");
        });
        assert_eq!(
            sidebar_is_visible(cx),
            (false, false),
            "收起后图标列与面板都不该留下"
        );
    }
}

#[gpui::test]
async fn hovering_the_left_edge_does_not_open_the_sidebar(cx: &mut TestAppContext) {
    // 「贴边唤出整条侧边栏」是误触重灾区（用户报修），整个自动隐藏机制已删。
    // 收起后指针贴左缘停留再久也不该有东西冒出来。
    init_sidebar_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
    cx.update(|window, cx| window.draw(cx).clear());

    cx.simulate_mouse_move(
        point(px(2.0), px(200.0)),
        gpui::MouseButton::Left,
        Modifiers::none(),
    );
    cx.update(|window, cx| window.draw(cx).clear());
    // 旧实现这里要停留满 dwell 再等滑入动画；现在等足够久也不该有反应。
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(1000));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear());
    std::thread::sleep(std::time::Duration::from_millis(400));
    cx.update(|window, cx| window.draw(cx).clear());

    editor.read_with(cx, |editor, _| {
        assert!(!editor.workspace.is_open, "贴左边缘不该展开侧边栏");
    });
    assert_eq!(
        sidebar_is_visible(cx),
        (false, false),
        "贴左边缘不该冒出图标列或面板"
    );
    assert!(
        cx.debug_bounds("sidebar-auto-hide-overlay").is_none(),
        "自动隐藏浮层已删除"
    );

    // 指针移开：什么都不用收，也不该有东西跟着出现或消失。
    cx.simulate_mouse_move(
        point(px(700.0), px(200.0)),
        gpui::MouseButton::Left,
        Modifiers::none(),
    );
    cx.update(|window, cx| window.draw(cx).clear());
    cx.update(|window, cx| window.draw(cx).clear());
    assert_eq!(
        sidebar_is_visible(cx),
        (false, false),
        "移开指针不该改变侧边栏状态"
    );
}

#[gpui::test]
async fn ctrl_b_toggles_the_sidebar_only_when_nothing_is_selected(cx: &mut TestAppContext) {
    // 用户要求：cmd/ctrl-b 兼作侧边栏开关，但只在没选中文字时；有选区时仍是加粗。
    init_sidebar_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| {
        Editor::from_markdown(cx, "alpha one\n\nbeta two\n".to_string(), None)
    });
    cx.update(|window, cx| window.draw(cx).clear());

    let first_block = editor.read_with(cx, |editor, _| {
        editor.document.visible_blocks()[0].entity.clone()
    });
    editor.update(cx, |editor, _| editor.focus_block(first_block.entity_id()));
    cx.update(|window, cx| window.draw(cx).clear());

    // 空选区（只有光标）：不管文本，只开关侧边栏。
    let buffer_before = editor.read_with(cx, |editor, _| editor.buffer.text());
    cx.simulate_keystrokes("ctrl-b");
    cx.update(|window, cx| window.draw(cx).clear());
    editor.read_with(cx, |editor, _| {
        assert!(editor.workspace.is_open, "空选区按 ctrl-b 应展开侧边栏");
    });
    assert_eq!(
        editor.read_with(cx, |editor, _| editor.buffer.text()),
        buffer_before,
        "空选区按 ctrl-b 不该动文本"
    );

    cx.simulate_keystrokes("ctrl-b");
    cx.update(|window, cx| window.draw(cx).clear());
    editor.read_with(cx, |editor, _| {
        assert!(!editor.workspace.is_open, "再按一次应收起侧边栏");
    });

    // 有选区：还是加粗，侧边栏状态不动。
    first_block.update(cx, |block, _cx| block.selected_range = 0..5);
    cx.simulate_keystrokes("ctrl-b");
    cx.update(|window, cx| window.draw(cx).clear());
    assert_eq!(
        editor.read_with(cx, |editor, _| editor.buffer.text()),
        "**alpha** one\n\nbeta two\n",
        "有选区时 ctrl-b 仍应是加粗"
    );
    editor.read_with(cx, |editor, _| {
        assert!(!editor.workspace.is_open, "有选区时不该碰侧边栏");
    });
}

#[gpui::test]
async fn scrolling_inside_the_sidebar_tree_does_not_scroll_the_document(cx: &mut TestAppContext) {
    // 用户报修过：在侧栏文件树里滚，正文跟着一起滚。滚轮命中必须只落在树那一列上。
    init_sidebar_test_app(cx);
    let root = std::env::temp_dir().join(format!("velora-sidebar-scroll-{}", uuid::Uuid::new_v4()));
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

    let markdown = (0..400)
        .map(|index| format!("## Section {index}\n\nParagraph body for section {index}.\n"))
        .collect::<Vec<_>>()
        .join("\n");
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, markdown, None));
    editor.update(cx, |editor, cx| {
        editor.set_workspace_root(root.clone(), cx);
        editor.workspace.is_open = true;
        editor.workspace.active_tab = WorkspaceTab::Files;
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear());

    // 先证明正文可滚、且滚轮命中正文时确实会滚。
    cx.simulate_event(ScrollWheelEvent {
        position: point(px(700.0), px(400.0)),
        delta: ScrollDelta::Pixels(point(px(0.0), px(-600.0))),
        modifiers: Modifiers::default(),
        touch_phase: TouchPhase::default(),
    });
    cx.update(|window, cx| window.draw(cx).clear());
    let body_offset = editor.read_with(cx, |editor, _| editor.scroll_handle.offset().y);
    assert!(
        body_offset < px(0.0),
        "正文应先滚起来，实测 {body_offset:?}"
    );

    let tree_max = editor.read_with(cx, |editor, _| {
        editor.workspace.tree_scroll_handle.max_offset().height
    });
    assert!(
        tree_max > px(0.0),
        "文件树应可滚（前置布局条件），实测 {tree_max:?}"
    );

    // 在文件树上滚：只有文件树滚，正文偏移不许动。图标列 50px，点落在面板列里。
    let tree_point = point(px(100.0), px(400.0));
    cx.simulate_event(ScrollWheelEvent {
        position: tree_point,
        delta: ScrollDelta::Pixels(point(px(0.0), px(-600.0))),
        modifiers: Modifiers::default(),
        touch_phase: TouchPhase::default(),
    });
    cx.update(|window, cx| window.draw(cx).clear());
    let tree_offset = editor.read_with(cx, |editor, _| {
        editor.workspace.tree_scroll_handle.offset().y
    });
    let body_after = editor.read_with(cx, |editor, _| editor.scroll_handle.offset().y);
    assert!(
        tree_offset < px(0.0),
        "滚轮应把文件树滚下去，实测 {tree_offset:?}"
    );
    assert_eq!(body_after, body_offset, "侧栏里滚动不该带动正文");
}
