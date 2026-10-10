//! Native Markdown table data model plus parse and serialize helpers.
//!
//! Tables are supported as native blocks at the root level and inside
//! quote-like containers in rendered mode. More complex nested contexts that
//! are still outside the runtime-safe subset continue to use raw-Markdown
//! fallback paths.

use gpui::{App, Entity, FontStyle, FontWeight, Pixels, SharedString, TextRun, Window, px};

use crate::components::{Block, InlineTextTree};
use crate::config::preferences::FontPreferences;
use crate::theme::Theme;

/// Horizontal alignment declared by the table's delimiter row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableColumnAlignment {
    /// No explicit alignment marker (`---`). Renders left, but stays distinct
    /// from [`Left`](Self::Left) so an unmarked column is not silently rewritten
    /// with a leading colon on the next serialize.
    Default,
    /// Explicit left alignment (`:---`).
    Left,
    /// Center-aligned cells (`:---:`).
    Center,
    /// Right-aligned cells (`---:`).
    Right,
}

/// Axis kinds addressable by rendered-mode native table maintenance UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableAxisKind {
    /// Table row axis.
    Row,
    /// Table column axis.
    Column,
}

/// A row or column marker inside one native table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TableAxisMarker {
    pub kind: TableAxisKind,
    pub index: usize,
}

/// Visual emphasis level used when previewing or selecting table axes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TableAxisHighlight {
    /// No axis emphasis.
    #[default]
    None,
    /// Hover preview emphasis.
    Preview,
    /// Persistent selected-axis emphasis.
    Selected,
}

/// Persistent cell contents for a native table block.
#[derive(Debug, Clone)]
pub struct TableData {
    pub header: Vec<InlineTextTree>,
    pub rows: Vec<Vec<InlineTextTree>>,
    pub alignments: Vec<TableColumnAlignment>,
}

impl PartialEq for TableData {
    fn eq(&self, other: &Self) -> bool {
        self.header == other.header
            && self.rows == other.rows
            && self.alignments == other.alignments
    }
}

impl Eq for TableData {}

