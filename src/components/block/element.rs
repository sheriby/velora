use std::cell::{Cell, RefCell};

thread_local! {
    static MEASURE_COUNT: Cell<usize> = const { Cell::new(0) };
}
use std::ops::Range;
use std::rc::Rc;

use gpui::*;

use super::{
    Block, BlockKind, InlineFootnoteHit, InlineLinkHit, ShapeMemoEntry, ShapeMemoKey,
    code_highlight_color,
};
use crate::components::HtmlCssColor;
use crate::theme::{ThemeColors, ThemeManager};

const SOURCE_LINE_NUMBER_MIN_DIGITS: usize = 2;
const SOURCE_LINE_NUMBER_GAP: f32 = 12.0;
const SOURCE_LINE_NUMBER_DIGIT_WIDTH_RATIO: f32 = 0.62;

fn prose_line_breaks(text: &str) -> Vec<usize> {
    use icu_segmenter::options::{LineBreakOptions, LineBreakStrictness, LineBreakWordOption};
    use icu_segmenter::{LineSegmenter, LineSegmenterBorrowed};
    static SEGMENTER: std::sync::OnceLock<LineSegmenterBorrowed<'static>> =
        std::sync::OnceLock::new();
    let segmenter = SEGMENTER.get_or_init(|| {
        let mut options = LineBreakOptions::default();
        options.strictness = Some(LineBreakStrictness::Strict);
        options.word_option = Some(LineBreakWordOption::Normal);
        LineSegmenter::new_dictionary(options)
    });
    segmenter.segment_str(text).collect()
}

fn prose_spacing(
    text: &str,
    code_ranges: &[Range<usize>],
    letter_spacing: Pixels,
    code_gap: Pixels,
    autospace: Pixels,
) -> Vec<(usize, Pixels)> {
    use unicode_segmentation::UnicodeSegmentation;
    let is_han = |c: char| matches!(c, '\u{3400}'..='\u{9fff}' | '\u{f900}'..='\u{faff}' | '\u{20000}'..='\u{323af}');
    let is_latin = |c: char| c.is_ascii_alphanumeric() || matches!(c, '\u{00c0}'..='\u{024f}');
    let mut ranges = code_ranges.iter().peekable();
    let mut previous: Option<(char, Option<usize>)> = None;
    let mut spacing = Vec::new();
    for (index, grapheme) in text.grapheme_indices(true) {
        while ranges.peek().is_some_and(|range| range.end <= index) {
            ranges.next();
        }
        let code = ranges.peek().filter(|range| range.contains(&index)).map(|range| range.start);
        let character = grapheme.chars().next().unwrap();
        if let Some((before, previous_code)) = previous {
            let mut gap = px(0.0);
            if previous_code != code {
                if previous_code.is_some() { gap += code_gap; }
                if code.is_some() { gap += code_gap; }
            }
            if previous_code.is_none() && code.is_none()
                && !before.is_whitespace() && !character.is_whitespace()
            {
                gap += letter_spacing;
            }
            if !(previous_code.is_some() && code.is_some())
                && ((is_han(before) && is_latin(character)) || (is_latin(before) && is_han(character)))
            {
                gap += autospace;
            }
            if gap > px(0.0) { spacing.push((index, gap)); }
        }
        previous = Some((character, code));
    }
    if previous.is_some_and(|(_, code)| code.is_some()) {
        spacing.push((text.len(), code_gap));
    }
    spacing
}

/// 在整块文本已排版的各行上复现渲染期的水平间距：行内代码两侧的 `code_gap`、
/// 相邻非空白字形间的 `letter_spacing`、中西文边界的 autospacing。折行前调用，
/// 行宽随间距增长。
///
/// 表格列宽测量与正文渲染必须走同一条管线：测量不加间距，水位法钉在
/// 「测量内容宽 + 余量」上的列，渲染时因间距变宽必然 mid-word 折行
/// （用户报修）。
pub(crate) fn add_render_spacing(
    lines: &mut [WrappedLine],
    code_ranges: &[Range<usize>],
    letter_spacing: Pixels,
    code_gap: Pixels,
    font_size: Pixels,
) {
    let mut line_start = 0;
    for line in lines.iter_mut() {
        let line_end = line_start + line.len();
        let ranges: Vec<_> = code_ranges
            .iter()
            .filter(|range| range.start < line_end && line_start < range.end)
            .map(|range| {
                range.start.saturating_sub(line_start)..range.end.min(line_end) - line_start
            })
            .collect();
        let spacing =
            prose_spacing(&line.text, &ranges, letter_spacing, code_gap, font_size * 0.25);
        line.add_horizontal_spacing(&spacing);
        line_start += line.len() + 1;
    }
}

fn inline_code_background_bounds(
    segment: Bounds<Pixels>,
    baseline: Pixels,
    ascent: Pixels,
    descent: Pixels,
    padding: Point<Pixels>,
) -> Bounds<Pixels> {
    Bounds::from_corners(
        point(segment.left() - padding.x,
            (segment.top() + baseline - ascent - padding.y).max(segment.top() + px(1.0))),
        point(segment.right() + padding.x,
            (segment.top() + baseline + descent + padding.y).min(segment.bottom() - px(1.0))),
    )
}

fn source_line_count(text: &str) -> usize {
    text.split('\n').count().max(1)
}

fn source_line_number_gutter_width(line_count: usize, font_size: Pixels) -> Pixels {
    let digits = line_count
        .max(1)
        .to_string()
        .len()
        .max(SOURCE_LINE_NUMBER_MIN_DIGITS);
    px(digits as f32 * f32::from(font_size) * SOURCE_LINE_NUMBER_DIGIT_WIDTH_RATIO)
        + px(SOURCE_LINE_NUMBER_GAP)
}

fn source_text_bounds(bounds: Bounds<Pixels>, gutter_width: Pixels) -> Bounds<Pixels> {
    if gutter_width <= px(0.0) {
        return bounds;
    }

    let max_gutter = (f32::from(bounds.size.width) - 1.0).max(0.0);
    let gutter_width = px(f32::from(gutter_width).min(max_gutter));
    Bounds::new(
        point(bounds.origin.x + gutter_width, bounds.origin.y),
        size(
            (bounds.size.width - gutter_width).max(px(1.0)),
            bounds.size.height,
        ),
    )
}

fn source_line_number_tops(lines: &[WrappedLine], line_height: Pixels) -> Vec<Pixels> {
    let mut tops = Vec::with_capacity(lines.len());
    let mut y = Pixels::default();
    for line in lines {
        tops.push(y);
        y += wrapped_line_height(line, line_height);
    }
    tops
}

pub(crate) fn build_text_runs(
    input: &Block,
    display_text: &SharedString,
    base_run: &TextRun,
    underline_thickness: Pixels,
    link_color: Hsla,
    code_text: Hsla,
    show_inline_code_backgrounds: bool,
    code_font_family: &str,
    code_font_size: Pixels,
) -> Vec<TextRun> {
    let spans = input.inline_spans();
    let mut boundaries = vec![0, display_text.len()];
    for span in spans {
        boundaries.push(span.range.start);
        boundaries.push(span.range.end);
    }
    if let Some(marked_range) = input.marked_range.as_ref() {
        boundaries.push(marked_range.start);
        boundaries.push(marked_range.end);
    }
    boundaries.sort_unstable();
    boundaries.dedup();

    let marked_range = input.marked_range.as_ref();
    let mut runs = Vec::new();
    let mut span_idx = 0usize;
    for boundary_pair in boundaries.windows(2) {
        let start = boundary_pair[0];
        let end = boundary_pair[1];
        if start >= end {
            continue;
        }

        // Spans are stored in ascending order and boundaries are sorted, so
        // we can advance a single index instead of re-scanning per boundary.
        while span_idx < spans.len() && spans[span_idx].range.end <= start {
            span_idx += 1;
        }
        let active_span = spans
            .get(span_idx)
            .filter(|span| span.range.start <= start && start < span.range.end);

        let inline_style = active_span.map(|s| s.style).unwrap_or_default();
        let html_style = active_span.and_then(|s| s.html_style);
        let is_link = active_span.map(|s| s.link.is_some()).unwrap_or(false);
        let is_footnote = active_span.map(|s| s.footnote.is_some()).unwrap_or(false);
        let is_marked = marked_range
            .map(|range| start < range.end && range.start < end)
            .unwrap_or(false);

        let mut font = base_run.font.clone();
        if inline_style.code {
            font.family = SharedString::from(code_font_family.to_string());
            if show_inline_code_backgrounds && font.weight < FontWeight::MEDIUM {
                font.weight = FontWeight::MEDIUM;
            }
        }
        if inline_style.bold && font.weight < FontWeight::BOLD {
            font.weight = FontWeight::BOLD;
        }
        if inline_style.italic {
            font.style = FontStyle::Italic;
        }

        let mut run_color = if is_link || is_footnote {
            link_color
        } else if inline_style.code && show_inline_code_backgrounds {
            code_text
        } else {
            base_run.color
        };
        if let Some(style) = html_style
            && let Some(color) = style.color
        {
            run_color = html_css_color_to_hsla(color, run_color);
        }
        let underline = (inline_style.underline || is_marked || is_link || is_footnote).then_some(
            UnderlineStyle {
                color: Some(run_color),
                thickness: underline_thickness,
                wavy: false,
            },
        );
        let strikethrough = inline_style.strikethrough.then_some(StrikethroughStyle {
            color: Some(run_color),
            thickness: underline_thickness,
        });

        // Inline code is painted as separate rounded fragments below the text.
        let mut background_color = base_run.background_color;
        if let Some(style) = html_style
            && let Some(color) = style.background_color
        {
            background_color = Some(html_css_color_to_hsla(color, run_color));
        }

        runs.push(TextRun {
            len: end - start,
            font,
            color: run_color,
            background_color,
            underline,
            strikethrough,
            // 行内代码跟随「代码块字体大小」设置（用户报修：显示态与编辑态
            // 必须同字号，否则点击进入编辑的瞬间字号会跳变）。
            font_size: inline_style.code.then_some(code_font_size),
        });
    }

    if runs.is_empty() {
        vec![base_run.clone()]
    } else {
        runs
    }
}

