//! Top-level editor controller and window state.
//!
//! [`Editor`] owns window-level concerns such as view mode, save/close flow,
//! scroll state, and focus deferral. The runtime block tree itself lives in
//! [`DocumentTree`], which centralizes structural mutations and cached visible
//! order metadata.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::*;

use self::context_menu::{ContextMenuState, TableInsertDialogState};
use self::document::ChunkCursor;
use self::tree::{DocumentTree, PendingSourceTail, PendingTail};
use crate::components::{
    Block, BlockKind, BlockRecord, FootnoteDefinitionBinding, FootnoteReferenceLocation,
    FootnoteRegistry, FootnoteResolvedOccurrence, ImageReferenceDefinitions, InlineTextTree,
    LinkReferenceDefinitions, parse_image_reference_definitions, parse_link_reference_definitions,
};
use crate::components::{
    TableAxisHighlight, TableAxisKind, TableAxisMarker, TableCellPosition, TableColumnAlignment,
    TableData, TableRuntime, UndoCaptureKind, serialize_table_cell_markdown,
};
mod close;
mod context_menu;
mod modal;
mod document;
mod events;
mod export;
mod file_drop;
mod history;
mod persistence;
mod render;
mod runtime_context;
mod selection;
mod source_mapping;
mod status_bar;
mod table_edit;
#[cfg(test)]
mod tests;
mod tree;
mod update;
mod window_state;
mod command_palette;
mod quick_open;
mod watcher;
mod workspace;

use self::status_bar::StatusBarState;
use self::workspace::WorkspaceState;

