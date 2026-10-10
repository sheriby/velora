//! 工作区双链自动补全。

use std::path::PathBuf;

use gpui::{AnyElement, Window};

use super::workspace::is_markdown_document;
use super::Editor;
use crate::theme::Theme;

/// 补全浮层的会话状态：锚定在某个块的某个 `[["` 之后，随编辑实时刷新。
pub(crate) struct WikilinkCompletion {
    pub(crate) block_id: gpui::EntityId,
    /// `[[` 之后第一个字节的偏移（查询串起点）。
    pub(crate) anchor: usize,
    pub(crate) query: String,
    pub(crate) selected: usize,
    pub(crate) results: Vec<PathBuf>,
    /// 浮层上一帧的屏幕区域：编辑器的捕获阶段点击落在其内时不关闭
    /// （行点击确认靠 bubble 阶段的同一次按下）。
    pub(crate) panel_bounds: Option<gpui::Bounds<gpui::Pixels>>,
}

/// 补全列表容量：浮动列表最多 8 行，超出靠继续输入收敛。
const WIKILINK_COMPLETION_LIMIT: usize = 8;

impl Editor {
    /// 块文本变化后刷新 `[[` 补全（`on_block_event` 的 Changed 分支调用）。
    /// 只在 Markdown 文档 + 工作区已打开时生效；无锚点/查询含 `]`/换行
    /// 即关闭。
    pub(crate) fn update_wikilink_completion_for_block(
        &mut self,
        block: &gpui::Entity<super::Block>,
        cx: &mut gpui::Context<Self>,
    ) {
        // 代码文档/降级源码是等宽源码视图，wikilink 不是其语义。
        if self.code_document || self.source_mode_fallback_required || self.workspace.root.is_none()
        {
            return self.close_wikilink_completion(cx);
        }
        let block_ref = block.read(cx);
        let text = block_ref.display_text();
        let up_to = block_ref.cursor_offset().min(text.len());
        let line_start = text[..up_to].rfind('\n').map(|index| index + 1).unwrap_or(0);
        let prefix = &text[line_start..up_to];
        let Some(anchor_rel) = prefix.rfind("[[") else {
            return self.close_wikilink_completion(cx);
        };
        let anchor = line_start + anchor_rel + 2;
        let query = &text[anchor..up_to];
        if query.contains(']') {
            return self.close_wikilink_completion(cx);
        }

        let query_lower = query.to_lowercase();
        let mut results: Vec<PathBuf> = self
            .workspace_text_files()
            .into_iter()
            .filter(|path| is_markdown_document(path))
            .filter(|path| {
                query_lower.is_empty()
                    || path
                        .file_name()
                        .map(|name| name.to_string_lossy().to_lowercase())
                        .is_some_and(|name| name.contains(&query_lower))
            })
            .take(WIKILINK_COMPLETION_LIMIT)
            .collect();
        results.sort_by_key(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().to_lowercase())
                .unwrap_or_default()
        });

        let unchanged = self.wikilink_completion.as_ref().is_some_and(|state| {
            state.block_id == block.entity_id()
                && state.anchor == anchor
                && state.query == query
        });
        match self.wikilink_completion.as_mut() {
            Some(_) if unchanged => {}
            Some(state) => {
                state.anchor = anchor;
                state.query = query.to_string();
                state.results = results;
                state.selected = 0;
            }
            None => {
                self.wikilink_completion = Some(WikilinkCompletion {
                    block_id: block.entity_id(),
                    anchor,
                    query: query.to_string(),
                    selected: 0,
                    results,
                    panel_bounds: None,
                });
            }
        }
        cx.notify();
    }

    pub(crate) fn close_wikilink_completion(&mut self, cx: &mut gpui::Context<Self>) {
        if self.wikilink_completion.take().is_some() {
            cx.notify();
        }
    }

    pub(crate) fn wikilink_completion_is_open(&self) -> bool {
        self.wikilink_completion.is_some()
    }

    /// 补全列表按键处理（intercept_keystrokes 钩子调用，先于 keymap 绑定，
    /// 否则 ↑/↓/Enter 会被焦点块的光标移动与换行绑定消费）。返回是否消费。
    pub(crate) fn wikilink_completion_key_down(
        &mut self,
        keystroke: &gpui::Keystroke,
        cx: &mut gpui::Context<Self>,
    ) -> bool {
        if !self.wikilink_completion_is_open() {
            return false;
        }
        match keystroke.key.as_str() {
            "up" => {
                if let Some(state) = self.wikilink_completion.as_mut()
                    && !state.results.is_empty()
                {
                    state.selected =
                        (state.selected + state.results.len() - 1) % state.results.len();
                    cx.notify();
                }
                cx.stop_propagation();
                true
            }
            "down" => {
                if let Some(state) = self.wikilink_completion.as_mut()
                    && !state.results.is_empty()
                {
                    state.selected = (state.selected + 1) % state.results.len();
                    cx.notify();
                }
                cx.stop_propagation();
                true
            }
            "enter" => {
                let selected = self
                    .wikilink_completion
                    .as_ref()
                    .map(|state| state.selected)
                    .unwrap_or(0);
                cx.stop_propagation();
                self.confirm_wikilink_completion(selected, cx);
                true
            }
            "escape" => {
                cx.stop_propagation();
                self.close_wikilink_completion(cx);
                true
            }
            _ => false,
        }
    }

    /// 用选中项替换查询串并补上 `]]`（C3 的 `open_wikilink` 按 stem 匹配，
    /// 因此插入 stem）。带一次不可合并的 undo 捕获。
    pub(crate) fn confirm_wikilink_completion(
        &mut self,
        selected: usize,
        cx: &mut gpui::Context<Self>,
    ) {
        let Some(state) = self.wikilink_completion.take() else {
            return;
        };
        let Some(path) = state.results.get(selected) else {
            cx.notify();
            return;
        };
        let stem = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().to_string())
            .unwrap_or_default();
        let Some(block) = self.document.block_entity_by_id(state.block_id) else {
            cx.notify();
            return;
        };
        block.update(cx, |block, block_cx| {
            let end = block
                .cursor_offset()
                .min(state.anchor + state.query.len())
                .max(state.anchor);
            block.prepare_undo_capture(
                crate::components::UndoCaptureKind::NonCoalescible,
                block_cx,
            );
            block.replace_text_in_visible_range(
                state.anchor..end,
                &format!("{stem}]]"),
                None,
                false,
                block_cx,
            );
            block.move_to(state.anchor + stem.len() + 2, block_cx);
        });
        cx.notify();
    }

    /// 补全浮层：锚在焦点块光标下方；布局未跟上（caret 无界）的帧不渲染。
    pub(crate) fn render_wikilink_completion_overlay(
        &mut self,
        theme: &Theme,
        window: &Window,
        cx: &mut gpui::Context<Self>,
    ) -> Option<AnyElement> {
        use gpui::*;
        let state = self.wikilink_completion.as_ref()?;
        let block = self.document.block_entity_by_id(state.block_id)?;
        let caret_bounds = block.read(cx).active_range_or_cursor_bounds()?;
        let state = self.wikilink_completion.as_mut()?;
        let panel_origin_x = caret_bounds
            .left()
            .min(window.viewport_size().width - px(292.0))
            .max(px(0.0));
        let panel_origin_y = (caret_bounds.bottom() + px(4.0))
            .min(window.viewport_size().height - px(48.0));
        state.panel_bounds = Some(Bounds::new(
            point(panel_origin_x, panel_origin_y),
            size(px(280.0), px(26.0 * state.results.len() as f32 + 8.0)),
        ));
        let selected = state.selected;
        let result_count = state.results.len();
        let c = &theme.colors;
        let t = &theme.typography;
        let viewport = window.viewport_size();
        let panel_width = px(280.0);
        let left = caret_bounds
            .left()
            .min(viewport.width - panel_width - px(12.0))
            .max(px(0.0));
        let top = (caret_bounds.bottom() + px(4.0)).min(viewport.height - px(48.0));

        let mut rows = Vec::new();
        for (index, path) in self
            .wikilink_completion
            .as_ref()
            .map(|state| state.results.clone())
            .unwrap_or_default()
            .iter()
            .enumerate()
            .take(if result_count > 0 { result_count } else { 0 })
        {
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_default();
            let dir = path
                .parent()
                .and_then(|parent| parent.file_name())
                .map(|parent| parent.to_string_lossy().to_string());
            let is_selected = index == selected;
            let confirm = cx.listener(move |editor, _event: &MouseDownEvent, _window, cx| {
                editor.confirm_wikilink_completion(index, cx);
            });
            rows.push(
                div()
                    .id(gpui::ElementId::Name(format!("wikilink-entry-{index}").into()))
                    .debug_selector(move || format!("wikilink-entry-{index}"))
                    .h(px(26.0))
                    .w_full()
                    .overflow_hidden()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .px(px(8.0))
                    .rounded(px(4.0))
                    .cursor_pointer()
                    .bg(if is_selected {
                        c.selection
                    } else {
                        hsla(0.0, 0.0, 0.0, 0.0)
                    })
                    .hover(|this| this.bg(c.dialog_secondary_button_hover))
                    .on_mouse_down(MouseButton::Left, confirm)
                    .child(
                        div()
                            .text_size(px(t.dialog_body_size * 0.9))
                            .text_color(c.text_default)
                            .child(name),
                    )
                    .children(dir.map(|dir| {
                        div()
                            .text_size(px(t.dialog_body_size * 0.75))
                            .text_color(c.dialog_muted)
                            .child(dir)
                    })),
            );
        }

        Some(
            div()
                .id("wikilink-completion")
                .debug_selector(|| "wikilink-completion".to_string())
                .absolute()
                .left(left)
                .top(top)
                .w(panel_width)
                .max_h(px(26.0 * WIKILINK_COMPLETION_LIMIT as f32 + 8.0))
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap(px(2.0))
                .p(px(4.0))
                .rounded(px(8.0))
                .bg(c.dialog_surface)
                .border(px(1.0))
                .border_color(c.dialog_border)
                .shadow_lg()
                .occlude()
                .children(rows)
                .into_any_element(),
        )
    }
}
