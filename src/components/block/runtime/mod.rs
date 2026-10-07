//! Editable block runtime and block-local state transitions.

pub(super) use std::ops::Range;
pub(super) use std::path::{Path, PathBuf};
pub(super) use std::sync::Arc;
pub(super) use std::time::{Duration, Instant};

pub(super) use gpui::*;
pub(super) use unicode_segmentation::*;

mod code;
mod image;
mod projection;
mod table;

use self::projection::{
    ExpandedInlineProjection, ExpandedInlineSegment, ExpandedInlineSegmentKind, ExpandedLinkRun,
    ProjectedLinkSelectionSnapshot,
};
use super::{
    BlockEvent, BlockKind, BlockRecord, CalloutVariant, FootnoteRegistry, InlineFootnoteHit,
    UndoCaptureKind,
};
use super::{CodeHighlightResult, CodeLanguageKey, highlight_code_block};
use crate::components::markdown::source_highlight::{
    MarkdownSourceState, highlight_latex_source, highlight_markdown_source,
};
use super::{
    ImageReferenceDefinitions, ImageResolvedSource, ImageSyntax, LinkReferenceDefinitions,
    parse_standalone_image, resolve_image_source, standalone_image_width_percent,
};
use crate::components::markdown::inline::{
    InlineFragment, InlineInsertionAttributes, InlineLink, InlineLinkHit, InlineRenderCache,
    InlineSpan, InlineStyle, InlineTextTree, StyleFlag, clamp_to_char_boundary,
};
use crate::components::{
    TableAxisHighlight, TableAxisMarker, TableCellPosition, TableColumnAlignment, TableRuntime,
};

/// Inline formatting command issued by editor actions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InlineFormat {
    /// Toggle bold formatting.
    Bold,
    /// Toggle italic formatting.
    Italic,
    /// Toggle underline formatting.
    Underline,
    /// Toggle strikethrough formatting.
    Strikethrough,
    /// Toggle inline code formatting.
    Code,
    /// 上标：`^x^`。
    Superscript,
    /// 下标：`~x~`。
    Subscript,
    /// Typora 式的标记文本：`==x==`。
    Highlight,
}

/// Editing semantics for the current block.
///
/// Rich blocks edit the attribute-based text tree, while source mode and code
/// blocks edit raw text without inline Markdown normalization.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EditMode {
    /// Attribute-based rich text editing for normal rendered blocks.
    RenderedRich,
    /// Raw Markdown editing for source-mode and raw fallback blocks.
    SourceRaw,
    /// Raw text editing for fenced code block contents.
    CodeBlockRaw,
}

impl EditMode {
    fn for_kind(kind: &BlockKind) -> Self {
        // FrontMatter 与代码块同构：始终以 YAML 源码 widget 呈现、块内 raw 编辑。
        // 不能归入 SourceRaw——那会让 `is_source_raw_mode()` 在未聚焦时把它
        // 引到纯文本渲染分支，绕过代码块外观。
        if kind.is_code_block() || matches!(kind, BlockKind::FrontMatter) {
            Self::CodeBlockRaw
        } else if matches!(
            kind,
            BlockKind::RawMarkdown
                | BlockKind::Comment
                | BlockKind::HtmlBlock
                | BlockKind::MathBlock
                | BlockKind::MermaidBlock
        ) {
            Self::SourceRaw
        } else {
            Self::RenderedRich
        }
    }

    fn uses_raw_text_editing(self) -> bool {
        matches!(self, Self::SourceRaw | Self::CodeBlockRaw)
    }

    fn supports_inline_projection(self) -> bool {
        matches!(self, Self::RenderedRich)
    }
}

impl EventEmitter<BlockEvent> for Block {}

/// 把可见文本拼进 markdown 时的转义：只有反斜杠要再转义一次。其余标记字符
/// （`*`、`` ` ``、`~` 等）保持原样，与渲染路径的实时标记解析一致。
fn escape_markdown_insertion(text: &str) -> String {
    if text.contains('\\') {
        text.replace('\\', "\\\\")
    } else {
        text.to_string()
    }
}

