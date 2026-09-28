//! HTML → Markdown conversion for clipboard pastes (roadmap B3).
//!
//! Conversion runs through `htmd`, the turndown.js port that passes turndown's
//! own test suite, so pasted browser and Word markup keeps its structure
//! without a hand-written tag walker. The paste policy stays here: a fragment
//! that adds no Markdown structure keeps the plain-text flavor of the
//! pasteboard instead.

use htmd::HtmlToMarkdown;
use htmd::options::{BulletListMarker, Options};

/// Converts an HTML fragment into Markdown. `text_fallback` is returned
/// unchanged when the fragment carries no convertible structure.
pub(crate) fn html_to_markdown_or(html: &str, text_fallback: &str) -> String {
    let converted = convert(html);
    let converted = converted.trim();
    let plain = text_fallback.trim();
    if converted.is_empty() {
        return text_fallback.to_string();
    }
    // Same visible text and no markdown structure means the HTML was plain
    // presentation (a <div>/<p> wrapper); prefer the original paste.
    if unmarkdown(converted) == unmarkdown(plain) && !looks_like_markdown(converted) {
        return text_fallback.to_string();
    }
    converted.to_string()
}

/// Reads `public.html` from the macOS pasteboard, if present.
#[cfg(target_os = "macos")]
#[allow(unexpected_cfgs)]
pub(crate) fn clipboard_html() -> Option<String> {
    use objc::{class, msg_send, sel, sel_impl};

    unsafe {
        let pasteboard: *mut objc::runtime::Object =
            msg_send![class!(NSPasteboard), generalPasteboard];
        if pasteboard.is_null() {
            return None;
        }
        // NSPasteboardTypeHTML is the UTI string "public.html".
        let html_type: *mut objc::runtime::Object =
            msg_send![class!(NSString), stringWithUTF8String: "public.html"];
        let data: *mut objc::runtime::Object = msg_send![pasteboard, dataForType: html_type];
        if data.is_null() {
            return None;
        }
        let length: usize = msg_send![data, length];
        if length == 0 {
            return None;
        }
        let bytes: *const u8 = msg_send![data, bytes];
        let bytes = std::slice::from_raw_parts(bytes, length);
        Some(String::from_utf8_lossy(bytes).into_owned())
    }
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn clipboard_html() -> Option<String> {
    None
}

/// Paste decision: convert the pasteboard HTML when it carries real Markdown
/// structure, otherwise keep the plain-text flavor untouched.
pub(crate) fn maybe_markdown_from_clipboard(plain: &str) -> String {
    let Some(html) = clipboard_html() else {
        return plain.to_string();
    };
    html_to_markdown_or(&html, plain)
}

fn convert(html: &str) -> String {
    converter().convert(html).unwrap_or_default()
}

fn converter() -> HtmlToMarkdown {
    HtmlToMarkdown::builder()
        // Velora serializes list items as `- ` and `1. `, so pasted lists match.
        .options(Options {
            bullet_list_marker: BulletListMarker::Dash,
            ul_bullet_spacing: 1,
            ol_number_spacing: 1,
            ..Options::default()
        })
        // A browser pasteboard carries page CSS and scripts; neither belongs in
        // the document text.
        .skip_tags(vec!["script", "style", "head", "title", "meta", "link"])
        .build()
}

fn looks_like_markdown(value: &str) -> bool {
    ["**", "```", "](", "\n# ", "\n- ", "\n1. ", "\n> ", "!["]
        .iter()
        .any(|marker| value.contains(marker))
}

/// Strips Markdown presentation markers for the equality heuristic above.
fn unmarkdown(value: &str) -> String {
    value
        .replace(['*', '`'], "")
        .replace('\n', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    #[test]
    fn headings_paragraph_and_emphasis_convert() {
        let markdown = super::html_to_markdown_or(
            "<h2>Title</h2><p>Some <b>bold</b> and <em>italic</em> and <code>x = 1</code>.</p>",
            "plain",
        );
        assert!(markdown.starts_with("## Title"), "actual: {markdown}");
        assert!(markdown.contains("**bold**"), "actual: {markdown}");
        assert!(markdown.contains("italic"), "actual: {markdown}");
        assert!(markdown.contains("`x = 1`"), "actual: {markdown}");
    }

    #[test]
    fn links_and_lists_convert() {
        let markdown = super::html_to_markdown_or(
            "<p>See <a href=\"https://example.com\">the site</a>.</p><ul><li>one</li><li>two</li></ul>",
            "plain",
        );
        assert!(
            markdown.contains("[the site](https://example.com)"),
            "actual: {markdown}"
        );
        assert!(markdown.contains("- one"), "actual: {markdown}");
        assert!(markdown.contains("- two"), "actual: {markdown}");
    }

    #[test]
    fn plain_div_without_structure_keeps_original_text() {
        let markdown = super::html_to_markdown_or("<div>just text</div>", "just text");
        assert_eq!(markdown, "just text");
    }

    #[test]
    fn code_blocks_survive() {
        let markdown = super::html_to_markdown_or("<pre><code>let x = 1;</code></pre>", "fallback");
        assert!(markdown.contains("```"), "actual: {markdown}");
        assert!(markdown.contains("let x = 1;"), "actual: {markdown}");
    }

    #[test]
    fn tables_convert_to_pipe_tables() {
        let markdown = super::html_to_markdown_or(
            "<table><tr><th>日期</th><th>版本</th></tr><tr><td>2026-08-04</td><td>1.0</td></tr></table>",
            "plain",
        );
        assert!(markdown.contains("| 日期"), "actual: {markdown}");
        assert!(markdown.contains("| 2026-08-04"), "actual: {markdown}");
    }

    #[test]
    fn nested_lists_keep_their_structure() {
        let markdown =
            super::html_to_markdown_or("<ul><li>one<ul><li>nested</li></ul></li></ul>", "plain");
        assert!(markdown.contains("- one"), "actual: {markdown}");
        assert!(markdown.contains("- nested"), "actual: {markdown}");
    }

    #[test]
    fn scripts_and_styles_are_dropped() {
        let markdown = super::html_to_markdown_or(
            "<p>keep</p><script>alert(1)</script><style>p{color:red}</style>",
            "plain",
        );
        assert!(markdown.contains("keep"), "actual: {markdown}");
        assert!(!markdown.contains("alert"), "actual: {markdown}");
        assert!(!markdown.contains("color:red"), "actual: {markdown}");
    }

    #[test]
    fn images_and_quotes_convert() {
        let markdown = super::html_to_markdown_or(
            "<blockquote><p>quoted</p></blockquote><p><img src=\"a.png\" alt=\"diagram\"></p>",
            "plain",
        );
        assert!(markdown.contains("> quoted"), "actual: {markdown}");
        assert!(markdown.contains("![diagram](a.png)"), "actual: {markdown}");
    }
}
