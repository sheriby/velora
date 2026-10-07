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
use std::time::{Duration, Instant};

use gpui::prelude::FluentBuilder;
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
/// 一视觉行的行框高；草稿字号 `INPUT_FONT_SIZE` 与它一起决定换行后的行位置。
pub(crate) const INPUT_LINE_HEIGHT: f32 = 21.0;
/// 面板内边距、条目间距、标题行高、输入框描边与内边距：补全浮层要按这套
/// 常量把光标位置换算成面板内坐标，任何一处改动都得跟着对。
const PANEL_PADDING: f32 = 14.0;
const PANEL_GAP: f32 = 8.0;
const TITLE_HEIGHT: f32 = 26.0;
const INPUT_BORDER: f32 = 1.0;
const INPUT_PADDING_X: f32 = 10.0;
/// 内容原点相对输入框外缘的上内缩（描边 + 内边距）：指针命中与光标定位共用。
pub(crate) const INPUT_PADDING_Y: f32 = 8.0;
/// 文本起点相对输入框外缘的左内缩（描边 + 内边距）：光标、选区色块、鼠标
/// 命中三处都要用同一个值。
pub(crate) const INPUT_CONTENT_INSET_X: f32 = INPUT_BORDER + INPUT_PADDING_X;
/// 草稿字号，与 `INPUT_LINE_HEIGHT` 一起决定行框；测试按它量字宽。
pub(crate) const INPUT_FONT_SIZE: f32 = 13.0;
/// 草稿输入区在面板内的纵向起点（标题 + 预览 + 两道间距）。
const INPUT_TOP: f32 = PANEL_PADDING + TITLE_HEIGHT + PANEL_GAP + PREVIEW_HEIGHT + PANEL_GAP;
/// 补全浮层：贴着光标的小列表，不铺满面板宽。
const COMPLETION_WIDTH: f32 = 240.0;
const COMPLETION_ROW_HEIGHT: f32 = 28.0;
/// 草稿文本的可用宽度：面板宽扣掉两侧内边距、输入框描边与内边距。软换行按
/// 这个宽度 shape，行 div 也按这个宽度定宽——两边宽度不一致时 GPUI 的换行点
/// 与算光标用的换行点就不是同一套。
const DRAFT_CONTENT_WIDTH: f32 =
    PANEL_WIDTH - 2.0 * PANEL_PADDING - 2.0 * INPUT_BORDER - 2.0 * INPUT_PADDING_X;

/// 分类页签的固定次序（与符号表的组织一致）。
const CATEGORIES: [LatexCategory; 6] = [
    LatexCategory::Structures,
    LatexCategory::Greek,
    LatexCategory::Operators,
    LatexCategory::Arrows,
    LatexCategory::Functions,
    LatexCategory::Symbols,
];

/// 草稿撤销栈的深度上限（一次弹窗生命周期内的编辑次数，再多就丢最早的）。
const DRAFT_UNDO_LIMIT: usize = 200;

/// 这次编辑算不算「光标处的单字符增删」：连着几次要合成一步撤销，否则打十个
/// 字要按十次 ⌘Z 才退得掉。
pub(crate) fn draft_edit_is_single_character(old: &str, range: &Range<usize>, new_text: &str) -> bool {
    if range.start == range.end {
        return new_text.chars().count() == 1;
    }
    new_text.is_empty() && draft_prev_boundary(old, range.end) == range.start
}

/// 编辑前把当前状态压进撤销栈。撤销栈存的是「编辑前」的样子，所以每段新编辑
/// 都要先留一份底；重做栈在编辑时清空（标准文本框口径）。
pub(crate) fn push_draft_undo(state: &mut FormulaEditorState, coalescible: bool) {
    if coalescible && state.last_edit_coalesced {
        return;
    }
    state.draft_undo.push((state.draft.clone(), state.selected_range.clone()));
    while state.draft_undo.len() > DRAFT_UNDO_LIMIT {
        state.draft_undo.remove(0);
    }
    state.draft_redo.clear();
    state.last_edit_coalesced = coalescible;
}

/// 草稿一行的渲染输入：行文本 + 铺满该行的样式段。
pub(crate) struct FormulaDraftLine {
    pub(crate) text: SharedString,
    pub(crate) runs: Vec<TextRun>,
}

/// 把草稿按行切成带 LaTeX 语法色的渲染段。配色与数学块编辑态同源（同一套
/// `highlight_latex_source` + `code_highlight_color`）：弹窗里改的就是那几行
/// 源码，两处颜色不一样会让人觉得不是同一个公式（用户报修：草稿没颜色）。
/// `selection` 是草稿坐标下的选区，落在哪段 run 上就给哪段加选中底色。
pub(crate) fn formula_draft_lines(
    draft: &str,
    colors: &ThemeColors,
    font: Font,
    selection: Range<usize>,
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
            selection.clone(),
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
    selection: Range<usize>,
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
            text: SharedString::from(" "),
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
    let sel_start = selection.start.saturating_sub(line_start).min(line.len());
    let sel_end = selection.end.saturating_sub(line_start).max(sel_start).min(line.len());
    apply_draft_selection_background(&mut runs, sel_start..sel_end, colors.selection);
    FormulaDraftLine {
        text: SharedString::from(line.to_string()),
        runs,
    }
}

