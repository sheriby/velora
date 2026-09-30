use super::super::Editor;
use gpui::{
    Modifiers, ScrollDelta, ScrollWheelEvent,
    TestAppContext, TouchPhase, point, px,
};
use std::fs;


#[gpui::test]
async fn activity_rail_buttons_all_toggle_the_sidebar(cx: &mut TestAppContext) {
    // 用户要求：文件 / 搜索 / 大纲 三个活动栏按钮都支持「再点一次收起」；
    // 侧边栏收起后整条（含窄条）隐藏，指针贴到窗口左边缘才作为浮层滑出。
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
    cx.update(|window, cx| window.draw(cx).clear());

    // 默认是收起状态：窄条不渲染，正文占满整宽。
    assert!(
        cx.debug_bounds("activity-files").is_none(),
        "收起状态下窄条不应占位"
    );

    for (id, tab) in [
        ("activity-files", super::super::WorkspaceTab::Files),
        ("activity-search", super::super::WorkspaceTab::Search),
        ("activity-outline", super::super::WorkspaceTab::Outline),
    ] {
        // 贴左边缘并停留满 dwell 唤出浮层；滑入动画按真实时间计时
        // （AnimationElement 用 Instant，测试时钟推不动），再等动画播完、
        // 浮层停在 x=0 后按钮位置才可点。
        cx.simulate_mouse_move(
            gpui::point(px(2.0), px(200.0)),
            gpui::MouseButton::Left,
            Modifiers::none(),
        );
        cx.update(|window, cx| window.draw(cx).clear());
        cx.executor().advance_clock(std::time::Duration::from_millis(400));
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear());
        std::thread::sleep(std::time::Duration::from_millis(450));
        cx.update(|window, cx| window.draw(cx).clear());
        let bounds = cx
            .debug_bounds(id)
            .unwrap_or_else(|| panic!("贴左边缘后活动栏按钮 {id} 应滑出"));

        // 第一次点：展开并切到这一页。
        cx.simulate_click(bounds.center(), Modifiers::none());
        cx.update(|window, cx| window.draw(cx).clear());
        editor.read_with(cx, |editor, _| {
            assert!(editor.workspace.is_open, "点 {id} 应展开侧边栏");
            assert_eq!(editor.workspace.active_tab, tab, "点 {id} 应切到对应页");
        });
        assert!(
            cx.debug_bounds(id).is_some(),
            "展开后窄条常驻，不该再依赖贴边"
        );

        // 第二次点：收起，整条侧边栏一起隐藏。
        let bounds = cx.debug_bounds(id).expect("展开后按钮应仍可见");
        cx.simulate_click(bounds.center(), Modifiers::none());
        cx.update(|window, cx| window.draw(cx).clear());
        editor.read_with(cx, |editor, _| {
            assert!(!editor.workspace.is_open, "再点 {id} 应收起侧边栏");
        });
        assert!(
            cx.debug_bounds(id).is_none(),
            "收起后窄条应一起隐藏，等指针贴左边缘才滑出"
        );

        // 第三次点：重新展开（同一个按钮能反复切）。同样停留唤出、等滑入
        // 动画播完后再点。
        cx.simulate_mouse_move(
            gpui::point(px(2.0), px(200.0)),
            gpui::MouseButton::Left,
            Modifiers::none(),
        );
        cx.update(|window, cx| window.draw(cx).clear());
        cx.executor().advance_clock(std::time::Duration::from_millis(400));
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear());
        std::thread::sleep(std::time::Duration::from_millis(450));
        cx.update(|window, cx| window.draw(cx).clear());
        let bounds = cx
            .debug_bounds(id)
            .unwrap_or_else(|| panic!("第二次贴边后活动栏按钮 {id} 应再次滑出"));
        cx.simulate_click(bounds.center(), Modifiers::none());
        cx.update(|window, cx| window.draw(cx).clear());
        editor.read_with(cx, |editor, _| {
            assert!(editor.workspace.is_open, "第三次点 {id} 应重新展开");
        });

        // 换下一个按钮前回到收起状态，避免上一个按钮的展开状态影响判断。
        editor.update(cx, |editor, _cx| {
            editor.workspace.is_open = false;
            editor.sidebar_peek = false;
            editor.sidebar_overlay_closing = false;
        });
        cx.update(|window, cx| window.draw(cx).clear());
    }
}