fn html_css_color_to_hsla(color: HtmlCssColor, current_color: Hsla) -> Hsla {
    match color {
        HtmlCssColor::CurrentColor => current_color,
        HtmlCssColor::Rgba(color) => Hsla::from(Rgba {
            r: color.red as f32 / 255.0,
            g: color.green as f32 / 255.0,
            b: color.blue as f32 / 255.0,
            a: color.alpha.clamp(0.0, 1.0),
        }),
    }
}

fn build_code_text_runs(
    input: &Block,
    display_text: &SharedString,
    base_run: &TextRun,
    underline_thickness: Pixels,
    colors: &ThemeColors,
) -> Vec<TextRun> {
    let highlight_spans = input
        .code_highlight_result()
        .map(|r| r.spans.as_slice())
        .unwrap_or(&[]);
    let mut boundaries = vec![0, display_text.len()];
    for span in highlight_spans {
        boundaries.push(span.range.start);
        boundaries.push(span.range.end);
    }
    if let Some(marked_range) = input.marked_range.as_ref() {
        boundaries.push(marked_range.start);
        boundaries.push(marked_range.end);
    }
    boundaries.sort_unstable();
    boundaries.dedup();

    let marked_range = input.marked_range.as_ref();
    let mut runs = Vec::new();
    let mut span_idx = 0usize;
    for boundary_pair in boundaries.windows(2) {
        let start = boundary_pair[0];
        let end = boundary_pair[1];
        if start >= end {
            continue;
        }

        let is_marked = marked_range
            .map(|range| start < range.end && range.start < end)
            .unwrap_or(false);
        while span_idx < highlight_spans.len() && highlight_spans[span_idx].range.end <= start {
            span_idx += 1;
        }
        let run_color = highlight_spans
            .get(span_idx)
            .filter(|span| span.range.start <= start && start < span.range.end)
            .map(|span| code_highlight_color(colors, span.class))
            .unwrap_or(base_run.color);

        runs.push(TextRun {
            len: end - start,
            font: base_run.font.clone(),
            color: run_color,
            background_color: base_run.background_color,
            underline: is_marked.then_some(UnderlineStyle {
                color: Some(run_color),
                thickness: underline_thickness,
                wavy: false,
            }),
            strikethrough: None,
            font_size: None,
        });
    }

    if runs.is_empty() {
        vec![base_run.clone()]
    } else {
        runs
    }
}

/// Compute byte ranges of each hard-line (`\n`-separated) segment in the
/// visible text.  Index `i` in the returned Vec corresponds to the `i`-th
/// WrappedLine produced by `shape_text`.
pub(super) fn hard_line_ranges(text: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut start = 0;
    for (idx, _) in text.match_indices('\n') {
        ranges.push(start..idx);
        start = idx + 1;
    }
    ranges.push(start..text.len());
    ranges
}

/// Map a flat visible-text offset to `(line_index, offset_within_line)`.
pub(super) fn line_index_for_offset(ranges: &[Range<usize>], offset: usize) -> (usize, usize) {
    let clamped = offset.min(ranges.last().map(|r| r.end).unwrap_or(0));
    for (i, range) in ranges.iter().enumerate() {
        if clamped <= range.end {
            return (i, clamped.saturating_sub(range.start));
        }
    }
    let last = ranges.len() - 1;
    (last, ranges[last].len())
}

pub(crate) fn aligned_line_left(
    line: &WrappedLine,
    bounds: Bounds<Pixels>,
    align: TextAlign,
) -> Pixels {
    let slack = (bounds.size.width - line.width()).max(px(0.0));
    match align {
        TextAlign::Left => bounds.left(),
        TextAlign::Center => bounds.left() + slack / 2.0,
        TextAlign::Right => bounds.left() + slack,
    }
}

pub(super) fn wrapped_line_height(line: &WrappedLine, line_height: Pixels) -> Pixels {
    line.size(line_height).height
}

pub(super) fn wrapped_line_top(
    lines: &[WrappedLine],
    line_height: Pixels,
    line_idx: usize,
) -> Pixels {
    lines.iter().take(line_idx).fold(px(0.0), |height, line| {
        height + wrapped_line_height(line, line_height)
    })
}

pub(super) fn wrapped_line_for_y(
    lines: &[WrappedLine],
    line_height: Pixels,
    relative_y: Pixels,
) -> Option<(usize, Pixels)> {
    if lines.is_empty() {
        return None;
    }

    let mut top = px(0.0);
    for (line_idx, line) in lines.iter().enumerate() {
        let height = wrapped_line_height(line, line_height);
        if relative_y < top + height || line_idx + 1 == lines.len() {
            return Some((line_idx, (relative_y - top).max(px(0.0))));
        }
        top += height;
    }

    Some((lines.len() - 1, px(0.0)))
}

fn wrap_boundary_offset(line: &WrappedLine, wrap_idx: usize) -> Option<usize> {
    let boundary = line.wrap_boundaries().get(wrap_idx)?;
    let run = line.unwrapped_layout.runs.get(boundary.run_ix)?;
    let glyph = run.glyphs.get(boundary.glyph_ix)?;
    Some(glyph.index)
}

fn wrapped_row_offsets(line: &WrappedLine) -> Vec<usize> {
    let mut offsets = Vec::with_capacity(line.wrap_boundaries().len() + 2);
    offsets.push(0);
    for wrap_idx in 0..line.wrap_boundaries().len() {
        if let Some(offset) = wrap_boundary_offset(line, wrap_idx) {
            offsets.push(offset.min(line.len()));
        }
    }
    offsets.push(line.len());
    offsets.dedup();
    offsets
}

fn wrapped_row_origin_x(
    line: &WrappedLine,
    bounds: Bounds<Pixels>,
    align: TextAlign,
    row_start: usize,
    row_end: usize,
) -> Pixels {
    let row_width =
        line.unwrapped_layout.x_for_index(row_end) - line.unwrapped_layout.x_for_index(row_start);
    let align_width = line.width();
    let slack = (align_width - row_width).max(px(0.0));
    let line_left = aligned_line_left(line, bounds, align);
    match align {
        TextAlign::Left => line_left,
        TextAlign::Center => line_left + slack / 2.0,
        TextAlign::Right => line_left + slack,
    }
}

pub(super) fn position_for_offset(
    line: &WrappedLine,
    offset: usize,
    line_height: Pixels,
    prefer_next_wrap_start: bool,
) -> Option<Point<Pixels>> {
    let offsets = wrapped_row_offsets(line);
    for row_idx in 0..offsets.len().saturating_sub(1) {
        let row_start = offsets[row_idx];
        let row_end = offsets[row_idx + 1];
        let is_start_of_wrapped_row = prefer_next_wrap_start && row_idx > 0 && offset == row_start;
        if is_start_of_wrapped_row || (offset >= row_start && offset < row_end) {
            let row_start_x = line.unwrapped_layout.x_for_index(row_start);
            let x = line.unwrapped_layout.x_for_index(offset) - row_start_x;
            return Some(point(x, line_height * row_idx as f32));
        }
    }

    line.position_for_index(offset, line_height)
}

