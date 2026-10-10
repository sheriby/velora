//! LaTeX display-math parsing and RaTeX SVG rendering helpers.
//! 缓存写入 velora 独立目录。

mod symbols;

use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

use anyhow::{Context as _, anyhow};
use directories::ProjectDirs;
use gpui::{Hsla, Rgba};

pub(crate) use symbols::{LatexCategory, LatexSymbol, LATEX_SYMBOLS, inside_inline_math, latex_command_before_cursor, latex_completions_for};

/// 块级公式与行内公式同字号（用户报修：`$$ ... $$` 渲染出来明显偏大）。
/// KaTeX/Typora 的 display 模式只改变极限位置，不放大字号。
const DISPLAY_MATH_SCALE: f32 = INLINE_MATH_SCALE;
const INLINE_MATH_SCALE: f32 = 1.12;

/// Parsed display-math source preserved from Markdown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DisplayMathSource {
    /// Full Markdown source, including `$$` delimiters.
    pub(crate) raw: String,
    /// LaTeX body between the display delimiters.
    pub(crate) body: String,
}

/// 块源码沿 `$$…$$` 切出来的一个分段：公式，或公式**以外**的文字。
///
/// 用户报修（cases/03-formula-tail.md）：单行 `$$x^2$$ LOST_SENTINEL` 里旧实现只
/// 取第一段公式体，闭合 `$$` 之后的文字被解析器整个丢掉，界面上什么都看不见。
/// 解析必须交代**整个源码跨度**——渲染端要用分段表（`parse_display_math_segments`），
/// `DisplayMathSource` 只是「第一个公式」的视图，供分类器与草稿提取使用。
/// 导出侧同样应当消费分段表，否则导出也会丢尾部文字。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DisplayMathSegment {
    /// 一个 `$$…$$` 公式段：`raw` 含定界符，`body` 是中间的 LaTeX 体。
    Formula { raw: String, body: String },
    /// 公式前后的文字：按行内 markdown 原样保留（`markdown` 即源码切片）。
    Text { markdown: String },
}

/// 首个 `$$` 关闭当前公式，后面仍可有文字或另一段公式；收集块区域与分段解析
/// 共用这一判据，避免一边要求关闭符在行尾、另一边又把最后一对当作关闭符。
pub(crate) fn split_display_math_closing_line(line: &str) -> Option<(&str, &str)> {
    line.split_once("$$")
}

/// 把块级源码完整切成公式/文字分段；一个公式都识别不出来才返回 `None`。
pub(crate) fn parse_display_math_segments(raw: &str) -> Option<Vec<DisplayMathSegment>> {
    let trimmed = raw.trim_matches('\n');
    let lines = trimmed.split('\n').collect::<Vec<_>>();
    if lines.is_empty() {
        return None;
    }

    if lines.len() == 1 {
        let line = strip_display_indent(lines.first()?)?.trim_end();
        return keep_segments_with_formula(single_line_segments(line));
    }

    // 多行块：开头的 `$$` 后面可以直接跟内容，结尾的 `$$` 前面也可以有内容。
    //
    // 笔记里很常见的是从 Typora 粘过来的这种写法（`$$` 不独占一行）：
    //
    //     $$\begin{aligned}
    //     a &= b \\
    //     \end{aligned}$$
    //
    // 旧实现要求首行是 `$$` 且末行也是 `$$`，这种写法会被当成普通文本（原样显示源码）。
    let opener = strip_display_indent(lines[0])?.trim_end();
    let opener_body = opener.strip_prefix("$$")?;
    // 首行的 `$$` 之后又出现 `$$`，说明公式在同一行就闭合了，不是多行块。
    if split_display_math_closing_line(opener_body).is_some() {
        return None;
    }

    let closer = lines.last()?.trim_end();
    // 结尾 `$$` 不必顶到行尾：`$$ = 5` 这类尾部文字按文字段交出去（旧实现直接判
    // 整块失败，公式都不渲染了）。
    let (closer_body, trailing) = split_display_math_closing_line(closer)?;

    let mut body_lines = Vec::with_capacity(lines.len());
    body_lines.push(opener_body);
    body_lines.extend_from_slice(&lines[1..lines.len() - 1]);
    body_lines.push(closer_body);
    let formula_raw = if trailing.is_empty() {
        trimmed.to_string()
    } else {
        trimmed[..trimmed.len() - trailing.len()].to_string()
    };
    let mut segments = vec![DisplayMathSegment::Formula {
        raw: formula_raw,
        body: body_lines.join("\n").trim().to_string(),
    }];
    if !trailing.trim().is_empty() {
        segments.extend(single_line_segments(trailing));
    }
    keep_segments_with_formula(segments)
}