/// 可见文本偏移 → 拼进 markdown 后的偏移（反斜杠占两个字符）。
fn markdown_insertion_offset(text: &str, visible_offset: usize) -> usize {
    let mut visible_offset = visible_offset.min(text.len());
    while visible_offset > 0 && !text.is_char_boundary(visible_offset) {
        visible_offset -= 1;
    }
    text[..visible_offset]
        .chars()
        .map(|ch| if ch == '\\' { 2 } else { ch.len_utf8() })
        .sum()
}

/// A single editable block in the document tree.
///
/// Each block holds a [`BlockRecord`] containing the persistent data (kind,
/// title, UUIDs) and a [`FocusHandle`] for keyboard routing.  Runtime state
/// such as selection, cursor blink, and layout cache live on the struct.
///
/// Blocks delegate structural operations (split, merge, indent, delete) to
/// the parent editor via `BlockEvent` emissions.
pub struct Block {
    pub record: BlockRecord,
    pub(crate) render_cache: InlineRenderCache,
    code_highlight: Option<CodeHighlightResult>,
    pub children: Vec<Entity<Block>>,
    pub focus_handle: FocusHandle,
    pub(crate) code_language_focus_handle: FocusHandle,
    pub(crate) code_language_selected_range: Range<usize>,
    pub(crate) code_language_selection_reversed: bool,
    pub(crate) code_language_marked_range: Option<Range<usize>>,
    pub(crate) code_language_last_layout: Option<ShapedLine>,
    pub(crate) code_language_last_bounds: Option<Bounds<Pixels>>,
    pub(crate) code_language_is_selecting: bool,
    pub selected_range: Range<usize>,
    /// Content-local byte ranges highlighted as document search matches
    /// (roadmap B2). Owned by the editor's search panel state.
    pub(crate) search_highlight_ranges: Vec<Range<usize>>,
    /// 当前活动命中的块内区间（循环跳转/点击结果时由高亮同步写入）。
    pub(crate) search_active_range: Option<Range<usize>>,
    /// Pending `#tag` click forwarded to the editor (roadmap C4).
    pub(crate) tag_query: Option<String>,
    /// Heading fold state (roadmap C7): when true, the section content below
    /// this heading is hidden.
    pub(crate) folded: bool,
    /// Whether this heading owns section content, i.e. folding it would hide
    /// anything. Recomputed by the editor's fold filter each render; the
    /// chevron only renders when true (or when already folded).
    pub(crate) foldable: bool,
    /// 会话内图片宽度缩放因子（roadmap C10 拖拽缩放），1.0 = 默认。
    pub(crate) image_width_factor: f32,
    /// Active image resize drag: pointer X at drag start + factor at start.
    pub(crate) image_resize_drag: Option<crate::editor::ImageResizeDrag>,
    /// Pending `[[wikilink]]` click target (roadmap C3).
    pub(crate) wikilink_target: Option<String>,
    pub selection_reversed: bool,
    pub(crate) editor_selection_range: Option<Range<usize>>,
    pub marked_range: Option<Range<usize>>,
    pub last_layout: Option<Vec<WrappedLine>>,
    pub last_bounds: Option<Bounds<Pixels>>,
    pub last_line_height: Pixels,
    pub render_depth: usize,
    pub quote_depth: usize,
    pub(crate) quote_group_anchor: Option<uuid::Uuid>,
    pub(crate) visible_quote_depth: usize,
    pub(crate) visible_quote_group_anchor: Option<uuid::Uuid>,
    pub(crate) callout_depth: usize,
    pub(crate) callout_anchor: Option<uuid::Uuid>,
    pub(crate) callout_variant: Option<CalloutVariant>,
    pub(crate) footnote_anchor: Option<uuid::Uuid>,
    pub(crate) parent_is_list_item: bool,
    pub list_ordinal: Option<usize>,
    pub is_selecting: bool,
    pub cursor_blink_epoch: Instant,
    pub vertical_motion_x: Option<Pixels>,
    pub(super) cursor_blink_task: Option<Task<()>>,
    /// Cached projection used to show editable inline delimiters for the
    /// currently touched inline span(s).
    pub(crate) projection: Option<ExpandedInlineProjection>,
    /// Inputs that produced the current `projection`. When the next
    /// `sync_inline_projection_for_focus` computes the same inputs, the
    /// rebuild is skipped — saves a full O(fragments + text) walk per
    /// render frame (cursor blink + every arrow keypress).
    projection_cache_key: Option<(bool, Range<usize>, Option<Range<usize>>)>,
    /// Display text held as a SharedString so renders can clone an Arc
    /// instead of re-allocating per frame. Refreshed in `sync_render_cache`,
    /// `rebuild_inline_projection`, and `clear_inline_projection`.
    cached_display_text: SharedString,
    /// 文本代数：cached_display_text 内容变化时递增（P3 shape 备忘键）。
    display_generation: u64,
    /// P3：跨帧 shape 备忘。键命中时布局闭包跳过 build_text_runs 与
    /// shape_text（taffy 一次布局会对同一元素多次 measure）。
    pub(crate) shape_memo: Option<ShapeMemoEntry>,
    /// 长行折叠计划：源行字节范围 + 哪些行超长。`(文本代数, 计划)` 按代数
    /// 缓存，文本变化时由下次布局重算（element 布局时读取）。
    pub(crate) long_line_plan: Option<(u64, std::sync::Arc<LongLinePlan>)>,
    /// 超长行的展开状态（块内源行下标）。展开 = 该行按容器宽换行；
    /// 折叠 = 单行不换行，超出部分裁切显示（不允许横向滚动）。
    pub(crate) expanded_long_lines: std::collections::BTreeSet<usize>,
    /// 展开/收起长行时递增：进 shape 备忘键（两种状态的换行结果不同）。
    long_line_wrap_generation: u64,
    /// 最近一次 paint 的行号槽宽度（带行号的块才有）；行号点击判定用。
    pub(crate) last_gutter_width: Pixels,
    /// 表格列宽备忘（性能）：`TableColumnLayout::measure` 会对每格做 no-wrap
    /// shape_text，此前每帧全量重测；命中键时整帧零 shape。表内容或键变化
    /// 时失效。
    pub(crate) column_layout_memo: Option<ColumnLayoutMemo>,
    collapsed_caret_affinity: CollapsedCaretAffinity,
    /// When true, block-level shortcuts and inline formatting are
    /// suppressed; the block stores raw text for source-mode editing.
    pub(crate) edit_mode: EditMode,
    show_source_line_numbers: bool,
    /// 1-based 文档行号，本块第一行对应的行号（源码文档按行分块后行号槽续号用）。
    source_line_start: usize,
    /// 行号栏宽度的**全文档基准**：整篇的总行数。各分块按它统一算栏宽，
    /// 上下块的行号才右对齐（按块内最后行号算的话，513 行那块的栏会宽出
    /// 一位——用户报修：512 上下行号对不齐）。0 = 未设置，按块内口径兜底。
    source_line_gutter_basis: usize,
    /// 源码文档分块的高亮语言（markdown 源码块是 Paragraph，语言记在这里；
    /// 代码文件的块是 CodeBlock，语言在 kind 里）。
    source_language: Option<SharedString>,
    /// markdown 源码分块的接缝状态：进入本块时所处的块级构造（上一块的
    /// `source_fence_exit`），由编辑器逐块串联，高亮扫描拿它当初始状态。
    source_fence_entry: Option<MarkdownSourceState>,
    /// 本块结束时的接缝状态；`sync_code_highlight` 顺带算出。
    source_fence_exit: Option<MarkdownSourceState>,
    /// 高亮代数：高亮结果变化时递增，进 shape 备忘键——否则围栏状态级联
    /// 更新（块文本没变）后新配色上不了屏。
    highlight_generation: u64,
    pub(crate) table_runtime: Option<TableRuntime>,
    pub(crate) table_cell_position: Option<TableCellPosition>,
    pub(crate) table_cell_alignment: Option<TableColumnAlignment>,
    pub(crate) table_axis_preview: Option<TableAxisMarker>,
    pub(crate) table_axis_selection: Option<TableAxisMarker>,
    pub(crate) table_axis_highlight: TableAxisHighlight,
    pub(crate) table_append_column_edge_hovered: bool,
    pub(crate) table_append_column_hovered: bool,
    pub(crate) table_append_column_zone_hovered: bool,
    pub(crate) table_append_column_button_hovered: bool,
    pub(crate) table_append_column_close_task: Option<Task<()>>,
    pub(crate) table_append_row_edge_hovered: bool,
    pub(crate) table_append_row_hovered: bool,
    pub(crate) table_append_row_zone_hovered: bool,
    pub(crate) table_append_row_button_hovered: bool,
    pub(crate) table_append_row_close_task: Option<Task<()>>,
    image_runtime: Option<ImageRuntime>,
    image_edit_expanded: bool,
    image_expand_requested: bool,
    pub(crate) html_details_open: bool,
    image_base_dir: Option<PathBuf>,
    image_reference_definitions: Arc<ImageReferenceDefinitions>,
    link_reference_definitions: Arc<LinkReferenceDefinitions>,
    footnote_registry: Arc<FootnoteRegistry>,
    pub(crate) list_group_separator_candidate: bool,
    /// 「复制代码」按钮的反馈时间戳（roadmap B9）：短时间内显示 ✓。
    pub(crate) code_copied_at: Option<Instant>,
    /// 正文为 `[TOC]` 时由编辑器填入的目录条目（roadmap C2）。
    pub(crate) toc_entries: Vec<TocEntry>,
    numbered_list_restart_requested: bool,
    quote_reparse_requested: bool,
    /// 刚在光标处插入的那段文字（可见偏移 + 内容）。编辑器据此只把这几个字节
    /// 插进缓冲区，块自己没碰过的定界符就不会被重新序列化改写。一次事件读走。
    pending_visible_insertion: Option<(usize, String)>,
    /// 这次「标题变短」是拆块切出来的，不是用户删的：光标之后的那截属于即将插进
    /// 来的新块。编辑器据此跳过这次写回，让紧随其后的结构写回连着区间一起处理。
    split_truncation_pending: bool,
}

