use super::*;

use super::document_menu::{
    QUICK_ACTION_BUTTON_SIZE, QUICK_ACTION_DIVIDER_WIDTH, QUICK_ACTION_GAP,
    QUICK_ACTION_GROUP_BREAK,
};

impl Editor {
    pub(super) fn on_toggle_table_headers(
        &mut self,
        _event: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let next = !crate::config::EditorSettings::show_table_headers(cx);
        crate::config::EditorSettings::set_show_table_headers(cx, next);
        self.close_context_menu(cx);
        // The preference is read while rendering table cells; re-render the
        // editor (and with it every table) to reflect the new styling.
        cx.notify();
    }

    pub(super) fn on_delete_table_column(
        &mut self,
        _event: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(selection) = self.active_axis_menu_selection() else {
            return;
        };
        if selection.kind != TableAxisKind::Column {
            return;
        }
        let Some(table_block) = self.table_block_by_id(selection.table_block_id, cx) else {
            return;
        };
        let column_count = table_block
            .read(cx)
            .record
            .table
            .as_ref()
            .map(|table| table.column_count());
        self.close_context_menu(cx);
        // Removing the only column empties the table, so drop the whole block.
        if column_count == Some(1) {
            self.remove_table_block(&table_block, cx);
        } else {
            self.delete_table_column(&table_block, selection.index, cx);
        }
    }

    fn render_axis_menu_item(
        theme: &Theme,
        id: &'static str,
        label: String,
        enabled: bool,
        danger: bool,
        on_click: fn(&mut Editor, &ClickEvent, &mut Window, &mut Context<Editor>),
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let row = crate::components::menu::menu_item(
            theme,
            id,
            label,
            None,
            enabled,
            danger,
            false,
            false,
            None,
        );
        if enabled {
            row.on_click(cx.listener(on_click)).into_any_element()
        } else {
            row.into_any_element()
        }
    }

    /// 一条分节线。
    fn menu_separator(theme: &Theme) -> AnyElement {
        crate::components::menu::menu_separator(theme).into_any_element()
    }

