//! Rendering for [`Block`] via GPUI's high-level [`Render`] trait.
//!
//! Each block kind produces a distinct visual style: H1 has a bottom border,
//! list items render a marker column (bullet / ordinal), and raw Markdown
//! fallback renders as plain text.

pub(super) use gpui::*;
pub(super) use gpui::prelude::FluentBuilder;

const BLOCK_EDITOR_CONTEXT: &str = "BlockEditor";

pub(super) use super::element::{BlockTextElement, CodeLanguageInputElement};
pub(super) use super::{Block, BlockEvent, BlockKind, ImageResolvedSource, ImageRuntime};
pub(super) use crate::components::{
    Editor, HtmlCssColor, HtmlDocument, HtmlNode, HtmlNodeKind, HtmlTextAlign, InlineScript,
    TableAxisHighlight, TableAxisKind, TableCellInlineImageSegment,
    ColumnLayoutMemo, TableColumnLayout, attr_value, display_math_font_size,
    inline_math_font_size,
    parse_html_image_block, parse_mermaid_fence_source,
    parse_table_cell_inline_images, render_display_math_svg, render_inline_math_svg,
    render_mermaid_svg_for_display, resolve_image_source, style_for_node,
};
use crate::i18n::{I18nManager, I18nStrings};
use crate::theme::{Theme, ThemeColors, ThemeDimensions, ThemeManager};

const TASK_CHECKMARK: &str = "\u{2713}";
const VISUAL_BLOCK_WIDTH_RATIO: f32 = 0.95;
const TABLE_CORNER_RADIUS: f32 = 10.0;

// 标题折叠 chevron（roadmap C7）：位于标题行左侧留白内的按钮。
const HEADING_FOLD_CHEVRON_RIGHT: &str = "icon/workspace/chevron-right.svg";
const HEADING_FOLD_CHEVRON_DOWN: &str = "icon/workspace/chevron-down.svg";
const HEADING_FOLD_CHEVRON_GUTTER: f32 = 18.0;
const HEADING_FOLD_CHEVRON_ICON_SIZE: f32 = 12.0;

/// Makes a row-axis highlight color more opaque (more solid, still translucent)
/// for the header row, keeping the theme's hue so the header handle reads as a
/// stronger version of the body-row handles in whatever colors the theme uses.
fn header_axis_emphasis(color: Hsla) -> Hsla {
    Hsla {
        a: color.a + (1.0 - color.a) * 0.5,
        ..color
    }
}

fn table_cell_colors(base: Hsla, highlight: TableAxisHighlight, focused: bool, colors: &ThemeColors) -> (Hsla, Hsla) {
    let tint = match highlight {
        TableAxisHighlight::None => hsla(0.0, 0.0, 0.0, 0.0),
        TableAxisHighlight::Preview => Hsla { a: colors.table_axis_preview_bg.a.min(0.06), ..colors.table_axis_preview_bg },
        TableAxisHighlight::Selected => Hsla { a: colors.table_axis_selected_bg.a.min(0.10), ..colors.table_axis_selected_bg },
    };
    let border = if focused { colors.table_cell_active_outline } else { colors.table_border };
    (base.blend(tint), border)
}

/// Detects a `#tag` word: `#` followed by at least one alphanumeric
/// (including CJK), `_` or `-` character, with no whitespace.
/// 悬停预览 tooltip（roadmap C8/C9）：脚注与链接目标预览。
pub(crate) struct HoverPreviewTooltip {
    pub(crate) label: SharedString,
}

impl Render for HoverPreviewTooltip {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<ThemeManager>().current_arc();
        div()
            .px(px(8.0))
            .py(px(5.0))
            .rounded(px(6.0))
            .bg(theme.colors.dialog_surface)
            .border_1()
            .border_color(theme.colors.dialog_border)
            .shadow_md()
            .text_size(px(11.5))
            .text_color(theme.colors.dialog_title)
            .child(self.label.clone())
    }
}

/// 行内片段在「混合分段」渲染路径（数学/上下标/行内图片同块）里用的显示字号。
///
/// 行内代码跟随「代码块字体大小」设置，上标/下标缩小到 72%，其余跟随正文。
/// 可编辑文本的 `TextRun` 现在也能带逐段字号（vendored gpui 本地补丁），显示态
/// 与编辑态用同一套字号规则，点击进入编辑不再跳变（用户报修）。
fn inline_display_font_size(
    span: &crate::components::InlineSpan,
    font_size: f32,
    code_font_size: f32,
) -> f32 {
    if span.style.code {
        code_font_size.max(1.0)
    } else if span.style.has_script() {
        (font_size * 0.72).max(6.0)
    } else {
        font_size
    }
}

/// 该段文本里的 `![alt](src)` 是否应提升为行内图片控件。
///
/// 行内代码内部是字面文本，不解析任何 Markdown（用户报修：表格单元格里被反引号
/// 包住的图片语法渲染成了「无法加载图片」占位框）。
fn promotes_inline_images(text: &str, style: &crate::components::InlineStyle) -> bool {
    !style.code && text.contains("![")
}

/// 链接/脚注悬停 tooltip 文案（roadmap C9/C8）。
fn segment_hash(text: &str, range_start: usize) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in text.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash ^= range_start as u64;
    hash = hash.wrapping_mul(0x100000001b3);
    hash
}