/// 目录（TOC）条目（roadmap C2）：编辑器按当前标题结构写入 `[TOC]` 块。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TocEntry {
    pub(crate) level: u8,
    pub(crate) title: String,
    /// 标题所在源码行（0 起）。
    pub(crate) line: usize,
}

/// Cached standalone image presentation state for a block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ImageRuntime {
    pub(crate) alt: String,
    pub(crate) src: String,
    pub(crate) title: Option<String>,
    pub(crate) resolved_source: ImageResolvedSource,
}

/// How a collapsed caret at an inline projection boundary inherits style.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum CollapsedCaretAffinity {
    /// Use the normal insertion-attribute lookup.
    #[default]
    Default,
    /// Treat the caret as being just outside the opening delimiter.
    OuterStart,
    /// Treat the caret as being just outside the closing delimiter.
    OuterEnd,
}

impl Block {
    pub fn with_record(cx: &mut Context<Self>, record: BlockRecord) -> Self {
        let edit_mode = EditMode::for_kind(&record.kind);
        let render_cache = record.title.render_cache();
        let mut block = Self {
            record,
            render_cache,
            code_highlight: None,
            children: Vec::new(),
            focus_handle: cx.focus_handle(),
            code_language_focus_handle: cx.focus_handle(),
            code_language_selected_range: 0..0,
            code_language_selection_reversed: false,
            code_language_marked_range: None,
            code_language_last_layout: None,
            code_language_last_bounds: None,
            code_language_is_selecting: false,
            selected_range: 0..0,
            search_highlight_ranges: Vec::new(),
            search_active_range: None,
            tag_query: None,
            folded: false,
            foldable: false,
            image_width_factor: 1.0,
            image_resize_drag: None,
            wikilink_target: None,
            selection_reversed: false,
            editor_selection_range: None,
            marked_range: None,
            last_layout: None,
            last_bounds: None,
            last_line_height: px(20.0),
            render_depth: 0,
            quote_depth: 0,
            quote_group_anchor: None,
            visible_quote_depth: 0,
            visible_quote_group_anchor: None,
            callout_depth: 0,
            callout_anchor: None,
            callout_variant: None,
            footnote_anchor: None,
            parent_is_list_item: false,
            list_ordinal: None,
            is_selecting: false,
            cursor_blink_epoch: Instant::now(),
            vertical_motion_x: None,
            cursor_blink_task: None,
            projection: None,
            projection_cache_key: None,
            cached_display_text: SharedString::default(),
            display_generation: 0,
            shape_memo: None,
            long_line_plan: None,
            expanded_long_lines: std::collections::BTreeSet::new(),
            long_line_wrap_generation: 0,
            last_gutter_width: Pixels::ZERO,
            column_layout_memo: None,
            collapsed_caret_affinity: CollapsedCaretAffinity::Default,
            edit_mode,
            show_source_line_numbers: false,
            source_line_start: 1,
            source_line_gutter_basis: 0,
            source_language: None,
            source_fence_entry: None,
            source_fence_exit: None,
            highlight_generation: 0,
            table_runtime: None,
            table_cell_position: None,
            table_cell_alignment: None,
            table_axis_preview: None,
            table_axis_selection: None,
            table_axis_highlight: TableAxisHighlight::None,
            table_append_column_edge_hovered: false,
            table_append_column_hovered: false,
            table_append_column_zone_hovered: false,
            table_append_column_button_hovered: false,
            table_append_column_close_task: None,
            table_append_row_edge_hovered: false,
            table_append_row_hovered: false,
            table_append_row_zone_hovered: false,
            table_append_row_button_hovered: false,
            table_append_row_close_task: None,
            image_runtime: None,
            image_edit_expanded: false,
            image_expand_requested: false,
            html_details_open: false,
            image_base_dir: None,
            image_reference_definitions: Arc::default(),
            link_reference_definitions: Arc::default(),
            footnote_registry: Arc::default(),
            list_group_separator_candidate: false,
            code_copied_at: None,
            toc_entries: Vec::new(),
            numbered_list_restart_requested: false,
            quote_reparse_requested: false,
            pending_visible_insertion: None,
            split_truncation_pending: false,
        };
        block.sync_code_highlight();
        block.refresh_cached_display_text();
        block
    }

