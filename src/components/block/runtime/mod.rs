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
use super::{CodeHighlightResult, highlight_code_block};
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
#[derive(Clone, Copy)]
pub(crate) enum InlineFormat {
    /// Toggle bold formatting.
    Bold,
    /// Toggle italic formatting.
    Italic,
    /// Toggle underline formatting.
    Underline,
    /// Toggle inline code formatting.
    Code,
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
        if kind.is_code_block() {
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

    pub(crate) fn set_source_raw_mode(&mut self) {
        self.clear_inline_projection();
        self.edit_mode = EditMode::SourceRaw;
        self.show_source_line_numbers = false;
    }

    pub(crate) fn set_source_document_mode(&mut self) {
        self.set_source_raw_mode();
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

    fn clear_vertical_motion(&mut self) {
        self.vertical_motion_x = None;
    }

    pub(crate) fn sync_render_cache(&mut self) {
        let clean_selected = self.current_to_clean_range(self.selected_range.clone());
        let clean_marked = self
            .marked_range
            .clone()
            .map(|range| self.current_to_clean_range(range));
        let (clean_anchor, clean_focus) = self.clean_selection_anchor_focus();
        let (anchor_affinity, focus_affinity) = self.selection_endpoint_affinities();
        let collapsed_affinity = self.current_collapsed_caret_affinity();
        let keep_projection =
            self.projection.is_some() && self.edit_mode.supports_inline_projection();
        self.render_cache = self.record.title.render_cache();
        self.sync_code_highlight();
        self.sync_image_runtime();
        self.projection = None;
        self.projection_cache_key = None;
        if keep_projection {
            self.rebuild_inline_projection(clean_selected.clone(), clean_marked.clone());
            if clean_selected.is_empty() {
                let affinity =
                    self.caret_affinity_for_clean_offset(clean_selected.start, collapsed_affinity);
                let offset = self
                    .clean_to_current_cursor_offset_with_affinity(clean_selected.start, affinity);
                self.assign_collapsed_selection_offset(offset, affinity, None);
            } else {
                self.set_selection_from_clean_anchor_focus(
                    clean_anchor,
                    clean_focus,
                    anchor_affinity,
                    focus_affinity,
                );
                self.collapsed_caret_affinity = CollapsedCaretAffinity::Default;
            }
            self.marked_range = clean_marked.map(|range| self.clean_to_current_range(range));
        } else {
            self.set_selection_from_anchor_focus(clean_anchor, clean_focus);
            self.marked_range = clean_marked;
            self.collapsed_caret_affinity = CollapsedCaretAffinity::Default;
        }
        self.refresh_cached_display_text();
    }

    fn sync_link_reference_definitions(
        &mut self,
        link_reference_definitions: Arc<LinkReferenceDefinitions>,
    ) {
        if self.link_reference_definitions == link_reference_definitions {
            return;
        }

        let selected_markdown = (!self.uses_raw_text_editing())
            .then(|| self.current_range_to_markdown_range(self.selected_range.clone()));
        let marked_markdown = (!self.uses_raw_text_editing())
            .then(|| {
                self.marked_range
                    .clone()
                    .map(|range| self.current_range_to_markdown_range(range))
            })
            .flatten();
        let selection_reversed = self.selection_reversed;
        let collapsed_affinity = self.current_collapsed_caret_affinity();
        let had_projection = self.projection.is_some();

        self.link_reference_definitions = link_reference_definitions;
        if self.uses_raw_text_editing() {
            return;
        }

        let markdown = self.record.title.serialize_markdown();
        let next_title = InlineTextTree::from_markdown_with_link_references(
            &markdown,
            &self.link_reference_definitions,
        );
        if self.record.title == next_title {
            return;
        }

        self.record.set_title(next_title);
        self.sync_edit_mode_from_kind();
        self.sync_render_cache();

        if let Some(selected_markdown) = selected_markdown {
            let restored = self.markdown_range_to_current_range(selected_markdown);
            if restored.is_empty() {
                self.assign_collapsed_selection_offset(
                    restored.start,
                    collapsed_affinity,
                    self.vertical_motion_x,
                );
            } else {
                self.selected_range = restored;
                self.selection_reversed = selection_reversed;
                self.collapsed_caret_affinity = CollapsedCaretAffinity::Default;
            }
        }

        self.marked_range =
            marked_markdown.map(|range| self.markdown_range_to_current_range(range));

        if had_projection {
            self.sync_inline_projection_for_focus(true);
        }
    }

    fn sync_footnote_registry(&mut self, footnote_registry: Arc<FootnoteRegistry>) {
        if self.footnote_registry == footnote_registry {
            return;
        }

        let selected_markdown = (!self.uses_raw_text_editing())
            .then(|| self.current_range_to_markdown_range(self.selected_range.clone()));
        let marked_markdown = (!self.uses_raw_text_editing())
            .then(|| {
                self.marked_range
                    .clone()
                    .map(|range| self.current_range_to_markdown_range(range))
            })
            .flatten();
        let selection_reversed = self.selection_reversed;
        let collapsed_affinity = self.current_collapsed_caret_affinity();
        let had_projection = self.projection.is_some();

        self.footnote_registry = footnote_registry;
        if self.uses_raw_text_editing() || !self.record.title.has_footnote_references() {
            return;
        }

        let mut next_title = self.record.title.clone();
        let mut occurrence_iter = self
            .footnote_registry
            .occurrences_for_block(self.record.id)
            .unwrap_or(&[])
            .iter();
        next_title.apply_footnote_reference_state(|id| {
            let occurrence = occurrence_iter.next()?;
            if occurrence.id != id {
                return None;
            }
            Some((occurrence.ordinal?, occurrence.occurrence_index))
        });
        if self.record.title == next_title {
            return;
        }

        self.record.set_title(next_title);
        self.sync_edit_mode_from_kind();
        self.sync_render_cache();

        if let Some(selected_markdown) = selected_markdown {
            let restored = self.markdown_range_to_current_range(selected_markdown);
            if restored.is_empty() {
                self.assign_collapsed_selection_offset(
                    restored.start,
                    collapsed_affinity,
                    self.vertical_motion_x,
                );
            } else {
                self.selected_range = restored;
                self.selection_reversed = selection_reversed;
                self.collapsed_caret_affinity = CollapsedCaretAffinity::Default;
            }
        }

        self.marked_range =
            marked_markdown.map(|range| self.markdown_range_to_current_range(range));

        if had_projection {
            self.sync_inline_projection_for_focus(true);
        }
    }

    fn should_use_markdown_space_link_edit(&self) -> bool {
        !self.uses_raw_text_editing() && self.record.title.has_source_preserving_links()
    }

    fn apply_markdown_space_title_edit(
        &mut self,
        visible_range: Range<usize>,
        new_text: &str,
        selected_range_relative: Option<Range<usize>>,
        mark_inserted_text: bool,
        cx: &mut Context<Self>,
    ) {
        let old_visible_len = self.record.title.visible_text().len();
        let markdown_range = self.current_range_to_markdown_range(visible_range.clone());
        let mut markdown = self.record.title.serialize_markdown();
        let replaced_text = markdown[markdown_range.clone()].to_string();
        // 键入的文本按可见字符拼进 markdown，反斜杠要再转义一次：否则它会与
        // `serialize_markdown` 重新转义出来的旧反斜杠叠加，每按一次数量翻倍
        // （用户报修：行首是自动链接的块里按反斜杠，可见文本 1→3→7）。
        let inserted_markdown = escape_markdown_insertion(new_text);
        let inserted_markdown_len = inserted_markdown.len();
        markdown.replace_range(markdown_range.clone(), &inserted_markdown);

        let next_title = InlineTextTree::from_markdown_with_link_references(
            &markdown,
            &self.link_reference_definitions,
        );
        let map = next_title.markdown_offset_map();
        let selected_markdown = selected_range_relative.as_ref().map(|relative| {
            let start =
                markdown_range.start + markdown_insertion_offset(new_text, relative.start);
            let end = markdown_range.start + markdown_insertion_offset(new_text, relative.end);
            start..end
        });
        let cursor_markdown = selected_markdown
            .as_ref()
            .map(|range| range.end)
            .unwrap_or(markdown_range.start + inserted_markdown_len);
        let marked_markdown = if mark_inserted_text && !new_text.is_empty() {
            Some(markdown_range.start..markdown_range.start + inserted_markdown_len)
        } else {
            None
        };
        let selected_clean = selected_markdown
            .as_ref()
            .map(|range| map.markdown_to_visible_range(range.clone()));
        let marked_clean = marked_markdown
            .as_ref()
            .map(|range| map.markdown_to_visible_range(range.clone()));
        let cursor_clean = map.markdown_to_visible_offset(cursor_markdown);

        let quote_structure_edit = self.quote_depth > 0
            && (new_text.contains('\n')
                || replaced_text.contains('\n')
                || (self.kind() == BlockKind::Quote
                    && Self::multiline_quote_edit_requires_reparse(&next_title.visible_text())));
        if quote_structure_edit {
            self.quote_reparse_requested = true;
        }

        // Typing a closing marker (for example the `)` that completes a link)
        // absorbs that markup into a span, so the clean text grows by less than
        // the inserted text. Flag it so the caret is placed just past the new
        // closing delimiter instead of landing inside the span.
        let caret_may_have_closed_span = !new_text.is_empty()
            && !mark_inserted_text
            && next_title.visible_text().len() < old_visible_len + new_text.len();

        self.apply_title_edit(
            next_title,
            cursor_clean,
            marked_clean,
            selected_clean.clone(),
            selected_clean
                .as_ref()
                .and_then(|range| (!range.is_empty()).then_some(false)),
            caret_may_have_closed_span,
            cx,
        );
    }

    pub(crate) fn current_cache(&self) -> &InlineRenderCache {
        self.projection
            .as_ref()
            .map(|projection| &projection.cache)
            .unwrap_or(&self.render_cache)
    }

    pub(crate) fn sync_inline_projection_for_focus(&mut self, focused: bool) {
        let supports_projection = self.edit_mode.supports_inline_projection();
        if !focused || !supports_projection {
            self.clear_inline_projection();
            return;
        }

        let projected_link_selection = self.projection.as_ref().and_then(|projection| {
            projection
                .link_run_fully_covering_range(&self.selected_range)
                .map(|run| ProjectedLinkSelectionSnapshot {
                    clean_range: run.clean_range.clone(),
                    display_relative_range: self
                        .selected_range
                        .start
                        .saturating_sub(run.display_range.start)
                        ..self
                            .selected_range
                            .end
                            .saturating_sub(run.display_range.start),
                    selection_reversed: self.selection_reversed,
                })
        });
        let clean_selected = self.current_to_clean_range(self.selected_range.clone());
        let clean_marked = self
            .marked_range
            .clone()
            .map(|range| self.current_to_clean_range(range));
        if self.projection_cache_key.as_ref()
            == Some(&(
                supports_projection,
                clean_selected.clone(),
                clean_marked.clone(),
            ))
        {
            return;
        }
        let (clean_anchor, clean_focus) = self.clean_selection_anchor_focus();
        let (anchor_affinity, focus_affinity) = self.selection_endpoint_affinities();
        let collapsed_affinity = self.current_collapsed_caret_affinity();
        self.rebuild_inline_projection(clean_selected.clone(), clean_marked.clone());
        if let Some(snapshot) = projected_link_selection
            && let Some(run) = self
                .projection
                .as_ref()
                .and_then(|projection| projection.link_run_for_clean_range(&snapshot.clean_range))
        {
            let start = run.display_range.start
                + snapshot
                    .display_relative_range
                    .start
                    .min(run.display_range.len());
            let end = run.display_range.start
                + snapshot
                    .display_relative_range
                    .end
                    .min(run.display_range.len());
            self.selected_range = start..end;
            self.selection_reversed = snapshot.selection_reversed;
            self.collapsed_caret_affinity = CollapsedCaretAffinity::Default;
        } else if clean_selected.is_empty() {
            let collapsed_affinity =
                self.caret_affinity_for_clean_offset(clean_selected.start, collapsed_affinity);
            let offset = self.clean_to_current_cursor_offset_with_affinity(
                clean_selected.start,
                collapsed_affinity,
            );
            self.assign_collapsed_selection_offset(offset, collapsed_affinity, None);
        } else {
            self.set_selection_from_clean_anchor_focus(
                clean_anchor,
                clean_focus,
                anchor_affinity,
                focus_affinity,
            );
            self.collapsed_caret_affinity = CollapsedCaretAffinity::Default;
        }
        self.marked_range = clean_marked.map(|range| self.clean_to_current_range(range));
    }

    pub(crate) fn clear_inline_projection(&mut self) {
        if self.projection.is_none() {
            self.projection_cache_key = None;
            return;
        }

        let clean_marked = self
            .marked_range
            .clone()
            .map(|range| self.current_to_clean_range(range));
        let (clean_anchor, clean_focus) = self.clean_selection_anchor_focus();
        self.projection = None;
        self.projection_cache_key = None;
        self.set_selection_from_anchor_focus(clean_anchor, clean_focus);
        self.marked_range = clean_marked;
        self.collapsed_caret_affinity = CollapsedCaretAffinity::Default;
        self.refresh_cached_display_text();
    }

    fn rebuild_inline_projection(
        &mut self,
        clean_selected: Range<usize>,
        clean_marked: Option<Range<usize>>,
    ) {
        self.projection_cache_key = Some((
            self.edit_mode.supports_inline_projection(),
            clean_selected.clone(),
            clean_marked.clone(),
        ));
        self.projection = ExpandedInlineProjection::build(
            &self.record.title.fragments,
            clean_selected,
            clean_marked,
        );
        self.refresh_cached_display_text();
    }

    fn projection_segments(&self) -> &[ExpandedInlineSegment] {
        self.projection
            .as_ref()
            .map(|projection| projection.segments.as_slice())
            .unwrap_or(&[])
    }

    fn projected_link_run_fully_covering_range(
        &self,
        range: &Range<usize>,
    ) -> Option<&ExpandedLinkRun> {
        self.projection
            .as_ref()
            .and_then(|projection| projection.link_run_fully_covering_range(range))
    }

    fn collapsed_caret_affinity_for_display_offset(&self, offset: usize) -> CollapsedCaretAffinity {
        self.projection
            .as_ref()
            .map(|projection| projection.collapsed_affinity_for_display_offset(offset))
            .unwrap_or(CollapsedCaretAffinity::Default)
    }

    /// Affinity of the current selection's anchor and focus, used to restore
    /// each endpoint accurately when the projection is rebuilt.
    fn selection_endpoint_affinities(&self) -> (CollapsedCaretAffinity, CollapsedCaretAffinity) {
        let (anchor, focus) = self.selection_anchor_focus();
        (
            self.collapsed_caret_affinity_for_display_offset(anchor),
            self.collapsed_caret_affinity_for_display_offset(focus),
        )
    }

    fn current_collapsed_caret_affinity(&self) -> CollapsedCaretAffinity {
        if !self.selected_range.is_empty() {
            return CollapsedCaretAffinity::Default;
        }

        self.projection
            .as_ref()
            .map(|projection| {
                projection.collapsed_affinity_for_display_offset(self.cursor_offset())
            })
            .unwrap_or(self.collapsed_caret_affinity)
    }

    fn sync_collapsed_caret_affinity(&mut self) {
        self.collapsed_caret_affinity = if self.selected_range.is_empty() {
            self.projection
                .as_ref()
                .map(|projection| {
                    projection.collapsed_affinity_for_display_offset(self.cursor_offset())
                })
                .unwrap_or(CollapsedCaretAffinity::Default)
        } else {
            CollapsedCaretAffinity::Default
        };
    }

    pub(crate) fn assign_collapsed_selection_offset(
        &mut self,
        offset: usize,
        affinity: CollapsedCaretAffinity,
        preferred_x: Option<Pixels>,
    ) {
        let clamped_offset = offset.min(self.visible_len());
        self.selected_range = clamped_offset..clamped_offset;
        self.selection_reversed = false;
        self.vertical_motion_x = preferred_x;
        self.collapsed_caret_affinity = affinity;
        self.sync_collapsed_caret_affinity();
    }

    fn clean_to_current_cursor_offset(&self, clean: usize) -> usize {
        let Some(projection) = &self.projection else {
            return clean;
        };
        projection
            .clean_to_display_cursor
            .get(clean.min(projection.clean_to_display_cursor.len().saturating_sub(1)))
            .copied()
            .unwrap_or(clean)
    }

    fn clean_to_current_cursor_offset_with_affinity(
        &self,
        clean: usize,
        affinity: CollapsedCaretAffinity,
    ) -> usize {
        let Some(projection) = &self.projection else {
            return clean;
        };
        projection
            .display_offset_for_clean_cursor(clean, affinity)
            .unwrap_or_else(|| self.clean_to_current_cursor_offset(clean))
    }

    /// 光标落在块首/块尾时用外侧亲和性。默认映射会把光标放到标记「里面」：块首是
    /// `**`、`<...>`、`[...]()` 这类标记时，打开文件后光标就不在行首了（用户报修），
    /// 行尾同理。块内部的偏移不受影响（在粗体里继续打字仍然留在粗体里）。
    fn caret_affinity_for_clean_offset(
        &self,
        clean: usize,
        fallback: CollapsedCaretAffinity,
    ) -> CollapsedCaretAffinity {
        if clean == 0 {
            CollapsedCaretAffinity::OuterStart
        } else if clean >= self.record.title.visible_text().len() {
            CollapsedCaretAffinity::OuterEnd
        } else {
            fallback
        }
    }

    fn clean_to_current_range_start(&self, clean: usize) -> usize {
        self.clean_to_current_cursor_offset(clean)
    }

    fn clean_to_current_range_end(&self, clean: usize) -> usize {
        self.clean_to_current_cursor_offset(clean)
    }

    pub(crate) fn clean_to_current_range(&self, range: Range<usize>) -> Range<usize> {
        if range.is_empty() {
            let offset = self.clean_to_current_cursor_offset(range.start);
            offset..offset
        } else {
            self.clean_to_current_range_start(range.start)
                ..self.clean_to_current_range_end(range.end)
        }
    }

    pub(crate) fn current_to_clean_range(&self, range: Range<usize>) -> Range<usize> {
        self.current_to_clean_offset(range.start)..self.current_to_clean_offset(range.end)
    }

    pub(crate) fn current_to_clean_offset(&self, offset: usize) -> usize {
        self.unexpand_offset(offset)
    }

    #[allow(dead_code)]
    pub(crate) fn pointer_target_offset(&self, offset: usize) -> usize {
        self.projection
            .as_ref()
            .map(|projection| projection.pointer_target_offset(offset))
            .unwrap_or(offset)
    }

    pub(crate) fn projected_move_left_target(
        &self,
        offset: usize,
    ) -> Option<(usize, CollapsedCaretAffinity)> {
        self.projection
            .as_ref()
            .and_then(|projection| projection.move_left_target(offset))
    }

    pub(crate) fn projected_move_right_target(
        &self,
        offset: usize,
    ) -> Option<(usize, CollapsedCaretAffinity)> {
        self.projection
            .as_ref()
            .and_then(|projection| projection.move_right_target(offset))
    }

    pub(crate) fn selection_clean_range(&self) -> Range<usize> {
        self.current_to_clean_range(self.selected_range.clone())
    }

    pub(crate) fn current_range_to_markdown_range(&self, range: Range<usize>) -> Range<usize> {
        if self.uses_raw_text_editing() || self.kind().is_code_block() {
            return range.start.min(self.visible_len())..range.end.min(self.visible_len());
        }

        // 带标记的块里，行尾/初始光标可能落在可见文本之外（标记占位没换算回来），
        // 先收敛到可见范围，否则映射会落到 `<...>` 内部这样的地方。
        let visible_len = self.visible_len();
        let range = range.start.min(visible_len)..range.end.min(visible_len);
        // 自动链接的「标签」就是 URL，不能按可编辑标签映射（那会把插入点放进 `<...>`
        // 里），交给下面的边界处理。
        if let Some(link_run) = self
            .projected_link_run_fully_covering_range(&range)
            .filter(|run| !matches!(run.link, InlineLink::Autolink { .. }))
        {
            let map = self.record.title.markdown_offset_map();
            let label_markdown_start = map.visible_to_markdown_offset(link_run.clean_range.start);
            let run_markdown_start =
                label_markdown_start.saturating_sub(link_run.link.open_marker().len());
            let start = run_markdown_start
                + range
                    .start
                    .saturating_sub(link_run.display_range.start)
                    .min(link_run.display_range.len());
            let end = run_markdown_start
                + range
                    .end
                    .saturating_sub(link_run.display_range.start)
                    .min(link_run.display_range.len());
            return start..end;
        }

        if let Some(footnote_run) = self
            .projection
            .as_ref()
            .and_then(|projection| projection.footnote_run_fully_covering_range(&range))
        {
            let raw = footnote_run.footnote.raw_markdown();
            let raw_len = raw.len();
            let local_start = range
                .start
                .saturating_sub(footnote_run.display_range.start)
                .min(footnote_run.display_range.len());
            let local_end = range
                .end
                .saturating_sub(footnote_run.display_range.start)
                .min(footnote_run.display_range.len());
            let mapped_start = (raw_len * local_start) / footnote_run.display_range.len().max(1);
            let mapped_end = (raw_len * local_end) / footnote_run.display_range.len().max(1);
            let map = self.record.title.markdown_offset_map();
            let run_markdown_start = map.visible_to_markdown_offset(footnote_run.clean_range.start);
            return run_markdown_start + mapped_start..run_markdown_start + mapped_end;
        }

        let clean_range = self.current_to_clean_range(range.clone());
        if let Some(mapped) = self.autolink_boundary_markdown_range(&clean_range) {
            return mapped;
        }
        self.record
            .title
            .markdown_offset_map()
            .visible_to_markdown_range(clean_range)
    }

    /// 光标贴在自动链接的可见文本边缘时，把插入点映射到 `<`/`>` 之外。
    /// 自动链接的「标签」就是 URL 本身，插到里面会把链接写坏，转义字符也会直接落进
    /// 显示文本（用户报修：行首自动链接前按反斜杠，可见数量翻倍）。
    fn autolink_boundary_markdown_range(&self, clean_range: &Range<usize>) -> Option<Range<usize>> {
        if clean_range.start != clean_range.end {
            return None;
        }
        let map = self.record.title.markdown_offset_map();
        let mut visible_start = 0;
        for fragment in &self.record.title.fragments {
            let visible_end = visible_start + fragment.text.len();
            if let Some(link @ InlineLink::Autolink { .. }) = fragment.link.as_ref() {
                if clean_range.start == visible_start {
                    let label_start = map.visible_to_markdown_offset(visible_start);
                    let offset = label_start.saturating_sub(link.open_marker().len());
                    return Some(offset..offset);
                }
                if clean_range.start == visible_end {
                    let label_end = map.visible_to_markdown_offset(visible_end);
                    let offset = label_end + link.close_marker().len();
                    return Some(offset..offset);
                }
            }
            visible_start = visible_end;
        }
        None
    }

    pub(crate) fn markdown_range_to_current_range(&self, range: Range<usize>) -> Range<usize> {
        if self.uses_raw_text_editing() || self.kind().is_code_block() {
            let len = self.visible_len();
            return range.start.min(len)..range.end.min(len);
        }

        let clean_range = self
            .record
            .title
            .markdown_offset_map()
            .markdown_to_visible_range(range);
        self.clean_to_current_range(clean_range)
    }

    pub(crate) fn markdown_offset_to_current_offset(&self, offset: usize) -> usize {
        self.markdown_range_to_current_range(offset..offset).start
    }

    pub(crate) fn prepare_undo_capture(&self, kind: UndoCaptureKind, cx: &mut Context<Self>) {
        cx.emit(BlockEvent::PrepareUndo { kind });
    }

    pub(super) fn utf16_to_utf8_in(text: &str, offset: usize) -> usize {
        let mut utf8_offset = 0;
        let mut utf16_count = 0;

        for ch in text.chars() {
            if utf16_count >= offset {
                break;
            }
            utf16_count += ch.len_utf16();
            utf8_offset += ch.len_utf8();
        }

        utf8_offset
    }

    pub(super) fn utf8_to_utf16_in(text: &str, offset: usize) -> usize {
        let mut utf16_offset = 0;
        let mut utf8_count = 0;

        for ch in text.chars() {
            if utf8_count >= offset {
                break;
            }
            utf8_count += ch.len_utf8();
            utf16_offset += ch.len_utf16();
        }

        utf16_offset
    }

    pub(super) fn utf16_range_to_utf8_in(text: &str, range_utf16: &Range<usize>) -> Range<usize> {
        Self::utf16_to_utf8_in(text, range_utf16.start)
            ..Self::utf16_to_utf8_in(text, range_utf16.end)
    }

    pub(super) fn utf8_range_to_utf16_in(text: &str, range: &Range<usize>) -> Range<usize> {
        Self::utf8_to_utf16_in(text, range.start)..Self::utf8_to_utf16_in(text, range.end)
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
