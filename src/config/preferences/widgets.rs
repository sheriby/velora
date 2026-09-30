use super::*;

impl PreferencesWindow {
    /// 侧边栏的一项：左对齐、选中时用强调色 + 左侧强调条。
    pub(crate) fn nav_button(
        &self,
        id: &'static str,
        label: String,
        selected: bool,
        theme: &Theme,
        on_click: fn(&mut Self, &ClickEvent, &mut Window, &mut Context<Self>),
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let accent = c.dialog_primary_button_bg;
        div()
            .id(id)
            .debug_selector(move || id.to_string())
            .w_full()
            .h(px(34.0))
            .px(px(10.0))
            .flex()
            .items_center()
            .gap(px(9.0))
            .rounded(px(d.menu_item_radius))
            .cursor_pointer()
            .text_size(px(t.dialog_body_size))
            .font_weight(t.dialog_button_weight.to_font_weight())
            .text_color(if selected {
                c.dialog_title
            } else {
                c.dialog_muted
            })
            .bg(if selected {
                c.selection
            } else {
                c.dialog_surface
            })
            .hover(move |this| {
                this.bg(if selected {
                    c.selection
                } else {
                    c.dialog_secondary_button_hover
                })
            })
            // 左侧强调条：未选中时用背景色占位，保证两种状态文字不左右跳。
            .child(
                div()
                    .w(px(3.0))
                    .h(px(16.0))
                    .flex_shrink_0()
                    .rounded(px(2.0))
                    .bg(if selected { accent } else { c.dialog_surface }),
            )
            .child(label)
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
            .w(px(200.0))
            .h(px(32.0))
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
            .w(px(200.0))
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
            .child(SharedString::from(label))
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

    pub(crate) fn theme_dropdown_item(
        index: usize,
        label: String,
        selected: bool,
        preview: (Hsla, Hsla, Hsla),
        theme: &Theme,
        on_click: impl Fn(&mut Self, &ClickEvent, &mut Window, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let (surface, text, accent) = preview;
        div()
            .id(("preferences-theme-option", index))
            .w(px(200.0))
            .min_h(px(38.0))
            .px(px(10.0))
            .flex()
            .items_center()
            .gap(px(10.0))
            .rounded(px(d.menu_item_radius))
            .cursor_pointer()
            .bg(if selected {
                c.selection
            } else {
                c.dialog_secondary_button_bg
            })
            .hover(|this| this.bg(c.dialog_secondary_button_hover))
            .text_size(px(t.dialog_body_size))
            .text_color(c.dialog_body)
            .child(
                div()
                    .w(px(36.0))
                    .h(px(24.0))
                    .px(px(5.0))
                    .flex_shrink_0()
                    .flex()
                    .flex_col()
                    .justify_center()
                    .gap(px(3.0))
                    .rounded(px(4.0))
                    .border(px(1.0))
                    .border_color(c.dialog_border)
                    .bg(surface)
                    .child(div().w(px(20.0)).h(px(3.0)).rounded(px(2.0)).bg(text))
                    .child(div().w(px(12.0)).h(px(3.0)).rounded(px(2.0)).bg(accent)),
            )
            .child(div().flex_1().min_w(px(0.0)).truncate().child(SharedString::from(label)))
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