#[gpui::test]
async fn collapsed_sidebar_slides_out_only_while_pointer_is_at_left_edge(
    cx: &mut TestAppContext,
) {
    // 收起 = 自动隐藏：整条侧边栏不占布局；指针贴左边缘滑出浮层，移开收回。
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
    cx.update(|window, cx| window.draw(cx).clear());

    editor.update(cx, |editor, _| {
        editor.workspace.is_open = false;
    });
    cx.update(|window, cx| window.draw(cx).clear());
    assert!(
        cx.debug_bounds("sidebar-auto-hide-overlay").is_none(),
        "收起状态不该有浮层"
    );
    assert!(
        cx.debug_bounds("activity-files").is_none(),
        "收起状态整条侧边栏（含窄条）不占位"
    );

    // 指针贴到左边缘并停留满 dwell：整条侧边栏（窄条 + 面板）作为浮层出现。
    cx.simulate_mouse_move(
        gpui::point(px(2.0), px(200.0)),
        gpui::MouseButton::Left,
        Modifiers::none(),
    );
    cx.update(|window, cx| window.draw(cx).clear());
    cx.executor().advance_clock(std::time::Duration::from_millis(400));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear());
    let titlebar_bottom = cx
        .debug_bounds("editor-titlebar")
        .map(|bounds| bounds.origin.y + bounds.size.height)
        .unwrap_or(px(0.0));
    let overlay = cx
        .debug_bounds("sidebar-auto-hide-overlay")
        .expect("贴左边缘应滑出浮层");
    assert!(
        overlay.origin.y >= titlebar_bottom,
        "浮层顶边必须从标题栏下方开始（y = {:?}，标题栏底 = {titlebar_bottom:?}），\
         否则会盖住红绿灯和标签栏",
        overlay.origin.y
    );
    assert!(
        cx.debug_bounds("activity-files").is_some(),
        "浮层里应包含窄条按钮"
    );

    // 指针移到正文：浮层带动画收回。动画期间仍挂载，播完（定时器收尾）才卸载。
    cx.simulate_mouse_move(
        gpui::point(px(700.0), px(200.0)),
        gpui::MouseButton::Left,
        Modifiers::none(),
    );
    // 悬停命中按上一帧的 hitbox 计算，退出事件要下一帧才派发。
    cx.update(|window, cx| window.draw(cx).clear());
    cx.update(|window, cx| window.draw(cx).clear());
    editor.read_with(cx, |editor, _| {
        assert!(!editor.sidebar_peek, "指针移开后贴边状态应结束");
        assert!(
            editor.sidebar_overlay_closing,
            "移开后应先播放收回动画而不是瞬间消失"
        );
    });
    assert!(
        cx.debug_bounds("sidebar-auto-hide-overlay").is_some(),
        "收回动画期间浮层仍在滑出，不该瞬间消失"
    );
    // 动画时长（350ms）走完后，定时器把浮层真正卸载。
    cx.executor().advance_clock(std::time::Duration::from_millis(400));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear());
    assert!(
        cx.debug_bounds("sidebar-auto-hide-overlay").is_none(),
        "收回动画播完应收起浮层"
    );
}

#[gpui::test]
async fn sidebar_collapse_timer_does_not_cut_a_second_slide_out(
    cx: &mut TestAppContext,
) {
    // 快速「贴边 → 移开 → 再贴边 → 再移开」：第一轮收回的定时器到点时，
    // 第二轮收回动画还在播。旧定时器必须因 generation 变化作废，否则会把
    // 第二轮浮层半路砍掉、看起来瞬间消失。
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
    cx.update(|window, cx| window.draw(cx).clear());
    editor.update(cx, |editor, _| {
        editor.workspace.is_open = false;
    });
    cx.update(|window, cx| window.draw(cx).clear());

    // 第一轮：贴边停留唤出，随即移开进入收回动画（定时器在测试时钟
    // T+400+350=750ms 到点）。
    cx.simulate_mouse_move(
        gpui::point(px(2.0), px(200.0)),
        gpui::MouseButton::Left,
        Modifiers::none(),
    );
    cx.update(|window, cx| window.draw(cx).clear());
    cx.executor().advance_clock(std::time::Duration::from_millis(400));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear());
    cx.simulate_mouse_move(
        gpui::point(px(700.0), px(200.0)),
        gpui::MouseButton::Left,
        Modifiers::none(),
    );
    cx.update(|window, cx| window.draw(cx).clear());
    cx.update(|window, cx| window.draw(cx).clear());
    cx.executor().advance_clock(std::time::Duration::from_millis(100));

    // 第二轮收回。停留判定下真实「再贴边」走不完 dwell 就会被第一轮定时器
    // 赶上，所以像抽屉切换路径那样直写状态置回贴边，再移开触发第二轮
    // （新定时器 T+500+350=850ms 到点）。
    editor.update(cx, |editor, _cx| {
        editor.sidebar_peek = true;
        editor.sidebar_overlay_closing = false;
    });
    cx.update(|window, cx| window.draw(cx).clear());
    cx.simulate_mouse_move(
        gpui::point(px(700.0), px(200.0)),
        gpui::MouseButton::Left,
        Modifiers::none(),
    );
    cx.update(|window, cx| window.draw(cx).clear());
    cx.update(|window, cx| window.draw(cx).clear());

    // 推到 T+800ms：旧定时器（750ms）到点，但 generation 已变，不得动状态；
    // 新定时器（850ms）还没到。
    cx.executor().advance_clock(std::time::Duration::from_millis(300));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear());
    editor.read_with(cx, |editor, _| {
        assert!(
            editor.sidebar_overlay_closing,
            "旧定时器到点不能终止第二轮收回动画"
        );
    });
    assert!(
        cx.debug_bounds("sidebar-auto-hide-overlay").is_some(),
        "第二轮收回动画应完整播完，不被旧定时器半路砍掉"
    );

    // 推到 T+900ms：新一轮定时器到点，浮层才真正卸载。
    cx.executor().advance_clock(std::time::Duration::from_millis(100));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear());
    assert!(
        cx.debug_bounds("sidebar-auto-hide-overlay").is_none(),
        "新一轮收回动画播完应收起浮层"
    );
}

