use super::*;

pub(crate) fn parse_html_document(raw_source: &str) -> HtmlDocument {
    if raw_source.trim().is_empty() {
        return HtmlDocument::raw(raw_source);
    }

    let dom = parse_html_fragment(raw_source);
    let nodes = map_html_fragment(&dom.document);

    if nodes.is_empty() {
        return HtmlDocument {
            raw_source: raw_source.to_string(),
            nodes,
            safety: HtmlSafetyClass::Empty,
        };
    }

    if nodes
        .iter()
        .all(|node| matches!(node.kind, HtmlNodeKind::RawTextBlock))
    {
        return HtmlDocument::raw(raw_source);
    }

    HtmlDocument {
        raw_source: raw_source.to_string(),
        nodes,
        safety: HtmlSafetyClass::Semantic,
    }
}

/// Parses a fragment the way a browser does: `html5ever` never fails, it
/// recovers from malformed markup instead of rejecting it.
fn parse_html_fragment(raw_source: &str) -> RcDom {
    let mut source = std::io::Cursor::new(raw_source.as_bytes());
    parse_document(RcDom::default(), Default::default())
        .from_utf8()
        .read_from(&mut source)
        .unwrap_or_else(|_| RcDom::default())
}

/// Maps the fragment onto the semantic tree. `html5ever` always builds a full
/// document (`html` > `head`/`body`), so those wrapper elements are unwrapped:
/// a leading `<script>` or `<style>` lands in `head`, and the fragment's own
/// markup is what must be classified.
fn map_html_fragment(document: &Handle) -> Vec<HtmlNode> {
    let mut nodes = Vec::new();
    collect_fragment_nodes(document, &mut nodes);
    nodes
}

fn collect_fragment_nodes(handle: &Handle, nodes: &mut Vec<HtmlNode>) {
    for child in handle.children.borrow().iter() {
        if let NodeData::Element { name, .. } = &child.data {
            let local = name.local.as_ref();
            if matches!(local, "html" | "head" | "body") {
                collect_fragment_nodes(child, nodes);
                continue;
            }
        }
        map_dom_node(child, nodes);
    }
}

fn map_dom_children(parent: &Handle, nodes: &mut Vec<HtmlNode>) {
    for child in parent.children.borrow().iter() {
        map_dom_node(child, nodes);
    }
}

/// Classifies one DOM node: an allowlisted element becomes a semantic node,
/// every other node keeps its serialized markup as raw text.
fn map_dom_node(node: &Handle, nodes: &mut Vec<HtmlNode>) {
    match &node.data {
        NodeData::Text { contents } => {
            let text = contents.borrow().to_string();
            if !text.is_empty() {
                nodes.push(HtmlNode {
                    kind: HtmlNodeKind::InlineSemantic,
                    tag_name: "#text".into(),
                    attrs: Vec::new(),
                    children: Vec::new(),
                    raw_source: text,
                });
            }
        }
        NodeData::Element { name, attrs, .. } => {
            let tag_name = name.local.to_string();
            let attrs = dom_attrs(&attrs.borrow());
            if is_safe_tag(&tag_name) && !has_dangerous_attrs(&attrs) {
                let mut children = Vec::new();
                map_dom_children(node, &mut children);
                nodes.push(HtmlNode {
                    kind: if is_inline_tag(&tag_name) {
                        HtmlNodeKind::InlineSemantic
                    } else {
                        HtmlNodeKind::BlockSemantic
                    },
                    tag_name,
                    attrs,
                    children,
                    raw_source: serialize_handle(node),
                });
            } else {
                nodes.push(raw_node(serialize_handle(node)));
            }
        }
        NodeData::Comment { contents } => {
            nodes.push(raw_node(format!("<!--{contents}-->")));
        }
        NodeData::Doctype { name, .. } => {
            nodes.push(raw_node(format!("<!DOCTYPE {name}>")));
        }
        NodeData::ProcessingInstruction { target, contents } => {
            nodes.push(raw_node(format!("<?{target} {contents}?>")));
        }
        NodeData::Document => map_dom_children(node, nodes),
    }
}

