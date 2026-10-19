//! Shared window chrome helpers for themed client-side title bars.

use std::sync::OnceLock;

use gpui::prelude::*;
use gpui::{
    AnyElement, App, Bounds, ClickEvent, Context, Decorations, DisplayId, Hsla, MouseButton,
    MouseDownEvent, Pixels,
    SharedString, TitlebarOptions, Window, WindowBackgroundAppearance, WindowBounds,
    WindowControlArea, WindowDecorations, WindowOptions, div, point, px,
};

use gpui_kit::component::{
    ActiveTheme as _, Icon, Sizable as _,
    button::{Button, ButtonCustomVariant, ButtonRounded, ButtonVariants},
};

use crate::app_identity::VELORA_APP_ID;
use crate::theme::{Theme, ThemeDimensions};

const TITLEBAR_MIN_HEIGHT: f32 = 36.0;
const TITLEBAR_BUTTON_WIDTH: f32 = 46.0;
const MAC_TRAFFIC_LIGHT_RESERVED_WIDTH: f32 = 84.0;
/// 标签占满时也要保留可抓取的空白，不能随标签宽度一起压缩。
const TITLEBAR_DRAG_FILLER_WIDTH: f32 = 40.0;
const TITLEBAR_CLOSE_ICON: &str = "icon/titlebar/chrome-close.svg";
const TITLEBAR_MAXIMIZE_ICON: &str = "icon/titlebar/chrome-maximize.svg";
const TITLEBAR_MINIMIZE_ICON: &str = "icon/titlebar/chrome-minimize.svg";
const TITLEBAR_RESTORE_ICON: &str = "icon/titlebar/chrome-restore.svg";
/// Windows 标题栏最左侧的「菜单」按钮（三个横线）。
pub(crate) const TITLEBAR_MENU_ICON: &str = "icon/titlebar/menu-hamburger.svg";

/// Selects whether velora or the platform should render window controls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TitlebarControlMode {
    NativeTrafficLights,
    AppControls,
}

/// Layout metadata shared by editor and preferences windows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CustomTitlebarLayout {
    pub(crate) height: f32,
    pub(crate) controls: TitlebarControlMode,
}

/// Chooses the drag mechanism for the platform titlebar implementation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TitlebarDragStrategy {
    PlatformHitTest,
    ExplicitMoveRequest,
}

/// A window control button kind recognised in desktop button-layout settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TitlebarButtonKind {
    Close,
    Minimize,
    Maximize,
}

/// Parsed desktop window button layout: which buttons appear on each side.
///
/// GNOME `org.gnome.desktop.wm.preferences button-layout` uses the format
/// `btn1,btn2:btn3,btn4` where the colon separates left-side from right-side
/// buttons.  Example: `close,minimize:maximize` puts close+minimize on the
/// left and maximize on the right.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TitlebarButtonLayout {
    pub(crate) left: Vec<TitlebarButtonKind>,
    pub(crate) right: Vec<TitlebarButtonKind>,
}

impl TitlebarButtonLayout {
    /// Parse a GNOME `button-layout` GSettings value.
    fn parse(s: &str) -> Self {
        let (left_part, right_part) = match s.split_once(':') {
            Some((l, r)) => (l, r),
            None => (s, ""),
        };

        let parse_side = |part: &str| -> Vec<TitlebarButtonKind> {
            part.split(',')
                .filter_map(|name| match name.trim() {
                    "close" => Some(TitlebarButtonKind::Close),
                    "minimize" => Some(TitlebarButtonKind::Minimize),
                    "maximize" => Some(TitlebarButtonKind::Maximize),
                    _ => None,
                })
                .collect()
        };

        let left = parse_side(left_part);
        let right = parse_side(right_part);

        if left.is_empty() && right.is_empty() {
            return Self::default();
        }

        TitlebarButtonLayout { left, right }
    }
}

impl Default for TitlebarButtonLayout {
    fn default() -> Self {
        TitlebarButtonLayout {
            left: Vec::new(),
            right: vec![
                TitlebarButtonKind::Minimize,
                TitlebarButtonKind::Maximize,
                TitlebarButtonKind::Close,
            ],
        }
    }
}

/// Cached result of reading the desktop's window button layout.
static LINUX_BUTTON_LAYOUT: OnceLock<TitlebarButtonLayout> = OnceLock::new();