    pub(crate) fn render_context_menu_overlay(
        &self,
        theme: &Theme,
        viewport: Size<Pixels>,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let menu = self.context_menu.as_ref()?;
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let s = cx.global::<I18nManager>().strings().clone();

        match menu {
            ContextMenuState::Document {
                position,
                open_submenu,
                ..
            } => {
                let rows = self.document_menu_rows();
                // 宽度按当前生效的键位那一列估，用户改过绑定也不会截字。
                let shortcut_of = &|command| document_menu_shortcut(command, cx);
                let panel = DocumentMenuGeometry::measure(&rows, &s, d, shortcut_of);
                // 二级面板与父行顶部对齐；父行离底部太近时由落点函数向上收。
                let submenu_panels = open_submenu.map(|submenu| {
                    let sub_rows = self.document_submenu_rows(submenu, cx);
                    let index = rows.iter().position(|row| {
                        matches!(row, DocumentMenuRow::Submenu { id, .. } if *id == submenu)
                    });
                    (
                        DocumentMenuGeometry::measure(&sub_rows, &s, d, shortcut_of),
                        panel.row_top(index),
                    )
                });
                let (origin, submenu_origin) = document_menu_origins(
                    *position,
                    viewport,
                    &panel,
                    submenu_panels
                        .as_ref()
                        .map(|(geometry, top)| (geometry, *top)),
                    px(d.context_menu_submenu_gap),
                );
                let submenu_panel = match (open_submenu, submenu_origin, submenu_panels.as_ref()) {
                    (Some(submenu), Some(origin), Some((geometry, _))) => Some(
                        self.render_document_submenu_panel(theme, &s, *submenu, origin, geometry, cx),
                    ),
                    _ => None,
                };

                let overlay = div()
                    .id("editor-context-menu-overlay")
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .occlude()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(Self::on_dismiss_context_menu_overlay),
                    )
                    .child(
                        div()
                            .id("editor-context-menu-panel")
                            // 测试按这个名字点名的就是这一份面板的边界（与工具栏
                            // `editor-selection-toolbar` 同一个写法）。
                            .debug_selector(|| "editor-context-menu-panel".to_string())
                            .absolute()
                            .left(origin.x)
                            .top(origin.y)
                            .min_w(panel.size.width)
                            .p(px(d.menu_panel_padding))
                            .flex()
                            .flex_col()
                            .gap(px(d.menu_panel_gap))
                            .occlude()
                            .bg(c.dialog_surface)
                            .border(px(d.dialog_border_width))
                            .border_color(c.dialog_border)
                            .rounded(px(d.menu_panel_radius))
                            .shadow_lg()
                            .on_mouse_down(MouseButton::Left, |_event, _window, cx| {
                                cx.stop_propagation()
                            })
                            .children(
                                rows.into_iter()
                                    .map(|row| self.render_document_menu_row(theme, &s, row, *open_submenu, cx)),
                            ),
                    );

                Some(match submenu_panel {
                    Some(panel) => overlay.child(panel).into_any_element(),
                    None => overlay.into_any_element(),
                })
            }
            ContextMenuState::TableAxis {
                position,
                selection,
            } => {
                let Some(table_block) = self.table_block_by_id(selection.table_block_id, cx) else {
                    return None;
                };
                let table = table_block.read(cx).record.table.clone()?;
                let items = match selection.kind {
                    TableAxisKind::Column => vec![
                        Self::render_axis_menu_item(
                            theme,
                            "table-axis-align-column-left",
                            s.table_axis_align_column_left.clone(),
                            true,
                            false,
                            Self::on_align_table_column_left,
                            cx,
                        ),
                        Self::render_axis_menu_item(
                            theme,
                            "table-axis-align-column-center",
                            s.table_axis_align_column_center.clone(),
                            true,
                            false,
                            Self::on_align_table_column_center,
                            cx,
                        ),
                        Self::render_axis_menu_item(
                            theme,
                            "table-axis-align-column-right",
                            s.table_axis_align_column_right.clone(),
                            true,
                            false,
                            Self::on_align_table_column_right,
                            cx,
                        ),
                        Self::menu_separator(theme).into_any_element(),
                        Self::render_axis_menu_item(
                            theme,
                            "table-axis-move-column-left",
                            s.table_axis_move_column_left.clone(),
                            selection.index > 0,
                            false,
                            Self::on_move_table_column_left,
                            cx,
                        ),
                        Self::render_axis_menu_item(
                            theme,
                            "table-axis-move-column-right",
                            s.table_axis_move_column_right.clone(),
                            selection.index + 1 < table.column_count(),
                            false,
                            Self::on_move_table_column_right,
                            cx,
                        ),
                        Self::menu_separator(theme).into_any_element(),
                        Self::render_axis_menu_item(
                            theme,
                            "table-axis-delete-column",
                            s.table_axis_delete_column.clone(),
                            // Always enabled: deleting the last column removes the
                            // whole table.
                            true,
                            true,
                            Self::on_delete_table_column,
                            cx,
                        ),
                    ],
                    TableAxisKind::Row => {
                        let mut items: Vec<AnyElement> = Vec::new();
                        // The header row (visual index 0) shares the normal row
                        // menu, with its Header Row styling toggle added on top.
                        if selection.index == 0 {
                            let headers_shown =
                                crate::config::EditorSettings::show_table_headers(cx);
                            items.push(
                                div()
                                    .id("table-header-toggle")
                                    .h(px(d.menu_item_height))
                                    .px(px(d.menu_item_padding_x))
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap(px(d.menu_item_padding_x))
                                    .rounded(px(d.menu_item_radius))
                                    .bg(c.dialog_surface)
                                    .text_size(px(d.menu_text_size))
                                    .font_weight(t.dialog_body_weight.to_font_weight())
                                    .text_color(c.dialog_secondary_button_text)
                                    .child(s.table_header_row.clone())
                                    .child(if headers_shown { "✓" } else { "" })
                                    .hover(|this| this.bg(c.dialog_secondary_button_hover))
                                    .cursor_pointer()
                                    .on_click(cx.listener(Self::on_toggle_table_headers))
                                    .into_any_element(),
                            );
                            items.push(
                                Self::menu_separator(theme).into_any_element(),
                            );
                        }
                        items.push(Self::render_axis_menu_item(
                            theme,
                            "table-axis-move-row-up",
                            s.table_axis_move_row_up.clone(),
                            selection.index > 0,
                            false,
                            Self::on_move_table_row_up,
                            cx,
                        ));
                        items.push(Self::render_axis_menu_item(
                            theme,
                            "table-axis-move-row-down",
                            s.table_axis_move_row_down.clone(),
                            selection.index < table.rows.len(),
                            false,
                            Self::on_move_table_row_down,
                            cx,
                        ));
                        items.push(
                            Self::menu_separator(theme).into_any_element(),
                        );
                        // Always enabled: deleting the header promotes the first
                        // body row, and deleting the last remaining row removes
                        // the whole table.
                        items.push(Self::render_axis_menu_item(
                            theme,
                            "table-axis-delete-row",
                            s.table_axis_delete_row.clone(),
                            true,
                            true,
                            Self::on_delete_table_row,
                            cx,
                        ));
                        items
                    }
                };

                Some(
                    div()
                        .id("table-axis-context-menu-overlay")
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        .bottom_0()
                        .occlude()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(Self::on_dismiss_context_menu_overlay),
                        )
                        .child(
                            div()
                                .id("table-axis-context-menu-panel")
                                .absolute()
                                .left(position.x)
                                .top(position.y)
                                .w(px(d.context_menu_axis_panel_width))
                                .p(px(d.menu_panel_padding))
                                .flex()
                                .flex_col()
                                .gap(px(d.menu_panel_gap))
                                .bg(c.dialog_surface)
                                .border(px(d.dialog_border_width))
                                .border_color(c.dialog_border)
                                .rounded(px(d.menu_panel_radius))
                                .shadow_lg()
                                .on_mouse_down(MouseButton::Left, |_event, _window, cx| {
                                    cx.stop_propagation()
                                })
                                .children(items),
                        )
                        .into_any_element(),
                )
            }
            ContextMenuState::Image {
                position,
                local_path,
                address: _,
            } => {
                let strings = cx.global::<I18nManager>().strings().clone();
                let items = vec![
                    Self::render_axis_menu_item(
                        theme,
                        "image-reveal-in-file-manager",
                        strings.image_reveal_in_file_manager.clone(),
                        local_path.is_some(),
                        false,
                        Self::on_image_reveal_in_file_manager,
                        cx,
                    ),
                    Self::render_axis_menu_item(
                        theme,
                        "image-copy-address",
                        strings.image_copy_address.clone(),
                        true,
                        false,
                        Self::on_image_copy_address,
                        cx,
                    ),
                ];
                Some(
                    div()
                        .id("image-context-menu-overlay")
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        .bottom_0()
                        .occlude()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(Self::on_dismiss_context_menu_overlay),
                        )
                        .child(
                            div()
                                .id("image-context-menu-panel")
                                .absolute()
                                .left(position.x)
                                .top(position.y)
                                .w(px(d.context_menu_axis_panel_width))
                                .p(px(d.menu_panel_padding))
                                .flex()
                                .flex_col()
                                .gap(px(d.menu_panel_gap))
                                .bg(c.dialog_surface)
                                .border(px(d.dialog_border_width))
                                .border_color(c.dialog_border)
                                .rounded(px(d.menu_panel_radius))
                                .shadow_lg()
                                .on_mouse_down(MouseButton::Left, |_event, _window, cx| {
                                    cx.stop_propagation()
                                })
                                .children(items),
                        )
                        .into_any_element(),
                )
            }
        }
    }

    pub(crate) fn render_table_insert_dialog_overlay(
        &self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let dialog = self.table_insert_dialog.as_ref()?;
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let s = cx.global::<I18nManager>().strings().clone();

        let stepper =
            |id_prefix: &'static str,
             label: String,
             value: usize,
             on_dec: fn(&mut Editor, &ClickEvent, &mut Window, &mut Context<Editor>),
             on_inc: fn(&mut Editor, &ClickEvent, &mut Window, &mut Context<Editor>)| {
                div()
                    .flex()
                    .flex_col()
                    .gap(px(d.table_insert_stepper_gap))
                    .child(
                        div()
                            .text_size(px(t.dialog_body_size))
                            .font_weight(t.dialog_button_weight.to_font_weight())
                            .text_color(c.dialog_body)
                            .child(label),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(d.table_insert_stepper_gap))
                            .child(
                                div()
                                    .id((id_prefix, 0usize))
                                    .size(px(d.table_insert_stepper_button_size))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(d.table_insert_stepper_radius))
                                    .border(px(d.dialog_border_width))
                                    .border_color(c.dialog_border)
                                    .bg(c.dialog_secondary_button_bg)
                                    .hover(|this| this.bg(c.dialog_secondary_button_hover))
                                    .cursor_pointer()
                                    .text_color(c.dialog_secondary_button_text)
                                    .on_click(cx.listener(on_dec))
                                    .child("-"),
                            )
                            .child(
                                div()
                                    .min_w(px(d.table_insert_stepper_value_min_width))
                                    .h(px(d.table_insert_stepper_button_size))
                                    .px(px(d.table_insert_stepper_value_padding_x))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(d.table_insert_stepper_radius))
                                    .border(px(d.dialog_border_width))
                                    .border_color(c.dialog_border)
                                    .bg(c.dialog_surface)
                                    .text_size(px(t.dialog_body_size))
                                    .text_color(c.dialog_title)
                                    .child(value.to_string()),
                            )
                            .child(
                                div()
                                    .id((id_prefix, 1usize))
                                    .size(px(d.table_insert_stepper_button_size))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(d.table_insert_stepper_radius))
                                    .border(px(d.dialog_border_width))
                                    .border_color(c.dialog_border)
                                    .bg(c.dialog_secondary_button_bg)
                                    .hover(|this| this.bg(c.dialog_secondary_button_hover))
                                    .cursor_pointer()
                                    .text_color(c.dialog_secondary_button_text)
                                    .on_click(cx.listener(on_inc))
                                    .child("+"),
                            ),
                    )
            };

        Some(
            div()
                .id("table-insert-dialog-overlay")
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .bottom_0()
                .occlude()
                .flex()
                .items_center()
                .justify_center()
                .bg(c.dialog_backdrop)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(Self::on_dismiss_context_menu_overlay),
                )
                .child(
                    div()
                        .w_full()
                        .px(px(d.editor_padding))
                        .flex()
                        .justify_center()
                        .child(
                            div()
                                .id("table-insert-dialog")
                                .w(px(d.dialog_width.min(d.table_insert_dialog_width)))
                                .max_w(relative(1.0))
                                .p(px(d.dialog_padding))
                                .flex()
                                .flex_col()
                                .gap(px(d.dialog_gap))
                                .bg(c.dialog_surface)
                                .border(px(d.dialog_border_width))
                                .border_color(c.dialog_border)
                                .rounded(px(d.dialog_radius))
                                .shadow_lg()
                                .on_mouse_down(MouseButton::Left, |_event, _window, cx| {
                                    cx.stop_propagation()
                                })
                                .child(
                                    div()
                                        .text_size(px(t.dialog_title_size))
                                        .font_weight(t.dialog_title_weight.to_font_weight())
                                        .text_color(c.dialog_title)
                                        .child(s.table_insert_title.clone()),
                                )
                                .child(
                                    div()
                                        .text_size(px(t.dialog_body_size))
                                        .font_weight(t.dialog_body_weight.to_font_weight())
                                        .text_color(c.dialog_body)
                                        .child(s.table_insert_description.clone()),
                                )
                                .child(stepper(
                                    "table-body-rows",
                                    s.table_insert_body_rows.clone(),
                                    dialog.body_rows,
                                    Self::on_table_rows_decrement,
                                    Self::on_table_rows_increment,
                                ))
                                .child(stepper(
                                    "table-columns",
                                    s.table_insert_columns.clone(),
                                    dialog.columns,
                                    Self::on_table_columns_decrement,
                                    Self::on_table_columns_increment,
                                ))
                                .child(
                                    div()
                                        .flex()
                                        .justify_end()
                                        .gap(px(d.dialog_button_gap))
                                        .child(
                                            div()
                                                .id("cancel-table-insert-dialog")
                                                .h(px(d.dialog_button_height))
                                                .px(px(d.dialog_button_padding_x))
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .rounded(px((d.dialog_radius - 4.0).max(0.0)))
                                                .border(px(d.dialog_border_width))
                                                .border_color(c.dialog_border)
                                                .bg(c.dialog_secondary_button_bg)
                                                .hover(|this| {
                                                    this.bg(c.dialog_secondary_button_hover)
                                                })
                                                .cursor_pointer()
                                                .text_size(px(t.dialog_button_size))
                                                .font_weight(
                                                    t.dialog_button_weight.to_font_weight(),
                                                )
                                                .text_color(c.dialog_secondary_button_text)
                                                .on_click(
                                                    cx.listener(
                                                        Self::on_cancel_table_insert_dialog,
                                                    ),
                                                )
                                                .child(s.table_insert_cancel.clone()),
                                        )
                                        .child(
                                            div()
                                                .id("confirm-table-insert-dialog")
                                                .h(px(d.dialog_button_height))
                                                .px(px(d.dialog_button_padding_x))
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .rounded(px((d.dialog_radius - 4.0).max(0.0)))
                                                .bg(c.dialog_primary_button_bg)
                                                .hover(|this| {
                                                    this.bg(c.dialog_primary_button_hover)
                                                })
                                                .cursor_pointer()
                                                .text_size(px(t.dialog_button_size))
                                                .font_weight(
                                                    t.dialog_button_weight.to_font_weight(),
                                                )
                                                .text_color(c.dialog_primary_button_text)
                                                .on_click(
                                                    cx.listener(
                                                        Self::on_confirm_table_insert_dialog,
                                                    ),
                                                )
                                                .child(s.table_insert_confirm.clone()),
                                        ),
                                ),
                        ),
                )
                .into_any_element(),
        )
    }
}