fn dom_attrs(attrs: &[Attribute]) -> Vec<HtmlAttr> {
    attrs
        .iter()
        .map(|attr| {
            let name = attr.name.local.to_string();
            let value = attr.value.to_string();
            HtmlAttr {
                raw_source: format!("{}=\"{}\"", name, escape_html_attr(&value)),
                name,
                value: Some(value),
            }
        })
        .collect()
}

fn serialize_handle(node: &Handle) -> String {
    let mut bytes = Vec::new();
    let serializable = SerializableHandle::from(node.clone());
    // `SerializeOpts::default()` is `ChildrenOnly`, which drops the node itself.
    let opts = SerializeOpts {
        traversal_scope: TraversalScope::IncludeNode,
        ..Default::default()
    };
    let _ = serialize(&mut bytes, &serializable, opts);
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Rewrites an HTML fragment for document export: safe semantic nodes keep
/// their HTML shape, while raw text nodes are escaped so browsers cannot
/// execute or interpret them.
pub(crate) fn sanitize_html_for_export(raw_source: &str) -> String {
    if let Some(image) = parse_html_image_block(raw_source) {
        return image.to_sanitized_html_with_src(&image.src);
    }

    let document = parse_html_document(raw_source);
    if document.renders_nothing() {
        return String::new();
    }
    if !document.is_semantic() {
        return format!(
            "<pre class=\"vlt-raw-html\">{}</pre>",
            escape_html(raw_source)
        );
    }

    document
        .nodes
        .iter()
        .map(sanitize_node_for_export)
        .collect::<String>()
}

/// Parses the safe visual subset of a semantic node's `style` attribute.
pub(crate) fn style_for_node(node: &HtmlNode) -> HtmlInlineStyle {
    if node.kind == HtmlNodeKind::RawTextBlock {
        return HtmlInlineStyle::default();
    }

    let mut parsed = attr_value(node, "style")
        .map(parse_inline_style)
        .unwrap_or_default();
    if parsed.text_align.is_none() {
        parsed.text_align = attr_value(node, "align").and_then(parse_text_align);
    }
    if parsed.text_align.is_none() && node.tag_name == "center" {
        parsed.text_align = Some(HtmlTextAlign::Center);
    }
    parsed
}

fn sanitize_node_for_export(node: &HtmlNode) -> String {
    if node.kind == HtmlNodeKind::RawTextBlock {
        return format!(
            "<span class=\"vlt-raw-html\">{}</span>",
            escape_html(&node.raw_source)
        );
    }

    if node.tag_name == "#text" {
        return escape_html(&node.raw_source);
    }

    if is_void_tag(&node.tag_name) {
        return sanitized_open_tag(node);
    }

    let Some(_open_end) = node.raw_source.find('>').map(|index| index + 1) else {
        return escape_html(&node.raw_source);
    };
    let close_start =
        find_closing_tag_start(&node.raw_source, &node.tag_name).unwrap_or(node.raw_source.len());
    let close = &node.raw_source[close_start..];
    let children = node
        .children
        .iter()
        .map(sanitize_node_for_export)
        .collect::<String>();
    format!("{}{children}{close}", sanitized_open_tag(node))
}

fn sanitized_open_tag(node: &HtmlNode) -> String {
    if node.tag_name == "img"
        && let Some(image) = parse_html_image_block(&node.raw_source)
    {
        return image.to_sanitized_html_with_src(&image.src);
    }

    let mut open = format!("<{}", node.tag_name);
    for attr in &node.attrs {
        if attr.name == "style" {
            continue;
        }
        open.push(' ');
        open.push_str(&attr.raw_source);
    }
    if let Some(style) = style_for_node(node).to_css() {
        open.push_str(" style=\"");
        open.push_str(&escape_html_attr(&style));
        open.push('"');
    }
    open.push('>');
    open
}

fn find_closing_tag_start(raw_source: &str, tag_name: &str) -> Option<usize> {
    let needle = format!("</{tag_name}");
    raw_source.to_ascii_lowercase().rfind(&needle)
}

pub(crate) fn escape_html_attr(value: &str) -> String {
    let mut escaped = String::new();
    for ch in value.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '"' => escaped.push_str("&quot;"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

fn escape_html(value: &str) -> String {
    let mut escaped = String::new();
    for ch in value.chars() {
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

/// Peek the next char at `index` without advancing. Returns `None` at EOF.
#[inline]
fn peek_char(source: &str, index: usize) -> Option<char> {
    source[index..].chars().next()
}

/// Advance `index` past the next char and return it. Returns `None` at EOF.
/// Encapsulates the byte-index ↔ UTF-8-boundary invariant so callers that
/// don't need the char's value can't drift into a panic by hand-incrementing
/// `index` by anything other than `ch.len_utf8()`. Loops that *do* need the
/// char for a check should peek with [`peek_char`], inspect the value, and
/// then advance with `index += ch.len_utf8()` — see [`parse_html_attrs`] —
/// so the char is read only once per iteration.
#[inline]
fn advance_char(source: &str, index: &mut usize) -> Option<char> {
    let ch = source[*index..].chars().next()?;
    *index += ch.len_utf8();
    Some(ch)
}

pub(crate) fn parse_html_attrs(source: &str) -> Vec<HtmlAttr> {
    let mut attrs = Vec::new();
    let mut index = 0usize;
    while index < source.len() {
        while let Some(ch) = peek_char(source, index).filter(|c| c.is_whitespace() || *c == '/') {
            index += ch.len_utf8();
        }
        if index >= source.len() {
            break;
        }

        let start = index;
        while let Some(ch) = peek_char(source, index) {
            if ch.is_whitespace() || ch == '=' || ch == '/' {
                break;
            }
            index += ch.len_utf8();
        }
        let name_end = index;
        if name_end == start {
            // Lone separator we couldn't classify — consume one char and retry.
            advance_char(source, &mut index);
            continue;
        }

        while let Some(ch) = peek_char(source, index).filter(|c| c.is_whitespace()) {
            index += ch.len_utf8();
        }

        let mut value = None;
        if source[index..].starts_with('=') {
            index += 1;
            while let Some(ch) = peek_char(source, index).filter(|c| c.is_whitespace()) {
                index += ch.len_utf8();
            }

            if let Some(quote) = peek_char(source, index).filter(|c| *c == '"' || *c == '\'') {
                index += quote.len_utf8();
                let value_start = index;
                while let Some(ch) = peek_char(source, index) {
                    if ch == quote {
                        break;
                    }
                    index += ch.len_utf8();
                }
                value = Some(source[value_start..index].to_string());
                if index < source.len() {
                    index += quote.len_utf8();
                }
            } else if peek_char(source, index).is_some() {
                let value_start = index;
                while let Some(ch) = peek_char(source, index) {
                    if ch.is_whitespace() || ch == '/' {
                        break;
                    }
                    index += ch.len_utf8();
                }
                value = Some(source[value_start..index].to_string());
            }
        }

        attrs.push(HtmlAttr {
            name: source[start..name_end].to_ascii_lowercase(),
            value,
            raw_source: source[start..index].to_string(),
        });
    }

    attrs
}

pub(crate) fn raw_node(raw_source: String) -> HtmlNode {
    HtmlNode {
        kind: HtmlNodeKind::RawTextBlock,
        tag_name: "#raw".into(),
        attrs: Vec::new(),
        children: Vec::new(),
        raw_source,
    }
}

pub(crate) fn has_dangerous_attrs(attrs: &[HtmlAttr]) -> bool {
    attrs.iter().any(|attr| {
        attr.name.starts_with("on")
            || attr.value.as_deref().is_some_and(|value| {
                let normalized = value
                    .chars()
                    .filter(|ch| !ch.is_whitespace() && *ch != '\0')
                    .collect::<String>()
                    .to_ascii_lowercase();
                matches!(
                    attr.name.as_str(),
                    "href" | "src" | "action" | "formaction" | "xlink:href"
                ) && normalized.starts_with("javascript:")
            })
    })
}

pub(crate) fn attr_value<'a>(node: &'a HtmlNode, name: &str) -> Option<&'a str> {
    node.attrs
        .iter()
        .find(|attr| attr.name == name)
        .and_then(|attr| attr.value.as_deref())
}

pub(crate) fn parse_html_image_block(raw_source: &str) -> Option<HtmlImageBlock> {
    let trimmed = raw_source.trim();
    if trimmed.is_empty() {
        return None;
    }

    let dom = parse_html_fragment(trimmed);
    let nodes = map_html_fragment(&dom.document);
    let mut image: Option<&[HtmlAttr]> = None;
    for node in &nodes {
        if node.tag_name == "#text" && node.raw_source.trim().is_empty() {
            continue;
        }
        if image.is_none() && node.tag_name == "img" && node.kind != HtmlNodeKind::RawTextBlock {
            image = Some(&node.attrs);
            continue;
        }
        return None;
    }
    let attrs = image?;

    let src = attr_value_in_attrs(attrs, "src")?.trim().to_string();
    if src.is_empty() {
        return None;
    }

    let alt = attr_value_in_attrs(attrs, "alt")
        .unwrap_or_default()
        .to_string();
    let zoom = attr_value_in_attrs(attrs, "style")
        .and_then(parse_html_zoom)
        .unwrap_or(1.0);

    Some(HtmlImageBlock { src, alt, zoom })
}

fn attr_value_in_attrs<'a>(attrs: &'a [HtmlAttr], name: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|attr| attr.name == name)
        .and_then(|attr| attr.value.as_deref())
}

pub(crate) fn parse_html_zoom(style: &str) -> Option<f32> {
    for declaration in style.split(';') {
        let Some((property, value)) = declaration.split_once(':') else {
            continue;
        };
        if !property.trim().eq_ignore_ascii_case("zoom") {
            continue;
        }

        let value = value.trim();
        let parsed = if let Some(percent) = value.strip_suffix('%') {
            parse_css_number(percent)? / 100.0
        } else {
            parse_css_number(value)?
        };
        return Some(parsed.clamp(0.1, 3.0));
    }
    None
}

pub(crate) fn parse_inline_style(style: &str) -> HtmlInlineStyle {
    let mut parsed = HtmlInlineStyle::default();
    for declaration in style.split(';') {
        let Some((property, value)) = declaration.split_once(':') else {
            continue;
        };
        let property = property.trim().to_ascii_lowercase();
        let value = value.trim();
        match property.as_str() {
            "color" => {
                if let Some(color) = parse_css_color(value) {
                    parsed.color = Some(color);
                }
            }
            "background-color" => {
                if let Some(color) = parse_css_color(value) {
                    parsed.background_color = Some(color);
                }
            }
            "font-size" => {
                if let Some(size) = parse_css_font_size(value) {
                    parsed.font_size = Some(size);
                }
            }
            "text-align" => {
                if let Some(align) = parse_text_align(value) {
                    parsed.text_align = Some(align);
                }
            }
            _ => {}
        }
    }
    parsed
}

fn parse_css_color(value: &str) -> Option<HtmlCssColor> {
    let value = value.trim();
    if value.eq_ignore_ascii_case("currentcolor") {
        return Some(HtmlCssColor::CurrentColor);
    }
    if value.eq_ignore_ascii_case("transparent") {
        return Some(HtmlCssColor::Rgba(HtmlCssRgba {
            red: 0,
            green: 0,
            blue: 0,
            alpha: 0.0,
        }));
    }
    if let Some(hex) = value.strip_prefix('#')
        && let Ok((red, green, blue, alpha)) = parse_hash_color(hex.as_bytes())
    {
        return Some(HtmlCssColor::Rgba(HtmlCssRgba {
            red,
            green,
            blue,
            alpha,
        }));
    }
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphabetic() || ch == '-')
        && let Ok((red, green, blue)) = parse_named_color(value)
    {
        return Some(HtmlCssColor::Rgba(HtmlCssRgba {
            red,
            green,
            blue,
            alpha: 1.0,
        }));
    }
    parse_rgb_color(value).or_else(|| parse_hsl_color(value))
}

