use super::*;
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Icon,
    menu::{PopupMenu, PopupMenuItem},
};

use super::document_menu::{
    QUICK_ACTION_BUTTON_SIZE, QUICK_ACTION_DIVIDER_WIDTH, QUICK_ACTION_GAP,
    QUICK_ACTION_GROUP_BREAK, submenu_bridge_span,
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

    /// 一条分节线。
    fn menu_separator(theme: &Theme) -> AnyElement {
        crate::components::menu::menu_separator(theme).into_any_element()
    }

    pub(crate) fn render_context_menu_overlay(
        &mut self,
        theme: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !matches!(self.context_menu, Some(ContextMenuState::Document { .. })) {
            return self.render_object_context_menu(window, cx);
        }
        let menu = self.context_menu.as_ref()?;
        let viewport = window.viewport_size();
        let c = &theme.colors;
        let d = &theme.dimensions;
        let s = cx.global::<I18nManager>().strings().clone();

        match menu {
            ContextMenuState::Document {
                position,
                open_submenu,
                ..
            } => {
                let rows = self.document_menu_rows(cx);
                // 平台快捷键的文字宽度不同，定位与绘制必须共用实测尺寸。
                let panel = DocumentMenuGeometry::measure_in_window(&rows, &s, theme, window, cx);
                // 二级面板与父行顶部对齐；父行离底部太近时由落点函数向上收。
                let submenu_panels = open_submenu.map(|submenu| {
                    let sub_rows = self.document_submenu_rows(submenu, cx);
                    let index = rows.iter().position(|row| {
                        matches!(row, DocumentMenuRow::Submenu { id, .. } if *id == submenu)
                    });
                    (
                        DocumentMenuGeometry::measure_in_window(&sub_rows, &s, theme, window, cx),
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
                // 两块面板中间那条缝的感应带：从主面板边缘一直盖到二级面板外沿，
                // 高度跟着二级面板走。缝只有几个像素宽，鼠标从父行横过去时就会
                // 经过它，停在那儿不该被当成离开菜单。
                // 与二级面板有重叠：两块各记一份悬停，同一次移动里谁后到都不影响判定。
                let submenu_bridge = match (submenu_origin, submenu_panels.as_ref()) {
                    (Some(submenu_origin), Some((geometry, _))) => {
                        let (left, right) = submenu_bridge_span(
                            origin.x,
                            panel.size.width,
                            submenu_origin.x,
                            geometry.size.width,
                        );
                        Some(
                            div()
                                .id("editor-context-menu-submenu-bridge")
                                .debug_selector(|| "editor-context-menu-submenu-bridge".to_string())
                                .absolute()
                                .left(left)
                                .top(submenu_origin.y)
                                .w(right - left)
                                .h(geometry.size.height)
                                .on_hover(cx.listener(
                                    |editor, hovered: &bool, _window, cx| {
                                        editor.set_document_submenu_bridge_hover(*hovered, cx);
                                    },
                                ))
                                .into_any_element(),
                        )
                    }
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
                            .w(panel.size.width)
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

                Some(match (submenu_panel, submenu_bridge) {
                    (Some(panel), Some(bridge)) => {
                        overlay.child(bridge).child(panel).into_any_element()
                    }
                    (Some(panel), None) => overlay.child(panel).into_any_element(),
                    (None, Some(bridge)) => overlay.child(bridge).into_any_element(),
                    (None, None) => overlay.into_any_element(),
                })
            }
            ContextMenuState::TableAxis { .. } | ContextMenuState::Image { .. } => None,
        }
    }

    fn render_object_context_menu(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let Some(request) = self.context_menu.as_ref() else {
            self.context_menu_popup = None;
            return None;
        };
        let (position, panel_id) = match request {
            ContextMenuState::TableAxis { position, .. } => {
                (*position, "table-axis-context-menu-panel")
            }
            ContextMenuState::Image { position, .. } => (*position, "image-context-menu-panel"),
            ContextMenuState::Document { .. } => return None,
        };
        if self.context_menu_popup.is_none() {
            let strings = cx.global::<I18nManager>().strings_arc();
            let owner = cx.entity().downgrade();
            let row = |label: &str, enabled: bool, checked: bool,
                handler: fn(&mut Self, &ClickEvent, &mut Window, &mut Context<Self>)| {
                let editor = owner.clone();
                let item = PopupMenuItem::new(label.to_owned()).disabled(!enabled).checked(checked);
                if !enabled { return item; }
                item.on_click(move |event, window, cx| {
                        if let Err(error) = editor.update(cx, |editor, cx| handler(editor, event, window, cx)) {
                            eprintln!("执行正文对象菜单失败：{error}");
                        }
                    })
            };
            let writable = !self.is_read_only();
            let trash = Icon::new(IconName::Trash).text_color(cx.theme().danger);
            let mut target_focus = None;
            let items = match request {
                ContextMenuState::TableAxis { selection, .. } => {
                    let block = self.table_block_by_id(selection.table_block_id, cx)?;
                    let table = block.read(cx).record.table.as_ref()?;
                    match selection.kind {
                        TableAxisKind::Column => {
                            let alignment = table
                                .alignments
                                .get(selection.index)
                                .copied()
                                .unwrap_or(TableColumnAlignment::Default);
                            vec![
                                row(
                                    &strings.table_axis_align_column_left,
                                    writable,
                                    alignment == TableColumnAlignment::Left,
                                    Self::on_align_table_column_left,
                                ),
                                row(
                                    &strings.table_axis_align_column_center,
                                    writable,
                                    alignment == TableColumnAlignment::Center,
                                    Self::on_align_table_column_center,
                                ),
                                row(
                                    &strings.table_axis_align_column_right,
                                    writable,
                                    alignment == TableColumnAlignment::Right,
                                    Self::on_align_table_column_right,
                                ),
                                PopupMenuItem::separator(),
                                row(
                                    &strings.table_axis_move_column_left,
                                    writable && selection.index > 0,
                                    false,
                                    Self::on_move_table_column_left,
                                ),
                                row(
                                    &strings.table_axis_move_column_right,
                                    writable && selection.index + 1 < table.column_count(),
                                    false,
                                    Self::on_move_table_column_right,
                                ),
                                PopupMenuItem::separator(),
                                row(
                                    &strings.table_axis_delete_column,
                                    writable,
                                    false,
                                    Self::on_delete_table_column,
                                )
                                .icon(trash),
                            ]
                        }
                        TableAxisKind::Row => {
                            let mut items = Vec::new();
                            if selection.index == 0 {
                                items.push(row(
                                    &strings.table_header_row,
                                    writable,
                                    crate::config::EditorSettings::show_table_headers(cx),
                                    Self::on_toggle_table_headers,
                                ));
                                items.push(PopupMenuItem::separator());
                            }
                            items.extend([
                                row(
                                    &strings.table_axis_move_row_up,
                                    writable && selection.index > 0,
                                    false,
                                    Self::on_move_table_row_up,
                                ),
                                row(
                                    &strings.table_axis_move_row_down,
                                    writable && selection.index < table.rows.len(),
                                    false,
                                    Self::on_move_table_row_down,
                                ),
                                PopupMenuItem::separator(),
                                row(
                                    &strings.table_axis_delete_row,
                                    writable,
                                    false,
                                    Self::on_delete_table_row,
                                )
                                .icon(trash),
                            ]);
                            items
                        }
                    }
                }
                ContextMenuState::Image {
                    entity_id,
                    local_path,
                    ..
                } => {
                    let current_percent = self.focusable_entity_by_id(*entity_id).map(|block| {
                        let block = block.read(cx);
                        target_focus = Some(block.focus_handle.clone());
                        (block.image_width_factor * 100.0).round() as u8
                    });
                    let mut items = Vec::new();
                    if local_path.is_some() {
                        items.push(row(
                            &strings.image_reveal_in_file_manager,
                            true,
                            false,
                            Self::on_image_reveal_in_file_manager,
                        ));
                    }
                    items.extend([
                        row(
                            &strings.image_copy_address,
                            true,
                            false,
                            Self::on_image_copy_address,
                        ),
                        PopupMenuItem::separator(),
                        PopupMenuItem::label(strings.image_scale.clone()),
                    ]);
                    for percent in [25u8, 50, 75, 100] {
                        let editor = owner.clone();
                        items.push(
                            PopupMenuItem::new(format!("{percent}%"))
                                .disabled(!writable)
                                .checked(current_percent == Some(percent))
                                .on_click(move |_, _, cx| {
                                    if let Err(error) = editor.update(cx, |editor, cx| {
                                        editor.set_image_scale_from_menu(percent, cx)
                                    }) {
                                        eprintln!("调整图片比例失败：{error}");
                                    }
                                }),
                        );
                    }
                    items
                }
                ContextMenuState::Document { .. } => return None,
            };
            // 连续打开菜单时，窗口焦点可能仍属于旧菜单；返回位置取自正文。
            let return_focus = target_focus
                .or_else(|| {
                    self.active_entity_id
                        .and_then(|id| self.focusable_entity_by_id(id))
                        .map(|block| block.read(cx).focus_handle.clone())
                })
                .or_else(|| window.focused(cx))
                .unwrap_or_else(|| cx.focus_handle());
            let focus = return_focus.clone();
            let popup = PopupMenu::build(window, cx, move |mut menu, window, _| {
                menu = menu
                    .action_context(focus)
                    .min_w(window.rem_size() * 13.75)
                    .scrollable(true);
                for item in items {
                    menu = menu.item(item);
                }
                menu
            });
            let dismiss = cx.subscribe(&popup, |editor, menu, _: &DismissEvent, cx| {
                if editor
                    .context_menu_popup
                    .as_ref()
                    .is_some_and(|popup| popup.view == menu)
                {
                    editor.close_context_menu(cx);
                }
            });
            window.focus(&popup.focus_handle(cx), cx);
            self.context_menu_popup = Some(ContextMenuPopup {
                view: popup,
                return_focus,
                _dismiss: dismiss,
            });
        }
        Some(crate::components::menu::popup_menu_layer(
            panel_id,
            position,
            &self.context_menu_popup.as_ref()?.view,
            window,
        ))
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
                        // 竖线与图标一样高，不跟着格子长到 26。
                        .h(px(crate::components::menu::MENU_ICON_SIZE))
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
            // 六颗均分整条宽度：按自然宽度挤在左边，右边空出来的那一截正是用户看着
            // 别扭的地方。格子高 26，宽交给 flex（面板最窄就是这一条自己的 169，
            // 所以每颗不会低于 26）。
            let button = div()
                .id(SharedString::from(selector.clone()))
                .flex_1()
                .h(px(QUICK_ACTION_BUTTON_SIZE))
                .flex()
                .items_center()
                .justify_center()
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
            // 测试按这个名字量二级面板的边界（缝上的落点靠它算出来）。
            .debug_selector(move || panel_id.to_string())
            .absolute()
            .left(origin.x)
            .top(origin.y)
            .w(geometry.size.width)
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
                editor.set_document_submenu_panel_hover(*hovered, cx);
            }))
            .children(
                rows.into_iter()
                    .map(|row| self.render_document_menu_row(theme, strings, row, None, cx)),
            )
            .into_any_element()
    }
}
