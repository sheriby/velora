//! Block semantic state and block-level Markdown parsing helpers.
//!
//! This module defines the persistent block record that is serialized to and
//! from Markdown. Block-level parsing stays intentionally narrow: only syntax
//! that the runtime tree can reconstruct is parsed into structured blocks.

use std::ops::Range;
use std::path::PathBuf;

use gpui::{Image, Pixels, Point, SharedString};
use uuid::Uuid;

use crate::components::markdown::html::{HtmlDocument, parse_html_document};
use crate::components::markdown::image::parse_standalone_image;
use crate::components::markdown::inline::InlineTextTree;
use crate::components::{TableAxisKind, TableData};

/// Supported callout variants parsed from `[!TYPE]` quote headers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalloutVariant {
    /// Informational note callout.
    Note,
    /// Helpful tip callout.
    Tip,
    /// High-emphasis important callout.
    Important,
    /// Warning callout for risky or surprising content.
    Warning,
    /// Caution callout for potentially harmful actions.
    Caution,
}

impl CalloutVariant {
    pub fn marker(self) -> &'static str {
        match self {
            Self::Note => "NOTE",
            Self::Tip => "TIP",
            Self::Important => "IMPORTANT",
            Self::Warning => "WARNING",
            Self::Caution => "CAUTION",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Note => "Note",
            Self::Tip => "Tip",
            Self::Important => "Important",
            Self::Warning => "Warning",
            Self::Caution => "Caution",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Note => "i",
            Self::Tip => "+",
            Self::Important => "*",
            Self::Warning => "!",
            Self::Caution => "x",
        }
    }

    pub fn parse_header_line(line: &str) -> Option<(Self, String)> {
        let trimmed = line.trim_start();
        let rest = trimmed.strip_prefix("[!")?;
        let marker_end = rest.find(']')?;
        let marker = &rest[..marker_end];
        let variant = match marker.to_ascii_uppercase().as_str() {
            "NOTE" => Self::Note,
            "TIP" => Self::Tip,
            "IMPORTANT" => Self::Important,
            "WARNING" => Self::Warning,
            "CAUTION" => Self::Caution,
            _ => return None,
        };
        let title = rest[marker_end + 1..].trim_start().to_string();
        Some((variant, title))
    }

    pub fn header_markdown(self, title_markdown: &str) -> String {
        if title_markdown.trim().is_empty() {
            format!("[!{}]", self.marker())
        } else {
            format!("[!{}] {}", self.marker(), title_markdown)
        }
    }

    pub fn escape_plain_quote_header(title_markdown: &str) -> String {
        let mut lines = title_markdown.splitn(2, '\n');
        let first = lines.next().unwrap_or_default();
        let rest = lines.next();
        let escaped_first = if Self::parse_header_line(first).is_some() {
            format!("\\{first}")
        } else {
            first.to_string()
        };
        match rest {
            Some(rest) => format!("{escaped_first}\n{rest}"),
            None => escaped_first,
        }
    }
}

/// The semantic type of a block, determining both its Markdown syntax and
/// visual rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockKind {
    /// Plain paragraph with inline formatting.
    Paragraph,
    /// Horizontal rule.
    Separator,
    /// ATX or Setext heading with a CommonMark heading level.
    Heading { level: u8 },
    /// Unordered list item.
    BulletedListItem,
    /// Task-list item with checked state.
    TaskListItem { checked: bool },
    /// Ordered list item; serialization uses canonical dot markers.
    NumberedListItem,
    /// Blockquote container.
    Quote,
    /// GitHub-style alert/callout container.
    Callout(CalloutVariant),
    /// Footnote definition container.
    FootnoteDefinition,
    /// Native pipe-table block.
    Table,
    /// Fenced code block with optional language info string.
    CodeBlock { language: Option<SharedString> },
    /// Visible HTML comment block preserved as raw comment text.
    Comment,
    /// Safe raw HTML rendered through native GPUI semantic elements.
    HtmlBlock,
    /// Display math block rendered with the LaTeX pipeline.
    MathBlock,
    /// Mermaid fenced block rendered as SVG.
    MermaidBlock,
    /// Raw Markdown fallback for syntax outside the native runtime subset.
    RawMarkdown,
    /// YAML front matter at the very start of the document (a `---` fence
    /// pair). Preserved byte-exact like raw markdown, rendered as a YAML
    /// source widget.
    FrontMatter,
}