fn parse_rgb_color(value: &str) -> Option<HtmlCssColor> {
    let args = css_function_args(value, &["rgb", "rgba"])?;
    let parts = css_function_parts(args);
    if parts.len() < 3 {
        return None;
    }

    let red = parse_rgb_component(&parts[0])?;
    let green = parse_rgb_component(&parts[1])?;
    let blue = parse_rgb_component(&parts[2])?;
    let alpha = parts
        .get(3)
        .and_then(|part| parse_alpha_component(part))
        .unwrap_or(1.0);
    Some(HtmlCssColor::Rgba(HtmlCssRgba {
        red,
        green,
        blue,
        alpha,
    }))
}

fn parse_hsl_color(value: &str) -> Option<HtmlCssColor> {
    let args = css_function_args(value, &["hsl", "hsla"])?;
    let parts = css_function_parts(args);
    if parts.len() < 3 {
        return None;
    }

    let hue = parse_hue(&parts[0])?;
    let saturation = parse_percent_component(&parts[1])?;
    let lightness = parse_percent_component(&parts[2])?;
    let alpha = parts
        .get(3)
        .and_then(|part| parse_alpha_component(part))
        .unwrap_or(1.0);
    let (red, green, blue) = hsl_to_rgb(hue, saturation, lightness);
    Some(HtmlCssColor::Rgba(HtmlCssRgba {
        red,
        green,
        blue,
        alpha,
    }))
}

