//! 菜单的共用表现：Kit 浮层定位与旧菜单行。
//!
//! 工作区、表格与图片菜单使用 Kit PopupMenu；正文及选区工具栏尚用下方旧行渲染，
//! 在迁移完成后退役。浮层共用同一个窗口坐标与点击遮罩，避免各菜单重复计算落点。

use gpui::*;
use gpui_kit::component::menu::PopupMenu;

use crate::theme::Theme;

/// 一行里「文字」与「快捷键」之间的那段间距。算面板宽度要用同一个数，
/// 所以提成常量而不是写在这两处。
pub(crate) const MENU_ROW_GAP: f32 = 12.0;
/// 行首图标的框边。它与文字之间用的就是行内那一份 `MENU_ROW_GAP`（flex 的间距对
/// 所有相邻子节点生效），量面板宽度时也按这两个数加，不留第三个常量去漂移。
pub(crate) const MENU_ICON_SIZE: f32 = 16.0;

/// 一行菜单项。`name` 同时用作元素 id 与测试选择器（`debug_bounds("menu-…")`），
/// 这样菜单行的几何能在测试里点名验证。视觉状态在这里定，点击由调用方接：不可用的项
/// 调用方不接 `on_click`，视觉上置灰但仍然占位——菜单宽度不能因为某一项不可用就变。
/// `icon` 给的是资产路径（`icon/editor/*.svg`），`None` 就是纯文字行；置灰时图标跟着
/// 换成 `dialog_muted`，与文字同一个口径。
pub(crate) fn menu_item(
    theme: &Theme,
    name: impl Into<SharedString>,
    label: impl Into<SharedString>,
    shortcut: Option<SharedString>,
    enabled: bool,
    danger: bool,
    submenu: bool,
    active: bool,
    icon: Option<SharedString>,
) -> Stateful<Div> {
    let name: SharedString = name.into();
    let colors = &theme.colors;
    let dimensions = &theme.dimensions;
    let typography = &theme.typography;
    let text_color = if danger {
        colors.dialog_danger_button_bg
    } else if enabled {
        colors.dialog_secondary_button_text
    } else {
        colors.dialog_muted
    };
    let selector = name.clone();
    let row = div()
        .h(px(dimensions.menu_item_height))
        .px(px(dimensions.menu_item_padding_x))
        .flex()
        .items_center()
        .gap(px(MENU_ROW_GAP))
        .rounded(px(dimensions.menu_item_radius))
        .bg(if active {
            colors.dialog_secondary_button_hover
        } else {
            colors.dialog_surface
        })
        .text_size(px(dimensions.menu_text_size))
        .font_weight(typography.dialog_body_weight.to_font_weight())
        .text_color(text_color)
        .debug_selector(move || format!("menu-item-{selector}"))
        .children(icon.map(|icon| {
            let selector = name.clone();
            div()
                .flex_shrink_0()
                .size(px(MENU_ICON_SIZE))
                .flex()
                .items_center()
                .justify_center()
                .debug_selector(move || format!("menu-icon-{selector}"))
                .child(
                    svg()
                        .path(icon)
                        .size(px(MENU_ICON_SIZE))
                        .text_color(text_color),
                )
        }))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .truncate()
                .child(label.into()),
        )
        .children(shortcut.map(|shortcut| {
            let selector = shortcut.clone();
            div()
                .flex_shrink_0()
                .text_color(colors.dialog_muted)
                .debug_selector(move || format!("menu-shortcut-{selector}"))
                .child(shortcut)
        }));
    let row = if submenu {
        row.child(div().flex_shrink_0().child("›"))
    } else {
        row
    };
    if enabled {
        row.id(name)
            .hover(|this| this.bg(colors.dialog_secondary_button_hover))
            .active(|this| this.opacity(0.92))
            .cursor_pointer()
    } else {
        row.id(name)
    }
}

/// 分节之间的一条细线。
pub(crate) fn menu_separator(theme: &Theme) -> Div {
    let dimensions = &theme.dimensions;
    div()
        .mx(px(dimensions.menu_separator_margin_x))
        .my(px(dimensions.menu_separator_margin_y))
        .h(px(dimensions.menu_separator_height))
        .bg(theme.colors.dialog_border)
}

pub(crate) fn popup_menu_layer(
    id: &'static str,
    position: Point<Pixels>,
    menu: &Entity<PopupMenu>,
    window: &Window,
) -> AnyElement {
    deferred(
        anchored().child(
            div()
                .w(window.viewport_size().width)
                .h(window.viewport_size().height)
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                .child(
                    anchored()
                        .position(position)
                        .snap_to_window_with_margin(px(8.0))
                        .child(
                            div()
                                .id(id)
                                .debug_selector(move || id.to_owned())
                                .child(menu.clone()),
                        ),
                ),
        ),
    )
    .with_priority(gpui_kit::base::POPUP_PRIORITY)
    .into_any_element()
}