/// Opening fence parsed from a fenced code block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeFenceOpening {
    /// Fence character, either backtick or tilde.
    pub ch: char,
    /// Length of the opening fence run.
    pub len: usize,
    /// Optional language/info string after the opening fence.
    pub language: Option<SharedString>,
}

/// 列表项在原文里写下的记号。
///
/// 解析时记下来，序列化与绘制都照它写。没有这两位，列表项就只是「有序/无序」，
/// 用户写的 `1)` 会在显示与重新落笔时变成 `1.`，`+ 项目` 变成 `- 项目`。
/// `None` 表示按规范写（无序 `-`、有序 `.`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ListMarkerStyle {
    /// 无序项（含任务项）的子弹字符：`-`、`*` 或 `+`。
    pub bullet: Option<char>,
    /// 有序项序号后面的分隔符：`.` 或 `)`。
    pub delimiter: Option<char>,
}

impl ListMarkerStyle {
    /// 无序项实际使用的子弹字符（没记过就是规范的 `-`）。
    pub fn bullet_or_default(&self) -> char {
        self.bullet.unwrap_or('-')
    }

    /// 有序项实际使用的分隔符（没记过就是规范的 `.`）。
    pub fn delimiter_or_default(&self) -> char {
        self.delimiter.unwrap_or('.')
    }
}

impl BlockKind {
    /// Returns true when blocks of this kind may own child blocks in the
    /// current runtime tree.
    pub fn supports_children(&self) -> bool {
        self.is_list_item() || self.is_quote_container() || self.is_footnote_definition()
    }

    pub fn is_list_item(&self) -> bool {
        matches!(
            self,
            Self::BulletedListItem | Self::TaskListItem { .. } | Self::NumberedListItem
        )
    }

    pub fn is_numbered_list_item(&self) -> bool {
        matches!(self, Self::NumberedListItem)
    }

    pub fn is_task_list_item(&self) -> bool {
        matches!(self, Self::TaskListItem { .. })
    }

    pub fn is_code_block(&self) -> bool {
        matches!(self, Self::CodeBlock { .. })
    }

    /// Whether the right-click "Insert Table" affordance makes sense when a
    /// block of this kind is the target. Atomic/structural blocks (tables,
    /// code, math, etc.) render as self-contained widgets where inserting a
    /// table from within them is nonsensical, so they are excluded.
    pub fn allows_context_table_insert(&self) -> bool {
        !matches!(
            self,
            Self::Table
                | Self::CodeBlock { .. }
                | Self::MathBlock
                | Self::MermaidBlock
                | Self::HtmlBlock
                | Self::Comment
                | Self::RawMarkdown
                | Self::FrontMatter
        )
    }

    pub fn is_quote_container(&self) -> bool {
        matches!(self, Self::Quote | Self::Callout(_))
    }

    /// Blocks that render as self-contained widgets with no caret position
    /// after them. At the end of a rendered document they need a trailing
    /// paragraph so a rendered-first user can keep typing past the structure
    /// instead of having to drop to source mode.
    pub fn is_atomic_structural(&self) -> bool {
        matches!(
            self,
            Self::Separator
                | Self::Table
                | Self::CodeBlock { .. }
                | Self::MathBlock
                | Self::MermaidBlock
                | Self::HtmlBlock
                | Self::Comment
                | Self::RawMarkdown
                | Self::FrontMatter
        )
    }

    pub fn is_callout(&self) -> bool {
        matches!(self, Self::Callout(_))
    }

    /// Blocks edited as multi-line raw text that render as self-contained
    /// widgets (code, math, HTML, mermaid, comment, raw markdown). Exiting one
    /// downward with `Down` or `Ctrl/Cmd+Enter` needs a line below to land on.
    pub fn is_multiline_text_block(&self) -> bool {
        self.is_code_block()
            || matches!(
                self,
                Self::MathBlock
                    | Self::HtmlBlock
                    | Self::MermaidBlock
                    | Self::Comment
                    | Self::RawMarkdown
                    | Self::FrontMatter
            )
    }

    pub fn is_footnote_definition(&self) -> bool {
        matches!(self, Self::FootnoteDefinition)
    }

    pub fn callout_variant(&self) -> Option<CalloutVariant> {
        match self {
            Self::Callout(variant) => Some(*variant),
            _ => None,
        }
    }