/// 单行源码沿 `$$…$$` 扫描：`$$a$$ tail $$b$$` → 公式、文字、公式、文字……
fn single_line_segments(line: &str) -> Vec<DisplayMathSegment> {
    let mut segments = Vec::new();
    let mut cursor = 0usize;
    while let Some(open_rel) = line[cursor..].find("$$") {
        let open = cursor + open_rel;
        let body_start = open + 2;
        let Some((body, _)) = line
            .get(body_start..)
            .and_then(split_display_math_closing_line)
        else {
            break;
        };
        let close = body_start + body.len();
        if open > cursor {
            segments.push(DisplayMathSegment::Text {
                markdown: line[cursor..open].to_string(),
            });
        }
        segments.push(DisplayMathSegment::Formula {
            raw: line[open..close + 2].to_string(),
            body: line[body_start..close].trim().to_string(),
        });
        cursor = close + 2;
    }
    if cursor < line.len() {
        segments.push(DisplayMathSegment::Text {
            markdown: line[cursor..].to_string(),
        });
    }
    segments
}

/// 至少要识别出一个公式才算数（否则整段是文字，调用方按普通文本处理）；
/// 纯空白的文字段没有可见内容，扔掉。
fn keep_segments_with_formula(segments: Vec<DisplayMathSegment>) -> Option<Vec<DisplayMathSegment>> {
    let has_formula = segments
        .iter()
        .any(|segment| matches!(segment, DisplayMathSegment::Formula { .. }));
    if !has_formula {
        return None;
    }
    Some(
        segments
            .into_iter()
            .filter(|segment| match segment {
                DisplayMathSegment::Text { markdown } => !markdown.trim().is_empty(),
                DisplayMathSegment::Formula { .. } => true,
            })
            .collect(),
    )
}

/// Result of rendering display math into an SVG cache file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LatexSvgRender {
    /// Path to the SVG file consumed by GPUI's image element.
    pub(crate) path: PathBuf,
    /// SVG document content, used by export paths.
    pub(crate) svg: String,
}

/// 把原始 `$$...$$` Markdown 块解析成它包含的 LaTeX 体——「第一个公式」的视图，
/// 供块分类器与公式编辑器草稿提取使用。
///
/// **渲染与导出不要走这个视图**：整个源码跨度（公式前后的文字）归
/// [`parse_display_math_segments`]；只取本函数会把 `$$x^2$$ LOST_SENTINEL` 的
/// 尾部文字丢掉（用户报修 cases/03-formula-tail.md 的根因）。本函数保持旧的严格
/// 口径：首段必须就是公式，`text $$x$$` 这种「文字开头」的形状仍按普通文本处理。
pub(crate) fn parse_display_math_source(raw: &str) -> Option<DisplayMathSource> {
    let segments = parse_display_math_segments(raw)?;
    let Some(DisplayMathSegment::Formula { body, .. }) = segments.first() else {
        return None;
    };
    Some(DisplayMathSource {
        raw: raw.trim_matches('\n').to_string(),
        body: body.clone(),
    })
}