impl TableData {
    /// Creates an empty table with one header row, `body_rows` body rows, and
    /// `columns` left-aligned columns.
    pub fn new_empty(body_rows: usize, columns: usize) -> Self {
        let columns = columns.max(1);
        let header = (0..columns)
            .map(|_| InlineTextTree::plain(String::new()))
            .collect::<Vec<_>>();
        let rows = (0..body_rows.max(1))
            .map(|_| {
                (0..columns)
                    .map(|_| InlineTextTree::plain(String::new()))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let alignments = vec![TableColumnAlignment::Default; columns];
        Self {
            header,
            rows,
            alignments,
        }
    }

    pub(crate) fn column_count(&self) -> usize {
        self.header
            .len()
            .max(self.alignments.len())
            .max(self.rows.iter().map(Vec::len).max().unwrap_or(0))
            .max(1)
    }

    fn normalize_shape(&mut self) {
        let columns = self.column_count();
        while self.header.len() < columns {
            self.header.push(InlineTextTree::plain(String::new()));
        }
        while self.alignments.len() < columns {
            self.alignments.push(TableColumnAlignment::Default);
        }
        for row in &mut self.rows {
            while row.len() < columns {
                row.push(InlineTextTree::plain(String::new()));
            }
        }
    }

    /// Appends one empty body row while preserving the current column count.
    pub fn append_row(&mut self) {
        self.normalize_shape();
        let columns = self.column_count();
        self.rows.push(
            (0..columns)
                .map(|_| InlineTextTree::plain(String::new()))
                .collect(),
        );
    }

    /// Appends one empty column to the header and every body row.
    pub fn append_column(&mut self, alignment: TableColumnAlignment) {
        self.normalize_shape();
        self.header.push(InlineTextTree::plain(String::new()));
        self.alignments.push(alignment);
        for row in &mut self.rows {
            row.push(InlineTextTree::plain(String::new()));
        }
    }

    /// Sets the alignment of one column if it exists.
    pub fn set_column_alignment(&mut self, column: usize, alignment: TableColumnAlignment) {
        self.normalize_shape();
        if let Some(slot) = self.alignments.get_mut(column) {
            *slot = alignment;
        }
    }

    /// Swaps two rows addressed by their visual index, where row `0` is the
    /// header and rows `1..=rows.len()` are the body rows. Swapping the header
    /// with the first body row exchanges header and body content, mirroring how
    /// the row handles treat the header as just another movable row.
    pub fn swap_visual_rows(&mut self, row_a: usize, row_b: usize) {
        self.normalize_shape();
        let total = self.rows.len() + 1;
        if row_a >= total || row_b >= total || row_a == row_b {
            return;
        }
        match (row_a, row_b) {
            (0, other) | (other, 0) => {
                std::mem::swap(&mut self.header, &mut self.rows[other - 1]);
            }
            (a, b) => self.rows.swap(a - 1, b - 1),
        }
    }

    /// Swaps two columns across header, body, and alignment vectors.
    pub fn swap_columns(&mut self, col_a: usize, col_b: usize) {
        self.normalize_shape();
        let columns = self.column_count();
        if col_a >= columns || col_b >= columns || col_a == col_b {
            return;
        }

        self.header.swap(col_a, col_b);
        self.alignments.swap(col_a, col_b);
        for row in &mut self.rows {
            row.swap(col_a, col_b);
        }
    }

    /// Removes one body row while preserving at least one body row.
    pub fn remove_body_row(&mut self, row_index: usize) {
        self.normalize_shape();
        if row_index >= self.rows.len() {
            return;
        }
        // A table may be left header-only; the editor removes the whole block
        // when the header itself is then deleted.
        self.rows.remove(row_index);
    }

    /// Removes the header row by promoting the first body row into its place.
    /// Returns false (leaving the table unchanged) when there are no body rows,
    /// since a pipe table must keep a header row.
    pub fn remove_header_row(&mut self) -> bool {
        self.normalize_shape();
        if self.rows.is_empty() {
            return false;
        }
        self.header = self.rows.remove(0);
        true
    }

    /// Removes one column while preserving at least one column.
    pub fn remove_column(&mut self, col_index: usize) {
        self.normalize_shape();
        let columns = self.column_count();
        if columns <= 1 || col_index >= columns {
            return;
        }

        self.header.remove(col_index);
        self.alignments.remove(col_index);
        for row in &mut self.rows {
            row.remove(col_index);
        }
    }
}

/// 内容窄的列钉宽时多给的容差（像素）。测量与渲染的字体度量不可能逐像素一致
/// （同族字体的版本差异、hinting、像素取整、比例换算的舍入），按用户建议留
/// 10px 容差兜住这类抖动；不会超过该列当前的均分份额。
const EXTRA_COLUMN_SLACK: f32 = 10.0;

/// Responsive width fractions shared by every row of a native table.
#[derive(Debug, Clone, PartialEq)]
pub struct TableColumnLayout {
    fractions: Vec<f32>,
}

impl TableColumnLayout {
    pub fn equal(column_count: usize) -> Self {
        let column_count = column_count.max(1);
        let fraction = 1.0 / column_count as f32;
        Self {
            fractions: vec![fraction; column_count],
        }
    }

    #[cfg(test)]
    pub(crate) fn fractions(&self) -> &[f32] {
        &self.fractions
    }

    pub fn fraction(&self, column: usize) -> f32 {
        self.fractions
            .get(column)
            .copied()
            .unwrap_or_else(|| 1.0 / self.fractions.len().max(1) as f32)
    }

    pub fn measure(
        table: &TableData,
        table_width: f32,
        window: &mut Window,
        theme: &Theme,
        cx: &App,
    ) -> Self {
        // 列宽/换行按字号估算，必须用与绘制一致的字号（含界面缩放）。
        let fonts = crate::config::EditorSettings::scaled_fonts(cx);
        let preferred_widths = measure_preferred_column_widths(
            table,
            window,
            theme,
            &fonts,
            crate::config::EditorSettings::show_table_headers(cx),
        )
        .into_iter()
        .map(f32::from)
        .collect::<Vec<_>>();
        Self::from_preferred_widths(&preferred_widths, table_width, minimum_column_width(theme))
    }

    pub fn from_preferred_widths(
        preferred_widths: &[f32],
        table_width: f32,
        min_column_width: f32,
    ) -> Self {
        if preferred_widths.is_empty() {
            return Self::equal(1);
        }

        let column_count = preferred_widths.len();
        let safe_table_width = table_width.max(1.0);
        let equal_share = safe_table_width / column_count as f32;

        // 水位法（用户方案）：先把空间平分，然后把内容装得下的列钉在「内容宽 +
        // 一点点」上，钉住后腾出的空间在还缺空间的列之间重新平分，反复迭代。
        //
        // 两种直觉做法都是错的：按内容宽度做权重，一列内容特別长时会把权重全吃
        // 走，其余列被压到只剩几个字符；把内容窄的列拉到平均份额，它白白占着空
        // 位，宽列反倒被压到内容宽度以下换行（用户报修）。
        let floor_width = min_column_width.max(0.0).min(equal_share);
        let mut widths = vec![floor_width; column_count];
        let mut remaining = (safe_table_width - floor_width * column_count as f32).max(0.0);
        let mut pending = (0..column_count).collect::<Vec<_>>();

        while !pending.is_empty() && remaining > f32::EPSILON {
            let share = remaining / pending.len() as f32;
            let squeezed = pending
                .iter()
                .any(|index| preferred_widths[*index] > widths[*index] + share + f32::EPSILON);
            if !squeezed {
                // 没有列再缺空间：剩余空间平分给还没定宽的列，表格铺满容器宽度。
                for index in &pending {
                    widths[*index] += share;
                }
                remaining = 0.0;
                break;
            }

            // 内容塞得进当前份额的列钉住：内容宽 + 一点点余量，但不超过自己的份额。
            let mut satisfied = Vec::new();
            for index in &pending {
                let capacity = (preferred_widths[*index] - widths[*index]).max(0.0);
                if capacity <= share + f32::EPSILON {
                    satisfied.push(*index);
                    let slack = (share - capacity).min(EXTRA_COLUMN_SLACK).max(0.0);
                    widths[*index] += capacity + slack;
                    remaining -= capacity + slack;
                }
            }
            if satisfied.is_empty() {
                // 没有列能被完全满足：剩下的列平分剩余空间，内容都会换行。
                for index in &pending {
                    widths[*index] += share;
                }
                remaining = 0.0;
                break;
            }
            pending.retain(|index| !satisfied.contains(index));
        }

        // 所有列都钉住后还有剩余：平分掉，表格铺满容器宽度。
        if remaining > f32::EPSILON {
            let share = remaining / column_count as f32;
            for width in &mut widths {
                *width += share;
            }
        }

        let assigned_sum = widths.iter().sum::<f32>();
        if assigned_sum <= f32::EPSILON {
            return Self::equal(column_count);
        }

        let fractions = widths.into_iter().map(|width| width / assigned_sum).collect();
        Self { fractions }
    }
}

/// Runtime-only location of a cell inside a native table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TableCellPosition {
    /// Zero-based visual row. Header is row `0`; first body row is `1`.
    pub row: usize,
    pub column: usize,
}

impl TableCellPosition {
    pub fn is_header(self) -> bool {
        self.row == 0
    }

