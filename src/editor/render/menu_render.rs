use super::*;

impl Editor {
    /// Windows：标题栏最左侧的汉堡按钮。点一下开/关一级菜单列表。
    pub(crate) fn render_hamburger_menu_button(
        &self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let is_open = self.hamburger_menu_open;
        let editor = cx.entity().downgrade();
        div()
            .id("app-hamburger-menu-button")
            .ml(px(d.menu_bar_padding_x))
            .w(px(d.menu_bar_button_height))
            .h(px(d.menu_bar_button_height))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(d.menu_bar_button_radius))
            .bg(if is_open {
                c.dialog_secondary_button_hover
            } else {
                c.dialog_surface
            })
            .hover(|this| this.bg(c.dialog_secondary_button_hover))
            .active(|this| this.opacity(0.92))
            .cursor_pointer()
            .child(
                svg()
                    .path(TITLEBAR_MENU_ICON)
                    .size(px(TITLEBAR_MENU_ICON_SIZE_PX))
                    .text_color(custom_titlebar_icon_color(theme)),
            )
            // 复用菜单栏的 hover 记账：鼠标停在按钮上就不该触发 120ms 自动关闭。
            .on_hover(cx.listener(Self::on_menu_bar_hover))
            .on_click(move |_, _window, cx| {
                let _ = editor.update(cx, |editor, cx| editor.toggle_hamburger_menu(cx));
            })
            .into_any_element()
    }

    /// Windows：汉堡按钮展开的一级菜单列表（文件/导出/语言/主题，一个竖列）。
    /// 划过哪一项，它的条目就在右边展开——条目面板仍由 `render_in_window_menu_panel`
    /// 渲染，只是锚点不同。
    pub(crate) fn render_hamburger_menu_panel(
        &self,
        theme: &Theme,
        cx: &mut Context<Self>,
        menus: &[gpui::OwnedMenu],
        titlebar_height: f32,
    ) -> Option<AnyElement> {
        if !self.hamburger_menu_open || menus.is_empty() {
            return None;
        }
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let editor = cx.entity().downgrade();
        let labels: Vec<SharedString> = menus.iter().map(|menu| menu.name.clone()).collect();
        let list_width = menu_panel_width_for_labels(&labels, d);
        Some(
            div()
                .id("app-hamburger-menu-panel")
                .absolute()
                .occlude()
                .top(px(hamburger_menu_panel_top(titlebar_height, d)))
                .left(px(d.menu_bar_padding_x))
                .w(px(list_width))
                .p(px(d.menu_panel_padding))
                .flex()
                .flex_col()
                .gap(px(d.menu_panel_gap))
                .bg(c.dialog_surface)
                .border(px(d.dialog_border_width))
                .border_color(c.dialog_border)
                .rounded(px(d.menu_panel_radius))
                .shadow_lg()
                .on_hover(cx.listener(Self::on_menu_bar_hover))
                .children(labels.iter().enumerate().map(|(index, label)| {
                    let label = label.clone();
                    let is_open = self.menu_bar_open == Some(index);
                    let entry_editor = editor.clone();
                    div()
                        .id(("app-hamburger-menu-item", index))
                        .w_full()
                        .h(px(d.menu_item_height))
                        .px(px(d.menu_item_padding_x))
                        .flex()
                        .items_center()
                        .rounded(px(d.menu_item_radius))
                        .bg(if is_open {
                            c.dialog_secondary_button_hover
                        } else {
                            c.dialog_surface
                        })
                        .hover(|this| this.bg(c.dialog_secondary_button_hover))
                        .active(|this| this.opacity(0.92))
                        .cursor_pointer()
                        .text_size(px(d.menu_text_size))
                        .font_weight(t.dialog_body_weight.to_font_weight())
                        .text_color(c.dialog_secondary_button_text)
                        .whitespace_nowrap()
                        .child(label)
                        .on_hover(move |hovered, _window, cx| {
                            if *hovered {
                                let _ = entry_editor.update(cx, |editor, cx| {
                                    editor.open_hamburger_menu_item(index, cx)
                                });
                            }
                        })
                }))
                .into_any_element(),
        )
    }

    /// Renders the in-window fallback menu bar backed by the app menus
    /// registered through `App::set_menus`. `menus` and `menu_labels` are
    /// fetched and computed once at the caller and shared with
    /// [`Self::render_in_window_menu_panel`].
    pub(crate) fn render_in_window_menu_bar(
        &self,
        theme: &Theme,
        cx: &mut Context<Self>,
        menus: Option<&[gpui::OwnedMenu]>,
        menu_labels: &[SharedString],
        top_offset: f32,
    ) -> Option<AnyElement> {
        let menus = menus?;
        if menus.is_empty() {
            return None;
        }

        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let editor = cx.entity().downgrade();
        let button_widths = menu_labels
            .iter()
            .map(|label| menu_bar_button_width(label, d))
            .collect::<Vec<_>>();

        Some(
            div()
                .id("app-menu-bar")
                .absolute()
                .top(px(top_offset))
                .left_0()
                .right_0()
                .h(px(d.menu_bar_height))
                .occlude()
                .flex()
                .items_center()
                .gap(px(d.menu_bar_gap))
                .px(px(d.menu_bar_padding_x))
                .py(px(d.menu_bar_padding_y))
                .bg(c.dialog_surface)
                .border_b(px(theme.dimensions.dialog_border_width))
                .border_color(c.dialog_border)
                .on_hover(cx.listener(Self::on_menu_bar_hover))
                .children(menu_labels.iter().enumerate().map(|(index, label)| {
                    let label = label.clone();
                    let is_open = self.menu_bar_open == Some(index);
                    let button_editor = editor.clone();
                    let button_width = button_widths[index];

                    div()
                        .id(("app-menu-button", index))
                        .h(px(d.menu_bar_button_height))
                        .w(px(button_width))
                        .px(px(d.menu_bar_button_padding_x))
                        .flex()
                        .flex_shrink_0()
                        .items_center()
                        .justify_center()
                        .rounded(px(d.menu_bar_button_radius))
                        .bg(if is_open {
                            c.dialog_secondary_button_hover
                        } else {
                            c.dialog_surface
                        })
                        .hover(|this| this.bg(c.dialog_secondary_button_hover))
                        .active(|this| this.opacity(0.92))
                        .cursor_pointer()
                        .text_size(px(d.menu_text_size))
                        .font_weight(t.dialog_button_weight.to_font_weight())
                        .text_color(c.dialog_secondary_button_text)
                        .whitespace_nowrap()
                        .child(label)
                        .on_hover(move |hovered, _window, cx| {
                            if *hovered {
                                let _ = button_editor
                                    .update(cx, |editor, cx| editor.open_menu_bar(index, cx));
                            }
                        })
                }))
                .into_any_element(),
        )
    }

    pub(crate) fn render_in_window_menu_item(
        &self,
        item: OwnedMenuItem,
        item_index: usize,
        theme: &Theme,
        editor: WeakEntity<Self>,
    ) -> AnyElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;

        match item {
            OwnedMenuItem::Separator => crate::components::menu::menu_separator(theme)
                .id(("app-menu-separator", item_index))
                .flex_shrink_0()
                .into_any_element(),
            OwnedMenuItem::Action { name, action, .. } => {
                let is_disabled = action.as_ref().as_any().is::<NoRecentFiles>();
                let click_editor = editor.clone();
                let hover_editor = editor.clone();
                let base = div()
                    .id(("app-menu-item", item_index))
                    .w_full()
                    .h(px(d.menu_item_height))
                    .flex_shrink_0()
                    .px(px(d.menu_item_padding_x))
                    .flex()
                    .items_center()
                    .rounded(px(d.menu_item_radius))
                    .bg(c.dialog_surface)
                    .text_size(px(d.menu_text_size))
                    .font_weight(t.dialog_body_weight.to_font_weight())
                    .text_color(if is_disabled {
                        c.dialog_muted
                    } else {
                        c.dialog_secondary_button_text
                    })
                    .child(name)
                    .on_hover(move |hovered, _window, cx| {
                        if *hovered {
                            let _ =
                                hover_editor.update(cx, |editor, cx| editor.close_menu_submenu(cx));
                        }
                    });

                if is_disabled {
                    base.into_any_element()
                } else {
                    base.hover(|this| this.bg(c.dialog_secondary_button_hover))
                        .active(|this| this.opacity(0.92))
                        .cursor_pointer()
                        .on_click(move |_, window, cx| {
                            let _ = click_editor.update(cx, |editor, cx| editor.close_menu_bar(cx));
                            dispatch_menu_action_for_editor(
                                action.as_ref(),
                                &click_editor,
                                window,
                                cx,
                            );
                        })
                        .into_any_element()
                }
            }
            OwnedMenuItem::Submenu(submenu) => {
                let is_open = self.menu_submenu_open == Some(item_index);
                let hover_editor = editor.clone();
                div()
                    .id(("app-menu-submenu", item_index))
                    .w_full()
                    .h(px(d.menu_item_height))
                    .flex_shrink_0()
                    .px(px(d.menu_item_padding_x))
                    .flex()
                    .items_center()
                    .justify_between()
                    .rounded(px(d.menu_item_radius))
                    .bg(if is_open {
                        c.dialog_secondary_button_hover
                    } else {
                        c.dialog_surface
                    })
                    .hover(|this| this.bg(c.dialog_secondary_button_hover))
                    .cursor_pointer()
                    .text_size(px(d.menu_text_size))
                    .font_weight(t.dialog_body_weight.to_font_weight())
                    .text_color(c.dialog_secondary_button_text)
                    .child(submenu.name.to_string())
                    .child(">")
                    .on_hover(move |hovered, _window, cx| {
                        if *hovered {
                            let _ = hover_editor
                                .update(cx, |editor, cx| editor.open_menu_submenu(item_index, cx));
                        }
                    })
                    .into_any_element()
            }
            OwnedMenuItem::SystemMenu(os_menu) => div()
                .id(("app-menu-system", item_index))
                .w_full()
                .h(px(d.menu_item_height))
                .flex_shrink_0()
                .px(px(d.menu_item_padding_x))
                .flex()
                .items_center()
                .rounded(px(d.menu_item_radius))
                .bg(c.dialog_surface)
                .text_size(px(d.menu_text_size))
                .text_color(c.dialog_muted)
                .child(os_menu.name.to_string())
                .into_any_element(),
        }
    }

    /// Renders the currently open in-window fallback menu as a floating
    /// panel. `menus` and `menu_labels` are fetched and computed once at
    /// the caller and shared with [`Self::render_in_window_menu_bar`].
    pub(crate) fn render_in_window_menu_panel(
        &self,
        theme: &Theme,
        cx: &mut Context<Self>,
        menus: Option<&[gpui::OwnedMenu]>,
        origin: MenuPanelOrigin,
        viewport_height: f32,
    ) -> Option<AnyElement> {
        let open_index = self.menu_bar_open?;
        let menus = menus?;
        let menu = menus.get(open_index)?.clone();
        let menu_items = menu.items.clone();
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let editor = cx.entity().downgrade();
        let menu_item_labels = owned_menu_item_labels(&menu_items);
        let menu_panel_width = menu_panel_width_for_labels(&menu_item_labels, d);
        let submenu_bridge = self.menu_submenu_open.and_then(|submenu_index| {
            match menu_items.get(submenu_index)? {
                OwnedMenuItem::Submenu(submenu) => {
                    let submenu_labels = owned_menu_item_labels(&submenu.items);
                    let geometry = submenu_bridge_geometry(
                        origin.panel_left,
                        &menu_items,
                        submenu_index,
                        &submenu_labels,
                        d,
                    )?;
                    Some(
                        div()
                            .id(("app-submenu-bridge", open_index * 1000 + submenu_index))
                            .absolute()
                            .occlude()
                            .top(px(origin.panel_top + geometry.top))
                            .left(px(geometry.left))
                            .w(px(geometry.width))
                            .h(px(geometry.height))
                            .bg(hsla(0.0, 0.0, 0.0, 0.0))
                            .on_hover(cx.listener(Self::on_menu_submenu_bridge_hover))
                            .into_any_element(),
                    )
                }
                _ => None,
            }
        });
        let submenu_panel =
            self.menu_submenu_open.and_then(|submenu_index| {
                match menu_items.get(submenu_index)? {
                    OwnedMenuItem::Submenu(submenu) => {
                        let submenu_labels = owned_menu_item_labels(&submenu.items);
                        let left = origin.panel_left
                            + menu_panel_width
                            + d.menu_panel_gap;
                        let top = submenu_panel_top(&menu_items, submenu_index, d);
                        let submenu_width = menu_panel_width_for_labels(&submenu_labels, d);
                        let submenu_items = submenu.items.clone().into_iter().enumerate().map(
                            |(item_index, item)| match item {
                                OwnedMenuItem::Separator => {
                                    crate::components::menu::menu_separator(theme)
                                        .id((
                                            "app-submenu-separator",
                                            submenu_index * 1000 + item_index,
                                        ))
                                        .into_any_element()
                                }
                                OwnedMenuItem::Action { name, action, .. } => {
                                    let is_disabled =
                                        action.as_ref().as_any().is::<NoRecentFiles>();
                                    let editor = editor.clone();
                                    let base = div()
                                        .id(("app-submenu-item", submenu_index * 1000 + item_index))
                                        .w_full()
                                        .h(px(d.menu_item_height))
                                        .px(px(d.menu_item_padding_x))
                                        .flex()
                                        .items_center()
                                        .rounded(px(d.menu_item_radius))
                                        .bg(c.dialog_surface)
                                        .text_size(px(d.menu_text_size))
                                        .font_weight(t.dialog_body_weight.to_font_weight())
                                        .text_color(if is_disabled {
                                            c.dialog_muted
                                        } else {
                                            c.dialog_secondary_button_text
                                        })
                                        .child(name);

                                    if is_disabled {
                                        base.into_any_element()
                                    } else {
                                        base.hover(|this| this.bg(c.dialog_secondary_button_hover))
                                            .active(|this| this.opacity(0.92))
                                            .cursor_pointer()
                                            .on_click(move |_, window, cx| {
                                                let _ = editor.update(cx, |editor, cx| {
                                                    editor.close_menu_bar(cx)
                                                });
                                                dispatch_menu_action_for_editor(
                                                    action.as_ref(),
                                                    &editor,
                                                    window,
                                                    cx,
                                                );
                                            })
                                            .into_any_element()
                                    }
                                }
                                OwnedMenuItem::Submenu(submenu) => div()
                                    .id(("app-submenu-nested", submenu_index * 1000 + item_index))
                                    .w_full()
                                    .h(px(d.menu_item_height))
                                    .px(px(d.menu_item_padding_x))
                                    .flex()
                                    .items_center()
                                    .rounded(px(d.menu_item_radius))
                                    .bg(c.dialog_surface)
                                    .text_size(px(d.menu_text_size))
                                    .text_color(c.dialog_muted)
                                    .child(submenu.name.to_string())
                                    .into_any_element(),
                                OwnedMenuItem::SystemMenu(os_menu) => div()
                                    .id(("app-submenu-system", submenu_index * 1000 + item_index))
                                    .w_full()
                                    .h(px(d.menu_item_height))
                                    .px(px(d.menu_item_padding_x))
                                    .flex()
                                    .items_center()
                                    .rounded(px(d.menu_item_radius))
                                    .bg(c.dialog_surface)
                                    .text_size(px(d.menu_text_size))
                                    .text_color(c.dialog_muted)
                                    .child(os_menu.name.to_string())
                                    .into_any_element(),
                            },
                        );

                        Some(
                            div()
                                .id(("app-submenu-panel", open_index * 1000 + submenu_index))
                                .absolute()
                                .occlude()
                                .top(px(origin.panel_top + top))
                                .left(px(left))
                                .w(px(submenu_width))
                                .p(px(d.menu_panel_padding))
                                .flex()
                                .flex_col()
                                .gap(px(d.menu_panel_gap))
                                .bg(c.dialog_surface)
                                .border(px(d.dialog_border_width))
                                .border_color(c.dialog_border)
                                .rounded(px(d.menu_panel_radius))
                                .shadow_lg()
                                .on_hover(cx.listener(Self::on_menu_submenu_panel_hover))
                                .children(submenu_items)
                                .into_any_element(),
                        )
                    }
                    _ => None,
                }
            });

        let main_panel = div()
            .id(("app-menu-panel", open_index))
            .absolute()
            .occlude()
            .top(px(origin.panel_top + d.menu_panel_top))
            .left(px(origin.panel_left))
            .w(px(menu_panel_width))
            .p(px(d.menu_panel_padding))
            .flex()
            .flex_col()
            .gap(px(d.menu_panel_gap))
            .bg(c.dialog_surface)
            .border(px(d.dialog_border_width))
            .border_color(c.dialog_border)
            .rounded(px(d.menu_panel_radius))
            .shadow_lg()
            .on_hover(cx.listener(Self::on_menu_panel_hover));
        let main_panel = if let Some(split_index) = import_menu_split_index(&menu_items) {
            let scroll_items = &menu_items[..split_index];
            let footer_items = &menu_items[split_index..];
            let scroll_height = scrollable_import_menu_scroll_height(
                scroll_items,
                footer_items,
                viewport_height,
                origin.panel_top,
                d,
            );
            let scroll_area = (!scroll_items.is_empty()).then(|| {
                div()
                    .id(("app-menu-scroll-area", open_index))
                    .w_full()
                    .h(px(scroll_height))
                    .flex_shrink_0()
                    .min_h(px(0.0))
                    .overflow_y_scroll()
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .flex_col()
                            .gap(px(d.menu_panel_gap))
                            .children(scroll_items.iter().cloned().enumerate().map(
                                |(item_index, item)| {
                                    self.render_in_window_menu_item(
                                        item,
                                        item_index,
                                        theme,
                                        editor.clone(),
                                    )
                                },
                            )),
                    )
                    .into_any_element()
            });
            let footer_elements =
                footer_items
                    .iter()
                    .cloned()
                    .enumerate()
                    .map(|(footer_index, item)| {
                        self.render_in_window_menu_item(
                            item,
                            split_index + footer_index,
                            theme,
                            editor.clone(),
                        )
                    });

            main_panel
                .children(scroll_area)
                .children(footer_elements)
                .into_any_element()
        } else {
            let items = menu_items
                .iter()
                .cloned()
                .enumerate()
                .map(|(item_index, item)| {
                    self.render_in_window_menu_item(item, item_index, theme, editor.clone())
                });

            main_panel.children(items).into_any_element()
        };

        let layer = div()
            .id(("app-menu-panel-layer", open_index))
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .bottom_0()
            .child(main_panel);
        let layer = if let Some(submenu_bridge) = submenu_bridge {
            layer.child(submenu_bridge)
        } else {
            layer
        };
        let layer = if let Some(submenu_panel) = submenu_panel {
            layer.child(submenu_panel)
        } else {
            layer
        };

        Some(layer.into_any_element())
    }

    /// Builds the unsaved-changes dialog with backdrop, message, and three
    /// action buttons (cancel, discard, save-and-close).
    pub(crate) fn on_folder_choice_cancel(
        &mut self,
        _: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.pending_folder_choice.take().is_some() {
            cx.notify();
        }
    }

    pub(crate) fn on_folder_choice_backdrop(
        &mut self,
        _: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.pending_folder_choice.take().is_some() {
            cx.notify();
        }
    }

    pub(crate) fn on_folder_choice_new_window(
        &mut self,
        _: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(folder) = self.pending_folder_choice.take() {
            let _ = crate::app_menu::open_workspace_window(cx, folder);
        }
        cx.notify();
    }

    pub(crate) fn on_folder_choice_replace(
        &mut self,
        _: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(folder) = self.pending_folder_choice.take() {
            self.set_workspace_root(folder, cx);
        }
    }

    pub(crate) fn on_copy_as_html(
        &mut self,
        _: &crate::components::CopyAsHtml,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.copy_as_html(cx);
    }

    pub(crate) fn on_zoom_in(
        &mut self,
        _: &crate::components::ZoomIn,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.zoom_by(10, cx);
    }

    pub(crate) fn on_zoom_out(
        &mut self,
        _: &crate::components::ZoomOut,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.zoom_by(-10, cx);
    }

    pub(crate) fn on_zoom_reset(
        &mut self,
        _: &crate::components::ZoomReset,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.zoom_reset(cx);
    }

    /// 按百分比步进调整界面缩放（roadmap H5：菜单与命令面板共用）。
    pub(crate) fn zoom_by(&mut self, delta: i64, cx: &mut Context<Self>) {
        let current = crate::config::EditorSettings::zoom_percent(cx);
        let next = (current + delta).clamp(60, 200);
        if next != current {
            crate::config::EditorSettings::set_zoom_percent(cx, next);
            cx.refresh_windows();
        }
    }

    /// 缩放回到 100%。
    pub(crate) fn zoom_reset(&mut self, cx: &mut Context<Self>) {
        crate::config::EditorSettings::set_zoom_percent(cx, 100);
        cx.refresh_windows();
    }

}