pub(super) fn cursor_bounds_for_offset(
    lines: &[WrappedLine],
    bounds: Bounds<Pixels>,
    line_height: Pixels,
    text: &str,
    offset: usize,
    align: TextAlign,
    cursor_width: Pixels,
) -> Option<Bounds<Pixels>> {
    let ranges = hard_line_ranges(text);
    let (line_idx, offset_in_line) = line_index_for_offset(&ranges, offset);
    let layout = lines.get(line_idx)?;
    let origin_x = aligned_line_left(layout, bounds, align);
    let cursor_pos = position_for_offset(layout, offset_in_line, line_height, true)?;
    let y_offset = bounds.top() + wrapped_line_top(lines, line_height, line_idx);
    Some(Bounds::new(
        point(origin_x + cursor_pos.x, y_offset + cursor_pos.y),
        size(cursor_width, line_height),
    ))
}

pub(super) fn range_bounds(
    lines: &[WrappedLine],
    bounds: Bounds<Pixels>,
    line_height: Pixels,
    text: &str,
    range: Range<usize>,
    align: TextAlign,
) -> Option<Bounds<Pixels>> {
    let segments = range_segment_bounds(lines, bounds, line_height, text, range.clone(), align);
    if segments.is_empty() {
        return cursor_bounds_for_offset(
            lines,
            bounds,
            line_height,
            text,
            range.start,
            align,
            px(1.0),
        );
    }

    let mut union = segments[0];
    for segment in segments.iter().skip(1) {
        union = Bounds::from_corners(
            point(
                union.left().min(segment.left()),
                union.top().min(segment.top()),
            ),
            point(
                union.right().max(segment.right()),
                union.bottom().max(segment.bottom()),
            ),
        );
    }
    Some(union)
}

fn range_segment_bounds_for_hard_line(
    lines: &[WrappedLine],
    bounds: Bounds<Pixels>,
    line_height: Pixels,
    line_idx: usize,
    start_offset: usize,
    end_offset: usize,
    align: TextAlign,
) -> Vec<Bounds<Pixels>> {
    let Some(line) = lines.get(line_idx) else {
        return Vec::new();
    };
    let line_top = bounds.top() + wrapped_line_top(lines, line_height, line_idx);
    let offsets = wrapped_row_offsets(line);
    let mut segments = Vec::new();

    for row_idx in 0..offsets.len().saturating_sub(1) {
        let row_start = offsets[row_idx];
        let row_end = offsets[row_idx + 1];
        let seg_start = start_offset.max(row_start).min(row_end);
        let seg_end = end_offset.min(row_end).max(row_start);
        if seg_start >= seg_end {
            continue;
        }

        let row_start_x = line.unwrapped_layout.x_for_index(row_start);
        let start_x = line.unwrapped_layout.x_for_index(seg_start) - row_start_x;
        let end_x = line.unwrapped_layout.x_for_index(seg_end) - row_start_x;
        let origin_x = wrapped_row_origin_x(line, bounds, align, row_start, row_end);
        let y = line_top + line_height * row_idx as f32;
        segments.push(Bounds::from_corners(
            point(origin_x + start_x, y),
            point(origin_x + end_x, y + line_height),
        ));
    }

    segments
}

pub(super) fn range_segment_bounds(
    lines: &[WrappedLine],
    bounds: Bounds<Pixels>,
    line_height: Pixels,
    text: &str,
    range: Range<usize>,
    align: TextAlign,
) -> Vec<Bounds<Pixels>> {
    if range.start >= range.end || lines.is_empty() {
        return Vec::new();
    }

    let ranges = hard_line_ranges(text);
    let (start_line, start_offset) = line_index_for_offset(&ranges, range.start);
    let (end_line, end_offset) = line_index_for_offset(&ranges, range.end);
    let mut segments = Vec::new();

    for line_idx in start_line..=end_line {
        let hard_range = &ranges[line_idx];
        let line_start = if line_idx == start_line {
            start_offset
        } else {
            0
        };
        let line_end = if line_idx == end_line {
            end_offset
        } else {
            hard_range.len()
        };
        segments.extend(range_segment_bounds_for_hard_line(
            lines,
            bounds,
            line_height,
            line_idx,
            line_start,
            line_end,
            align,
        ));
    }

    segments
}

fn point_inside_bounds(bounds: Bounds<Pixels>, position: Point<Pixels>) -> bool {
    position.x >= bounds.left()
        && position.x < bounds.right()
        && position.y >= bounds.top()
        && position.y < bounds.bottom()
}

pub(crate) fn link_at_position<'a>(
    input: &'a Block,
    lines: &[WrappedLine],
    bounds: Bounds<Pixels>,
    line_height: Pixels,
    position: Point<Pixels>,
) -> Option<&'a InlineLinkHit> {
    if input.is_source_raw_mode()
        || input.display_text().is_empty()
        || lines.is_empty()
        || position.y < bounds.top()
        || position.y >= bounds.bottom()
    {
        return None;
    }

    let text = input.display_text();
    let align = input.text_align();

    for span in input.inline_spans() {
        let Some(link) = span.link.as_ref() else {
            continue;
        };
        if span.range.is_empty() {
            continue;
        }

        for link_bounds in
            range_segment_bounds(lines, bounds, line_height, text, span.range.clone(), align)
        {
            if point_inside_bounds(link_bounds, position) {
                return Some(link);
            }
        }
    }

    None
}

pub(crate) fn footnote_at_position<'a>(
    input: &'a Block,
    lines: &[WrappedLine],
    bounds: Bounds<Pixels>,
    line_height: Pixels,
    position: Point<Pixels>,
) -> Option<&'a InlineFootnoteHit> {
    if input.is_source_raw_mode()
        || input.display_text().is_empty()
        || lines.is_empty()
        || position.y < bounds.top()
        || position.y >= bounds.bottom()
    {
        return None;
    }

    let text = input.display_text();
    let align = input.text_align();

    for span in input.inline_spans() {
        let Some(footnote) = span.footnote.as_ref() else {
            continue;
        };
        if span.range.is_empty() {
            continue;
        }

        for footnote_bounds in
            range_segment_bounds(lines, bounds, line_height, text, span.range.clone(), align)
        {
            if point_inside_bounds(footnote_bounds, position) {
                return Some(footnote);
            }
        }
    }

    None
}

/// Custom low-level [`Element`] that renders a block's inline-formatted
/// text with selection highlights and a blinking cursor.
///
/// Supports multi-line text (used by code blocks) via hard `\n` breaks.
/// Each `\n` produces a separate `WrappedLine` from the text shaper.
pub struct BlockTextElement {
    input: Entity<Block>,
    is_placeholder: bool,
    placeholder_text: Option<SharedString>,
    placeholder_color: Option<Hsla>,
}

/// Single-line text element used to edit a fenced code block's info string.
pub struct CodeLanguageInputElement {
    input: Entity<Block>,
    placeholder: SharedString,
}

impl CodeLanguageInputElement {
    pub fn new(input: Entity<Block>, placeholder: SharedString) -> Self {
        Self { input, placeholder }
    }
}

pub struct CodeLanguageInputPrepaintState {
    line: Option<ShapedLine>,
    cursor: Option<PaintQuad>,
    selection: Option<PaintQuad>,
    hitbox: Option<Hitbox>,
}

