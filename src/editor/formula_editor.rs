//! 公式编辑器：独立的弹窗编辑窗口。
//!
//! 双击数学块弹出（或从块上入口打开）：窗口里有自己的草稿输入区（独立焦点
//! 与光标，走编辑器 overlay 输入的 IME 路由，中文也能打）、上方实时渲染
//! 草稿的公式、下方分类符号面板（点击插入草稿光标处）。「应用」把草稿写回
//! 数学块（一次不可合并的 undo 组），Esc/取消丢弃草稿。
//!
//! 为什么是草稿而不是直接改块：弹窗是独立编辑现场，取消必须能干净丢弃；
//! 直接改块会把中间状态泄进 undo 栈与自动保存。

use std::ops::Range;
use std::path::PathBuf;

use gpui::*;

use crate::components::latex::{
    LatexCategory, LatexSymbol, LATEX_SYMBOLS, latex_command_before_cursor,
    latex_completions_for,
};
use crate::components::markdown::code_highlight::{CodeHighlightSpan, code_highlight_color};
use crate::components::markdown::source_highlight::highlight_latex_source;
use crate::i18n::I18nStrings;
use crate::theme::{Theme, ThemeColors};

use super::Editor;

/// 弹窗几何：居中卡片，预览、输入、符号面板三段。
const PANEL_WIDTH: f32 = 720.0;
const PREVIEW_HEIGHT: f32 = 150.0;
const INPUT_HEIGHT: f32 = 132.0;
const GRID_COLS: u16 = 8;
const CELL: f32 = 46.0;
const CELL_GAP: f32 = 3.0;
const PANEL_VIEWPORT_MARGIN: f32 = 16.0;
const INPUT_LINE_HEIGHT: f32 = 21.0;
/// 面板内边距、条目间距、标题行高、输入框描边与内边距：补全浮层要按这套
/// 常量把光标位置换算成面板内坐标，任何一处改动都得跟着对。
const PANEL_PADDING: f32 = 14.0;
const PANEL_GAP: f32 = 8.0;
const TITLE_HEIGHT: f32 = 26.0;
const INPUT_BORDER: f32 = 1.0;
const INPUT_PADDING_X: f32 = 10.0;
const INPUT_PADDING_Y: f32 = 8.0;
/// 草稿输入区在面板内的纵向起点（标题 + 预览 + 两道间距）。
const INPUT_TOP: f32 = PANEL_PADDING + TITLE_HEIGHT + PANEL_GAP + PREVIEW_HEIGHT + PANEL_GAP;
/// 补全浮层：贴着光标的小列表，不铺满面板宽。
const COMPLETION_WIDTH: f32 = 240.0;
const COMPLETION_ROW_HEIGHT: f32 = 28.0;

/// 分类页签的固定次序（与符号表的组织一致）。
const CATEGORIES: [LatexCategory; 6] = [
    LatexCategory::Structures,
    LatexCategory::Greek,
    LatexCategory::Operators,
    LatexCategory::Arrows,
    LatexCategory::Functions,
    LatexCategory::Symbols,
];

/// 草稿一行的渲染输入：行文本 + 铺满该行的样式段。
pub(crate) struct FormulaDraftLine {
    pub(crate) text: String,
    pub(crate) runs: Vec<TextRun>,
}

/// 把草稿按行切成带 LaTeX 语法色的渲染段。配色与数学块编辑态同源（同一套
/// `highlight_latex_source` + `code_highlight_color`）：弹窗里改的就是那几行
/// 源码，两处颜色不一样会让人觉得不是同一个公式（用户报修：草稿没颜色）。
pub(crate) fn formula_draft_lines(
    draft: &str,
    colors: &ThemeColors,
    font: Font,
) -> Vec<FormulaDraftLine> {
    let spans = highlight_latex_source(draft);
    let mut lines = Vec::new();
    let mut line_start = 0usize;
    loop {
        let line_end = match draft[line_start..].find('\n') {
            Some(offset) => line_start + offset,
            None => draft.len(),
        };
        lines.push(formula_draft_line(
            &draft[line_start..line_end],
            line_start,
            &spans,
            colors,
            &font,
        ));
        if line_end == draft.len() {
            return lines;
        }
        line_start = line_end + 1;
    }
}

