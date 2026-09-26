//! HTML → Markdown conversion for clipboard pastes (roadmap B3).
//!
//! A compact tag-walking converter covering the structures browsers put on
//! the pasteboard: headings, paragraphs, emphasis, inline code, links,
//! images, lists (nested one level), blockquotes, code blocks, and simple
//! tables. Unknown markup degrades to its text content, so the paste is
//! never worse than plain text.

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
        let data: *mut objc::runtime::Object =
            msg_send![pasteboard, dataForType: html_type];
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

fn looks_like_markdown(value: &str) -> bool {
    ["**", "```", "](", "\n# ", "\n- ", "\n1. ", "\n> ", "!["]
        .iter()
        .any(|marker| value.contains(marker))
}

/// Strips Markdown presentation markers for the equality heuristic above.
fn unmarkdown(value: &str) -> String {
    value
        .replace("**", "")
        .replace('*', "")
        .replace('`', "")
        .replace('\n', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Clone, Copy, PartialEq)]
enum ListKind {
    Bullet,
    Ordered,
}

struct Converter<'a> {
    html: &'a str,
    cursor: usize,
    out: String,
    lists: Vec<ListKind>,
    ordered_counter: usize,
    blockquote_depth: usize,
    in_pre: bool,
    pending_link_href: Option<String>,
}

impl<'a> Converter<'a> {
    fn convert(mut self) -> String {
        while let Some(tag_start) = self
            .html[self.cursor..]
            .find('<')
            .map(|offset| self.cursor + offset)
        {
            self.push_text(&self.html[self.cursor..tag_start]);
            let Some(tag_end) = self.html[tag_start..].find('>').map(|offset| tag_start + offset + 1)
            else {
                break;
            };
            let raw_tag = &self.html[tag_start..tag_end];
            self.cursor = tag_end;
            let name = tag_name(raw_tag);
            let open = !raw_tag.starts_with("</");
            match (open, name.as_str()) {
                (true, "strong") | (true, "b") => self.out.push_str("**"),
                (false, "strong") | (false, "b") => self.out.push_str("**"),
                (true, "em") | (true, "i") => self.out.push('*'),
                (false, "em") | (false, "i") => self.out.push('*'),
                (true, "code") if !self.in_pre => self.out.push('`'),
                (false, "code") if !self.in_pre => self.out.push('`'),
                (true, "br") => self.out.push('\n'),
                (true, "h1" | "h2" | "h3" | "h4" | "h5" | "h6") => {
                    self.close_paragraph();
                    let level = name[1..].parse::<usize>().unwrap_or(1);
                    self.out.push_str(&"#".repeat(level));
                    self.out.push(' ');
                }
                (false, "h1" | "h2" | "h3" | "h4" | "h5" | "h6") => self.close_paragraph(),
                (true, "blockquote") => {
                    self.close_paragraph();
                    self.blockquote_depth += 1;
                }
                (false, "blockquote") => {
                    self.blockquote_depth = self.blockquote_depth.saturating_sub(1);
                    self.close_paragraph();
                }
                (true, "pre") => {
                    self.close_paragraph();
                    self.in_pre = true;
                    self.out.push_str("```\n");
                }
                (false, "pre") => {
                    self.in_pre = false;
                    if !self.out.ends_with('\n') {
                        self.out.push('\n');
                    }
                    self.out.push_str("```\n");
                }
                (true, "ul") => self.lists.push(ListKind::Bullet),
                (true, "ol") => {
                    self.lists.push(ListKind::Ordered);
                    self.ordered_counter = 1;
                }
                (false, "ul" | "ol") => {
                    self.lists.pop();
                }
                (true, "li") => {
                    self.close_paragraph();
                    let indent = "  ".repeat(self.lists.len().saturating_sub(1));
                    self.out.push_str(&indent);
                    match self.lists.last() {
                        Some(ListKind::Ordered) => {
                            self.out.push_str(&format!("{}. ", self.ordered_counter));
                            self.ordered_counter += 1;
                        }
                        _ => self.out.push_str("- "),
                    }
                }
                (false, "li") => self.out.push('\n'),
                (true, "a") => {
                    let href = attribute(raw_tag, "href").unwrap_or_default();
                    self.pending_link_href = Some(href);
                    self.out.push('[');
                }
                (false, "a") => {
                    let href = self.pending_link_href.take().unwrap_or_default();
                    self.out.push_str(&format!("]({href})"));
                }
                (true, "img") => {
                    let src = attribute(raw_tag, "src").unwrap_or_default();
                    let alt = attribute(raw_tag, "alt").unwrap_or_default();
                    if !src.is_empty() {
                        self.out.push_str(&format!("![{alt}]({src})"));
                    }
                }
                (true, "tr") => {
                    self.close_paragraph();
                    self.out.push('|');
                }
                (false, "tr") => {
                    if self.out.ends_with('|') {
                        self.out.push('\n');
                    }
                }
                (true, "td" | "th") => self.out.push(' '),
                (false, "td" | "th") => self.out.push_str(" |"),
                _ => {}
            }
        }
        self.push_text(&self.html[self.cursor..]);
        self.collapse_blank_runs()
    }