impl IntoElement for CodeLanguageInputElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for CodeLanguageInputElement {
    type RequestLayoutState = ();
    type PrepaintState = CodeLanguageInputPrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let theme = cx.global::<ThemeManager>().current_arc();
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = px(theme.dimensions.code_language_input_height)
            .max(window.line_height())
            .into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {

        let theme = cx.global::<ThemeManager>().current_arc();
        let input = self.input.read(cx);
        let content = input.code_language_text().to_string();
        let is_placeholder = content.is_empty();
        let display_text: SharedString = if is_placeholder {
            self.placeholder.clone()
        } else {
            content.into()
        };
        let focused = input.code_language_focus_handle.is_focused(window);
        let style = window.text_style();
        let run_color = if is_placeholder {
            theme.colors.code_language_input_placeholder
        } else {
            theme.colors.code_language_input_text
        };
        let base_run = TextRun {
            len: display_text.len(),
            font: style.font(),
            color: run_color,
            background_color: None,
            underline: None,
            strikethrough: None,
            font_size: None,
        };

        let runs = if let Some(marked_range) = input
            .code_language_marked_range
            .as_ref()
            .filter(|_| !is_placeholder)
        {
            vec![
                TextRun {
                    len: marked_range.start,
                    ..base_run.clone()
                },
                TextRun {
                    len: marked_range.end - marked_range.start,
                    underline: Some(UnderlineStyle {
                        color: Some(run_color),
                        thickness: px(theme.dimensions.underline_thickness),
                        wavy: false,
                    }),
                    ..base_run.clone()
                },
                TextRun {
                    len: display_text.len() - marked_range.end,
                    ..base_run
                },
            ]
            .into_iter()
            .filter(|run| run.len > 0)
            .collect()
        } else {
            vec![base_run]
        };

        let font_size = style.font_size.to_pixels(window.rem_size());
        let line = window
            .text_system()
            .shape_line(display_text, font_size, &runs, None);
        let line_height = bounds.size.height;
        let selection = if focused && !input.code_language_selected_range.is_empty() {
            let start = line.x_for_index(input.code_language_selected_range.start);
            let end = line.x_for_index(input.code_language_selected_range.end);
            Some(fill(
                Bounds::from_corners(
                    point(bounds.left() + start, bounds.top()),
                    point(bounds.left() + end, bounds.bottom()),
                ),
                theme.colors.selection,
            ))
        } else {
            None
        };
        let cursor = if focused && input.code_language_selected_range.is_empty() {
            let cursor_x = line.x_for_index(input.code_language_cursor_offset());
            let mut cursor_color = theme.colors.cursor;
            cursor_color.a *= input.cursor_opacity();
            Some(fill(
                Bounds::new(
                    point(bounds.left() + cursor_x, bounds.top()),
                    size(px(theme.dimensions.cursor_width), line_height),
                ),
                cursor_color,
            ))
        } else {
            None
        };
        let hitbox = Some(window.insert_hitbox(bounds, HitboxBehavior::Normal));

        CodeLanguageInputPrepaintState {
            line: Some(line),
            cursor,
            selection,
            hitbox,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(hitbox) = prepaint.hitbox.as_ref()
            && hitbox.is_hovered(window)
        {
            window.set_cursor_style(CursorStyle::IBeam, hitbox);
        }

        let focus_handle = self.input.read(cx).code_language_focus_handle.clone();
        if focus_handle.is_focused(window) {
            window.handle_input(
                &focus_handle,
                ElementInputHandler::new(bounds, self.input.clone()),
                cx,
            );
        }

        if let Some(selection) = prepaint.selection.take() {
            window.paint_quad(selection);
        }

        let line = prepaint.line.take().expect("line should be shaped");
        line.paint(bounds.origin, bounds.size.height, window, cx)
            .ok();

        if focus_handle.is_focused(window)
            && let Some(cursor) = prepaint.cursor.take()
        {
            window.paint_quad(cursor);
        }

        self.input.update(cx, |input, _cx| {
            input.code_language_last_layout = Some(line);
            input.code_language_last_bounds = Some(bounds);
        });
    }
}

impl BlockTextElement {
    pub fn new(input: Entity<Block>, is_placeholder: bool) -> Self {
        Self {
            input,
            is_placeholder,
            placeholder_text: None,
            placeholder_color: None,
        }
    }

    pub fn with_placeholder(
        input: Entity<Block>,
        is_placeholder: bool,
        placeholder_text: SharedString,
        placeholder_color: Option<Hsla>,
    ) -> Self {
        Self {
            input,
            is_placeholder,
            placeholder_text: Some(placeholder_text),
            placeholder_color,
        }
    }
}

/// Prepared text layout and paint geometry for one `BlockTextElement` frame.
pub struct PrepaintState {
    lines: std::sync::Arc<Vec<WrappedLine>>,
    source_line_numbers: Vec<ShapedLine>,
    source_line_number_gutter_width: Pixels,
    cursor: Option<PaintQuad>,
    selection: Vec<PaintQuad>,
    code_backgrounds: Vec<PaintQuad>,
    search_highlights: Vec<PaintQuad>,
    line_height: Pixels,
    hitbox: Hitbox,
}

impl IntoElement for BlockTextElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for BlockTextElement {
    type RequestLayoutState = Rc<RefCell<Option<std::sync::Arc<Vec<WrappedLine>>>>>;
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let theme = cx.global::<ThemeManager>().current_arc();
        let input = self.input.read(cx);
        let shared_text = input.shared_display_text();
        let is_placeholder = self.is_placeholder;
        let show_inline_code_backgrounds = !input.is_source_raw_mode();
        let show_source_line_numbers = input.show_source_line_numbers();
        let source_line_count = source_line_count(shared_text.as_ref());
        let wrap_prose = !is_placeholder
            && !input.is_source_raw_mode()
            && (input.kind() == BlockKind::Paragraph || input.kind().is_list_item());
        let code_ranges: Vec<_> = input.inline_spans().iter().filter(|span| span.style.code)
            .map(|span| span.range.clone()).collect();
        let space_prose = !is_placeholder && !input.is_source_raw_mode() && !input.kind().is_code_block();
        let style = window.text_style();

        let (display_text, text_color): (SharedString, Hsla) = if is_placeholder {
            (
                self.placeholder_text
                    .clone()
                    .unwrap_or_else(|| theme.placeholders.empty_editing.clone().into()),
                self.placeholder_color
                    .unwrap_or(theme.colors.text_placeholder),
            )
        } else {
            (shared_text, style.color)
        };

        let run = TextRun {
            len: display_text.len(),
            font: style.font(),
            color: text_color,
            background_color: None,
            underline: None,
            strikethrough: None,
            font_size: None,
        };

        let runs: Vec<TextRun> = if !is_placeholder {
            if input.kind().is_code_block() {
                build_code_text_runs(
                    input,
                    &display_text,
                    &run,
                    px(theme.dimensions.underline_thickness),
                    &theme.colors,
                )
            } else {
                build_text_runs(
                    input,
                    &display_text,
                    &run,
                    px(theme.dimensions.underline_thickness),
                    theme.colors.text_link,
                    theme.colors.text_link,
                    show_inline_code_backgrounds,
                    &crate::config::EditorSettings::fonts(cx).code_family,
                    px(crate::config::EditorSettings::fonts(cx).code_size as f32),
                )
            }
        } else {
            vec![run]
        };

        let font_size = style.font_size.to_pixels(window.rem_size());
        let letter_spacing = font_size * theme.typography.text_letter_spacing;
        let code_gap = px(theme.dimensions.code_bg_pad_x) + font_size * 0.125;
        let code_size = runs.iter().filter_map(|run| run.font_size).fold(px(0.0), Pixels::max);
        let line_height = window.line_height().max(code_size * 1.35);
        let source_line_start = input.source_line_start();
        let source_line_number_gutter_width = show_source_line_numbers
            .then(|| {
                source_line_number_gutter_width(source_line_start + source_line_count, font_size)
            })
            .unwrap_or(px(0.0));

        // P3：shape 备忘键。任何影响 shape 结果的输入都进键：文本代数、
        // 换行宽、基准字号、字体（偏好 + 主题排版）、主题（run 颜色烘焙）。
        use std::hash::{Hash, Hasher};
        let mut fingerprint_hasher = std::hash::DefaultHasher::new();
        let font_prefs = crate::config::EditorSettings::fonts(cx);
        font_prefs.markdown_family.hash(&mut fingerprint_hasher);
        font_prefs.markdown_size.hash(&mut fingerprint_hasher);
        font_prefs.code_family.hash(&mut fingerprint_hasher);
        font_prefs.code_size.hash(&mut fingerprint_hasher);
        let style_font = style.font();
        style_font.family.hash(&mut fingerprint_hasher);
        style_font.weight.hash(&mut fingerprint_hasher);
        style_font.style.hash(&mut fingerprint_hasher);
        let font_fingerprint = fingerprint_hasher.finish();
        let theme_fingerprint =
            std::sync::Arc::as_ptr(&theme) as usize as u64;
        let memo_key_base = ShapeMemoKey {
            generation: input.display_generation(),
            wrap_width: None,
            wrap_prose,
            space_prose,
            font_size: f32::from(font_size).to_bits(),
            font_fingerprint,
            theme_fingerprint,
        };
        let cached_memo = input.shape_memo_entry();
        let input_entity = self.input.clone();

        let shared_lines: Rc<RefCell<Option<std::sync::Arc<Vec<WrappedLine>>>>> =
            Rc::new(RefCell::new(None));
        let shared_lines_clone = shared_lines.clone();

        let mut layout_style = Style::default();
        layout_style.size.width = relative(1.).into();
        layout_style.min_size.width = px(0.0).into();
        layout_style.max_size.width = relative(1.).into();