impl Editor {
    /// 顶部那一行纯图标（Windows 11 的那一种）：六颗 26 的格子，「撤销/重做」与
    /// 「剪切/复制/粘贴/粘贴为纯文本」两组之间一条竖线。格子上不放文字，标签与生效键位
    /// 写在悬停说明里；点得动的格子才有底色与小手，点不动的连悬停反馈也没有——
    /// 与选中工具栏那九颗同一口径。
    fn render_document_menu_quick_actions(
        &self,
        theme: &Theme,
        strings: &crate::i18n::I18nStrings,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let mut cells: Vec<AnyElement> = Vec::new();
        for (index, command) in DOCUMENT_MENU_QUICK_ACTIONS.into_iter().enumerate() {
            if index == QUICK_ACTION_GROUP_BREAK {
                cells.push(
                    div()
                        .flex_shrink_0()
                        .w(px(QUICK_ACTION_DIVIDER_WIDTH))
                        .h(px(16.0))
                        .rounded(px(0.5))
                        .bg(c.dialog_border)
                        .into_any_element(),
                );
            }
            let enabled = self.quick_action_is_available(command, cx);
            let icon = document_menu_command_icon(command).unwrap_or_default();
            let tooltip = quick_action_tooltip(command, strings, cx);
            let name = command.row_name();
            let selector = format!("menu-quick-action-{name}");
            let button = div()
                .id(SharedString::from(selector.clone()))
                .size(px(QUICK_ACTION_BUTTON_SIZE))
                .flex()
                .items_center()
                .justify_center()
                .flex_shrink_0()
                .rounded(px(d.menu_item_radius))
                .text_color(if enabled {
                    c.dialog_secondary_button_text
                } else {
                    c.dialog_muted
                })
                .debug_selector(move || selector.clone())
                .tooltip(move |_, cx| {
                    cx.new(|_| crate::components::HoverPreviewTooltip {
                        label: tooltip.clone().into(),
                    })
                    .into()
                })
                .child(
                    svg()
                        .path(icon)
                        .size(px(crate::components::menu::MENU_ICON_SIZE))
                        .text_color(if enabled {
                            c.dialog_secondary_button_text
                        } else {
                            c.dialog_muted
                        }),
                );
            let button = if enabled {
                button
                    .cursor_pointer()
                    .hover(|this| this.bg(c.dialog_secondary_button_hover))
                    .active(|this| this.opacity(0.92))
                    .on_click(cx.listener(move |editor, _event, window, cx| {
                        editor.run_document_menu_command(command, window, cx);
                    }))
            } else {
                button
            };
            cells.push(button.into_any_element());
        }
        div()
            .id("editor-context-menu-quick-actions")
            .w_full()
            .h(px(QUICK_ACTION_BUTTON_SIZE))
            .flex()
            .items_center()
            .gap(px(QUICK_ACTION_GAP))
            .children(cells)
            .into_any_element()
    }