    pub fn is_separator(&self) -> bool {
        matches!(self, Self::Separator)
    }

    pub fn can_nest_under(&self, parent: &Self) -> bool {
        if !parent.is_list_item() {
            return false;
        }

        self.is_list_item()
            || matches!(
                self,
                Self::Paragraph
                    | Self::Quote
                    | Self::Callout(_)
                    | Self::FootnoteDefinition
                    | Self::Table
                    | Self::CodeBlock { .. }
                    | Self::Comment
                    | Self::HtmlBlock
                    | Self::MathBlock
                    | Self::MermaidBlock
                    | Self::RawMarkdown
            )
    }

    pub fn newline_sibling_kind(&self) -> Self {
        if matches!(self, Self::TaskListItem { .. }) {
            Self::TaskListItem { checked: false }
        } else if self.is_list_item() {
            self.clone()
        } else if self.is_quote_container() {
            self.clone()
        } else if self.is_footnote_definition() {
            Self::Paragraph
        } else if self.is_code_block() || self.is_separator() {
            Self::Paragraph
        } else {
            Self::Paragraph
        }
    }

    /// Live-detects a Markdown prefix from user input and returns the
    /// corresponding [`BlockKind`] together with the character count of
    /// the prefix that should be stripped.
    pub fn detect_markdown_shortcut(value: &str) -> Option<(Self, usize)> {
        if value.starts_with("###### ") {
            Some((Self::Heading { level: 6 }, 7))
        } else if value.starts_with("##### ") {
            Some((Self::Heading { level: 5 }, 6))
        } else if value.starts_with("#### ") {
            Some((Self::Heading { level: 4 }, 5))
        } else if value.starts_with("### ") {
            Some((Self::Heading { level: 3 }, 4))
        } else if value.starts_with("## ") {
            Some((Self::Heading { level: 2 }, 3))
        } else if value.starts_with("# ") {
            Some((Self::Heading { level: 1 }, 2))
        } else if let Some((checked, prefix_len)) = Self::parse_task_list_shortcut(value) {
            Some((Self::TaskListItem { checked }, prefix_len))
        } else if value.starts_with("* ") || value.starts_with("+ ") {
            Some((Self::BulletedListItem, 2))
        } else if value.starts_with("- ") {
            Some((Self::BulletedListItem, 2))
        } else if let Some(prefix_len) = Self::numbered_list_shortcut_prefix_len(value) {
            Some((Self::NumberedListItem, prefix_len))
        } else if value.starts_with("> ") {
            Some((Self::Quote, 2))
        } else {
            None
        }
    }

    /// 从用户刚敲下的一行文本里读出列表记号的**写法**：`+ ` 的加号、`1)` 的括号。
    /// `detect_markdown_shortcut` 只回答「这是哪种块」，写法是它丢掉的半个信息。
    pub fn detect_list_marker_style(value: &str) -> Option<ListMarkerStyle> {
        if let Some(bullet @ ('-' | '*' | '+')) = value.chars().next()
            && value[bullet.len_utf8()..].starts_with([' ', '\t'])
        {
            return Some(ListMarkerStyle {
                bullet: Some(bullet),
                delimiter: None,
            });
        }
        let digits = value.bytes().take_while(|byte| byte.is_ascii_digit()).count();
        if digits > 0
            && let Some(delimiter @ ('.' | ')')) = value[digits..].chars().next()
            && value[digits + delimiter.len_utf8()..].starts_with([' ', '\t'])
        {
            return Some(ListMarkerStyle {
                bullet: None,
                delimiter: Some(delimiter),
            });
        }
        None
    }

    pub fn parse_atx_heading_line(line: &str) -> Option<(u8, String)> {
        Self::parse_atx_heading_line_with_marker(line)
            .map(|(level, content, _marker_len)| (level, content))
    }

