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

    /// 一组设置：行装在圆角面板里，行与行之间一条细线。
    pub(crate) fn settings_card(&self, theme: &Theme, rows: Vec<AnyElement>) -> AnyElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let mut column = div().w_full().flex().flex_col();
        for (index, row) in rows.into_iter().enumerate() {
            if index > 0 {
                column = column.child(
                    div()
                        .w_full()
                        .h(px(d.dialog_border_width.max(1.0)))
                        .bg(c.dialog_border),
                );
            }
            column = column.child(row);
        }
        div()
            .w_full()
            .flex_shrink_0()
            .rounded(px(d.dialog_radius))
            .border(px(d.dialog_border_width))
            .border_color(c.dialog_border)
            .bg(c.dialog_surface)
            .overflow_hidden()
            .child(column)
            .into_any_element()
    }

    /// 一行设置：左边标签，右边控件；整行悬停时高亮。
    pub(crate) fn settings_row(
        &self,
        theme: &Theme,
        label: impl Into<SharedString>,
        control: impl IntoElement,
    ) -> AnyElement {
        let c = &theme.colors;
        let t = &theme.typography;
        div()
            .w_full()
            .min_h(px(52.0))
            .px(px(14.0))
            .py(px(8.0))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(16.0))
            .hover(|this| this.bg(c.dialog_secondary_button_hover))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .text_size(px(t.dialog_body_size))
                    .text_color(c.dialog_body)
                    .child(label.into()),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_end()
                    .child(control),
            )
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

    fn dropdown_button_height(theme: &Theme) -> f32 {
        (theme.typography.dialog_body_size * 1.5 + 8.0).max(32.0)
    }

    pub(crate) fn dropdown_menu(
        &self,
        id: &'static str,
        content: impl IntoElement,
        theme: &Theme,
    ) -> impl IntoElement {
        let menu_id = SharedString::from(format!("{id}-list"));
        // 延后绘制让菜单越过设置卡片与页面的裁剪区；绝对定位不参与设置行的高度计算。
        deferred(
            anchored()
                .offset(point(
                    px(0.0),
                    px(Self::dropdown_button_height(theme) + 4.0),
                ))
                .child(
                    div()
                        .id(menu_id.clone())
                        .debug_selector(move || menu_id.to_string())
                        .w(px(200.0))
                        .max_h(px(240.0))
                        .p(px(4.0))
                        .rounded(px(theme.dimensions.menu_item_radius))
                        .border_1()
                        .border_color(theme.colors.dialog_border)
                        .bg(theme.colors.dialog_surface)
                        .shadow_md()
                        .overflow_y_scroll()
                        .track_scroll(&self.dropdown_scroll)
                        .occlude()
                        .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                        .child(div().w_full().flex_shrink_0().child(content)),
                ),
        )
        .with_priority(1)
    }

    pub(crate) fn dropdown_button(
        id: &'static str,
        label: String,
        theme: &Theme,
        on_click: fn(&mut Self, &ClickEvent, &mut Window, &mut Context<Self>),
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        div()
            .id(id)
            .debug_selector(move || id.to_string())
            .w(px(200.0))
            .min_h(px(Self::dropdown_button_height(theme)))
            .px(px(10.0))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(8.0))
            .rounded(px(d.menu_item_radius))
            .border(px(d.dialog_border_width))
            .border_color(c.dialog_border)
            .bg(c.dialog_secondary_button_bg)
            .hover(|this| this.bg(c.dialog_secondary_button_hover))
            .cursor_pointer()
            .text_size(px(t.dialog_body_size))
            .text_color(c.dialog_body)
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .truncate()
                    .child(SharedString::from(label)),
            )
            .child(
                svg()
                    .path("icon/workspace/chevron-down.svg")
                    .size(px(10.0))
                    .text_color(c.dialog_muted),
            )
            .on_click(cx.listener(on_click))
    }

    pub(crate) fn dropdown_item(
        id: impl Into<ElementId>,
        label: String,
        selected: bool,
        theme: &Theme,
        on_click: impl Fn(&mut Self, &ClickEvent, &mut Window, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let debug_id = id.into();
        div()
            .id(debug_id.clone())
            // 测试里按 id 查边界（release 构建为空操作）：下拉能不能点中
            // 只有真的点一下才验得出来。
            .debug_selector(move || debug_id.to_string())
            .w_full()
            .min_h(px(30.0))
            .px(px(10.0))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(8.0))
            .rounded(px(d.menu_item_radius))
            .cursor_pointer()
            .bg(if selected {
                c.selection
            } else {
                c.dialog_secondary_button_bg
            })
            .hover(|this| this.bg(c.dialog_secondary_button_hover))
            .text_size(px(t.dialog_body_size))
            .text_color(if selected {
                c.dialog_title
            } else {
                c.dialog_body
            })
            .child(
                div().flex_1().min_w(px(0.0)).truncate().child(SharedString::from(label)),
            )
            .when(selected, |this| {
                this.child(
                    svg()
                        .path("icon/workspace/check.svg")
                        .size(px(10.0))
                        .text_color(c.dialog_primary_button_bg),
                )
            })
            .on_click(cx.listener(on_click))
    }

}
