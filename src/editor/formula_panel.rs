//! 公式编辑器面板：从公式块上的「ƒx 符号」按钮打开，绑定**这一个块**。
//! 面板顶部实时渲染块里当前的公式，符号网格点一格就写进块内光标处——
//! 面板与公式输入是同一条编辑路径，不是孤立的插入器。

use gpui::*;

use crate::components::latex::{LatexCategory, LatexSymbol, LATEX_SYMBOLS};
use crate::i18n::I18nStrings;
use crate::theme::Theme;

use super::Editor;

/// 面板几何：6 列格子，页签自动换行，整体贴着公式块排。
const PANEL_COLS: u16 = 6;
const CELL: f32 = 46.0;
const CELL_GAP: f32 = 3.0;
const PANEL_PADDING: f32 = 10.0;
const PANEL_WIDTH: f32 = 6.0 * (CELL + CELL_GAP) - CELL_GAP + PANEL_PADDING * 2.0;
const PANEL_VIEWPORT_MARGIN: f32 = 8.0;

/// 面板打开期间的现场：绑定的公式块与当前分组。
pub(crate) struct FormulaPanelState {
    pub(crate) target: gpui::EntityId,
    pub(crate) category: LatexCategory,
    /// 面板上一帧的屏幕区域：编辑器捕获阶段的点击落在其内时不关闭
    /// （格子的插入靠 bubble 阶段的同一次按下）。
    pub(crate) panel_bounds: Option<Bounds<Pixels>>,
}

/// 分类页签的固定次序（与符号表的组织一致）。
const CATEGORIES: [LatexCategory; 6] = [
    LatexCategory::Structures,
    LatexCategory::Greek,
    LatexCategory::Operators,
    LatexCategory::Arrows,
    LatexCategory::Functions,
    LatexCategory::Symbols,
];

fn category_label(category: LatexCategory, strings: &I18nStrings) -> String {
    match category {
        LatexCategory::Greek => strings.latex_category_greek.clone(),
        LatexCategory::Operators => strings.latex_category_operators.clone(),
        LatexCategory::Arrows => strings.latex_category_arrows.clone(),
        LatexCategory::Structures => strings.latex_category_structures.clone(),
        LatexCategory::Functions => strings.latex_category_functions.clone(),
        LatexCategory::Symbols => strings.latex_category_symbols.clone(),
    }
}

impl Editor {
    /// 面板上一帧的屏幕区域（捕获阶段点击判定用）。
    pub(crate) fn formula_panel_bounds(&self) -> Option<Bounds<Pixels>> {
        self.formula_panel.as_ref()?.panel_bounds
    }

    /// 数学块的「ƒx 符号」按钮：本块已开面板就收起，否则打开并绑定本块。
    pub(crate) fn toggle_formula_panel_for_block(
        &mut self,
        target: EntityId,
        cx: &mut Context<Self>,
    ) {
        if let Some(state) = self.formula_panel.as_ref()
            && state.target == target
        {
            self.close_formula_panel(cx);
            return;
        }
        self.formula_panel = Some(FormulaPanelState {
            target,
            category: LatexCategory::Structures,
            panel_bounds: None,
        });
        cx.notify();
    }

    pub(crate) fn close_formula_panel(&mut self, cx: &mut Context<Self>) {
        if self.formula_panel.take().is_some() {
            cx.notify();
        }
    }

    fn set_formula_panel_category(&mut self, category: LatexCategory, cx: &mut Context<Self>) {
        if let Some(state) = self.formula_panel.as_mut() {
            state.category = category;
            cx.notify();
        }
    }