    /// 同 `parse_atx_heading_line`，另外给出**内容在这一行里从第几个字节开始**。
    ///
    /// 记号（含它后面的那个空格与本行前导缩进）占几位是文件里的事实，按模型拼
    /// `# ` 会算错：缩进过的 `  # 标题` 内容在第 3 个字节。读侧的块内偏移靠它。
    pub fn parse_atx_heading_line_with_marker(line: &str) -> Option<(u8, String, usize)> {
        let trimmed_end = line.trim_end();
        let leading_spaces = trimmed_end.bytes().take_while(|b| *b == b' ').count();
        if leading_spaces > 3 {
            return None;
        }

        let rest = &trimmed_end[leading_spaces..];
        let level = rest.bytes().take_while(|b| *b == b'#').count();
        if !(1..=6).contains(&level) {
            return None;
        }

        let content = rest[level..].strip_prefix(' ')?;
        let marker_len = leading_spaces + level + 1;
        let mut content = content.trim_end().to_string();
        if let Some(closing_hash_start) = content.rfind(' ')
            && content[closing_hash_start + 1..]
                .chars()
                .all(|ch| ch == '#')
        {
            content.truncate(closing_hash_start);
            content = content.trim_end().to_string();
        }

        Some((level as u8, content, marker_len))
    }

    pub fn parse_setext_underline(line: &str) -> Option<u8> {
        let trimmed_end = line.trim_end();
        let leading_spaces = trimmed_end.bytes().take_while(|b| *b == b' ').count();
        if leading_spaces > 3 {
            return None;
        }

        let rest = &trimmed_end[leading_spaces..];
        if rest.len() < 3 {
            return None;
        }

        if rest.bytes().all(|b| b == b'=') {
            Some(1)
        } else if rest.bytes().all(|b| b == b'-') {
            Some(2)
        } else {
            None
        }
    }

    pub fn parse_code_fence_opening(value: &str) -> Option<CodeFenceOpening> {
        let trimmed = value.trim_end();
        let ch = trimmed.chars().next()?;
        if ch != '`' && ch != '~' {
            return None;
        }

        let len = trimmed.chars().take_while(|&c| c == ch).count();
        if len < 3 {
            return None;
        }

        let rest = &trimmed[ch.len_utf8() * len..];
        if ch == '`' && rest.contains('`') {
            return None;
        }

        let language = rest.trim();
        Some(CodeFenceOpening {
            ch,
            len,
            language: if language.is_empty() {
                None
            } else {
                Some(language.to_string().into())
            },
        })
    }

    pub fn parse_separator_line(value: &str) -> bool {
        let trimmed_end = value.trim_end();
        let leading_spaces = trimmed_end.bytes().take_while(|b| *b == b' ').count();
        if leading_spaces > 3 {
            return false;
        }

        let rest = &trimmed_end[leading_spaces..];
        let mut marker = None;
        let mut marker_count = 0usize;
        for ch in rest.chars() {
            if ch == ' ' {
                continue;
            }
            if !matches!(ch, '-' | '*' | '_') {
                return false;
            }
            if let Some(existing) = marker {
                if existing != ch {
                    return false;
                }
            } else {
                marker = Some(ch);
            }
            marker_count += 1;
        }

        marker_count >= 3
    }

    /// Parses a task-list marker at the start of list-item content.
    ///
    /// Accepted forms are `[ ]`, `[x]`, and `[X]`, optionally followed by a
    /// space or tab before the item text. An empty title is also valid.
    pub fn parse_task_list_item_prefix(value: &str) -> Option<(bool, usize)> {
        let bytes = value.as_bytes();
        if bytes.len() < 3 || bytes[0] != b'[' || bytes[2] != b']' {
            return None;
        }

        let checked = match bytes[1] {
            b' ' => false,
            b'x' | b'X' => true,
            _ => return None,
        };

        if bytes.len() == 3 {
            return Some((checked, 3));
        }

        if matches!(bytes[3], b' ' | b'\t') {
            Some((checked, 4))
        } else {
            None
        }
    }

    fn parse_task_list_shortcut(value: &str) -> Option<(bool, usize)> {
        let rest = value.strip_prefix("- ")?;
        let (checked, prefix_len) = Self::parse_task_list_item_prefix(rest)?;
        Some((checked, 2 + prefix_len))
    }

    fn numbered_list_shortcut_prefix_len(value: &str) -> Option<usize> {
        let digit_len = value.bytes().take_while(|b| b.is_ascii_digit()).count();
        if !(1..=9).contains(&digit_len) {
            return None;
        }

        let marker = *value.as_bytes().get(digit_len)?;
        if !matches!(marker, b'.' | b')') {
            return None;
        }

        let separator = *value.as_bytes().get(digit_len + 1)?;
        matches!(separator, b' ' | b'\t').then_some(digit_len + 2)
    }
}