    /// 正文右键菜单的一行。行的视觉状态在 `components::menu::menu_item` 里定，
    /// 这里只管「点下去派发哪个动作」与「悬停展开哪一块二级菜单」。
    fn render_document_menu_row(
        &self,
        theme: &Theme,
        strings: &crate::i18n::I18nStrings,
        row: DocumentMenuRow,
        open_submenu: Option<DocumentSubmenu>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match row {
            DocumentMenuRow::Separator => Self::menu_separator(theme).into_any_element(),
            DocumentMenuRow::QuickActions => {
                self.render_document_menu_quick_actions(theme, strings, cx)
            }
            DocumentMenuRow::Item {
                command,
                name,
                enabled,
            } => {
                let item = crate::components::menu::menu_item(
                    theme,
                    name,
                    document_menu_label(command, strings),
                    document_menu_shortcut(command, cx),
                    enabled,
                    false,
                    false,
                    false,
                    document_menu_command_icon(command).map(SharedString::from),
                );
                if enabled {
                    item.on_click(cx.listener(move |editor, _event, window, cx| {
                        editor.run_document_menu_command(command, window, cx);
                    }))
                    .into_any_element()
                } else {
                    item.into_any_element()
                }
            }
            DocumentMenuRow::Submenu { id, name } => {
                let label = document_submenu_label(id, strings);
                crate::components::menu::menu_item(
                    theme,
                    name,
                    label,
                    None,
                    true,
                    false,
                    true,
                    open_submenu == Some(id),
                    document_submenu_icon(id).map(SharedString::from),
                )
                    .on_hover(cx.listener(move |editor, hovered: &bool, _window, cx| {
                        editor.set_document_menu_hover(*hovered, Some(id), cx);
                    }))
                    .into_any_element()
            }
        }
    }