fn css_function_args<'a>(value: &'a str, names: &[&str]) -> Option<&'a str> {
    let open = value.find('(')?;
    let close = value.rfind(')')?;
    if close <= open || !value[close + 1..].trim().is_empty() {
        return None;
    }
    let name = value[..open].trim();
    names
        .iter()
        .any(|candidate| name.eq_ignore_ascii_case(candidate))
        .then_some(&value[open + 1..close])
}

fn css_function_parts(args: &str) -> Vec<String> {
    if args.contains(',') {
        return args
            .split(',')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .map(str::to_string)
            .collect();
    }

    let normalized = args.replace('/', " / ");
    normalized
        .split_whitespace()
        .filter(|token| *token != "/")
        .map(str::to_string)
        .collect()
}

fn parse_rgb_component(value: &str) -> Option<u8> {
    if let Some(percent) = value.trim().strip_suffix('%') {
        let value = parse_css_number(percent)?;
        return Some((value.clamp(0.0, 100.0) * 255.0 / 100.0).round() as u8);
    }

    let value = parse_css_number(value)?;
    Some(value.clamp(0.0, 255.0).round() as u8)
}

fn parse_percent_component(value: &str) -> Option<f32> {
    let value = value.trim().strip_suffix('%')?;
    Some((parse_css_number(value)? / 100.0).clamp(0.0, 1.0))
}