fn hover_preview_label(
    footnote_id: Option<&str>,
    open_target: &str,
    is_remote: bool,
    strings: &I18nStrings,
) -> String {
    if let Some(id) = footnote_id {
        return format!("{} [^{}]", strings.hover_footnote_prefix, id);
    }
    if is_remote {
        return open_target.to_string();
    }
    let exists = std::path::Path::new(open_target).exists();
    if exists {
        format!("\u{2713} {open_target} \u{2014} {}", strings.hover_target_exists)
    } else {
        format!("\u{2717} {open_target} \u{2014} {}", strings.hover_target_missing)
    }
}
/// Detects a `[[target]]` wikilink inside a rendered word/segment and returns
/// the trimmed target name (roadmap C3).
fn wikilink_target(word: &str) -> Option<String> {
    let start = word.find("[[")?;
    let inner = &word[start + 2..];
    let end = inner.find("]]")?;
    let target = inner[..end].trim();
    (!target.is_empty()).then(|| target.to_string())
}

fn tag_query(word: &str) -> Option<String> {
    let rest = word.strip_prefix('#')?;
    let valid = !rest.is_empty()
        && rest
            .chars()
            .all(|ch| ch.is_alphanumeric() || ch == '_' || ch == '-');
    valid.then(|| format!("#{rest}"))
}

fn fallback_image_label(alt: &str, strings: &I18nStrings) -> SharedString {
    if alt.trim().is_empty() {
        SharedString::from(strings.image_placeholder.clone())
    } else {
        SharedString::from(alt.to_string())
    }
}

/// Compact strip shown when an image fails to load. A full-size empty box
/// reads as a rendering bug, so the failure state stays one line tall.
fn render_image_placeholder(
    runtime: &ImageRuntime,
    width: Length,
    height: Pixels,
    theme: &Theme,
    strings: &I18nStrings,
) -> AnyElement {
    let c = &theme.colors;
    let d = &theme.dimensions;
    let t = &theme.typography;
    let compact_height = height.min(px(72.0));
    let label = fallback_image_label(&runtime.alt, strings);
    div()
        .min_w(px(0.0))
        .max_w(width)
        .h(compact_height)
        .debug_selector(|| "image-placeholder".to_string())
        .flex()
        .items_center()
        .justify_center()
        .gap(px(6.0))
        .rounded(px(d.image_radius))
        .border(px(1.0))
        .border_color(c.image_placeholder_border)
        .bg(c.image_placeholder_bg)
        .px(px(d.block_padding_x))
        .text_size(px(t.text_size * 0.82))
        .text_color(c.image_placeholder_text)
        .child(SharedString::from(strings.image_load_failed.clone()))
        .child(
            div()
                .min_w(px(0.0))
                .max_w(px(360.0))
                .truncate()
                .text_size(px(t.code_size))
                .text_color(c.dialog_muted)
                .child(label),
        )
        .into_any_element()
}

fn render_loading_placeholder(
    runtime: &ImageRuntime,
    width: Length,
    height: Pixels,
    theme: &Theme,
    strings: &I18nStrings,
) -> AnyElement {
    let c = &theme.colors;
    let d = &theme.dimensions;
    let t = &theme.typography;
    // Remote fetches can stall; cap the loading box so a hung image does not
    // reserve a huge empty area.
    let capped_height = height.min(px(120.0));
    div()
        .min_w(px(0.0))
        .max_w(width)
        .h(capped_height)
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(d.image_radius))
        .border(px(1.0))
        .border_color(c.image_placeholder_border)
        .bg(c.image_placeholder_bg)
        .px(px(d.block_padding_x))
        .text_center()
        .text_size(px(t.code_size))
        .text_color(c.image_placeholder_text)
        .child(if runtime.alt.trim().is_empty() {
            SharedString::from(strings.image_loading_without_alt.clone())
        } else {
            SharedString::from(
                strings
                    .image_loading_with_alt_template
                    .replace("{alt}", &runtime.alt),
            )
        })
        .into_any_element()
}

fn wrap_with_quote_guides(content: AnyElement, quote_depth: usize, theme: &Theme) -> AnyElement {
    if quote_depth == 0 {
        return content;
    }

    let c = &theme.colors;
    let d = &theme.dimensions;
    let guide_offset = d.quote_padding_left;
    let total_padding = guide_offset * quote_depth as f32;

    div()
        .w_full()
        .relative()
        .pl(px(total_padding))
        .child(content)
        .children((0..quote_depth).map(|level| {
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .left(px(guide_offset * level as f32))
                .w(px(d.quote_border_width))
                .bg(c.border_quote)
        }))
        .into_any_element()
}

fn callout_accent_and_background(variant: super::CalloutVariant, theme: &Theme) -> (Hsla, Hsla) {
    let c = &theme.colors;
    match variant {
        super::CalloutVariant::Note => (c.callout_note_border, c.callout_note_bg),
        super::CalloutVariant::Tip => (c.callout_tip_border, c.callout_tip_bg),
        super::CalloutVariant::Important => (c.callout_important_border, c.callout_important_bg),
        super::CalloutVariant::Warning => (c.callout_warning_border, c.callout_warning_bg),
        super::CalloutVariant::Caution => (c.callout_caution_border, c.callout_caution_bg),
    }
}

fn visible_quote_guides(block: &Block) -> usize {
    block.visible_quote_depth
}

/// 与编辑器正文列同一条公式的内容列宽度（`Editor::writing_column_width`），表格量宽、
/// 图片与 mermaid 估宽必须用同一个宽度：两处各算各的，拿超宽容器算出的列宽比例套到
/// 真实窄容器上，钉住列会被压到内容宽以下折行（用户报修：水位法列宽完全不对）。
fn content_column_width(viewport_width: f32, d: &ThemeDimensions, cx: &App) -> f32 {
    Editor::writing_column_width(viewport_width, d, cx)
}


pub(crate) use link_cursor::*;
pub(crate) use parts::*;

mod content;
mod inline_visuals;
mod link_cursor;
mod paint_parts;
mod parts;
mod shell;

#[cfg(test)]
mod tests;
