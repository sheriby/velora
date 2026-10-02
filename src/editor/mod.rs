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
mod buffer;
mod close;
mod context_menu;
mod modal;
mod document;
pub(crate) mod encoding;
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
mod file_history;
pub(crate) mod workspace_index;

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
    /// 文档文本的**唯一事实源**：块树是它上面的一份投影，保存写的是它。
    buffer: buffer::TextBuffer,
    /// 撤销/重做刚把缓冲区摆到目标状态，下一次 `mark_dirty` 就别再重投影盖掉它。
    skip_next_resync: bool,
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
    /// 状态栏整篇统计（字数、超长块）的静默窗口状态：打字期间沿用旧值，
    /// 停手 250ms 后补算一次（P2：每键整篇分词/扫行曾占单键成本一截）。
    status_scan_settled_revision: Option<u64>,
    status_scan_task: Option<Task<()>>,
    /// 文件树「复制」暂存的源文件路径（roadmap D6）。
    pub(crate) tree_clipboard: Option<std::path::PathBuf>,
    /// 拖动/缩放中的窗口 frame（roadmap A2 加固）：与磁盘上一致时不再调度写盘。
    observed_window_frame: Option<crate::config::WindowFrame>,
    /// 窗口 bounds 变化监听：它活着，`observe_window_bounds` 的回调才会被调用。
    window_bounds_subscription: Option<Subscription>,
    /// 防抖中的 frame 写盘任务；重新赋值即取消上一个计时。
    window_frame_write_task: Option<Task<()>>,
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
    /// 内容栏宽度变更时丢弃另一宽度下测得的行高。
    row_stride_width: Option<f32>,
    /// 性能计数器：整篇序列化次数（`current_document_source`）。大文档里
    /// 每键一次是 P2 热点，测试用它守住「每键最多一次」。
    source_serializations: std::cell::Cell<u64>,
    /// 性能计数器：source mapping 重建次数（每帧重建也是 P2 热点）。
    source_mapping_builds: std::cell::Cell<u64>,
    /// 性能计数器：状态栏整篇字数扫描次数（unicode 分词在大文档里很贵）。
    word_count_scans: std::cell::Cell<u64>,
    /// 性能计数器：行结构计划重建次数（每键重建整篇计划是 P2 热点）。
    row_plan_rebuilds: std::cell::Cell<u64>,
    /// 计数器：光标滚动实际改动的次数。撤销等操作会替换整篇块，
    /// 用旧布局的边界先滚一次、下一帧再纠正，就会让用户看到来回滚。
    caret_scroll_applications: std::cell::Cell<u64>,
    /// 性能诊断：四类全文遍数的累计耗时（纳秒），只给基准用例与诊断读。
    source_serialization_nanos: std::cell::Cell<u64>,
    source_mapping_nanos: std::cell::Cell<u64>,
    row_plan_nanos: std::cell::Cell<u64>,
    /// Where last frame's run sat among the scroll container's children.
    prev_mounted_run: Option<MountedRun>,
    /// 撤销/换模式等整篇替换后，光标滚动的「再看几帧」计数：行高在头几帧
    /// 还是估计值，滚动目标会随后续测量漂移，一次到位并不成立。
    scroll_settle_frames: u8,
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
    /// C13：模态键盘拦截器（App::intercept_keystrokes）。绑定解析先于
    /// 元素监听，只有这个钩子能在回车进入焦点块的 Newline 绑定之前
    /// 截住它。构造为 None，render 首帧注册后保持订阅存活。
    modal_key_interceptor: Option<gpui::Subscription>,
    /// 反链/标签面板与 [[ 补全共享的工作区链接索引（后台增量维护）。
    workspace_link_index: workspace_index::WorkspaceLinkIndex,
    /// 反链/标签面板的快照（防抖缓存，见 workspace_index::LinkPanelState）。
    link_panels: workspace_index::LinkPanelState,
    /// [[ 补全会话（编辑锚定块，键经 intercept_keystrokes 拦截）。
    wikilink_completion: Option<workspace_index::WikilinkCompletion>,
    /// 文件历史浮层（保存版本浏览/恢复）。
    file_history_overlay: Option<file_history::FileHistoryOverlay>,
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
    /// 已启动文件监听的工作区根：同一根不重复启动（含隐含根）。
    watched_workspace_root: Option<PathBuf>,
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
    /// Windows：标题栏左侧汉堡按钮展开的一级菜单列表是否打开。
    hamburger_menu_open: bool,
    /// 侧边栏收起时，指针贴到窗口左边缘临时滑出的浮层是否可见。
    /// 收起状态下的「自动隐藏」：不占布局，滑出时盖在正文上。
    sidebar_peek: bool,
    /// 浮层正在播放「收回」动画：动画期间浮层仍挂载，播完才真正卸载。
    sidebar_overlay_closing: bool,
    /// 每次收回递增，用于作废旧收回定时器：快速「贴边→移开→再贴边」时，
    /// 上一轮的定时器不能把新一轮滑入砍掉。
    sidebar_collapse_generation: u32,
    /// 每次进出贴边感应区递增：贴边必须停留满 dwell 才唤出，扫过不停留时，
    /// 挂着的停留定时器靠它作废。
    sidebar_edge_dwell_generation: u32,
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
    /// 帧级选区快照对应的 (活动块, 选区)：没变就不重算（见
    /// `refresh_selection_snapshot_if_changed`）。
    last_selection_snapshot_source: Option<(EntityId, std::ops::Range<usize>)>,
    /// ⌘P/⇧⌘P 打开前的正文焦点块：关闭浮层时还回去（不然敲字全丢）。
    overlay_focus_restore_target: Option<EntityId>,
    last_stable_source_text: String,
    history_restore_in_progress: bool,
    image_reference_definitions: Arc<ImageReferenceDefinitions>,
    link_reference_definitions: Arc<LinkReferenceDefinitions>,
    footnote_registry: Arc<FootnoteRegistry>,
    runtime_context_sensitive_blocks: HashSet<EntityId>,
}

