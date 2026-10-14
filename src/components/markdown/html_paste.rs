//! HTML → Markdown conversion for clipboard pastes (roadmap B3).
//!
//! Conversion runs through `htmd`, the turndown.js port that passes turndown's
//! own test suite, so pasted browser and Word markup keeps its structure
//! without a hand-written tag walker. The paste policy stays here: a fragment
//! that adds no Markdown structure keeps the plain-text flavor of the
//! pasteboard instead.

use std::cell::RefCell;
use std::rc::Rc;

use htmd::HtmlToMarkdown;
use htmd::options::{BulletListMarker, Options};

use super::html::{Handle, Node, NodeData, parse_html_fragment, serialize_handle};

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

/// Reads `public.html` from the macOS pasteboard, if present. GPUI does not fill
/// the HTML flavor of the clipboard item on macOS, so the pasteboard is read
/// directly.
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
/// structure, otherwise keep the plain-text flavor untouched. `item_html` is
/// the HTML flavor of the clipboard item, which GPUI fills on Windows.
pub(crate) fn maybe_markdown_from_clipboard(item_html: Option<&str>, plain: &str) -> String {
    let html = item_html.map(str::to_string).or_else(clipboard_html);
    let Some(html) = html else {
        return plain.to_string();
    };
    html_to_markdown_or(&html, plain)
}

fn convert(html: &str) -> String {
    converter()
        .convert(&with_table_headers(html))
        .unwrap_or_default()
}

/// Returns `html` rewritten so every table starts with a header row.
///
/// `htmd` converts a table only when the markup carries an explicit `<th>` or
/// `<thead>`; a table whose cells are all `<td>`, which is what most sites
/// emit, silently degrades to paragraphs. Promoting each table's first row to
/// `<th>` cells gives `htmd` the same header it takes for a header-less table
/// in its faithful mode. Fragments without such a table keep their exact
/// source text, so nothing but those tables passes through the DOM round trip.
fn with_table_headers(html: &str) -> String {
    let dom = parse_html_fragment(html);
    if promote_first_rows(&dom.document) == 0 {
        return html.to_string();
    }
    // rcdom refuses to serialize a document node, so its children go one by
    // one; that is the same text without the wrapping element.
    dom.document
        .children
        .borrow()
        .iter()
        .map(serialize_handle)
        .collect()
}

/// Promotes the first row of every table in the subtree. Returns how many rows
/// were promoted.
fn promote_first_rows(node: &Handle) -> usize {
    let mut promoted = 0;
    for child in node.children.borrow().iter() {
        if let NodeData::Element { name, .. } = &child.data
            && name.local.as_ref() == "table"
            && let Some(row) = first_row(child)
        {
            promoted += promote_row_to_header(&row);
        }
        promoted += promote_first_rows(child);
    }
    promoted
}

/// First `<tr>` of one table. `html5ever` moves loose rows into an implicit
/// `<tbody>`, so the search walks the table's section elements.
fn first_row(table: &Handle) -> Option<Handle> {
    table.children.borrow().iter().find_map(|child| {
        let NodeData::Element { name, .. } = &child.data else {
            return None;
        };
        match name.local.as_ref() {
            "tr" => Some(child.clone()),
            "tbody" | "thead" | "tfoot" => first_row(child),
            _ => None,
        }
    })
}