/// TeX 排版样式：行间公式用 Display，行内公式必须用 Text。
///
/// 旧实现两条路径都用 ratex 的默认（`MathStyle::Display`）：行内 `$\sum_0^1$` 会按行间
/// 尺寸排版（大字形、上下限放上下），再塞进行内公式那条 `max_h = 1.65em` 的框里，
/// 于是整体被压小、上下标位置也不对（用户报修：`\sum`、`\int` 非常小）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum MathLayout {
    /// 行间公式（`$$ ... $$`）。
    Display,
    /// 行内公式（`$ ... $`）。
    Inline,
}

impl MathLayout {
    fn ratex_style(self) -> ratex_types::MathStyle {
        match self {
            Self::Display => ratex_types::MathStyle::Display,
            Self::Inline => ratex_types::MathStyle::Text,
        }
    }
}

/// Display font size used for rendered display-math blocks.
pub(crate) fn display_math_font_size(base_font_size: f32) -> f32 {
    base_font_size * DISPLAY_MATH_SCALE
}

/// Display font size used for rendered inline math.
pub(crate) fn inline_math_font_size(base_font_size: f32) -> f32 {
    base_font_size * INLINE_MATH_SCALE
}

/// Render a display-math source into a cached SVG file.
pub(crate) fn render_display_math_svg(
    source: &DisplayMathSource,
    text_color: Hsla,
    font_size: f32,
) -> anyhow::Result<LatexSvgRender> {
    render_latex_svg_to_cache(&source.body, text_color, font_size, MathLayout::Display)
}

/// Render an inline LaTeX body into a cached SVG file.
pub(crate) fn render_inline_math_svg(
    latex: &str,
    text_color: Hsla,
    font_size: f32,
) -> anyhow::Result<LatexSvgRender> {
    render_latex_svg_to_cache(latex, text_color, font_size, MathLayout::Inline)
}

fn render_latex_svg_to_cache(
    latex: &str,
    text_color: Hsla,
    font_size: f32,
    math_layout: MathLayout,
) -> anyhow::Result<LatexSvgRender> {
    let svg = render_latex_to_svg(latex, text_color, font_size, math_layout)?;
    let key = latex_cache_key(latex, text_color, font_size, math_layout);
    let path = latex_cache_dir()?.join(format!("{key}.svg"));
    if !path.exists() {
        fs::write(&path, &svg)
            .with_context(|| format!("failed to write LaTeX SVG cache '{}'", path.display()))?;
    }
    Ok(LatexSvgRender { path, svg })
}

/// Render a LaTeX expression into self-contained SVG text.
pub(crate) fn render_latex_to_svg(
    latex: &str,
    text_color: Hsla,
    font_size: f32,
    math_layout: MathLayout,
) -> anyhow::Result<String> {
    let parsed = ratex_parser::parse(latex).map_err(|err| anyhow!("{err}"))?;
    let layout_options = ratex_layout::LayoutOptions {
        style: math_layout.ratex_style(),
        ..ratex_layout::LayoutOptions::default()
    };
    let layout = ratex_layout::layout(&parsed, &layout_options);
    let display_list = ratex_layout::to_display_list(&layout);
    let mut svg = ratex_svg::render_to_svg(
        &display_list,
        &ratex_svg::SvgOptions {
            font_size: f64::from(font_size.max(1.0)),
            padding: f64::from((font_size * 0.35).max(4.0)),
            embed_glyphs: true,
            ..ratex_svg::SvgOptions::default()
        },
    );
    svg = recolor_default_black(&svg, &svg_color(text_color));
    Ok(normalize_svg_size_units_to_px(&svg))
}

/// ratex 把根标签的 `width`/`height` 标成 `pt`，而 usvg/gpui（以及浏览器）会按
/// 96/72 把 pt 换成 px，于是公式比请求的字号大 1/3（用户报修：块级公式明显过大）。
/// 它的坐标空间本身就是「1 单位 = 该字号下的 1px」，所以把单位改写成 `px`。
fn normalize_svg_size_units_to_px(svg: &str) -> String {
    let head_end = svg.find('>').unwrap_or(svg.len());
    let (head, rest) = svg.split_at(head_end);
    format!("{}{}", head.replace("pt\"", "px\""), rest)
}