    pub fn body_row_index(self) -> Option<usize> {
        self.row.checked_sub(1)
    }
}

/// Runtime cell editors attached to one native table block.
#[derive(Clone)]
pub struct TableRuntime {
    pub header: Vec<Entity<Block>>,
    pub rows: Vec<Vec<Entity<Block>>>,
}

impl TableRuntime {
    pub fn cell(&self, position: TableCellPosition) -> Option<Entity<Block>> {
        if position.is_header() {
            self.header.get(position.column).cloned()
        } else {
            self.rows
                .get(position.body_row_index()?)
                .and_then(|row| row.get(position.column))
                .cloned()
        }
    }
}

fn measure_preferred_column_widths(
    table: &TableData,
    window: &mut Window,
    theme: &Theme,
    fonts: &FontPreferences,
    style_headers: bool,
) -> Vec<Pixels> {
    let column_count = table.header.len().max(1);
    let mut preferred_widths = vec![Pixels::ZERO; column_count];

    for (column, cell) in table.header.iter().enumerate() {
        preferred_widths[column] = preferred_widths[column]
            .max(measure_cell_preferred_width(cell, style_headers, window, theme, fonts));
    }

    for row in &table.rows {
        for (column, cell) in row.iter().enumerate().take(column_count) {
            preferred_widths[column] = preferred_widths[column]
                .max(measure_cell_preferred_width(cell, false, window, theme, fonts));
        }
    }

    preferred_widths
}

fn measure_cell_preferred_width(
    cell: &InlineTextTree,
    is_header: bool,
    window: &mut Window,
    theme: &Theme,
    fonts: &FontPreferences,
) -> Pixels {
    let cache = cell.render_cache();
    let text = cache.visible_text();
    let cell_chrome_width = cell_chrome_width(theme);
    if text.is_empty() {
        return cell_chrome_width;
    }

    let display_text = SharedString::from(text.to_string());
    let mut font = window.text_style().font();
    if is_header && font.weight < FontWeight::BOLD {
        font.weight = FontWeight::BOLD;
    }
    let base_run = TextRun {
        len: display_text.len(),
        font,
        color: theme.colors.text_default,
        background_color: None,
        underline: None,
        strikethrough: None,
        font_size: None,
    };
    let runs = measurement_runs(&cache, &base_run, fonts);
    let font_size = px(theme.typography.text_size);

    let text_width = window
        .text_system()
        .shape_text(display_text, font_size, &runs, None, None)
        .ok()
        .map(|mut lines| {
            // 列宽必须按渲染管线算：正文渲染在折行前会给行加水平间距（行内代码
            // 两侧 code_gap、相邻字形的字距、中西文边界 autospacing）。测量不补
            // 上这部分，钉在「内容宽+余量」上的列渲染时必然 mid-word 折行
            // （用户报修）。
            let code_ranges: Vec<_> = cache
                .spans()
                .iter()
                .filter(|span| span.style.code)
                .map(|span| span.range.clone())
                .collect();
            let letter_spacing = font_size * theme.typography.text_letter_spacing;
            let code_gap = px(theme.dimensions.code_bg_pad_x) + font_size * 0.125;
            crate::components::block::element::add_render_spacing(
                &mut lines,
                &code_ranges,
                letter_spacing,
                code_gap,
                font_size,
            );
            lines
                .iter()
                .map(|line| line.width())
                .max()
                .unwrap_or(Pixels::ZERO)
        })
        .unwrap_or(Pixels::ZERO);

    text_width + cell_chrome_width
}

fn measurement_runs(
    cache: &crate::components::InlineRenderCache,
    base_run: &TextRun,
    fonts: &FontPreferences,
) -> Vec<TextRun> {
    let mut boundaries = vec![0, cache.visible_text().len()];
    for span in cache.spans() {
        boundaries.push(span.range.start);
        boundaries.push(span.range.end);
    }
    boundaries.sort_unstable();
    boundaries.dedup();

    let mut runs = Vec::new();
    for boundary_pair in boundaries.windows(2) {
        let start = boundary_pair[0];
        let end = boundary_pair[1];
        if start >= end {
            continue;
        }

        let inline_style = cache.style_at(start);
        let mut font = base_run.font.clone();
        if inline_style.code {
            // 哪段用什么字体渲染，就用什么字体量：行内代码段与渲染端
            // （build_text_runs）同样换 code 字体族、提到 Medium、用「代码块
            // 字体大小」。按正文字体量等宽 token 会系统性偏窄，钉住列渲染时
            // mid-word 折行（用户报修）。正文段仍按正文字体量。
            font.family = SharedString::from(fonts.code_family.clone());
            if font.weight < FontWeight::MEDIUM {
                font.weight = FontWeight::MEDIUM;
            }
        }
        if inline_style.bold && font.weight < FontWeight::BOLD {
            font.weight = FontWeight::BOLD;
        }
        if inline_style.italic {
            font.style = FontStyle::Italic;
        }

        runs.push(TextRun {
            len: end - start,
            font,
            color: base_run.color,
            background_color: None,
            underline: None,
            strikethrough: None,
            font_size: inline_style.code.then_some(px(fonts.code_size as f32)),
        });
    }

    if runs.is_empty() {
        vec![base_run.clone()]
    } else {
        runs
    }
}

fn cell_chrome_width(theme: &Theme) -> Pixels {
    px(theme.dimensions.table_cell_padding_x * 2.0 + 2.0)
}

fn minimum_column_width(theme: &Theme) -> f32 {
    theme.dimensions.table_cell_padding_x * 2.0 + theme.typography.text_size * 4.0 + 2.0
}

fn strip_table_indent(line: &str) -> Option<&str> {
    let indent = line.bytes().take_while(|b| *b == b' ').count();
    (indent <= 3).then_some(&line[indent..])
}

/// 一行里会**把单元格切开**的 `|` 的字节位置（相对 `text`）。
///
/// 切分尊重 markdown 上下文，两种情况不是分隔符：
/// - 代码段之内（反引号串成对定界；配对不成的串按字面处理）。代码段里的反斜杠
///   是字面字符，不参与转义判断；
/// - 代码段之外、前面是奇数个反斜杠（`\|` 是转义竖线，属于格内容）。
///
/// 只做**位置**判断，不做任何反转义——未转义是行内解析器在切完格之后的事，
/// 那时才知道每段是不是代码/公式。切分与序列化共用这一份扫描（
/// `escape_cell_separator_pipes`），两端才不会对「哪个竖线会切开」各说各话
/// （用户报修 cases/02-table-code.md：预处理反转义把 `` `a\\b` `` 显示成 `a\b`、
/// 把 `$a\|b$` 的转义竖线洗成裸竖线，改了文档的意思）。
fn cell_separator_positions(text: &str) -> Vec<usize> {
    let chars = text.chars().collect::<Vec<_>>();
    let byte_offsets = chars
        .iter()
        .scan(0usize, |offset, ch| {
            let current = *offset;
            *offset += ch.len_utf8();
            Some(current)
        })
        .collect::<Vec<_>>();
    let is_punctuation = |index: usize| -> bool {
        chars
            .get(index)
            .is_some_and(|ch| crate::components::markdown::inline::is_commonmark_escapable(*ch))
    };

    let mut separators = Vec::new();
    let mut index = 0usize;
    while index < chars.len() {
        match chars[index] {
            '\\' if is_punctuation(index + 1) => {
                index += 2;
            }
            '`' => {
                let mut run_end = index;
                while run_end < chars.len() && chars[run_end] == '`' {
                    run_end += 1;
                }
                let run_len = run_end - index;
                match find_closing_backtick_run(&chars, run_end, run_len) {
                    Some(close_start) => {
                        // 代码段整体跳过：段内不认转义、竖线不是分隔符。
                        index = close_start + run_len;
                    }
                    None => {
                        index = run_end;
                    }
                }
            }
            '|' => {
                separators.push(byte_offsets[index]);
                index += 1;
            }
            _ => {
                index += 1;
            }
        }
    }
    separators
}

/// 从 `start` 起找**恰好** `run_len` 个连续反引号的闭合串（CommonMark：长度不等
/// 不算闭合），返回闭合串起点。找到闭合之前反斜杠按「尚未确认在代码段内」处理：
/// `\` + 可转义字符吃掉下一位，与外层扫描同一判据。
fn find_closing_backtick_run(chars: &[char], mut start: usize, run_len: usize) -> Option<usize> {
    let is_punctuation = |index: usize| -> bool {
        chars
            .get(index)
            .is_some_and(|ch| crate::components::markdown::inline::is_commonmark_escapable(*ch))
    };
    while start < chars.len() {
        if chars[start] == '\\' && is_punctuation(start + 1) {
            start += 2;
            continue;
        }
        if chars[start] == '`' {
            let mut end = start;
            while end < chars.len() && chars[end] == '`' {
                end += 1;
            }
            if end - start == run_len {
                return Some(start);
            }
            start = end;
            continue;
        }
        start += 1;
    }
    None
}

/// 把序列化出来的格内容里**会被切开**的裸 `|` 补成 `\|`；代码段内的竖线不动
/// （切分本来就不在那里下刀）。已带奇数反斜杠的竖线原样保留。
fn escape_cell_separator_pipes(markdown: &str) -> String {
    let separators = cell_separator_positions(markdown);
    let mut output = String::with_capacity(markdown.len() + separators.len());
    let mut cursor = 0usize;
    for position in separators {
        output.push_str(&markdown[cursor..position]);
        output.push('\\');
        output.push('|');
        cursor = position + 1;
    }
    output.push_str(&markdown[cursor..]);
    output
}

fn split_table_cells(line: &str) -> Option<Vec<String>> {
    let rest = strip_table_indent(line)?.trim_end();
    if rest.is_empty() {
        return None;
    }
    // Outer pipes are optional (GFM): strip them when present so pipeless rows
    // like `Name | Score` split the same way as `| Name | Score |`.
    let inner = rest.strip_prefix('|').unwrap_or(rest);
    let inner = inner.strip_suffix('|').unwrap_or(inner);

    let mut cells = Vec::new();
    let mut cursor = 0usize;
    for position in cell_separator_positions(inner) {
        cells.push(inner[cursor..position].trim().to_string());
        cursor = position + 1;
    }
    cells.push(inner[cursor..].trim().to_string());
    Some(cells)
}

fn parse_alignment_cell(cell: &str) -> Option<TableColumnAlignment> {
    let trimmed = cell.trim();
    let left = trimmed.starts_with(':');
    let right = trimmed.ends_with(':');
    let mut core = trimmed;
    if left {
        core = &core[1..];
    }
    if right && !core.is_empty() {
        core = &core[..core.len() - 1];
    }
    // GFM asks for one hyphen per delimiter cell, with at most one optional
    // colon on either end. Demanding three hyphens made valid tables render as
    // plain text: `htmd` writes `| ---- | --- | -- |` when it converts an HTML
    // table, and `|:--|:--:|` headers are common in hand-written Markdown.
    if core.is_empty() || !core.chars().all(|ch| ch == '-') {
        return None;
    }

    Some(match (left, right) {
        (true, true) => TableColumnAlignment::Center,
        (false, true) => TableColumnAlignment::Right,
        (true, false) => TableColumnAlignment::Left,
        (false, false) => TableColumnAlignment::Default,
    })
}

fn serialize_alignment(alignment: TableColumnAlignment) -> &'static str {
    match alignment {
        TableColumnAlignment::Default => "---",
        TableColumnAlignment::Left => ":---",
        TableColumnAlignment::Center => ":---:",
        TableColumnAlignment::Right => "---:",
    }
}