/// 单行：把落在本行的高亮区间裁成边界，逐段取色。空行给一个空格占位，
/// 行高与光标落点才有依托。
fn formula_draft_line(
    line: &str,
    line_start: usize,
    spans: &[CodeHighlightSpan],
    colors: &ThemeColors,
    font: &Font,
) -> FormulaDraftLine {
    let base_run = |len: usize, color: Hsla| TextRun {
        len,
        font: font.clone(),
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
        font_size: None,
    };
    if line.is_empty() {
        return FormulaDraftLine {
            text: " ".to_string(),
            runs: vec![base_run(1, colors.text_default)],
        };
    }
    let mut boundaries: Vec<usize> = vec![0, line.len()];
    for span in spans {
        boundaries.push(span.range.start.saturating_sub(line_start).min(line.len()));
        boundaries.push(span.range.end.saturating_sub(line_start).min(line.len()));
    }
    boundaries.sort_unstable();
    boundaries.dedup();

    let mut runs = Vec::new();
    for pair in boundaries.windows(2) {
        let (start, end) = (pair[0], pair[1]);
        if start >= end {
            continue;
        }
        let absolute = line_start + start;
        let color = spans
            .iter()
            .find(|span| span.range.start <= absolute && absolute < span.range.end)
            .map(|span| code_highlight_color(colors, span.class))
            .unwrap_or(colors.text_default);
        runs.push(base_run(end - start, color));
    }
    FormulaDraftLine {
        text: line.to_string(),
        runs,
    }
}

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

/// 编辑器弹窗的现场：目标块、草稿文本与其光标、实时预览。
pub(crate) struct FormulaEditorState {
    pub(crate) target: gpui::EntityId,
    /// 草稿 = 公式体（不含 `$$` 定界符）；「应用」时才写回块。
    pub(crate) draft: String,
    pub(crate) selected_range: Range<usize>,
    pub(crate) marked_range: Option<Range<usize>>,
    pub(crate) focus: Option<FocusHandle>,
    pub(crate) category: LatexCategory,
    /// 草稿的实时预览：ratex 渲染好的缓存 SVG。渲染失败时为 Err（界面上
    /// 显示 LaTeX 源码 + 错误），空草稿为 None（显示占位提示）。
    pub(crate) preview_path: Option<PathBuf>,
    pub(crate) preview_error: Option<String>,
    /// 草稿里打 `\` 弹出的命令补全：锚在反斜杠上，随编辑刷新。
    pub(crate) completion: Option<FormulaDraftCompletion>,
}

/// 草稿输入区的 `\` 命令补全会话（弹窗内的，与块编辑的 latex_completion
/// 互不相干）。
pub(crate) struct FormulaDraftCompletion {
    /// 反斜杠在草稿里的偏移。
    pub(crate) anchor: usize,
    pub(crate) selected: usize,
    pub(crate) results: Vec<&'static LatexSymbol>,
}

/// 补全列表容量。
const COMPLETION_LIMIT: usize = 8;

/// 光标前移一个字素边界（退格用）。
fn draft_prev_boundary(text: &str, offset: usize) -> usize {
    text[..offset]
        .char_indices()
        .next_back()
        .map(|(index, _)| index)
        .unwrap_or(0)
}

/// 光标后移一个字素边界（前向删除用）。
fn draft_next_boundary(text: &str, offset: usize) -> usize {
    text[offset..]
        .char_indices()
        .nth(1)
        .map(|(index, _)| offset + index)
        .unwrap_or(text.len())
}

impl Editor {
    /// 双击数学块（或其它入口）打开公式编辑器：公式体拷进草稿，焦点交给
    /// 草稿输入。
    pub(crate) fn open_formula_editor_for_block(
        &mut self,
        target: gpui::EntityId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(block) = self.document.block_entity_by_id(target) else {
            return;
        };
        let raw = block.read(cx).display_text().to_string();
        let draft = crate::components::latex::parse_display_math_source(&raw)
            .map(|source| source.body)
            .unwrap_or(raw);
        self.dismiss_contextual_overlays(cx);
        let focus = cx.focus_handle();
        window.focus(&focus);
        let mut editor = FormulaEditorState {
            target,
            selected_range: draft.len()..draft.len(),
            marked_range: None,
            focus: Some(focus),
            category: LatexCategory::Structures,
            draft,
            preview_path: None,
            preview_error: None,
            completion: None,
        };
        Self::sync_formula_preview(&mut editor, cx);
        self.formula_editor = Some(editor);
        cx.notify();
    }