/// 把选区落到该行的 run 上：在选区边界处切开 run，选中段加底色。选区画在
/// run 上而不是另铺色块，软换行时 GPUI 会按行裁开背景（`paint_line_background`
/// 在换行边界处断笔），手铺的色块对不上换行后的位置。
fn apply_draft_selection_background(
    runs: &mut Vec<TextRun>,
    selection: Range<usize>,
    background: Hsla,
) {
    if selection.start >= selection.end {
        return;
    }
    let mut offset = 0usize;
    let mut split: Vec<TextRun> = Vec::with_capacity(runs.len() + 2);
    for run in runs.drain(..) {
        let start = offset;
        let end = start + run.len;
        offset = end;
        let head_end = selection.start.clamp(start, end);
        let tail_start = selection.end.clamp(start, end);
        let mut slice = |from: usize, to: usize, highlighted: bool| {
            if to <= from {
                return;
            }
            let mut part = run.clone();
            part.len = to - from;
            part.background_color = highlighted.then_some(background);
            split.push(part);
        };
        slice(start, head_end, false);
        slice(head_end, tail_start, true);
        slice(tail_start, end, false);
    }
    *runs = split;
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
    /// 选区锚点：拖动与 shift 扩展时固定这一端，另一端跟着指针/光标走。
    pub(crate) selection_anchor: usize,
    /// 左键在输入区按下到抬手之间为真：只有这期间 mouse_move 才改选区。
    pub(crate) selecting_with_mouse: bool,
    /// 光标闪烁计时起点：每次移动光标都重置，前半秒常亮。
    pub(crate) caret_epoch: Instant,
    /// 闪烁的当前相位。任务只在相位真的翻转时才 notify，避免弹窗开着时
    /// 按 33ms 重排整篇文档。
    pub(crate) caret_visible: bool,
    /// 上下键要保住的落点（x 列 + y 行）。横向移动、编辑、点击清掉它，纵向
    /// 移动时用它当基准——换行边界那个偏移在 gpui 的两套换算里分属相邻两行，
    /// 只按当前位置算会在行界上原地打转。
    pub(crate) caret_preferred: Option<Point<Pixels>>,
    /// 上一次「把光标滚进可见区」时的 caret_epoch。光标每动一次才允许自动滚
    /// 一次，否则会把用户手动的滚动一直抢回去。
    pub(crate) last_autoscroll_epoch: Option<Instant>,
    /// 草稿自己的撤销栈（文本 + 选区）：弹窗是草稿现场，块编辑器的 undo 管不到
    /// 这里，也没有别的地方能给草稿做撤销。
    pub(crate) draft_undo: Vec<(String, Range<usize>)>,
    pub(crate) draft_redo: Vec<(String, Range<usize>)>,
    /// 上一次编辑是不是「光标处的单字符增删」——连打一串字要合成一步撤销。
    pub(crate) last_edit_coalesced: bool,
    /// 闪烁重绘任务。挂在 state 上，弹窗关闭（state 被 take 走）时随 Task
    /// drop 一起取消。
    pub(crate) caret_blink_task: Option<Task<()>>,
    /// 输入区滚动句柄：鼠标点换算成草稿偏移要用它的原点与滚动偏移。
    pub(crate) input_scroll: ScrollHandle,
    pub(crate) focus: Option<FocusHandle>,
    pub(crate) category: LatexCategory,
    /// 草稿的实时预览：ratex 渲染好的缓存 SVG。渲染失败时为 Err（界面上
    /// 显示 LaTeX 源码 + 错误），空草稿为 None（显示占位提示）。
    pub(crate) preview_path: Option<PathBuf>,
    pub(crate) preview_error: Option<String>,
    /// 草稿里打 `\` 弹出的命令补全：锚在反斜杠上，随编辑刷新。
    pub(crate) completion: Option<FormulaDraftCompletion>,
}

/// 光标是否可见：移动后前半秒常亮，之后每半秒开关一次。
/// 不做块编辑器那种逐帧余弦淡入淡出——弹窗开着时一次 notify 重排的是整篇
/// 文档，按 33ms 刷不划算，两态闪烁每秒只重绘两次。
fn draft_caret_visible(epoch: Instant) -> bool {
    let elapsed = epoch.elapsed().as_secs_f32();
    elapsed < 0.5 || (elapsed * 2.0) as u32 % 2 == 0
}