        let layout_id = window.request_measured_layout(
            layout_style,
            move |known_dimensions, available_space, window, closure_cx| {
                let wrap_width = known_dimensions.width.or(match available_space.width {
                    AvailableSpace::Definite(x) => Some(x),
                    AvailableSpace::MinContent => Some(px(1.0)),
                    AvailableSpace::MaxContent => Some(window.viewport_size().width.max(px(1.0))),
                });
                let text_wrap_width =
                    wrap_width.map(|width| (width - source_line_number_gutter_width).max(px(1.0)));

                let mut key = memo_key_base;
                key.wrap_width = text_wrap_width.map(|width| f32::from(width).to_bits());
                // P3：min-content 测量（≈1px 宽）会让整块文本按每字符一行
                // 病态换行（53KB → 数万行）。最小宽度对布局无意义（块宽由
                // centered_width 固定），直接返回保守下界，跳过 shape。
                if text_wrap_width.is_some_and(|width| width <= px(2.0)) {
                    let estimate_lines = display_text.split('\n').count().max(1) as f32;
                    return Size::new(
                        px(1.0),
                        px(estimate_lines * f32::from(line_height)),
                    );
                }
                if let Some(memo) = cached_memo.as_ref()
                    && memo.key == key
                {
                    // P3：键命中，跳过 build_text_runs 产物与 shape_text。
                    let lines = memo.lines.clone();
                    let mut total_size: Size<Pixels> = Size::default();
                    for line in lines.iter() {
                        let ls = line.size(line_height);
                        total_size.height += ls.height;
                        total_size.width = total_size.width.max(ls.width);
                    }
                    total_size.width += source_line_number_gutter_width;
                    *shared_lines_clone.borrow_mut() = Some(lines);
                    return total_size;
                }
                match window.text_system().shape_text(
                    display_text.clone(),
                    font_size,
                    &runs,
                    text_wrap_width,
                    None,
                ) {
                    Ok(mut lines) => {
                        if space_prose {
                            add_render_spacing(
                                &mut lines,
                                &code_ranges,
                                letter_spacing,
                                code_gap,
                                font_size,
                            );
                        }
                        for line in lines.iter_mut() {
                            if wrap_prose || space_prose {
                                if line
                                    .wrap_width
                                    .is_some_and(|width| line.unwrapped_layout.width > width)
                                {
                                    use unicode_segmentation::UnicodeSegmentation;
                                    let emergency_breaks: Vec<_> = line.text.grapheme_indices(true)
                                        .map(|(index, _)| index).collect();
                                    line.wrap_at_boundaries(&prose_line_breaks(&line.text), &emergency_breaks);
                                }
                            }
                        }
                        let mut total_size: Size<Pixels> = Size::default();
                        for line in lines.iter() {
                            let ls = line.size(line_height);
                            total_size.height += ls.height;
                            total_size.width = total_size.width.max(ls.width);
                        }
                        total_size.width += source_line_number_gutter_width;
                        let lines = std::sync::Arc::new(lines.into_vec());
                        // 立即写入备忘：同一次布局内 taffy 可能多次 measure，
                        // 迟写会让每次 measure 都重新 shape。
                        input_entity.update(closure_cx, |block, _block_cx| {
                            block.set_shape_memo(ShapeMemoEntry {
                                key,
                                lines: lines.clone(),
                            });
                        });
                        *shared_lines_clone.borrow_mut() = Some(lines);
                        total_size
                    }
                    Err(_) => Size::default(),
                }
            },
        );

        (layout_id, shared_lines)
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let theme = cx.global::<ThemeManager>().current_arc();
        let input = self.input.read(cx);
        let editor_selection_range = input
            .editor_selection_range
            .as_ref()
            .filter(|range| !range.is_empty())
            .cloned();
        let selected_range = editor_selection_range
            .clone()
            .unwrap_or_else(|| input.selected_range.clone());
        let cursor = input.cursor_offset();
        let code_size = if input.inline_spans().iter().any(|span| span.style.code) {
            px(crate::config::EditorSettings::fonts(cx).code_size as f32)
        } else { px(0.0) };
        let line_height = window.line_height().max(code_size * 1.35);
        let focused = input.focus_handle.is_focused(window);
        let show_inline_code_backgrounds = !input.is_source_raw_mode();
        let show_source_line_numbers = input.show_source_line_numbers();
        let style = window.text_style();
        let font_size = style.font_size.to_pixels(window.rem_size());

        let lines = request_layout.borrow_mut().take().unwrap_or_default();
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        let source_line_number_gutter_width = show_source_line_numbers
            .then(|| source_line_number_gutter_width(lines.len().max(1) + input.source_line_start() - 1, font_size))
            .unwrap_or(px(0.0));
        let text_bounds = source_text_bounds(bounds, source_line_number_gutter_width);
        let source_line_numbers = if show_source_line_numbers {
            let run_color = theme.colors.text_placeholder;
            let line_start = input.source_line_start();
            (line_start..line_start + lines.len().max(1))
                .map(|line_number| {
                    let label = line_number.to_string();
                    window.text_system().shape_line(
                        SharedString::from(label.clone()),
                        font_size,
                        &[TextRun {
                            len: label.len(),
                            font: style.font(),
                            color: run_color,
                            background_color: None,
                            underline: None,
                            strikethrough: None,
                            font_size: None,
                        }],
                        None,
                    )
                })
                .collect()
        } else {
            Vec::new()
        };

        let cursor_opacity = input.cursor_opacity();
        let cursor_color = {
            let mut c = theme.colors.cursor;
            c.a *= cursor_opacity;
            c
        };
        let cursor_width = theme.dimensions.cursor_width;
        let selection_color = theme.colors.selection;
        let text_align = input.text_align();

        let (selection_quads, cursor_quad) =
            if (focused || editor_selection_range.is_some()) && !lines.is_empty() {
                if self.is_placeholder {
                    // Placeholder: cursor after the placeholder text
                    let layout = &lines[0];
                    let origin_x = aligned_line_left(layout, text_bounds, text_align);
                    let cursor_pos = layout
                        .position_for_index(0, line_height)
                        .unwrap_or_default();
                    (
                        vec![],
                        Some(fill(
                            Bounds::new(
                                point(origin_x + cursor_pos.x, text_bounds.top() + cursor_pos.y),
                                size(px(cursor_width), line_height),
                            ),
                            cursor_color,
                        )),
                    )
                } else if selected_range.is_empty() {
                    // No selection: just draw the cursor
                    let text = input.display_text();
                    (
                        vec![],
                        cursor_bounds_for_offset(
                            &lines,
                            text_bounds,
                            line_height,
                            text,
                            cursor,
                            text_align,
                            px(cursor_width),
                        )
                        .map(|bounds| fill(bounds, cursor_color)),
                    )
                } else {
                    let text = input.display_text();
                    let quads = range_segment_bounds(
                        &lines,
                        text_bounds,
                        line_height,
                        text,
                        selected_range,
                        text_align,
                    )
                    .into_iter()
                    .map(|bounds| fill(bounds, selection_color))
                    .collect();
                    (quads, None)
                }
            } else {
                (vec![], None)
            };

        // Compute code-span background quads with rounded corners and padding.
        let mut code_quads = Vec::new();
        if show_inline_code_backgrounds && !self.is_placeholder {
            let text = input.display_text();
            let hard_ranges = hard_line_ranges(text);
            let code_color = theme.colors.code_bg;
            let pad_x = px(theme.dimensions.code_bg_pad_x);
            let pad_y = px(theme.dimensions.code_bg_pad_y);
            let radius = px(theme.dimensions.code_bg_radius);
            for span in input.inline_spans() {
                if !span.style.code || span.range.is_empty() {
                    continue;
                }
                for segment in range_segment_bounds(
                    &lines,
                    text_bounds,
                    line_height,
                    text,
                    span.range.clone(),
                    text_align,
                ) {
                    let (line_idx, _) = wrapped_line_for_y(
                        &lines, line_height, segment.top() - text_bounds.top(),
                    ).expect("code fragment belongs to a shaped line");
                    let layout = &lines[line_idx];
                    let row_idx = ((segment.top() - text_bounds.top() - wrapped_line_top(&lines, line_height, line_idx)) / line_height) as usize;
                    let row_end = wrap_boundary_offset(layout, row_idx).unwrap_or(layout.len());
                    let end = span.range.end.saturating_sub(hard_ranges[line_idx].start).min(row_end);
                    let code_run = layout.runs().iter().find(|run| run.font_size.is_some())
                        .expect("inline code has a font-size run");
                    let code_size = code_run.font_size.unwrap();
                    let code_ascent = cx.text_system().ascent(code_run.font_id, code_size);
                    let code_descent = px(f32::from(cx.text_system().descent(code_run.font_id, code_size)).abs());
                    let baseline = (line_height - layout.ascent() - layout.descent()) / 2.0 + layout.ascent();
                    let mut quad_bounds = inline_code_background_bounds(
                        segment, baseline, code_ascent, code_descent, point(pad_x, pad_y),
                    );
                    quad_bounds.size.width -= layout.spacing_before(end);
                    code_quads.push({
                        let mut q = fill(quad_bounds, code_color);
                        q.corner_radii = Corners::all(radius);
                        q
                    });
                }
            }
        }

