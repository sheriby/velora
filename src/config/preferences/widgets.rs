use super::*;
use gpui_kit::component::{button::{Button, ButtonVariants}, Selectable};

impl PreferencesWindow {
    pub(crate) fn nav_button(
        &self,
        id: &'static str,
        label: String,
        icon: gpui_kit::assets::IconName,
        selected: bool,
        _theme: &Theme,
        on_click: fn(&mut Self, &ClickEvent, &mut Window, &mut Context<Self>),
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        Button::new(id)
            .debug_selector(move || id.to_string())
            .ghost()
            .selected(selected)
            .icon(icon)
            .accessibility_label(label.clone())
            .child(div().flex_1().min_w_0().text_left().child(label))
            .w_full()
            .justify_start()
            .on_click(cx.listener(on_click))
    }

    /// 页面标题（放在滚动区外，不跟着内容滚）。
    pub(crate) fn page_header(&self, title: String, theme: &Theme) -> AnyElement {
        let c = &theme.colors;
        let t = &theme.typography;
        div()
            .w_full()
            .flex_shrink_0()
            .text_size(px(t.dialog_title_size))
            .font_weight(t.dialog_title_weight.to_font_weight())
            .text_color(c.dialog_title)
            .child(SharedString::from(title))
            .into_any_element()
    }

    /// 页面顶部的错误条（保存失败、快捷键冲突）。
    pub(crate) fn error_banner(&self, theme: &Theme, message: String) -> AnyElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        div()
            .w_full()
            .flex_shrink_0()
            .px(px(12.0))
            .py(px(8.0))
            .rounded(px((d.dialog_radius - 4.0).max(4.0)))
            .border(px(d.dialog_border_width))
            .border_color(c.dialog_danger_button_bg)
            .bg(c.dialog_surface)
            .text_size(px(t.dialog_body_size))
            .text_color(c.dialog_danger_button_bg)
            .child(SharedString::from(message))
            .into_any_element()
    }

}
