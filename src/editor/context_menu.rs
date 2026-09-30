//! Rendered-mode context menus and native table insertion dialog.

pub(super) use std::path::PathBuf;
pub(super) use std::time::Duration;

pub(super) use gpui::*;

pub(super) use super::{Editor, TableAxisSelection, ViewMode};
pub(super) use crate::components::{DismissTransientUi, TableAxisKind, TableColumnAlignment, TableData};
use crate::i18n::I18nManager;
use crate::theme::Theme;

/// Target block position for inserting a native table.
#[derive(Clone, Copy)]
pub(super) enum TableInsertTarget {
    /// Insert the table immediately after the referenced block.
    After(EntityId),
    /// Append the table to the end of the current root list.
    Append,
}

/// Rendered-mode context menu currently open in the editor.
pub(super) enum ContextMenuState {
    /// General block context menu with an insert submenu.
    Insert {
        position: Point<Pixels>,
        target: TableInsertTarget,
        insert_hovered: bool,
        submenu_hovered: bool,
        submenu_open: bool,
    },
    /// Table row or column context menu for an existing native table.
    TableAxis {
        position: Point<Pixels>,
        selection: TableAxisSelection,
    },
    /// Image block menu: reveal in file manager / copy address.
    Image {
        position: Point<Pixels>,
        /// 本地图片绝对路径（远程图 None，「在文件管理器中显示」禁用）。
        local_path: Option<PathBuf>,
        /// 原始地址（相对路径或 URL），「复制图片地址」用。
        address: String,
    },
}

/// State for the table insertion dialog opened from the context menu.
pub(super) struct TableInsertDialogState {
    pub target: TableInsertTarget,
    pub body_rows: usize,
    pub columns: usize,
}

impl Editor {
    pub(super) fn root_ancestor_entity_id(&self, entity_id: EntityId) -> EntityId {
        let mut current = entity_id;
        while let Some(location) = self.document.find_block_location(current) {
            let Some(parent) = location.parent else {
                break;
            };
            current = parent.entity_id();
        }
        current
    }

    fn open_insert_context_menu(
        &mut self,
        position: Point<Pixels>,
        target: TableInsertTarget,
        cx: &mut Context<Self>,
    ) {
        if self.view_mode != ViewMode::Rendered {
            return;
        }

        self.close_menu_bar(cx);
        self.context_menu_submenu_close_task = None;
        self.context_menu = Some(ContextMenuState::Insert {
            position,
            target,
            insert_hovered: false,
            submenu_hovered: false,
            submenu_open: false,
        });
        cx.notify();
    }

    /// 打开图片块右键菜单。
    pub(super) fn open_image_context_menu(
        &mut self,
        position: Point<Pixels>,
        local_path: Option<PathBuf>,
        address: String,
        cx: &mut Context<Self>,
    ) {
        self.context_menu = Some(ContextMenuState::Image {
            position,
            local_path,
            address,
        });
        cx.notify();
    }

    /// 「在文件管理器中显示」：macOS open -R / Windows explorer /select /
    /// Linux xdg-open 目录。失败走应用内模态。
    pub(super) fn on_image_reveal_in_file_manager(
        &mut self,
        _: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(ContextMenuState::Image {
            local_path: Some(path),
            ..
        }) = self.context_menu.take()
        else {
            return;
        };
        let (program, args) = reveal_command_spec(&path);
        match std::process::Command::new(program).args(&args).spawn() {
            Ok(_) => {}
            Err(error) => {
                let strings = cx.global::<I18nManager>().strings().clone();
                let detail: SharedString = error.to_string().into();
                self.show_message_modal(
                    strings.image_reveal_in_file_manager.clone(),
                    detail,
                    cx,
                );
            }
        }
        let _ = window;
    }

    /// 「复制图片地址」：本地图为文档相对路径原文，远程图为 URL。
    pub(super) fn on_image_copy_address(
        &mut self,
        _: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(ContextMenuState::Image { address, .. }) = self.context_menu.take() else {
            return;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(address));
    }

    pub(super) fn open_table_axis_context_menu(
        &mut self,
        position: Point<Pixels>,
        selection: TableAxisSelection,
        cx: &mut Context<Self>,
    ) {
        if self.view_mode != ViewMode::Rendered {
            return;
        }

        self.close_menu_bar(cx);
        self.context_menu_submenu_close_task = None;
        self.context_menu = Some(ContextMenuState::TableAxis {
            position,
            selection,
        });
        cx.notify();
    }

    pub(super) fn close_table_insert_dialog(&mut self, cx: &mut Context<Self>) {
        if self.table_insert_dialog.take().is_some() {
            cx.notify();
        }
    }

