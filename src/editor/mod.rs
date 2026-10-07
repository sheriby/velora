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
use self::selection::{TableTextPosition, TableTextSelection};
use self::document::ChunkCursor;
use self::tree::{DocumentTree, PendingSourceTail, PendingTail};
use crate::components::{
    Block, BlockKind, BlockRecord, FootnoteDefinitionBinding, FootnoteReferenceLocation,
    FootnoteRegistry, FootnoteResolvedOccurrence, ImageReferenceDefinitions, InlineTextTree,
    LinkReferenceDefinitions, parse_image_reference_definitions, parse_link_reference_definitions,
};
use crate::components::markdown::source_highlight::MarkdownSourceState;
use crate::components::{
    TableAxisHighlight, TableAxisKind, TableAxisMarker, TableCellPosition, TableColumnAlignment,
    TableData, TableRuntime, UndoCaptureKind, serialize_table_cell_markdown,
};
mod buffer;
mod clipboard_ops;
mod close;
mod context_menu;
mod modal;
mod document;
pub(crate) mod encoding;
mod events;
mod export;
mod file_drop;
mod format_ops;
mod history;
mod insert_ops;
mod latex_completion;
mod formula_editor;
mod paragraph_ops;
mod persistence;
mod render;
mod runtime_context;
mod selection;
mod selection_toolbar;
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
    /// 行结构计划缓存（P4b）：键 (块树的行元数据版本, fold_state_version,
    /// toc 版本, 渲染模式)，见 render::RenderedRowPlan。行元数据版本只在
    /// 层级/分组锚点/目录标记真变时才动——打字不再每键重排整张计划。
    rendered_row_plan: Option<std::sync::Arc<render::RenderedRowPlan>>,
    /// 折叠状态版本：任何 folded 变更都递增，使行计划重建。
    fold_state_version: u64,
    /// P7：文档打开代数——replace_document_content 每次递增，
    /// 用于后台 file_version 哈希写回时的竞态校验。
    open_generation: u64,
    /// TOC 条目版本：大纲重建后递增（[TOC] 块的条目同步随行计划进行）。
    /// 只在正文里真有 `[TOC]` 块（读者）时递增，见 tree_sync 的换标签路径。
    toc_state_version: u64,
    /// 内容栏宽度变更时丢弃另一宽度下测得的行高。
    row_stride_width: Option<f32>,
    /// 性能计数器：整篇序列化次数（`current_document_source`）。大文档里
    /// 每键一次是 P2 热点，测试用它守住「每键最多一次」。
    source_serializations: std::cell::Cell<u64>,
    /// 性能计数器：source mapping 重建次数（每帧重建也是 P2 热点）。
    source_mapping_builds: std::cell::Cell<u64>,
    /// 性能计数器：其中「整篇重建」的次数。按单块取映射是 O(这一块)，整篇重建是
    /// O(文档)——打字这类每键动作只许付前者的钱。
    source_mapping_full_builds: std::cell::Cell<u64>,
    /// 性能计数器：状态栏整篇字数扫描次数（unicode 分词在大文档里很贵）。
    word_count_scans: std::cell::Cell<u64>,
    /// 性能计数器：文档命中表**重扫整篇**的次数。缓存命中不算；
    /// 每键之外的动作（跳转、点结果行、切视图）只许复用，不许重扫。
    document_match_scans: std::cell::Cell<u64>,
    /// 性能计数器：行结构计划重建次数（每键重建整篇计划是 P2 热点）。
    row_plan_rebuilds: std::cell::Cell<u64>,
    /// 计数器：一次行计划重建里**读了几个块实体**。折叠过滤与分组扫描原来每个可见
    /// 块都要 `entity.read` 一遍（10 MiB 实测 159,683 块 → 121ms 里的大头），
    /// 行元数据缓存进可见列表快照后，这个数只该随「标题数 + 目录候选数」长，
    /// 不该随文档长度长。
    row_plan_block_reads: std::cell::Cell<u64>,
    /// 性能计数器：重投影出来的**根块数**。整篇重解析一次就是「文档有多少根块」，
    /// 区域重解析只重建改动那一段的几根——增量重投影要守的就是这个数不跟着文档长。
    roots_reprojected: std::cell::Cell<u64>,
    /// 性能计数器：文档大纲**整棵树重拼**了几次（侧栏收起也算，`[TOC]` 靠它）。
    /// 大纲把整个缓冲区按行重扫一遍找标题，一次就是 O(文档)——它此前不在任何
    /// 计数器里，所以「整篇落笔 0 次」全绿的同时，10 MiB 一次按键仍有 110ms
    /// 花在这里（实测占该键渲染量的 74%）。标题集合只是文字变了时走原地换标签
    /// （`relabel_outline_in_place`），不算重拼。
    outline_rebuilds: std::cell::Cell<u64>,
    /// 计数器：其中「整篇重扫」的次数。增量按块重扫是常态，这一档只该在块带不出
    /// 可用区间（跨块的未闭合围栏、没有源码区间的块）时出现——它一涨就说明增量那条
    /// 路对这个文档没生效。
    outline_full_rescans: std::cell::Cell<u64>,
    /// 计数器：大纲重建累计扫过的**行数**（问缓冲区的行计数，不是数换行符）。
    outline_lines_scanned: std::cell::Cell<u64>,
    /// 性能计数器：大纲重建累计耗时。
    outline_nanos: std::cell::Cell<u64>,
    /// 计数器：侧栏大纲**上一帧**建了几行元素。行数只该随视口高度长
    /// （窗口化后每帧只建视口那几十行）；它跟着文档里的标题数长，就说明
    /// 一整棵树又被走成了元素树。
    outline_rows_rendered: std::cell::Cell<u64>,
    /// 计数器：上一帧大纲窗口从第几行开始——随滚动走。它不跟着滚动走，
    /// 就是窗口没接上滚动位置。
    outline_first_row_rendered: std::cell::Cell<u64>,
    /// 侧栏大纲首帧还没有滚动视口尺寸时续挂的帧数（上限与正文冷启动共用同一个
    /// 手法：排下一帧，量到尺寸就停，避免每帧重排）。
    outline_fill_frames: u8,
    /// 计数器：块内每行的记号宽度有几行是**解析期记下来的**（数据）。
    line_prefix_from_record: std::cell::Cell<u64>,
    /// 计数器：有几行还在事后拿文件行与模型行比（`measured_block_line_prefixes` 那一族）。
    /// 比出来的宽度是「猜」：缩进过两格、`#标题` 少一个空格、硬换行被序列化成 `\\`，
    /// 每种写法都能把它带漂一位，字就落进别的字节里。#33 的进度就看这一档往 0 降。
    line_prefix_measured: std::cell::Cell<u64>,
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
    /// `\\` 公式命令补全会话（数学块/行内公式里打反斜杠弹出）。
    latex_completion: Option<latex_completion::LatexCompletion>,
    /// 公式编辑器弹窗（独立编辑窗口：草稿输入+实时预览+符号面板）。
    formula_editor: Option<formula_editor::FormulaEditorState>,
    /// 双击数学块记下的「待打开」目标：块事件链没有 &mut Window，焦点要等
    /// 下一帧 render（有 window）时交给弹窗（apply_pending_formula_editor）。
    pending_formula_editor: Option<gpui::EntityId>,
    /// 公式编辑器弹窗（独立编辑窗口：草稿输入+实时预览+符号面板）。
    
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
    /// 上一次画上「活动命中」单独标记的那几根块；循环跳转只挪这几块上的标记。
    pub(super) search_active_blocks: Vec<Entity<Block>>,
    /// 上一次全量高亮同步的输入指纹。对得上就走只挪标记的快路径。
    pub(super) search_highlight_key: Option<workspace::SearchHighlightKey>,
    /// Quick file switcher overlay (⌘P); `None` while closed.
    quick_open: Option<quick_open::QuickOpenState>,
    /// Command palette overlay (⇧⌘P); `None` while closed.
    command_palette: Option<command_palette::CommandPaletteState>,
    /// Workspace change watcher (roadmap D3); `None` until a root is set.
    external_watcher: Option<notify::RecommendedWatcher>,
    /// 已启动文件监听的工作区根：同一根不重复启动（含隐含根）。
    watched_workspace_root: Option<PathBuf>,
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
    /// 选中一段文字后浮出的那条工具栏；None 表示这一帧没有可挂的选区。
    selection_toolbar: Option<selection_toolbar::SelectionToolbarState>,
    table_axis_preview: Option<TableAxisSelection>,
    table_axis_selection: Option<TableAxisSelection>,
    table_text_selection: Option<TableTextSelection>,
    /// 已经铺到格子上的那一批跨格高亮（表格重建会换掉格子实体，位置解析不到就当没有）。
    table_text_selection_painted: Vec<(EntityId, TableCellPosition)>,
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