/// Persistent data of a block independent of the editor runtime.
///
/// Holds the block's identity, kind, inline-formatted title, and tree
/// references (parent/children via UUID). Raw-preserved Markdown keeps its
/// original source in `raw_fallback` so it round-trips through save/load
/// losslessly.
#[derive(Debug, Clone)]
pub struct BlockRecord {
    pub id: Uuid,
    pub kind: BlockKind,
    pub title: InlineTextTree,
    pub table: Option<TableData>,
    pub html: Option<HtmlDocument>,
    pub parent: Option<Uuid>,
    pub content: Vec<Uuid>,
    pub raw_fallback: Option<String>,
    /// 列表项自己写的记号（`+`/`*`/`-`、`.`/`)`）。见 [`ListMarkerStyle`]。
    pub list_marker: ListMarkerStyle,
    /// 该块在文档缓冲区里占的源码区间（字节）。由导入器在解析时记录，
    /// 不是序列化之后反推出来的——这是「块是文本的投影」这条不变式的载体。
    /// 子块与新建块暂时没有区间（`None`），等接入写回路径后由重投影补上。
    pub source_span: Option<std::ops::Range<usize>>,
    /// 标题树版本：每次 `set_title` 递增。markdown 序列化备忘键就靠它，
    /// 块自己的 markdown 只在自己被改时重算（P2：序列化曾占每键成本大半）。
    title_revision: u64,
    /// markdown 输出备忘：(标题树版本, markdown)。`RefCell` 以便 `&self` 读取路径
    /// 命中。（序列化是纯函数，版本不对就重算，不存在脏读风险。）
    markdown_memo: std::cell::RefCell<Option<(u64, String)>>,
}

impl BlockRecord {
    pub fn new(kind: BlockKind, title: InlineTextTree) -> Self {
        let mut record = Self {
            id: Uuid::new_v4(),
            kind,
            title,
            table: None,
            html: None,
            parent: None,
            content: Vec::new(),
            raw_fallback: None,
            list_marker: ListMarkerStyle::default(),
            source_span: None,
            title_revision: 0,
            markdown_memo: std::cell::RefCell::new(None),
        };
        record.sync_raw_fallback();
        record
    }

    pub fn with_plain_text(kind: BlockKind, text: impl Into<String>) -> Self {
        Self::new(kind, InlineTextTree::plain(text.into()))
    }

    pub fn paragraph(text: impl Into<String>) -> Self {
        Self::with_plain_text(BlockKind::Paragraph, text)
    }

    pub fn raw_markdown(markdown: impl Into<String>) -> Self {
        let markdown = markdown.into();
        let mut record = Self::with_plain_text(BlockKind::RawMarkdown, markdown.clone());
        record.raw_fallback = Some(markdown);
        record
    }

    pub fn front_matter(markdown: impl Into<String>) -> Self {
        let markdown = markdown.into();
        let mut record = Self::with_plain_text(BlockKind::FrontMatter, markdown.clone());
        record.raw_fallback = Some(markdown);
        record
    }

    pub fn comment(markdown: impl Into<String>) -> Self {
        let markdown = markdown.into();
        let mut record = Self::with_plain_text(BlockKind::Comment, markdown.clone());
        record.raw_fallback = Some(markdown);
        record
    }

    pub fn html(markdown: impl Into<String>) -> Self {
        let markdown = markdown.into();
        let html = parse_html_document(&markdown);
        let mut record = Self::with_plain_text(BlockKind::HtmlBlock, markdown.clone());
        record.html = Some(html);
        record.raw_fallback = Some(markdown);
        record
    }

    pub fn math(markdown: impl Into<String>) -> Self {
        let markdown = markdown.into();
        let mut record = Self::with_plain_text(BlockKind::MathBlock, markdown.clone());
        record.raw_fallback = Some(markdown);
        record
    }

    pub fn mermaid(markdown: impl Into<String>) -> Self {
        let markdown = markdown.into();
        let mut record = Self::with_plain_text(BlockKind::MermaidBlock, markdown.clone());
        record.raw_fallback = Some(markdown);
        record
    }

    pub fn table(table: TableData) -> Self {
        let mut record = Self::new(BlockKind::Table, InlineTextTree::plain(String::new()));
        record.table = Some(table);
        record
    }

    pub fn set_title(&mut self, title: InlineTextTree) {
        self.title = title;
        self.title_revision = self.title_revision.wrapping_add(1);
        self.sync_raw_fallback();
    }