fn parse_alpha_component(value: &str) -> Option<f32> {
    if let Some(percent) = value.trim().strip_suffix('%') {
        return Some((parse_css_number(percent)? / 100.0).clamp(0.0, 1.0));
    }
    Some(parse_css_number(value)?.clamp(0.0, 1.0))
}

fn parse_hue(value: &str) -> Option<f32> {
    let trimmed = value.trim().to_ascii_lowercase();
    if let Some(value) = trimmed.strip_suffix("deg") {
        return parse_css_number(value);
    }
    if let Some(value) = trimmed.strip_suffix("turn") {
        return Some(parse_css_number(value)? * 360.0);
    }
    if let Some(value) = trimmed.strip_suffix("rad") {
        return Some(parse_css_number(value)? * 180.0 / std::f32::consts::PI);
    }
    parse_css_number(&trimmed)
}

fn hsl_to_rgb(hue_degrees: f32, saturation: f32, lightness: f32) -> (u8, u8, u8) {
    let hue = hue_degrees.rem_euclid(360.0) / 60.0;
    let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let x = chroma * (1.0 - (hue % 2.0 - 1.0).abs());
    let (red, green, blue) = match hue.floor() as i32 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    let m = lightness - chroma / 2.0;
    (
        ((red + m).clamp(0.0, 1.0) * 255.0).round() as u8,
        ((green + m).clamp(0.0, 1.0) * 255.0).round() as u8,
        ((blue + m).clamp(0.0, 1.0) * 255.0).round() as u8,
    )
}

