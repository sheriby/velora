//! Rendered-mode context menus and native table insertion dialog.

pub(super) use std::path::PathBuf;

pub(super) use gpui::*;

pub(super) use super::{Editor, TableAxisSelection, ViewMode};
pub(super) use crate::components::{
    DismissTransientUi, TableAxisKind, TableCellPosition, TableColumnAlignment, TableData,
};
use crate::i18n::I18nManager;
use crate::theme::Theme;
pub(super) use document_menu::{
    DOCUMENT_MENU_QUICK_ACTIONS, DocumentMenuCommand, DocumentMenuGeometry, DocumentMenuRow,
    DocumentSubmenu, document_menu_command_icon, document_menu_label, document_menu_origins,
    document_menu_shortcut, document_submenu_icon, document_submenu_label, quick_action_tooltip,
};

/// Target block position for inserting a native table.
#[derive(Clone, Copy)]
pub(super) enum TableInsertTarget {
    /// Insert the table immediately after the referenced block.
    After(EntityId),
    /// Append the table to the end of the current root list.
    Append,
}

pub(super) struct ContextMenuPopup {
    view: Entity<gpui_kit::component::menu::PopupMenu>,
    return_focus: FocusHandle,
    _dismiss: Subscription,
}

/// Rendered-mode context menu currently open in the editor.
pub(super) enum ContextMenuState {
    /// 正文右键菜单：编辑 → 格式 → 段落 → 插入 → 视图五段，后三段走二级面板。
    Document {
        position: Point<Pixels>,
        target: TableInsertTarget,
        /// 菜单的焦点陷阱：打开后按键留在菜单上，不再打进遮罩后面的正文。
        /// 没有它，开着菜单打字会改看不见的内容，vim 普通模式按键也会直接生效。
        focus: FocusHandle,
        /// 打开那一刻正文里握着焦点的控件；关闭时归还。`None` 表示还没挂载过。
        restore_focus: Option<Option<WeakFocusHandle>>,
        /// 当前展开的二级菜单；None 表示只有主菜单。
        open_submenu: Option<DocumentSubmenu>,
        /// 鼠标正停在哪个二级菜单的父行上；None 表示没停在任何一条上。
        parent_row_hovered: Option<DocumentSubmenu>,
        /// 鼠标正停在二级面板上吗。与父行那一份分开记：同一次移动里父行的
        /// mouse-out 与面板的 mouse-in 会前后脚到，谁后到都不能把另一处的悬停抹掉。
        submenu_panel_hovered: bool,
        /// 鼠标正停在一级面板与二级面板中间那条缝里吗（缝只有几个像素宽，
        /// 从父行横到面板上时正好会经过它，停在那儿不该算离开）。
        submenu_bridge_hovered: bool,
        /// 打开那一刻剪贴板里有没有内容：菜单开着期间的按键/渲染不再逐帧读剪贴板。
        clipboard_has_content: bool,
        /// 打开时拍下的行数据与实测几何：菜单开着期间行/主题不变，逐帧重建
        /// 行、逐帧重测 label 宽度都是白付的。
        rows: std::rc::Rc<Vec<DocumentMenuRow>>,
        geometry: Option<(u64, std::rc::Rc<DocumentMenuGeometry>)>,
        submenu_cache: Option<(
            DocumentSubmenu,
            std::rc::Rc<Vec<DocumentMenuRow>>,
            std::rc::Rc<DocumentMenuGeometry>,
        )>,
    },
    /// Table row or column context menu for an existing native table.
    TableAxis {
        position: Point<Pixels>,
        selection: TableAxisSelection,
    },
    /// Cell context menu for an existing native table: insert rows or columns
    /// next to the clicked cell, or delete its row/column.
    TableCell {
        position: Point<Pixels>,
        table_block_id: EntityId,
        cell: TableCellPosition,
    },
    /// 图片块菜单：缩放、在文件管理器中显示、复制地址。
    Image {
        position: Point<Pixels>,
        entity_id: EntityId,
        /// 本地图片绝对路径；远程图为 None，不提供本地定位命令。
        local_path: Option<PathBuf>,
        /// 原始地址（相对路径或 URL），「复制图片地址」用。
        address: String,
    },
}