/// 把偏移夹到字符边界（鼠标点的是一串字节，落在多字节字符中间不能直接用）。
fn draft_clamp_boundary(text: &str, offset: usize) -> usize {
    let mut offset = offset.min(text.len());
    while offset > 0 && !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

/// 双选取词：偏移所在的那段连续字母/数字（落在非词字符上就是不选）。
fn draft_word_range(draft: &str, offset: usize) -> Range<usize> {
    let offset = draft_clamp_boundary(draft, offset);
    let bytes = draft.as_bytes();
    let is_word = |byte: u8| byte.is_ascii_alphanumeric();
    let mut start = offset;
    while start > 0 && is_word(bytes[start - 1]) {
        start -= 1;
    }
    let mut end = offset;
    while end < bytes.len() && is_word(bytes[end]) {
        end += 1;
    }
    start..end
}

/// 选区夹到草稿内的字符边界上；空选区返回 None（复制/剪切没内容可给）。
fn draft_selected_range(draft: &str, range: Range<usize>) -> Option<Range<usize>> {
    let start = draft_clamp_boundary(draft, range.start);
    let end = draft_clamp_boundary(draft, range.end).max(start);
    (start < end).then_some(start..end)
}

/// 选区里的文本；空选区没有内容。
fn draft_selected_text(draft: &str, range: Range<usize>) -> Option<String> {
    draft_selected_range(draft, range).map(|range| draft[range].to_string())
}

/// 一硬行的换行布局：软换行后的行高、光标落点与指针命中都从这里问。
/// 渲染侧的行框高度必须用同一个 `height`，否则长行软换行后光标会跑到别的行上。
pub(crate) struct DraftLineLayout {
    /// 该硬行在草稿里的字节区间（不含换行符）。
    pub(crate) range: Range<usize>,
    /// 渲染用的行文本（空行是兜底空格）。
    pub(crate) text: SharedString,
    /// 带语法色与选区底色的样式段。
    pub(crate) runs: Vec<TextRun>,
    /// shape_text 出来的条目（内含软换行边界）。
    pub(crate) wrapped: WrappedLine,
    /// 该硬行占的像素高（软换行数 × line_height）。
    pub(crate) height: Pixels,
    /// 该硬行顶部相对内容原点的 y。
    pub(crate) top: Pixels,
}

/// 草稿每个硬行的字节区间（不含换行符）；与 `split('\n')` 同序同数。
fn draft_line_ranges(draft: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut start = 0usize;
    loop {
        match draft[start..].find('\n') {
            Some(offset) => {
                ranges.push(start..start + offset);
                start += offset + 1;
            }
            None => {
                ranges.push(start..draft.len());
                return ranges;
            }
        }
    }
}

/// 当前活动端：有选区时是「不是锚点的那一端」，收起时就是光标。
fn draft_active_end(state: &FormulaEditorState) -> usize {
    let start = state.selected_range.start;
    let end = state.selected_range.end;
    if start == end {
        return end;
    }
    if state.selection_anchor == start {
        end
    } else {
        start
    }
}

/// 把偏移夹进草稿并落到字符边界上，再定位它所在的硬行。
fn draft_clamp_offset(draft: &str, offset: usize) -> usize {
    draft_clamp_boundary(draft, offset.min(draft.len()))
}

/// 内容坐标（输入区文本原点为 0,0）里的光标/选区端点位置。软换行时 y 会落在
/// 第二、第三行上，这正是以前按「一硬行一行」算错的地方。
fn draft_point_for_offset(
    layouts: &[DraftLineLayout],
    offset: usize,
    line_height: Pixels,
) -> Option<Point<Pixels>> {
    for layout in layouts {
        if offset > layout.range.end {
            continue;
        }
        let index = offset.saturating_sub(layout.range.start).min(layout.range.len());
        // 换行边界那个偏移按 gpui 的口径算给上一行行尾：End 之后光标停在行尾
        // 才是直觉里的位置。上下键不依赖这里的 y，用的是 `caret_preferred`。
        let position = layout
            .wrapped
            .position_for_index(index, line_height)
            .unwrap_or_default();
        return Some(point(position.x, layout.top + position.y));
    }
    layouts
        .last()
        .map(|layout| point(px(0.0), layout.top + layout.height - line_height))
}

/// 内容坐标里的指针位置 → 草稿偏移（软换行后按视觉行找）。
fn draft_offset_for_content_point(
    draft: &str,
    layouts: &[DraftLineLayout],
    position: Point<Pixels>,
    line_height: Pixels,
) -> usize {
    let Some(last) = layouts.last() else {
        return draft.len();
    };
    // 指针落在哪一硬行：超出末行就归末行，早于首行归首行（find 天然满足）。
    let layout = layouts
        .iter()
        .find(|layout| position.y < layout.top + layout.height)
        .unwrap_or(last);
    let y_in_line = (position.y - layout.top)
        .clamp(px(0.0), (layout.height - line_height).max(px(0.0)));
    let index = match layout
        .wrapped
        .closest_index_for_position(point(position.x, y_in_line), line_height)
    {
        Ok(index) => index,
        Err(index) => index,
    };
    draft_clamp_offset(draft, layout.range.start + index.min(layout.range.len()))
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
            selection_anchor: draft.len(),
            selecting_with_mouse: false,
            caret_epoch: Instant::now(),
            caret_visible: true,
            caret_preferred: None,
            last_autoscroll_epoch: None,
            draft_undo: Vec::new(),
            draft_redo: Vec::new(),
            last_edit_coalesced: false,
            caret_blink_task: None,
            input_scroll: ScrollHandle::new(),
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
        let coalescible = draft_edit_is_single_character(&old, &(start..end), new_text);
        let mut updated = old.clone();
        updated.replace_range(start..end, new_text);
        if updated != old {
            push_draft_undo(state, coalescible);
        }
        let inserted_end = start + new_text.len();
        let selection = selected_in_inserted
            .map(|selection| {
                start + selection.start.min(new_text.len())
                    ..start + selection.end.min(new_text.len())
            })
            .unwrap_or(inserted_end..inserted_end);
        state.draft = updated;
        state.selected_range = selection;
        // 编辑后选区收起：锚点跟着光标走，下一次 shift 扩展从这里长出去。
        state.selection_anchor = state.selected_range.start;
        state.caret_epoch = Instant::now();
        // 文本变了，之前记住的列不再有意义。
        state.caret_preferred = None;
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
        // 平台主修饰键：macOS 是 ⌘，其余是 Ctrl（与搜索框那套自管快捷键同口径）。
        let secondary = modifiers.platform || modifiers.control;
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
                let completion_open = self
                    .formula_editor
                    .as_ref()
                    .and_then(|state| state.completion.as_ref())
                    .is_some_and(|completion| !completion.results.is_empty());
                if completion_open {
                    if let Some(state) = self.formula_editor.as_mut()
                        && let Some(completion) = state.completion.as_mut()
                    {
                        let len = completion.results.len() as i32;
                        completion.selected =
                            ((completion.selected as i32 + delta).rem_euclid(len)) as usize;
                    }
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                // 没补全时上下键走视觉行（软换行的长行也算两行）：列与行都以
                // 记住的落点为基准，换行边界那个偏移才不会原地打转。
                let layouts = self.measure_formula_draft(window, cx);
                let line_height = px(INPUT_LINE_HEIGHT);
                let moved = self.formula_editor.as_ref().map(|state| {
                    let active = draft_active_end(state);
                    let current = draft_point_for_offset(&layouts, active, line_height)
                        .unwrap_or(point(px(0.0), px(0.0)));
                    let preferred = state.caret_preferred.unwrap_or(current);
                    let content_height = layouts
                        .last()
                        .map(|layout| layout.top + layout.height)
                        .unwrap_or(line_height);
                    let target_y = (preferred.y + px(delta as f32 * INPUT_LINE_HEIGHT))
                        .clamp(px(0.0), (content_height - line_height).max(px(0.0)));
                    let offset = draft_offset_for_content_point(
                        &state.draft,
                        &layouts,
                        point(preferred.x, target_y),
                        line_height,
                    );
                    (offset, point(preferred.x, target_y))
                });
                let Some((offset, preferred_point)) = moved else {
                    return;
                };
                self.move_formula_caret(offset, modifiers.shift, Some(preferred_point), cx);
                cx.stop_propagation();
            }
            "backspace" => {
                cx.stop_propagation();
                self.delete_in_formula_draft(false, cx);
            }
            "delete" => {
                cx.stop_propagation();
                self.delete_in_formula_draft(true, cx);
            }
            "home" | "end" | "left" | "right" => {
                let to_start = matches!(key, "home" | "left");
                // 补全开着时先收掉（左右改成移动光标，比移动候选更直观）。
                if let Some(state) = self.formula_editor.as_mut() {
                    state.completion = None;
                }
                if secondary {
                    // ⌘←/⌘→ 与 ⌘Home/⌘End 同义：跳到草稿首/尾。
                    let draft_len = self
                        .formula_editor
                        .as_ref()
                        .map(|state| state.draft.len())
                        .unwrap_or_default();
                    self.move_formula_caret(
                        if to_start { 0 } else { draft_len },
                        modifiers.shift,
                        None,
                        cx,
                    );
                    cx.stop_propagation();
                    return;
                }
                if key == "home" || key == "end" {
                    // 行首/行尾按视觉行算，软换行的长行才不会一下跳到整条硬行末尾。
                    let layouts = self.measure_formula_draft(window, cx);
                    let line_height = px(INPUT_LINE_HEIGHT);
                    let target = self.formula_editor.as_ref().map(|state| {
                        let active = draft_active_end(state);
                        let current = draft_point_for_offset(&layouts, active, line_height)
                            .unwrap_or(point(px(0.0), px(0.0)));
                        let x = if key == "home" {
                            px(0.0)
                        } else {
                            px(DRAFT_CONTENT_WIDTH)
                        };
                        draft_offset_for_content_point(
                            &state.draft,
                            &layouts,
                            point(x, current.y),
                            line_height,
                        )
                    });
                    let Some(offset) = target else {
                        return;
                    };
                    self.move_formula_caret(offset, modifiers.shift, None, cx);
                    cx.stop_propagation();
                    return;
                }
                // 左右走一个字素；有选区且没按 shift 时先收起（左落头、右落尾）。
                let stepped = self.formula_editor.as_ref().map(|state| {
                    let range = state.selected_range.clone();
                    if range.start != range.end && !modifiers.shift {
                        return if to_start { range.start } else { range.end };
                    }
                    let active = draft_active_end(state);
                    if to_start {
                        draft_prev_boundary(&state.draft, active)
                    } else {
                        draft_next_boundary(&state.draft, active)
                    }
                });
                let Some(offset) = stepped else {
                    return;
                };
                self.move_formula_caret(offset, modifiers.shift, None, cx);
                cx.stop_propagation();
            }
            // 全选 / 复制 / 剪切 / 粘贴：草稿没有 key context，块编辑器那套
            // BlockEditor 绑定的动作到不了这里，只能在元素级按键里自管
            // （与搜索框 render_search.rs 同口径）。
            "a" if secondary && !modifiers.shift => {
                if let Some(state) = self.formula_editor.as_mut() {
                    let draft_len = state.draft.len();
                    state.selected_range = 0..draft_len;
                    state.selection_anchor = 0;
                    state.completion = None;
                    state.caret_epoch = Instant::now();
                    cx.notify();
                }
                cx.stop_propagation();
            }
            "c" if secondary => self.copy_formula_draft_selection(cx),
            "x" if secondary => self.cut_formula_draft_selection(cx),
            "v" if secondary => self.paste_formula_draft_clipboard(cx),
            // 撤销 / 重做：草稿的编辑历史在弹窗自己的栈上（块编辑器的 undo 管不到
            // 没写回块的草稿）。
            "z" if secondary => {
                cx.stop_propagation();
                if modifiers.shift {
                    self.redo_formula_draft(cx);
                } else {
                    self.undo_formula_draft(cx);
                }
            }
            "y" if secondary => {
                cx.stop_propagation();
                self.redo_formula_draft(cx);
            }
            _ => {}
        }
        let _ = plain;
    }

    /// ⌘Z：撤销草稿的一步编辑。
    pub(crate) fn undo_formula_draft(&mut self, cx: &mut Context<Self>) {
        self.restore_formula_draft_step(true, cx);
    }

    /// ⌘⇧Z / ⌘Y：重做被撤销的编辑。
    pub(crate) fn redo_formula_draft(&mut self, cx: &mut Context<Self>) {
        self.restore_formula_draft_step(false, cx);
    }

    /// 在撤销/重做两个栈之间搬草稿状态。栈里存的是「编辑前」的样子，所以先把
    /// 当前状态塞进对面那个栈，再取出要恢复的一步。
    fn restore_formula_draft_step(&mut self, undo: bool, cx: &mut Context<Self>) {
        let Some(state) = self.formula_editor.as_mut() else {
            return;
        };
        let step = if undo {
            state.draft_undo.pop()
        } else {
            state.draft_redo.pop()
        };
        let Some((draft, selection)) = step else {
            return;
        };
        let current = (state.draft.clone(), state.selected_range.clone());
        if undo {
            state.draft_redo.push(current);
        } else {
            state.draft_undo.push(current);
            while state.draft_undo.len() > DRAFT_UNDO_LIMIT {
                state.draft_undo.remove(0);
            }
        }
        let start = draft_clamp_boundary(&draft, selection.start);
        let end = draft_clamp_boundary(&draft, selection.end).max(start).min(draft.len());
        state.draft = draft;
        state.selected_range = start..end;
        state.selection_anchor = start;
        state.marked_range = None;
        state.completion = None;
        state.caret_preferred = None;
        state.caret_epoch = Instant::now();
        state.last_edit_coalesced = false;
        Self::sync_formula_preview(state, cx);
        cx.notify();
    }

    /// 把光标落到 `offset`：shift 时从锚点长出选区，否则收起选区并把锚点放好。
    /// `preferred` 是上下键要保住的落点（列 + 行），横向移动与编辑传 None。
    fn move_formula_caret(
        &mut self,
        offset: usize,
        shift: bool,
        preferred: Option<Point<Pixels>>,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.formula_editor.as_mut() else {
            return;
        };
        let offset = draft_clamp_offset(&state.draft, offset);
        if shift {
            let anchor = state.selection_anchor.min(state.draft.len());
            state.selected_range = anchor.min(offset)..anchor.max(offset);
        } else {
            state.selected_range = offset..offset;
            state.selection_anchor = offset;
        }
        state.marked_range = None;
        state.caret_epoch = Instant::now();
        state.caret_preferred = preferred;
        cx.notify();
    }

    /// 复制选区：没有选区就不抢键。
    fn copy_formula_draft_selection(&mut self, cx: &mut Context<Self>) {
        let Some(text) = self
            .formula_editor
            .as_ref()
            .and_then(|state| draft_selected_text(&state.draft, state.selected_range.clone()))
        else {
            return;
        };
        cx.stop_propagation();
        cx.write_to_clipboard(ClipboardItem::new_string(text));
    }

    /// 剪切：先落剪贴板再删选区（删走统一的编辑路径，预览与补全会跟着刷）。
    fn cut_formula_draft_selection(&mut self, cx: &mut Context<Self>) {
        let Some((range, text)) = self
            .formula_editor
            .as_ref()
            .and_then(|state| {
                draft_selected_range(&state.draft, state.selected_range.clone()).map(|range| {
                    (range.clone(), state.draft[range].to_string())
                })
            })
        else {
            return;
        };
        cx.stop_propagation();
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.replace_formula_draft(range, "", None, false, cx);
    }

    /// 粘贴：剪贴板文本替换选区；没有选区就插到光标处。
    fn paste_formula_draft_clipboard(&mut self, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        let Some(range) = self.formula_editor.as_ref().map(|state| {
            let start = draft_clamp_boundary(&state.draft, state.selected_range.start);
            let end = draft_clamp_boundary(&state.draft, state.selected_range.end).max(start);
            start..end
        }) else {
            return;
        };
        cx.stop_propagation();
        self.replace_formula_draft(range, &text, None, false, cx);
    }

    /// 量出草稿当前的分行换行布局（文本、run、软换行后的行高与顶部）。
    /// 渲染、光标、补全浮层、指针命中四处共用这一份：各算各的就会在长行软换行
    /// 之后对不上（用户报修：多行光标位置不对）。
    fn measure_formula_draft(&self, window: &Window, cx: &App) -> Vec<DraftLineLayout> {
        let Some(state) = self.formula_editor.as_ref() else {
            return Vec::new();
        };
        let draft = state.draft.clone();
        let theme = cx.global::<crate::theme::ThemeManager>().current_arc();
        let font = font(crate::config::EditorSettings::fonts(cx).code_family);
        let selection_start = draft_clamp_boundary(&draft, state.selected_range.start);
        let selection_end = draft_clamp_boundary(&draft, state.selected_range.end)
            .max(selection_start)
            .min(draft.len());
        let lines = formula_draft_lines(
            &draft,
            &theme.colors,
            font,
            selection_start..selection_end,
        );
        let ranges = draft_line_ranges(&draft);
        let line_height = px(INPUT_LINE_HEIGHT);
        let mut layouts = Vec::with_capacity(lines.len());
        let mut top = px(0.0);
        for (range, line) in ranges.into_iter().zip(lines.into_iter()) {
            let wrapped = window
                .text_system()
                .shape_text(
                    line.text.clone(),
                    px(INPUT_FONT_SIZE),
                    &line.runs,
                    Some(px(DRAFT_CONTENT_WIDTH)),
                    None,
                )
                .ok()
                .and_then(|shaped| shaped.into_iter().next())
                .unwrap_or_default();
            let height = wrapped.size(line_height).height.max(line_height);
            layouts.push(DraftLineLayout {
                range,
                text: line.text,
                runs: line.runs,
                wrapped,
                height,
                top,
            });
            top += height;
        }
        if layouts.is_empty() {
            layouts.push(DraftLineLayout {
                range: 0..0,
                text: SharedString::from(" "),
                runs: Vec::new(),
                wrapped: WrappedLine::default(),
                height: line_height,
                top: px(0.0),
            });
        }
        layouts
    }

    /// 输入区里的一个点 → 草稿偏移。滚动句柄给的是可视框原点与滚动偏移
    /// （向下滚为负），所以内容原点 = bounds.origin + offset。
    fn draft_offset_for_point(&self, position: Point<Pixels>, window: &Window, cx: &App) -> usize {
        let Some(state) = self.formula_editor.as_ref() else {
            return 0;
        };
        let draft = state.draft.clone();
        let bounds = state.input_scroll.bounds();
        let scroll = state.input_scroll.offset();
        let layouts = self.measure_formula_draft(window, cx);
        let local = point(
            position.x - bounds.left() - scroll.x - px(INPUT_CONTENT_INSET_X),
            position.y - bounds.top() - scroll.y - px(INPUT_BORDER + INPUT_PADDING_Y),
        );
        draft_offset_for_content_point(&draft, &layouts, local, px(INPUT_LINE_HEIGHT))
    }

    /// 输入区按下：焦点交给草稿，光标落到点上的位置（shift 扩展、双击选词），
    /// 并开始一段拖动选择。补全浮层先收起。
    pub(crate) fn formula_editor_pointer_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        let position = event.position;
        let shift = event.modifiers.shift;
        let double_click = event.click_count >= 2;
        let offset = self.draft_offset_for_point(position, window, cx);
        let Some(state) = self.formula_editor.as_mut() else {
            return;
        };
        if let Some(focus) = state.focus.as_ref() {
            window.focus(focus);
        }
        state.completion = None;
        state.selecting_with_mouse = true;
        state.marked_range = None;
        // 指针定位后，上下键的「想待的那一列」以点击位置为准。
        state.caret_preferred = None;
        if shift {
            let anchor = state.selection_anchor.min(state.draft.len());
            state.selected_range = anchor.min(offset)..anchor.max(offset);
        } else if double_click {
            let word = draft_word_range(&state.draft, offset);
            state.selection_anchor = word.start;
            state.selected_range = word;
        } else {
            state.selection_anchor = offset;
            state.selected_range = offset..offset;
        }
        state.caret_epoch = Instant::now();
        cx.notify();
    }

    /// 拖动中：选区从锚点拉到指针处。没在拖动就不理这些指针噪声。
    pub(crate) fn formula_editor_pointer_move(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self
            .formula_editor
            .as_ref()
            .is_some_and(|state| state.selecting_with_mouse)
        {
            return;
        }
        let offset = self.draft_offset_for_point(position, window, cx);
        let Some(state) = self.formula_editor.as_mut() else {
            return;
        };
        let anchor = state.selection_anchor.min(state.draft.len());
        let next = anchor.min(offset)..anchor.max(offset);
        if next == state.selected_range {
            return;
        }
        state.selected_range = next;
        state.caret_epoch = Instant::now();
        cx.notify();
    }

    pub(crate) fn formula_editor_pointer_up(&mut self, cx: &mut Context<Self>) {
        if let Some(state) = self.formula_editor.as_mut()
            && state.selecting_with_mouse
        {
            state.selecting_with_mouse = false;
            cx.notify();
        }
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
        // 闪烁任务跟着焦点走：聚焦草稿时跑，失焦就停（Task drop 即取消）。
        // 与 Block::render 里 start_cursor_blink 的开关口径一致。
        // 窗口不是 key（macOS 红绿灯变灰）时 gpui 不清 window.focus，光靠
        // is_focused 判不出「失焦」——光标得连带窗口激活态一起看（用户报修）。
        let draft_focused = self
            .formula_editor
            .as_ref()
            .and_then(|state| state.focus.as_ref())
            .is_some_and(|focus| focus.is_focused(window))
            && window.is_window_active();
        if let Some(state) = self.formula_editor.as_mut() {
            if draft_focused && state.caret_blink_task.is_none() {
                state.caret_blink_task = Some(cx.spawn(
                    async |this: WeakEntity<Editor>, cx: &mut AsyncApp| loop {
                        cx.background_executor()
                            .timer(Duration::from_millis(33))
                            .await;
                        let modal_open = this
                            .update(cx, |editor: &mut Editor, cx| {
                                let Some(state) = editor.formula_editor.as_mut() else {
                                    return false;
                                };
                                // 只在亮/灭翻转的那一帧重绘，其余 tick 什么都不做。
                                let visible = draft_caret_visible(state.caret_epoch);
                                if visible != state.caret_visible {
                                    state.caret_visible = visible;
                                    cx.notify();
                                }
                                true
                            })
                            .unwrap_or(false);
                        if !modal_open {
                            break;
                        }
                    },
                ));
            } else if !draft_focused {
                state.caret_blink_task = None;
            }
        }
        // 行布局一次量好（文本、run、软换行后的行高与顶部），渲染、光标、浮层、
        // 命中四处共用同一份：各算各的就会在长行软换行之后对不上。
        let line_height = px(INPUT_LINE_HEIGHT);
        let layouts = self.measure_formula_draft(window, cx);
        // 光标被挤出可见高度时滚回来。只在光标动过的那一帧滚，否则会把用户
        // 手动的滚动一直抢回去。
        if let Some(state) = self.formula_editor.as_mut()
            && state.last_autoscroll_epoch != Some(state.caret_epoch)
        {
            let caret_y = px(INPUT_PADDING_Y)
                + draft_point_for_offset(
                    &layouts,
                    draft_clamp_offset(&state.draft, state.selected_range.start),
                    line_height,
                )
                .map(|position| position.y)
                .unwrap_or(px(0.0));
            let viewport = px(INPUT_HEIGHT) - px(2.0 * INPUT_BORDER);
            let current = state.input_scroll.offset();
            let visible_top = -current.y;
            let mut desired = current.y;
            if caret_y + line_height > visible_top + viewport {
                desired = -(caret_y + line_height - viewport);
            } else if caret_y < visible_top {
                desired = -caret_y;
            }
            // max_offset 要等滚动容器画过一帧才有（首帧是 0），这时先不记完成，
            // 补一帧再滚；不然光标永远进不了可见区。
            let max_offset_y = state.input_scroll.max_offset().height;
            let needs_scroll = desired != current.y;
            if needs_scroll && max_offset_y <= px(0.0) {
                state.last_autoscroll_epoch = None;
                cx.notify();
            } else {
                state.last_autoscroll_epoch = Some(state.caret_epoch);
                if needs_scroll {
                    let clamped = desired.clamp(-max_offset_y, px(0.0));
                    state.input_scroll.set_offset(point(current.x, clamped));
                }
            }
        }
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

        // ===== 草稿输入区（多行 + 软换行 + 选区 + 光标） =====
        let draft = state.draft.clone();
        let selection_start = draft_clamp_offset(&draft, state.selected_range.start);
        let selection_end =
            draft_clamp_offset(&draft, state.selected_range.end).max(selection_start);
        let has_selection = selection_start != selection_end;
        let code_family = crate::config::EditorSettings::fonts(cx).code_family;
        let input_font_size = px(INPUT_FONT_SIZE);
        let input_color = c.text_default;

        // 光标条与它所在的视觉行行框同高同位：字在半行距里居中（GPUI 的
        // paint_line 用 (line_height - ascent - descent)/2 定基线）。
        let caret_point = draft_point_for_offset(&layouts, selection_start, line_height)
            .unwrap_or(point(px(0.0), px(0.0)));
        let caret_div = div()
            .debug_selector(|| "formula-editor-caret".to_string())
            .absolute()
            .left(caret_point.x + px(INPUT_PADDING_X))
            .top(px(INPUT_PADDING_Y) + caret_point.y)
            .w(px(2.0))
            .h(line_height)
            .bg(c.cursor);

        // 选区底色落在 run 上（见 `apply_draft_selection_background`）：软换行时
        // GPUI 按视觉行断笔，手铺色块对不上换行后的位置。
        let draft_lines: Vec<AnyElement> = layouts
            .iter()
            .enumerate()
            .map(|(line_index, layout)| {
                div()
                    .h(layout.height)
                    .w(px(DRAFT_CONTENT_WIDTH))
                    .whitespace_normal()
                    .debug_selector(move || format!("formula-editor-line-{line_index}"))
                    .child(
                        StyledText::new(layout.text.clone())
                            .with_runs(layout.runs.clone()),
                    )
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
            .track_scroll(&state.input_scroll)
            .cursor(CursorStyle::IBeam)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|editor, event: &MouseDownEvent, window, cx| {
                    editor.formula_editor_pointer_down(event, window, cx);
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|editor, _event: &MouseUpEvent, _window, cx| {
                    editor.formula_editor_pointer_up(cx);
                }),
            )
            // 抬手发生在输入区外（拖出框外松手）也要结束拖动，否则
            // selecting_with_mouse 一直挂着，之后鼠标只是划过就会改选区。
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|editor, _event: &MouseUpEvent, _window, cx| {
                    editor.formula_editor_pointer_up(cx);
                }),
            )
            .on_mouse_move(cx.listener(
                |editor, event: &MouseMoveEvent, window, cx| {
                    // 只跟左键真的按住的移动：没按键的划过不该动选区。
                    if event.pressed_button != Some(MouseButton::Left) {
                        return;
                    }
                    editor.formula_editor_pointer_move(event.position, window, cx);
                },
            ))
            .child(
                div()
                    .px(px(INPUT_PADDING_X))
                    .py(px(INPUT_PADDING_Y))
                    .relative()
                    .w_full()
                    .children(draft_lines)
                    // 光标只在草稿聚焦、无选区、且处在闪烁的「亮」相位时画。
                    .when(
                        draft_focused && !has_selection && draft_caret_visible(state.caret_epoch),
                        |this| this.child(caret_div),
                    ),
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
                // 光标在面板内的落点：与 caret_div 同一份行布局算出来，再扣掉
                // 输入区的滚动偏移，浮层才真的贴着光标（长行软换行后也在下面）。
                let scroll_y = state.input_scroll.offset().y;
                let caret_left = px(PANEL_PADDING + INPUT_BORDER + INPUT_PADDING_X) + caret_point.x;
                let caret_bottom = px(INPUT_TOP + INPUT_BORDER + INPUT_PADDING_Y)
                    + scroll_y
                    + caret_point.y
                    + line_height;
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