fn parse_css_font_size(value: &str) -> Option<HtmlCssFontSize> {
    let trimmed = value.trim().to_ascii_lowercase();
    match trimmed.as_str() {
        "xx-small" => return Some(HtmlCssFontSize::Keyword(HtmlCssFontSizeKeyword::XxSmall)),
        "x-small" => return Some(HtmlCssFontSize::Keyword(HtmlCssFontSizeKeyword::XSmall)),
        "small" => return Some(HtmlCssFontSize::Keyword(HtmlCssFontSizeKeyword::Small)),
        "medium" => return Some(HtmlCssFontSize::Keyword(HtmlCssFontSizeKeyword::Medium)),
        "large" => return Some(HtmlCssFontSize::Keyword(HtmlCssFontSizeKeyword::Large)),
        "x-large" => return Some(HtmlCssFontSize::Keyword(HtmlCssFontSizeKeyword::XLarge)),
        "xx-large" => return Some(HtmlCssFontSize::Keyword(HtmlCssFontSizeKeyword::XxLarge)),
        "smaller" => return Some(HtmlCssFontSize::Keyword(HtmlCssFontSizeKeyword::Smaller)),
        "larger" => return Some(HtmlCssFontSize::Keyword(HtmlCssFontSizeKeyword::Larger)),
        _ => {}
    }

    if let Some(value) = trimmed.strip_suffix("rem") {
        return Some(HtmlCssFontSize::Rem(parse_non_negative_css_number(value)?));
    }
    if let Some(value) = trimmed.strip_suffix("em") {
        return Some(HtmlCssFontSize::Em(parse_non_negative_css_number(value)?));
    }
    if let Some(value) = trimmed.strip_suffix("px") {
        return Some(HtmlCssFontSize::Px(parse_non_negative_css_number(value)?));
    }
    if let Some(value) = trimmed.strip_suffix('%') {
        return Some(HtmlCssFontSize::Percent(parse_non_negative_css_number(
            value,
        )?));
    }
    None
}

fn parse_non_negative_css_number(value: &str) -> Option<f32> {
    let value = parse_css_number(value)?;
    (value >= 0.0).then_some(value)
}

fn parse_css_number(value: &str) -> Option<f32> {
    let value = value.trim().parse::<f32>().ok()?;
    value.is_finite().then_some(value)
}

pub(crate) fn css_number(value: f32) -> String {
    let mut formatted = format!("{:.3}", value);
    while formatted.contains('.') && formatted.ends_with('0') {
        formatted.pop();
    }
    if formatted.ends_with('.') {
        formatted.pop();
    }
    formatted
}

fn is_safe_tag(name: &str) -> bool {
    is_inline_tag(name) || is_block_tag(name)
}

pub(crate) fn is_inline_tag(name: &str) -> bool {
    matches!(
        name,
        "a" | "strong"
            | "em"
            | "b"
            | "i"
            | "u"
            | "mark"
            | "del"
            | "ins"
            | "code"
            | "kbd"
            | "sup"
            | "sub"
            | "small"
            | "abbr"
            | "dfn"
            | "time"
            | "q"
            | "span"
    )
}

