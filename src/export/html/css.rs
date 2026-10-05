use super::*;

pub(crate) fn theme_css(theme: &Theme) -> String {
    let c = &theme.colors;
    let d = &theme.dimensions;
    let t = &theme.typography;
    let color_scheme = if c.editor_background.l >= 0.5 {
        "light"
    } else {
        "dark"
    };
    let pre_overflow = "overflow: auto;";
    let media_overflow = "overflow-x: auto;";
    format!(
        r#":root {{
  color-scheme: {};
  --vlt-bg: {};
  --vlt-text: {};
  --vlt-muted: {};
  --vlt-link: {};
  --vlt-border: {};
  --vlt-code-bg: {};
  --vlt-code-text: {};
  --vlt-comment-bg: {};
  --vlt-table-head-bg: {};
  --vlt-table-cell-bg: {};
  --vlt-quote-border: {};
  --vlt-quote-text: {};
  --vlt-callout-note-bg: {};
  --vlt-callout-note-border: {};
  --vlt-callout-tip-bg: {};
  --vlt-callout-tip-border: {};
  --vlt-callout-important-bg: {};
  --vlt-callout-important-border: {};
  --vlt-callout-warning-bg: {};
  --vlt-callout-warning-border: {};
  --vlt-callout-caution-bg: {};
  --vlt-callout-caution-border: {};
}}