/// Top-level controller that owns editor-wide state and delegates tree
/// mutations to [`DocumentTree`].
///
/// The editor subscribes to every [`BlockEvent`](crate::components::BlockEvent)
/// emitted by child blocks. Structural changes are handled centrally so focus,
/// scrolling, dirty tracking, and serialization stay synchronized.
pub struct Editor {
    document: DocumentTree,
    table_cells: HashMap<EntityId, TableCellBinding>,
    /// Which view the editor is currently presenting.
    pub(crate) view_mode: ViewMode,
    focus_mode: bool,
    typewriter_mode: bool,
    /// Keeps ambiguous Markdown extensions in source mode until their syntax is removed.
    source_mode_fallback_required: bool,
    code_document: bool,
    code_uses_crlf: bool,
    /// Deferred focus target applied during render when a [`Window`] is
    /// available.
    pending_focus: Option<EntityId>,
    active_entity_id: Option<EntityId>,
    pending_scroll_active_block_into_view: bool,
    /// Jump-style scroll (outline/search): land the target at the viewport
    /// center instead of the minimal scroll-into-view adjustment.
    pending_scroll_center_into_view: bool,
    pending_scroll_recheck_after_layout: bool,
    pending_save: bool,
    pending_save_as: bool,
    pending_window_edited: bool,
    pending_window_unedited: bool,
    pending_window_title_refresh: bool,
    document_dirty: bool,
    document_revision: u64,
    /// 长块护栏提示缓存：(文档修订, 是否含超长块)（roadmap B12）。
    long_source_block_hint: Option<(u64, bool)>,
    /// 文件树「复制」暂存的源文件路径（roadmap D6）。
    pub(crate) tree_clipboard: Option<std::path::PathBuf>,
    autosave_task: Option<Task<()>>,
    /// Background task importing the rest of a document that was opened with a
    /// partial block tree (roadmap G8).
    pending_materialization_task: Option<Task<()>>,
    recovery_id: uuid::Uuid,
    recovery_source_path: Option<PathBuf>,
    is_recovered_document: bool,
    file_version: Option<u64>,
    window_handle: Option<AnyWindowHandle>,
    system_appearance_subscription: Option<Subscription>,
    file_path: Option<PathBuf>,
    scroll_handle: ScrollHandle,
    last_scroll_viewport_size: Option<Size<Pixels>>,
    /// Last frame's visible block ids, to detect structural edits so the height
    /// cache is refreshed only when the row/block mapping is unchanged.
    prev_visible_block_ids: Vec<EntityId>,
    /// Per-row footprint (height plus trailing gap), keyed by the row's first
    /// block. Scroll-invariant, unlike raw painted positions, so windowing from
    /// their running sum stays correct as the document scrolls. Filled as rows
    /// paint; unknown rows use a minimum-height estimate.
    row_stride_cache: HashMap<EntityId, f32>,
    /// 状态栏整篇字数缓存（P4a）：键 document_revision。渲染每帧读取，
    /// 全文扫描只允许在每次修订后发生一次。
    word_count_cache: std::cell::Cell<Option<(u64, usize)>>,
    /// 状态栏代码文档行数缓存（P4a）：键 document_revision，避免每帧
    /// 重新序列化整篇文档只为数行数。
    code_line_count_cache: std::cell::Cell<Option<(u64, usize)>>,
    /// 行结构计划缓存（P4b）：键 (document_revision, fold_state_version,
    /// 渲染模式)，见 render::RenderedRowPlan。
    rendered_row_plan: Option<std::sync::Arc<render::RenderedRowPlan>>,
    /// 折叠状态版本：任何 folded 变更都递增，使行计划重建。
    fold_state_version: u64,
    /// P7：文档打开代数——replace_document_content 每次递增，
    /// 用于后台 file_version 哈希写回时的竞态校验。
    open_generation: u64,
    /// TOC 条目版本：大纲重建后递增（[TOC] 块的条目同步随行计划进行）。
    toc_state_version: u64,
    /// Content column the cached footprints were measured at. Rows rewrap when it
    /// changes, so entries from another width are discarded rather than reused.
    row_stride_width: Option<f32>,
    /// Where last frame's run sat among the scroll container's children.
    prev_mounted_run: Option<MountedRun>,
    /// 冷启动续挂已排的帧数（上限 COLD_FILL_MAX_FRAMES，避免每帧重排）。
    cold_fill_frames: u8,
    close_guard_installed: bool,
    show_unsaved_changes_dialog: bool,
    /// When true, the window will close after the next successful save.
    pending_close_after_save: bool,
    /// Focus target to restore when the close dialog is dismissed.
    close_dialog_restore_focus: Option<EntityId>,
    pending_drop_replace_path: Option<PathBuf>,
    show_drop_replace_dialog: bool,
    pending_drop_replace_after_save: bool,
    drop_replace_restore_focus: Option<EntityId>,
    /// 应用内模态（取代系统原生 window.prompt，用户要求全软件不用原生弹窗）。
    modal: Option<modal::EditorModal>,
    /// 切换工作区后，下一帧（拿得到 `&mut Window` 时）要打开的标签页。
    pending_workspace_tab_activation: Option<PathBuf>,
    /// Optional informational dialog shown from the Help menu.
    info_dialog: Option<InfoDialogKind>,
    /// Set while the active tab is a file the editor can't preview; the
    /// content area renders a centered placeholder instead of blocks.
    pub(super) unsupported_preview_path: Option<PathBuf>,
    /// Extra explanation line for the unsupported-preview placeholder.
    pub(super) unsupported_preview_detail: Option<String>,
    /// Folder picked through 文件 → 打开文件 that is waiting for the user to
    /// choose between replacing this window's working set and a new window.
    pub(super) pending_folder_choice: Option<PathBuf>,
    /// Window opened with no document (fresh workspace, empty startup): the
    /// content area shows the welcome page instead of an empty editor.
    pub(super) show_welcome: bool,
    /// Blocks currently carrying in-document search highlights (roadmap B2);
    /// tracked so the next sync can clear them cheaply.
    pub(super) search_highlighted_blocks: Vec<Entity<Block>>,
    /// Quick file switcher overlay (⌘P); `None` while closed.
    quick_open: Option<quick_open::QuickOpenState>,
    /// Command palette overlay (⇧⌘P); `None` while closed.
    command_palette: Option<command_palette::CommandPaletteState>,
    /// Workspace change watcher (roadmap D3); `None` until a root is set.
    external_watcher: Option<notify::RecommendedWatcher>,
    /// Cached block→source-range map + newline offsets for outline-follow
    /// scroll (roadmap C5), keyed by document revision.
    pub(super) outline_follow_cache:
        Option<(u64, std::collections::HashMap<EntityId, std::ops::Range<usize>>, Vec<usize>)>,
    /// Scroll offset at the last outline-follow update.
    pub(super) last_outline_follow_offset: f32,
    /// Cursor position history (roadmap E6): jump points to return to.
    pub(super) cursor_history_back: Vec<CursorLocation>,
    pub(super) cursor_history_forward: Vec<CursorLocation>,
    /// True while an online update check is running in the background.
    update_check_in_progress: bool,
    workspace: WorkspaceState,
    status_bar: StatusBarState,
    context_menu: Option<ContextMenuState>,
    table_insert_dialog: Option<TableInsertDialogState>,
    context_menu_submenu_close_task: Option<Task<()>>,
    table_axis_preview: Option<TableAxisSelection>,
    table_axis_selection: Option<TableAxisSelection>,
    cross_block_selection: Option<CrossBlockSelection>,
    cross_block_drag: Option<CrossBlockDrag>,
    rendered_select_all_cycle: Option<RenderedSelectAllCycle>,
    /// Open top-level menu in the in-window fallback menu bar.
    menu_bar_open: Option<usize>,
    /// Open child submenu inside the in-window fallback menu panel.
    menu_submenu_open: Option<usize>,
    menu_bar_hovered: bool,
    menu_panel_hovered: bool,
    menu_submenu_panel_hovered: bool,
    /// Hover state for the invisible bridge spanning the gap between the menu
    /// panel and an open submenu. Tracked separately from
    /// `menu_submenu_panel_hovered` so the handoff between the two regions
    /// cannot clobber a single shared flag and tear the menu down.
    menu_submenu_bridge_hovered: bool,
    menu_close_task: Option<Task<()>>,
    scrollbar_hovered: bool,
    scrollbar_visible_until: Instant,
    scrollbar_fade_task: Option<Task<()>>,
    /// Forces a repaint shortly after a pending scroll-into-view that could
    /// not be satisfied yet (the target block has no measured bounds), so the
    /// scroll lands on the next frame instead of waiting for the cursor blink.
    scroll_recheck_task: Option<Task<()>>,
    scrollbar_drag: Option<ScrollbarDragSession>,
    undo_history: Vec<HistoryEntry>,
    redo_history: Vec<HistoryEntry>,
    pending_undo_capture: Option<PendingUndoCapture>,
    last_selection_snapshot: UndoSelectionSnapshot,
    last_stable_source_text: String,
    history_restore_in_progress: bool,
    image_reference_definitions: Arc<ImageReferenceDefinitions>,
    link_reference_definitions: Arc<LinkReferenceDefinitions>,
    footnote_registry: Arc<FootnoteRegistry>,
    runtime_context_sensitive_blocks: HashSet<EntityId>,
}