impl ContextMenuState {
    /// 二级面板的悬停区状态：`(还有没有哪儿停着指针, 展开的那一档还在不在)`。
    /// 三个区（父行、两块面板中间那条缝、二级面板）各记一份，这里合起来看一眼；
    /// 不是正文菜单（表格轴菜单、图片菜单）返回 None。
    fn submenu_hover_regions(&self) -> Option<(bool, bool)> {
        let ContextMenuState::Document {
            open_submenu,
            parent_row_hovered,
            submenu_panel_hovered,
            submenu_bridge_hovered,
            ..
        } = self
        else {
            return None;
        };
        Some((
            parent_row_hovered.is_some() || *submenu_panel_hovered || *submenu_bridge_hovered,
            open_submenu.is_some(),
        ))
    }
}

/// State for the table insertion dialog opened from the context menu.
impl TableInsertDialogState {
    /// 行列步进只从对话框自身增长，钳到 TableData 的建表上限
    /// （无上限连点会同步建几万个块实体、前台冻结）。
    pub(crate) fn increment_rows(&mut self) {
        self.body_rows = (self.body_rows + 1).min(crate::components::markdown::table::TableData::MAX_CREATED_ROWS);
    }

    pub(crate) fn increment_columns(&mut self) {
        self.columns = (self.columns + 1).min(crate::components::markdown::table::TableData::MAX_CREATED_COLUMNS);
    }
}

pub(super) struct TableInsertDialogState {
    pub target: TableInsertTarget,
    pub body_rows: usize,
    pub columns: usize,
    /// 对话框自己的焦点：这是真模态，打开后按键必须留在对话框里，
    /// 不能打进遮罩后面的正文（vim 的 `dd`、回车换行都曾直接生效）。
    pub focus: FocusHandle,
    /// 四颗步进按钮各自的焦点：Tab 在它们之间轮转，Enter 激活当前那颗。
    pub steppers: [FocusHandle; 4],
    /// 打开那一刻正文里握着焦点的控件；关闭时归还。`None` 表示还没挂载过。
    pub restore_focus: Option<Option<WeakFocusHandle>>,
}

impl TableInsertDialogState {
    pub(super) fn new(
        target: TableInsertTarget,
        body_rows: usize,
        columns: usize,
        cx: &mut Context<Editor>,
    ) -> Self {
        Self {
            target,
            body_rows,
            columns,
            focus: cx.focus_handle(),
            steppers: [
                cx.focus_handle(),
                cx.focus_handle(),
                cx.focus_handle(),
                cx.focus_handle(),
            ],
            restore_focus: None,
        }
    }
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
        if self.view_mode != ViewMode::Rendered && !self.is_read_only() {
            return;
        }

