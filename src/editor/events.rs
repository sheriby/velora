//! Editor-side handling for [`BlockEvent`] values emitted by child blocks.
//!
//! This is the central mutation engine for split, merge, indent, outdent,
//! delete, multiline paste, focus transfer, and dirty-state tracking. Runtime
//! tree mutations are delegated to [`DocumentTree`](super::tree::DocumentTree)
//! so visible-order metadata stays in sync with every edit.

pub(super) use std::fs;
pub(super) use std::path::{Path, PathBuf};
pub(super) use std::time::{Duration, Instant};

pub(super) use anyhow::{Context as _, anyhow};
pub(super) use gpui::*;

pub(super) use super::Editor;
pub(super) use super::tree::VisibleBlock;
pub(super) use crate::components::{
    BlockEvent, BlockKind, BlockRecord, CollapsedCaretAffinity, IndentBlock, InlineTextTree,
    OutdentBlock, PastedImageSource, TableCellPosition, is_table_row_candidate,
    parse_root_table_region, parse_table_body_row,
};
pub(super) use crate::config::{ImagePasteBehavior, read_app_preferences};


impl Editor {
    /// Vertical movement target for caret and block navigation: steps over
    /// blocks that draw nothing in rendered mode (stray closing tags) so the
    /// caret never lands in an invisible row.
    fn vertical_neighbor_index(
        &self,
        visible: &[VisibleBlock],
        from: usize,
        delta: isize,
        cx: &mut Context<Editor>,
    ) -> Option<usize> {
        let mut index = from.checked_add_signed(delta)?;
        while let Some(entry) = visible.get(index) {
            if !entry.entity.read(cx).renders_nothing() {
                return Some(index);
            }
            index = index.checked_add_signed(delta)?;
        }
        None
    }

    fn focused_block_for_tab_key(
        &self,
        window: &mut Window,
        cx: &App,
    ) -> Option<Entity<super::Block>> {
        let is_focused = |block: &Entity<super::Block>| {
            let block = block.read(cx);
            block.focus_handle.is_focused(window)
                || block.code_language_focus_handle.is_focused(window)
        };

        if let Some(block) = self
            .active_entity_id
            .and_then(|entity_id| self.focusable_entity_by_id(entity_id))
            .filter(is_focused)
        {
            return Some(block);
        }

        for binding in self.table_cells.values() {
            if is_focused(&binding.cell) {
                return Some(binding.cell.clone());
            }
        }

        self.document
            .visible_blocks()
            .iter()
            .find_map(|visible| is_focused(&visible.entity).then(|| visible.entity.clone()))
    }

    pub(crate) fn on_editor_key_down_capture(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.search_navigation_selection.is_some()
            && event.keystroke.modifiers.shift
            && matches!(event.keystroke.key.as_str(), "left" | "right" | "up" | "down" | "home" | "end")
            && self.focused_block_for_tab_key(window, cx).is_some()
        {
            self.workspace.search_navigation_selection = None;
        }
        if event.keystroke.key != "tab" {
            return;
        }

        let modifiers = event.keystroke.modifiers;
        if modifiers.control || modifiers.platform || modifiers.alt || modifiers.function {
            return;
        }

        let Some(target) = self.focused_block_for_tab_key(window, cx) else {
            return;
        };

        let handles_tab = {
            let block = target.read(cx);
            if block.code_language_focus_handle.is_focused(window) {
                cx.stop_propagation();
                return;
            }
            block.is_table_cell()
                || block.kind().is_list_item()
                || block.kind() == BlockKind::Paragraph
                || block.kind().is_code_block()
        };

        if !handles_tab {
            return;
        }

        if modifiers.shift {
            target.update(cx, |block, block_cx| {
                block.on_outdent_block(&OutdentBlock, window, block_cx);
            });
        } else {
            target.update(cx, |block, block_cx| {
                block.on_indent_block(&IndentBlock, window, block_cx);
            });
        }
        cx.stop_propagation();
    }

    pub(crate) fn focus_block(&mut self, entity_id: EntityId) {
        self.pending_focus = Some(entity_id);
        self.active_entity_id = Some(entity_id);
        self.pending_scroll_active_block_into_view = true;
    }

    /// 折叠标题后，若当前编辑目标落在被隐藏的章节区间内，把焦点移回标题：
    /// 隐藏块不再渲染，键盘事件会静默丢失（roadmap C7）。
    fn refocus_caret_hidden_by_fold(
        &mut self,
        heading: &Entity<super::Block>,
        cx: &mut Context<Self>,
    ) {
        let Some(target) = self.current_edit_target_from_state(cx) else {
            return;
        };
        if target.entity_id() == heading.entity_id() {
            return;
        }
        let visible = self.document.visible_blocks();
        let Some(heading_index) = visible
            .iter()
            .position(|visible| visible.entity.entity_id() == heading.entity_id())
        else {
            return;
        };
        let Some(target_index) = visible
            .iter()
            .position(|visible| visible.entity.entity_id() == target.entity_id())
        else {
            return;
        };
        if target_index <= heading_index {
            return;
        }
        let level = match heading.read(cx).kind() {
            BlockKind::Heading { level } => level,
            _ => return,
        };
        let inside_section = visible[heading_index + 1..target_index]
            .iter()
            .all(|visible| match visible.entity.read(cx).kind() {
                BlockKind::Heading { level: inner } => inner > level,
                _ => true,
            });
        if inside_section {
            self.focus_block(heading.entity_id());
        }
    }

    fn reset_block_cursor(block: &Entity<super::Block>, cursor: usize, cx: &mut Context<Self>) {
        block.update(cx, move |block, cx| {
            block.selected_range = cursor..cursor;
            block.selection_reversed = false;
            block.marked_range = None;
            block.vertical_motion_x = None;
            block.cursor_blink_epoch = Instant::now();
            cx.notify();
        });
    }

    fn focus_block_range(
        &mut self,
        block: &Entity<super::Block>,
        range: std::ops::Range<usize>,
        cx: &mut Context<Self>,
    ) {
        block.update(cx, move |block, cx| {
            block.selected_range = range.clone();
            block.selection_reversed = false;
            block.marked_range = None;
            block.vertical_motion_x = None;
            block.cursor_blink_epoch = Instant::now();
            cx.notify();
        });
        self.focus_block(block.entity_id());
    }

}

mod block_event;
mod paste;
mod quotes;
mod scroll_mouse;
mod structural;
mod table_nav;

#[cfg(test)]
mod tests;