/// Returns the window button layout for the current Linux/FreeBSD desktop.
///
/// Reads `org.gnome.desktop.wm.preferences button-layout` via `gsettings` on
/// first use, then caches the result.  Falls back to all-buttons-on-right when
/// `gsettings` is unavailable or the value is invalid.
fn cached_linux_button_layout() -> TitlebarButtonLayout {
    LINUX_BUTTON_LAYOUT
        .get_or_init(|| {
            let output = std::process::Command::new("gsettings")
                .args(["get", "org.gnome.desktop.wm.preferences", "button-layout"])
                .output()
                .ok()
                .and_then(|o| {
                    if o.status.success() {
                        String::from_utf8(o.stdout).ok()
                    } else {
                        None
                    }
                });

            match output {
                Some(raw) => {
                    // gsettings wraps the value in single quotes: "'close,minimize:maximize'"
                    let trimmed = raw.trim().trim_matches('\'');
                    TitlebarButtonLayout::parse(trimmed)
                }
                None => TitlebarButtonLayout::default(),
            }
        })
        .clone()
}

/// Returns the button layout appropriate for `target_os`.
fn button_layout_for_target_os(target_os: &str) -> TitlebarButtonLayout {
    match target_os {
        "linux" | "freebsd" => cached_linux_button_layout(),
        _ => TitlebarButtonLayout::default(),
    }
}

pub(crate) fn titlebar_options_for_target_os(
    target_os: &str,
    title: SharedString,
) -> TitlebarOptions {
    TitlebarOptions {
        title: Some(title),
        appears_transparent: matches!(target_os, "macos" | "windows"),
        traffic_light_position: if target_os == "macos" {
            Some(point(px(14.0), px(12.0)))
        } else {
            None
        },
    }
}

pub(crate) fn window_decorations_for_target_os(target_os: &str) -> Option<WindowDecorations> {
    match target_os {
        "linux" | "freebsd" => Some(WindowDecorations::Client),
        _ => None,
    }
}

pub(crate) fn velora_window_options_for_target_os(
    target_os: &str,
    title: SharedString,
    bounds: Bounds<Pixels>,
    display_id: Option<DisplayId>,
) -> WindowOptions {
    WindowOptions {
        app_id: Some(VELORA_APP_ID.to_string()),
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        // 记住的 frame 可能属于副屏：Windows 后端只在 frame 中心点落在
        // 目标显示器上时才采用它，否则整块换成显示器默认 bounds（位置和大小一起
        // 丢）。把 frame 所在的显示器一并传下去，副屏上的窗口才能原样恢复。
        display_id,
        titlebar: Some(titlebar_options_for_target_os(target_os, title)),
        // AppKit 仍会在视图收到鼠标事件后拖窗，macOS 只由标题栏显式发起移动。
        is_movable: target_os != "macos",
        window_background: WindowBackgroundAppearance::Opaque,
        window_decorations: window_decorations_for_target_os(target_os),
        ..WindowOptions::default()
    }
}

pub(crate) fn velora_window_options(title: SharedString, bounds: Bounds<Pixels>) -> WindowOptions {
    velora_window_options_for_target_os(std::env::consts::OS, title, bounds, None)
}

/// Same as [`velora_window_options`], for a window restored onto a specific display.
pub(crate) fn velora_window_options_on_display(
    title: SharedString,
    bounds: Bounds<Pixels>,
    display_id: Option<DisplayId>,
) -> WindowOptions {
    velora_window_options_for_target_os(std::env::consts::OS, title, bounds, display_id)
}

pub(crate) fn custom_titlebar_layout_for_target_os(
    target_os: &str,
    decorations: Decorations,
    dimensions: &ThemeDimensions,
) -> Option<CustomTitlebarLayout> {
    let height = dimensions.menu_bar_height.max(TITLEBAR_MIN_HEIGHT);
    match target_os {
        "macos" => Some(CustomTitlebarLayout {
            height,
            controls: TitlebarControlMode::NativeTrafficLights,
        }),
        "windows" => Some(CustomTitlebarLayout {
            height,
            controls: TitlebarControlMode::AppControls,
        }),
        "linux" | "freebsd" if matches!(decorations, Decorations::Client { .. }) => {
            Some(CustomTitlebarLayout {
                height,
                controls: TitlebarControlMode::AppControls,
            })
        }
        _ => None,
    }
}