        // In-document search matches (roadmap B2): translucent quads under
        // the text, computed like selection segments.
        let mut search_highlights = Vec::new();
        if !input.search_highlight_ranges.is_empty() {
            let highlight_color = theme.colors.search_highlight_bg;
            let text = input.display_text();
            for range in &input.search_highlight_ranges {
                for segment in range_segment_bounds(
                    &lines,
                    text_bounds,
                    line_height,
                    text,
                    range.clone(),
                    text_align,
                ) {
                    let mut quad = fill(segment, highlight_color);
                    quad.corner_radii = Corners::all(px(2.0));
                    search_highlights.push(quad);
                }
            }
        }

        PrepaintState {
            lines,
            source_line_numbers,
            source_line_number_gutter_width,
            cursor: cursor_quad,
            selection: selection_quads,
            code_backgrounds: code_quads,
            search_highlights,
            line_height,
            hitbox,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let (focus_handle, hovering_link) = {
            let input = self.input.read(cx);
            let text_bounds = source_text_bounds(bounds, prepaint.source_line_number_gutter_width);
            let hovering_link = !self.is_placeholder
                && !input.is_source_raw_mode()
                && prepaint.hitbox.is_hovered(window)
                && link_at_position(
                    input,
                    &prepaint.lines,
                    text_bounds,
                    prepaint.line_height,
                    window.mouse_position(),
                )
                .is_some();
            (input.focus_handle.clone(), hovering_link)
        };

        if hovering_link {
            // 悬停在链接上就显示小手，提示可点击；Cmd/Ctrl+点击仍由
            // `on_mouse_up` 负责跟随链接。
            window.set_cursor_style(CursorStyle::PointingHand, &prepaint.hitbox);
        }

        if focus_handle.is_focused(window) {
            let text_bounds = source_text_bounds(bounds, prepaint.source_line_number_gutter_width);
            window.handle_input(
                &focus_handle,
                ElementInputHandler::new(text_bounds, self.input.clone()),
                cx,
            );
        }

        // Paint code backgrounds behind text.
        for code_bg in prepaint.code_backgrounds.drain(..) {
            window.paint_quad(code_bg);
        }

        // Search matches sit above code backgrounds and below the selection.
        for highlight in prepaint.search_highlights.drain(..) {
            window.paint_quad(highlight);
        }

        for selection in prepaint.selection.drain(..) {
            window.paint_quad(selection);
        }

        let line_height = prepaint.line_height;
        let lines = std::mem::take(&mut prepaint.lines);
        let text_align = self.input.read(cx).text_align();
        let text_bounds = source_text_bounds(bounds, prepaint.source_line_number_gutter_width);
        let line_number_tops = source_line_number_tops(&lines, line_height);
        let line_number_gap = px(SOURCE_LINE_NUMBER_GAP);
        let line_numbers = std::mem::take(&mut prepaint.source_line_numbers);
        for (line_number, y_offset) in line_numbers.iter().zip(line_number_tops.iter()) {
            let line_number_width = line_number.x_for_index(line_number.len());
            line_number
                .paint(
                    point(
                        text_bounds.left() - line_number_gap - line_number_width,
                        bounds.origin.y + *y_offset,
                    ),
                    line_height,
                    window,
                    cx,
                )
                .ok();
        }

        let mut y_offset = Pixels::default();
        for line in lines.iter() {
            let origin_x = aligned_line_left(line, text_bounds, text_align);
            line.paint(
                point(origin_x, text_bounds.origin.y + y_offset),
                line_height,
                TextAlign::Left,
                None,
                window,
                cx,
            )
            .ok();
            y_offset += wrapped_line_height(line, line_height);
        }

        if focus_handle.is_focused(window)
            && let Some(cursor) = prepaint.cursor.take()
        {
            window.paint_quad(cursor);
        }

        self.input.update(cx, |input, _cx| {
            input.last_layout = Some((*lines).clone());
            input.last_bounds = Some(text_bounds);
            input.last_line_height = line_height;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{
        link_at_position, source_line_number_gutter_width, source_line_number_tops,
        source_text_bounds, wrapped_line_height,
    };
    use crate::components::{Block, BlockKind, BlockRecord, InlineTextTree, TableCellPosition};
    use gpui::{
        AppContext, Bounds, Hsla, Modifiers, MouseButton, MouseDownEvent, SharedString,
        TestAppContext, TextAlign, TextRun, VisualTestContext, font, point, px, rgba, size,
    };

    #[test]
    fn prose_wrapping_preserves_punctuation_and_graphemes() {
        use unicode_segmentation::UnicodeSegmentation;
        let text = "alpha! commas, periods. questions? semicolons; colons: (closing) [bracket] {brace} 中文（右括号）👩‍💻e\u{301} words";
        let breaks = super::prose_line_breaks(text);
        let graphemes: Vec<_> = text.grapheme_indices(true).map(|(ix, _)| ix).collect();
        assert!(!breaks.contains(&1), "普通英文单词内部不应断行");
        for ix in breaks.into_iter().filter(|ix| *ix > 0 && *ix < text.len()) {
            assert!(graphemes.contains(&ix), "不能拆开 emoji 或组合字符");
            let first = text[ix..].chars().next().unwrap();
            assert!(
                !"!！,，.。?？;；:：)]}）】”’".contains(first),
                "成熟断行器也必须应用严格标点禁则：{first:?}"
            );
        }
    }

    #[test]
    fn wrapped_inline_code_backgrounds_leave_space_between_rows() {
        let row = Bounds::new(point(px(0.0), px(0.0)), size(px(160.0), px(28.0)));
        let next_row = Bounds::new(point(px(0.0), px(28.0)), row.size);
        let first = super::inline_code_background_bounds(row, px(22.0), px(13.0), px(4.0), point(px(2.0), px(1.0)));
        let second = super::inline_code_background_bounds(next_row, px(22.0), px(13.0), px(4.0), point(px(2.0), px(1.0)));
        assert!(first.bottom() < second.top(), "多行代码背景不能粘连");
        assert!(first.size.height >= px(17.0), "背景必须覆盖代码字形");
        assert!(first.size.height < row.size.height, "背景不能使用整行高度");
    }

    #[test]
    fn prose_spacing_keeps_code_fixed_and_adds_margins_and_autospace() {
        let text = "中文hello文 abc";
        let code = "中文".len().."中文hello".len();
        let spacing = super::prose_spacing(text, &[code.clone()], px(0.25), px(4.0), px(4.0));
        assert!(spacing.contains(&(code.start, px(8.0))), "代码前应保留 margin/padding 和中西间距");
        assert!(spacing.contains(&(code.end, px(8.0))), "代码后应保留 margin/padding 和中西间距");
        assert!(!spacing.iter().any(|(index, _)| code.start < *index && *index < code.end), "代码内部仍须等宽");
        assert!(spacing.contains(&(text.len() - 1, px(0.25))), "正文应有独立字距");
    }

    #[gpui::test]
    async fn prose_spacing_keeps_text_indices_and_hit_testing(cx: &mut TestAppContext) {
        use unicode_segmentation::UnicodeSegmentation;
        let cx = cx.add_empty_window();
        let text = "中文hello世界 abc continuation";
        let mut lines = shaped_lines(text, px(130.0), cx);
        let indices: Vec<_> = lines[0].runs().iter().flat_map(|run| run.glyphs.iter().map(|glyph| glyph.index)).collect();
        let spacing = super::prose_spacing(text, &[], px(0.3), px(4.0), px(4.0));
        lines[0].add_horizontal_spacing(&spacing);
        let emergency: Vec<_> = text.grapheme_indices(true).map(|(index, _)| index).collect();
        lines[0].wrap_at_boundaries(&super::prose_line_breaks(text), &emergency);
        assert_eq!(lines[0].text.as_ref(), text, "排版不应写入空格");
        assert_eq!(indices, lines[0].runs().iter().flat_map(|run| run.glyphs.iter().map(|glyph| glyph.index)).collect::<Vec<_>>());
        for index in indices {
            if lines[0].wrap_boundaries().iter().any(|boundary| lines[0].runs()[boundary.run_ix].glyphs[boundary.glyph_ix].index == index) { continue; }
            let position = lines[0].position_for_index(index, px(28.0)).unwrap();
            assert_eq!(lines[0].closest_index_for_position(position, px(28.0)).unwrap(), index, "字距变化后点击位置必须仍匹配原文");
        }
    }

    fn shaped_lines(
        text: &str,
        width: gpui::Pixels,
        cx: &mut VisualTestContext,
    ) -> Vec<gpui::WrappedLine> {
        cx.update(|window, _app| {
            window
                .text_system()
                .shape_text(
                    text.to_string().into(),
                    px(16.0),
                    &[TextRun {
                        len: text.len(),
                        font: font(".SystemUIFont"),
                        color: Hsla::from(rgba(0xffffffff)),
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                        font_size: None,
                    }],
                    Some(width),
                    None,
                )
                .expect("text should shape")
                .into_vec()
        })
    }

    #[test]
    fn source_line_number_gutter_grows_with_digit_count() {
        let one_digit = source_line_number_gutter_width(9, px(16.0));
        let two_digits = source_line_number_gutter_width(10, px(16.0));
        let three_digits = source_line_number_gutter_width(100, px(16.0));

        assert_eq!(one_digit, two_digits);
        assert!(three_digits > two_digits);
    }

    #[test]
    fn source_text_bounds_are_offset_by_gutter_width() {
        let bounds = Bounds::new(point(px(10.0), px(20.0)), size(px(300.0), px(120.0)));
        let text_bounds = source_text_bounds(bounds, px(48.0));

        assert_eq!(text_bounds.left(), px(58.0));
        assert_eq!(text_bounds.top(), px(20.0));
        assert_eq!(text_bounds.size.width, px(252.0));
        assert_eq!(text_bounds.size.height, px(120.0));
    }

    #[gpui::test]
    async fn source_line_number_tops_follow_soft_wrapped_hard_lines(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        let lines = shaped_lines(
            "this line should wrap before the next hard line\nsecond",
            px(92.0),
            cx,
        );
        assert!(
            !lines[0].wrap_boundaries().is_empty(),
            "first hard line should soft-wrap"
        );

        let tops = source_line_number_tops(&lines, px(20.0));
        assert_eq!(tops[0], px(0.0));
        assert_eq!(tops[1], wrapped_line_height(&lines[0], px(20.0)));
    }

    #[gpui::test]
    async fn link_hit_matches_only_rendered_link_text(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        let block = cx.new(|cx| {
            Block::with_record(
                cx,
                BlockRecord::new(
                    BlockKind::Paragraph,
                    InlineTextTree::from_markdown("[link](https://example.com)"),
                ),
            )
        });

        let display_text = block.read_with(cx, |block, _cx| block.display_text().to_string());
        let lines = shaped_lines(&display_text, px(320.0), cx);
        let (hit, miss_right) = block.read_with(cx, |block, _cx| {
            let bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(320.0), px(20.0)));
            let span = block
                .inline_spans()
                .iter()
                .find(|span| span.link.is_some())
                .expect("link span should exist");
            let layout = &lines[0];
            let start = layout
                .position_for_index(span.range.start, px(20.0))
                .expect("start position");
            let end = layout
                .position_for_index(span.range.end, px(20.0))
                .expect("end position");
            let hit = point((start.x + end.x) / 2.0, px(10.0));
            let miss_right = point(end.x + px(24.0), px(10.0));
            (
                link_at_position(block, &lines, bounds, px(20.0), hit)
                    .map(|link| link.open_target.clone()),
                link_at_position(block, &lines, bounds, px(20.0), miss_right)
                    .map(|link| link.open_target.clone()),
            )
        });

        assert_eq!(hit, Some("https://example.com".to_string()));
        assert_eq!(miss_right, None);
    }

    #[gpui::test]
    async fn secondary_click_follows_link_while_plain_click_edits(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        let block = cx.new(|cx| {
            Block::with_record(
                cx,
                BlockRecord::new(
                    BlockKind::Paragraph,
                    InlineTextTree::from_markdown("a [link](https://example.com) bbbb"),
                ),
            )
        });

        let display_text = block.read_with(cx, |block, _cx| block.display_text().to_string());
        let lines = shaped_lines(&display_text, px(320.0), cx);
        let bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(320.0), px(20.0)));

