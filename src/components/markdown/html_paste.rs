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
use html5ever::tendril::Tendril;
use html5ever::tendril::fmt::UTF8;

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

/// 读取与纯文本同一次复制产生的 HTML。
#[cfg(target_os = "macos")]
pub(crate) fn clipboard_html(expected_change: u64) -> Option<String> {
    use cocoa::{appkit::NSPasteboard, base::nil};
    let pasteboard = unsafe { NSPasteboard::generalPasteboard(nil) };
    read_pasteboard_html(pasteboard, expected_change)
}

#[cfg(target_os = "macos")]
#[allow(unexpected_cfgs)]
fn read_pasteboard_html(pasteboard: cocoa::base::id, expected_change: u64) -> Option<String> {
    use cocoa::appkit::NSPasteboardTypeHTML;
    use objc::{msg_send, sel, sel_impl};

    unsafe {
        if pasteboard.is_null() {
            return None;
        }
        let change: isize = msg_send![pasteboard, changeCount];
        if change as u64 != expected_change {
            return None;
        }
        let data: *mut objc::runtime::Object =
            msg_send![pasteboard, dataForType: NSPasteboardTypeHTML];
        if data.is_null() {
            return None;
        }
        let length: usize = msg_send![data, length];
        if length == 0 {
            return None;
        }
        let bytes: *const u8 = msg_send![data, bytes];
        let bytes = std::slice::from_raw_parts(bytes, length);
        let html = String::from_utf8_lossy(bytes).into_owned();
        let change: isize = msg_send![pasteboard, changeCount];
        (change as u64 == expected_change).then_some(html)
    }
}