/// Renames the `<td>` cells of `row` to `<th>` cells. Attributes and children
/// stay, so cell formatting and cell contents survive the rename.
fn promote_row_to_header(row: &Handle) -> usize {
    let mut promoted = 0;
    for cell in row.children.borrow_mut().iter_mut() {
        let NodeData::Element { name, attrs, .. } = &cell.data else {
            continue;
        };
        if name.local.as_ref() != "td" {
            continue;
        }

        let mut header_name = name.clone();
        header_name.local = "th".into();
        let header = Node::new(NodeData::Element {
            name: header_name,
            attrs: RefCell::new(attrs.borrow().clone()),
            template_contents: RefCell::new(None),
            mathml_annotation_xml_integration_point: false,
        });
        // rcdom nodes hold their parent as a weak handle. The moved children
        // must point at the new cell, or ancestry checks inside it stop at a
        // dead node and treat nested tables as top-level ones.
        let children = std::mem::take(&mut *cell.children.borrow_mut());
        for child in children.iter() {
            child.parent.set(Some(Rc::downgrade(&header)));
        }
        *header.children.borrow_mut() = children;

        *cell = header;
        promoted += 1;
    }
    promoted
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

    #[test]
    fn converted_html_tables_parse_as_native_tables() {
        // 回归：`htmd` 转换 HTML 表格时写出的分隔行是 `| ---- | --- | -- |`，
        // 而原生表格解析器当时要求每个分隔单元格至少 3 个短横。结果是粘贴进来的
        // HTML 表格不被识别，只能当纯文本显示。
        let markdown = super::html_to_markdown_or(
            "<table><thead><tr><th>源文件</th><th>行数</th><th>动作</th></tr></thead><tbody><tr><td>a.md</td><td>160</td><td>增强</td></tr><tr><td>b.md</td><td>92</td><td>剔除</td></tr></tbody></table>",
            "plain",
        );
        let lines = markdown.lines().map(str::to_string).collect::<Vec<_>>();
        let table = super::super::table::parse_root_table_region(&lines)
            .expect("转换出的 HTML 表格必须能被原生表格解析器识别");
        assert_eq!(table.header.len(), 3);
        assert_eq!(table.rows.len(), 2);
        assert_eq!(table.header[0].serialize_markdown(), "源文件");
        assert_eq!(table.rows[1][2].serialize_markdown(), "剔除");
    }

    #[test]
    fn header_less_table_pastes_as_a_native_table() {
        // 报修：网站上复制的表格在 Word/Typora 里是表格，在 Velora 里成了纯文本。
        // 根因：`htmd` 的 Pure 模式拒收没有显式表头的表格，整表退化成段落文字，
        // 粘贴策略再把那段文字换回剪贴板的纯文本味道，表格结构就此丢失。
        let markdown = super::html_to_markdown_or(
            "<meta charset='utf-8'><table border=\"1\" cellpadding=\"1\" style=\"border-collapse: collapse\"><tbody><tr><td>日期</td><td>版本</td></tr><tr><td>2026-08-04</td><td>1.0</td></tr></tbody></table>",
            "日期\t版本\n2026-08-04\t1.0",
        );
        let lines = markdown.lines().map(str::to_string).collect::<Vec<_>>();
        let table = super::super::table::parse_root_table_region(&lines)
            .expect("无表头的 HTML 表格必须粘成原生表格");
        assert_eq!(table.header[0].serialize_markdown(), "日期");
        assert_eq!(table.header[1].serialize_markdown(), "版本");
        assert_eq!(table.rows.len(), 1);
        assert_eq!(table.rows[0][0].serialize_markdown(), "2026-08-04");
        assert_eq!(table.rows[0][1].serialize_markdown(), "1.0");
    }

    #[test]
    fn header_less_table_keeps_surrounding_text_and_cell_markup() {
        let markdown = super::html_to_markdown_or(
            "<p>版本记录</p><table><tbody><tr><td><b>源文件</b></td><td>动作</td></tr><tr><td><a href=\"https://example.com/a.md\">a.md</a></td><td>增强</td></tr></tbody></table>",
            "版本记录\n源文件\t动作\na.md\t增强",
        );
        assert!(markdown.contains("版本记录"), "actual: {markdown}");
        assert!(
            markdown.contains("[a.md](https://example.com/a.md)"),
            "actual: {markdown}"
        );
        let lines = markdown
            .lines()
            .skip_while(|line| !line.starts_with('|'))
            .map(str::to_string)
            .collect::<Vec<_>>();
        let table = super::super::table::parse_root_table_region(&lines)
            .expect("无表头的 HTML 表格必须粘成原生表格");
        assert_eq!(table.header[0].serialize_markdown(), "**源文件**");
        assert_eq!(table.rows.len(), 1);
    }

    #[test]
    fn clipboard_html_flavor_pastes_as_a_native_table() {
        // 报修的粘法：剪贴板里既有 HTML 味道（网站的表格），又有纯文本味道
        // （制表符分隔）。HTML 味道优先，粘出来才是表格。
        let markdown = super::maybe_markdown_from_clipboard(
            Some(
                "<table><tbody><tr><td>日期</td><td>版本</td></tr><tr><td>2026-08-04</td><td>1.0</td></tr></tbody></table>",
            ),
            "日期\t版本\n2026-08-04\t1.0",
        );
        let lines = markdown.lines().map(str::to_string).collect::<Vec<_>>();
        let table = super::super::table::parse_root_table_region(&lines)
            .expect("剪贴板 HTML 味道里的表格必须粘成原生表格");
        assert_eq!(table.header[0].serialize_markdown(), "日期");
        assert_eq!(table.rows.len(), 1);
    }

    // `gpui` 的 `cf_html_fragment` 只有 Velora 这一个调用方，且 gpui 包在仓库外
    // （`[patch.crates-io]` 指过来的 vendor 目录）不能单独跑测试，所以它的检查
    // 落在用得着它的地方。
    #[test]
    fn windows_clipboard_payload_yields_the_markup() {
        // 一段 Chrome 形状的 `CF_HTML`：头部偏移是字节偏移。
        let payload = "Version:0.9\nStartHTML:0000000131\nEndHTML:0000000267\nStartFragment:0000000164\nEndFragment:0000000234\nSourceURL:https://example.com/\n<html><body>\n<!--StartFragment--><table border=\"1\"><tbody><tr><td>A</td><td>B</td></tr></tbody></table><!--EndFragment-->\n</body></html>";
        let fragment = gpui::cf_html_fragment(payload.as_bytes()).expect("必须取出选中的那段标记");
        assert_eq!(
            fragment,
            "<table border=\"1\"><tbody><tr><td>A</td><td>B</td></tr></tbody></table>"
        );
        let markdown = super::html_to_markdown_or(&fragment, "A\tB");
        let lines = markdown.lines().map(str::to_string).collect::<Vec<_>>();
        assert!(
            super::super::table::parse_root_table_region(&lines).is_some(),
            "actual: {markdown}"
        );
    }

    #[test]
    fn windows_clipboard_payload_without_fragment_offsets_yields_the_markup() {
        let payload = "Version:0.9\nStartHTML:0000000052\nEndHTML:0000000089\n<html><body><p>text</p></body></html>";
        let fragment =
            gpui::cf_html_fragment(payload.as_bytes()).expect("没有片段偏移时取整段标记");
        assert_eq!(fragment, "<html><body><p>text</p></body></html>");
    }

    #[test]
    fn windows_clipboard_payload_without_markup_yields_nothing() {
        for payload in [
            "",
            "Version:0.9\nStartHTML:0000000105\nEndFragment:0000000999\n",
            "Version:0.9\nStartFragment:0000000164\nEndFragment:0000000164\n<html></html>",
        ] {
            assert!(
                gpui::cf_html_fragment(payload.as_bytes()).is_none(),
                "actual payload: {payload}"
            );
        }
    }

    /// 载荷不是合法 UTF-8（中文版 Word/写字板按自己声明的编码写多字节）时，这一味道算
    /// **没有**：让粘贴回退到同一次复制的纯文本味道，而不是把乱码写进缓冲区、还可能被
    /// 保存回文件。口径与不变量 5 一致——判不出来的字节拒绝，不以乱码呈现。
    #[test]
    fn windows_clipboard_payload_with_invalid_utf8_yields_nothing() {
        let payload = b"Version:0.9\n<p>\xc3\x28\xe6\x97</p>";
        assert!(
            gpui::cf_html_fragment(payload).is_none(),
            "actual: {:?}",
            gpui::cf_html_fragment(payload)
        );
    }
}