pub(crate) fn serialize_table_cell_markdown(tree: &InlineTextTree) -> String {
    // 行内序列化器负责反斜杠与转义写法（`is_commonmark_escapable` 同源）；格层面
    // 只补「会被切开」的裸竖线，不盲目加倍反斜杠——`replace('\\', "\\\\")` 那种
    // 全局预处理会把行内已经写好的 `\|` 洗成 `\\|`（切开格），改了文档的意思。
    escape_cell_separator_pipes(&tree.serialize_markdown())
        // 硬换行的行内写法（`\`+换行）在单行格里没有意义，统一成 `<br>`；
        // 裸换行同理，否则会把表格行拆成两行。
        .replace("\\\n", "<br>")
        .replace('\n', "<br>")
}

fn serialize_row<'a>(cells: impl IntoIterator<Item = &'a InlineTextTree>) -> String {
    let rendered = cells
        .into_iter()
        .map(serialize_table_cell_markdown)
        .collect::<Vec<_>>();
    format!("| {} |", rendered.join(" | "))
}

/// Returns true when a line is a candidate native table row in the current
/// container scope.
pub fn is_table_candidate_line(line: &str) -> bool {
    strip_table_indent(line)
        .map(str::trim_end)
        .is_some_and(|rest| rest.starts_with('|'))
}

