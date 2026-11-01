use super::common::*;

use gpui::MouseButton;

/// 状态栏左端是左侧栏的常驻开关（issue #14：收起后面板整个不渲染，
/// 重开只能靠快捷键/菜单）。点击开合侧栏，再点一次回到原状。
#[gpui::test]
async fn status_bar_left_toggle_flips_the_workspace_sidebar(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, "正文".to_string(), None));
    redraw(cx);
    redraw(cx);

    let toggle = cx
        .debug_bounds("status-bar-sidebar-toggle")
        .expect("状态栏应有左侧栏开关");
    let open_before = editor.read_with(cx, |editor, _cx| editor.workspace.is_open);

    cx.simulate_mouse_down(toggle.center(), MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(toggle.center(), MouseButton::Left, Modifiers::none());
    redraw(cx);
    redraw(cx);

    let open_after = editor.read_with(cx, |editor, _cx| editor.workspace.is_open);
    assert_eq!(open_after, !open_before, "点击左侧栏开关应翻转侧栏开合");

    let toggle = cx
        .debug_bounds("status-bar-sidebar-toggle")
        .expect("开合后开关仍在状态栏上");
    cx.simulate_mouse_down(toggle.center(), MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(toggle.center(), MouseButton::Left, Modifiers::none());
    redraw(cx);
    redraw(cx);

    let open_restored = editor.read_with(cx, |editor, _cx| editor.workspace.is_open);
    assert_eq!(open_restored, open_before, "再点一次应回到原开合状态");
}

/// AI 侧栏开关在状态栏右端，且只在 AI 功能开启时存在：关闭时按钮与
/// AI 侧栏一起不存在，开启后点击展开侧栏，关闭功能时会话与按钮一起清掉。
#[gpui::test]
async fn status_bar_ai_toggle_requires_enabled_ai_and_toggles_the_sidebar(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, "正文".to_string(), None));
    redraw(cx);
    redraw(cx);

    assert!(
        cx.debug_bounds("status-bar-ai-toggle").is_none(),
        "AI 功能未开启时状态栏不应有 AI 开关"
    );

    cx.update(|_, cx| crate::ai::settings::AiFeature::set_enabled(cx, true));
    redraw(cx);
    redraw(cx);

    let toggle = cx
        .debug_bounds("status-bar-ai-toggle")
        .expect("AI 开启后状态栏应有 AI 开关");
    cx.simulate_mouse_down(toggle.center(), MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(toggle.center(), MouseButton::Left, Modifiers::none());
    redraw(cx);
    redraw(cx);

    editor.read_with(cx, |editor, _cx| {
        assert!(
            editor.ai_session.is_some(),
            "点击 AI 开关应创建 AI 会话"
        );
    });
    assert!(
        cx.debug_bounds("ai-sidebar").is_some(),
        "点击 AI 开关应展开 AI 侧栏"
    );

    cx.update(|_, cx| crate::ai::settings::AiFeature::set_enabled(cx, false));
    redraw(cx);
    redraw(cx);

    editor.read_with(cx, |editor, _cx| {
        assert!(editor.ai_session.is_none(), "AI 关闭后应清掉会话");
    });
    assert!(
        cx.debug_bounds("status-bar-ai-toggle").is_none(),
        "AI 关闭后状态栏不应再有 AI 开关"
    );
}

/// 状态栏引用的图标必须登记进资产表（main.rs 的 VeloraAssets 显式枚举），
/// 漏登记在真机上渲染为空，而按钮存在性断言逮不住它。
#[test]
fn status_bar_toggle_icons_are_registered_in_the_asset_source() {
    use gpui::AssetSource as _;
    for path in ["icon/workspace/panel-left.svg", "icon/editor/ai.svg"] {
        let loaded = crate::VeloraAssets.load(path);
        assert!(
            loaded.is_ok_and(|bytes| bytes.is_some()),
            "图标 {path} 未登记进 VeloraAssets"
        );
    }
}