#[cfg(target_os = "windows")]
pub(crate) fn clipboard_html(expected_change: u64) -> Option<String> {
    use windows::{
        Win32::{
            Foundation::{GetLastError, HGLOBAL},
            System::{
                DataExchange::{
                    CloseClipboard, GetClipboardData, GetClipboardSequenceNumber,
                    IsClipboardFormatAvailable, OpenClipboard, RegisterClipboardFormatW,
                },
                Memory::{GlobalLock, GlobalSize, GlobalUnlock},
            },
        },
        core::w,
    };
    unsafe {
        let format = RegisterClipboardFormatW(w!("HTML Format"));
        if format == 0 || IsClipboardFormatAvailable(format).is_err() {
            return None;
        }
        if let Err(error) = OpenClipboard(None) {
            eprintln!("无法读取 HTML 剪贴板，使用纯文本：{error}");
            return None;
        }
        let html = (|| {
            if u64::from(GetClipboardSequenceNumber()) != expected_change {
                return None;
            }
            let handle = HGLOBAL(GetClipboardData(format).ok()?.0);
            let length = GlobalSize(handle);
            if length == 0 || length > 2 * 1024 * 1024 {
                return None;
            }
            let bytes = GlobalLock(handle);
            if bytes.is_null() {
                return None;
            }
            let html = cf_html_fragment(std::slice::from_raw_parts(bytes.cast::<u8>(), length));
            // 最后一次解锁返回零并不表示失败，必须同时检查系统错误码。
            if GlobalUnlock(handle).is_err() && GetLastError().0 != 0 {
                eprintln!("无法解锁 HTML 剪贴板内存");
            }
            html
        })();
        if let Err(error) = CloseClipboard() {
            eprintln!("无法关闭 HTML 剪贴板读取：{error}");
        }
        html
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub(crate) fn clipboard_html(_expected_change: u64) -> Option<String> {
    None
}

pub(crate) fn clipboard_change_count() -> u64 {
    #[cfg(target_os = "macos")]
    {
        use cocoa::{appkit::NSPasteboard, base::nil};
        unsafe { NSPasteboard::generalPasteboard(nil).changeCount() as u64 }
    }
    #[cfg(target_os = "windows")]
    unsafe {
        u64::from(windows::Win32::System::DataExchange::GetClipboardSequenceNumber())
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        0
    }
}

/// HTML 有可转换结构时优先使用，否则保留纯文本。系统读取由调用方处理。
pub(crate) fn maybe_markdown_from_clipboard(item_html: Option<&str>, plain: &str) -> String {
    let Some(html) = item_html else {
        return plain.to_string();
    };
    html_to_markdown_or(html, plain)
}

/// 私用区字符：正常粘贴内容里不会出现，htmd 的转义扫描也不认它，正好拿来当占位符。
const UNDERSCORE_PLACEHOLDER: char = '\u{e000}';

fn convert(html: &str) -> String {
    let html = with_table_headers(html);
    let neutralized = neutralize_intraword_underscores(&html);
    let source = neutralized.as_deref().unwrap_or(html.as_str());
    let markdown = converter().convert(source).unwrap_or_default();
    match neutralized {
        // 反斜杠只由 htmd 决定写不写，还原这里只管把字面字符放回去。
        Some(_) => markdown.replace(UNDERSCORE_PLACEHOLDER, "_"),
        None => markdown,
    }
}

/// htmd 在文本节点里把 `_` 无条件转义：`flash_attention_score` 粘进来就成了
/// `flash\_attention\_score`，缓冲区、存盘文件与「复制为纯文本」于是全带反斜杠。
/// 词内下划线的反斜杠是纯污染：两侧都是字词字符时它既不左倾也不右倾（CommonMark
/// 的 flanking 规则），既开不了也闭不了强调，多出来的只有字节。
///
/// 判据落在 HTML 文本节点上而不是转换后的 Markdown 上：文本节点正是 htmd 决定
/// 转义的地方，换成占位符它就看不见那颗下划线，转换完再还原成字面字符。`pre` /
/// `code` / `script` / `style` 的内文不碰——htmd 本来也不转义那里，动了只会把
/// 作者自己的 `\_`（LaTeX、正则）洗掉。返回 `None` 表示没有要改的节点（HTML
/// 原文一个字节都不动），或原文里已有占位符、还原会张冠李戴。
fn neutralize_intraword_underscores(html: &str) -> Option<String> {
    // htmd 原样搬运内文的元素。
    const RAW_TEXT_ELEMENTS: [&str; 5] = ["pre", "code", "script", "style", "textarea"];

    if html.contains(UNDERSCORE_PLACEHOLDER) {
        return None;
    }
    let dom = parse_html_fragment(html);
    if !replace_intraword_underscores(&dom.document, &RAW_TEXT_ELEMENTS) {
        return None;
    }
    Some(serialize_document_children(&dom.document))
}

/// 把 `node` 子树里文本节点的词内下划线换成占位符；有没有换过。
fn replace_intraword_underscores(node: &Handle, raw_text_elements: &[&str]) -> bool {
    let mut replaced = false;
    for child in node.children.borrow().iter() {
        replaced |= match &child.data {
            NodeData::Text { contents } => replace_underscores_in_text(contents),
            NodeData::Element { name, .. } => {
                !raw_text_elements.contains(&name.local.as_ref())
                    && replace_intraword_underscores(child, raw_text_elements)
            }
            _ => false,
        };
    }
    replaced
}

/// 单个文本节点的替换。没有词内下划线时不动这个节点。
fn replace_underscores_in_text(contents: &RefCell<Tendril<UTF8>>) -> bool {
    let replaced = {
        let text = contents.borrow();
        let mut replaced = false;
        let mut output = String::with_capacity(text.len());
        let mut previous: Option<char> = None;
        let mut iterator = text.chars().peekable();
        while let Some(ch) = iterator.next() {
            let intraword =
                ch == '_' && is_word_char(previous) && is_word_char(iterator.peek().copied());
            if intraword {
                output.push(UNDERSCORE_PLACEHOLDER);
                replaced = true;
            } else {
                output.push(ch);
            }
            previous = Some(ch);
        }
        if !replaced {
            return false;
        }
        output
    };

    *contents.borrow_mut() = Tendril::from(replaced);
    true
}

/// 下划线两侧算不算「字词字符」：字母、数字或下划线本身。
fn is_word_char(ch: Option<char>) -> bool {
    ch.is_some_and(|ch| ch.is_alphanumeric() || ch == '_')
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
    serialize_document_children(&dom.document)
}

/// rcdom refuses to serialize a document node, so its children go one by one;
/// that is the same text without the wrapping element.
fn serialize_document_children(document: &Handle) -> String {
    document
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

    /// 失败现场：从网页/文档里复制 `flash_attention_score` 粘进 Velora，缓冲区、
    /// 存盘文件与再复制到 txt 的全是 `flash\_attention\_score`。词内下划线按
    /// CommonMark 不是强调定界符，反斜杠是 htmd 文本节点转义的副作用。
    #[test]
    fn intraword_underscores_paste_without_backslashes() {
        assert_eq!(
            super::html_to_markdown_or(
                "<p>变量 flash_attention_score 很重要</p>",
                "变量 flash_attention_score 很重要",
            ),
            "变量 flash_attention_score 很重要"
        );
        assert_eq!(
            super::html_to_markdown_or(
                "<p><b>topic_embedding_attention</b> 与 <code>x_y_z</code></p>",
                "",
            ),
            "**topic_embedding_attention** 与 `x_y_z`"
        );
        assert_eq!(
            super::html_to_markdown_or("<ul><li>flash_attention_score</li></ul>", ""),
            "- flash_attention_score"
        );
    }

    /// 代码里的 `\_` 是作者自己写的（LaTeX、正则），转换不许把它洗成 `_`。
    #[test]
    fn code_content_keeps_its_own_backslash_escapes() {
        let markdown = super::html_to_markdown_or("<pre><code>a\\_b</code></pre>", "fallback");
        assert_eq!(markdown, "```\na\\_b\n```");
    }

    /// 两侧不是字词字符的下划线仍要转义：`_foo_`、`foo_b_` 去掉反斜杠就成强调。
    #[test]
    fn standalone_underscores_stay_escaped() {
        let markdown = super::html_to_markdown_or("<p>_foo_ 与 bar_</p>", "");
        assert_eq!(markdown, "\\_foo\\_ 与 bar\\_");
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
        // 粘贴策略再把那段文字换回剪贴板的纯文本版本，表格结构就此丢失。
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
        // 报修的粘法：剪贴板里既有 HTML 版本（网站的表格），又有纯文本版本
        // （制表符分隔）。HTML 版本优先，粘出来才是表格。
        let markdown = super::maybe_markdown_from_clipboard(
            Some(
                "<table><tbody><tr><td>日期</td><td>版本</td></tr><tr><td>2026-08-04</td><td>1.0</td></tr></tbody></table>",
            ),
            "日期\t版本\n2026-08-04\t1.0",
        );
        let lines = markdown.lines().map(str::to_string).collect::<Vec<_>>();
        let table = super::super::table::parse_root_table_region(&lines)
            .expect("剪贴板 HTML 版本里的表格必须粘成原生表格");
        assert_eq!(table.header[0].serialize_markdown(), "日期");
        assert_eq!(table.rows.len(), 1);
    }

    // CF_HTML 的字节偏移只在应用这一处解析，读剪贴板和转换测试共用。
    #[test]
    fn windows_clipboard_payload_yields_the_markup() {
        // 一段 Chrome 形状的 `CF_HTML`：头部偏移是字节偏移。
        let payload = "Version:0.9\nStartHTML:0000000131\nEndHTML:0000000267\nStartFragment:0000000164\nEndFragment:0000000234\nSourceURL:https://example.com/\n<html><body>\n<!--StartFragment--><table border=\"1\"><tbody><tr><td>A</td><td>B</td></tr></tbody></table><!--EndFragment-->\n</body></html>";
        let fragment = super::cf_html_fragment(payload.as_bytes()).expect("必须取出选中的那段标记");
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
            super::cf_html_fragment(payload.as_bytes()).expect("没有片段偏移时取整段标记");
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
                super::cf_html_fragment(payload.as_bytes()).is_none(),
                "actual payload: {payload}"
            );
        }
    }

    /// 载荷不是合法 UTF-8（中文版 Word/写字板按自己声明的编码写多字节）时，这一版本算
    /// **没有**：让粘贴回退到同一次复制的纯文本版本，而不是把乱码写进缓冲区、还可能被
    /// 保存回文件。这与不变量 5 是同一条规则——读不懂的字节不认，不换成乱码留下。
    #[test]
    fn windows_clipboard_payload_with_invalid_utf8_yields_nothing() {
        let payload = b"Version:0.9\n<p>\xc3\x28\xe6\x97</p>";
        assert!(
            super::cf_html_fragment(payload).is_none(),
            "actual: {:?}",
            super::cf_html_fragment(payload)
        );
    }
}