    /// 块事件链记下的待打开目标：render 首帧（有 window）时真正开弹窗。
    pub(crate) fn apply_pending_formula_editor(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(target) = self.pending_formula_editor.take() else {
            return;
        };
        self.open_formula_editor_for_block(target, window, cx);
    }

    pub(crate) fn close_formula_editor(&mut self, cx: &mut Context<Self>) {
        if self.formula_editor.take().is_some() {
            self.restore_focus_after_overlay(cx);
            cx.notify();
        }
    }

    /// 把草稿渲染进预览（同步 ratex + 磁盘缓存；公式短，每次编辑一次渲染
    /// 与 Typora 同档）。结果存进弹窗状态，帧间零成本。
    pub(crate) fn sync_formula_preview(state: &mut FormulaEditorState, cx: &App) {
        let (color, font_size) = {
            let theme = cx.global::<crate::theme::ThemeManager>().current_arc();
            (
                theme.colors.text_default,
                crate::components::latex::display_math_font_size(theme.typography.text_size),
            )
        };
        if state.draft.trim().is_empty() {
            state.preview_path = None;
            state.preview_error = None;
            return;
        }
        match crate::components::latex::render_display_math_svg(
            &crate::components::latex::DisplayMathSource {
                raw: state.draft.clone(),
                body: state.draft.clone(),
            },
            color,
            font_size,
        ) {
            Ok(rendered) => {
                state.preview_path = Some(rendered.path);
                state.preview_error = None;
            }
            Err(err) => {
                state.preview_path = None;
                state.preview_error = Some(err.to_string());
            }
        }
    }

    /// 在草稿光标处替换/插入文本（输入系统、符号面板共用这一条），随后
    /// 刷新预览。`selected_in_inserted` 是插入文本内部的光标落点（模板用）。
    pub(crate) fn replace_formula_draft(
        &mut self,
        range: Range<usize>,
        new_text: &str,
        selected_in_inserted: Option<Range<usize>>,
        marked: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.formula_editor.as_mut() else {
            return;
        };
        let old = state.draft.clone();
        let start = range.start.min(old.len());
        let end = range.end.min(old.len()).max(start);
        if !old.is_char_boundary(start) || !old.is_char_boundary(end) {
            return;
        }
        let mut updated = old.clone();
        updated.replace_range(start..end, new_text);
        let inserted_end = start + new_text.len();
        let selection = selected_in_inserted
            .map(|selection| {
                start + selection.start.min(new_text.len())
                    ..start + selection.end.min(new_text.len())
            })
            .unwrap_or(inserted_end..inserted_end);
        state.draft = updated;
        state.selected_range = selection;
        state.marked_range = (marked && !new_text.is_empty()).then_some(start..inserted_end);
        Self::refresh_formula_draft_completion(state);
        Self::sync_formula_preview(state, cx);
        cx.notify();
    }

    /// 草稿光标前是「`\` + 字母」时弹命令补全（弹窗里处处是公式上下文，
    /// 无需再判行内数学）。敲字（overlay 输入）与自管编辑两条路径都要调，
    /// 漏一条就是「只打 `\` 不弹，再敲一个字删掉才弹」。
    pub(crate) fn refresh_formula_draft_completion(state: &mut FormulaEditorState) {
        let caret = state.selected_range.start.min(state.draft.len());
        state.completion = match latex_command_before_cursor(&state.draft[..caret]) {
            Some((backslash, query)) => {
                let results = latex_completions_for(query, COMPLETION_LIMIT);
                (!results.is_empty()).then(|| FormulaDraftCompletion {
                    anchor: backslash,
                    selected: 0,
                    results,
                })
            }
            None => None,
        };
    }

    /// 确认补全：用选中项替换 `\查询串`，光标按模板落点摆好。
    pub(crate) fn confirm_formula_draft_completion(
        &mut self,
        selected: usize,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.formula_editor.as_ref() else {
            return;
        };
        let Some(completion) = state.completion.as_ref() else {
            return;
        };
        let Some(entry) = completion.results.get(selected) else {
            return;
        };
        let anchor = completion.anchor;
        let caret = state.selected_range.start.min(state.draft.len());
        let end = caret.max(anchor);
        self.replace_formula_draft(anchor..end, entry.insert, Some(entry.caret..entry.caret), false, cx);
    }