    fn collapse_blank_runs(&mut self) -> String {
        let mut collapsed = String::with_capacity(self.out.len());
        let mut blank_run = 0usize;
        for line in self.out.lines() {
            if line.trim().is_empty() {
                blank_run += 1;
                if blank_run > 1 {
                    continue;
                }
            } else {
                blank_run = 0;
            }
            collapsed.push_str(line);
            collapsed.push('\n');
        }
        collapsed.trim().to_string()
    }

    fn push_text(&mut self, text: &str) {
        let decoded = decode_entities(text);
        if self.in_pre {
            self.out.push_str(&decoded);
            return;
        }
        let condensed = decoded.split_whitespace().collect::<Vec<_>>().join(" ");
        if condensed.is_empty() {
            return;
        }
        if self.blockquote_depth > 0 {
            self.out.push_str(&"> ".repeat(self.blockquote_depth));
        }
        self.out.push_str(&condensed);
    }

    fn close_paragraph(&mut self) {
        if !self.out.ends_with("\n\n") {
            self.out.push_str("\n\n");
        }
    }
}

fn convert(html: &str) -> String {
    let converter = Converter {
        html,
        cursor: 0,
        out: String::new(),
        lists: Vec::new(),
        ordered_counter: 1,
        blockquote_depth: 0,
        in_pre: false,
        pending_link_href: None,
    };
    converter.convert()
}

fn tag_name(raw_tag: &str) -> String {
    let trimmed = raw_tag.trim_start_matches("</");
    let name: String = trimmed
        .chars()
        .skip_while(|ch| ch.is_whitespace() || *ch == '<' || *ch == '/')
        .take_while(|ch| ch.is_ascii_alphanumeric())
        .collect();
    name.to_ascii_lowercase()
}

fn attribute(raw_tag: &str, name: &str) -> Option<String> {
    let lower = raw_tag.to_ascii_lowercase();
    let needle = format!("{name}=");
    let start = lower.find(&needle)? + needle.len();
    let quote = lower.as_bytes().get(start).copied()?;
    let (value_start, value_end) = match quote {
        b'"' | b'\'' => (start + 1, lower[start + 1..].find(quote as char)? + start + 1),
        _ => {
            let end = lower[start..]
                .find(|ch: char| ch.is_whitespace() || ch == '>')
                .unwrap_or(lower.len() - start)
                + start;
            (start, end)
        }
    };
    Some(raw_tag[value_start..value_end.min(raw_tag.len())].to_string())
}

fn decode_entities(text: &str) -> String {
    text.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}

#[cfg(test)]
mod tests {
    use super::html_to_markdown_or;

    #[test]
    fn headings_paragraph_and_emphasis_convert() {
        let markdown = super::html_to_markdown_or(
            "<h2>Title</h2><p>Some <b>bold</b> and <em>italic</em> and <code>x = 1</code>.</p>",
            "plain",
        );
        assert!(markdown.starts_with("## Title"));
        assert!(markdown.contains("**bold**"));
        assert!(markdown.contains("*italic*"));
        assert!(markdown.contains("`x = 1`"));
    }

    #[test]
    fn links_and_lists_convert() {
        let markdown = super::html_to_markdown_or(
            "<p>See <a href=\"https://example.com\">the site</a>.</p><ul><li>one</li><li>two</li></ul>",
            "plain",
        );
        assert!(markdown.contains("[the site](https://example.com)"));
        assert!(markdown.contains("- one"));
        assert!(markdown.contains("- two"));
    }

    #[test]
    fn plain_div_without_structure_keeps_original_text() {
        let markdown = super::html_to_markdown_or("<div>just text</div>", "just text");
        assert_eq!(markdown, "just text");
    }

    #[test]
    fn code_blocks_survive() {
        let markdown = super::html_to_markdown_or(
            "<pre><code>let x = 1;</code></pre>",
            "fallback",
        );
        assert!(markdown.contains("```"));
        assert!(markdown.contains("let x = 1;"));
    }
}