    pub fn kind(&self) -> BlockKind {
        self.record.kind.clone()
    }

    /// True when the block draws nothing in rendered mode: an HTML block whose
    /// fragment has no renderable nodes (stray closing tags). Source mode still
    /// edits the same text as part of the single source block.
    pub(crate) fn renders_nothing(&self) -> bool {
        self.record
            .html
            .as_ref()
            .is_some_and(|document| document.renders_nothing())
    }

    pub(crate) fn is_source_raw_mode(&self) -> bool {
        self.edit_mode == EditMode::SourceRaw
    }

    pub(crate) fn show_source_line_numbers(&self) -> bool {
        self.show_source_line_numbers
    }

    /// 本块第一行在整篇源码文档中的 1-based 行号（分块导入后行号槽续号）。
    pub(crate) fn source_line_start(&self) -> usize {
        self.source_line_start
    }

    pub(crate) fn set_source_line_start(&mut self, start: usize) {
        self.source_line_start = start.max(1);
    }

    /// 全文档总行数（行号栏宽度基准）；0 = 未设置。
    pub(crate) fn source_line_gutter_basis(&self) -> usize {
        self.source_line_gutter_basis
    }

    pub(crate) fn set_source_line_gutter_basis(&mut self, basis: usize) {
        self.source_line_gutter_basis = basis;
    }

