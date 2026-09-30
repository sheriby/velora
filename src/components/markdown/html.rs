//! Native-safe HTML classification for Markdown raw HTML blocks.
//!
//! Parsing follows the HTML living standard through `html5ever`, the parser
//! Servo uses. The resulting DOM is classified into a conservative semantic
//! tree of nodes that GPUI can render natively; anything risky or outside the
//! allowlist keeps its serialized markup as raw text.

pub(super) use cssparser::color::{parse_hash_color, parse_named_color};
pub(super) use html5ever::serialize::{SerializeOpts, TraversalScope, serialize};
pub(super) use html5ever::tendril::TendrilSink;
pub(super) use html5ever::{Attribute, parse_document};
pub(super) use markup5ever_rcdom::{Handle, NodeData, RcDom, SerializableHandle};

/// Safety classification for an HTML fragment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HtmlSafetyClass {
    /// The fragment has at least one safe semantic node.
    Semantic,
    /// The fragment draws nothing: closing tags without a matching opening tag,
    /// such as a stray `</div>`. The source text is kept for editing.
    Empty,
    /// The entire fragment must be shown and stored as plain raw text.
    RawTextBlock,
}

/// Broad rendering category of a parsed HTML node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum HtmlNodeKind {
    /// Safe inline tag or text that can be represented with text runs.
    InlineSemantic,
    /// Safe block tag that maps to a native block-like GPUI element.
    BlockSemantic,
    /// Opaque raw source that must not be interpreted as HTML.
    RawTextBlock,
}

/// One source attribute from an HTML tag.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HtmlAttr {
    /// Lowercase attribute name used for safety checks.
    pub(crate) name: String,
    /// Parsed attribute value without surrounding quotes.
    pub(crate) value: Option<String>,
    /// Exact attribute source text.
    pub(crate) raw_source: String,
}

/// Parsed CSS color value from a safe inline `style` attribute.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum HtmlCssColor {
    /// The CSS `currentColor` keyword.
    CurrentColor,
    /// An sRGB color with alpha.
    Rgba(HtmlCssRgba),
}

/// RGBA channels normalized enough for both GPUI rendering and export CSS.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct HtmlCssRgba {
    pub(crate) red: u8,
    pub(crate) green: u8,
    pub(crate) blue: u8,
    pub(crate) alpha: f32,
}

/// Parsed CSS font-size value from a safe inline `style` attribute.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum HtmlCssFontSize {
    Px(f32),
    Em(f32),
    Rem(f32),
    Percent(f32),
    Keyword(HtmlCssFontSizeKeyword),
}

/// CSS absolute and relative font-size keywords supported by rendered HTML.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HtmlCssFontSizeKeyword {
    XxSmall,
    XSmall,
    Small,
    Medium,
    Large,
    XLarge,
    XxLarge,
    Smaller,
    Larger,
}

/// Horizontal text alignment from `text-align` or the legacy `align` attribute.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HtmlTextAlign {
    Left,
    Center,
    Right,
}

/// Whitelisted visual CSS parsed from a safe HTML `style` attribute.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct HtmlInlineStyle {
    pub(crate) color: Option<HtmlCssColor>,
    pub(crate) background_color: Option<HtmlCssColor>,
    pub(crate) font_size: Option<HtmlCssFontSize>,
    pub(crate) text_align: Option<HtmlTextAlign>,
}

impl Eq for HtmlInlineStyle {}

/// Safe data extracted from a standalone HTML `<img>` block.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct HtmlImageBlock {
    pub(crate) src: String,
    pub(crate) alt: String,
    pub(crate) zoom: f32,
}

impl HtmlImageBlock {
    pub(crate) fn zoom_factor(&self) -> f32 {
        self.zoom.clamp(0.1, 3.0)
    }

    pub(crate) fn to_sanitized_html_with_src(&self, src: &str) -> String {
        let mut html = format!("<img src=\"{}\"", escape_html_attr(src));
        if !self.alt.is_empty() {
            html.push_str(" alt=\"");
            html.push_str(&escape_html_attr(&self.alt));
            html.push('"');
        }
        if (self.zoom_factor() - 1.0).abs() > f32::EPSILON {
            html.push_str(" style=\"zoom: ");
            html.push_str(&css_number(self.zoom_factor() * 100.0));
            html.push_str("%;\"");
        }
        html.push('>');
        html
    }
}

/// A classified HTML node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HtmlNode {
    /// Rendering category selected by the safety policy.
    pub(crate) kind: HtmlNodeKind,
    /// Lowercase tag name, or `#text` for text nodes.
    pub(crate) tag_name: String,
    /// Safe attributes retained as semantic data.
    pub(crate) attrs: Vec<HtmlAttr>,
    /// Classified child nodes. Empty for raw text nodes.
    pub(crate) children: Vec<HtmlNode>,
    /// Serialized markup or decoded text covered by this node.
    pub(crate) raw_source: String,
}

