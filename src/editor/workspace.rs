//! Lightweight workspace panel state, file-tree scanning, and outline parsing.

pub(super) use std::collections::{HashMap, HashSet};
pub(super) use std::fs;
pub(super) use std::ops::Range;
pub(super) use std::path::{Path, PathBuf};
pub(super) use std::time::Duration;

pub(super) use anyhow::{Context as _, Result};
pub(super) use gpui::*;
pub(super) use pulldown_cmark::{Event, LinkType, Options, Parser, Tag};
pub(super) use unicode_segmentation::UnicodeSegmentation;

pub(super) use super::{BlockKind, CursorLocation, Editor, UndoSelectionSnapshot, CURSOR_HISTORY_LIMIT};
pub(super) use crate::editor::modal::ModalSpec;
pub(super) use crate::components::{CursorHistoryBack, CursorHistoryForward, TocEntry};
pub(super) use crate::config::TreeSortPreference;
pub(super) use crate::i18n::{I18nManager, I18nStrings};
pub(super) use crate::theme::{Theme, ThemeManager};

const FOLDER_ICON: &str = "icon/workspace/folder.svg";
const ACTIVITY_FILES_ICON: &str = "icon/workspace/activity-files.svg";
const ACTIVITY_SEARCH_ICON: &str = "icon/workspace/activity-search.svg";
const ACTIVITY_OUTLINE_ICON: &str = "icon/workspace/activity-outline.svg";
const ACTIVITY_BACKLINKS_ICON: &str = "icon/workspace/activity-backlinks.svg";
const ACTIVITY_TAGS_ICON: &str = "icon/workspace/activity-tags.svg";
const MARKDOWN_ICON: &str = "icon/workspace/markdown.svg";
const CODE_ICON: &str = "icon/workspace/code.svg";
const CHEVRON_RIGHT_ICON: &str = "icon/workspace/chevron-right.svg";
const CHEVRON_DOWN_ICON: &str = "icon/workspace/chevron-down.svg";
const TAB_CLOSE_ICON: &str = "icon/workspace/tab-close.svg";
const GENERIC_FILE_ICON: &str = "icon/workspace/generic-file.svg";
const WORKSPACE_NODE_HEIGHT: f32 = 24.0;
const WORKSPACE_NODE_INDENT: f32 = 16.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum WorkspaceTab {
    #[default]
    Files,
    Search,
    Outline,
    Backlinks,
    Tags,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum WorkspaceTreeKind {
    Directory(PathBuf),
    MarkdownFile(PathBuf),
    CodeFile(PathBuf),
    /// Any non-Markdown, non-code file. Shown in the tree for completeness;
    /// clicking it reports that the type can't be opened yet.
    OtherFile(PathBuf),
    Heading { line: usize, level: u8 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WorkspaceTreeNode {
    id: String,
    label: String,
    kind: WorkspaceTreeKind,
    children: Vec<WorkspaceTreeNode>,
}

struct WorkspaceTooltip {
    label: String,
}

impl Render for WorkspaceTooltip {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<ThemeManager>().current_arc();
        div()
            .px(px(8.0))
            .py(px(5.0))
            .rounded(px(6.0))
            .bg(theme.colors.dialog_surface)
            .border_1()
            .border_color(theme.colors.dialog_border)
            .shadow_md()
            .text_size(px(12.0))
            .text_color(theme.colors.dialog_title)
            .child(self.label.clone())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WorkspaceDocumentTab {
    path: PathBuf,
    recovery_id: uuid::Uuid,
    file_version: u64,
    markdown: String,
    dirty: bool,
    /// 预览标签（用户需求）：单击树节点打开，切换到其它文件时未修改的
    /// 预览标签被替换、不再占据标签栏；双击打开或产生修改后转为固定展示。
    preview: bool,
}

/// 打开文件时的标签模式（用户需求：单击预览、双击固定）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkspaceOpenMode {
    /// 仅激活已有标签（点标签栏）：不改动其固定/预览状态。
    Activate,
    /// 单击打开：新建预览标签；已修改的预览不会被后续切换替换。
    Preview,
    /// 双击/常规路径打开：新建固定标签；已有的预览标签升级为固定。
    Pinned,
}

#[derive(Clone, Copy)]
struct WorkspaceContextMenu {
    position: Point<Pixels>,
    has_target: bool,
}

#[derive(Clone, Copy)]
struct WorkspaceResizeDrag {
    start_x: f32,
    start_width: f32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WorkspaceSearchHit {
    path: PathBuf,
    label: String,
    line: Option<usize>,
    /// Byte range of the match inside its line (used to select the match when
    /// jumping and to open the hit's file at the right spot).
    match_range: Option<Range<usize>>,
    /// Absolute byte range in the current document (document-scope hits only).
    source_range: Option<Range<usize>>,
    preview: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum WorkspaceSearchScope {
    #[default]
    Workspace,
    Document,
}

#[derive(Clone, Copy)]
enum WorkspaceMenuAction {
    NewFile,
    NewFolder,
    Duplicate,
    Copy,
    Paste,
    Rename,
    Delete,
}

/// Drag payload for reordering document tabs (roadmap E3).
#[derive(Clone)]
pub(super) struct TabDrag {
    pub(super) from_path: PathBuf,
}

/// 拖拽预览视图：被拖动标签的文件名。
pub(super) struct DraggedTabPreview {
    pub(super) label: SharedString,
}

impl Render for DraggedTabPreview {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<ThemeManager>().current_arc();
        let c = &theme.colors;
        div()
            .px(px(10.0))
            .py(px(4.0))
            .rounded(px(6.0))
            .bg(c.dialog_surface)
            .border_1()
            .border_color(c.dialog_border)
            .shadow_md()
            .text_size(px(12.0))
            .text_color(c.text_default)
            .child(self.label.clone())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TabMenuAction {
    Close,
    CloseOthers,
    CloseLeft,
    CloseRight,
    CloseAll,
}

#[derive(Clone, Copy)]
struct TabContextMenu {
    position: Point<Pixels>,
    target_index: usize,
}

/// Which search-panel input a key/IME event targets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SearchInputKind {
    Query,
    Replace,
}

/// Which single-line overlay input the editor's input handler serves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OverlayInputKind {
    Query,
    Replace,
    QuickOpen,
    CommandPalette,
}

impl From<SearchInputKind> for OverlayInputKind {
    fn from(kind: SearchInputKind) -> Self {
        match kind {
            SearchInputKind::Query => Self::Query,
            SearchInputKind::Replace => Self::Replace,
        }
    }
}

pub(crate) struct WorkspaceAutosaveDocument {
    pub(super) recovery_id: uuid::Uuid,
    pub(super) file_version: u64,
    pub(super) path: PathBuf,
    pub(super) markdown: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum WorkspaceSelection {
    Directory(PathBuf),
    File(PathBuf),
    WorkspaceRoot(PathBuf),
    Outline(String),
}

pub(super) struct WorkspaceState {
    pub(super) is_open: bool,
    pub(super) active_tab: WorkspaceTab,
    pub(super) root: Option<PathBuf>,
    file_tree: Option<WorkspaceTreeNode>,
    file_error: Option<String>,
    /// 外部修改冲突（自动保存检测到磁盘内容变了）：独立于 `file_error`，
    /// 只有该文件被重新加载才清除，扫描或普通错误清空不得影响它。
    external_change_conflict: Option<(PathBuf, String)>,
    outline_tree: Vec<WorkspaceTreeNode>,
    outline_source: Option<String>,
    /// 扁平标题清单（roadmap C2）：供正文里的 `[TOC]` 块渲染目录。
    toc_entries: Vec<TocEntry>,
    expanded: HashSet<String>,
    /// 外部文件事件的树刷新防抖代数（见 `schedule_workspace_tree_refresh`）。
    tree_refresh_generation: u32,
    selected: Option<WorkspaceSelection>,
    open_documents: Vec<WorkspaceDocumentTab>,
    active_document: Option<PathBuf>,
    pub(super) search_query: String,
    search_scope: WorkspaceSearchScope,
    search_active_index: Option<usize>,
    search_selected_range: Range<usize>,
    search_marked_range: Option<Range<usize>>,
    replace_query: String,
    replace_visible: bool,
    replace_focus: Option<FocusHandle>,
    replace_selected_range: Range<usize>,
    replace_marked_range: Option<Range<usize>>,
    search_match_case: bool,
    search_whole_word: bool,
    search_use_regex: bool,
    search_fuzzy: bool,
    search_focus: Option<FocusHandle>,
    search_focus_pending: bool,
    search_results: Vec<WorkspaceSearchHit>,
    document_search_source: Option<String>,
    document_active_range: Option<Range<usize>>,
    search_pending: bool,
    search_generation: u64,
    /// 顶栏标签条横向滚动（诊断/断言用）。
    pub(crate) tabs_scroll_handle: ScrollHandle,
    /// 文件树后台扫描（roadmap D9）：任务句柄 + 代数，用于丢弃过期结果。
    tree_scan_task: Option<Task<()>>,
    tree_scan_generation: u64,
    /// 当前已持有（或正在等待）扫描结果的根：避免每帧重复发起扫描。
    tree_scan_root: Option<PathBuf>,
    context_menu: Option<WorkspaceContextMenu>,
    tab_context_menu: Option<TabContextMenu>,
    /// 文件树过滤框（roadmap D8）：非空时树显示扁平匹配列表。
    pub(super) tree_filter: String,
    tree_filter_focus: Option<FocusHandle>,
    panel_width: Option<f32>,
    resize_drag: Option<WorkspaceResizeDrag>,
    /// 侧栏文件树滚动位置：让「点文件后重扫不跳回顶部」可断言（用户报修）。
    pub(crate) tree_scroll_handle: ScrollHandle,
}

impl Default for WorkspaceState {
    fn default() -> Self {
        Self {
            // 应用启动不展开侧边栏（用户需求）：需要时点状态栏按钮/快捷键打开。
            is_open: false,
            active_tab: WorkspaceTab::Files,
            root: None,
            file_tree: None,
            file_error: None,
            external_change_conflict: None,
            outline_tree: Vec::new(),
            outline_source: None,
            toc_entries: Vec::new(),
            expanded: HashSet::new(),
            tree_refresh_generation: 0,
            selected: None,
            open_documents: Vec::new(),
            active_document: None,
            search_query: String::new(),
            search_scope: WorkspaceSearchScope::Workspace,
            search_active_index: None,
            search_selected_range: 0..0,
            search_marked_range: None,
            replace_query: String::new(),
            replace_visible: false,
            replace_focus: None,
            replace_selected_range: 0..0,
            replace_marked_range: None,
            search_match_case: false,
            search_whole_word: false,
            search_use_regex: false,
            search_fuzzy: false,
            search_focus: None,
            search_focus_pending: false,
            search_results: Vec::new(),
            document_search_source: None,
            document_active_range: None,
            search_pending: false,
            search_generation: 0,
            tabs_scroll_handle: ScrollHandle::new(),
            tree_scan_task: None,
            tree_scan_generation: 0,
            tree_scan_root: None,
            context_menu: None,
            tab_context_menu: None,
            tree_filter: String::new(),
            tree_filter_focus: None,
            panel_width: None,
            resize_drag: None,
            tree_scroll_handle: ScrollHandle::new(),
        }
    }
}

/// 后台任务里往某个编辑器窗口弹应用内模态（取代系统原生弹窗）。
fn show_async_message_modal(
    window_handle: AnyWindowHandle,
    cx: &mut AsyncApp,
    build: impl FnOnce(&crate::i18n::I18nStrings) -> (String, String),
) {
    let Some(handle) = window_handle.downcast::<Editor>() else {
        return;
    };
    let _ = handle.update(cx, move |editor, _window, cx| {
        let strings = cx.global::<crate::i18n::I18nManager>().strings().clone();
        let (title, detail) = build(&strings);
        editor.show_message_modal(title, detail, cx);
    });
}

/// 「<名> copy[ 序号]」式唯一副本路径（roadmap D6，与右键「创建副本」共用）。
/// 标签路径是否落在工作区根目录内（两边都尽力规范化，失败则按原样比较）。
fn path_is_within_root(root: &Path, path: &Path) -> bool {
    let normalize = |value: &Path| -> PathBuf {
        std::fs::canonicalize(value).unwrap_or_else(|_| value.to_path_buf())
    };
    normalize(path).starts_with(normalize(root))
}

pub(crate) fn unique_workspace_copy_path(dir: &Path, source: &Path) -> PathBuf {
    let stem = source
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let extension = source
        .extension()
        .map(|extension| format!(".{}", extension.to_string_lossy()))
        .unwrap_or_default();
    let mut candidate = dir.join(format!("{stem} copy{extension}"));
    let mut counter = 2u32;
    while candidate.exists() {
        candidate = dir.join(format!("{stem} copy {counter}{extension}"));
        counter += 1;
    }
    candidate
}

fn unique_workspace_file_name(dir: &Path, preferred_name: &str) -> PathBuf {
    let preferred = Path::new(preferred_name);
    let stem = preferred
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("image");
    let extension = preferred
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| format!(".{extension}"))
        .unwrap_or_default();
    let mut candidate = dir.join(preferred_name);
    let mut counter = 2u32;
    while candidate.exists() {
        candidate = dir.join(format!("{stem} {counter}{extension}"));
        counter += 1;
    }
    candidate
}

/// 剪贴板图片字节的 8 位十六进制摘要（与编辑器粘贴命名一致，roadmap D6/B10）。
pub(crate) fn pasted_image_bytes_hash(bytes: &[u8]) -> String {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    format!("{:08x}", hasher.finish() as u32)
}

pub(crate) fn clipboard_image_extension(format: gpui::ImageFormat) -> &'static str {
    match format {
        gpui::ImageFormat::Png => "png",
        gpui::ImageFormat::Jpeg => "jpg",
        gpui::ImageFormat::Webp => "webp",
        gpui::ImageFormat::Gif => "gif",
        gpui::ImageFormat::Svg => "svg",
        gpui::ImageFormat::Bmp => "bmp",
        gpui::ImageFormat::Tiff => "tiff",
    }
}

/// 复制为 HTML 的剪贴板载荷：纯文本仍为 HTML 源码，另加 text/html 富文本
/// flavor 供 Word/浏览器等富文本目标使用（roadmap F2 增强）。
pub(crate) fn copy_as_html_clipboard_item(html: String) -> ClipboardItem {
    ClipboardItem::new_string_with_html(html.clone(), html)
}


// 各子模块的自由函数/类型经此聚合，供根与兄弟模块(含 tests)按原路径引用。
pub(super) use file_tree::*;
pub(super) use input_handler::*;
pub(super) use render_panel::*;
pub(super) use search_backend::*;

mod context_menus;
mod documents;
mod file_tree;
mod find_replace;
mod input_handler;
mod overlay_input;
mod prompts;
mod render_panel;
mod render_search;
mod render_tabs;
mod render_tree;
mod search_backend;
mod session_watcher;
mod sidebar;
mod tabs;
mod tree_ops;
mod tree_sync;

#[cfg(test)]
mod search_bench;

#[cfg(test)]
mod tests;