        self.close_menu_bar(cx);
        self.context_menu_submenu_close_task = None;
        self.context_menu_popup = None;
        let rows = std::rc::Rc::new(self.document_menu_rows(cx));
        self.context_menu = Some(ContextMenuState::Document {
            position,
            target,
            focus: cx.focus_handle(),
            restore_focus: None,
            open_submenu: None,
            parent_row_hovered: None,
            submenu_panel_hovered: false,
            submenu_bridge_hovered: false,
            clipboard_has_content: cx.read_from_clipboard().is_some(),
            rows,
            geometry: None,
            submenu_cache: None,
        });
        cx.notify();
    }

    /// 打开图片块右键菜单。
    pub(super) fn open_image_context_menu(
        &mut self,
        position: Point<Pixels>,
        entity_id: EntityId,
        local_path: Option<PathBuf>,
        address: String,
        cx: &mut Context<Self>,
    ) {
        self.close_menu_bar(cx);
        self.context_menu_submenu_close_task = None;
        self.context_menu_popup = None;
        self.context_menu = Some(ContextMenuState::Image {
            position,
            entity_id,
            local_path,
            address,
        });
        cx.notify();
    }

    fn set_image_scale_from_menu(&mut self, percent: u8, cx: &mut Context<Self>) {
        if self.is_read_only() {
            return;
        }

        if !(20..=100).contains(&percent) || self.view_mode != ViewMode::Rendered {
            return;
        }
        let Some(ContextMenuState::Image { entity_id, .. }) = self.context_menu.as_ref() else {
            return;
        };
        let entity_id = *entity_id;
        let Some(block) = self.focusable_entity_by_id(entity_id) else {
            self.close_context_menu(cx);
            return;
        };
        if !block.read(cx).showing_rendered_image() {
            self.close_context_menu(cx);
            return;
        }
        self.close_context_menu(cx);
        block.update(cx, |block, cx| {
            block.image_width_factor = percent as f32 / 100.0;
            block.write_image_width_back_to_source(cx);
            cx.notify();
        });
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
        }) = self.context_menu.as_ref()
        else {
            return;
        };
        let path = path.clone();
        self.close_context_menu(cx);
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
        let Some(ContextMenuState::Image { address, .. }) = self.context_menu.as_ref() else {
            return;
        };
        let address = address.clone();
        self.close_context_menu(cx);
        cx.write_to_clipboard(ClipboardItem::new_string(address));
    }

    pub(super) fn open_table_axis_context_menu(
        &mut self,
        position: Point<Pixels>,
        selection: TableAxisSelection,
        cx: &mut Context<Self>,
    ) {
        if self.is_read_only() {
            self.open_insert_context_menu(position, TableInsertTarget::Append, cx);
            return;
        }
        if self.view_mode != ViewMode::Rendered {
            return;
        }

        self.close_menu_bar(cx);
        self.context_menu_submenu_close_task = None;
        self.context_menu_popup = None;
        self.context_menu = Some(ContextMenuState::TableAxis {
            position,
            selection,
        });
        cx.notify();
    }

    pub(super) fn open_table_cell_context_menu(
        &mut self,
        table_block_id: EntityId,
        cell: TableCellPosition,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        if self.is_read_only() || self.view_mode != ViewMode::Rendered {
            return;
        }

        self.close_menu_bar(cx);
        self.context_menu_submenu_close_task = None;
        self.context_menu_popup = None;
        self.context_menu = Some(ContextMenuState::TableCell {
            position,
            table_block_id,
            cell,
        });
        cx.notify();
    }

    pub(super) fn close_table_insert_dialog(&mut self, cx: &mut Context<Self>) {
        if self.table_insert_dialog.take().is_some() {
            cx.notify();
        }
    }

    /// 关掉插入对话框并把焦点还给打开前的控件。Esc（Dialog 的 Cancel）、
    /// 点遮罩与「取消」按钮都走这里——它们拿得到 window，焦点只在
    /// 这些路径上归还；其余路径由随后的动作自己落焦点。
    pub(super) fn cancel_table_insert_dialog_with_focus(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.restore_transient_overlay_focus(window, cx);
        self.close_table_insert_dialog(cx);
    }

    fn close_context_menu(&mut self, cx: &mut Context<Self>) {
        self.context_menu_popup = None;
        let menu = self.context_menu.take();
        let had_menu = menu.is_some();
        if let Some(ContextMenuState::Image { entity_id, .. }) = menu {
            self.focus_block(entity_id);
        }
        let had_submenu_close = self.context_menu_submenu_close_task.take().is_some();
        if had_menu || had_submenu_close {
            cx.notify();
        }
    }

    /// 把焦点还给打开瞬态浮层前的那个控件（正文菜单、表格插入对话框共用）。
    ///
    /// 这两个浮层打开时会抢走正文焦点挡按键；关闭路径里只有 Esc、点遮罩、
    /// 点菜单行这几条拿得到 `window`，归还只在它们身上做。其余路径（各命令
    /// 处理器）随后的动作自己会把焦点落到目标块上；什么都没落时焦点悬空，
    /// 下一次点击自然恢复——按键不会落进正文，这正是焦点陷阱的目的。
    pub(super) fn restore_transient_overlay_focus(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut holders: Vec<FocusHandle> = Vec::new();
        let mut restores: Vec<Option<Option<WeakFocusHandle>>> = Vec::new();
        if let Some(ContextMenuState::Document {
            focus,
            restore_focus,
            ..
        }) = self.context_menu.as_ref()
        {
            holders.push(focus.clone());
            restores.push(restore_focus.clone());
        }
        if let Some(dialog) = self.table_insert_dialog.as_ref() {
            holders.push(dialog.focus.clone());
            restores.push(dialog.restore_focus.clone());
        }
        for (holder, restore) in holders.into_iter().zip(restores) {
            if !(holder.is_focused(window) || holder.contains_focused(window, cx)) {
                continue;
            }
            match restore.flatten().and_then(|focus| focus.upgrade()) {
                Some(previous) => previous.focus(window, cx),
                None => window.blur(cx),
            }
        }
    }

    pub(super) fn close_document_menu_for_modal(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(popup) = &self.context_menu_popup
            && popup.view.focus_handle(cx).is_focused(window)
        {
            window.focus(&popup.return_focus, cx);
        }
        self.restore_transient_overlay_focus(window, cx);
        self.close_context_menu(cx);
    }

    pub(super) fn dismiss_contextual_overlays(&mut self, cx: &mut Context<Self>) {
        self.close_workspace_context_menu(cx);
        self.close_tab_context_menu(cx);
        self.close_wikilink_completion(cx);
        self.close_latex_completion(cx);
        self.close_formula_editor(cx);
        self.close_context_menu(cx);
        let had_dialog = self.table_insert_dialog.take().is_some();
        let had_submenu_close = self.context_menu_submenu_close_task.take().is_some();
        if had_dialog || had_submenu_close {
            cx.notify();
        }
    }

    pub(super) fn on_editor_context_menu_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.view_mode != ViewMode::Rendered && !self.is_read_only() {
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
        if self.view_mode != ViewMode::Rendered && !self.is_read_only() {
            return;
        }
        cx.stop_propagation();

        if self.is_read_only() {
            self.open_insert_context_menu(event.position, TableInsertTarget::Append, cx);
            return;
        }

        // 图片菜单持有右键目标，避免缩放写到此前聚焦的图片上。
        if let Some(block) = self.focusable_entity_by_id(entity_id)
            && block.read(cx).showing_rendered_image()
            && let Some(runtime) = block.read(cx).image_runtime()
        {
            let local_path = match &runtime.resolved_source {
                crate::components::markdown::image::ImageResolvedSource::Local(path) => Some(
                    // 图片菜单里的「复制路径」跟最近列表一样不能带 `\\?\`（Windows）。
                    crate::file_url::canonical_path(path),
                ),
                crate::components::markdown::image::ImageResolvedSource::Remote(_) => None,
            };
            let address = runtime.src.clone();
            self.open_image_context_menu(event.position, entity_id, local_path, address, cx);
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
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // 关掉之前先把焦点还给正文：遮罩吃掉的这一下鼠标不该顺带把焦点弄丢。
        self.restore_transient_overlay_focus(window, cx);
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
        // 预览浮层占着焦点：焦点陷阱激活后按键捕获钩子不再拦键，Esc 会走全局
        // 绑定到这条入口，关闭与焦点归还都靠这里。
        if self.mermaid_preview.is_some() {
            cx.stop_propagation();
            self.close_mermaid_preview(window, cx);
            return;
        }
        // 信息弹窗与标题栏菜单也是瞬态 UI，且没有其他键盘退出口——Esc 必须能关掉
        // 「关于/检查更新」弹窗与标题栏菜单。
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
        if self.close_selection_toolbar_menu(cx) {
            cx.stop_propagation();
            return;
        }
        if self.dismiss_ai_panel(window, cx) { cx.stop_propagation(); return; }
        if self.cancel_workspace_name_edit(window, cx) {
            cx.stop_propagation();
            self.dismiss_contextual_overlays(cx);
            return;
        }
        // Esc 是瞬态浮层（正文菜单、表格插入对话框）最主要的退出口，归还焦点
        // 只在拿得到 window 的路径上做，这里是其中之一。
        self.restore_transient_overlay_focus(window, cx);
        self.dismiss_contextual_overlays(cx);
        // escape 由全局快捷键路由到这里，浮层面板自己的 key_down 收不到。
        self.close_quick_open(cx);
        self.close_command_palette(cx);
    }

    /// 从当前正文菜单的插入位置打开表格对话框（菜单那一行与插入表格的命令共用这一条）。
    pub(super) fn open_table_insert_dialog_from_menu(&mut self, cx: &mut Context<Self>) {
        if self.is_read_only() {
            return;
        }

        let Some(ContextMenuState::Document { target, .. }) = self.context_menu.take() else {
            return;
        };
        self.context_menu_submenu_close_task = None;
        self.table_insert_dialog = Some(TableInsertDialogState::new(target, 2, 2, cx));
        cx.notify();
    }

    pub(super) fn on_table_rows_decrement(
        &mut self,
        _event: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.step_table_dialog_value(false, false, cx);
    }

    pub(super) fn on_table_rows_increment(
        &mut self,
        _event: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.step_table_dialog_value(false, true, cx);
    }

    pub(super) fn on_table_columns_decrement(
        &mut self,
        _event: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.step_table_dialog_value(true, false, cx);
    }

    pub(super) fn on_table_columns_increment(
        &mut self,
        _event: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.step_table_dialog_value(true, true, cx);
    }

    /// 对话框里一颗步进按钮的核心：鼠标点击与键盘 Enter（Confirm 动作）共用，
    /// 钳制口径只在这一处。
    fn step_table_dialog_value(
        &mut self,
        columns: bool,
        increment: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(dialog) = self.table_insert_dialog.as_mut() else {
            return;
        };
        match (columns, increment) {
            (false, true) => dialog.increment_rows(),
            (false, false) => dialog.body_rows = dialog.body_rows.saturating_sub(1).max(1),
            (true, true) => dialog.increment_columns(),
            (true, false) => dialog.columns = dialog.columns.saturating_sub(1).max(1),
        }
        cx.notify();
    }

    pub(super) fn on_cancel_table_insert_dialog(
        &mut self,
        _event: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cancel_table_insert_dialog_with_focus(window, cx);
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
    /// 结构插入纳入撤销栈：其它表格操作都走 prepare/finalize，此前这条漏了。
    pub(super) fn insert_table_from_dialog(&mut self, cx: &mut Context<Self>) -> bool {
        if self.is_read_only() {
            return false;
        }

        let Some(dialog) = self.table_insert_dialog.take() else {
            return false;
        };
        self.prepare_undo_capture(
            crate::components::UndoCaptureKind::NonCoalescible,
            cx,
        );

        let table = TableData::new_empty(dialog.body_rows, dialog.columns);
        let new_block = Self::new_table_block(cx, table);
        // 插入字节位要在动树之前算好：After 拿锚点根块的区间尾，Append 落在
        // 缓冲区末尾。算不出（锚点没有区间、目标块已不在树里）就退回整篇
        // 重投影——宁可洗写法，不能把字节写错位置。
        let insert_offset = match dialog.target {
            TableInsertTarget::After(entity_id) => self
                .document
                .find_block_location(entity_id)
                .and_then(|_| self.document.source_span_of(entity_id))
                .map(|span| span.end),
            TableInsertTarget::Append => Some(self.buffer.byte_len()),
        };

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
        // 它要在表格字节落笔之后做：尾段落的空间就是按落笔后的缓冲区末尾补的。
        self.rebuild_table_runtimes(cx);
        if let Some(first_cell) = new_block
            .read(cx)
            .table_runtime
            .as_ref()
            .and_then(|runtime| runtime.header.first())
        {
            self.focus_block(first_cell.entity_id());
        }

        // 仿照「插入代码块」的区间写回（insert_ops::write_inserted_block_bytes）：
        // 只在插入点写「接缝 + 新表的 markdown」，后面的根块区间整体平移。
        // 整篇 mark_dirty 的重投影会把别处的 `__强调__`、Setext、列宽填充
        // 一起洗掉。写不出去（算不出字节位）才退回整篇重投影。
        let written = match insert_offset {
            Some(offset) if offset <= self.buffer.byte_len() => {
                let markdown = self.document.block_markdown_source(&new_block, cx);
                let before = match offset {
                    0 => "",
                    _ => match dialog.target {
                        // 锚点根块的区间尾不含它自己的换行，接缝固定一个空行。
                        // Append 落在缓冲区末尾：末尾已有几个换行决定还欠几个——
                        // 已隔一个空行（`\n\n`）就不再补，别把空行越叠越多。
                        TableInsertTarget::After(_) => "\n\n",
                        TableInsertTarget::Append => match (
                            self.buffer.byte_at(offset - 1),
                            offset.checked_sub(2).and_then(|at| self.buffer.byte_at(at)),
                        ) {
                            (Some(b'\n'), Some(b'\n')) => "",
                            (Some(b'\n'), _) => "\n",
                            _ => "\n\n",
                        },
                    },
                };
                let text = format!("{before}{markdown}");
                let applied = self.buffer.edit(offset..offset, &text);
                self.record_buffer_edit(applied);
                self.shift_root_spans_after(offset, text.len() as i64);
                let start = offset + before.len();
                self.document
                    .set_source_span(new_block.entity_id(), start..start + markdown.len());
                true
            }
            _ => false,
        };
        self.ensure_trailing_paragraph_after_structural(&new_block, cx);
        if written {
            self.mark_dirty_written_back(cx);
        } else {
            self.mark_dirty(cx);
        }
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

    fn active_table_cell_menu_target(&self) -> Option<(EntityId, TableCellPosition)> {
        match self.context_menu.as_ref() {
            Some(ContextMenuState::TableCell {
                table_block_id,
                cell,
                ..
            }) => Some((*table_block_id, *cell)),
            _ => None,
        }
    }

    pub(super) fn on_insert_table_row_above(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.insert_table_row_from_cell_menu(false, cx);
    }

    pub(super) fn on_insert_table_row_below(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.insert_table_row_from_cell_menu(true, cx);
    }

    fn insert_table_row_from_cell_menu(&mut self, below: bool, cx: &mut Context<Self>) {
        let Some((table_block_id, cell)) = self.active_table_cell_menu_target() else {
            return;
        };
        let Some(table_block) = self.table_block_by_id(table_block_id, cx) else {
            self.close_context_menu(cx);
            return;
        };
        self.close_context_menu(cx);
        // 焦点落在「新行 × 右击的那一列」：插行不把用户的心智位置挪回行首。
        self.insert_table_row(&table_block, cell.row, below, Some(cell), cx);
    }

    pub(super) fn on_insert_table_column_left(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.insert_table_column_from_cell_menu(false, cx);
    }

    pub(super) fn on_insert_table_column_right(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.insert_table_column_from_cell_menu(true, cx);
    }

    fn insert_table_column_from_cell_menu(&mut self, right: bool, cx: &mut Context<Self>) {
        let Some((table_block_id, cell)) = self.active_table_cell_menu_target() else {
            return;
        };
        let Some(table_block) = self.table_block_by_id(table_block_id, cx) else {
            self.close_context_menu(cx);
            return;
        };
        self.close_context_menu(cx);
        // 焦点落在「右击的那一行 × 新列」：插列后第一下输入不该污染表头。
        self.insert_table_column(&table_block, cell.column, right, Some(cell), cx);
    }

    pub(super) fn on_delete_table_cell_row(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((table_block_id, cell)) = self.active_table_cell_menu_target() else {
            return;
        };
        let Some(table_block) = self.table_block_by_id(table_block_id, cx) else {
            self.close_context_menu(cx);
            return;
        };
        self.close_context_menu(cx);
        if cell.is_header() {
            // 仅剩表头的表上，「删除行」与轴菜单同语义：没有数据行可升，
            // 删的就是整张表。静默什么都不做会让两个菜单同一动作两种结果。
            let row_count = table_block
                .read(cx)
                .record
                .table
                .as_ref()
                .map(|table| table.rows.len());
            if row_count == Some(0) {
                self.remove_table_block(&table_block, cx);
            } else {
                self.delete_table_header_row(&table_block, cx);
            }
        } else if let Some(body_row) = cell.body_row_index() {
            self.delete_table_row(&table_block, body_row, cx);
        }
    }

    pub(super) fn on_delete_table_cell_column(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((table_block_id, cell)) = self.active_table_cell_menu_target() else {
            return;
        };
        let Some(table_block) = self.table_block_by_id(table_block_id, cx) else {
            self.close_context_menu(cx);
            return;
        };
        let column_count = table_block
            .read(cx)
            .record
            .table
            .as_ref()
            .map(|table| table.column_count());
        self.close_context_menu(cx);
        if column_count == Some(1) {
            self.remove_table_block(&table_block, cx);
        } else {
            self.delete_table_column(&table_block, cell.column, cx);
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

mod document_menu;
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