    /// 符号面板点一格：模板写进草稿光标处，光标按落点摆好，弹窗保持打开。
    pub(crate) fn insert_formula_symbol(
        &mut self,
        entry: &'static LatexSymbol,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.formula_editor.as_ref() else {
            return;
        };
        let start = state.selected_range.start.min(state.draft.len());
        let end = state.selected_range.end.min(state.draft.len()).max(start);
        let caret = entry.caret;
        self.replace_formula_draft(
            start..end,
            entry.insert,
            Some(caret..caret),
            false,
            cx,
        );
    }

    /// 「应用」：把草稿写回目标数学块。写法保真——原块是单行 `$$…$$` 且
    /// 草稿不含换行时保持单行，否则用多行形式；一次不可合并的 undo 组。
    pub(crate) fn apply_formula_editor(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.formula_editor.take() else {
            return;
        };
        let draft = state.draft.clone();
        let target = state.target;
        let Some(block) = self.document.block_entity_by_id(target) else {
            self.restore_focus_after_overlay(cx);
            cx.notify();
            return;
        };
        let raw = block.read(cx).display_text().to_string();
        let single_line = !raw.contains('\n');
        let new_text = if single_line && !draft.contains('\n') {
            format!("$${draft}$$")
        } else {
            format!("$$\n{}\n$$", draft.trim())
        };
        block.update(cx, |block, block_cx| {
            block.prepare_undo_capture(
                crate::components::UndoCaptureKind::NonCoalescible,
                block_cx,
            );
            let end = block.visible_len();
            block.replace_text_in_visible_range(0..end, &new_text, None, false, block_cx);
        });
        self.restore_focus_after_overlay(cx);
        cx.notify();
    }

    /// 弹窗内的专用按键（输入框元素上注册）：Esc 取消、⌘/Ctrl+Enter 应用、
    /// 普通 Enter 换行（多行公式环境常用）、退格/前向删除与方向键自管——
    /// DeleteBack 等动作绑定在块编辑器的 key context 上，焦点在弹窗草稿时
    /// 根本不派发，删除会静默失效（用户报修：草稿无法删除）。
    pub(crate) fn formula_editor_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = event.keystroke.key.as_str();
        let modifiers = event.keystroke.modifiers;
        let plain = !modifiers.control
            && !modifiers.alt
            && !modifiers.platform
            && !modifiers.function
            && !modifiers.shift;
        match key {
            "escape" => {
                cx.stop_propagation();
                // 补全开着先收补全，再按一次才关弹窗。
                if let Some(state) = self.formula_editor.as_mut()
                    && state.completion.take().is_some()
                {
                    cx.notify();
                    return;
                }
                self.close_formula_editor(cx);
            }
            "enter" => {
                cx.stop_propagation();
                if modifiers.platform || modifiers.control {
                    self.apply_formula_editor(window, cx);
                    return;
                }
                // 补全开着时 Enter 确认（shift+enter 才换行）。
                if plain
                    && let Some(selected) = self
                        .formula_editor
                        .as_ref()
                        .and_then(|state| state.completion.as_ref())
                        .map(|completion| completion.selected)
                {
                    self.confirm_formula_draft_completion(selected, cx);
                    return;
                }
                let Some(state) = self.formula_editor.as_ref() else {
                    return;
                };
                let at = state.selected_range.start.min(state.draft.len());
                let end = state.selected_range.end.min(state.draft.len()).max(at);
                self.replace_formula_draft(at..end, "\n", None, false, cx);
            }
            "tab" => {
                if let Some(selected) = self
                    .formula_editor
                    .as_ref()
                    .and_then(|state| state.completion.as_ref())
                    .map(|completion| completion.selected)
                {
                    cx.stop_propagation();
                    self.confirm_formula_draft_completion(selected, cx);
                }
            }
            "up" | "down" => {
                let delta = if key == "up" { -1i32 } else { 1 };
                if let Some(state) = self.formula_editor.as_mut() {
                    if let Some(completion) = state.completion.as_mut()
                        && !completion.results.is_empty()
                    {
                        let len = completion.results.len() as i32;
                        completion.selected =
                            ((completion.selected as i32 + delta).rem_euclid(len)) as usize;
                        cx.notify();
                        cx.stop_propagation();
                        return;
                    }
                }
                // 没补全时上下键留给系统（多行滚屏），不消费。
            }
            "backspace" => {
                cx.stop_propagation();
                self.delete_in_formula_draft(false, cx);
            }
            "delete" => {
                cx.stop_propagation();
                self.delete_in_formula_draft(true, cx);
            }
            "left" | "right" => {
                if let Some(state) = self.formula_editor.as_mut() {
                    let len = state.draft.len();
                    let caret = state.selected_range.start.min(len);
                    // 补全开着时左右先收掉（或者移动选择——收掉更直观）。
                    state.completion = None;
                    let offset = if key == "left" {
                        draft_prev_boundary(&state.draft, caret)
                    } else {
                        draft_next_boundary(&state.draft, caret)
                    };
                    state.selected_range = offset..offset;
                    state.marked_range = None;
                    cx.notify();
                }
                cx.stop_propagation();
            }
            _ => {}
        }
        let _ = plain;
    }

    /// 草稿内删除：退格删光标前一个字素，前向删除（Delete 键）删光标后一个。
    fn delete_in_formula_draft(&mut self, forward: bool, cx: &mut Context<Self>) {
        let Some(state) = self.formula_editor.as_ref() else {
            return;
        };
        let len = state.draft.len();
        let start = state.selected_range.start.min(len);
        let end = state.selected_range.end.min(len).max(start);
        let range = if start < end {
            start..end
        } else if forward {
            start..draft_next_boundary(&state.draft, start)
        } else {
            draft_prev_boundary(&state.draft, start)..start
        };
        if range.start == range.end {
            return;
        }
        self.replace_formula_draft(range, "", None, false, cx);
    }

    /// 弹窗浮层：遮罩 + 居中卡片。预览、草稿输入、符号面板三段。
    pub(crate) fn render_formula_editor_overlay(
        &mut self,
        theme: &Theme,
        strings: &I18nStrings,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let state = self.formula_editor.as_ref()?;
        let focus = state.focus.clone()?;
        let viewport = window.viewport_size();

        let panel_height = px(PREVIEW_HEIGHT + INPUT_HEIGHT + 300.0);
        let left = ((viewport.width - px(PANEL_WIDTH)) / 2.0)
            .max(px(PANEL_VIEWPORT_MARGIN));
        let top = ((viewport.height - panel_height) / 2.0).max(px(PANEL_VIEWPORT_MARGIN));

        // ===== 预览区 =====
        let preview_element: AnyElement = if let Some(path) = &state.preview_path {
            img(path.clone())
                .max_h(px(PREVIEW_HEIGHT - 16.0))
                .max_w(px(PANEL_WIDTH - 48.0))
                .object_fit(ObjectFit::Contain)
                .into_any_element()
        } else if let Some(error) = &state.preview_error {
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .items_center()
                .child(
                    div()
                        .font_family(crate::config::EditorSettings::fonts(cx).code_family)
                        .text_size(px(t.text_size * 0.8))
                        .text_color(c.text_default)
                        .child(state.draft.clone()),
                )
                .child(
                    div()
                        .text_size(px(t.text_size * 0.72))
                        .text_color(c.callout_caution_border)
                        .child(error.clone()),
                )
                .into_any_element()
        } else {
            div()
                .text_size(px(t.text_size * 0.85))
                .text_color(c.text_placeholder)
                .child(strings.formula_editor_preview_empty.clone())
                .into_any_element()
        };

        // ===== 草稿输入区（多行 + 光标） =====
        let draft = state.draft.clone();
        let selection_start = state.selected_range.start.min(draft.len());
        let code_family = crate::config::EditorSettings::fonts(cx).code_family;
        let input_font = font(code_family.clone());
        let input_font_size = px(13.0);
        let input_color = c.text_default;
        let line_height = px(INPUT_LINE_HEIGHT);

        // 光标位置：行下标 + 行内前缀宽（等宽也用 shape 量，稳妥）。
        let caret_line = draft[..selection_start].matches('\n').count();
        let line_start = draft[..selection_start]
            .rfind('\n')
            .map(|index| index + 1)
            .unwrap_or(0);
        let caret_prefix = &draft[line_start..selection_start];
        let caret_x = window
            .text_system()
            .shape_line(
                SharedString::from(caret_prefix.to_string()),
                input_font_size,
                &[TextRun {
                    len: caret_prefix.len(),
                    font: input_font.clone(),
                    color: input_color,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                    font_size: None,
                }],
                None,
            )
            .width;
        // 光标条与文本行框同高同位：字在半行距里居中（GPUI 的 paint_line 用
        // (line_height - ascent - descent)/2 定基线），条子贴行框顶就会比字高出一截。
        let caret_div = div()
            .debug_selector(|| "formula-editor-caret".to_string())
            .absolute()
            .left(caret_x + px(INPUT_PADDING_X))
            .top(px(INPUT_PADDING_Y) + line_height * caret_line as f32)
            .w(px(2.0))
            .h(line_height)
            .bg(c.cursor);

        let draft_lines: Vec<AnyElement> = formula_draft_lines(&draft, c, input_font.clone())
            .into_iter()
            .enumerate()
            .map(|(line_index, line)| {
                div()
                    .h(line_height)
                    .whitespace_normal()
                    .debug_selector(move || format!("formula-editor-line-{line_index}"))
                    .child(StyledText::new(line.text).with_runs(line.runs))
                    .into_any_element()
            })
            .collect();

        let input_element = div()
            .id("formula-editor-input")
            .debug_selector(|| "formula-editor-input".to_string())
            .track_focus(&focus)
            .relative()
            .w_full()
            .h(px(INPUT_HEIGHT))
            .rounded(px(6.0))
            .border_1()
            .border_color(c.dialog_border)
            .bg(c.editor_background)
            .font_family(code_family.clone())
            .text_size(input_font_size)
            .line_height(line_height)
            .text_color(input_color)
            .overflow_y_scroll()
            .scrollbar_width(px(0.0))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .px(px(INPUT_PADDING_X))
                    .py(px(INPUT_PADDING_Y))
                    .relative()
                    .w_full()
                    .children(draft_lines)
                    .child(caret_div),
            )
            .child(
                canvas(|_, _, _| (), {
                    let focus = focus.clone();
                    let input_editor = cx.entity();
                    move |bounds, _, window, cx| {
                        window.handle_input(
                            &focus,
                            ElementInputHandler::new(bounds, input_editor.clone()),
                            cx,
                        );
                    }
                })
                .absolute()
                .top_0()
                .right_0()
                .bottom_0()
                .left_0(),
            )
            .on_key_down({
                let editor_handle = cx.entity().downgrade();
                move |event: &KeyDownEvent, window, cx| {
                    let _ = editor_handle.update(cx, |editor, cx| {
                        editor.formula_editor_key_down(event, window, cx);
                    });
                }
            });

        // ===== 符号面板 =====
        let category = state.category;
        let tabs: Vec<AnyElement> = CATEGORIES
            .iter()
            .map(|&tab| {
                let is_active = tab == category;
                let tab = div()
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
                        c.dialog_body
                    })
                    .bg(if is_active {
                        c.dialog_primary_button_bg
                    } else {
                        hsla(0.0, 0.0, 0.0, 0.0)
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |editor, _event: &MouseDownEvent, _window, cx| {
                            if let Some(state) = editor.formula_editor.as_mut() {
                                state.category = tab;
                                cx.notify();
                            }
                        }),
                    )
                    .child(category_label(tab, strings));
                // 选中的页签不挂 hover 变浅——浮层上 hover 色与选中底色相近
                // 时，选中态会一晃就没（用户报修）。
                if is_active {
                    tab.into_any_element()
                } else {
                    tab.hover(|this| this.bg(c.dialog_secondary_button_hover))
                        .into_any_element()
                }
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
                // 列宽交给 grid 均分（w_full），行高固定——内容宽度不齐时
                // 格子也不会七扭八歪（用户报修：函数分类排版乱）。
                .h(px(CELL))
                .w_full()
                .flex()
                .items_center()
                .justify_center()
                .overflow_hidden()
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
                        cx.stop_propagation();
                        editor.insert_formula_symbol(entry, cx);
                    }),
                );
            let content: AnyElement = match crate::components::latex::render_inline_math_svg(
                entry.preview,
                preview_color,
                preview_size,
            ) {
                Ok(rendered) => img(rendered.path)
                    .flex_shrink_0()
                    .max_h(px(CELL * 0.66))
                    .max_w(px(CELL * 0.92))
                    .object_fit(ObjectFit::Contain)
                    .into_any_element(),
                Err(_) => div()
                    .text_size(px(t.text_size * 0.72))
                    .text_color(c.dialog_muted)
                    .truncate()
                    .child(format!("\\{}", entry.name))
                    .into_any_element(),
            };
            cells.push(cell.child(content).into_any_element());
        }

        let grid_width = px(PANEL_WIDTH - 2.0 * PANEL_PADDING);

        // 草稿的 \ 命令补全浮层：锚在光标正下方，宽度只够放命令名与预览。
        // 光标位置由面板常量 + 输入区排布换算成面板内坐标（与 caret_div 同源）。
        let completion_element: AnyElement = match state
            .completion
            .as_ref()
            .filter(|completion| !completion.results.is_empty())
        {
            Some(completion) => {
                let selected = completion.selected;
                let rows: Vec<AnyElement> = completion
                    .results
                    .iter()
                    .enumerate()
                    .map(|(index, entry)| {
                        let is_selected = index == selected;
                        let confirm = cx.listener(move |editor, _e: &MouseDownEvent, _w, cx| {
                            cx.stop_propagation();
                            editor.confirm_formula_draft_completion(index, cx);
                        });
                        let preview: AnyElement = match crate::components::latex::render_inline_math_svg(
                            entry.preview,
                            preview_color,
                            preview_size,
                        ) {
                            Ok(rendered) => img(rendered.path)
                                .max_h(px(20.0))
                                .max_w(px(72.0))
                                .object_fit(ObjectFit::Contain)
                                .into_any_element(),
                            Err(_) => div()
                                .text_size(px(t.text_size * 0.72))
                                .text_color(c.dialog_muted)
                                .child(format!("\\{}", entry.name))
                                .into_any_element(),
                        };
                        div()
                            .id(ElementId::Name(format!("formula-completion-{index}").into()))
                            .h(px(COMPLETION_ROW_HEIGHT))
                            .w_full()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
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
                                    .min_w(px(96.0))
                                    .font_family(code_family.clone())
                                    .text_size(px(t.text_size * 0.8))
                                    .text_color(c.text_default)
                                    .child(format!("\\{}", entry.name)),
                            )
                            .child(
                                div().flex().items_center().justify_end().flex_1().child(preview),
                            )
                            .into_any_element()
                    })
                    .collect();
                let row_count = completion.results.len().max(1);
                let popup_width = px(COMPLETION_WIDTH);
                let popup_height = px(
                    8.0 + COMPLETION_ROW_HEIGHT * row_count as f32
                        + 2.0 * (row_count as f32 - 1.0),
                );
                // 光标在面板内的落点：与 caret_div 用同一套常量换算，两处才不会各说各话。
                let caret_left = px(PANEL_PADDING + INPUT_BORDER + INPUT_PADDING_X) + caret_x;
                let caret_bottom = px(INPUT_TOP
                    + INPUT_BORDER
                    + INPUT_PADDING_Y
                    + (caret_line as f32 + 1.0) * INPUT_LINE_HEIGHT);
                let popup_left = caret_left
                    .min(px(PANEL_WIDTH - PANEL_PADDING) - popup_width)
                    .max(px(PANEL_PADDING));
                let popup_top = (caret_bottom + px(2.0))
                    .min(panel_height - popup_height - px(PANEL_PADDING))
                    .max(px(PANEL_PADDING));
                div()
                    .id("formula-editor-completion")
                    .debug_selector(|| "formula-editor-completion".to_string())
                    .absolute()
                    .left(popup_left)
                    .top(popup_top)
                    .w(popup_width)
                    .bg(c.dialog_surface)
                    .border_1()
                    .border_color(c.dialog_border)
                    .rounded(px(6.0))
                    .shadow_lg()
                    .p(px(4.0))
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .occlude()
                    .children(rows)
                    .into_any_element()
            }
            None => div().into_any_element(),
        };

        // ===== 组装 =====
        Some(
            div()
                .id("formula-editor-overlay")
                .debug_selector(|| "formula-editor-overlay".to_string())
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .bottom_0()
                .occlude()
                .bg(c.dialog_backdrop)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|editor, _event: &MouseDownEvent, _window, cx| {
                        editor.close_formula_editor(cx);
                    }),
                )
                .child(
                    div()
                        .id("formula-editor-panel")
                        .debug_selector(|| "formula-editor-panel".to_string())
                        .absolute()
                        .left(left)
                        .top(top)
                        .w(px(PANEL_WIDTH))
                        .flex()
                        .flex_col()
                        .gap(px(PANEL_GAP))
                        .p(px(PANEL_PADDING))
                        .bg(c.dialog_surface)
                        .border(px(d.dialog_border_width))
                        .border_color(c.dialog_border)
                        .rounded(px(d.dialog_radius))
                        .shadow_lg()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(
                            div()
                                .h(px(TITLE_HEIGHT))
                                .flex()
                                .items_center()
                                .child(
                                    div()
                                        .text_size(px(t.dialog_title_size * 0.8))
                                        .font_weight(t.dialog_title_weight.to_font_weight())
                                        .text_color(c.dialog_title)
                                        .child(strings.insert_formula.clone()),
                                )
                                .child(div().flex_1())
                                .child(
                                    div()
                                        .id("formula-editor-close")
                                        .size(px(24.0))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .rounded(px(5.0))
                                        .cursor_pointer()
                                        .text_size(px(t.text_size * 0.9))
                                        .text_color(c.dialog_muted)
                                        .hover(|this| this.bg(c.dialog_secondary_button_hover))
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(
                                                |editor, _e: &MouseDownEvent, _w, cx| {
                                                    editor.close_formula_editor(cx);
                                                },
                                            ),
                                        )
                                        .child("×"),
                                ),
                        )
                        .child(
                            div()
                                .h(px(PREVIEW_HEIGHT))
                                .w_full()
                                .rounded(px(6.0))
                                .bg(c.editor_background)
                                .border_1()
                                .border_color(c.dialog_border)
                                .flex()
                                .items_center()
                                .justify_center()
                                .overflow_hidden()
                                .child(preview_element),
                        )
                        .child(input_element)
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .gap(px(4.0))
                                .children(tabs),
                        )
                        .child(
                            div()
                                .id("formula-editor-grid")
                                .w(grid_width)
                                .max_h(px(160.0))
                                .overflow_y_scroll()
                                .scrollbar_width(px(4.0))
                                .grid()
                                .grid_cols(GRID_COLS)
                                .gap(px(CELL_GAP))
                                .children(cells),
                        )

                        .child(completion_element)
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.0))
                                .child(
                                    div()
                                        .flex_1()
                                        .text_size(px(t.text_size * 0.72))
                                        .text_color(c.dialog_muted)
                                        .child(strings.formula_editor_hint.clone()),
                                )
                                .child(
                                    div()
                                        .id("formula-editor-cancel")
                                        .h(px(d.dialog_button_height))
                                        .px(px(d.dialog_button_padding_x))
                                        .flex()
                                        .items_center()
                                        .rounded(px(6.0))
                                        .border_1()
                                        .border_color(c.dialog_border)
                                        .bg(c.dialog_secondary_button_bg)
                                        .hover(|this| this.bg(c.dialog_secondary_button_hover))
                                        .cursor_pointer()
                                        .text_size(px(t.dialog_button_size))
                                        .text_color(c.dialog_secondary_button_text)
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(
                                                |editor, _e: &MouseDownEvent, _w, cx| {
                                                    editor.close_formula_editor(cx);
                                                },
                                            ),
                                        )
                                        .child(strings.formula_editor_cancel.clone()),
                                )
                                .child(
                                    div()
                                        .id("formula-editor-apply")
                                        .debug_selector(|| {
                                            "formula-editor-apply".to_string()
                                        })
                                        .h(px(d.dialog_button_height))
                                        .px(px(d.dialog_button_padding_x))
                                        .flex()
                                        .items_center()
                                        .rounded(px(6.0))
                                        .bg(c.dialog_primary_button_bg)
                                        .hover(|this| this.bg(c.dialog_primary_button_hover))
                                        .cursor_pointer()
                                        .text_size(px(t.dialog_button_size))
                                        .text_color(c.dialog_primary_button_text)
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(
                                                |editor, _e: &MouseDownEvent, window, cx| {
                                                    editor.apply_formula_editor(window, cx);
                                                },
                                            ),
                                        )
                                        .child(strings.formula_editor_apply.clone()),
                                ),
                        ),
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