/// macOS 与 Linux 自绘标题栏显式发起移动，Windows 使用原生命中测试。
pub(crate) fn titlebar_drag_strategy_for_target_os(
    target_os: &str,
    decorations: Decorations,
) -> TitlebarDragStrategy {
    match target_os {
        "macos" => TitlebarDragStrategy::ExplicitMoveRequest,
        "linux" | "freebsd" if matches!(decorations, Decorations::Client { .. }) => {
            TitlebarDragStrategy::ExplicitMoveRequest
        }
        _ => TitlebarDragStrategy::PlatformHitTest,
    }
}

fn on_titlebar_mouse_down(event: &MouseDownEvent, window: &mut Window, cx: &mut App) {
    if event.click_count >= 2 {
        window.zoom_window();
    } else {
        window.start_window_move();
    }
    cx.stop_propagation();
}

pub(crate) fn render_titlebar_drag_area(
    id: impl Into<gpui::ElementId>,
    window: &Window,
) -> gpui::Stateful<gpui::Div> {
    let area = div().id(id).h_full();
    if cfg!(target_os = "macos") && window.is_fullscreen() {
        return area;
    }
    // 各空白区共用平台策略，避免原生命中与显式移动的区域脱节。
    let area = area.window_control_area(WindowControlArea::Drag);
    match titlebar_drag_strategy_for_target_os(std::env::consts::OS, window.window_decorations()) {
        TitlebarDragStrategy::PlatformHitTest => area,
        TitlebarDragStrategy::ExplicitMoveRequest => {
            area.on_mouse_down(MouseButton::Left, on_titlebar_mouse_down)
        }
    }
}

pub(crate) fn custom_titlebar_height_for_target_os(
    target_os: &str,
    decorations: Decorations,
    dimensions: &ThemeDimensions,
) -> f32 {
    custom_titlebar_layout_for_target_os(target_os, decorations, dimensions)
        .map(|layout| layout.height)
        .unwrap_or(0.0)
}

pub(crate) fn custom_titlebar_height(window: &Window, dimensions: &ThemeDimensions) -> f32 {
    if cfg!(target_os = "macos") && window.is_fullscreen() {
        return 0.0;
    }

    custom_titlebar_height_for_target_os(
        std::env::consts::OS,
        window.window_decorations(),
        dimensions,
    )
}

pub(crate) fn custom_titlebar_background(theme: &Theme) -> Hsla {
    theme.colors.dialog_surface
}

pub(crate) fn titlebar_maximize_icon(is_maximized: bool, is_fullscreen: bool) -> &'static str {
    if is_maximized || is_fullscreen {
        TITLEBAR_RESTORE_ICON
    } else {
        TITLEBAR_MAXIMIZE_ICON
    }
}