    /// 把这份记录里「用户自己写的记号」清成默认写法：列表记号（`+`/`1)`）与行内强调
    /// 记号（`__`），表格连带每一格。显式「格式化文档」专用，见
    /// [`InlineTextTree::reset_emphasis_markers`]。
    ///
    /// 走 [`Self::set_title`] 而不是就地改树：markdown 备忘按 `title_revision` 缓存，
    /// 版本号不动就会把旧写法继续交出去。
    pub fn canonicalize_writing_style(&mut self) {
        self.list_marker = ListMarkerStyle::default();
        let mut title = self.title.clone();
        title.reset_emphasis_markers();
        self.set_title(title);
        if let Some(table) = self.table.as_mut() {
            for cell in table.header.iter_mut() {
                cell.reset_emphasis_markers();
            }
            for row in table.rows.iter_mut() {
                for cell in row.iter_mut() {
                    cell.reset_emphasis_markers();
                }
            }
        }
    }

    /// Export the block title as Markdown: fragment style flags are
    /// serialized back to delimiter markers via [`InlineTextTree::serialize_markdown`].
    ///
    /// 按 [`Self::title_revision`] 备忘：整篇序列化时每个未改动的块直接拿上次
    /// 结果，1 MiB 文档的序列化从 416ms 降到只算被改的那几块。
    pub fn title_markdown(&self) -> String {
        if let Some((revision, markdown)) = self.markdown_memo.borrow().as_ref()
            && *revision == self.title_revision
        {
            return markdown.clone();
        }
        let markdown = self.title.serialize_markdown();
        *self.markdown_memo.borrow_mut() = Some((self.title_revision, markdown.clone()));
        markdown
    }

    /// Returns true for block kinds that keep their original source text
    /// in `raw_fallback` because they are preserved as opaque Markdown.
    pub fn kind_uses_raw_fallback(&self) -> bool {
        matches!(
            self.kind,
            BlockKind::RawMarkdown
                | BlockKind::FrontMatter
                | BlockKind::Comment
                | BlockKind::HtmlBlock
                | BlockKind::MathBlock
                | BlockKind::MermaidBlock
        )
    }

    /// Serialize this block back to a single Markdown line, including
    /// indentation for nested blocks and list ordinal for numbered items.
    /// Raw-preserved blocks produce their fallback text when at depth 0.
    pub fn markdown_line(&self, depth: usize, list_ordinal: Option<usize>) -> String {
        let indentation = "  ".repeat(depth);
        let title_markdown = self.title_markdown_for_output();
        match self.kind {
            BlockKind::Paragraph => indent_multiline(&title_markdown, &indentation),
            BlockKind::Separator => "---".to_string(),
            BlockKind::Heading { level } => {
                format!(
                    "{indentation}{} {title_markdown}",
                    "#".repeat(level as usize)
                )
            }
            BlockKind::BulletedListItem => prefixed_multiline(
                &title_markdown,
                &format!("{indentation}{} ", self.list_marker.bullet_or_default()),
                &format!("{indentation}  "),
            ),
            BlockKind::TaskListItem { checked } => prefixed_multiline(
                &title_markdown,
                &format!(
                    "{indentation}{} [{}] ",
                    self.list_marker.bullet_or_default(),
                    if checked { "x" } else { " " }
                ),
                &format!("{indentation}      "),
            ),
            BlockKind::NumberedListItem => {
                let ordinal = list_ordinal.unwrap_or(1);
                prefixed_multiline(
                    &title_markdown,
                    &format!(
                        "{indentation}{ordinal}{} ",
                        self.list_marker.delimiter_or_default()
                    ),
                    &format!("{indentation}   "),
                )
            }
            BlockKind::Quote => prefixed_multiline(
                &CalloutVariant::escape_plain_quote_header(&title_markdown),
                &format!("{indentation}> "),
                &format!("{indentation}> "),
            ),
            BlockKind::Callout(variant) => format!(
                "{indentation}> {}",
                variant.header_markdown(&title_markdown)
            ),
            BlockKind::FootnoteDefinition => {
                format!("{indentation}[^{}]: ", self.title.visible_text())
            }
            BlockKind::Table => String::new(),
            BlockKind::CodeBlock { .. } => title_markdown,
            BlockKind::RawMarkdown
            | BlockKind::FrontMatter
            | BlockKind::Comment
            | BlockKind::HtmlBlock
            | BlockKind::MathBlock
            | BlockKind::MermaidBlock => {
                if depth == 0 {
                    self.raw_fallback.clone().unwrap_or(title_markdown)
                } else {
                    indent_multiline(
                        &self.raw_fallback.clone().unwrap_or(title_markdown),
                        &indentation,
                    )
                }
            }
        }
    }