/// Stable cache key for formula content and visual parameters.
pub(crate) fn latex_cache_key(
    latex: &str,
    text_color: Hsla,
    font_size: f32,
    math_layout: MathLayout,
) -> String {
    let mut hasher = DefaultHasher::new();
    latex.hash(&mut hasher);
    svg_color(text_color).hash(&mut hasher);
    font_size.to_bits().hash(&mut hasher);
    // 同一段 LaTeX 在行内/行间下排版不同，必须分开缓存。
    math_layout.hash(&mut hasher);
    // 缓存格式版本：SVG 尺寸单位由 `pt` 改为 `px`（v2）、行内公式改用 Text 样式
    // （v3）、KaTeX 字体改为真正嵌入二进制（v4，见 Cargo.toml 的 debug-embed）
    // 之后，旧文件必须整体失效重生成——否则会把「退化 <text>」的旧图一直显示下去。
    "ratex-svg-embed-v4".hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn strip_display_indent(line: &str) -> Option<&str> {
    let indent = line.bytes().take_while(|byte| *byte == b' ').count();
    (indent <= 3).then_some(&line[indent..])
}

fn latex_cache_dir() -> anyhow::Result<PathBuf> {
    let root = ProjectDirs::from("app", "velora", "velora")
        .map(|dirs| dirs.cache_dir().to_path_buf())
        .unwrap_or_else(|| std::env::temp_dir().join("velora"));
    let dir = root.join("latex-svg");
    fs::create_dir_all(&dir)
        .with_context(|| format!("failed to create LaTeX SVG cache '{}'", dir.display()))?;
    Ok(dir)
}

fn svg_color(color: Hsla) -> String {
    let color = Rgba::from(color);
    format!(
        "rgba({},{},{},{})",
        color_channel(color.r),
        color_channel(color.g),
        color_channel(color.b),
        trim_float(f64::from(color.a.clamp(0.0, 1.0)))
    )
}

fn color_channel(channel: f32) -> u8 {
    (channel.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn trim_float(value: f64) -> String {
    let formatted = format!("{value:.3}");
    formatted
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
}

fn recolor_default_black(svg: &str, color: &str) -> String {
    svg.replace("rgba(0,0,0,1)", color)
        .replace("rgba(0, 0, 0, 1)", color)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::rgba;

    #[test]
    fn svg_root_uses_pixel_size_units() {
        // ratex 输出 `pt`，usvg/浏览器按 96/72 换算，公式会整体比请求字号大 1/3；
        // 根标签尺寸必须是 `px`，字号设置才等于看到的字号。
        let svg = render_latex_to_svg("x^2", Hsla::default(), 16.0, MathLayout::Display).expect("svg");
        let head = &svg[..svg.find('>').expect("svg root tag")];
        assert!(head.contains("px\""), "根标签尺寸应为 px: {head}");
        assert!(!head.contains("pt\""), "根标签不应残留 pt 单位: {head}");
        // 1 单位 = 该字号下的 1px：字号翻倍，尺寸也翻倍。
        let height_at = |size: f32| {
            let svg = render_latex_to_svg("\\frac{1}{3}", Hsla::default(), size, MathLayout::Display).expect("svg");
            let head = &svg[..svg.find('>').expect("svg root tag")];
            let after = head.split_once("height=\"").expect("height attr").1;
            let value: String = after
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
                .collect();
            value.parse::<f32>().expect("numeric height")
        };
        let single_em_height = height_at(16.0);
        assert!(
            single_em_height > 24.0 && single_em_height < 48.0,
            "16px 字号的 \\frac{{1}}{{3}} 高度应在 1.5~3em（含留白）之间，实测 {single_em_height}"
        );
        assert!(
            (height_at(32.0) - single_em_height * 2.0).abs() < 0.5,
            "尺寸应随字号线性变化"
        );
    }

    #[test]
    fn embeds_glyph_outlines_instead_of_falling_back_to_text() {
        // 回归护栏：算子（Σ/∫）的大字形来自 KaTeX_Size* 字体文件里的轮廓。
        // 一旦字体取不到，ratex 会静默把整条公式退化成 <text font-family="KaTeX_Size2">，
        // 系统字体按 1em 画出小号字形，而版式仍是行间尺寸——用户看到的就是「又小又歪」。
        let svg = render_latex_to_svg(
            "F = \\int_0^1 m a",
            Hsla::default(),
            16.0,
            MathLayout::Display,
        )
        .expect("渲染成功");
        assert!(svg.contains("<path"), "公式必须嵌成矢量轮廓，不能退化成 <text>");
        assert!(!svg.contains("<text"), "不应残留 <text> 元素：{svg:.160}");
    }

    #[test]
    fn inline_layout_shrinks_big_operators_to_text_style() {
        // 行内公式必须走 Text 样式：`\sum_0^1` 的行内高度应明显低于行间（Display）——
        // 否则行内那条 1.65em 的框会把公式整体压小、上下标位置也会怪。
        let display =
            render_latex_to_svg("\\sum_0^1", Hsla::default(), 16.0, MathLayout::Display).unwrap();
        let inline =
            render_latex_to_svg("\\sum_0^1", Hsla::default(), 16.0, MathLayout::Inline).unwrap();
        let display_height = svg_pixel_height(&display);
        let inline_height = svg_pixel_height(&inline);
        assert!(
            inline_height < display_height * 0.75,
            "行内高度 {inline_height} 应明显矮于行间 {display_height}"
        );

        // 同一段公式在两种排版下不能共用缓存。
        assert_ne!(
            latex_cache_key("\\sum_0^1", Hsla::default(), 16.0, MathLayout::Display),
            latex_cache_key("\\sum_0^1", Hsla::default(), 16.0, MathLayout::Inline)
        );
    }

    fn svg_pixel_height(svg: &str) -> f64 {
        let start = svg.find("height=\"").expect("SVG 应有 height 属性") + 8;
        let rest = &svg[start..];
        let end = rest.find('"').expect("height 属性应闭合");
        rest[..end]
            .trim_end_matches("px")
            .parse()
            .unwrap_or_else(|err| panic!("height 应是数字：{:?} ({err})", &rest[..end]))
    }

    #[test]
    fn parses_aligned_block_with_inline_fence_delimiters() {
        // Typora 写法：`$$` 后面直接跟 `\begin{aligned}`，末行 `\end{aligned}$$`。
        // 旧实现要求 `$$` 独占一行，这种块会原样显示成源码。
        let raw = "$$\\begin{aligned}\na &= b\\\\[2pt]\n&\\neq 0\n\\end{aligned}$$";
        let source = parse_display_math_source(raw).expect("应识别为公式块");
        assert_eq!(source.raw, raw);
        assert_eq!(
            source.body,
            "\\begin{aligned}\na &= b\\\\[2pt]\n&\\neq 0\n\\end{aligned}"
        );
        assert!(
            render_latex_to_svg(&source.body, Hsla::default(), 16.0, MathLayout::Display).is_ok(),
            "去掉两侧 $$ 后应能交给 ratex 渲染"
        );

        // `$$` 独占一行的老写法仍然支持。
        let classic = parse_display_math_source("$$\nx^2\n$$").expect("老写法");
        assert_eq!(classic.body, "x^2");

        // 没闭合的仍然是普通文本。
        assert!(parse_display_math_source("$$\\begin{aligned}\nx &= y").is_none());
    }

    #[test]
    fn parses_single_line_display_math() {
        let parsed = parse_display_math_source("$$x^2$$").expect("display math");
        assert_eq!(parsed.body, "x^2");
        assert_eq!(parsed.raw, "$$x^2$$");
    }

    #[test]
    fn display_math_segments_account_for_the_whole_source_span() {
        // 用户报修（cases/03-formula-tail.md）：`$$x^2$$ LOST_SENTINEL` 里闭合 `$$`
        // 之后的文字被单行解析分支整个丢掉，界面上看不见。解析必须交代整个跨度，
        // 尾部文字作为行内内容交给渲染端。
        let segments = parse_display_math_segments("$$x^2$$ LOST_SENTINEL").expect("segments");
        assert_eq!(
            segments,
            vec![
                DisplayMathSegment::Formula {
                    raw: "$$x^2$$".to_string(),
                    body: "x^2".to_string(),
                },
                DisplayMathSegment::Text {
                    markdown: " LOST_SENTINEL".to_string(),
                },
            ]
        );
        // 「第一个公式」视图保持旧口径（分类器与草稿提取都靠它）。
        assert_eq!(
            parse_display_math_source("$$x^2$$ LOST_SENTINEL").expect("view").body,
            "x^2"
        );

        // 同一行两个 `$$…$$` 都要渲染：公式、空白、公式。
        let segments = parse_display_math_segments("$$a^2$$ $$b^2$$").expect("segments");
        assert_eq!(
            segments,
            vec![
                DisplayMathSegment::Formula {
                    raw: "$$a^2$$".to_string(),
                    body: "a^2".to_string(),
                },
                DisplayMathSegment::Formula {
                    raw: "$$b^2$$".to_string(),
                    body: "b^2".to_string(),
                },
            ]
        );

        // 多行块尾随文字：闭合 `$$` 之后的按文字段交出，不再整块判失败。
        let segments = parse_display_math_segments("$$\nx^2\n$$ = 5").expect("segments");
        assert_eq!(segments.len(), 2);
        assert_eq!(
            segments[0],
            DisplayMathSegment::Formula {
                raw: "$$\nx^2\n$$".to_string(),
                body: "x^2".to_string(),
            }
        );

        // 没有公式的源码不是数学块；文字开头的 `text $$x$$` 也不走块级渲染
        // （`parse_display_math_source` 保持旧严格口径：首段必须是公式）。
        assert!(parse_display_math_segments("plain text").is_none());
        assert!(parse_display_math_source("text $$x$$").is_none());
        assert!(parse_display_math_source("$$x^2$$ LOST_SENTINEL").is_some());
    }

    #[test]
    fn parses_multiline_display_math() {
        let parsed = parse_display_math_source("$$\n\\int_0^1 x^2 dx\n$$").expect("display math");
        assert_eq!(parsed.body, "\\int_0^1 x^2 dx");
    }

    #[test]
    fn rejects_unclosed_display_math() {
        assert!(parse_display_math_source("$$\n\\frac{1}{2}").is_none());
    }

    #[test]
    fn cache_key_changes_with_theme_inputs() {
        let first = latex_cache_key("\\frac{1}{2}", Hsla::from(rgba(0xffffffff)), 18.0, MathLayout::Display);
        let second = latex_cache_key("\\frac{1}{2}", Hsla::from(rgba(0x000000ff)), 18.0, MathLayout::Display);
        assert_ne!(first, second);
    }

    #[test]
    fn display_math_font_size_matches_inline_math() {
        // 用户报修：块级公式字号明显大于正文/行内公式，两者应同字号。
        assert_eq!(display_math_font_size(20.0), inline_math_font_size(20.0));
        assert!((display_math_font_size(20.0) - 22.4).abs() < 0.001);
    }

    #[test]
    fn inline_math_font_size_scales_base_text_size() {
        assert!((inline_math_font_size(20.0) - 22.4).abs() < 0.001);
    }

    #[test]
    fn renders_basic_formula_svg() {
        let svg =
            render_latex_to_svg("\\frac{1}{2}", Hsla::from(rgba(0xffffffff)), 18.0, MathLayout::Display).expect("svg");
        assert!(svg.contains("<svg"));
        assert!(svg.contains("</svg>"));
    }

    #[test]
    fn invalid_latex_returns_error() {
        assert!(render_latex_to_svg("\\frac{a}", Hsla::from(rgba(0xffffffff)), 18.0, MathLayout::Display).is_err());
    }
}