    /// 源码文档分块的高亮语言（markdown 源码 = "markdown"）。设置即重算高亮。
    pub(crate) fn set_source_language(&mut self, language: &str) {
        self.source_language = Some(language.to_string().into());
        self.sync_code_highlight();
    }

    /// 本块现在按哪种语言做源码高亮；没设过就是纯文本（高亮为空）。
    pub(crate) fn source_language(&self) -> Option<&str> {
        self.source_language.as_ref().map(|value| &value[..])
    }

    pub(crate) fn take_quote_reparse_requested(&mut self) -> bool {
        let requested = self.quote_reparse_requested;
        self.quote_reparse_requested = false;
        requested
    }

    pub(crate) fn take_numbered_list_restart_requested(&mut self) -> bool {
        let requested = self.numbered_list_restart_requested;
        self.numbered_list_restart_requested = false;
        requested
    }

    pub(crate) fn take_pending_visible_insertion(&mut self) -> Option<(usize, String)> {
        self.pending_visible_insertion.take()
    }

    /// 这次切分是不是拆块切出来的（只读，不清标记）。
    pub(crate) fn split_truncation_pending(&self) -> bool {
        self.split_truncation_pending
    }

    /// 块准备在光标处把自己切成两半。
    pub(crate) fn mark_split_truncation(&mut self) {
        self.split_truncation_pending = true;
    }