/// Number of pipe-separated cells in `line`, treating outer pipes as optional
/// (GFM) so pipeless rows like `Name | Score` are recognized. Returns `None`
/// for single-column lines so prose containing a stray `|` is not mistaken for
/// a table row.
pub fn table_row_column_count(line: &str) -> Option<usize> {
    split_table_cells(line)
        .map(|cells| cells.len())
        .filter(|count| *count >= 2)
}

/// True when `line` could be a table row, including a pipeless GFM row.
pub fn is_table_row_candidate(line: &str) -> bool {
    table_row_column_count(line).is_some()
}

/// Collects a contiguous table-candidate region in the current container
/// scope.
pub fn collect_table_candidate_region(lines: &[String], start: usize) -> usize {
    let mut index = start + 1;
    while index < lines.len() && is_table_candidate_line(&lines[index]) {
        index += 1;
    }
    index
}

/// Parses a pipe-table region into native table data.
pub fn parse_table_region(lines: &[String]) -> Option<TableData> {
    if lines.len() < 2 {
        return None;
    }

    let header = split_table_cells(&lines[0])?;
    let alignment_cells = split_table_cells(&lines[1])?;
    if header.is_empty() || alignment_cells.len() != header.len() {
        return None;
    }

    let alignments = alignment_cells
        .iter()
        .map(|cell| parse_alignment_cell(cell))
        .collect::<Option<Vec<_>>>()?;

    let mut rows = Vec::new();
    for line in &lines[2..] {
        // GFM normalizes body rows to the header width: short rows are padded
        // with empty cells and long rows drop their trailing cells, instead of
        // invalidating the whole table.
        let mut cells = split_table_cells(line)?;
        cells.resize(header.len(), String::new());
        rows.push(
            cells
                .into_iter()
                .map(|cell| InlineTextTree::from_markdown(&cell))
                .collect::<Vec<_>>(),
        );
    }

    Some(TableData {
        header: header
            .into_iter()
            .map(|cell| InlineTextTree::from_markdown(&cell))
            .collect(),
        rows,
        alignments,
    })
}