        let link_position = block.read_with(cx, |block, _cx| {
            let span = block
                .inline_spans()
                .iter()
                .find(|span| span.link.is_some())
                .expect("link span should exist");
            let layout = &lines[0];
            let start = layout
                .position_for_index(span.range.start, px(20.0))
                .expect("start position");
            let end = layout
                .position_for_index(span.range.end, px(20.0))
                .expect("end position");
            point((start.x + end.x) / 2.0, px(10.0))
        });

        block.update(cx, |block, _cx| {
            block.last_layout = Some(lines.clone());
            block.last_bounds = Some(bounds);
            block.last_line_height = px(20.0);
            block.selected_range = 0..0;
        });

        let mut event = MouseDownEvent {
            button: MouseButton::Left,
            position: link_position,
            modifiers: Modifiers::default(),
            click_count: 1,
            first_mouse: false,
        };

        // A plain click on the link moves the caret into the text for editing.
        cx.update(|window, app| {
            block.update(app, |block, cx| block.on_mouse_down(&event, window, cx));
        });
        block.read_with(cx, |block, _cx| {
            assert_ne!(block.selected_range, 0..0);
        });

        // Cmd/Ctrl+click follows the link instead: the caret is left untouched
        // and no drag-selection begins.
        block.update(cx, |block, _cx| block.selected_range = 0..0);
        event.modifiers = Modifiers::secondary_key();
        cx.update(|window, app| {
            block.update(app, |block, cx| block.on_mouse_down(&event, window, cx));
        });
        block.read_with(cx, |block, _cx| {
            assert_eq!(block.selected_range, 0..0);
            assert!(!block.is_selecting);
        });
    }