    /// 取走拆块标记：紧跟的那次 RequestNewline 就是这次切分的另一半。
    pub(crate) fn take_split_truncation(&mut self) -> bool {
        std::mem::take(&mut self.split_truncation_pending)
    }

    pub(crate) fn set_runtime_context(
        &mut self,
        base_dir: Option<PathBuf>,
        image_reference_definitions: Arc<ImageReferenceDefinitions>,
        link_reference_definitions: Arc<LinkReferenceDefinitions>,
        footnote_registry: Arc<FootnoteRegistry>,
    ) {
        if self.image_base_dir != base_dir {
            self.image_base_dir = base_dir;
        }
        if self.image_reference_definitions != image_reference_definitions {
            self.image_reference_definitions = image_reference_definitions;
        }
        self.sync_link_reference_definitions(link_reference_definitions);
        self.sync_footnote_registry(footnote_registry);
        self.sync_image_runtime();
    }

    /// 单块源码超过阈值时渲染态降级为源码文本（roadmap B12）。
    pub(crate) fn exceeds_long_block_source_limit(&self) -> bool {
        self.display_text().len() > crate::components::LONG_BLOCK_SOURCE_LIMIT
    }

    pub(crate) fn uses_raw_text_editing(&self) -> bool {
        self.edit_mode.uses_raw_text_editing()
    }

    /// 显式「格式化文档」的那一步：把自己和**整棵子树**的写法数据清成默认写法。
    ///
    /// 子块要一起清——引用/标注里的正文、列表项里的段落各有各的强调记号，只清根块
    /// 会留下半新半旧的写法。
    pub(crate) fn canonicalize_writing_style(&mut self, cx: &mut Context<Self>) {
        self.record.canonicalize_writing_style();
        for child in self.children.clone() {
            child.update(cx, |child, cx| child.canonicalize_writing_style(cx));
        }
    }

    pub(crate) fn set_source_raw_mode(&mut self) {
        self.clear_inline_projection();
        self.edit_mode = EditMode::SourceRaw;
        self.show_source_line_numbers = false;
    }

    pub(crate) fn set_source_document_mode(&mut self) {        self.set_source_raw_mode();
        self.show_source_line_numbers = true;
    }

    pub(crate) fn sync_edit_mode_from_kind(&mut self) {
        if self.table_cell_position.is_some() {
            self.edit_mode = EditMode::RenderedRich;
            self.show_source_line_numbers = false;
            return;
        }
        if self.edit_mode != EditMode::SourceRaw {
            if self.kind().is_code_block() {
                self.clear_inline_projection();
            }
            self.edit_mode = EditMode::for_kind(&self.record.kind);
            self.show_source_line_numbers = false;
        }
    }

    pub fn display_text(&self) -> &str {
        self.current_cache().visible_text()
    }

    /// Cheap clone of the current display text as a `SharedString` (Arc bump)
    /// — avoids a fresh String allocation per render. The cached value is
    /// refreshed by [`Self::refresh_cached_display_text`] whenever the
    /// underlying text might have changed.
    pub(crate) fn shared_display_text(&self) -> SharedString {
        self.cached_display_text.clone()
    }

    fn refresh_cached_display_text(&mut self) {
        let current = self.current_cache().visible_text();
        if self.cached_display_text.as_ref() != current {
            self.cached_display_text = SharedString::from(current.to_string());
            self.display_generation = self.display_generation.wrapping_add(1);
            self.shape_memo = None;
        }
    }

    /// P3：读取 shape 备忘（键由 BlockTextElement 布局时计算）。
    pub(crate) fn shape_memo_entry(&self) -> Option<ShapeMemoEntry> {
        self.shape_memo.clone()
    }

    pub(crate) fn set_shape_memo(&mut self, entry: ShapeMemoEntry) {
        self.shape_memo = Some(entry);
    }

    pub(crate) fn display_generation(&self) -> u64 {
        self.display_generation
    }

    pub(crate) fn highlight_generation(&self) -> u64 {
        self.highlight_generation
    }

    /// 本块是否启用长行折叠：只有带行号槽的源码/源文件块（JSONL 等按行分块
    /// 打开的代码文档、降级源码模式的整文档块）才有行号可点、才有折叠意义。
    pub(crate) fn long_line_folding_enabled(&self) -> bool {
        self.show_source_line_numbers
            && (self.kind().is_code_block() || self.kind() == BlockKind::Paragraph)
    }