/// Runtime binding between a table block and one cell editor.
#[derive(Clone)]
struct TableCellBinding {
    table_block: Entity<Block>,
    cell: Entity<Block>,
    position: TableCellPosition,
}

/// Selected row or column in a rendered native table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TableAxisSelection {
    table_block_id: EntityId,
    kind: TableAxisKind,
    index: usize,
}

/// Pixel geometry for the custom editor scrollbar.
#[derive(Clone, Copy, Debug, PartialEq)]
struct ScrollbarGeometry {
    track_height: f32,
    thumb_height: f32,
    thumb_top: f32,
    max_scroll_y: f32,
}

/// Windowing result: the run of rows to mount, plus the spacer heights standing
/// in for the culled rows. `top_h` is the spacer directly above the run and
/// `bottom_h` the one closing out the document.
#[derive(Clone, Copy, Debug, PartialEq)]
struct RenderWindow {
    run_start: usize,
    run_end: usize,
    top_h: f32,
    bottom_h: f32,
    focus_island: Option<FocusIsland>,
    /// 行高仍是估计值时，这个 run 受冷启动上限截断、还没铺到视口底部。
    /// 渲染期据此再排一帧续挂（否则整屏 spacer 要等下一次输入才补上）。
    needs_fill: bool,
}

/// Where a frame's mounted run sat among the scroll container's children, so the
/// next frame can read its recorded bounds back by index. `child_count` is what
/// makes that mapping checkable rather than assumed.
#[derive(Clone, Copy, Debug, PartialEq)]
struct MountedRun {
    row_start: usize,
    row_end: usize,
    child_base: usize,
    child_count: usize,
}

/// Focused row mounted on its own, away from the run. Its position relative to
/// the run follows from `row`, and `lead_h` is the spacer directly above it.
#[derive(Clone, Copy, Debug, PartialEq)]
struct FocusIsland {
    row: usize,
    lead_h: f32,
}

/// Active drag session for the custom scrollbar thumb.
#[derive(Clone, Copy, Debug, PartialEq)]
struct ScrollbarDragSession {
    pointer_offset_y: f32,
    track_height: f32,
    thumb_height: f32,
    max_scroll_y: f32,
}

