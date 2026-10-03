use super::*;

impl Block {
    /// 表格列宽备忘（性能）：`measure` 会对每格做 no-wrap `shape_text`，此前
    /// 每帧全量重测——文档打开后每次悬停/点击触发的重绘都拖着 O(单元格) 的
    /// 文字排版，Windows DirectWrite 上尤其明显（用户报修：打开文件后第二次
    /// 点击起整个界面卡死）。键（主题代数/字号/容器宽/表内容）均未变时整帧
    /// 零 shape。
    pub(crate) fn cached_table_column_layout(
        &mut self,
        table_width: f32,
        theme: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<TableColumnLayout> {
        let table = self.record.table.clone()?;
        let theme_fingerprint = {
            // 参与 measure 的主题标量混一个指纹：字号/字距/内边距变了要重测。
            let t = &theme.typography;
            let d = &theme.dimensions;
            t.text_size.to_bits() as u64
                ^ (t.code_size.to_bits() as u64).rotate_left(1)
                ^ (t.text_letter_spacing.to_bits() as u64).rotate_left(2)
                ^ (d.table_cell_padding_x.to_bits() as u64).rotate_left(3)
                ^ (d.code_bg_pad_x.to_bits() as u64).rotate_left(4)
        };
        let width_bits = table_width.to_bits();
        let code_size_bits = theme.typography.code_size.to_bits();
        let text_size_bits = theme.typography.text_size.to_bits();

        if let Some(memo) = self.column_layout_memo()
            && memo.theme_fingerprint == theme_fingerprint
            && memo.code_size_bits == code_size_bits
            && memo.text_size_bits == text_size_bits
            && memo.width_bits == width_bits
            && memo.table == table
        {
            return Some(memo.layout.clone());
        }

        let layout = TableColumnLayout::measure(&table, table_width, window, theme, cx);
        self.set_column_layout_memo(ColumnLayoutMemo {
            theme_fingerprint,
            code_size_bits,
            text_size_bits,
            width_bits,
            table,
            layout: layout.clone(),
        });
        Some(layout)
    }
}

pub(crate) fn effective_table_width(block: &Block, viewport_width: f32, d: &ThemeDimensions, cx: &App) -> f32 {
    let centered_width = content_column_width(viewport_width, d, cx);
    let visible_quote_guides = visible_quote_guides(block);
    let quote_inset = d.quote_padding_left * visible_quote_guides as f32;
    let callout_inset = if block.callout_depth > 0 {
        d.callout_padding_x * 2.0 + d.callout_border_width
    } else {
        0.0
    };

    (centered_width - quote_inset - callout_inset)
        .max((d.table_cell_padding_x * 2.0 + 80.0).max(120.0))
}

pub(crate) fn container_image_width_budget(
    block: &Block,
    viewport_width: f32,
    d: &ThemeDimensions,
    cx: &App,
) -> f32 {
    let centered_width = content_column_width(viewport_width, d, cx);
    let visible_quote_guides = visible_quote_guides(block);
    let quote_inset = d.quote_padding_left * visible_quote_guides as f32;
    let callout_inset = if block.callout_depth > 0 {
        d.callout_padding_x * 2.0 + d.callout_border_width
    } else {
        0.0
    };

    centered_width - quote_inset - callout_inset
}

pub(crate) fn effective_image_width(
    block: &Block,
    viewport_width: f32,
    d: &ThemeDimensions,
    cx: &App,
) -> f32 {
    let list_inset = d.nested_block_indent * block.render_depth as f32;
    (container_image_width_budget(block, viewport_width, d, cx)
        - d.block_padding_x * 2.0
        - list_inset)
        .max(160.0)
}

/// Returns a human-readable list ordinal: numbers at depth 0, lowercase
/// letters at depth 1, and unicode roman numerals at depth 2+.
/// `delimiter` 是该列表项在原文里写的分隔符（`.` 或 `)`），显示时照写。
pub(crate) fn numbered_list_marker(depth: usize, ordinal: usize, delimiter: char) -> String {
    match depth {
        0 => format!("{ordinal}{delimiter}"),
        1 => format!("{}{delimiter}", alphabetic_list_marker(ordinal)),
        _ => format!("{}{delimiter}", roman_list_marker(ordinal)),
    }
}

/// Expands beyond 26 by wrapping: a...z, a1...z1, a2...z2, ...
pub(crate) fn alphabetic_list_marker(ordinal: usize) -> String {
    const ALPHABET: &[u8; 26] = b"abcdefghijklmnopqrstuvwxyz";

    let ordinal = ordinal.max(1);
    if ordinal <= ALPHABET.len() {
        return char::from(ALPHABET[ordinal - 1]).to_string();
    }

    let wrapped = ordinal - (ALPHABET.len() + 1);
    let letter = char::from(ALPHABET[wrapped % ALPHABET.len()]);
    let suffix = wrapped + 1;
    format!("{letter}{suffix}")
}

/// Converts an ASCII roman numeral string to its unicode ligature equivalents
/// where possible (for example, "III" to a single roman numeral glyph).
pub(crate) fn roman_list_marker(ordinal: usize) -> String {
    let ascii = ascii_roman_numeral(ordinal.max(1));
    let mut index = 0;
    let mut marker = String::new();

    while index < ascii.len() {
        let remaining = &ascii[index..];
        if let Some((token_len, token)) = roman_unicode_token(remaining) {
            marker.push_str(token);
            index += token_len;
        } else {
            break;
        }
    }

    marker
}

pub(crate) fn ascii_roman_numeral(mut ordinal: usize) -> String {
    const MAP: &[(usize, &str)] = &[
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];

    let mut result = String::new();
    for (value, symbol) in MAP {
        while ordinal >= *value {
            result.push_str(symbol);
            ordinal -= *value;
        }
    }
    result
}

pub(crate) fn roman_unicode_token(remaining: &str) -> Option<(usize, &'static str)> {
    const TOKENS: &[(&str, &str)] = &[
        ("XII", "\u{216B}"),
        ("XI", "\u{216A}"),
        ("IX", "\u{2168}"),
        ("VIII", "\u{2167}"),
        ("VII", "\u{2166}"),
        ("VI", "\u{2165}"),
        ("IV", "\u{2163}"),
        ("III", "\u{2162}"),
        ("II", "\u{2161}"),
        ("I", "\u{2160}"),
        ("V", "\u{2164}"),
        ("X", "\u{2169}"),
        ("L", "\u{216C}"),
        ("C", "\u{216D}"),
        ("D", "\u{216E}"),
        ("M", "\u{216F}"),
    ];

    TOKENS.iter().find_map(|(ascii, unicode)| {
        remaining
            .starts_with(ascii)
            .then_some((ascii.len(), *unicode))
    })
}

pub(crate) fn html_children_text(node: &HtmlNode) -> String {
    if node.children.is_empty() {
        return node.raw_source.clone();
    }

    let mut text = String::new();
    for child in &node.children {
        if child.tag_name == "br" {
            text.push('\n');
        } else {
            text.push_str(&html_children_text(child));
        }
    }
    text
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct HtmlComputedStyle {
    pub(crate) color: Hsla,
    pub(crate) font_size: f32,
    pub(crate) root_font_size: f32,
    pub(crate) text_align: Option<TextAlign>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct HtmlNodeVisualStyle {
    pub(crate) computed: HtmlComputedStyle,
    pub(crate) background: Option<Hsla>,
}

impl HtmlComputedStyle {
    pub(crate) fn root(theme: &Theme) -> Self {
        Self {
            color: theme.colors.text_default,
            font_size: theme.typography.text_size,
            root_font_size: theme.typography.text_size,
            text_align: None,
        }
    }
}

pub(crate) fn html_css_color_to_hsla(color: HtmlCssColor, current_color: Hsla) -> Hsla {
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

pub(crate) fn html_node_visual_style(
    node: &HtmlNode,
    parent: HtmlComputedStyle,
    theme: &Theme,
) -> HtmlNodeVisualStyle {
    let c = &theme.colors;
    let t = &theme.typography;
    let mut computed = parent;
    let mut background = None;

    match node.tag_name.as_str() {
        "a" => computed.color = c.text_link,
        "blockquote" => computed.color = c.text_quote,
        "code" | "kbd" | "pre" => {
            computed.color = c.code_text;
            computed.font_size = t.code_size;
            background = Some(c.code_bg);
        }
        "mark" => background = Some(c.comment_bg),
        "figcaption" => {
            computed.color = c.image_caption_text;
            computed.font_size = t.code_size;
        }
        "small" | "sup" | "sub" => computed.font_size = (computed.font_size * 0.8).max(6.0),
        "th" => background = Some(c.table_header_bg),
        "td" => background = Some(c.table_cell_bg),
        _ => {}
    }

    let inline_style = style_for_node(node);
    if let Some(color) = inline_style.color {
        computed.color = html_css_color_to_hsla(color, computed.color);
    }
    if let Some(font_size) = inline_style.font_size {
        computed.font_size = font_size.resolve(computed.font_size, computed.root_font_size);
    }
    if let Some(color) = inline_style.background_color {
        background = Some(html_css_color_to_hsla(color, computed.color));
    }
    if let Some(align) = inline_style.text_align {
        computed.text_align = Some(match align {
            HtmlTextAlign::Left => TextAlign::Left,
            HtmlTextAlign::Center => TextAlign::Center,
            HtmlTextAlign::Right => TextAlign::Right,
        });
    }

    HtmlNodeVisualStyle {
        computed,
        background,
    }
}