    fn close_context_menu(&mut self, cx: &mut Context<Self>) {
        let had_menu = self.context_menu.take().is_some();
        let had_submenu_close = self.context_menu_submenu_close_task.take().is_some();
        if had_menu || had_submenu_close {
            cx.notify();
        }
    }

    pub(super) fn dismiss_contextual_overlays(&mut self, cx: &mut Context<Self>) {
        self.close_workspace_context_menu(cx);
        self.close_wikilink_completion(cx);
        let had_menu = self.context_menu.take().is_some();
        let had_dialog = self.table_insert_dialog.take().is_some();
        let had_submenu_close = self.context_menu_submenu_close_task.take().is_some();
        if had_menu || had_dialog || had_submenu_close {
            cx.notify();
        }
    }

    fn schedule_context_menu_submenu_close(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.context_menu, Some(ContextMenuState::Insert { .. })) {
            return;
        }

        let weak_editor = cx.entity().downgrade();
        self.context_menu_submenu_close_task = Some(cx.spawn(
            async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
                cx.background_executor()
                    .timer(Duration::from_millis(120))
                    .await;
                let _ = weak_editor.update(cx, |editor, cx| {
                    editor.context_menu_submenu_close_task = None;
                    let Some(ContextMenuState::Insert {
                        insert_hovered,
                        submenu_hovered,
                        submenu_open,
                        ..
                    }) = editor.context_menu.as_mut()
                    else {
                        return;
                    };
                    if !*insert_hovered && !*submenu_hovered && *submenu_open {
                        *submenu_open = false;
                        cx.notify();
                    }
                });
            },
        ));
    }

    fn set_context_menu_hover_state(
        &mut self,
        hovered: bool,
        submenu: bool,
        cx: &mut Context<Self>,
    ) {
        let mut changed = false;
        let mut should_clear_close = false;
        let mut should_schedule_close = false;

        if let Some(ContextMenuState::Insert {
            insert_hovered,
            submenu_hovered,
            submenu_open,
            ..
        }) = self.context_menu.as_mut()
        {
            if submenu {
                if *submenu_hovered != hovered {
                    *submenu_hovered = hovered;
                    changed = true;
                }
            } else if *insert_hovered != hovered {
                *insert_hovered = hovered;
                changed = true;
            }

            if hovered {
                should_clear_close = true;
                if !*submenu_open {
                    *submenu_open = true;
                    changed = true;
                }
            } else {
                let insert_still_hovered = *insert_hovered;
                let submenu_still_hovered = *submenu_hovered;
                if !insert_still_hovered && !submenu_still_hovered {
                    should_schedule_close = true;
                }
            }
        }

        if should_clear_close {
            self.context_menu_submenu_close_task = None;
        }
        if should_schedule_close {
            self.schedule_context_menu_submenu_close(cx);
        }
        if changed {
            cx.notify();
        }
    }

    pub(super) fn on_editor_context_menu_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.view_mode != ViewMode::Rendered {
            return;
        }
        cx.stop_propagation();
        self.open_insert_context_menu(event.position, TableInsertTarget::Append, cx);
    }

    pub(super) fn on_block_context_menu_mouse_down(
        &mut self,
        entity_id: EntityId,
        event: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.view_mode != ViewMode::Rendered {
            return;
        }
        cx.stop_propagation();

        // 图片块：右键出图片菜单（在文件管理器中显示 / 复制图片地址）。
        if let Some(block) = self.focusable_entity_by_id(entity_id)
            && block.read(cx).showing_rendered_image()
            && let Some(runtime) = block.read(cx).image_runtime()
        {
            let local_path = match &runtime.resolved_source {
                crate::components::markdown::image::ImageResolvedSource::Local(path) => Some(
                    std::fs::canonicalize(path).unwrap_or_else(|_| path.clone()),
                ),
                crate::components::markdown::image::ImageResolvedSource::Remote(_) => None,
            };
            let address = runtime.src.clone();
            self.open_image_context_menu(event.position, local_path, address, cx);
            return;
        }

        // Right-clicking inside a table cell, or any block where inserting a
        // table makes no sense (code, math, etc.), offers no insert menu.
        if self.table_cell_binding(entity_id).is_some() {
            return;
        }
        let allows_insert = self
            .focusable_entity_by_id(entity_id)
            .is_none_or(|block| block.read(cx).kind().allows_context_table_insert());
        if !allows_insert {
            return;
        }
        let target = TableInsertTarget::After(self.root_ancestor_entity_id(entity_id));
        self.open_insert_context_menu(event.position, target, cx);
    }

    pub(super) fn on_dismiss_context_menu_overlay(
        &mut self,
        _event: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dismiss_contextual_overlays(cx);
    }

    pub(super) fn on_dismiss_transient_ui(
        &mut self,
        _: &DismissTransientUi,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // 模态是最高优先级的瞬态 UI：Esc 先归它（与按键捕获钩子双保险，
        // cancel_modal 幂等）。
        if self.modal_is_open() {
            cx.stop_propagation();
            self.cancel_modal(window, cx);
            return;
        }
        // 信息弹窗与标题栏菜单也是瞬态 UI，且没有其他键盘退出口（用户报修：
        // Esc 关不掉「关于/检查更新」弹窗，也关不掉标题栏菜单）。
        if self.info_dialog.is_some() {
            cx.stop_propagation();
            self.hide_info_dialog(cx);
            return;
        }
        if self.menu_bar_open.is_some() {
            cx.stop_propagation();
            self.close_menu_bar(cx);
            return;
        }
        self.dismiss_contextual_overlays(cx);
        // escape 由全局快捷键路由到这里，浮层面板自己的 key_down 收不到。
        self.close_quick_open(cx);
        self.close_command_palette(cx);
    }

    pub(super) fn on_context_menu_insert_hover(
        &mut self,
        hovered: &bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_context_menu_hover_state(*hovered, false, cx);
    }

    pub(super) fn on_context_menu_submenu_hover(
        &mut self,
        hovered: &bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_context_menu_hover_state(*hovered, true, cx);
    }

    pub(super) fn on_open_table_insert_dialog(
        &mut self,
        _event: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(ContextMenuState::Insert { target, .. }) = self.context_menu.take() else {
            return;
        };
        self.context_menu_submenu_close_task = None;
        self.table_insert_dialog = Some(TableInsertDialogState {
            target,
            body_rows: 2,
            columns: 2,
        });
        cx.notify();
    }

    pub(super) fn on_table_rows_decrement(
        &mut self,
        _event: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(dialog) = self.table_insert_dialog.as_mut() {
            dialog.body_rows = dialog.body_rows.saturating_sub(1).max(1);
            cx.notify();
        }
    }

    pub(super) fn on_table_rows_increment(
        &mut self,
        _event: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(dialog) = self.table_insert_dialog.as_mut() {
            dialog.body_rows += 1;
            cx.notify();
        }
    }

    pub(super) fn on_table_columns_decrement(
        &mut self,
        _event: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(dialog) = self.table_insert_dialog.as_mut() {
            dialog.columns = dialog.columns.saturating_sub(1).max(1);
            cx.notify();
        }
    }

    pub(super) fn on_table_columns_increment(
        &mut self,
        _event: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(dialog) = self.table_insert_dialog.as_mut() {
            dialog.columns += 1;
            cx.notify();
        }
    }

    pub(super) fn on_cancel_table_insert_dialog(
        &mut self,
        _event: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_table_insert_dialog(cx);
    }

    pub(super) fn on_confirm_table_insert_dialog(
        &mut self,
        _event: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.insert_table_from_dialog(cx);
    }

    /// 从已打开的插入对话框插入表格。与 UI 事件解耦（便于测试），并把这次
    /// 结构插入纳入撤销栈：其它表格操作都走 prepare/finalize，此前这条漏了
    /// （用户报修：插入表格后 Ctrl+Z 没反应，或者把上一次编辑一并揪掉）。
    pub(super) fn insert_table_from_dialog(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(dialog) = self.table_insert_dialog.take() else {
            return false;
        };
        self.prepare_undo_capture(
            crate::components::UndoCaptureKind::NonCoalescible,
            cx,
        );

        let table = TableData::new_empty(dialog.body_rows, dialog.columns);
        let new_block = Self::new_table_block(cx, table);

        match dialog.target {
            TableInsertTarget::After(entity_id) => {
                if let Some(location) = self.document.find_block_location(entity_id) {
                    self.document.insert_blocks_at(
                        location.parent,
                        location.index + 1,
                        vec![new_block.clone()],
                        cx,
                    );
                } else {
                    self.document.insert_blocks_at(
                        None,
                        self.document.root_count(),
                        vec![new_block.clone()],
                        cx,
                    );
                }
            }
            TableInsertTarget::Append => {
                self.document.insert_blocks_at(
                    None,
                    self.document.root_count(),
                    vec![new_block.clone()],
                    cx,
                );
            }
        }

        // A table inserted as the last block in its container leaves no line
        // below it, so in rendered mode the caret cannot move past the table.
        // Add a trailing empty paragraph to land on when nothing follows it.
        self.ensure_trailing_paragraph_after_structural(&new_block, cx);

        self.rebuild_table_runtimes(cx);
        if let Some(first_cell) = new_block
            .read(cx)
            .table_runtime
            .as_ref()
            .and_then(|runtime| runtime.header.first())
        {
            self.focus_block(first_cell.entity_id());
        }
        self.mark_dirty(cx);
        self.finalize_pending_undo_capture(cx);
        self.request_active_block_scroll_into_view(cx);
        cx.notify();
        true
    }

    fn active_axis_menu_selection(&self) -> Option<TableAxisSelection> {
        match self.context_menu.as_ref() {
            Some(ContextMenuState::TableAxis { selection, .. }) => Some(*selection),
            _ => None,
        }
    }

    fn on_apply_column_alignment(
        &mut self,
        alignment: TableColumnAlignment,
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
        self.close_context_menu(cx);
        self.set_table_column_alignment(&table_block, selection.index, alignment, cx);
    }

    pub(super) fn on_align_table_column_left(
        &mut self,
        _event: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Left is the default, so emit the unmarked `---` form rather than an
        // explicit `:---`; an explicit colon is only kept when the source had
        // one. This keeps the menu's output unchanged from before.
        self.on_apply_column_alignment(TableColumnAlignment::Default, cx);
    }

    pub(super) fn on_align_table_column_center(
        &mut self,
        _event: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.on_apply_column_alignment(TableColumnAlignment::Center, cx);
    }

    pub(super) fn on_align_table_column_right(
        &mut self,
        _event: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.on_apply_column_alignment(TableColumnAlignment::Right, cx);
    }

    pub(super) fn on_move_table_row_up(
        &mut self,
        _event: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(selection) = self.active_axis_menu_selection() else {
            return;
        };
        if selection.kind != TableAxisKind::Row || selection.index == 0 {
            return;
        }
        let Some(table_block) = self.table_block_by_id(selection.table_block_id, cx) else {
            return;
        };
        self.close_context_menu(cx);
        self.move_table_row(&table_block, selection.index, -1, cx);
    }

    pub(super) fn on_move_table_row_down(
        &mut self,
        _event: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(selection) = self.active_axis_menu_selection() else {
            return;
        };
        if selection.kind != TableAxisKind::Row {
            return;
        }
        let Some(table_block) = self.table_block_by_id(selection.table_block_id, cx) else {
            return;
        };
        let can_move = table_block
            .read(cx)
            .record
            .table
            .as_ref()
            .map(|table| selection.index < table.rows.len())
            .unwrap_or(false);
        if !can_move {
            return;
        }
        self.close_context_menu(cx);
        self.move_table_row(&table_block, selection.index, 1, cx);
    }

    pub(super) fn on_move_table_column_left(
        &mut self,
        _event: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(selection) = self.active_axis_menu_selection() else {
            return;
        };
        if selection.kind != TableAxisKind::Column || selection.index == 0 {
            return;
        }
        let Some(table_block) = self.table_block_by_id(selection.table_block_id, cx) else {
            return;
        };
        self.close_context_menu(cx);
        self.move_table_column(&table_block, selection.index, -1, cx);
    }

    pub(super) fn on_move_table_column_right(
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
        let can_move = table_block
            .read(cx)
            .record
            .table
            .as_ref()
            .map(|table| selection.index + 1 < table.column_count())
            .unwrap_or(false);
        if !can_move {
            return;
        }
        self.close_context_menu(cx);
        self.move_table_column(&table_block, selection.index, 1, cx);
    }

    pub(super) fn on_delete_table_row(
        &mut self,
        _event: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(selection) = self.active_axis_menu_selection() else {
            return;
        };
        if selection.kind != TableAxisKind::Row {
            return;
        }
        let Some(table_block) = self.table_block_by_id(selection.table_block_id, cx) else {
            return;
        };
        let row_count = table_block
            .read(cx)
            .record
            .table
            .as_ref()
            .map(|table| table.rows.len());
        self.close_context_menu(cx);
        // Visual index 0 is the header: deleting it promotes the first body row,
        // unless there is no body row left, in which case it was the table's last
        // row and the whole table is removed.
        if selection.index == 0 {
            if row_count == Some(0) {
                self.remove_table_block(&table_block, cx);
            } else {
                self.delete_table_header_row(&table_block, cx);
            }
        } else {
            self.delete_table_row(&table_block, selection.index - 1, cx);
        }
    }
}

mod render;

#[cfg(test)]
mod tests;

/// 「在文件管理器中显示」的平台命令：程序名 + 参数（纯数据便于测试）。
pub(super) fn reveal_command_spec(path: &std::path::Path) -> (&'static str, Vec<String>) {
    #[cfg(target_os = "macos")]
    {
        ("open", vec!["-R".to_string(), path.display().to_string()])
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let dir = path
            .parent()
            .map(|parent| parent.to_path_buf())
            .unwrap_or_else(|| path.to_path_buf());
        ("xdg-open", vec![dir.display().to_string()])
    }
    #[cfg(windows)]
    {
        (
            "explorer",
            vec![format!("/select,{}", path.display())],
        )
    }
}