    #[gpui::test]
    async fn link_hit_respects_center_alignment(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        let block = cx.new(|cx| {
            let mut block = Block::with_record(
                cx,
                BlockRecord::new(
                    BlockKind::Paragraph,
                    InlineTextTree::from_markdown("[link](https://example.com)"),
                ),
            );
            block.set_table_cell_mode(
                TableCellPosition { row: 0, column: 0 },
                crate::components::TableColumnAlignment::Center,
            );
            block
        });

        let display_text = block.read_with(cx, |block, _cx| block.display_text().to_string());
        let lines = shaped_lines(&display_text, px(240.0), cx);
        let (miss_left, hit_center) = block.read_with(cx, |block, _cx| {
            let bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(240.0), px(20.0)));
            let span = block
                .inline_spans()
                .iter()
                .find(|span| span.link.is_some())
                .expect("link span should exist");
            let layout = &lines[0];
            let origin_x = super::aligned_line_left(layout, bounds, block.text_align());
            let start = layout
                .position_for_index(span.range.start, px(20.0))
                .expect("start position");
            let end = layout
                .position_for_index(span.range.end, px(20.0))
                .expect("end position");
            let miss_left = point(origin_x - px(12.0), px(10.0));
            let hit_center = point(origin_x + (start.x + end.x) / 2.0, px(10.0));
            (
                link_at_position(block, &lines, bounds, px(20.0), miss_left)
                    .map(|link| link.open_target.clone()),
                link_at_position(block, &lines, bounds, px(20.0), hit_center)
                    .map(|link| link.open_target.clone()),
            )
        });

        assert_eq!(miss_left, None);
        assert_eq!(hit_center, Some("https://example.com".to_string()));
    }

    #[gpui::test]
    async fn text_runs_apply_inline_html_color_and_background(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        let block = cx.new(|cx| {
            Block::with_record(
                cx,
                BlockRecord::new(
                    BlockKind::Paragraph,
                    InlineTextTree::from_markdown(
                        "before <span style='color:blue;background-color:#ff0'>marked</span>",
                    ),
                ),
            )
        });

        block.read_with(cx, |block, _cx| {
            let display_text: SharedString = block.display_text().to_string().into();
            let base_run = TextRun {
                len: display_text.len(),
                font: font(".SystemUIFont"),
                color: Hsla::from(rgba(0xffffffff)),
                background_color: None,
                underline: None,
                strikethrough: None,
                font_size: None,
            };
            let runs = super::build_text_runs(
                block,
                &display_text,
                &base_run,
                px(1.0),
                Hsla::from(rgba(0x0066ccff)),
                Hsla::from(rgba(0x111111ff)),
                true,
                "Menlo",
                px(13.0),
            );
            let marked_run = runs.last().expect("styled text should create a final run");

            assert_eq!(block.display_text(), "before marked");
            assert_eq!(marked_run.len, "marked".len());
            assert_eq!(marked_run.color, Hsla::from(rgba(0x0000ffff)));
            assert_eq!(
                marked_run.background_color,
                Some(Hsla::from(rgba(0xffff00ff)))
            );
        });
    }

    #[gpui::test]
    async fn inline_code_runs_use_the_code_font_size(cx: &mut TestAppContext) {
        // 用户报修：点击含行内代码的行，代码字号跳回正文。这要求可编辑文本
        // 也带上逐段字号（vendored gpui 本地补丁），且行内代码跟随
        // 「代码块字体大小」设置——显示态（分段路径）与编辑态（本路径）同值。
        let cx = cx.add_empty_window();
        let block = cx.new(|cx| {
            Block::with_record(
                cx,
                BlockRecord::new(
                    BlockKind::Paragraph,
                    InlineTextTree::from_markdown("plain `code` tail"),
                ),
            )
        });

        cx.update(|window, app| {
            let display_text: SharedString = block.read(app).display_text().to_string().into();
            let base_run = TextRun {
                len: display_text.len(),
                font: font(".SystemUIFont"),
                color: Hsla::from(rgba(0xffffffff)),
                background_color: None,
                underline: None,
                strikethrough: None,
                font_size: None,
            };
            let runs = block.read_with(app, |block, _| {
                super::build_text_runs(
                    block,
                    &display_text,
                    &base_run,
                    px(1.0),
                    Hsla::from(rgba(0x0066ccff)),
                    Hsla::from(rgba(0x111111ff)),
                    true,
                    "Menlo",
                    px(13.0),
                )
            });

            let code_run = runs
                .iter()
                .find(|run| run.font_size.is_some())
                .expect("行内代码段应带字号覆盖");
            assert_eq!(code_run.len, "code".len());
            assert_eq!(code_run.font_size, Some(px(13.0)));
            assert!(
                runs.iter().filter(|run| run.font_size.is_none()).count() >= 2,
                "代码段两侧的正文段不带字号覆盖"
            );

            let lines = window
                .text_system()
                .shape_text(display_text.clone(), px(16.0), &runs, None, None)
                .expect("text should shape");
            let layout = &lines[0];
            let code_shaped = layout
                .runs()
                .iter()
                .find(|run| run.font_size == Some(px(13.0)))
                .expect("整形结果应保留代码段字号");
            let body_shaped = layout
                .runs()
                .iter()
                .find(|run| run.font_size.is_none())
                .expect("整形结果应保留正文段");

            let advance = |run: &gpui::ShapedRun| {
                let first = run.glyphs.first().expect("run should have glyphs");
                let last = run.glyphs.last().expect("run should have glyphs");
                (last.position.x - first.position.x) / (run.glyphs.len().max(2) - 1) as f32
            };
            assert!(
                advance(code_shaped) < advance(body_shaped),
                "代码段步进 {:?} 应小于正文段 {:?}",
                advance(code_shaped),
                advance(body_shaped)
            );
        });
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn mac_platform_text_system_applies_per_run_font_size() {
        // 真实 CoreText 的逐段字号：步进必须按字号缩放，且 ShapedRun 要带上字号
        // 供绘制取用（测试平台的 NoopTextSystem 只是模拟这条契约）。
        let run_with = |len: usize, family: &'static str, font_size: Option<gpui::Pixels>| TextRun {
            len,
            font: font(family),
            color: Hsla::from(rgba(0xffffffff)),
            background_color: None,
            underline: None,
            strikethrough: None,
            font_size,
        };
        let text = "plain code";
        let runs = [
            run_with("plain ".len(), "Menlo", None),
            run_with("code".len(), "Menlo", Some(px(13.0))),
        ];
        let layout = gpui::shape_line_with_platform_text_system(text.into(), px(16.0), &runs);

        let advance = |run: &gpui::ShapedRun| {
            let first = run.glyphs.first().expect("run should have glyphs");
            let last = run.glyphs.last().expect("run should have glyphs");
            (last.position.x - first.position.x) / (run.glyphs.len().max(2) - 1) as f32
        };
        let code_run = layout
            .runs
            .iter()
            .find(|run| run.font_size == Some(px(13.0)) && !run.glyphs.is_empty())
            .expect("CoreText 排版结果应保留逐段字号");
        assert_eq!(code_run.glyphs.len(), "code".len());
        let body_run = layout
            .runs
            .iter()
            .find(|run| run.font_size.is_none() && !run.glyphs.is_empty())
            .expect("正文段不带字号覆盖");
        assert!(
            advance(code_run) < advance(body_run) * 0.9,
            "13px 代码段步进 {:?} 应明显小于 16px 正文段步进 {:?}",
            advance(code_run),
            advance(body_run)
        );
    }

    #[test]
    fn vendored_text_system_keeps_the_per_run_font_size_patch() {
        // 行内代码跟随「代码块字体大小」依赖 vendored gpui 的本地补丁：逐段字号
        // 必须一路走到各平台排版层。mac 这条路径需要 Metal 工具链才能编译测试
        // （本机没有），这里守住补丁本身，升级 vendored 副本时不会被悄悄覆盖。
        let mac = include_str!("../../../vendor/gpui/src/platform/mac/text_system.rs");
        assert!(
            mac.contains("run.font_size.unwrap_or(font_size)"),
            "mac 排版层应把逐段字号用于字体实例"
        );
        assert!(
            mac.contains("clone_with_font_size(run_font_size.into())"),
            "mac 排版层应按逐段字号创建 CTFont"
        );
        assert!(
            mac.contains("font_size: run_font_size"),
            "mac 排版层应把逐段字号写回 ShapedRun"
        );

        let windows = include_str!("../../../vendor/gpui/src/platform/windows/direct_write.rs");
        assert!(
            windows.contains("run.font_size.unwrap_or(font_size).0"),
            "windows 排版层应把逐段字号写进 DirectWrite 布局"
        );
        assert!(
            windows.contains("Some(px(glyphrun.fontEmSize))"),
            "windows 绘制层应把逐段字号写回 ShapedRun"
        );
    }

    #[gpui::test]
    async fn soft_wrapped_range_segments_stay_within_wrap_width(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        let text = "abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz";
        let lines = shaped_lines(text, px(80.0), cx);
        assert!(
            !lines[0].wrap_boundaries().is_empty(),
            "test text should soft-wrap"
        );

        let bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(80.0), px(120.0)));
        let segments = super::range_segment_bounds(
            &lines,
            bounds,
            px(20.0),
            text,
            0..text.len(),
            TextAlign::Left,
        );

        assert!(segments.len() > 1);
        for segment in segments {
            assert!(segment.left() >= bounds.left());
            assert!(segment.right() <= bounds.right() + px(0.5));
        }
    }

    #[gpui::test]
    async fn wrapped_link_hit_matches_only_visible_segments(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        let label = "abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz";
        let block = cx.new(|cx| {
            Block::with_record(
                cx,
                BlockRecord::new(
                    BlockKind::Paragraph,
                    InlineTextTree::from_markdown(&format!("[{label}](https://example.com)")),
                ),
            )
        });

        let display_text = block.read_with(cx, |block, _cx| block.display_text().to_string());
        let lines = shaped_lines(&display_text, px(80.0), cx);
        assert!(
            !lines[0].wrap_boundaries().is_empty(),
            "link text should soft-wrap"
        );

        let (hit, miss_right) = block.read_with(cx, |block, _cx| {
            let bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(80.0), px(120.0)));
            let span = block
                .inline_spans()
                .iter()
                .find(|span| span.link.is_some())
                .expect("link span should exist");
            let segments = super::range_segment_bounds(
                &lines,
                bounds,
                px(20.0),
                &display_text,
                span.range.clone(),
                block.text_align(),
            );
            assert!(segments.len() > 1);
            let second_segment = segments[1];
            let hit = point(
                (second_segment.left() + second_segment.right()) / 2.0,
                (second_segment.top() + second_segment.bottom()) / 2.0,
            );
            let miss_right = point(second_segment.right() + px(24.0), hit.y);
            (
                link_at_position(block, &lines, bounds, px(20.0), hit)
                    .map(|link| link.open_target.clone()),
                link_at_position(block, &lines, bounds, px(20.0), miss_right)
                    .map(|link| link.open_target.clone()),
            )
        });

        assert_eq!(hit, Some("https://example.com".to_string()));
        assert_eq!(miss_right, None);
    }

    #[gpui::test]
    async fn wrapped_hard_line_top_accumulates_soft_wrap_height(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        let text = "abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz\nnext";
        let lines = shaped_lines(text, px(80.0), cx);
        assert_eq!(lines.len(), 2);
        assert!(
            !lines[0].wrap_boundaries().is_empty(),
            "first hard line should soft-wrap"
        );

        let first_height = lines[0].size(px(20.0)).height;
        assert!(first_height > px(20.0));
        assert_eq!(super::wrapped_line_top(&lines, px(20.0), 1), first_height);
    }
}