fn window_control_button(
    id: &'static str,
    label: &'static str,
    icon: &'static str,
    area: WindowControlArea,
) -> Button {
    Button::new(id)
        .ghost()
        .xsmall()
        .rounded(ButtonRounded::None)
        .w(px(TITLEBAR_BUTTON_WIDTH))
        .h_full()
        .window_control_area(area)
        .accessibility_label(label)
        .icon(Icon::empty().path(icon))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn render_custom_titlebar<T: 'static>(
    id: &'static str,
    title: SharedString,
    leading: Option<AnyElement>,
    tabs: Option<AnyElement>,
    theme: &Theme,
    window: &Window,
    cx: &mut Context<T>,
    on_close: fn(&mut T, &ClickEvent, &mut Window, &mut Context<T>),
) -> Option<AnyElement> {
    render_custom_titlebar_for_target_os(
        std::env::consts::OS,
        id,
        title,
        leading,
        tabs,
        theme,
        window,
        cx,
        on_close,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn render_custom_titlebar_for_target_os<T: 'static>(
    target_os: &str,
    id: &'static str,
    title: SharedString,
    leading: Option<AnyElement>,
    tabs: Option<AnyElement>,
    theme: &Theme,
    window: &Window,
    cx: &mut Context<T>,
    on_close: fn(&mut T, &ClickEvent, &mut Window, &mut Context<T>),
) -> Option<AnyElement> {
    if target_os == "macos" && window.is_fullscreen() {
        return None;
    }

    let layout = custom_titlebar_layout_for_target_os(
        target_os,
        window.window_decorations(),
        &theme.dimensions,
    )?;
    let c = &theme.colors;
    let t = &theme.typography;
    let controls = window.window_controls();

    let breadcrumb = title
        .to_string()
        .split_once("Velora - ")
        .map(|(_, name)| name)
        .unwrap_or("")
        .to_string();
    let make_drag_title = || {
        render_titlebar_drag_area("window-titlebar-drag-title", window)
            .flex_1()
            .min_w(px(0.0))
            .px(px(12.0))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .min_w(px(0.0))
                    .truncate()
                    .text_size(px(theme.dimensions.menu_text_size))
                    .font_weight(t.dialog_button_weight.to_font_weight())
                    .text_color(c.dialog_body)
                    .child(breadcrumb.clone()),
            )
            .on_click(|event, window, _cx| {
                if event.is_right_click() {
                    window.show_window_menu(event.position());
                }
            })
    };

    let drag_filler = || {
        render_titlebar_drag_area("window-titlebar-drag-filler", window)
            .debug_selector(|| "window-titlebar-drag-filler".into())
            .flex_1()
            .min_w(px(TITLEBAR_DRAG_FILLER_WIDTH))
            .flex_shrink_0()
    };

    let root = div()
        .id(id)
        .absolute()
        .top_0()
        .left_0()
        .right_0()
        .h(px(layout.height))
        .occlude()
        .flex()
        .items_center()
        .bg(custom_titlebar_background(theme))
        .border_b(px(theme.dimensions.dialog_border_width))
        .border_color(c.dialog_border);

    let root = match layout.controls {
        TitlebarControlMode::NativeTrafficLights => {
            // 红绿灯预留位不能被压缩：标签多了整行溢出时，taffy 会按
            // flex_shrink 把这个空档挤掉，第一个标签就滑到窗口按钮下面。
            let traffic_light_spacer = || {
                div()
                    .w(px(MAC_TRAFFIC_LIGHT_RESERVED_WIDTH))
                    .h_full()
                    .flex_shrink_0()
            };
            if let Some(tabs) = tabs {
                root.child(traffic_light_spacer())
                    .child(tabs)
                    .child(drag_filler())
            } else {
                root.child(traffic_light_spacer())
                    .child(make_drag_title())
                    .child(traffic_light_spacer())
            }
        }
        TitlebarControlMode::AppControls => {
            let layout_buttons = button_layout_for_target_os(target_os);

            let mut minimize_btn = controls.minimize.then(|| {
                window_control_button(
                    "window-titlebar-minimize",
                    "Minimize window",
                    TITLEBAR_MINIMIZE_ICON,
                    WindowControlArea::Min,
                )
                .on_click(|event, window, _| {
                    if event.standard_click() {
                        window.minimize_window();
                    }
                })
                .into_any_element()
            });
            let mut maximize_btn = controls.maximize.then(|| {
                let label = if window.is_maximized() || window.is_fullscreen() {
                    "Restore window"
                } else {
                    "Maximize window"
                };
                window_control_button(
                    "window-titlebar-maximize",
                    label,
                    titlebar_maximize_icon(window.is_maximized(), window.is_fullscreen()),
                    WindowControlArea::Max,
                )
                .on_click(|event, window, _| {
                    if event.standard_click() {
                        window.zoom_window();
                    }
                })
                .into_any_element()
            });
            let mut close_btn = Some(
                window_control_button(
                    "window-titlebar-close",
                    "Close window",
                    TITLEBAR_CLOSE_ICON,
                    WindowControlArea::Close,
                )
                .custom(
                    ButtonCustomVariant::new(cx)
                        .hover(cx.theme().danger.opacity(0.14))
                        .active(cx.theme().danger.opacity(0.24)),
                )
                .on_click(cx.listener(move |view, event: &ClickEvent, window, cx| {
                    if event.standard_click() {
                        on_close(view, event, window, cx);
                    }
                }))
                .into_any_element(),
            );

            let mut left_row = div().h_full().flex().items_center().flex_shrink_0();
            for kind in &layout_buttons.left {
                let btn = match kind {
                    TitlebarButtonKind::Minimize => minimize_btn.take(),
                    TitlebarButtonKind::Maximize => maximize_btn.take(),
                    TitlebarButtonKind::Close => close_btn.take(),
                };
                if let Some(b) = btn {
                    left_row = left_row.child(b);
                }
            }

            let mut right_row = div().h_full().flex().items_center().flex_shrink_0();
            for kind in &layout_buttons.right {
                let btn = match kind {
                    TitlebarButtonKind::Minimize => minimize_btn.take(),
                    TitlebarButtonKind::Maximize => maximize_btn.take(),
                    TitlebarButtonKind::Close => close_btn.take(),
                };
                if let Some(b) = btn {
                    right_row = right_row.child(b);
                }
            }

            // Any buttons not referenced by the layout fall back to the right.
            for btn in [minimize_btn.take(), maximize_btn.take(), close_btn.take()]
                .into_iter()
                .flatten()
            {
                right_row = right_row.child(btn);
            }

            let middle: AnyElement = match tabs {
                Some(tabs) => div()
                    .h_full()
                    .flex_1()
                    .min_w(px(0.0))
                    .flex()
                    .items_center()
                    .child(tabs)
                    .child(drag_filler())
                    .into_any_element(),
                None => make_drag_title().into_any_element(),
            };

            // Windows 的菜单入口（汉堡按钮）放标题栏最左边、标签栏之前；
            // 没有标签栏（欢迎页）时它也在最左边。
            let root = match leading {
                Some(leading) => root.child(leading),
                None => root,
            };
            root.child(left_row).child(middle).child(right_row)
        }
    };

    Some(root.into_any_element())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::Tiling;

    #[test]
    fn macos_tabs_disable_automatic_window_dragging() {
        for target_os in ["macos", "windows"] {
            let options = velora_window_options_for_target_os(
                target_os,
                "Velora".into(),
                Bounds::new(point(px(0.0), px(0.0)), gpui::size(px(800.0), px(600.0))),
                None,
            );
            assert_eq!(
                options.is_movable,
                target_os != "macos",
                "macOS 必须关闭原生默认拖窗，否则标签和窗口会同时跟着鼠标移动"
            );
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[allow(unexpected_cfgs)]
    fn macos_content_view_keeps_mouse_drags_for_document_tabs() {
        use cocoa::appkit::NSView;
        use cocoa::base::nil;
        use cocoa::foundation::{NSAutoreleasePool, NSPoint, NSRect, NSSize};
        use objc::runtime::{BOOL, Class, NO, Object, Sel};
        use objc::{sel, sel_impl};

        unsafe {
            let pool = NSAutoreleasePool::new(nil);
            let view = NSView::alloc(nil).initWithFrame_(NSRect::new(
                NSPoint::new(0.0, 0.0),
                NSSize::new(10.0, 10.0),
            ));
            assert!(!view.is_null());
            let view_class = Class::get("GPUIView").expect("GPUI 原生视图");
            let method = view_class
                .instance_method(sel!(mouseDownCanMoveWindow))
                .expect("原生拖窗判定");
            let can_move: unsafe extern "C" fn(&Object, Sel) -> BOOL =
                std::mem::transmute(method.implementation());
            let actual = can_move(&*view, sel!(mouseDownCanMoveWindow));
            view.autorelease();
            pool.drain();
            assert_eq!(actual, NO, "内容视图不能默认把标签上的拖动交给原生拖窗");
        }
    }

    #[test]
    fn titlebar_options_enable_transparency_on_mac_and_windows() {
        assert!(titlebar_options_for_target_os("windows", "velora".into()).appears_transparent);
        assert!(titlebar_options_for_target_os("macos", "velora".into()).appears_transparent);
        assert!(!titlebar_options_for_target_os("linux", "velora".into()).appears_transparent);
    }

    #[test]
    fn linux_and_freebsd_request_client_decorations() {
        assert_eq!(
            window_decorations_for_target_os("linux"),
            Some(WindowDecorations::Client)
        );
        assert_eq!(
            window_decorations_for_target_os("freebsd"),
            Some(WindowDecorations::Client)
        );
        assert_eq!(window_decorations_for_target_os("unknown"), None);
    }

    #[test]
    fn custom_titlebar_height_respects_platform_and_decorations() {
        let dimensions = Theme::default_theme().dimensions;
        assert_eq!(
            custom_titlebar_height_for_target_os("windows", Decorations::Server, &dimensions),
            dimensions.menu_bar_height.max(TITLEBAR_MIN_HEIGHT)
        );
        assert_eq!(
            custom_titlebar_height_for_target_os(
                "linux",
                Decorations::Client {
                    tiling: Tiling::default()
                },
                &dimensions,
            ),
            dimensions.menu_bar_height.max(TITLEBAR_MIN_HEIGHT)
        );
        assert_eq!(
            custom_titlebar_height_for_target_os("linux", Decorations::Server, &dimensions),
            0.0
        );
        assert_eq!(
            custom_titlebar_height_for_target_os("unknown", Decorations::Server, &dimensions),
            0.0
        );
    }

    #[test]
    fn titlebar_drag_strategy_matches_platform_window_api() {
        assert_eq!(
            titlebar_drag_strategy_for_target_os("windows", Decorations::Server),
            TitlebarDragStrategy::PlatformHitTest
        );
        assert_eq!(
            titlebar_drag_strategy_for_target_os("macos", Decorations::Server),
            TitlebarDragStrategy::ExplicitMoveRequest
        );
        assert_eq!(
            titlebar_drag_strategy_for_target_os(
                "linux",
                Decorations::Client {
                    tiling: Tiling::default()
                },
            ),
            TitlebarDragStrategy::ExplicitMoveRequest
        );
        assert_eq!(
            titlebar_drag_strategy_for_target_os("linux", Decorations::Server),
            TitlebarDragStrategy::PlatformHitTest
        );
    }

    #[test]
    fn custom_titlebar_background_uses_dialog_surface_token() {
        let theme = Theme::light_theme();
        assert_eq!(
            custom_titlebar_background(&theme),
            theme.colors.dialog_surface
        );
    }

    #[test]
    fn titlebar_maximize_icon_tracks_window_state() {
        assert_eq!(titlebar_maximize_icon(false, false), TITLEBAR_MAXIMIZE_ICON);
        assert_eq!(titlebar_maximize_icon(true, false), TITLEBAR_RESTORE_ICON);
        assert_eq!(titlebar_maximize_icon(false, true), TITLEBAR_RESTORE_ICON);
    }

    #[test]
    fn button_layout_parses_gnome_left_side_format() {
        // macOS-style: all buttons on the left
        let layout = TitlebarButtonLayout::parse("close,minimize,maximize:");
        assert_eq!(
            layout.left,
            vec![
                TitlebarButtonKind::Close,
                TitlebarButtonKind::Minimize,
                TitlebarButtonKind::Maximize,
            ]
        );
        assert!(layout.right.is_empty());
    }

    #[test]
    fn button_layout_parses_gnome_right_side_format() {
        // Traditional GNOME: all buttons on the right
        let layout = TitlebarButtonLayout::parse(":minimize,maximize,close");
        assert!(layout.left.is_empty());
        assert_eq!(
            layout.right,
            vec![
                TitlebarButtonKind::Minimize,
                TitlebarButtonKind::Maximize,
                TitlebarButtonKind::Close,
            ]
        );
    }

    #[test]
    fn button_layout_parses_split_format() {
        // Mixed: close on left, minimize+maximize on right
        let layout = TitlebarButtonLayout::parse("close:minimize,maximize");
        assert_eq!(layout.left, vec![TitlebarButtonKind::Close]);
        assert_eq!(
            layout.right,
            vec![TitlebarButtonKind::Minimize, TitlebarButtonKind::Maximize]
        );
    }

    #[test]
    fn button_layout_parses_no_colon_as_left_side() {
        // No colon — everything goes to the left side
        let layout = TitlebarButtonLayout::parse("close,minimize");
        assert_eq!(
            layout.left,
            vec![TitlebarButtonKind::Close, TitlebarButtonKind::Minimize]
        );
        assert!(layout.right.is_empty());
    }

    #[test]
    fn button_layout_ignores_unknown_button_names() {
        let layout = TitlebarButtonLayout::parse("close,menu:minimize,spacer");
        assert_eq!(layout.left, vec![TitlebarButtonKind::Close]);
        assert_eq!(layout.right, vec![TitlebarButtonKind::Minimize]);
    }

    #[test]
    fn button_layout_invalid_value_falls_back_to_default() {
        // No recognised button names at all
        let layout = TitlebarButtonLayout::parse("foo,bar");
        assert_eq!(layout, TitlebarButtonLayout::default());
    }

    #[test]
    fn button_layout_default_is_all_right() {
        let layout = TitlebarButtonLayout::default();
        assert!(layout.left.is_empty());
        assert_eq!(
            layout.right,
            vec![
                TitlebarButtonKind::Minimize,
                TitlebarButtonKind::Maximize,
                TitlebarButtonKind::Close,
            ]
        );
    }

}