/// Source-mode selection snapshot stored with undo history.
#[derive(Clone, Debug, PartialEq, Eq)]
struct UndoSelectionSnapshot {
    range: std::ops::Range<usize>,
    reversed: bool,
}

/// One undo history entry containing source text and selection state.
#[derive(Clone, Debug)]
struct HistoryEntry {
    source_text: String,
    selection: UndoSelectionSnapshot,
    timestamp: Instant,
    kind: UndoCaptureKind,
}

/// Deferred undo capture used to coalesce adjacent typing edits.
#[derive(Clone, Debug)]
struct PendingUndoCapture {
    snapshot: HistoryEntry,
}

/// Cross-block selection endpoint in visible block order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct CrossBlockSelectionEndpoint {
    pub(super) entity_id: EntityId,
    pub(super) offset: usize,
}

/// Editor-level selection spanning two visible block endpoints.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct CrossBlockSelection {
    pub(super) anchor: CrossBlockSelectionEndpoint,
    pub(super) focus: CrossBlockSelectionEndpoint,
}

/// Drag state while creating or extending a cross-block selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct CrossBlockDrag {
    pub(super) anchor: CrossBlockSelectionEndpoint,
}

/// Short-lived Ctrl/Cmd+A press counter for rendered-mode selection upgrade.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct RenderedSelectAllCycle {
    entity_id: EntityId,
    count: u8,
    last_pressed_at: Instant,
}

/// Mapping from one visible block's text range to canonical Markdown offsets.
/// A remembered caret location for cursor-history navigation (roadmap E6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct CursorLocation {
    pub(super) path: Option<PathBuf>,
    pub(super) range: std::ops::Range<usize>,
}

const CURSOR_HISTORY_LIMIT: usize = 100;

pub(super) struct SourceTargetMapping {
    entity: Entity<Block>,
    full_source_range: std::ops::Range<usize>,
    content_to_source: Vec<usize>,
    source_to_content: Vec<usize>,
}

/// Active image corner-resize drag session state (roadmap C10).
#[derive(Clone, Copy, Debug)]
pub(crate) struct ImageResizeDrag {
    pub(crate) start_x: f32,
    pub(crate) base_factor: f32,
}

/// The two editing views the editor can present.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    /// Rich rendered view where each block is styled by its semantic kind.
    Rendered,
    /// Plain source view where the full Markdown document is edited as a
    /// single raw buffer.
    Source,
}

/// The informational dialogs that can be shown from the Help menu.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum InfoDialogKind {
    /// Dialog describing update-check availability.
    CheckForUpdates,
    /// Dialog with app name and version information.
    About,
}

impl Editor {
    const HISTORY_LIMIT: usize = 200;
    const HISTORY_COALESCE_WINDOW: Duration = Duration::from_millis(1_000);
    const RENDERED_SELECT_ALL_CYCLE_WINDOW: Duration = Duration::from_millis(750);
    /// Root blocks imported before the first frame is shown. Documents at or
    /// below this size are built in one pass, exactly as before; larger ones
    /// stream the remainder in from a background task (roadmap G8).
    const FIRST_CHUNK_ROOTS: usize = 2_000;
    /// Root blocks imported per streaming step while the rest of a huge
    /// document arrives.
    const STEADY_CHUNK_ROOTS: usize = 250;
    /// 代码/纯文本文档每个流式步骤 materialize 的分块数（每块
    /// `SOURCE_DOCUMENT_CHUNK_LINES` 行）。
    const CODE_CHUNKS_PER_STEP: usize = 4;

    pub fn from_markdown(
        cx: &mut Context<Self>,
        markdown: String,
        file_path: Option<PathBuf>,
    ) -> Self {
        Self::from_markdown_with_chunk_budget(cx, markdown, file_path, Self::FIRST_CHUNK_ROOTS)
    }