* {{ box-sizing: border-box; }}
html {{ background-color: var(--vlt-bg); color: var(--vlt-text); }}
body {{
  margin: 0;
  background-color: var(--vlt-bg);
  color: var(--vlt-text);
  font-family: {};
  font-size: {}px;
  line-height: {};
  letter-spacing: {}em;
  text-autospace: ideograph-alpha ideograph-numeric;
}}
{}
p, ul, ol, blockquote, pre, table, hr {{ margin: 0 0 1rem; }}
h1, h2, h3, h4, h5, h6 {{
  margin: 1.6em 0 0.65em;
  line-height: 1.2;
  font-weight: {};
}}
h1 {{ color: {}; font-size: {}px; border-bottom: 1px solid; border-color: {}; padding-bottom: 0.2em; }}
h2 {{ color: {}; font-size: {}px; border-bottom: 1px solid; border-color: {}; padding-bottom: 0.18em; }}
h3 {{ color: {}; font-size: {}px; }}
h4 {{ color: {}; font-size: {}px; }}
h5 {{ color: {}; font-size: {}px; }}
h6 {{ color: {}; font-size: {}px; }}
a {{ color: var(--vlt-link); text-decoration-thickness: 0.08em; text-underline-offset: 0.18em; }}
blockquote {{
  margin-left: 0;
  padding: 0.5rem 0 0.5rem 1rem;
  border-left: 3px solid;
  border-color: var(--vlt-quote-border);
  color: var(--vlt-quote-text);
}}
blockquote.markdown-alert-note,
blockquote.markdown-alert-tip,
blockquote.markdown-alert-important,
blockquote.markdown-alert-warning,
blockquote.markdown-alert-caution {{
  padding: 0.75rem 1rem;
  border-left: 4px solid;
  border-radius: {}px;
}}
blockquote.markdown-alert-note {{ background-color: var(--vlt-callout-note-bg); border-color: var(--vlt-callout-note-border); }}
blockquote.markdown-alert-tip {{ background-color: var(--vlt-callout-tip-bg); border-color: var(--vlt-callout-tip-border); }}
blockquote.markdown-alert-important {{ background-color: var(--vlt-callout-important-bg); border-color: var(--vlt-callout-important-border); }}
blockquote.markdown-alert-warning {{ background-color: var(--vlt-callout-warning-bg); border-color: var(--vlt-callout-warning-border); }}
blockquote.markdown-alert-caution {{ background-color: var(--vlt-callout-caution-bg); border-color: var(--vlt-callout-caution-border); }}
code {{
  background-color: var(--vlt-code-bg);
  color: var(--vlt-link);
  font-weight: 500;
  letter-spacing: normal;
  margin: 0 0.125em;
  -webkit-box-decoration-break: clone;
  box-decoration-break: clone;
  border-radius: 4px;
  padding: 0.12em {}px;
  font-family: {};
  font-size: {}px;
}}
pre {{
  letter-spacing: normal;
  {}
  background-color: var(--vlt-code-bg);
  color: var(--vlt-code-text);
  border-radius: {}px;
  padding: 1rem;
}}
pre code {{ padding: 0; margin: 0; background-color: transparent; color: inherit; font-weight: normal; }}
strong code {{ font-weight: 700; }}
.vlt-comment {{
  white-space: pre-wrap;
  background-color: var(--vlt-comment-bg);
  color: var(--vlt-text);
}}
/* 标记文本 `==x==` 导出为 mark 标签；配色与注释块同源，编辑态里的 `==` 底色也是这个。 */
mark {{
  background-color: var(--vlt-comment-bg);
  color: inherit;
}}
.vlt-raw-html {{
  white-space: pre-wrap;
  background-color: var(--vlt-code-bg);
  color: var(--vlt-code-text);
}}
.vlt-math {{
  display: flex;
  justify-content: center;
  margin: 1rem 0;
  {}
}}
.vlt-math svg {{
  max-width: 100%;
  height: auto;
}}
.vlt-mermaid {{
  display: flex;
  justify-content: center;
  margin: 1rem 0;
  {}
}}
.vlt-mermaid img {{
  max-width: 100%;
  height: auto;
  display: block;
  margin: 0 auto;
}}
.vlt-inline-math {{
  display: inline-flex;
  align-items: center;
  vertical-align: middle;
  max-width: 100%;
}}
.vlt-inline-math svg {{
  max-height: 1.8em;
  width: auto;
}}
.vlt-math-error {{
  white-space: pre-wrap;
  background-color: var(--vlt-code-bg);
  color: var(--vlt-code-text);
}}
.vlt-mermaid-error {{
  white-space: pre-wrap;
  background-color: var(--vlt-code-bg);
  color: var(--vlt-code-text);
}}
table {{
  width: 100%;
  border-collapse: collapse;
  display: table;
}}
th, td {{
  border: 1px solid;
  border-color: var(--vlt-border);
  padding: 0.5rem 0.65rem;
  vertical-align: top;
}}
th {{ background-color: var(--vlt-table-head-bg); font-weight: 600; }}
td {{ background-color: var(--vlt-table-cell-bg); }}
img {{ max-width: 100%; height: auto; display: block; margin: 1rem auto; }}
hr {{ border: 0; border-top: 1px solid; border-color: var(--vlt-border); }}
.footnote-definition {{
  color: var(--vlt-muted);
  font-size: 0.92em;
}}
.vlt-front-matter table {{ width: auto; min-width: 30%; }}
.vlt-front-matter th {{
  background-color: var(--vlt-code-bg);
  font-family: "SFMono-Regular", Consolas, "Liberation Mono", Menlo, monospace;
  font-weight: 500;
  white-space: nowrap;
}}
.vlt-front-matter td ul {{
  margin: 0;
  padding-left: 1.2em;
}}
"#,
        color_scheme,
        css_color(c.editor_background),
        css_color(c.text_default),
        css_color(c.dialog_muted),
        css_color(c.text_link),
        css_color(c.table_border),
        css_color(c.code_bg),
        css_color(c.code_text),
        css_color(c.comment_bg),
        css_color(c.table_header_bg),
        css_color(c.table_cell_bg),
        css_color(c.border_quote),
        css_color(c.text_quote),
        css_color(c.callout_note_bg),
        css_color(c.callout_note_border),
        css_color(c.callout_tip_bg),
        css_color(c.callout_tip_border),
        css_color(c.callout_important_bg),
        css_color(c.callout_important_border),
        css_color(c.callout_warning_bg),
        css_color(c.callout_warning_border),
        css_color(c.callout_caution_bg),
        css_color(c.callout_caution_border),
        body_font_stack(),
        t.text_size,
        t.text_line_height,
        t.text_letter_spacing,
        document_layout_css(),
        css_font_weight(&t.h1_weight),
        css_color(c.text_h1),
        t.h1_size,
        css_color(c.border_h1),
        css_color(c.text_h2),
        t.h2_size,
        css_color(c.border_h2),
        css_color(c.text_h3),
        t.h3_size,
        css_color(c.text_h4),
        t.h4_size,
        css_color(c.text_h5),
        t.h5_size,
        css_color(c.text_h6),
        t.h6_size,
        d.callout_radius,
        d.code_bg_pad_x,
        "\"SFMono-Regular\", Consolas, \"Liberation Mono\", Menlo, monospace",
        t.code_size,
        pre_overflow,
        d.code_bg_radius,
        media_overflow,
        media_overflow
    )
}