/// Runtime binding between a table block and one cell editor.
#[derive(Clone)]
pub(crate) struct TableCellBinding {
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
    /// 这一步在缓冲区上留下的写入记录，按文档顺序。
    ///
    /// 撤销 = 从后往前把每条的 `new_range` 换回 `removed`；重做 = 把逆操作的结果
    /// 再反过来放回去。存的是增量，不是全文快照——这是 10 MiB 文档单键 13 秒、
    /// 撤销栈最坏 2 GB 那笔账的还法。
    edits: Vec<buffer::AppliedEdit>,
    selection: UndoSelectionSnapshot,
    timestamp: Instant,
    kind: UndoCaptureKind,
}

impl HistoryEntry {
    /// 撤销栈闸门盯的这个数：正常打字与拆合块之后，它不该随文档大小增长。
    pub(crate) fn byte_len(&self) -> usize {
        self.edits.iter().map(|edit| edit.removed.len()).sum()
    }
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
        // 缓冲区先于块树建好：导入器要把每根块的源码区间换算成字节区间，
        // 靠的就是它的行索引。
        let buffer = buffer::TextBuffer::from_text(&normalized);
        let source_mode_fallback_required =
            Self::markdown_requires_source_mode_fallback(&normalized);
        let mut pending_tail = None;
        let mut roots = if source_mode_fallback_required {
            let block = Self::new_block(cx, BlockRecord::paragraph(normalized.clone()));
            block.update(cx, |block, _cx| block.set_source_document_mode());
            vec![block]
        } else {
            let lines = Arc::new(Self::split_markdown_lines(&normalized));
            let (roots, root_spans, next_line) = Self::build_root_block_chunk(
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
            Self::attach_root_spans(&buffer, &roots, &root_spans, 0, cx);
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
            buffer,
            skip_next_resync: false,
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
            status_scan_settled_revision: None,
            status_scan_task: None,
            tree_clipboard: None,
            observed_window_frame: None,
            window_bounds_subscription: None,
            window_frame_write_task: None,
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
            source_serializations: std::cell::Cell::default(),
            source_serialization_nanos: std::cell::Cell::default(),
            source_mapping_nanos: std::cell::Cell::default(),
            row_plan_nanos: std::cell::Cell::default(),
            source_mapping_builds: std::cell::Cell::default(),
            word_count_scans: std::cell::Cell::default(),
            row_plan_rebuilds: std::cell::Cell::default(),
            caret_scroll_applications: std::cell::Cell::default(),
            prev_mounted_run: None,
            scroll_settle_frames: 0,
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
            modal_key_interceptor: None,
            workspace_link_index: workspace_index::WorkspaceLinkIndex::default(),
            link_panels: workspace_index::LinkPanelState::default(),
            wikilink_completion: None,
            file_history_overlay: None,
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
            watched_workspace_root: None,
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
            hamburger_menu_open: false,
            sidebar_peek: false,
            sidebar_overlay_closing: false,
            sidebar_collapse_generation: 0,
            sidebar_edge_dwell_generation: 0,
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
            last_selection_snapshot_source: None,
            overlay_focus_restore_target: None,
            last_stable_source_text: normalized.clone(),
            history_restore_in_progress: false,
            image_reference_definitions: Arc::default(),
            link_reference_definitions: Arc::default(),
            footnote_registry: Arc::default(),
            runtime_context_sensitive_blocks: HashSet::new(),
        };
        // last_stable_source_text 必须取导入模型的序列化文本，而不是原始输入：
        // 序列化会对非规范输入做规范化（如代码围栏后直接跟 `---` 会补空行），
        // 大纲/锚点跳转/源码映射全部以这份文本为基准；拿原文当基准时偏移随
        // 文档深度累积漂移（用户报修：大纲跳转光标落进标题两个字之间）。
        // 代码文档例外：内容没有 Markdown 规范化问题，且撤销恢复按原始
        // 字节走，套上围栏会破坏文档。文件本身的规范化与现状一致——首次
        // 保存时才落盘。
        editor.last_stable_source_text = if editor.code_document {
            normalized
        } else {
            editor.document.markdown_text(cx)
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

        let (roots, root_spans, consumed) = Self::build_root_block_chunk(
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
        // 续建块的行区间要加上这一片在全文里的行基址，否则第二块之后全部错位。
        Self::attach_root_spans(&self.buffer, &roots, &root_spans, tail.next_line, cx);
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

    /// 从一次真实读盘建编辑器：文本进缓冲区，**原始字节**留作「未编辑就原样
    /// 写回」的依据。字节与文本必须来自同一次读盘，所以在这里一起接住。
    pub(crate) fn from_loaded_document(
        cx: &mut Context<Self>,
        document: encoding::LoadedDocument,
        file_path: Option<PathBuf>,
    ) -> Self {
        let encoding::LoadedDocument { raw, text } = document;
        let mut editor = Self::from_file_source(cx, text, file_path);
        editor.attach_file_origin(raw);
        editor
    }

    /// 记下这个文档来自磁盘的原始字节与文件形状。空字节表示来源不是文件
    /// （新建、恢复快照），于是没有「原样写回」的依据，保存走重新编码。
    pub(crate) fn attach_file_origin(&mut self, raw: Vec<u8>) {
        if raw.is_empty() {
            return;
        }
        let shape = buffer::FileShape::detect(&raw);
        self.buffer.set_file_origin(raw, shape);
    }

    /// 把导入器记录的「根块消费的行区间」换算成缓冲区字节区间，挂到块上。
    ///
    /// 区间右端是**下一块的起始行**，换算成字节时要减掉那个换行符：块的内容
    /// 不含它自己的行尾换行，块与块之间的空行更不归入任何块——这样编辑一个块
    /// 时，写回的字节不会越界碰到邻居或分隔空行。
    ///
    /// 唯一的例外是文档末尾没有换行符：那里没有换行可减，最后一块的区间右端
    /// 就是文档末尾，减一个字节会把块内容和多字节字符一起切坏。
    pub(crate) fn attach_root_spans(
        buffer: &buffer::TextBuffer,
        roots: &[Entity<Block>],
        line_spans: &[std::ops::Range<usize>],
        line_base: usize,
        cx: &mut App,
    ) {
        let total = buffer.byte_len();
        let ends_with_newline = buffer.line_start(buffer.line_count().saturating_sub(1)) >= total;
        for (block, span) in roots.iter().zip(line_spans) {
            let start = buffer.line_start(line_base + span.start).min(total);
            let raw_end = buffer.line_start(line_base + span.end).min(total);
            let end = if raw_end < total || ends_with_newline {
                raw_end.saturating_sub(1)
            } else {
                raw_end
            };
            let span = start..end.max(start);
            block.update(cx, |block, _cx| {
                block.record.source_span = Some(span);
            });
        }
    }

    /// 把这个块当前的源码写回它自己占的缓冲区区间——只经唯一写入口
    /// [`buffer::TextBuffer::edit`]。
    ///
    /// 返回 `false` 表示这个块没有可用区间（新建的块、子块、整篇重投影后没被
    /// 记到的空块）：那种情况下按区间写会把字节落错位置，调用方必须退回
    /// [`Self::resync_buffer_and_stable_snapshot`]。
    pub(crate) fn write_back_block_source(
        &mut self,
        block: &Entity<Block>,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(old_span) = block.read(cx).record.source_span.clone() else {
            return false;
        };
        let new_source = self.document.block_markdown_source(block, cx);
        if self.buffer.slice(old_span.clone()) == new_source {
            // 投影刷新但内容没变：不动缓冲区，区间照旧有效。
            return true;
        }

        let applied = self.buffer.edit(old_span.clone(), &new_source);
        self.record_buffer_edit(applied.clone());
        block.update(cx, |block, _cx| {
            block.record.source_span = Some(applied.new_range);
        });
        let delta = new_source.len() as i64 - (old_span.end - old_span.start) as i64;
        if delta != 0 {
            self.shift_root_spans_after(old_span.end, delta, cx);
        }
        true
    }

    /// 结构变更（拆块、合块）之后，把被换掉的那一段连续根块写回缓冲区。
    ///
    /// `before` 是变更前的根块布局。用「首尾对齐」算出 old_run / new_run：变更
    /// 前后从尾部数第一个不相等的根块位置就是这段的右界，锚点所在根块是左界。
    /// 返回 `false` 表示这段算不出来（纯插入、根块没挂区间、整段都是空段落），
    /// 调用方继续用整篇重投影兜底。
    pub(crate) fn write_back_root_region(
        &mut self,
        anchor: &Entity<Block>,
        before: &[(EntityId, Option<std::ops::Range<usize>>)],
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(anchor_index) = before.iter().position(|(id, _)| *id == anchor.entity_id()) else {
            return false;
        };
        let after = self.document.root_layout(cx);
        let suffix = before
            .iter()
            .rev()
            .zip(after.iter().rev())
            .take_while(|((before_id, _), (after_id, _))| before_id == after_id)
            .count();
        let Some(old_end) = before.len().checked_sub(suffix) else {
            return false;
        };
        let Some(new_end) = after.len().checked_sub(suffix) else {
            return false;
        };
        // 纯插入：要补的分隔换行数取决于右边那块的种类，这一版不算，交给兜底。
        if anchor_index >= old_end {
            return false;
        }
        let Some(region_start) = before[anchor_index].1.clone().map(|span| span.start) else {
            return false;
        };
        let Some(region_end) = before[old_end - 1].1.clone().map(|span| span.end) else {
            return false;
        };
        if region_end <= region_start {
            return false;
        }

        let Some((text, local_spans)) =
            self.document.markdown_region_for_roots(anchor_index..new_end, cx)
        else {
            return false;
        };
        let replaced_len = region_end - region_start;
        let applied = self.buffer.edit(region_start..region_end, &text);
        self.record_buffer_edit(applied);
        // 先平移再分配：新块区间的右端可能已经越过 region_end（拆块会变长）。
        let delta = text.len() as i64 - replaced_len as i64;
        if delta != 0 {
            self.shift_root_spans_after(region_start, delta, cx);
        }
        for (id, local) in local_spans {
            let span = region_start + local.start..region_start + local.end;
            let roots = self.document.root_blocks();
            let Some(block) = roots
                .iter()
                .find(|block| block.entity_id() == id)
                .cloned()
            else {
                continue;
            };
            block.update(cx, |block, _cx| {
                block.record.source_span = Some(span);
            });
        }
        true
    }

    /// 编辑点之后的根块区间整体平移；之前的块字节没被碰到，区间自然不动。
    fn shift_root_spans_after(&mut self, from: usize, delta: i64, cx: &mut App) {
        let total = self.buffer.byte_len() as i64;
        let moved = self
            .document
            .root_blocks()
            .iter()
            .filter_map(|block| {
                let span = block.read(cx).record.source_span.clone()?;
                (span.start >= from).then_some((block.clone(), span))
            })
            .collect::<Vec<_>>();
        for (block, span) in moved {
            let start = (span.start as i64 + delta).clamp(0, total) as usize;
            let end = (span.end as i64 + delta).clamp(0, total) as usize;
            block.update(cx, |block, _cx| {
                block.record.source_span = Some(start..end.max(start));
            });
        }
    }

    /// 拆块事件的写回入口：先看根块序列变了没有。
    ///
    /// - 变了（段落一分为二、两块合一）→ 只重写被换掉的那一段区间。
    /// - 没变（子块拆合，新块挂在某个根块底下）→ 整根块重投影，它的区间本来就
    ///   盖住整棵子树，别的根块照样不动。
    pub(crate) fn write_back_newline_region(
        &mut self,
        block: &Entity<Block>,
        before: Option<&[(EntityId, Option<std::ops::Range<usize>>)]>,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(before) = before else { return false };
        if self.write_back_root_region(block, before, cx) {
            return true;
        }
        let Some(root) = self.document.root_ancestor_of(block.entity_id()) else {
            return false;
        };
        // 根块序列一模一样才敢走整块重投影：不一样说明结构变更没算出区间，得兜底。
        let unchanged_sequence = before
            .iter()
            .map(|(id, _)| *id)
            .eq(self.document.root_layout(cx).into_iter().map(|(id, _)| id));
        unchanged_sequence && self.write_back_block_source(&root, cx)
    }

    /// 这个文档的字节是否由缓冲区说了算。
    ///
    /// 源码视图与代码/纯文本文件的块树是「整篇源码的一份投影」：根块没有源码区间
    /// （写回无处落笔），而 `markdown_text` 还会给整篇文本套上围栏。这类文档继续
    /// 走序列化，直到阶段 2 让源码模式直接读写缓冲区为止。
    pub(crate) fn writes_through_the_buffer(&self) -> bool {
        !self.code_document
            && !self.source_mode_fallback_required
            && self.view_mode == ViewMode::Rendered
    }

    /// 块树变了：把它的序列化换进缓冲区（除非这次改动自己声明过区间），并刷新
    /// 搜索/大纲赖以定位的稳定快照。两者必须是同一份文本，否则坐标会漂。
    ///
    /// 这是写回的保底档位，代价是**未编辑的块也被重新序列化一次**（表格列宽填充、
    /// `__` 强调这些写法就此改写），原始字节也随之丢弃。每多一条走到这里的路径，
    /// 就少一块「保住原文」的地盘——收敛方向是让改动自己声明区间，不是让这里变快。
    pub(crate) fn resync_buffer_and_stable_snapshot(&mut self, cx: &mut Context<Self>) {
        let skip_resync = std::mem::take(&mut self.skip_next_resync);
        if self.writes_through_the_buffer() {
            let (text, block_spans) = self.document.markdown_text_with_block_spans(cx);
            self.last_stable_source_text = text.clone();
            if !skip_resync {
                self.apply_resynced_text(&text);
                // 区间只有在它派生自的那份文本里才成立：跳过重投影时也别动区间，
                // 写回路径已经把区间按缓冲区字节摆好了。
                self.reattach_root_spans(&block_spans, &text, cx);
            }
            return;
        }
        // 源码/代码文档：缓冲区装的是不套围栏的源码文本。
        let text = self.document.raw_source_text(cx);
        self.last_stable_source_text = text.clone();
        if !skip_resync {
            self.apply_resynced_text(&text);
        }
    }

    /// 把重投影出来的文本作为**一次**写入落进缓冲区。
    ///
    /// 走 `edit` 而不是整个换掉缓冲区，是为了让撤销组拿到它的逆操作；文本没变时
    /// 什么都不做，「打开后没改过」那份原始字节也就保住了。
    fn apply_resynced_text(&mut self, text: &str) {
        if self.buffer.matches_text(text) {
            return;
        }
        let range = 0..self.buffer.byte_len();
        let applied = self.buffer.edit(range, text);
        self.record_buffer_edit(applied);
    }

    /// 按重投影出来的文本重建所有根块的源码区间。
    fn reattach_root_spans(
        &mut self,
        block_spans: &[(EntityId, std::ops::Range<usize>)],
        text: &str,
        cx: &mut Context<Self>,
    ) {
        let roots = self.document.root_blocks().to_vec();
        for block in roots {
            let span = block_spans
                .iter()
                .find(|(id, _)| *id == block.entity_id())
                .map(|(_, span)| {
                    // 序列化那边记的区间右端含本块行尾的换行，写回约定不含。
                    let start = span.start.min(text.len());
                    let end = span.end.min(text.len());
                    let end = if end > start && text.as_bytes()[end - 1] == b'\n' {
                        end - 1
                    } else {
                        end
                    };
                    Some(start..end.max(start))
                })
                .unwrap_or_default();
            block.update(cx, |block, _cx| {
                block.record.source_span = span;
            });
        }
    }

    /// 文档内容被整体替换（切标签、拖拽打开、会话恢复）时重建缓冲区。
    ///
    /// 缓冲区是唯一事实源，绝不能留着上一个文档的内容——那会让保存写出别的
    /// 文件的字节。传入的必须是已规范成 LF 的文本。
    pub(crate) fn reset_buffer_for_text(&mut self, normalized_text: &str) {
        self.buffer = buffer::TextBuffer::from_text(normalized_text);
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