    /// 长行折叠计划（按文本代数缓存）：源行字节范围 + 超长行下标。
    pub(crate) fn long_line_plan(&mut self) -> std::sync::Arc<LongLinePlan> {
        let generation = self.display_generation;
        if let Some((cached_generation, plan)) = &self.long_line_plan
            && *cached_generation == generation
        {
            return plan.clone();
        }
        let text = self.shared_display_text();
        let ranges = super::element::hard_line_ranges(&text);
        let long_lines = ranges
            .iter()
            .enumerate()
            .filter(|(_, range)| {
                text[range.start..range.end].chars().count() > crate::components::LONG_LINE_SOURCE_LIMIT
            })
            .map(|(line_idx, _)| line_idx)
            .collect();
        let plan = std::sync::Arc::new(LongLinePlan { ranges, long_lines });
        self.long_line_plan = Some((generation, plan.clone()));
        plan
    }

    /// 该行是否超长（折叠单行渲染）。未启用或计划未建时恒 false。
    pub(crate) fn is_long_line(&self, line_idx: usize) -> bool {
        self.long_line_plan
            .as_ref()
            .is_some_and(|(_, plan)| plan.is_long(line_idx))
    }

    /// 行号槽点击：切换该超长行的折叠/展开。返回是否发生了切换。
    pub(crate) fn toggle_long_line_expanded(&mut self, line_idx: usize) -> bool {
        if !self.is_long_line(line_idx) {
            return false;
        }
        if self.expanded_long_lines.contains(&line_idx) {
            self.expanded_long_lines.remove(&line_idx);
        } else {
            self.expanded_long_lines.insert(line_idx);
        }
        self.long_line_wrap_generation = self.long_line_wrap_generation.wrapping_add(1);
        // 换行结果变了：作废 shape 备忘（键里也带 long_line_wrap_generation，
        // 这里清掉保证同帧内 taffy 重复 measure 不命中旧条目）。
        self.shape_memo = None;
        true
    }

    pub(crate) fn long_line_wrap_generation(&self) -> u64 {
        self.long_line_wrap_generation
    }

    /// 表格列宽备忘读取/写入（性能：命中时整帧零 shape_text）。
    pub(crate) fn column_layout_memo(&self) -> Option<&ColumnLayoutMemo> {
        self.column_layout_memo.as_ref()
    }

    pub(crate) fn set_column_layout_memo(&mut self, memo: ColumnLayoutMemo) {
        self.column_layout_memo = Some(memo);
    }

    pub(crate) fn inline_tree_from_markdown_with_context(&self, markdown: &str) -> InlineTextTree {
        InlineTextTree::from_markdown_with_link_references(
            markdown,
            &self.link_reference_definitions,
        )
    }

    pub fn inline_spans(&self) -> &[InlineSpan] {
        self.current_cache().spans()
    }

    #[allow(dead_code)]
    pub fn inline_style_at(&self, offset: usize) -> InlineStyle {
        self.current_cache().style_at(offset)
    }

    #[allow(dead_code)]
    pub(crate) fn inline_html_style_at(
        &self,
        offset: usize,
    ) -> Option<crate::components::HtmlInlineStyle> {
        self.current_cache().html_style_at(offset)
    }

    #[allow(dead_code)]
    pub(crate) fn inline_link_at(&self, offset: usize) -> Option<&str> {
        self.current_cache().link_at(offset)
    }

    #[allow(dead_code)]
    pub(crate) fn inline_link_hit_at(&self, offset: usize) -> Option<&InlineLinkHit> {
        self.current_cache().link_hit_at(offset)
    }

    #[allow(dead_code)]
    pub(crate) fn inline_footnote_hit_at(&self, offset: usize) -> Option<&InlineFootnoteHit> {
        self.current_cache().footnote_hit_at(offset)
    }

    #[allow(dead_code)]
    pub(crate) fn inline_math_at(&self, offset: usize) -> Option<&crate::components::InlineMath> {
        self.current_cache().inline_math_at(offset)
    }

    pub(crate) fn has_mixed_inline_visuals(&self) -> bool {
        self.record.title.has_mixed_inline_visuals()
    }