pub(crate) fn chromium_pdf_theme_css(theme: &Theme) -> String {
    let mut css = theme_css(theme);
    css = css.replace(
        document_layout_css(),
        ".vlt-document {\n  width: auto;\n  max-width: none;\n  margin: 0;\n  padding: 0;\n}",
    );
    css.push_str(CHROMIUM_PRINT_CSS);
    css
}

/// 供系统打印/预览使用的打印版式覆盖（roadmap F4）。
///
/// 与 `chromium_pdf_theme_css` 共用同一段规则，保证「打印」与「导出 PDF」
/// 两条路径在打印媒体下版式一致。
const CHROMIUM_PRINT_CSS: &str = r#"

@page {
  size: A4;
  margin: 15mm;
}

@media print {
  html,
  body {
    background-color: var(--vlt-bg);
    border: 0;
    outline: 0;
    box-shadow: none;
    print-color-adjust: exact;
    -webkit-print-color-adjust: exact;
  }

  .vlt-document {
    width: auto;
    max-width: none;
    margin: 0;
    padding: 0;
    border: 0;
    outline: 0;
    box-shadow: none;
  }

  pre,
  code {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  img,
  svg {
    max-width: 100%;
    height: auto;
    break-inside: avoid;
  }

  table,
  blockquote,
  pre,
  .vlt-math,
  .vlt-mermaid {
    break-inside: avoid;
  }
}
"#;

/// 为浏览器导出 HTML 注入打印版式；缺少 `<style>` 时原样返回。
pub(crate) fn prepare_print_html(html: &str) -> String {
    match html.rfind("</style>") {
        Some(index) => format!("{}{}{}", &html[..index], CHROMIUM_PRINT_CSS, &html[index..]),
        None => html.to_string(),
    }
}

pub(crate) fn body_font_stack() -> &'static str {
    "system-ui, -apple-system, BlinkMacSystemFont, \"Segoe UI\", \"Noto Serif Tibetan\", \"Noto Sans Tibetan\", \"Microsoft Himalaya\", Kailasa, \"BabelStone Tibetan\", sans-serif"
}

pub(crate) fn document_layout_css() -> &'static str {
    ".vlt-document {\n  width: min(100% - 48px, 920px);\n  margin: 0 auto;\n  padding: 48px 0 72px;\n}"
}

pub(crate) fn contains_tibetan_text(text: &str) -> bool {
    text.chars()
        .any(|ch| ('\u{0f00}'..='\u{0fff}').contains(&ch))
}

pub(crate) fn css_color(color: Hsla) -> String {
    let color = Rgba::from(color);
    format!(
        "rgba({},{},{},{:.3})",
        css_color_channel(color.r),
        css_color_channel(color.g),
        css_color_channel(color.b),
        color.a.clamp(0.0, 1.0)
    )
}

pub(crate) fn css_color_channel(channel: f32) -> u8 {
    (channel.clamp(0.0, 1.0) * 255.0).round() as u8
}

pub(crate) fn css_font_weight(weight: &FontWeightDef) -> u16 {
    match weight {
        FontWeightDef::Thin => 100,
        FontWeightDef::Light => 300,
        FontWeightDef::Normal => 400,
        FontWeightDef::Medium => 500,
        FontWeightDef::Semibold => 600,
        FontWeightDef::Bold => 700,
        FontWeightDef::Extrabold => 800,
        FontWeightDef::Black => 900,
    }
}

pub(crate) fn escape_html(input: &str) -> String {
    let mut escaped = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(ch),
        }
    }
    escaped
}