    /// Builds an editor that imports at most `first_chunk_roots` root blocks up
    /// front and streams the rest in (roadmap G8).
    ///
    /// Tests pass a tiny budget to exercise many chunk boundaries on a small
    /// document; `usize::MAX` builds everything synchronously.
    pub(crate) fn from_markdown_with_chunk_budget(
        cx: &mut Context<Self>,
        markdown: String,
        file_path: Option<PathBuf>,
        first_chunk_roots: usize,
    ) -> Self {
        let normalized = markdown.replace("\r\n", "\n").replace('\r', "\n");
        let source_mode_fallback_required =
            Self::markdown_requires_source_mode_fallback(&normalized);
        let mut pending_tail = None;
        let mut roots = if source_mode_fallback_required {
            let block = Self::new_block(cx, BlockRecord::paragraph(normalized.clone()));
            block.update(cx, |block, _cx| block.set_source_document_mode());
            vec![block]
        } else {
            let lines = Arc::new(Self::split_markdown_lines(&normalized));
            let (roots, next_line) = Self::build_root_block_chunk(
                cx,
                &lines,
                ChunkCursor {
                    root_budget: first_chunk_roots.max(1),
                    is_document_start: true,
                    previous_root_is_list_item: false,
                },
            );
            if next_line < lines.len() {
                pending_tail = Some(PendingTail {
                    previous_root_is_list_item: roots
                        .last()
                        .map(|block| block.read(cx).kind().is_list_item())
                        .unwrap_or(false),
                    next_line,
                    lines,
                });
            }
            roots
        };
        if roots.is_empty() {
            roots.push(Self::new_block(cx, BlockRecord::paragraph(String::new())));
        }

        let mut document = DocumentTree::new(roots);
        document.set_pending_tail(pending_tail);
        document.rebuild_metadata_and_snapshot(cx);
        let pending_focus = document.first_root().map(|block| block.entity_id());

        let mut editor = Self {
            document,
            table_cells: HashMap::new(),
            view_mode: if source_mode_fallback_required {
                ViewMode::Source
            } else {
                ViewMode::Rendered
            },
            focus_mode: false,
            typewriter_mode: false,
            source_mode_fallback_required,
            code_document: false,
            code_uses_crlf: false,
            pending_focus,
            active_entity_id: pending_focus,
            pending_scroll_active_block_into_view: true,
            pending_scroll_center_into_view: false,
            pending_scroll_recheck_after_layout: true,
            pending_save: false,
            pending_save_as: false,
            pending_window_edited: false,
            pending_window_unedited: false,
            // A fresh window pushes its title on the first frame, so a window
            // created while the display is locked still gets the right title as
            // soon as frames resume (roadmap A7).
            pending_window_title_refresh: true,
            document_dirty: false,
            document_revision: 0,
            long_source_block_hint: None,
            tree_clipboard: None,
            autosave_task: None,
            pending_materialization_task: None,
            recovery_id: uuid::Uuid::new_v4(),
            recovery_source_path: None,
            is_recovered_document: false,
            file_version: file_path
                .as_ref()
                .map(|_| persistence::file_content_version(&normalized)),
            window_handle: None,
            system_appearance_subscription: None,
            file_path,
            scroll_handle: ScrollHandle::new(),
            last_scroll_viewport_size: None,
            prev_visible_block_ids: Vec::new(),
            row_stride_cache: HashMap::new(),
            word_count_cache: std::cell::Cell::default(),
            code_line_count_cache: std::cell::Cell::default(),
            rendered_row_plan: None,
            fold_state_version: 0,
            open_generation: 0,
            toc_state_version: 0,
            row_stride_width: None,
            prev_mounted_run: None,
            cold_fill_frames: 0,
            close_guard_installed: false,
            show_unsaved_changes_dialog: false,
            pending_close_after_save: false,
            close_dialog_restore_focus: None,
            pending_drop_replace_path: None,
            show_drop_replace_dialog: false,
            pending_drop_replace_after_save: false,
            drop_replace_restore_focus: None,
            modal: None,
            pending_workspace_tab_activation: None,
            info_dialog: None,
            unsupported_preview_path: None,
            unsupported_preview_detail: None,
            pending_folder_choice: None,
            show_welcome: false,
            search_highlighted_blocks: Vec::new(),
            quick_open: None,
            command_palette: None,
            external_watcher: None,
            outline_follow_cache: None,
            last_outline_follow_offset: f32::NAN,
            cursor_history_back: Vec::new(),
            cursor_history_forward: Vec::new(),
            update_check_in_progress: false,
            workspace: WorkspaceState::default(),
            status_bar: StatusBarState::default(),
            context_menu: None,
            table_insert_dialog: None,
            context_menu_submenu_close_task: None,
            table_axis_preview: None,
            table_axis_selection: None,
            cross_block_selection: None,
            cross_block_drag: None,
            rendered_select_all_cycle: None,
            menu_bar_open: None,
            menu_submenu_open: None,
            menu_bar_hovered: false,
            menu_panel_hovered: false,
            menu_submenu_panel_hovered: false,
            menu_submenu_bridge_hovered: false,
            menu_close_task: None,
            scrollbar_hovered: false,
            scrollbar_visible_until: Instant::now(),
            scrollbar_fade_task: None,
            scroll_recheck_task: None,
            scrollbar_drag: None,
            undo_history: Vec::new(),
            redo_history: Vec::new(),
            pending_undo_capture: None,
            last_selection_snapshot: Self::empty_selection_snapshot(),
            last_stable_source_text: normalized,
            history_restore_in_progress: false,
            image_reference_definitions: Arc::default(),
            link_reference_definitions: Arc::default(),
            footnote_registry: Arc::default(),
            runtime_context_sensitive_blocks: HashSet::new(),
        };
        editor.rebuild_table_runtimes(cx); // Also refreshes image and reference contexts.
        editor.pending_focus = editor.first_focusable_entity_id(cx);
        editor.active_entity_id = editor.pending_focus;
        editor.start_pending_materialization_task(cx);
        // The loaded source and initial cursor are already the first undo baseline.
        editor
    }