    fn title_markdown_for_output(&self) -> String {
        let visible = self.title.visible_text();
        if self.can_present_title_as_standalone_image()
            && parse_standalone_image(&visible).is_some()
        {
            return visible;
        }

        self.title_markdown()
    }

    fn can_present_title_as_standalone_image(&self) -> bool {
        matches!(
            self.kind,
            BlockKind::Paragraph
                | BlockKind::BulletedListItem
                | BlockKind::NumberedListItem
                | BlockKind::TaskListItem { .. }
        )
    }

    fn sync_raw_fallback(&mut self) {
        if self.kind_uses_raw_fallback() {
            self.raw_fallback = Some(self.title.visible_text().to_string());
            if self.kind == BlockKind::HtmlBlock {
                self.html = self
                    .raw_fallback
                    .as_ref()
                    .map(|raw| parse_html_document(raw));
            }
        } else {
            self.raw_fallback = None;
            self.html = None;
        }
    }
}

fn indent_multiline(content: &str, indentation: &str) -> String {
    content
        .split('\n')
        .map(|line| format!("{indentation}{line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn prefixed_multiline(content: &str, first_prefix: &str, continuation_prefix: &str) -> String {
    let mut lines = content.split('\n');
    let mut rendered = String::new();
    if let Some(first) = lines.next() {
        rendered.push_str(first_prefix);
        rendered.push_str(first);
    }

    for line in lines {
        rendered.push('\n');
        rendered.push_str(continuation_prefix);
        rendered.push_str(line);
    }

    rendered
}

/// Image payload extracted from GPUI's clipboard abstraction.
///
/// File-manager copies are usually represented as local paths, while bitmap
/// copies from image editors or browsers arrive as encoded image bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PastedImageSource {
    ClipboardImage(Image),
    LocalPath(PathBuf),
}

/// Events emitted by a block to its parent editor when structural
/// changes or focus transfers are needed that the block cannot handle alone.
///
/// The Editor subscribes to these events on every block via
/// `cx.subscribe(&block, Self::on_block_event)`.
#[derive(Debug, Clone)]
pub enum BlockEvent {
    /// Capture the current document state before an upcoming mutation.
    PrepareUndo { kind: UndoCaptureKind },
    /// The block's content or kind changed; the editor should mark the
    /// document dirty and optionally scroll to keep the block visible.
    Changed,
    /// The heading's fold chevron was clicked (roadmap C7). The editor flips
    /// `folded` and re-renders; view-only state, so the document is not
    /// marked dirty.
    RequestToggleFold,
    /// A `[TOC]` entry was clicked (roadmap C2): jump to that heading line.
    RequestJumpToHeadingLine { line: usize },
    /// The user pressed Enter; a new block should be created after this
    /// one with the given trailing text.
    RequestNewline {
        trailing: InlineTextTree,
        source_already_mutated: bool,
    },
    /// The user pressed Enter on a callout header; the editor should ensure
    /// the callout owns a body entry and move focus into it.
    RequestEnterCalloutBody,
    /// The user requested a quote-group break at the current quote depth.
    /// The editor should insert a new empty quote group at the current depth,
    /// with whatever separator structure is required by Markdown at that level.
    RequestQuoteBreak,
    /// The user requested to exit the current callout into a plain text block.
    /// The editor should insert the separator structure needed to end the
    /// surrounding quote group, then focus a plain paragraph entry below it.
    RequestCalloutBreak,
    /// The user pressed Backspace at the start of this block; its entire
    /// content should be appended to the previous block.
    RequestMergeIntoPrev { content: InlineTextTree },
    /// 源码分块文档中，用户在块尾按了 Delete（前向删除）：下一个块的
    /// 内容应并入本块（删掉块边界换行）。
    RequestMergeFromNext,
    /// A multi-line paste was detected; the editor must split the pasted
    /// lines into separate blocks and re-attach the leading/trailing text
    /// to the correct positions.
    RequestPasteMultiline {
        leading: InlineTextTree,
        lines: Vec<String>,
        trailing: InlineTextTree,
        split_physical_lines: bool,
    },
    /// An image-like clipboard payload was pasted. The editor resolves
    /// storage preferences and inserts either an image block or image text.
    RequestPasteImage {
        leading: InlineTextTree,
        source: PastedImageSource,
        trailing: InlineTextTree,
    },
    /// Replace the current editor-level cross-block selection with text
    /// submitted through the focused block input handler.
    RequestReplaceCrossBlockSelection {
        text: String,
        selected_range_relative: Option<Range<usize>>,
        mark_inserted_text: bool,
        undo_kind: UndoCaptureKind,
    },
    /// Ctrl/Cmd+A was pressed in rendered editing. The editor decides whether
    /// this press selects the focused block or upgrades to all rendered blocks.
    RequestRenderedSelectAll,
    /// Tab pressed in list context; increase the current block's nesting when
    /// the previous visible block can adopt it.
    RequestIndent,
    /// Shift-Tab pressed in list context; lift the current block out one level.
    RequestOutdent,
    /// Backspace on a nested list item should remove its marker first,
    /// degrading it into a direct list-child paragraph at the same depth.
    RequestDowngradeNestedListItemToChildParagraph,
    /// Toggle the checked state of a task-list item.
    ToggleTaskChecked,
    /// A `#tag` word was clicked in rendered text; the editor should open the
    /// search panel scoped to the workspace with this query (roadmap C4).
    RequestSearchTag {
        query: String,
    },
    /// A `[[wikilink]]` was clicked; open the named workspace file, creating
    /// it when missing (roadmap C3).
    RequestOpenWikilink {
        target: String,
    },
    /// 打开被点击的行内链接目标（`open_target` 是解析后的落点）。
    /// 编辑器直接跳转，不再弹确认框。
    RequestOpenLink { open_target: String },
    /// Jump from a rendered footnote reference to the corresponding
    /// in-place footnote definition block.
    RequestJumpToFootnoteDefinition { id: String },
    /// Jump from an in-place footnote definition back to its first reference.
    RequestJumpToFootnoteBackref { id: String },
    /// Move focus horizontally across native table cells.
    RequestTableCellMoveHorizontal { delta: i32 },
    /// Move focus vertically across native table cells.
    RequestTableCellMoveVertical { delta: i32 },
    /// Append one empty column to a native table.
    RequestAppendTableColumn,
    /// Append one empty body row to a native table.
    RequestAppendTableRow,
    /// A native table axis handle was entered or left by the pointer.
    /// `hovered` distinguishes the two so the editor can ignore a leave
    /// that arrives after an adjacent handle has already taken the preview.
    RequestTableAxisPreview {
        kind: TableAxisKind,
        index: usize,
        hovered: bool,
    },
    /// Select one native table row or column for batch operations.
    RequestSelectTableAxis { kind: TableAxisKind, index: usize },
    /// Open the axis context menu for a native table row or column.
    RequestOpenTableAxisMenu {
        kind: TableAxisKind,
        index: usize,
        position: Point<Pixels>,
    },
    /// Cursor reached the top of this block; move focus to the previous
    /// visible block, preserving the preferred horizontal position.
    RequestFocusPrev { preferred_x: Option<f32> },
    /// Cursor reached the bottom of this block; move focus to the next
    /// visible block, preserving the preferred horizontal position.
    RequestFocusNext { preferred_x: Option<f32> },
    /// Move focus to the start of the previous visible block.
    RequestBlockUp,
    /// Move focus to the start of the next visible block.
    RequestBlockDown,
    /// This block should be deleted (empty and backspace/delete pressed).
    RequestDelete,
    /// The user clicked this block; notify siblings so they re-render
    /// in display mode.
    RequestFocus,
}

/// Undo coalescing category captured before a mutation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UndoCaptureKind {
    /// Text edits that may merge with adjacent typing within the coalescing window.
    CoalescibleText,
    /// An in-progress input-method composition, which must remain one undo step
    /// regardless of how long the candidate window stays open.
    ImeComposition,
    /// The commit or cancellation that ends an input-method composition.
    ImeCompositionCommit,
    /// Structural or discrete edits that always form their own undo entry.
    NonCoalescible,
}


#[cfg(test)]
mod tests;
