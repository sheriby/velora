use super::*;
use gpui_kit::assets::IconName;
use gpui_kit::base::{Dialog, ElementExt as _, FocusTrapElement as _};
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
        let s = cx.global::<I18nManager>().strings_arc();
        // 先把菜单状态快照下来，后面写几何缓存要用 &mut self。
        let (position, open_submenu, menu_focus, rows) = match menu {
            ContextMenuState::Document {
                position,
                open_submenu,
                focus,
                rows,
                ..
            } => (*position, *open_submenu, focus.clone(), rows.clone()),
            _ => return None,
        };

        // 打开时拍下的行数据与几何：菜单开着期间行/主题不变，逐帧重建
        // 行、逐帧重测 label 宽度都是白付的。主题代数变了才重测。
        let theme_generation = cx
            .global::<crate::theme::ThemeManager>()
            .current_theme_generation();
        let cached_panel = match self.context_menu.as_ref() {
            Some(ContextMenuState::Document {
                geometry: Some((generation, geometry)),
                ..
            }) if *generation == theme_generation => Some(geometry.clone()),
            _ => None,
        };
        let panel = match cached_panel {
            Some(panel) => panel,
            None => {
                // 平台快捷键的文字宽度不同，定位与绘制必须共用实测尺寸。
                self.context_menu_measurements
                    .set(self.context_menu_measurements.get() + 1);
                let panel = std::rc::Rc::new(DocumentMenuGeometry::measure_in_window(
                    rows.as_slice(),
                    &s,
                    theme,
                    window,
                    cx,
                ));
                if let Some(ContextMenuState::Document { geometry, .. }) =
                    self.context_menu.as_mut()
                {
                    *geometry = Some((theme_generation, panel.clone()));
                }
                panel
            }
        };
        // 二级面板与父行顶部对齐；父行离底部太近时由落点函数向上收。
        let submenu_panels = open_submenu.map(|submenu| {
            let cached = match self.context_menu.as_ref() {
                Some(ContextMenuState::Document {
                    submenu_cache: Some((id, rows, geometry)),
                    ..
                }) if *id == submenu => Some((rows.clone(), geometry.clone())),
                _ => None,
            };
            let (sub_rows, sub_geometry) = match cached {
                Some(cached) => cached,
                None => {
                    let sub_rows = std::rc::Rc::new(self.document_submenu_rows(submenu, cx));
                    let sub_geometry = std::rc::Rc::new(DocumentMenuGeometry::measure_in_window(
                        sub_rows.as_slice(),
                        &s,
                        theme,
                        window,
                        cx,
                    ));
                    if let Some(ContextMenuState::Document { submenu_cache, .. }) =
                        self.context_menu.as_mut()
                    {
                        *submenu_cache = Some((submenu, sub_rows.clone(), sub_geometry.clone()));
                    }
                    (sub_rows, sub_geometry)
                }
            };
            let index = rows.iter().position(|row| {
                matches!(row, DocumentMenuRow::Submenu { id, .. } if *id == submenu)
            });
            (sub_rows, sub_geometry, panel.row_top(index))
        });
        let (origin, submenu_origin) = document_menu_origins(
            position,
            viewport,
            &panel,
            submenu_panels
                .as_ref()
                .map(|(_, geometry, top)| (geometry.as_ref(), *top)),
            px(d.context_menu_submenu_gap),
        );
        let submenu_panel = match (open_submenu, submenu_origin, submenu_panels.as_ref()) {
            (Some(submenu), Some(origin), Some((sub_rows, geometry, _))) => {
                Some(self.render_document_submenu_panel(
                    theme,
                    &s,
                    submenu,
                    origin,
                    sub_rows.as_slice(),
                    geometry,
                    cx,
                ))
            }
            _ => None,
        };
        // 两块面板中间那条缝的感应带：从主面板边缘一直盖到二级面板外沿，
        // 高度跟着二级面板走。缝只有几个像素宽，鼠标从父行横过去时就会
        // 经过它，停在那儿不该被当成离开菜单。
        // 与二级面板有重叠：两块各记一份悬停，同一次移动里谁后到都不影响判定。
        let submenu_bridge = match (submenu_origin, submenu_panels.as_ref()) {
            (Some(submenu_origin), Some((_, geometry, _))) => {
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
                        .on_hover(cx.listener(|editor, hovered: &bool, _window, cx| {
                            editor.set_document_submenu_bridge_hover(*hovered, cx);
                        }))
                        .into_any_element(),
                )
            }
            _ => None,
        };

        // 菜单要接管键盘：焦点落在面板上并登记焦点陷阱，打字、
        // vim 普通模式按键都留在菜单里，不再打进遮罩后面的正文。
        // Esc 走全局的 DismissTransientUi 绑定照常关闭并归还焦点。
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
                    .on_prepaint({
                        // 与模态同一套做法：挂载的第一帧先记住正文里的焦点，
                        // 再把焦点交给菜单；关闭路径负责归还。
                        let owner = cx.entity().downgrade();
                        move |_, window, cx| {
                            if let Err(error) = owner.update(cx, |editor, cx| {
                                let Some(ContextMenuState::Document {
                                    focus,
                                    restore_focus,
                                    ..
                                }) = editor.context_menu.as_mut()
                                else {
                                    return;
                                };
                                if restore_focus.is_some() {
                                    return;
                                }
                                *restore_focus =
                                    Some(window.focused(cx).map(|focus| focus.downgrade()));
                                let focus = focus.clone();
                                focus.focus(window, cx);
                            }) {
                                eprintln!("聚焦正文菜单失败：{error}");
                            }
                        }
                    })
                    .focus_trap("editor-document-menu-focus-trap", &menu_focus)
                    .children(rows.iter().cloned().map(|row| {
                        self.render_document_menu_row(theme, &s, row, open_submenu, cx)
                    })),
            );

        Some(match (submenu_panel, submenu_bridge) {
            (Some(panel), Some(bridge)) => overlay.child(bridge).child(panel).into_any_element(),
            (Some(panel), None) => overlay.child(panel).into_any_element(),
            (None, Some(bridge)) => overlay.child(bridge).into_any_element(),
            (None, None) => overlay.into_any_element(),
        })
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
            ContextMenuState::TableCell { position, .. } => {
                (*position, "table-cell-context-menu-panel")
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
                ContextMenuState::TableCell {
                    table_block_id,
                    cell,
                    ..
                } => {
                    let Some(table_block) = self.table_block_by_id(*table_block_id, cx) else {
                        return None;
                    };
                    if table_block.read(cx).record.table.is_none() {
                        return None;
                    }
                    let mut items = Vec::new();
                    // 表头之上没有行，不渲染一个按了没反应的禁用项。
                    if cell.row > 0 {
                        items.push(
                            row(
                                &strings.table_cell_insert_row_above,
                                writable,
                                false,
                                Self::on_insert_table_row_above,
                            )
                            .icon(Icon::new(IconName::ArrowUp)),
                        );
                    }
                    items.extend([
                        row(
                            &strings.table_cell_insert_row_below,
                            writable,
                            false,
                            Self::on_insert_table_row_below,
                        )
                        .icon(Icon::new(IconName::ArrowDown)),
                        row(
                            &strings.table_cell_insert_column_left,
                            writable,
                            false,
                            Self::on_insert_table_column_left,
                        )
                        .icon(Icon::new(IconName::ArrowLeft)),
                        row(
                            &strings.table_cell_insert_column_right,
                            writable,
                            false,
                            Self::on_insert_table_column_right,
                        )
                        .icon(Icon::new(IconName::ArrowRight)),
                        PopupMenuItem::separator(),
                        row(
                            &strings.table_axis_delete_row,
                            writable,
                            false,
                            Self::on_delete_table_cell_row,
                        )
                        .icon(trash.clone()),
                        row(
                            &strings.table_axis_delete_column,
                            writable,
                            false,
                            Self::on_delete_table_cell_column,
                        )
                        .icon(trash.clone()),
                    ]);
                    items
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
        &mut self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let dialog = self.table_insert_dialog.as_ref()?;
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let s = cx.global::<I18nManager>().strings_arc();
        let dialog_focus = dialog.focus.clone();
        let [rows_dec, rows_inc, columns_dec, columns_inc] = dialog.steppers.clone();
        let owner = cx.entity().downgrade();

        // 键盘 Enter 激活当前那颗步进按钮：Confirm 动作在按钮上就地消费掉，
        // 不再冒泡成整个对话框的「确认」。
        let confirm_step = |owner: &gpui::WeakEntity<Editor>,
                            columns: bool,
                            increment: bool|
         -> std::rc::Rc<dyn Fn(&gpui_kit::base::actions::Confirm, &mut Window, &mut App)> {
            let owner = owner.clone();
            std::rc::Rc::new(
                move |_: &gpui_kit::base::actions::Confirm, _window, cx| {
                    if let Err(error) = owner.update(cx, |editor, cx| {
                        editor.step_table_dialog_value(columns, increment, cx);
                    }) {
                        eprintln!("步进表格行列失败：{error}");
                    }
                },
            )
        };

        let stepper =
            |id_prefix: &'static str,
             label: String,
             value: usize,
             dec_focus: FocusHandle,
             inc_focus: FocusHandle,
             on_dec: fn(&mut Editor, &ClickEvent, &mut Window, &mut Context<Editor>),
             on_inc: fn(&mut Editor, &ClickEvent, &mut Window, &mut Context<Editor>),
             confirm_dec: std::rc::Rc<dyn Fn(&gpui_kit::base::actions::Confirm, &mut Window, &mut App)>,
             confirm_inc: std::rc::Rc<dyn Fn(&gpui_kit::base::actions::Confirm, &mut Window, &mut App)>| {
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
                                    .track_focus(&dec_focus)
                                    .on_click(cx.listener(on_dec))
                                    .on_action({
                                        let confirm_dec = confirm_dec.clone();
                                        move |event: &gpui_kit::base::actions::Confirm,
                                              window,
                                              cx| {
                                            confirm_dec(event, window, cx)
                                        }
                                    })
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
                                    .track_focus(&inc_focus)
                                    .on_click(cx.listener(on_inc))
                                    .on_action({
                                        let confirm_inc = confirm_inc.clone();
                                        move |event: &gpui_kit::base::actions::Confirm,
                                              window,
                                              cx| {
                                            confirm_inc(event, window, cx)
                                        }
                                    })
                                    .child("+"),
                            ),
                    )
            };

        let panel = div()
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
            .on_prepaint({
                // 与模态同一套做法：挂载的第一帧先记住正文里的焦点，再把焦点
                // 交给对话框。没有它，回车与 vim 按键都会打进遮罩后面的正文。
                let owner = owner.clone();
                move |_, window, cx| {
                    if let Err(error) = owner.update(cx, |editor, cx| {
                        let Some(dialog) = editor.table_insert_dialog.as_mut() else {
                            return;
                        };
                        if dialog.restore_focus.is_some() {
                            return;
                        }
                        dialog.restore_focus =
                            Some(window.focused(cx).map(|focus| focus.downgrade()));
                        let focus = dialog.focus.clone();
                        focus.focus(window, cx);
                    }) {
                        eprintln!("聚焦表格插入对话框失败：{error}");
                    }
                }
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
                rows_dec,
                rows_inc,
                Self::on_table_rows_decrement,
                Self::on_table_rows_increment,
                confirm_step(&owner, false, false),
                confirm_step(&owner, false, true),
            ))
            .child(stepper(
                "table-columns",
                s.table_insert_columns.clone(),
                dialog.columns,
                columns_dec,
                columns_inc,
                Self::on_table_columns_decrement,
                Self::on_table_columns_increment,
                confirm_step(&owner, true, false),
                confirm_step(&owner, true, true),
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
                            .hover(|this| this.bg(c.dialog_secondary_button_hover))
                            .cursor_pointer()
                            .text_size(px(t.dialog_button_size))
                            .font_weight(t.dialog_button_weight.to_font_weight())
                            .text_color(c.dialog_secondary_button_text)
                            .on_click(cx.listener(Self::on_cancel_table_insert_dialog))
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
                            .hover(|this| this.bg(c.dialog_primary_button_hover))
                            .cursor_pointer()
                            .text_size(px(t.dialog_button_size))
                            .font_weight(t.dialog_button_weight.to_font_weight())
                            .text_color(c.dialog_primary_button_text)
                            .on_click(cx.listener(Self::on_confirm_table_insert_dialog))
                            .child(s.table_insert_confirm.clone()),
                    ),
            );

        Some(
            // 真模态走 Kit Dialog：焦点陷阱挡下外围按键、Esc 取消、Enter 确认、
            // 遮罩可点，绘制层级也在补全/公式浮层之上——自绘 overlay 只挡鼠标。
            Dialog::new(cx)
                .focus_handle(dialog_focus)
                .layer(crate::editor::modal::next_modal_layer(), true)
                .backdrop(div().size_full().bg(c.dialog_backdrop))
                .popup(
                    div()
                        .w_full()
                        .px(px(d.editor_padding))
                        .flex()
                        .justify_center()
                        .child(panel),
                )
                .on_ok({
                    let owner = owner.clone();
                    move |_, _window, cx| {
                        if let Err(error) = owner.update(cx, |editor, cx| {
                            editor.insert_table_from_dialog(cx);
                        }) {
                            eprintln!("确认表格插入失败：{error}");
                        }
                        false
                    }
                })
                .on_cancel({
                    let owner = owner.clone();
                    move |_, window, cx| {
                        if let Err(error) = owner.update(cx, |editor, cx| {
                            editor.cancel_table_insert_dialog_with_focus(window, cx);
                        }) {
                            eprintln!("取消表格插入失败：{error}");
                        }
                        false
                    }
                })
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
        rows: &[DocumentMenuRow],
        geometry: &DocumentMenuGeometry,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let d = &theme.dimensions;
        let panel_id = match submenu {
            DocumentSubmenu::Format => "editor-context-menu-format",
            DocumentSubmenu::Paragraph => "editor-context-menu-paragraph",
            DocumentSubmenu::Insert => "editor-context-menu-insert",
            DocumentSubmenu::Ai => "editor-context-menu-ai",
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
                    .map(|row| self.render_document_menu_row(theme, strings, row.clone(), None, cx)),
            )
            .into_any_element()
    }
}