    /// Streams the rest of a partially imported document into the tree.
    ///
    /// Each step builds one chunk on the main thread and hands control back
    /// between steps, so frames, scrolling and typing keep running while a huge
    /// document finishes loading (roadmap G8).
    fn start_pending_materialization_task(&mut self, cx: &mut Context<Self>) {
        if self.document.pending_tail().is_none()
            && self.document.pending_source().is_none()
            || self.pending_materialization_task.is_some()
        {
            return;
        }

        self.pending_materialization_task = Some(cx.spawn(
            async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
                loop {
                    let Ok(more) = this
                        .update(cx, |editor, cx| editor.materialize_next_pending_chunk(cx))
                    else {
                        return;
                    };
                    if !more {
                        break;
                    }
                }
                let _ = this.update(cx, |editor, _cx| editor.pending_materialization_task = None);
            },
        ));
    }

    /// Imports one more chunk of the pending tail. Returns whether work remains.
    fn materialize_next_pending_chunk(&mut self, cx: &mut Context<Self>) -> bool {
        // 代码/纯文本文档：原始字节尾部按行切块续建（P6a）。
        if self.code_document {
            if self.document.pending_source().is_some() {
                return self.materialize_next_pending_source_chunk(cx);
            }
            return false;
        }

        let Some(tail) = self.document.pending_tail().cloned() else {
            return false;
        };

        let (roots, consumed) = Self::build_root_block_chunk(
            cx,
            &tail.lines[tail.next_line..],
            ChunkCursor {
                root_budget: Self::STEADY_CHUNK_ROOTS,
                is_document_start: false,
                previous_root_is_list_item: tail.previous_root_is_list_item,
            },
        );
        debug_assert!(consumed > 0, "a chunk with a non-zero budget always advances");
        let previous_root_is_list_item = roots
            .last()
            .map(|block| block.read(cx).kind().is_list_item())
            .unwrap_or(tail.previous_root_is_list_item);
        let tables = roots
            .iter()
            .filter(|block| block.read(cx).kind() == BlockKind::Table)
            .cloned()
            .collect::<Vec<_>>();
        let next_line = tail.next_line + consumed;
        self.document.append_roots(roots, cx);
        for block in tables {
            let Some(table) = block.read(cx).record.table.clone() else {
                continue;
            };
            self.install_table_runtime_for_block(&block, &table, cx);
        }

        if next_line < tail.lines.len() {
            self.document.set_pending_tail(Some(PendingTail {
                lines: tail.lines,
                next_line,
                previous_root_is_list_item,
            }));
            cx.notify();
            true
        } else {
            self.document.set_pending_tail(None);
            self.finish_pending_materialization(cx);
            false
        }
    }

    /// P6a：代码文档的流式续建步骤——从原始字节尾部按
    /// SOURCE_DOCUMENT_CHUNK_LINES 行粒度切最多 CODE_CHUNKS_PER_STEP 块。
    fn materialize_next_pending_source_chunk(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(mut tail) = self.document.take_pending_source() else {
            return false;
        };
        let kind = self
            .document
            .first_root()
            .map(|block| block.read(cx).kind())
            .unwrap_or(BlockKind::Paragraph);
        let mut roots = Vec::new();
        let mut offset = 0usize;
        let mut line_start = tail.next_line + 1;
        for _ in 0..Self::CODE_CHUNKS_PER_STEP {
            match file_drop::scan_chunk_end(
                tail.source.as_bytes(),
                offset,
                file_drop::SOURCE_DOCUMENT_CHUNK_LINES,
            ) {
                Some(end) => {
                    // 末块保留文件末换行（空行片段），与整开切分语义一致。
                    let is_last = end == tail.source.len();
                    let text = if is_last {
                        &tail.source[offset..]
                    } else {
                        &tail.source[offset..end - 1]
                    };
                    let block =
                        Self::new_block(cx, BlockRecord::with_plain_text(kind.clone(), text));
                    let chunk_line_start = line_start;
                    block.update(cx, |block, _cx| {
                        block.set_source_document_mode();
                        block.set_source_line_start(chunk_line_start);
                    });
                    roots.push(block);
                    line_start += text.split('\n').count();
                    offset = end;
                }
                None => {
                    let text = &tail.source[offset..];
                    if !text.is_empty() {
                        let block = Self::new_block(
                            cx,
                            BlockRecord::with_plain_text(kind.clone(), text),
                        );
                        let chunk_line_start = line_start;
                        block.update(cx, |block, _cx| {
                            block.set_source_document_mode();
                            block.set_source_line_start(chunk_line_start);
                        });
                        roots.push(block);
                        line_start += text.split('\n').count();
                    }
                    offset = tail.source.len();
                    break;
                }
            }
        }
        self.document.append_roots(roots, cx);

        if offset < tail.source.len() {
            let rest = tail.source.split_off(offset);
            self.document.set_pending_source(Some(PendingSourceTail {
                source: rest,
                next_line: line_start - 1,
            }));
            cx.notify();
            return true;
        }
        self.finish_pending_materialization(cx);
        false
    }

    /// Runs the document-wide passes a fresh open would have run, now that the
    /// whole document is materialized.
    fn finish_pending_materialization(&mut self, cx: &mut Context<Self>) {
        self.rebuild_table_runtimes(cx);
        cx.notify();
    }

    /// Materializes every remaining pending line, for callers that must see the
    /// whole document (structural edits, document-wide search).
    pub(crate) fn flush_pending_materialization(&mut self, cx: &mut Context<Self>) {
        self.document.flush_pending_tail(cx);
        self.document.flush_pending_source(cx);
    }

    pub(crate) fn from_file_source(
        cx: &mut Context<Self>,
        source: String,
        file_path: Option<PathBuf>,
    ) -> Self {
        if file_path
            .as_ref()
            .is_some_and(|path| workspace::is_code_file(path))
        {
            let mut editor = Self::from_markdown(cx, String::new(), None);
            editor.replace_document_from_code_source(source, file_path.unwrap(), cx);
            editor
        } else {
            Self::from_markdown(cx, source, file_path)
        }
    }

    pub(crate) fn from_recovery(
        cx: &mut Context<Self>,
        snapshot: crate::config::RecoverySnapshot,
    ) -> Self {
        // Markdown rendering is for .md/.markdown only; every other text file
        // (code, dotfiles, plain text) restores as monospace source text.
        let markdown_file = snapshot
            .source_path
            .as_ref()
            .is_none_or(|path| {
                workspace::is_markdown_file(path)
                    || path
                        .extension()
                        .is_some_and(|extension| {
                            extension.to_string_lossy().eq_ignore_ascii_case("markdown")
                        })
            });
        let code_language = snapshot
            .source_path
            .as_ref()
            .filter(|_path| !markdown_file)
            .and_then(|path| path.extension())
            .map(|extension| extension.to_string_lossy().into_owned().into())
            .or_else(|| Some("text".into()));
        let mut editor = Self::from_markdown(cx, snapshot.markdown.clone(), None);
        if !markdown_file {
            editor.replace_document_content(snapshot.markdown, None, code_language, cx);
        }
        editor.recovery_id = snapshot.id;
        editor.recovery_source_path = snapshot.source_path;
        editor.is_recovered_document = true;
        editor.document_dirty = true;
        editor.pending_window_edited = true;
        editor.pending_window_title_refresh = true;
        editor
    }
}