#[cfg(any(test, target_os = "windows"))]
/// Extracts the HTML from a `CF_HTML` clipboard payload. Returns `None` when
/// the payload holds no markup.
pub(crate) fn cf_html_fragment(payload: &[u8]) -> Option<String> {
    // The header is ASCII key/value lines; the markup starts at the first `<`.
    let header_end = payload
        .iter()
        .position(|byte| *byte == b'<')
        .unwrap_or(payload.len());

    let start = offset(payload, header_end, "StartFragment")
        .or_else(|| offset(payload, header_end, "StartHTML"))
        .unwrap_or(header_end);
    let end = offset(payload, header_end, "EndFragment")
        .or_else(|| offset(payload, header_end, "EndHTML"))
        .unwrap_or(payload.len());
    let (start, end) = (start.min(payload.len()), end.min(payload.len()));
    if start >= end {
        return None;
    }

    // A payload that is not valid UTF-8 (a Chinese-locale Word or Write clipboard
    // writes its declared code page) is not a flavor we can show: decoding it
    // lossily would put U+FFFD runs into the document and let them be saved back.
    // Reporting no flavor lets the paste fall back to the plain-text one.
    let fragment = String::from_utf8(payload[start..end].to_vec()).ok()?;
    let fragment = fragment.trim().to_string();
    if fragment.is_empty() {
        None
    } else {
        Some(fragment)
    }
}

/// Reads one header value. `header_end` bounds the search to the header, so a
/// `StartHTML` mentioned inside the markup itself cannot be picked up.
#[cfg(any(test, target_os = "windows"))]
fn offset(payload: &[u8], header_end: usize, key: &str) -> Option<usize> {
    let header = std::str::from_utf8(&payload[..header_end]).ok()?;
    header.split(['\r', '\n']).find_map(|line| {
        let value = line.strip_prefix(key)?.strip_prefix(':')?;
        value.trim().parse().ok()
    })
}