/// 行内 void / 自闭 HTML 标签的分类：**行内解析器识别 void 标签的唯一表**。
/// 没有这张表时 `<br>` 会被 autolink 规则吃掉尖括号（用户报修 cases/11-table-br.md：
/// 单元格里渲染成带下划线的 "br"）。下一个要支持的 void 标签（`<hr>`、`<wbr>`、
/// 行内 `<img>`……）进表即被识别，不需要再补一处特例。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum VoidInlineHtmlKind {
    /// 渲染为断点（序列化统一写回 `<br>`）。
    LineBreak,
    /// 行内暂不特殊渲染（`<wbr>` 要真正的软断点、`<img>` 要行内图片部件，都不在这一档）：
    /// 整体保持字面源码，但**绝不**按 autolink 解析。
    Literal,
}

pub(crate) fn void_inline_html_tag(name: &str) -> Option<VoidInlineHtmlKind> {
    match name {
        "br" => Some(VoidInlineHtmlKind::LineBreak),
        "area" | "base" | "basefont" | "col" | "embed" | "frame" | "hr" | "img" | "input"
        | "link" | "meta" | "param" | "source" | "track" | "wbr" => {
            Some(VoidInlineHtmlKind::Literal)
        }
        _ => None,
    }
}

fn is_block_tag(name: &str) -> bool {
    matches!(
        name,
        "center"
            | "div"
            | "p"
            | "blockquote"
            | "hr"
            | "br"
            | "details"
            | "summary"
            | "figure"
            | "figcaption"
            | "table"
            | "thead"
            | "tbody"
            | "tfoot"
            | "tr"
            | "th"
            | "td"
            | "img"
            | "pre"
    )
}

fn is_void_tag(name: &str) -> bool {
    matches!(name, "br" | "hr" | "img")
}

/// CommonMark HTML block kind 6 names: a line that starts with one of these
/// tags opens an HTML block, and the block ends at the next blank line.
pub(crate) fn is_block_level_html_tag(name: &str) -> bool {
    matches!(
        name,
        "address"
            | "article"
            | "aside"
            | "base"
            | "basefont"
            | "blockquote"
            | "body"
            | "caption"
            | "center"
            | "col"
            | "colgroup"
            | "dd"
            | "details"
            | "dialog"
            | "dir"
            | "div"
            | "dl"
            | "dt"
            | "fieldset"
            | "figcaption"
            | "figure"
            | "footer"
            | "form"
            | "frame"
            | "frameset"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "head"
            | "header"
            | "hr"
            | "html"
            | "iframe"
            | "legend"
            | "li"
            | "link"
            | "main"
            | "menu"
            | "menuitem"
            | "nav"
            | "noframes"
            | "ol"
            | "optgroup"
            | "option"
            | "p"
            | "param"
            | "search"
            | "section"
            | "summary"
            | "table"
            | "tbody"
            | "td"
            | "tfoot"
            | "th"
            | "thead"
            | "title"
            | "tr"
            | "track"
            | "ul"
    )
}

/// CommonMark HTML block kind 1 names: the block runs to the matching end tag
/// and blank lines do not end it.
pub(crate) fn is_raw_text_html_tag(name: &str) -> bool {
    matches!(name, "script" | "style" | "pre" | "textarea")
}

/// HTML containers whose content is expected to span blank lines. Velora
/// renders them as one native block, so their region runs to the closing tag
/// instead of ending at the first blank line.
pub(crate) fn is_html_container_tag(name: &str) -> bool {
    matches!(name, "details" | "figure" | "table")
}

fn parse_text_align(value: &str) -> Option<HtmlTextAlign> {
    match value.trim().to_ascii_lowercase().as_str() {
        "left" => Some(HtmlTextAlign::Left),
        "center" => Some(HtmlTextAlign::Center),
        "right" => Some(HtmlTextAlign::Right),
        _ => None,
    }
}