/// 一次「换掉整棵块树」之后要把选区放回原处时所记的区间，单位是缓冲区偏移。
/// 撤销、切视图、外部改动重载、标签切换共用这一份形状。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct UndoSelectionSnapshot {
    range: std::ops::Range<usize>,
    reversed: bool,
}

/// 一篇文档的阅读现场：视图模式、视口纵向偏移（像素，负值向下）、光标区间。
///
/// 只换内容、不换文档的路径靠它把现场交还给用户——外部改动重载当场记下再交回，
/// 标签切换则把它存在标签上（`WorkspaceDocumentTab::view`），切回来时交回。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DocumentView {
    view_mode: ViewMode,
    /// 存 `f32` 而不是 `Pixels`：`Pixels` 没有 `Debug`，标签要能打印。代价是这一项
    /// 只能比到 `PartialEq`——浮点相等在这里就是「同一个偏移」，不做近似。
    scroll_y: f32,
    selection: UndoSelectionSnapshot,
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
    /// 只有测试闸门读它（见 `undo_history_byte_len`）。
    #[cfg(test)]
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
    /// 按下的那一下落在哪个表格格子上、格内第几个字。指针没离开这一格时选字仍是
    /// 格子自己的事；离开之后编辑器才接手，把两格之间的文字按格切段选上。
    pub(super) anchor_table_cell: Option<(EntityId, TableTextPosition)>,
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
        let mut initial_spans: Vec<(gpui::EntityId, std::ops::Range<usize>)> = Vec::new();
        let mut roots = if source_mode_fallback_required {
            let block = Self::new_block(cx, BlockRecord::paragraph(normalized.clone()));
            block.update(cx, |block, _cx| {
                block.set_source_document_mode();
                block.set_source_language("markdown");
            });
            let roots = vec![block];
            if let Some(spans) = Self::source_slice_span_ranges(&buffer, &roots, cx) {
                initial_spans.extend(roots.iter().map(|block| block.entity_id()).zip(spans));
            }
            roots
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
            initial_spans.extend(
                roots
                    .iter()
                    .map(|block| block.entity_id())
                    .zip(Self::root_span_ranges(&buffer, &root_spans, 0)),
            );
            roots
        };
        if roots.is_empty() {
            roots.push(Self::new_block(cx, BlockRecord::paragraph(String::new())));
        }

        let mut document = DocumentTree::new(roots);
        for (entity_id, span) in initial_spans {
            document.set_source_span(entity_id, span);
        }
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
            source_mapping_full_builds: std::cell::Cell::default(),
            word_count_scans: std::cell::Cell::default(),
            document_match_scans: std::cell::Cell::default(),
            row_plan_rebuilds: std::cell::Cell::default(),
            row_plan_block_reads: std::cell::Cell::default(),
            roots_reprojected: std::cell::Cell::default(),
            outline_rebuilds: std::cell::Cell::default(),
            outline_full_rescans: std::cell::Cell::default(),
            outline_lines_scanned: std::cell::Cell::default(),
            outline_nanos: std::cell::Cell::default(),
            outline_rows_rendered: std::cell::Cell::default(),
            outline_first_row_rendered: std::cell::Cell::default(),
            outline_fill_frames: 0,
            line_prefix_from_record: std::cell::Cell::default(),
            line_prefix_measured: std::cell::Cell::default(),
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
            latex_completion: None,
            formula_editor: None,
            pending_formula_editor: None,
            file_history_overlay: None,
            pending_workspace_tab_activation: None,
            info_dialog: None,
            unsupported_preview_path: None,
            unsupported_preview_detail: None,
            pending_folder_choice: None,
            show_welcome: false,
            search_highlighted_blocks: Vec::new(),
            search_active_blocks: Vec::new(),
            search_highlight_key: None,
            quick_open: None,
            command_palette: None,
            external_watcher: None,
            watched_workspace_root: None,
            last_outline_follow_offset: f32::NAN,
            cursor_history_back: Vec::new(),
            cursor_history_forward: Vec::new(),
            update_check_in_progress: false,
            workspace: WorkspaceState::default(),
            status_bar: StatusBarState::default(),
            context_menu: None,
            table_insert_dialog: None,
            context_menu_submenu_close_task: None,
            selection_toolbar: None,
            table_axis_preview: None,
            table_axis_selection: None,
            table_text_selection: None,
            table_text_selection_painted: Vec::new(),
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
        self.attach_root_spans(&roots, &root_spans, tail.next_line);
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
        // 区间按**整棵根序列**重挂：这一片是接在已物化的那些片之后的，只按新块自己
        // 从 0 数会把第二片排到第一片身上（区间重叠，写回就把字节写到别的块里）。
        self.reattach_source_document_spans(cx);

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
        if self.view_mode == ViewMode::Source {
            self.reattach_source_document_spans(cx);
        }
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
    pub(crate) fn root_span_ranges(
        buffer: &buffer::TextBuffer,
        line_spans: &[std::ops::Range<usize>],
        line_base: usize,
    ) -> Vec<std::ops::Range<usize>> {
        let total = buffer.byte_len();
        let ends_with_newline = buffer.line_start(buffer.line_count().saturating_sub(1)) >= total;
        line_spans
            .iter()
            .map(|span| {
                let start = buffer.line_start(line_base + span.start).min(total);
                let raw_end = buffer.line_start(line_base + span.end).min(total);
                let end = if raw_end < total || ends_with_newline {
                    raw_end.saturating_sub(1)
                } else {
                    raw_end
                };
                start..end.max(start)
            })
            .collect()
    }

    pub(crate) fn attach_root_spans(
        &mut self,
        roots: &[Entity<Block>],
        line_spans: &[std::ops::Range<usize>],
        line_base: usize,
    ) {
        let spans = Self::root_span_ranges(&self.buffer, line_spans, line_base);
        for (block, span) in roots.iter().zip(spans) {
            self.document.set_source_span(block.entity_id(), span);
        }
    }

    /// 源码/代码文档的根块区间：每根块是缓冲区的一段切片，块与块之间只隔一个换行。
    ///
    /// 推进量取块自己的文本长度，所以 `buffer.slice(span) == block.display_text()`
    /// 恒成立——这一档没有记号、没有空行分隔、也没有跨块容器，块落在哪一位不需要问
    /// 解析器（渲染态那套「按行区间换算」在这里没有对应的东西）。
    ///
    /// 块文本加起来比缓冲区还长（尾部还在后台续建到一半、或某一步没落笔）就一根都不挂：
    /// 没有区间的块自然退回整篇重投影那一档，挂错位置的区间会让字节写到邻居身上。
    pub(crate) fn source_slice_span_ranges(
        buffer: &buffer::TextBuffer,
        roots: &[Entity<Block>],
        cx: &App,
    ) -> Option<Vec<std::ops::Range<usize>>> {
        let total = buffer.byte_len();
        let mut spans = Vec::with_capacity(roots.len());
        let mut offset = 0usize;
        for block in roots {
            let len = block.read(cx).display_text().len();
            if offset + len > total {
                return None;
            }
            spans.push(offset..offset + len);
            offset += len + 1;
        }
        Some(spans)
    }

    pub(crate) fn attach_source_slice_spans(&mut self, roots: &[Entity<Block>], cx: &mut App) {
        let Some(spans) = Self::source_slice_span_ranges(&self.buffer, roots, cx) else {
            return;
        };
        for (block, span) in roots.iter().zip(spans) {
            self.document.set_source_span(block.entity_id(), span);
        }
        // 行号栏宽度用全文档总行数做统一基准：分块各按自己的最后行号算，
        // 513 行那块的栏会宽出一位，上下行号对不齐（用户报修）。缓冲区在
        // 打开时就是完整的，续建的块到这里也一并拿到正确的基准。
        let basis = self.buffer.line_count();
        for block in roots {
            block.update(cx, |block, _cx| block.set_source_line_gutter_basis(basis));
        }
    }

    /// 树里已经有一批源码文档的根块：按上面的口径把区间全部重挂一遍。
    fn reattach_source_document_spans(&mut self, cx: &mut Context<Self>) {
        let roots = self.document.root_blocks().to_vec();
        self.attach_source_slice_spans(&roots, cx);
    }

    /// markdown 源码文档的接缝状态整棵重串一遍：每根块拿上一根的 exit 当
    /// entry 重算高亮。打开、切视图、undo 重投影这些整棵重建的入口走这条，
    /// O(文档) 一次——块级高亮扫描本身线性于块文本，加起来就是文档一遍。
    pub(crate) fn resync_source_fence_states(&mut self, cx: &mut App) {
        let roots = self.document.root_blocks().to_vec();
        let mut state: Option<MarkdownSourceState> = None;
        for block in &roots {
            let entry = state.clone();
            let mut exit = None;
            block.update(cx, |block, _cx| {
                block.set_source_fence_entry(entry);
                block.refresh_source_highlight();
                exit = block.source_fence_exit();
            });
            state = exit;
        }
    }

    /// 编辑后的增量级联：编辑块自己的 exit 已随它的 `sync_code_highlight`
    /// 更新。下一块的 entry 与之不一致（这次改动开/关了围栏、公式、frontmatter）
    /// 才从下一块起重串；一致就立刻停——打字热路径为此只多一次实体读。
    pub(crate) fn cascade_source_fence_states_after(
        &mut self,
        block_id: gpui::EntityId,
        cx: &mut Context<Self>,
    ) {
        if self.code_document || self.view_mode != ViewMode::Source {
            return;
        }
        let Some(location) = self.document.find_block_location(block_id) else {
            return;
        };
        if location.parent.is_some() {
            return;
        }
        let roots = self.document.root_blocks().to_vec();
        let Some(edited) = roots.get(location.index) else {
            return;
        };
        let mut state = edited.read(cx).source_fence_exit();
        for block in &roots[location.index + 1..] {
            if block.read(cx).source_fence_entry() == state {
                break;
            }
            let entry = state.clone();
            let mut exit = None;
            block.update(cx, |block, _cx| {
                block.set_source_fence_entry(entry);
                block.refresh_source_highlight();
                exit = block.source_fence_exit();
            });
            state = exit;
        }
    }

    /// 把这个块当前的源码写回它自己占的缓冲区区间——只经唯一写入口
    /// [`buffer::TextBuffer::edit`]。
    ///
    /// 打字这种「在光标处插入几个字节」的改动：直接把那几个字节插到缓冲区里
    /// 对应的源码位置。
    ///
    /// 整块重新序列化会顺手改写这一块自己的定界符——`1)` 变 `1.`、`__粗__` 变
    /// `**粗**`、`>引用` 补成 `> 引用`、`~~~~` 换成 ```，可用户只碰了中间那几个字。
    /// 插入点由可见偏移换算成源码字节位（`caret_source_offset`）：插入点之前的
    /// 文本没动过，所以换算照样成立。算不出落点、或插入的文字序列化后会变样
    /// （打进去的是个字面 `*`）都退回整块写回。
    pub(crate) fn write_back_visible_insertion(
        &mut self,
        block: &Entity<Block>,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some((visible_offset, inserted)) =
            block.update(cx, |block, _cx| block.take_pending_visible_insertion())
        else {
            return false;
        };
        if crate::components::InlineTextTree::plain(inserted.clone()).serialize_markdown()
            != inserted
        {
            return false;
        }
        let Some(at) = self.caret_source_offset(block.entity_id(), visible_offset, cx) else {
            return false;
        };
        let Some(root) = self.document.root_ancestor_of(block.entity_id()) else {
            return false;
        };
        let Some(span) = self.document.source_span_of(root.entity_id()) else {
            return false;
        };
        // 换算出来的位置必须在缓冲区里、也得落在这一根块的区间内：映射表是按块树
        // 的序列化算的，容器里的块（引用、列表）区间与缓冲区并不一一对应，越界就
        // 说明这次改动不是「往源码里插几个字节」那么简单，退回整块写回。
        if at > self.buffer.byte_len() || !self.buffer.is_char_boundary(at) {
            return false;
        }
        if at < span.start || at > span.end {
            return false;
        }
        // 映射按块的**序列化**记前缀（`"> "`、`"- "`、`"1. "`），用户写非规范前缀
        //（或解析期账本对不上现状）时换算出的落点会整体漂移，而且仍然落在根块
        // 区间内、上述守卫拦不住——直接 splice 会把字节插进相邻字符中间。落笔前
        // 核对插入点两侧的缓冲区字节确实与模型一致：不一致就退整块写回（内容
        // 正确，只是这一块的写法可能被规范化），绝不写错位。
        if !self.insertion_context_matches_source(block, visible_offset, at, cx) {
            return false;
        }

        let applied = self.buffer.edit(at..at, &inserted);
        self.record_buffer_edit(applied);
        let delta = inserted.len() as i64;
        // 先平移别人的、再写自己这一根的区间：区间是零宽的块（回车刚建出来的
        // 空块）新起点正好等于 span.end，反过来写就会被 `shift` 二次平移，
        // 之后每个字符都以错一字节的锚点落笔（源码模式打字打成乱码的根因）。
        self.shift_root_spans_after(span.end, delta);
        self.document
            .set_source_span(root.entity_id(), span.start..span.end + delta as usize);
        true
    }

    /// 插入点两侧的缓冲区字节与模型一致吗？
    ///
    /// 映射声称「可见偏移 n 的插入点在源码字节 `at`」。这个换算建立在前缀规范
    /// 的前提上；前提不成立（非规范前缀、解析期账过期）时 `at` 会漂移。核对
    /// 办法：插入点之前的模型字节（同行）必须与缓冲区逐字节相同——字节都对上，
    /// splice 就不可能插错；前文跨了行（行首还有没量过的容器前缀）就改核对
    /// 插入点之后的同行字节。两个方向都对不上时返回 false，由调用方退整块写回。
    fn insertion_context_matches_source(
        &self,
        block: &Entity<Block>,
        visible_offset: usize,
        at: usize,
        cx: &App,
    ) -> bool {
        // 比对文本必须与映射的 content 同口径：代码/源码块用 display 文本 + 原样
        // 偏移（`markdown()` 里是含缩进的原始字节，映射表的账记在内容上）；其余
        // 块用 markdown + 映射偏移（与 `current_range_to_markdown_range` 的分派
        // 同一条规则）。
        let (markdown, markdown_offset) = {
            let block_ref = block.read(cx);
            if block_ref.uses_raw_text_editing() || block_ref.kind().is_code_block() {
                let text = block_ref.display_text().to_string();
                let offset = visible_offset.min(text.len());
                (text, offset)
            } else {
                let markdown = block_ref
                    .record
                    .title
                    .markdown_offset_map()
                    .markdown()
                    .to_string();
                let markdown_offset = block_ref
                    .current_range_to_markdown_range(visible_offset..visible_offset)
                    .start;
                (markdown, markdown_offset)
            }
        };
        if markdown_offset > markdown.len() || !markdown.is_char_boundary(markdown_offset) {
            return false;
        }
        // 行内局部核对：模型当前行里、插入点之前/之后的内容字节，必须与缓冲区
        // 里 `at` 两侧逐字节相同。内容字节在模型与缓冲区里是同一份（容器前缀在
        // 内容之前，不参与比较），所以逐行异形前缀的账、多行引用、缩进代码都
        // 不会被误伤；账过期或前缀漂移时两侧对不上，退整块写回。
        let line_start_in_md = markdown[..markdown_offset]
            .rfind('\n')
            .map(|index| index + 1)
            .unwrap_or(0);
        let within_before = &markdown[line_start_in_md..markdown_offset];
        if !within_before.is_empty() {
            return at >= within_before.len()
                && self.buffer.is_char_boundary(at - within_before.len())
                && self.buffer.slice(at - within_before.len()..at) == within_before;
        }
        // 行首插入（前文为空）有「行首 vs 内容起点」的歧义，且过期账在行首的
        // 漂移方向不定：放行，交给既有的越界/边界守卫与整块写回兜底。
        true
    }

    /// 把 `new_text` 与该区间现有字节的**最小差异**写进缓冲区，返回长度差（新 − 旧）。
    ///
    /// 拆块只是在光标处多一个分隔空行，可「整段重贴」会把这一块里几百行没碰过的
    /// 字节也重新写一遍：撤销组因此记下这一整块（86 KiB 的段落走 200 步就是 17 MB），
    /// 而且序列化写法与原文不同的地方会被顺手规范化（`1)`→`1.`、`__粗__`→`**粗**`）。
    /// 区间写回的承诺就是「改动以外不动」，那就连真正写下去的字节也照这个来。
    fn write_minimal_diff(&mut self, range: std::ops::Range<usize>, new_text: &str) -> i64 {
        let old = self.buffer.slice(range.clone());
        let common_prefix = old
            .bytes()
            .zip(new_text.bytes())
            .take_while(|(before, after)| before == after)
            .count();
        let prefix = (0..=common_prefix)
            .rev()
            .find(|count| old.is_char_boundary(*count) && new_text.is_char_boundary(*count))
            .unwrap_or(0);
        let rest_old = &old[prefix..];
        let rest_new = &new_text[prefix..];
        let common_suffix = rest_old
            .bytes()
            .rev()
            .zip(rest_new.bytes().rev())
            .take_while(|(before, after)| before == after)
            .count();
        let suffix = (0..=common_suffix)
            .rev()
            .find(|count| {
                rest_old.is_char_boundary(rest_old.len() - count)
                    && rest_new.is_char_boundary(rest_new.len() - count)
            })
            .unwrap_or(0);

        let from = range.start + prefix;
        let to = range.end - suffix;
        let middle = &new_text[prefix..new_text.len() - suffix];
        let applied = self.buffer.edit(from..to, middle);
        self.record_buffer_edit(applied);
        new_text.len() as i64 - old.len() as i64
    }

    /// 返回 `false` 表示这个块没有可用区间（新建的块、子块、整篇重投影后没被
    /// 记到的空块）：那种情况下按区间写会把字节落错位置，调用方必须退回
    /// [`Self::resync_buffer_from_projection`]。
    pub(crate) fn write_back_block_source(
        &mut self,
        block: &Entity<Block>,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(old_span) = self.document.source_span_of(block.entity_id()) else {
            return false;
        };
        // 区间属于**在树里**的块：事件流程里「先摘块、后收尾」的路径会把已删除的
        // 块传过来，拿幽灵块的旧区间写缓冲区等于复活已删内容。找不到位置就退回
        // 整篇重投影。
        if self.document.find_block_location(block.entity_id()).is_none() {
            return false;
        }
        // 区间是这一根块上一次写回时挂上的字节位；中途有人（比如标注结构的归一化）
        // 自己重投影了缓冲区，这个区间就是过期的。拿过期区间去写会越界，宁可退回
        // 整篇重投影。
        if old_span.end > self.buffer.byte_len() || !self.buffer.is_char_boundary(old_span.end) {
            return false;
        }
        let new_source = self.document.block_markdown_source(block, cx);
        if self.buffer.slice(old_span.clone()) == new_source {
            // 投影刷新但内容没变：不动缓冲区，区间照旧有效。
            return true;
        }

        let old_len = old_span.end - old_span.start;
        let delta = self.write_minimal_diff(old_span.clone(), &new_source);
        let new_span = old_span.start..old_span.start + (old_len as i64 + delta) as usize;
        // 字节刚按本块账本拼出来重写：写侧认账的族（段落/标题/列表/代码）账随
        // 字节仍然一致，把新区间连着账一起落——不重记的话旧账在下一次落笔时
        // 行内漂移（过期账的错位，越界守卫拦不住）。写侧不认账的族（引用等，
        // 账由解析期与 adjust 路径自管）不动账，过期由落笔前的上下文核对兜住。
        let ledger = {
            let block_ref = block.read(cx);
            Self::written_line_ledger(block_ref)
        };
        // 同 `write_back_visible_insertion`：先平移再写自己这一根的区间，否则
        // 零宽区间（回车新建的空块）会被平移两遍。
        if delta != 0 {
            self.shift_root_spans_after(old_span.end, delta);
        }
        self.document.set_source_span(block.entity_id(), new_span);
        if let Some((prefixes, fence)) = ledger {
            block.update(cx, |block, _cx| {
                block.record.source_line_prefixes = prefixes;
                block.record.source_fence_lines = fence;
                block.record.source_separator_bytes = 0;
            });
        }
        true
    }

    /// 这一块刚按模型的写法落笔（`write_back_root_region` 拼的就是这份文本），它每一行
    /// 让开几字节也就是拼出来的那一段：根块的前缀是记号（`# `、`- [ ] `、`1)`），代码块
    /// 还多一对顶格的围栏行。认不出形状的那一族返回 `None`，账留着空、交回按文件量。
    fn written_line_ledger(block_ref: &Block) -> Option<(Vec<u32>, Option<(u32, u32)>)> {
        let kind = block_ref.record.kind.clone();
        if kind.is_code_block() {
            let content = block_ref.record.title.visible_text();
            let lines = if content.is_empty() {
                0
            } else {
                content.split('\n').count()
            };
            if block_ref.record.code_is_indented {
                // 缩进那一族没有围栏行，落笔补的就是账上那几位（账缺了按四格，与
                // `collect_single_block_markdown_lines` 那条分支同一个口径）。
                let prefixes = block_ref.record.source_line_prefixes.clone();
                let widths: Vec<u32> = if prefixes.len() == lines {
                    prefixes
                } else {
                    vec![4; lines]
                };
                return Some((widths, None));
            }
            // 围栏那一族：落笔文本是按本块现有账拼的（`recorded_code_block_widths`），
            // 重记也必须用同一份账——硬编码「顶格 (0,0)」会让缩进围栏在「结构写回 →
            // 重记 → 再写回」两步之间被洗掉缩进。账不完整（没记过围栏行/行数不齐）
            // 交回 None，写侧走它自己的族内回退。
            let prefixes = block_ref.record.source_line_prefixes.clone();
            if prefixes.len() == lines && let Some(fence) = block_ref.record.source_fence_lines {
                return Some((prefixes, Some(fence)));
            }
            return None;
        }
        let simple = kind == BlockKind::Paragraph
            || matches!(kind, BlockKind::Heading { .. })
            || kind.is_list_item();
        let markdown = block_ref.record.title_markdown();
        if !simple || markdown.contains('\n') {
            return None;
        }
        let line = block_ref.record.markdown_line(0, block_ref.list_ordinal);
        Some((vec![line.len().saturating_sub(markdown.len()) as u32], None))
    }

    /// 把 `block` 的源码区间换成新的，并让它连子块在解析期记下的记号宽度一起作废。
    ///
    /// 这一段字节被重新写过（拆块、合块、Setext 提成 ATX……），块自己那一行的形状
    /// 可能跟着变了：旧账让每个块内偏移整体漂几位（实测报内容末尾在第 9 位，文件里在
    /// 11 位）。容器的行也一样会把改动传给子块（`>` 写成 `>>` 时每行的继承量都变），
    /// 所以递归到子块。清掉之后位置换算交回按文件量那一条，直到重新解析给它新的账。
    fn reanchor_record_span(
        &mut self,
        block: &Entity<Block>,
        span: std::ops::Range<usize>,
        cx: &mut Context<Self>,
    ) {
        let children = block.read(cx).children.clone();
        let ledger = Self::written_line_ledger(block.read(cx));
        self.document.set_source_span(block.entity_id(), span);
        block.update(cx, |block, _cx| {
            block.record.source_separator_bytes = 0;
            match ledger {
                Some((prefixes, fence)) => {
                    block.record.source_line_prefixes = prefixes;
                    block.record.source_fence_lines = fence;
                }
                None => block.record.source_line_prefixes.clear(),
            }
        });
        for child in children {
            Self::invalidate_record_prefixes(&child, cx);
        }
    }

    fn invalidate_record_prefixes(block: &Entity<Block>, cx: &mut Context<Self>) {
        let children = block.read(cx).children.clone();
        block.update(cx, |block, _cx| {
            block.record.source_line_prefixes.clear();
            // 围栏行的账一并清：留半份账（有围栏行没前缀）会让写侧的核对永远
            // 失配，掉进 list_depth 规范分支把缩进洗掉。
            block.record.source_fence_lines = None;
            block.record.source_separator_bytes = 0;
        });
        for child in children {
            Self::invalidate_record_prefixes(&child, cx);
        }
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
        // 这一段要写的文本是按**渲染态的接缝规则**拼的：根块之间空一行、代码块补一对
        // 围栏。源码/代码文档的根块之间只隔一个换行，文件里也没有围栏行——拿这套拼法
        // 写下去会把用户的纯文本改成 markdown。这一档对源码文档不适用，结构命令仍走
        // 整篇重投影（`raw_source_text` 那份拼接才是它的接缝口径）。
        if self.view_mode == ViewMode::Source {
            return false;
        }
        let Some(anchor_index) = before.iter().position(|(id, _)| *id == anchor.entity_id()) else {
            return false;
        };
        let after = self.document.root_layout();
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
        // 与 `write_back_block_source` 同一条守卫：区间来自上一次写回，中途缓冲区
        // 被别的路径重投影过就过期了，过期区间写下去要么写错位置要么越界。
        if region_end > self.buffer.byte_len() || region_start > region_end {
            return false;
        }

        let Some((text, local_spans)) =
            self.document.markdown_region_for_roots(anchor_index..new_end, cx)
        else {
            // 换进去的那一段全是空段落（删掉整张表、原位留一个空段落）：空段落
            // 序列化不出文本，但文件里要改的字节很清楚——把旧的这几行连它们自己的
            // 换行一起收掉，接缝上剩下的空行就是那些空段落。
            return self.write_back_blank_run(
                region_start,
                region_end,
                new_end - anchor_index,
                old_end < before.len(),
            );
        };

        // 纯插入空段落（单元格里按回车是在块后加一段）：旧的一块没少，只往里加
        // 空内容，文件里多出来的就只有空行。这时整段重写会把锚点自己的字节重排
        // 一遍（表格的列宽填充就是在这里被洗掉的），所以只把「序列化多出来的那
        // 几个换行」插在锚点行之后。接缝不合这个形状（锚点是文档最后一段、接缝
        // 本来不空着一行、插进去的带内容）都退回整篇重投影兜底。
        if old_end == anchor_index + 1
            && new_end > old_end
            && after[anchor_index].0 == anchor.entity_id()
            && old_end < before.len()
        {
            let added = self
                .document
                .markdown_region_for_roots(anchor_index..old_end, cx)
                .and_then(|(old_text, _)| text.strip_prefix(old_text.as_str()).map(str::to_string))
                .unwrap_or_default();
            let seam_is_one_blank = self.buffer.byte_at(region_end) == Some(b'\n')
                && self.buffer.byte_at(region_end + 1) == Some(b'\n')
                && self.buffer.byte_at(region_end + 2).is_some_and(|byte| byte != b'\n');
            if !added.is_empty()
                && added.bytes().all(|byte| byte == b'\n')
                && seam_is_one_blank
            {
                let applied = self.buffer.edit(region_end + 1..region_end + 1, &added);
                self.record_buffer_edit(applied);
                self.shift_root_spans_after(region_end + 1, added.len() as i64);
                return true;
            }
        }

        // 整段被删空时要连分隔一起收掉：接缝处不该留下两个连续空行，文档开头也不该
        // 以空行开始，否则删一个空段落会在文件里留下多余空行。
        let (region_start, region_end) = if text.is_empty() {
            let total = self.buffer.byte_len();
            let mut start = region_start;
            // 先吃掉这一行自己的换行（区间右端点本来就停在换行之前）。
            let mut end = if self.buffer.byte_at(region_end) == Some(b'\n') {
                region_end + 1
            } else {
                region_end
            };
            let blank_before = start >= 2
                && self.buffer.byte_at(start - 1) == Some(b'\n')
                && self.buffer.byte_at(start - 2) == Some(b'\n');
            if blank_before {
                if self.buffer.byte_at(end) == Some(b'\n') {
                    // 前面本来就空着一行：把后面的分隔也吃掉一行。
                    end += 1;
                } else if end == region_end && end < total {
                    // 后面没有分隔可吃（删的是文档最后一段）：退回去吃前面那行空行。
                    start -= 1;
                }
            } else if start == 0 && self.buffer.byte_at(end) == Some(b'\n') {
                // 删的是文档开头，紧跟其后的空行一起收掉。
                end += 1;
            }
            (start, end)
        } else {
            (region_start, region_end)
        };
        if region_end <= region_start {
            // 有文字要写却算不出非空区间（例如这一块的区间已经被折叠成零宽）：
            // 交给调用方兜底。整段删空又没有分隔可收（空文档里那个空段落）才是
            // 「没字节要写」，区间照旧有效。
            return text.is_empty();
        }

        let delta = self.write_minimal_diff(region_start..region_end, &text);
        // 先平移再分配：新块区间的右端可能已经越过 region_end（拆块会变长）。
        if delta != 0 {
            self.shift_root_spans_after(region_start, delta);
        }
        let assigned: Vec<EntityId> = local_spans.iter().map(|(id, _)| *id).collect();
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
            self.reanchor_record_span(&block, span, cx);
        }
        // 拆块拆出的空块在段首：空段落序列化不出字节，分不到区间，留着旧的整块
        // 区间就是过期区间——下一次按区间写会把字节落错位置。它在文件里就是接缝上
        // 的那一行，记零宽在段首。
        if !assigned.contains(&anchor.entity_id())
            && text.starts_with('\n')
            && self.document.source_span_of(anchor.entity_id()).is_some()
        {
            self.document
                .set_source_span(anchor.entity_id(), region_start..region_start);
        }
        // 这一段里的区间接缝被重新分配过：落在里面的块要重算派生数据（大纲摘要）。
        // 块的字节可以一个字没动，但它现在指着的是另一段字节——只问缓冲区「哪些字节
        // 被编辑过」看不到这一类。
        let covered = region_start..(region_start + text.len()).max(region_end);
        self.note_outline_dirty_region(covered.start..covered.end.min(self.buffer.byte_len()));
        true
    }

    /// 换成「只剩空段落」的那一段怎么写回：把旧的这几行连它们自己的换行收掉，
    /// 接缝上剩下的空行就是那些空段落——比该有的多就一起吃掉，少就补换行。该有
    /// 几行按序列化的接缝规则：后面还跟着别的根块时多留一个分隔空行。
    fn write_back_blank_run(
        &mut self,
        region_start: usize,
        region_end: usize,
        blank_roots: usize,
        has_next_root: bool,
    ) -> bool {
        if blank_roots == 0 {
            return false;
        }
        // 区间左边紧挨着的空行：往左数连续的换行，最靠近内容的那个是上一行的
        // 结束符，不算空行。
        let mut blanks_before = 0usize;
        let mut cursor = region_start;
        while cursor > 0 && self.buffer.byte_at(cursor - 1) == Some(b'\n') {
            cursor -= 1;
            blanks_before += 1;
        }
        let blanks_before = blanks_before.saturating_sub(1);
        // 区间右边紧挨着的空行：先跳过这一行自己的结束符，再往右数连续的换行。
        let mut line_end = region_end;
        if self.buffer.byte_at(line_end) == Some(b'\n') {
            line_end += 1;
        }
        let mut blanks_after = 0usize;
        while self.buffer.byte_at(line_end + blanks_after) == Some(b'\n') {
            blanks_after += 1;
        }
        let want = blank_roots + usize::from(has_next_root);
        let have = blanks_before + blanks_after;
        let eaten = line_end + have.saturating_sub(want);
        let filler = "\n".repeat(want.saturating_sub(have));
        let applied = self.buffer.edit(region_start..eaten, &filler);
        self.record_buffer_edit(applied);
        let delta = filler.len() as i64 - (eaten - region_start) as i64;
        if delta != 0 {
            self.shift_root_spans_after(eaten, delta);
        }
        true
    }

    /// 一次编辑落在某根块自己身上：它的区间尾部跟着涨/缩 `delta`，
    /// 编辑点（编辑前坐标 `from`）之后的块整体平移。
    fn grow_root_span_after_edit(&mut self, block: &Entity<Block>, delta: i64, from: usize) {
        if let Some(span) = self.document.source_span_of(block.entity_id()) {
            self.document.set_source_span(
                block.entity_id(),
                span.start..(span.end as i64 + delta).max(span.start as i64) as usize,
            );
        }
        self.shift_root_spans_after(from, delta);
    }

    /// 编辑点之后的区间整体平移；之前的块字节没被碰到，区间自然不动。
    /// 表在块树那边（`DocumentTree::shift_source_spans_after`），一趟紧凑循环——
    /// 以前逐块 `read`+`update` 10 万根实体，实测一次按键 57ms。
    fn shift_root_spans_after(&mut self, from: usize, delta: i64) {
        let total = self.buffer.byte_len();
        self.document.shift_source_spans_after(from, delta, total);
    }

    /// 结构命令的写回入口：先看根块序列变了没有。
    ///
    /// - 变了（段落一分为二、两块合一）→ 只重写被换掉的那一段区间。
    /// - 没变（子块拆合、换块种类、勾任务框）→ 整根块重投影，它的区间本来就
    ///   盖住整棵子树，别的根块照样不动。
    pub(crate) fn write_back_structural_change(
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
            .eq(self.document.root_layout().into_iter().map(|(id, _)| id));
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

    /// 块树变了：把它的序列化换进缓冲区（除非这次改动自己声明过区间）。读取侧
    /// 全部以缓冲区为准，所以区间也要按这份新文本重挂，否则坐标会漂。
    ///
    /// 这是写回的保底档位，代价是**未编辑的块也被重新序列化一次**（表格列宽填充、
    /// `__` 强调这些写法就此改写），原始字节也随之丢弃。每多一条走到这里的路径，
    /// 就少一块「保住原文」的地盘——收敛方向是让改动自己声明区间，不是让这里变快。
    pub(crate) fn resync_buffer_from_projection(&mut self, cx: &mut Context<Self>) {
        let skip_resync = std::mem::take(&mut self.skip_next_resync);
        if self.writes_through_the_buffer() {
            if !skip_resync {
                let started = std::time::Instant::now();
                let (text, block_spans) = self.document.markdown_text_with_block_spans(cx);
                // 性能闸门盯的就是这一遍：改动自己声明了区间（`skip_resync`）时
                // 一次按键应该是 0 遍整篇序列化。
                self.source_serializations
                    .set(self.source_serializations.get() + 1);
                self.source_serialization_nanos.set(
                    self.source_serialization_nanos.get()
                        + started.elapsed().as_nanos().min(u64::MAX as u128) as u64,
                );
                self.apply_resynced_text(&text);
                // 区间只有在它派生自的那份文本里才成立：跳过重投影时也别动区间，
                // 写回路径已经把区间按缓冲区字节摆好了。
                self.reattach_root_spans(&block_spans, &text);
            }
        } else if !skip_resync {
            // 源码/代码文档：缓冲区装的是不套围栏的源码文本。这一遍同样是 O(文档)
            // 的整篇落笔，闸门必须看得见（实测 1 MiB 的源码模式文档一次按键 2.28 秒，
            // 以前这里连计数都不加，白名单空表反而掩盖了它）。
            let started = std::time::Instant::now();
            let text = self.document.raw_source_text(cx);
            self.source_serializations
                .set(self.source_serializations.get() + 1);
            self.source_serialization_nanos.set(
                self.source_serialization_nanos.get()
                    + started.elapsed().as_nanos().min(u64::MAX as u128) as u64,
            );
            self.apply_resynced_text(&text);
            // 整篇落笔之后区间要重挂：缓冲区刚被换成块树拼出来的那份文本。
            self.reattach_source_document_spans(cx);
        }
    }

    /// 把重投影出来的文本作为**一次**写入落进缓冲区。
    ///
    /// 走 `edit` 而不是整个换掉缓冲区，是为了让撤销组拿到它的逆操作；文本没变时
    /// 什么都不做，「打开后没改过」那份原始字节也就保住了。
    fn apply_resynced_text(&mut self, text: &str) {
        let text = self.resynced_text(text);
        if self.buffer.matches_text(text.as_ref()) {
            return;
        }
        // 仍然只写最小差异：整篇重投影这一遍已经把全文序列化过了，代价付了就付，
        // 但撤销组不该因此再存一份全文副本（文档 86 KiB × 200 步 = 17 MB）。
        let range = 0..self.buffer.byte_len();
        self.write_minimal_diff(range, text.as_ref());
    }

    /// 整篇落笔的文本口径：序列化把每根块当「一行」，行尾那个换行不在它的产物里，
    /// 而缓冲区原本以换行结尾时必须补回来——否则走一次整篇落笔就吃掉文件的末行换行
    /// （CRLF 文件连带少一个 `\r`），那是用户没改过的字节。
    ///
    /// `apply_resynced_text` 与显式的 `format_document` 共用这一条，两边对「目标文本
    /// 长什么样」的理解必须一致，否则「有没有变化」这个问题会答错（答错就多留一条空
    /// 撤销组、把文档标脏）。
    fn resynced_text<'a>(&self, text: &'a str) -> std::borrow::Cow<'a, str> {
        if text.ends_with('\n')
            || self.buffer.byte_len() == 0
            || self.buffer.byte_at(self.buffer.byte_len() - 1) != Some(b'\n')
        {
            return std::borrow::Cow::Borrowed(text);
        }
        let mut kept = String::with_capacity(text.len() + 1);
        kept.push_str(text);
        kept.push('\n');
        std::borrow::Cow::Owned(kept)
    }

    /// 按重投影出来的文本重建所有根块的源码区间。
    fn reattach_root_spans(
        &mut self,
        block_spans: &[(EntityId, std::ops::Range<usize>)],
        text: &str,
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
            match span {
                Some(span) => self.document.set_source_span(block.entity_id(), span),
                None => self.document.clear_source_span(block.entity_id()),
            }
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
            editor.replace_document_content(
                snapshot.markdown,
                None,
                code_language,
                file_drop::ImportKind::Open,
                cx,
            );
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
