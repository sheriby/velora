//! `\` 公式命令补全：数学块或行内公式里打 `\` 弹出，↑/↓ 选择、Enter 确认、
//! Esc 关闭。会话结构与 [[ 补全同构：锚定在某个块的某个反斜杠之后，随编辑
//! 实时刷新。

use gpui::{Bounds, Pixels};

use crate::components::latex::{
    LatexSymbol, inside_inline_math, latex_command_before_cursor, latex_completions_for,
};
use super::Editor;

/// 补全列表容量：浮动列表最多 8 行，超出靠继续输入收敛。
const LATEX_COMPLETION_LIMIT: usize = 8;

/// 补全浮层的会话状态：锚定在某个块的某个 `\` 之后，随编辑实时刷新。
pub(crate) struct LatexCompletion {
    pub(crate) block_id: gpui::EntityId,
    /// `\` 的偏移（查询串起点）。
    pub(crate) anchor: usize,
    /// 反斜杠之后的查询串（只含字母，可能为空）。
    pub(crate) query: String,
    pub(crate) selected: usize,
    pub(crate) results: Vec<&'static LatexSymbol>,
    /// 浮层上一帧的屏幕区域：编辑器的捕获阶段点击落在其内时不关闭
    /// （行点击确认靠 bubble 阶段的同一次按下）。
    pub(crate) panel_bounds: Option<Bounds<Pixels>>,
}

impl Editor {
    /// 块文本变化后刷新 `\` 补全（`on_block_event` 的 Changed 分支调用）。
    /// 只在渲染视图生效；光标前不是「公式上下文里的 `\命令`」即关闭。
    pub(crate) fn update_latex_completion_for_block(
        &mut self,
        block: &gpui::Entity<super::Block>,
        cx: &mut gpui::Context<Self>,
    ) {
        // 源码视图不是打公式的场景；代码文档更不是；[[ 补全开着时让它独占。
        if self.code_document
            || self.source_mode_fallback_required
            || self.view_mode != super::ViewMode::Rendered
            || self.wikilink_completion.is_some()
        {
            return self.close_latex_completion(cx);
        }
        let block_ref = block.read(cx);
        // 代码块里的 `$\fr` 是代码不是公式；HTML/mermaid 原文同理。
        let math_context_kind = matches!(
            block_ref.kind(),
            super::BlockKind::Paragraph | super::BlockKind::MathBlock
        );
        let is_math_block = matches!(block_ref.kind(), super::BlockKind::MathBlock);
        let text = block_ref.display_text();
        let up_to = block_ref.cursor_offset().min(text.len());
        let line_start = text[..up_to].rfind('\n').map(|index| index + 1).unwrap_or(0);
        let prefix = &text[line_start..up_to];
        let Some((backslash_rel, query)) = latex_command_before_cursor(prefix) else {
            return self.close_latex_completion(cx);
        };
        // 数学上下文：数学块里处处算数；段落里要看行内 `$` 配对；其余
        // 块类（代码、HTML、mermaid）不算公式。
        if !math_context_kind
            || (!is_math_block && !inside_inline_math(prefix[..backslash_rel].trim_end()))
        {
            return self.close_latex_completion(cx);
        }

        let anchor = line_start + backslash_rel;
        let results = latex_completions_for(query, LATEX_COMPLETION_LIMIT);
        let unchanged = self.latex_completion.as_ref().is_some_and(|state| {
            state.block_id == block.entity_id()
                && state.anchor == anchor
                && state.query == query
        });
        match self.latex_completion.as_mut() {
            Some(_) if unchanged => {}
            Some(state) => {
                state.anchor = anchor;
                state.query = query.to_string();
                state.results = results;
                state.selected = 0;
            }
            None => {
                self.latex_completion = Some(LatexCompletion {
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

    pub(crate) fn close_latex_completion(&mut self, cx: &mut gpui::Context<Self>) {
        if self.latex_completion.take().is_some() {
            cx.notify();
        }
    }

    pub(crate) fn latex_completion_is_open(&self) -> bool {
        self.latex_completion.is_some()
    }

    /// 补全列表按键处理（intercept_keystrokes 钩子调用，先于 keymap 绑定，
    /// 否则 ↑/↓/Enter 会被焦点块的光标移动与换行绑定消费）。返回是否消费。
    pub(crate) fn latex_completion_key_down(
        &mut self,
        keystroke: &gpui::Keystroke,
        cx: &mut gpui::Context<Self>,
    ) -> bool {
        if !self.latex_completion_is_open() {
            return false;
        }
        match keystroke.key.as_str() {
            "up" => {
                if let Some(state) = self.latex_completion.as_mut()
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
                if let Some(state) = self.latex_completion.as_mut()
                    && !state.results.is_empty()
                {
                    state.selected = (state.selected + 1) % state.results.len();
                    cx.notify();
                }
                cx.stop_propagation();
                true
            }
            "enter" => {
                let (selected, has_results) = self
                    .latex_completion
                    .as_ref()
                    .map(|state| (state.selected, !state.results.is_empty()))
                    .unwrap_or((0, false));
                if !has_results {
                    // 没有候选就不抢换行键：关掉浮层，让回车落进公式里。
                    self.close_latex_completion(cx);
                    return false;
                }
                cx.stop_propagation();
                self.confirm_latex_completion(selected, cx);
                true
            }
            "escape" => {
                cx.stop_propagation();
                self.close_latex_completion(cx);
                true
            }
            _ => false,
        }
    }

    /// 用选中项替换 `\查询串` 并按模板落光标。带一次不可合并的 undo 捕获。
    pub(crate) fn confirm_latex_completion(
        &mut self,
        selected: usize,
        cx: &mut gpui::Context<Self>,
    ) {
        let Some(state) = self.latex_completion.take() else {
            return;
        };
        let Some(entry) = state.results.get(selected) else {
            cx.notify();
            return;
        };
        let Some(block) = self.document.block_entity_by_id(state.block_id) else {
            cx.notify();
            return;
        };
        let insert = entry.insert;
        let caret = entry.caret;
        block.update(cx, |block, block_cx| {
            let end = block
                .cursor_offset()
                .min(state.anchor + 1 + state.query.len())
                .max(state.anchor);
            block.prepare_undo_capture(
                crate::components::UndoCaptureKind::NonCoalescible,
                block_cx,
            );
            block.replace_text_in_visible_range(state.anchor..end, insert, None, false, block_cx);
            block.move_to(state.anchor + caret, block_cx);
        });
        cx.notify();
    }

}

#[cfg(test)]
mod tests {
    // 不能 `use super::*`：外层的 `use gpui::*` 会把 gpui 的 `test`
    // 属性宏带进来遮蔽内置 `#[test]`，宏展开直接递归爆栈。
    use crate::components::latex::latex_completions_for;

    const LATEX_COMPLETION_LIMIT: usize = 8;

    #[test]
    fn limit_keeps_the_curated_order() {
        let results = latex_completions_for("", LATEX_COMPLETION_LIMIT);
        assert_eq!(results.len(), LATEX_COMPLETION_LIMIT);
        assert_eq!(results[0].name, "frac");
    }
}