    pub(crate) fn footnote_definition_id(&self) -> Option<String> {
        self.kind()
            .is_footnote_definition()
            .then(|| self.record.title.visible_text())
    }

    pub(crate) fn footnote_definition_ordinal(&self) -> Option<usize> {
        self.footnote_definition_id()
            .as_deref()
            .and_then(|id| self.footnote_registry.ordinal(id))
    }

    pub(crate) fn footnote_definition_has_backref(&self) -> bool {
        self.footnote_definition_id().as_deref().is_some_and(|id| {
            self.footnote_registry
                .binding(id)
                .and_then(|binding| binding.first_reference.as_ref())
                .is_some()
        })
    }

    pub(crate) fn current_range_for_footnote_occurrence(
        &self,
        occurrence_index: usize,
    ) -> Option<Range<usize>> {
        let mut clean_offset = 0usize;
        for fragment in &self.record.title.fragments {
            let len = fragment.text.len();
            if fragment
                .footnote
                .as_ref()
                .is_some_and(|footnote| footnote.occurrence_index == occurrence_index)
            {
                return Some(self.clean_to_current_range(clean_offset..clean_offset + len));
            }
            clean_offset += len;
        }
        None
    }

    pub fn is_empty(&self) -> bool {
        self.display_text().is_empty()
    }

    pub fn is_direct_list_child(&self) -> bool {
        self.parent_is_list_item && !self.kind().is_list_item()
    }

    pub fn is_nested_list_item(&self) -> bool {
        self.parent_is_list_item && self.kind().is_list_item()
    }

    pub fn can_adjust_list_nesting(&self) -> bool {
        (self.kind().is_list_item() || self.parent_is_list_item) && !self.kind().is_code_block()
    }

    pub fn can_outdent_list_nesting(&self) -> bool {
        self.kind().is_list_item() || self.parent_is_list_item
    }

    pub(crate) fn visible_len(&self) -> usize {
        self.current_cache().visible_len()
    }

    pub(crate) fn split_title(&self, offset: usize) -> (InlineTextTree, InlineTextTree) {
        self.record
            .title
            .split_at(self.current_to_clean_offset(offset))
    }
}



mod normalize;
mod text_ops;

#[cfg(test)]
mod tests;

/// P3：shape 备忘的键：文本代数 + 换行宽 + 基准字号 + 字体指纹 + 主题代数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ShapeMemoKey {
    pub generation: u64,
    /// 高亮代数：markdown 源码分块的接缝状态级联更新不改块文本，靠它把
    /// 旧配色的 shape 备忘作废。
    pub highlight_generation: u64,
    pub wrap_width: Option<u32>,
    pub wrap_prose: bool,
    pub space_prose: bool,
    pub font_size: u32,
    pub font_fingerprint: u64,
    pub theme_fingerprint: u64,
    /// 长行折叠的展开代数：展开/收起改变超长行的换行结果。
    pub long_line_wrap_generation: u64,
}

/// 长行折叠计划：源行字节范围（与 `hard_line_ranges` 对齐，layout 行按下标
/// 一一对应）+ 超长行下标（升序）。shape 按行切分时用。
#[derive(Debug)]
pub(crate) struct LongLinePlan {
    pub ranges: Vec<std::ops::Range<usize>>,
    pub long_lines: Vec<usize>,
}


mod cursor;
mod display_cache;

impl LongLinePlan {
    pub fn is_long(&self, line_idx: usize) -> bool {
        self.long_lines.binary_search(&line_idx).is_ok()
    }
}

/// 表格列宽备忘的键：主题代数 + 字号 + 容器宽 + 表内容本身。
pub(crate) struct ColumnLayoutMemo {
    pub theme_fingerprint: u64,
    pub code_size_bits: u32,
    pub text_size_bits: u32,
    pub width_bits: u32,
    pub table: crate::components::markdown::table::TableData,
    pub layout: crate::components::markdown::table::TableColumnLayout,
}

/// P3：shape 备忘的值：整块已换行行布局（Arc 共享，跨帧零拷贝复用）。
#[derive(Clone)]
pub(crate) struct ShapeMemoEntry {
    pub key: ShapeMemoKey,
    pub lines: std::sync::Arc<Vec<gpui::WrappedLine>>,
}