    /// 点一格：把那条 LaTeX 写进绑定块**当前光标**处（一次不可合并的 undo
    /// 步），光标按模板落点摆好。面板点击不抢焦点，块内光标始终是活的，
    /// 打字与点符号可以交替进行。
    pub(crate) fn insert_latex_symbol(
        &mut self,
        entry: &'static LatexSymbol,
        cx: &mut Context<Self>,
    ) {
        let Some(target) = self.formula_panel.as_ref().map(|state| state.target) else {
            return;
        };
        let Some(block) = self.document.block_entity_by_id(target) else {
            self.close_formula_panel(cx);
            return;
        };
        let insert = entry.insert;
        let entry_caret = entry.caret;
        block.update(cx, |block, block_cx| {
            let at = block.cursor_offset().min(block.visible_len());
            block.prepare_undo_capture(
                crate::components::UndoCaptureKind::NonCoalescible,
                block_cx,
            );
            block.replace_text_in_visible_range(at..at, insert, None, false, block_cx);
            block.move_to(at + entry_caret, block_cx);
        });
        cx.notify();
    }

    /// 面板浮层：锚在绑定公式块旁边，顶部实时渲染块内当前公式。
    pub(crate) fn render_formula_panel_overlay(
        &mut self,
        theme: &Theme,
        strings: &I18nStrings,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let c = &theme.colors;
        let t = &theme.typography;
        let state = self.formula_panel.as_ref()?;
        let block = self.document.block_entity_by_id(state.target)?;
        let viewport = window.viewport_size();

        // 锚点：块在本帧的 bounds 右侧；没画过就退回视口中上。
        let anchor = block
            .read(cx)
            .last_bounds
            .map(|bounds| point(bounds.right() + px(10.0), bounds.top()))
            .unwrap_or_else(|| point(viewport.width * 0.5, px(96.0)));

        // 实时预览随后一笔接入；此处先占位。
        let preview_element: AnyElement = div().into_any_element();
        let category = state.category;
        let tabs: Vec<AnyElement> = CATEGORIES
            .iter()
            .map(|&tab| {
                let is_active = tab == category;
                div()
                    .id(ElementId::Name(format!("formula-tab-{:?}", tab).into()))
                    .px(px(9.0))
                    .h(px(24.0))
                    .flex()
                    .items_center()
                    .rounded(px(999.0))
                    .cursor_pointer()
                    .text_size(px(t.text_size * 0.78))
                    .text_color(if is_active {
                        c.dialog_primary_button_text
                    } else {
                        c.dialog_muted
                    })
                    .bg(if is_active {
                        c.dialog_primary_button_bg
                    } else {
                        hsla(0.0, 0.0, 0.0, 0.0)
                    })
                    .hover(|this| this.bg(c.dialog_secondary_button_hover))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |editor, _event: &MouseDownEvent, _window, cx| {
                            editor.set_formula_panel_category(tab, cx);
                        }),
                    )
                    .child(category_label(tab, strings))
                    .into_any_element()
            })
            .collect();

        let entries: Vec<&'static LatexSymbol> = LATEX_SYMBOLS
            .iter()
            .filter(|entry| entry.category == category)
            .collect();
        let preview_color = c.text_default;
        let preview_size = f32::from(t.text_size) * 0.95;
        let mut cells = Vec::with_capacity(entries.len());
        for entry in entries {
            let insert_label: SharedString = format!("\\{}", entry.name).into();
            let cell = div()
                .id(ElementId::Name(format!("formula-cell-{}", entry.name).into()))
                .size(px(CELL))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(6.0))
                .cursor_pointer()
                .bg(c.dialog_secondary_button_bg)
                .hover(|this| this.bg(c.dialog_secondary_button_hover))
                .tooltip(move |_, cx| {
                    cx.new(|_| FormulaSymbolTooltip {
                        label: insert_label.clone(),
                        text_color: preview_color,
                    })
                    .into()
                })
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |editor, _event: &MouseDownEvent, _window, cx| {
                        editor.insert_latex_symbol(entry, cx);
                    }),
                );
            // 预览渲染失败退化成命令名，不出空格子。
            let content: AnyElement = match crate::components::latex::render_inline_math_svg(
                entry.preview,
                preview_color,
                preview_size,
            ) {
                Ok(rendered) => img(rendered.path)
                    .max_h(px(CELL * 0.66))
                    .max_w(px(CELL * 0.92))
                    .object_fit(ObjectFit::Contain)
                    .into_any_element(),
                Err(_) => div()
                    .text_size(px(t.text_size * 0.72))
                    .text_color(c.dialog_muted)
                    .child(format!("\\{}", entry.name))
                    .into_any_element(),
            };
            cells.push(cell.child(content).into_any_element());
        }

        let grid_width = px(PANEL_COLS as f32 * (CELL + CELL_GAP) - CELL_GAP);
        let rows = (cells.len() as f32 / PANEL_COLS as f32).ceil().max(1.0);
        let grid_height = px(rows * (CELL + CELL_GAP) - CELL_GAP);
        let panel_height = px(PANEL_PADDING * 2.0)
            + px(26.0)
            + px(6.0)
            + px(80.0)
            + px(6.0)
            + px(56.0)
            + px(6.0)
            + grid_height
            + px(6.0)
            + px(20.0);
        let left = anchor
            .x
            .min(viewport.width - px(PANEL_WIDTH) - px(PANEL_VIEWPORT_MARGIN))
            .max(px(PANEL_VIEWPORT_MARGIN));
        let top = anchor
            .y
            .min(viewport.height - panel_height - px(PANEL_VIEWPORT_MARGIN))
            .max(px(PANEL_VIEWPORT_MARGIN));
        let state = self.formula_panel.as_mut()?;
        state.panel_bounds = Some(Bounds::new(
            point(left, top),
            size(px(PANEL_WIDTH), panel_height),
        ));

        Some(
            div()
                .id("formula-panel")
                .debug_selector(|| "formula-panel".to_string())
                .absolute()
                .left(left)
                .top(top)
                .w(px(PANEL_WIDTH))
                .bg(c.dialog_surface)
                .border_1()
                .border_color(c.dialog_border)
                .rounded(px(10.0))
                .shadow_lg()
                .p(px(PANEL_PADDING))
                .occlude()
                .flex()
                .flex_col()
                .gap(px(6.0))
                .child(
                    div()
                        .h(px(26.0))
                        .flex()
                        .items_center()
                        .child(
                            div()
                                .text_size(px(t.text_size * 0.9))
                                .text_color(c.text_default)
                                .font_weight(FontWeight::MEDIUM)
                                .child(strings.insert_formula.clone()),
                        )
                        .child(div().flex_1())
                        .child(
                            div()
                                .id("formula-panel-close")
                                .size(px(22.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(5.0))
                                .cursor_pointer()
                                .text_size(px(t.text_size * 0.85))
                                .text_color(c.dialog_muted)
                                .hover(|this| this.bg(c.dialog_secondary_button_hover))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|editor, _event: &MouseDownEvent, _window, cx| {
                                        editor.close_formula_panel(cx);
                                    }),
                                )
                                .child("×"),
                        ),
                )
                .child(
                    div()
                        .h(px(80.0))
                        .w_full()
                        .rounded(px(6.0))
                        .bg(c.code_bg)
                        .flex()
                        .items_center()
                        .justify_center()
                        .overflow_hidden()
                        .child(preview_element),
                )
                .child(div().flex().flex_wrap().gap(px(4.0)).children(tabs))
                .child(
                    div()
                        .grid()
                        .grid_cols(PANEL_COLS)
                        .gap(px(CELL_GAP))
                        .w(grid_width)
                        .children(cells),
                )
                .child(
                    div()
                        .text_size(px(t.text_size * 0.72))
                        .text_color(c.dialog_muted)
                        .child(strings.formula_panel_hint.clone()),
                )
                .into_any_element(),
        )
    }
}

/// 悬停说明：一格的 LaTeX 写法。
struct FormulaSymbolTooltip {
    label: SharedString,
    text_color: Hsla,
}

impl Render for FormulaSymbolTooltip {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px(px(6.0))
            .py(px(3.0))
            .rounded(px(4.0))
            .bg(black().opacity(0.85))
            .text_color(self.text_color)
            .text_size(px(12.0))
            .child(self.label.clone())
    }
}