/// Returns true when `line` is a delimiter row of exactly `columns` cells, each
/// a valid alignment specifier.
fn is_delimiter_row(line: &str, columns: usize) -> bool {
    split_table_cells(line).is_some_and(|cells| {
        cells.len() == columns
            && cells
                .iter()
                .all(|cell| parse_alignment_cell(cell).is_some())
    })
}

/// Detects a table that starts at `start` without requiring outer pipes,
/// returning the region end (exclusive) when `lines[start]` is a multi-column
/// header followed by a matching delimiter row. Body rows extend to the next
/// blank line, matching GFM. Returns `None` for ordinary prose so a stray `|`
/// is never mistaken for a table; single-column pipeless candidates are also
/// rejected because they are ambiguous with setext headings.
pub fn collect_pipeless_table_region(lines: &[String], start: usize) -> Option<usize> {
    let header = split_table_cells(lines.get(start)?)?;
    if header.len() < 2 {
        return None;
    }
    if !is_delimiter_row(lines.get(start + 1)?, header.len()) {
        return None;
    }

    let mut end = start + 2;
    while end < lines.len() && !lines[end].trim().is_empty() {
        end += 1;
    }
    Some(end)
}

/// Returns true when a root-level line is a candidate native table row.
pub fn is_root_table_candidate_line(line: &str) -> bool {
    is_table_candidate_line(line)
}