/// Classified HTML fragment plus its preserved source text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HtmlDocument {
    /// Exact source string used for serialization and raw editing.
    pub(crate) raw_source: String,
    /// Root-level classified nodes.
    pub(crate) nodes: Vec<HtmlNode>,
    /// Overall fragment safety.
    pub(crate) safety: HtmlSafetyClass,
}

impl HtmlDocument {
    pub(crate) fn raw(raw_source: impl Into<String>) -> Self {
        let raw_source = raw_source.into();
        Self {
            nodes: vec![raw_node(raw_source.clone())],
            safety: HtmlSafetyClass::RawTextBlock,
            raw_source,
        }
    }

    pub(crate) fn is_semantic(&self) -> bool {
        self.safety == HtmlSafetyClass::Semantic
    }

    /// True when the fragment has nothing to draw: stray closing tags parse to
    /// an empty document.
    pub(crate) fn renders_nothing(&self) -> bool {
        self.safety == HtmlSafetyClass::Empty
    }
}

impl HtmlCssColor {
    pub(crate) fn to_css(self) -> String {
        match self {
            Self::CurrentColor => "currentColor".to_string(),
            Self::Rgba(color) => format!(
                "rgba({},{},{},{:.3})",
                color.red,
                color.green,
                color.blue,
                color.alpha.clamp(0.0, 1.0)
            ),
        }
    }
}

impl HtmlCssFontSize {
    pub(crate) fn resolve(self, parent_px: f32, root_px: f32) -> f32 {
        let resolved = match self {
            Self::Px(value) => value,
            Self::Em(value) => parent_px * value,
            Self::Rem(value) => root_px * value,
            Self::Percent(value) => parent_px * value / 100.0,
            Self::Keyword(keyword) => match keyword {
                HtmlCssFontSizeKeyword::XxSmall => root_px * 0.6,
                HtmlCssFontSizeKeyword::XSmall => root_px * 0.75,
                HtmlCssFontSizeKeyword::Small => root_px * 0.875,
                HtmlCssFontSizeKeyword::Medium => root_px,
                HtmlCssFontSizeKeyword::Large => root_px * 1.125,
                HtmlCssFontSizeKeyword::XLarge => root_px * 1.5,
                HtmlCssFontSizeKeyword::XxLarge => root_px * 2.0,
                HtmlCssFontSizeKeyword::Smaller => parent_px * 0.833,
                HtmlCssFontSizeKeyword::Larger => parent_px * 1.2,
            },
        };

        if resolved.is_finite() {
            resolved.clamp(6.0, 96.0)
        } else {
            parent_px
        }
    }

    pub(crate) fn to_css(self) -> String {
        match self {
            Self::Px(value) => format!("{}px", css_number(value)),
            Self::Em(value) => format!("{}em", css_number(value)),
            Self::Rem(value) => format!("{}rem", css_number(value)),
            Self::Percent(value) => format!("{}%", css_number(value)),
            Self::Keyword(keyword) => match keyword {
                HtmlCssFontSizeKeyword::XxSmall => "xx-small",
                HtmlCssFontSizeKeyword::XSmall => "x-small",
                HtmlCssFontSizeKeyword::Small => "small",
                HtmlCssFontSizeKeyword::Medium => "medium",
                HtmlCssFontSizeKeyword::Large => "large",
                HtmlCssFontSizeKeyword::XLarge => "x-large",
                HtmlCssFontSizeKeyword::XxLarge => "xx-large",
                HtmlCssFontSizeKeyword::Smaller => "smaller",
                HtmlCssFontSizeKeyword::Larger => "larger",
            }
            .to_string(),
        }
    }
}

impl HtmlInlineStyle {
    pub(crate) fn is_empty(&self) -> bool {
        self.color.is_none()
            && self.background_color.is_none()
            && self.font_size.is_none()
            && self.text_align.is_none()
    }

    pub(crate) fn to_css(self) -> Option<String> {
        if self.is_empty() {
            return None;
        }

        let mut declarations = Vec::new();
        if let Some(color) = self.color {
            declarations.push(format!("color: {}", color.to_css()));
        }
        if let Some(color) = self.background_color {
            declarations.push(format!("background-color: {}", color.to_css()));
        }
        if let Some(font_size) = self.font_size {
            declarations.push(format!("font-size: {}", font_size.to_css()));
        }
        if let Some(align) = self.text_align {
            let keyword = match align {
                HtmlTextAlign::Left => "left",
                HtmlTextAlign::Center => "center",
                HtmlTextAlign::Right => "right",
            };
            declarations.push(format!("text-align: {keyword}"));
        }
        Some(format!("{};", declarations.join("; ")))
    }
}

/// Parses and classifies a raw HTML fragment. The returned document always
/// preserves `raw_source` exactly, even when semantic parsing succeeds.

pub(crate) use parse::*;

mod parse;

#[cfg(test)]
mod tests;