    /// 二级菜单的面板：与主菜单同款外观，贴在主菜单右侧。
    fn render_document_submenu_panel(
        &self,
        theme: &Theme,
        strings: &crate::i18n::I18nStrings,
        submenu: DocumentSubmenu,
        origin: Point<Pixels>,
        geometry: &DocumentMenuGeometry,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let d = &theme.dimensions;
        let rows = self.document_submenu_rows(submenu, cx);
        let panel_id = match submenu {
            DocumentSubmenu::Format => "editor-context-menu-format",
            DocumentSubmenu::Paragraph => "editor-context-menu-paragraph",
            DocumentSubmenu::Insert => "editor-context-menu-insert",
        };
        div()
            .id(panel_id)
            .absolute()
            .left(origin.x)
            .top(origin.y)
            .min_w(geometry.size.width)
            .p(px(d.menu_panel_padding))
            .flex()
            .flex_col()
            .gap(px(d.menu_panel_gap))
            .occlude()
            .bg(theme.colors.dialog_surface)
            .border(px(d.dialog_border_width))
            .border_color(theme.colors.dialog_border)
            .rounded(px(d.menu_panel_radius))
            .shadow_lg()
            .on_mouse_down(MouseButton::Left, |_event, _window, cx| {
                cx.stop_propagation()
            })
            .on_hover(cx.listener(move |editor, hovered: &bool, _window, cx| {
                editor.set_document_menu_hover(*hovered, Some(submenu), cx);
            }))
            .children(
                rows.into_iter()
                    .map(|row| self.render_document_menu_row(theme, strings, row, None, cx)),
            )
            .into_any_element()
    }
}