/// Collects a contiguous root-level table candidate region.
pub fn collect_root_table_candidate_region(lines: &[String], start: usize) -> usize {
    collect_table_candidate_region(lines, start)
}

/// Parses a root-level pipe table region into native table data.
pub fn parse_root_table_region(lines: &[String]) -> Option<TableData> {
    parse_table_region(lines)
}

/// Parses a single table body row, normalized to `columns` cells (padded when
/// short, truncated when long). Returns `None` when the line is not a table
/// row at all.
pub fn parse_table_body_row(line: &str, columns: usize) -> Option<Vec<InlineTextTree>> {
    let mut cells = split_table_cells(line)?;
    cells.resize(columns, String::new());
    Some(
        cells
            .into_iter()
            .map(|cell| InlineTextTree::from_markdown(&cell))
            .collect(),
    )
}

/// Serializes native table data to canonical pipe-table Markdown lines.
pub fn serialize_table_markdown_lines(table: &TableData) -> Vec<String> {
    let mut lines = Vec::with_capacity(2 + table.rows.len());
    lines.push(serialize_row(table.header.iter()));
    lines.push(format!(
        "| {} |",
        table
            .alignments
            .iter()
            .map(|alignment| serialize_alignment(*alignment))
            .collect::<Vec<_>>()
            .join(" | ")
    ));
    lines.extend(table.rows.iter().map(|row| serialize_row(row.iter())));
    lines
}

#[cfg(test)]
mod tests;