#[gpui::test]
async fn sidebar_edge_hover_must_dwell_before_peeking(cx: &mut TestAppContext) {
    // 防误触：贴边必须停留满 dwell 才唤出；扫过左缘不停留不弹，
    // 移开后旧的停留定时器到点也不许再弹。
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
    cx.update(|window, cx| window.draw(cx).clear());
    editor.update(cx, |editor, _| {
        editor.workspace.is_open = false;
    });
    cx.update(|window, cx| window.draw(cx).clear());

    // 贴边但停留不足 dwell（300ms 只推 200ms）：不唤出。
    cx.simulate_mouse_move(
        gpui::point(px(2.0), px(200.0)),
        gpui::MouseButton::Left,
        Modifiers::none(),
    );
    cx.update(|window, cx| window.draw(cx).clear());
    cx.executor().advance_clock(std::time::Duration::from_millis(200));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear());
    editor.read_with(cx, |editor, _| {
        assert!(!editor.sidebar_peek, "停留不满 dwell 不应唤出浮层");
    });
    assert!(
        cx.debug_bounds("sidebar-auto-hide-overlay").is_none(),
        "停留不满 dwell 不应出现浮层"
    );

    // 移开：挂着的停留定时器被作废，到点也不许再弹。
    cx.simulate_mouse_move(
        gpui::point(px(700.0), px(200.0)),
        gpui::MouseButton::Left,
        Modifiers::none(),
    );
    cx.update(|window, cx| window.draw(cx).clear());
    cx.executor().advance_clock(std::time::Duration::from_millis(400));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear());
    editor.read_with(cx, |editor, _| {
        assert!(!editor.sidebar_peek, "移开后旧停留定时器不应唤出浮层");
    });
    assert!(
        cx.debug_bounds("sidebar-auto-hide-overlay").is_none(),
        "移开后旧停留定时器不应弹出浮层"
    );
}

#[gpui::test]
async fn scrolling_inside_the_peek_overlay_does_not_scroll_the_document(
    cx: &mut TestAppContext,
) {
    // 用户报修：侧栏收起时贴边唤出浮层，在浮层里滚文件树，正文跟着一起滚。
    // 展开时侧栏占布局，正文滚区不在指针底下；收起后浮层盖在正文上，若不
    // 遮挡鼠标命中，一次滚轮会同时命中「浮层里的文件树」和「后面的编辑器
    // 滚动区」两个可滚区，两边都滚。
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let root =
        std::env::temp_dir().join(format!("velora-peek-scroll-{}", uuid::Uuid::new_v4()));
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
        editor.workspace.is_open = false;
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

    // 收起 + 贴边唤出（直接置位，等价停留满 dwell 后的状态）。滑入动画
    // 按真实时间推进，推时钟 + 睡一觉等它停到终位（和上面贴边测试同节奏）。
    editor.update(cx, |editor, _cx| {
        editor.sidebar_peek = true;
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear());
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(400));
    cx.run_until_parked();
    std::thread::sleep(std::time::Duration::from_millis(450));
    cx.update(|window, cx| window.draw(cx).clear());
    let overlay = cx
        .debug_bounds("sidebar-auto-hide-overlay")
        .expect("贴边状态下浮层应已挂载");
    assert!(
        overlay.origin.x <= px(1.0),
        "滑入动画结束后浮层应贴住左边缘，实测 origin.x = {:?}",
        overlay.origin.x
    );
    let tree_max = editor.read_with(cx, |editor, _| {
        editor.workspace.tree_scroll_handle.max_offset().height
    });
    assert!(
        tree_max > px(0.0),
        "文件树应可滚（前置布局条件），实测 {tree_max:?}"
    );

    // 在浮层里的文件树上滚：只有文件树滚，正文偏移不许动。
    let panel_point = point(overlay.origin.x + px(100.0), px(400.0));
    cx.simulate_event(ScrollWheelEvent {
        position: panel_point,
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
    assert_eq!(body_after, body_offset, "浮层里滚动不该带动正文");
}

