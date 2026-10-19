use super::*;
use gpui_kit::component::{
    Selectable, Sizable as _,
    alert::Alert,
    button::{Button, ButtonVariants},
    scroll::ScrollableElement as _,
};

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

    pub(crate) fn render_save_feedback(&self, message: String, max_height: Pixels) -> AnyElement {
        // Scrollable 默认填满父项，固定操作区必须显式使用内容高度。
        div()
            .id("preferences-save-feedback")
            .debug_selector(|| "preferences-save-feedback".into())
            .w_full()
            .flex_shrink_0()
            .h_auto()
            .max_h(max_height)
            .overflow_y_scrollbar()
            .child(Alert::error("preferences-save-error", message).small())
            .into_any_element()
    }
}
