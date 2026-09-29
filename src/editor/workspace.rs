//! Lightweight workspace panel state, file-tree scanning, and outline parsing.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context as _, Result};
use gpui::*;
use pulldown_cmark::{Event, LinkType, Options, Parser, Tag};
use unicode_segmentation::UnicodeSegmentation;

use super::{BlockKind, CursorLocation, Editor, UndoSelectionSnapshot, CURSOR_HISTORY_LIMIT};
use crate::editor::modal::ModalSpec;
use crate::components::{CursorHistoryBack, CursorHistoryForward, TocEntry};
use crate::config::TreeSortPreference;
use crate::i18n::{I18nManager, I18nStrings};
use crate::theme::{Theme, ThemeManager};

const FOLDER_ICON: &str = "icon/workspace/folder.svg";
const ACTIVITY_FILES_ICON: &str = "icon/workspace/activity-files.svg";
const ACTIVITY_SEARCH_ICON: &str = "icon/workspace/activity-search.svg";
const ACTIVITY_OUTLINE_ICON: &str = "icon/workspace/activity-outline.svg";
const MARKDOWN_ICON: &str = "icon/workspace/markdown.svg";
const CODE_ICON: &str = "icon/workspace/code.svg";
const CHEVRON_RIGHT_ICON: &str = "icon/workspace/chevron-right.svg";
const CHEVRON_DOWN_ICON: &str = "icon/workspace/chevron-down.svg";
const TAB_CLOSE_ICON: &str = "icon/workspace/tab-close.svg";
const GENERIC_FILE_ICON: &str = "icon/workspace/generic-file.svg";
const WORKSPACE_NODE_HEIGHT: f32 = 24.0;
const WORKSPACE_NODE_INDENT: f32 = 16.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum WorkspaceTab {
    #[default]
    Files,
    Search,
    Outline,
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
pub(super) struct WorkspaceTreeNode {
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
struct WorkspaceDocumentTab {
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
pub(super) enum WorkspaceOpenMode {
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
struct WorkspaceSearchHit {
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
enum TabMenuAction {
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
enum SearchInputKind {
    Query,
    Replace,
}

/// Which single-line overlay input the editor's input handler serves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OverlayInputKind {
    Query,
    Replace,
    QuickOpen,
}

impl From<SearchInputKind> for OverlayInputKind {
    fn from(kind: SearchInputKind) -> Self {
        match kind {
            SearchInputKind::Query => Self::Query,
            SearchInputKind::Replace => Self::Replace,
        }
    }
}

pub(super) struct WorkspaceAutosaveDocument {
    pub(super) recovery_id: uuid::Uuid,
    pub(super) file_version: u64,
    pub(super) path: PathBuf,
    pub(super) markdown: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum WorkspaceSelection {
    Directory(PathBuf),
    File(PathBuf),
    WorkspaceRoot(PathBuf),
    Outline(String),
}

pub(super) struct WorkspaceState {
    pub(super) is_open: bool,
    active_tab: WorkspaceTab,
    root: Option<PathBuf>,
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
    selected: Option<WorkspaceSelection>,
    open_documents: Vec<WorkspaceDocumentTab>,
    active_document: Option<PathBuf>,
    search_query: String,
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

impl Editor {
    /// Opens a welcome-page recent entry: folders replace the working set,
    /// files open as a tab in this window.
    /// Recomputes in-document search highlight ranges (roadmap B2): clears
    /// the previous blocks, then maps every match through the source→content
    /// mappings onto the owning block.
    pub(super) fn sync_document_search_highlights(&mut self, cx: &mut Context<Self>) {
        let previous = std::mem::take(&mut self.search_highlighted_blocks);
        for entity in &previous {
            let _ = entity.update(cx, |block, _| block.search_highlight_ranges.clear());
        }

        let query = self.workspace.search_query.trim().to_string();
        let active = self.workspace.is_open
            && self.workspace.active_tab == WorkspaceTab::Search
            && self.workspace.search_scope == WorkspaceSearchScope::Document
            && !query.is_empty();
        if !active {
            cx.notify();
            return;
        }

        let matcher = SearchMatcher::new(&query, self.search_options());
        let source = self.current_document_source(cx);
        let mappings = self.build_source_target_mappings(cx);
        let mut highlighted = Vec::new();
        for mapping in &mappings {
            let Some(block_source) = source.get(mapping.full_source_range.clone()) else {
                continue;
            };
            let matches = matcher.find_in_line(block_source);
            if matches.is_empty() {
                continue;
            }
            let mut ranges = Vec::with_capacity(matches.len());
            for found in matches {
                let local_start = found.start;
                let local_end = found.end;
                if local_end >= mapping.source_to_content.len() {
                    continue;
                }
                let content_start = mapping.source_to_content[local_start];
                let content_end = mapping.source_to_content[local_end];
                if content_end > content_start {
                    let range = mapping
                        .entity
                        .read(cx)
                        .markdown_range_to_current_range(content_start..content_end);
                    if !range.is_empty() {
                        ranges.push(range);
                    }
                }
            }
            if ranges.is_empty() {
                continue;
            }
            let entity = mapping.entity.clone();
            entity.update(cx, |block, _| block.search_highlight_ranges = ranges);
            highlighted.push(entity);
        }
        self.search_highlighted_blocks = highlighted;
        cx.notify();
    }

    /// Cycles the file tree sort order (roadmap D2) and rescans.
    pub(crate) fn on_cycle_tree_sort(
        &mut self,
        _: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let next = match crate::config::EditorSettings::tree_sort(cx) {
            TreeSortPreference::Name => TreeSortPreference::ModifiedTime,
            TreeSortPreference::ModifiedTime => TreeSortPreference::Type,
            TreeSortPreference::Type => TreeSortPreference::Name,
        };
        crate::config::EditorSettings::set_tree_sort(cx, next);
        self.refresh_workspace_tree(cx);
    }

    /// 标题折叠（roadmap C7）：折叠标题之后的块隐藏，直到同级或更高
    /// 级标题出现。顺带刷新每个标题的 `foldable`（其后方是否有章节内容），
    /// 供标题行内的折叠 chevron 决定是否显示。
    /// 跳到源码行并选中该行（roadmap C2 的 `[TOC]` 条目点击）。
    pub(super) fn jump_to_source_line(&mut self, line: usize, cx: &mut Context<Self>) {
        let source = self.last_stable_source_text.clone();
        let line_start = source
            .split_inclusive('\n')
            .take(line)
            .map(str::len)
            .sum::<usize>()
            .min(source.len());
        let line_end = source[line_start..]
            .find('\n')
            .map(|offset| line_start + offset)
            .unwrap_or(source.len());
        if source.is_char_boundary(line_start) && source.is_char_boundary(line_end) {
            self.jump_to_document_search_range(line_start..line_end, cx);
        }
    }

    pub(super) fn apply_heading_fold_filter(
        &self,
        all: Vec<super::tree::VisibleBlock>,
        cx: &mut Context<Self>,
    ) -> Vec<super::tree::VisibleBlock> {
        for (index, visible) in all.iter().enumerate() {
            // P4b 后这里只在行计划重建时运行，但仍是 O(文档)：先做最廉价
            // 的类型判断，[TOC] 全文 trim 比较只可能命中 Paragraph。
            let kind = visible.entity.read(cx).kind();
            if !matches!(kind, BlockKind::Paragraph) {
                let level = match kind {
                    BlockKind::Heading { level } => level,
                    _ => continue,
                };
                let has_section = all.get(index + 1).is_some_and(|next| {
                    match next.entity.read(cx).kind() {
                        BlockKind::Heading { level: next_level } => next_level > level,
                        _ => true,
                    }
                });
                visible
                    .entity
                    .update(cx, |block, _cx| block.foldable = has_section);
                continue;
            }
            let (is_toc, had_toc) = {
                let block = visible.entity.read(cx);
                (
                    block.display_text().trim().eq_ignore_ascii_case("[toc]"),
                    !block.toc_entries.is_empty(),
                )
            };
            if is_toc {
                let entries = self.workspace.toc_entries.clone();
                visible
                    .entity
                    .update(cx, |block, _cx| block.toc_entries = entries);
            } else if had_toc {
                visible
                    .entity
                    .update(cx, |block, _cx| block.toc_entries.clear());
            }
            let level = match kind {
                BlockKind::Heading { level } => level,
                _ => continue,
            };
            let has_section = all.get(index + 1).is_some_and(|next| {
                match next.entity.read(cx).kind() {
                    BlockKind::Heading { level: next_level } => next_level > level,
                    _ => true,
                }
            });
            visible
                .entity
                .update(cx, |block, _cx| block.foldable = has_section);
        }
        let mut filtered = Vec::with_capacity(all.len());
        let mut hide_below_level: Option<u8> = None;
        for visible in all {
            let block = visible.entity.read(cx);
            match block.kind() {
                BlockKind::Heading { level } => {
                    if let Some(hide) = hide_below_level
                        && level <= hide
                    {
                        hide_below_level = None;
                    }
                    if block.folded {
                        hide_below_level = Some(level);
                    }
                    filtered.push(visible);
                }
                _ => {
                    if hide_below_level.is_none() {
                        filtered.push(visible);
                    }
                }
            }
        }
        filtered
    }

    /// ⌘1-⌘9: focus the Nth document tab (roadmap E5).
    pub(crate) fn on_select_tab_index(
        &mut self,
        action: &crate::components::SelectTabIndex,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let index = usize::from(action.index).checked_sub(1);
        let Some(path) = index
            .and_then(|index| self.workspace.open_documents.get(index))
            .map(|tab| tab.path.clone())
        else {
            return;
        };
        self.open_workspace_file(path, window, cx);
    }

    pub(crate) fn open_recent_entry(
        &mut self,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if path.is_dir() {
            self.set_workspace_root(path.to_path_buf(), cx);
        } else {
            self.open_workspace_file(path.to_path_buf(), window, cx);
        }
    }

    /// Reloads an externally modified open document when clean (roadmap D3).
    pub(super) fn reload_externally_changed_document(
        &mut self,
        path: &Path,
        cx: &mut Context<Self>,
    ) {
        let is_active = self.file_path.as_deref() == Some(path);
        let Some((cached_markdown, dirty)) = self.cached_tab_content_for_path(path, cx) else {
            return;
        };
        if dirty {
            // Unsaved edits win; the save path already detects conflicts.
            return;
        }
        // 外部变更策略（roadmap H2）：manual 模式不自动重载，交由用户手动刷新。
        if crate::config::EditorSettings::external_change_policy(cx)
            == crate::config::ExternalChangePolicy::Manual
        {
            return;
        }
        let Ok(disk) = std::fs::read_to_string(path) else {
            return;
        };
        if disk == cached_markdown {
            return;
        }
        let file_version = super::persistence::file_content_version(&disk);
        if let Some(tab) = self
            .workspace
            .open_documents
            .iter_mut()
            .find(|tab| tab.path == path)
        {
            tab.markdown = disk.clone();
            tab.file_version = file_version;
        }
        if is_active {
            let path = path.to_path_buf();
            if is_markdown_file(&path) {
                self.replace_document_from_markdown(disk, Some(path), cx);
            } else {
                self.replace_document_from_code_source(disk, path, cx);
            }
        }
        cx.notify();
    }

    /// Cached markdown + dirty state for an open document path (watcher).
    pub(super) fn cached_tab_content_for_path(
        &self,
        path: &Path,
        cx: &App,
    ) -> Option<(String, bool)> {
        if self.file_path.as_deref() == Some(path) {
            return Some((self.serialized_document_text(cx), self.document_dirty));
        }
        self.workspace
            .open_documents
            .iter()
            .find(|tab| &tab.path == path)
            .map(|tab| (tab.markdown.clone(), tab.dirty))
    }

    /// Current workspace root, if set.
    pub(super) fn workspace_root_path(&self) -> Option<&Path> {
        self.workspace.root.as_deref()
    }

    /// 原生「打开文件」对话框的起始目录：优先工作区根，其次当前文件所在目录。
    /// 见 `PathPromptOptions::directory`——给了目录，Windows 上壳层就不会回到它记住的
    /// 上次位置（可能已不可达，显示前会卡在那里）。
    pub(crate) fn open_dialog_start_dir(&self) -> Option<PathBuf> {
        self.workspace_root_path()
            .map(Path::to_path_buf)
            .or_else(|| self.workspace_root_for_current_file())
    }

    /// All markdown/code files of the workspace tree, for the quick switcher.
    pub(super) fn workspace_text_files(&self) -> Vec<PathBuf> {
        self.workspace
            .file_tree
            .as_ref()
            .map(|tree| collect_workspace_files(tree))
            .unwrap_or_default()
    }

    /// Outline-follows-scroll (roadmap C5): while the Outline tab is visible,
    /// select the deepest heading at or above the topmost visible block.
    /// Cheap per frame: block bounds are cached by the previous layout and the
    /// source mapping is rebuilt only when the document revision changes.
    pub(super) fn sync_outline_follow_scroll(
        &mut self,
        viewport_top: Pixels,
        cx: &mut Context<Self>,
    ) {
        if !self.workspace.is_open || self.workspace.active_tab != WorkspaceTab::Outline {
            return;
        }
        let offset = f32::from(self.scroll_handle.offset().y);
        if !self.last_outline_follow_offset.is_nan()
            && (offset - self.last_outline_follow_offset).abs() < 2.0
        {
            return;
        }
        self.last_outline_follow_offset = offset;

        let revision = self.document_revision;
        if self
            .outline_follow_cache
            .as_ref()
            .map(|(cached_revision, _, _)| *cached_revision)
            != Some(revision)
        {
            let (_, ranges) = self.build_source_target_mappings_with_block_ranges(cx);
            let source = self.current_document_source(cx);
            let newlines: Vec<usize> = source
                .bytes()
                .enumerate()
                .filter(|(_, byte)| *byte == b'\n')
                .map(|(offset, _)| offset)
                .collect();
            self.outline_follow_cache = Some((revision, ranges, newlines));
        }
        let Some((_, ranges, newlines)) = self.outline_follow_cache.as_ref() else {
            return;
        };

        // First block whose body extends below the viewport top band.
        let cutoff = viewport_top + px(48.0);
        let mut target_source_start: Option<usize> = None;
        for visible in self.document.visible_blocks() {
            let Some(bounds) = visible.entity.read(cx).last_bounds else {
                continue;
            };
            if bounds.bottom() > cutoff {
                if let Some(range) = ranges.get(&visible.entity.entity_id()) {
                    target_source_start = Some(range.start);
                }
                break;
            }
        }
        let Some(source_start) = target_source_start else {
            return;
        };
        let line = newlines.partition_point(|&newline_offset| newline_offset < source_start);

        // Preorder walk visits headings in ascending source line order, so the
        // last heading at or above the target is the deepest enclosing one.
        let mut current: Option<String> = None;
        {
            let tree = &self.workspace.outline_tree;
            fn visit(nodes: &[WorkspaceTreeNode], line: usize, current: &mut Option<String>) {
                for node in nodes {
                    if let WorkspaceTreeKind::Heading {
                        line: heading_line, ..
                    } = &node.kind
                    {
                        if *heading_line <= line {
                            *current = Some(node.id.clone());
                        }
                    }
                    visit(&node.children, line, current);
                }
            }
            visit(tree, line, &mut current);
        }
        if let Some(id) = current
            && self.workspace.selected != Some(WorkspaceSelection::Outline(id.clone()))
        {
            self.workspace.selected = Some(WorkspaceSelection::Outline(id));
            cx.notify();
        }
    }

    /// Persists the open-tab set for session restore (roadmap A4). The JSON
    /// write runs on the background executor: on Windows a synchronous small
    /// write in a click handler stalls the interaction (Defender 实时扫描放大
    /// 延迟，用户报修：打开第二个文件起界面卡顿)。写入用全局锁串行，避免并发
    /// 交错。
    pub(crate) fn persist_session(&mut self, cx: &mut Context<Self>) {
        self.snapshot_current_document(cx);
        let session = crate::config::SessionState {
            root: self
                .workspace
                .root
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned()),
            tabs: self
                .workspace
                .open_documents
                .iter()
                .map(|tab| tab.path.to_string_lossy().into_owned())
                .collect(),
            active: self
                .workspace
                .active_document
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned()),
            sidebar_width: self.workspace.panel_width.map(|width| width.round() as u16),
        };
        let background = cx.background_executor().clone();
        background
            .spawn(async move {
                static SESSION_WRITE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
                let _guard = SESSION_WRITE_LOCK.lock().ok();
                if let Err(error) = crate::config::save_session(&session) {
                    eprintln!("failed to save session: {error}");
                }
            })
            .detach();
    }

    pub(crate) fn set_workspace_root(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        // Canonicalize so recent-folder entries read as real absolute paths
        // (a CLI "." would otherwise be recorded as "<cwd>/.").
        let root = std::fs::canonicalize(&root).unwrap_or(root);
        // 恢复该工作区记忆的侧栏宽度（roadmap E7）。
        if let Ok(session) = crate::config::read_session()
            && session.root.as_deref() == Some(root.to_string_lossy().as_ref())
            && let Some(width) = session.sidebar_width
        {
            self.workspace.panel_width = Some(width as f32);
        }
        if crate::config::record_recent_folder(&root).is_ok()
            && cx.try_global::<ThemeManager>().is_some()
            && cx.try_global::<crate::i18n::I18nManager>().is_some()
        {
            crate::app_menu::install_menus(cx);
        }
        self.workspace.selected = Some(WorkspaceSelection::Directory(root.clone()));
        // 切换工作区 = 换一套工作集：不属于新根目录的标签必须收起（用户报修：
        // 换了工作区之后顶栏还留着上一个工作区的标签）。
        self.prune_workspace_tabs_outside_root(&root);
        self.workspace.root = Some(root);
        self.workspace.file_tree = None;
        // 打开新文件夹必须重新扫描：清掉缓存结果标记（roadmap D9）。
        self.workspace.tree_scan_root = None;
        self.clear_workspace_file_error();
        self.workspace.expanded.clear();
        self.workspace.active_tab = WorkspaceTab::Files;
        self.workspace.search_scope = WorkspaceSearchScope::Workspace;
        self.workspace.search_focus_pending = false;
        self.workspace.search_query.clear();
        self.workspace.search_selected_range = 0..0;
        self.workspace.search_marked_range = None;
        self.workspace.search_results.clear();
        self.workspace.document_search_source = None;
        self.workspace.document_active_range = None;
        self.workspace.search_pending = false;
        self.workspace.search_generation = self.workspace.search_generation.wrapping_add(1);
        self.sync_workspace_file_tree(cx);
        self.sync_workspace_outline(cx);
        let active_root = self.workspace.root.clone();
        if let Some(active_root) = active_root.as_deref() {
            super::watcher::start_watching(self, active_root, cx);
        }
        self.persist_session(cx);
        cx.notify();
    }

    /// 收起落在新工作区之外的标签；脏标签先写回自己的文件，内容不丢。
    /// 若当前文档被收起：还有标签就延后打开最近的那个（需要 `&mut Window`），
    /// 没有标签就清空文档并回到欢迎页。
    fn prune_workspace_tabs_outside_root(&mut self, root: &Path) {
        let stale = self
            .workspace
            .open_documents
            .iter()
            .filter(|tab| !path_is_within_root(root, &tab.path))
            .cloned()
            .collect::<Vec<_>>();
        if stale.is_empty() {
            return;
        }
        let stale_paths = stale
            .iter()
            .map(|tab| tab.path.clone())
            .collect::<Vec<PathBuf>>();
        for tab in &stale {
            if !tab.dirty {
                continue;
            }
            match std::fs::write(&tab.path, tab.markdown.as_str()) {
                Ok(()) => {
                    let _ = crate::config::remove_recovery_snapshot(tab.recovery_id);
                }
                Err(err) => {
                    self.workspace.file_error = Some(format!(
                        "无法保存「{}」：{err}",
                        tab.path.display()
                    ));
                }
            }
        }
        self.workspace
            .open_documents
            .retain(|tab| !stale_paths.contains(&tab.path));

        // 「正在看的那篇」可能只体现在 file_path 上（active_document 由
        // ensure_current_document_tab 在渲染时才补齐），两者都要算。
        let current_was_stale = self
            .file_path
            .clone()
            .or_else(|| self.workspace.active_document.clone())
            .is_some_and(|current| stale_paths.iter().any(|path| *path == current));
        if !current_was_stale {
            return;
        }
        match self
            .workspace
            .open_documents
            .iter()
            .find(|_tab| true)
            .map(|tab| tab.path.clone())
        {
            Some(next) => {
                // 真正的打开动作要等下一帧（那时才拿得到 Window）。
                self.file_path = None;
                self.document_dirty = false;
                self.workspace.active_document = None;
                self.pending_workspace_tab_activation = Some(next);
            }
            None => {
                self.workspace.active_document = None;
                self.file_path = None;
                self.document_dirty = false;
                self.pending_workspace_tab_activation = None;
                self.show_welcome = true;
                self.pending_window_unedited = true;
            }
        }
    }

    fn selected_workspace_directory(&self) -> Option<PathBuf> {
        match self.workspace.selected.as_ref() {
            Some(WorkspaceSelection::Directory(path)) => Some(path.clone()),
            Some(WorkspaceSelection::File(path)) => path.parent().map(Path::to_path_buf),
            Some(WorkspaceSelection::WorkspaceRoot(path)) => Some(path.clone()),
            _ => self.workspace.root.clone(),
        }
    }

    fn refresh_workspace_tree(&mut self, cx: &mut Context<Self>) {
        // 保留旧树直到新扫描落地，避免侧栏在扫描期间闪空。
        self.sync_workspace_file_tree_inner(true, cx);
        if self.workspace.active_tab == WorkspaceTab::Search
            && !self.workspace.search_query.is_empty()
        {
            self.schedule_workspace_search(cx);
        }
        cx.notify();
    }

    fn show_workspace_file_error(
        window_handle: AnyWindowHandle,
        detail: String,
        cx: &mut AsyncApp,
    ) {
        show_async_message_modal(window_handle, cx, move |strings| {
            (strings.open_failed_title.clone(), detail.clone())
        });
    }

    pub(super) fn show_external_change_error(
        window_handle: AnyWindowHandle,
        detail: String,
        cx: &mut AsyncApp,
    ) {
        show_async_message_modal(window_handle, cx, move |strings| {
            (
                strings.external_change_title.clone(),
                format!("{}\n\n{}", strings.external_change_message, detail),
            )
        });
    }

    pub(super) fn show_workspace_save_error(
        window_handle: AnyWindowHandle,
        detail: String,
        cx: &mut AsyncApp,
    ) {
        show_async_message_modal(window_handle, cx, move |strings| {
            (strings.save_failed_title.clone(), detail.clone())
        });
    }

    pub(super) fn report_workspace_file_error(&mut self, detail: String, cx: &mut Context<Self>) {
        if self.workspace.root.is_none() {
            self.workspace.root = self.workspace_root_for_current_file();
        }
        self.workspace.file_error = Some(detail);
        cx.notify();
    }

    fn prompt_create_workspace_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(directory) = self.selected_workspace_directory() else {
            return;
        };
        let prompt = cx.prompt_for_new_path(&directory, Some("untitled.md"));
        let editor = cx.entity().downgrade();
        let window_handle = window.window_handle();
        cx.spawn(async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let Ok(Ok(Some(path))) = prompt.await else {
                return;
            };
            if let Err(err) = create_workspace_file(&path) {
                Self::show_workspace_file_error(
                    window_handle,
                    format!("无法新建文件：{}", err),
                    cx,
                );
                return;
            }
            let _ = editor.update(cx, |editor, cx| editor.refresh_workspace_tree(cx));
            let _ = cx.update_window(
                window_handle,
                move |_view: AnyView, window: &mut Window, cx: &mut App| {
                    let _ = editor.update(cx, |editor, cx| {
                        editor.open_workspace_file(path, window, cx);
                    });
                },
            );
        })
        .detach();
    }

    fn prompt_create_workspace_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(directory) = self.selected_workspace_directory() else {
            return;
        };
        let prompt = cx.prompt_for_new_path(&directory, Some("New Folder"));
        let editor = cx.entity().downgrade();
        let window_handle = window.window_handle();
        cx.spawn(async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let Ok(Ok(Some(path))) = prompt.await else {
                return;
            };
            if let Err(err) = create_workspace_folder(&path) {
                Self::show_workspace_file_error(
                    window_handle,
                    format!("无法新建文件夹：{}", err),
                    cx,
                );
                return;
            }
            let folder_path = path.clone();
            let _ = editor.update(cx, move |editor, cx| {
                editor.refresh_workspace_tree(cx);
                editor.workspace.expanded.insert(file_node_id(&folder_path));
                editor.workspace.selected = Some(WorkspaceSelection::Directory(folder_path));
                cx.notify();
            });
        })
        .detach();
    }

    fn prompt_rename_or_move_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(source) = self.selected_workspace_path() else {
            return;
        };
        let Some(parent) = source.parent().map(Path::to_path_buf) else {
            return;
        };
        let suggested_name = source
            .file_name()
            .map(|name| name.to_string_lossy().into_owned());
        let prompt = cx.prompt_for_new_path(&parent, suggested_name.as_deref());
        let editor = cx.entity().downgrade();
        let window_handle = window.window_handle();
        let source_is_directory = source.is_dir();
        let background = cx.background_executor().clone();

        cx.spawn(async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let Ok(Ok(Some(destination))) = prompt.await else {
                return;
            };
            if destination == source {
                return;
            }
            if destination.exists() {
                Self::show_workspace_file_error(
                    window_handle,
                    format!("目标路径已存在：{}", destination.display()),
                    cx,
                );
                return;
            }

            let source_directory = source.parent().map(Path::to_path_buf);
            let destination_directory = destination.parent().map(Path::to_path_buf);
            let markdown_move = !source_is_directory
                && is_markdown_file(&source)
                && source_directory != destination_directory;
            let open_markdown = if markdown_move {
                editor
                    .update(cx, |editor, cx| editor.markdown_state_for_path(&source, cx))
                    .ok()
                    .flatten()
            } else {
                None
            };
            let source_for_read = source.clone();
            let disk_markdown = if markdown_move {
                match background
                    .spawn(async move { fs::read_to_string(source_for_read) })
                    .await
                {
                    Ok(markdown) => Some(markdown),
                    Err(error) => {
                        Self::show_workspace_file_error(
                            window_handle,
                            format!("无法读取待移动的 Markdown 文件：{error}"),
                            cx,
                        );
                        return;
                    }
                }
            } else {
                None
            };
            if let (Some((_, _, expected_version)), Some(markdown)) =
                (open_markdown.as_ref(), disk_markdown.as_ref())
                && super::persistence::file_content_version(markdown) != *expected_version
            {
                Self::show_external_change_error(
                    window_handle,
                    format!("检测到外部修改：{}", source.display()),
                    cx,
                );
                return;
            }
            let rewritten_disk_markdown = match (
                markdown_move,
                disk_markdown.as_deref(),
                source_directory.as_deref(),
                destination_directory.as_deref(),
            ) {
                (true, Some(markdown), Some(source_directory), Some(destination_directory)) => {
                    Some(rewrite_relative_image_targets(
                        markdown,
                        source_directory,
                        destination_directory,
                    ))
                }
                _ => None,
            };
            let moved_disk_markdown = rewritten_disk_markdown
                .clone()
                .or_else(|| disk_markdown.clone());
            let moved_disk_version = moved_disk_markdown
                .as_deref()
                .map(super::persistence::file_content_version);

            if let Err(err) = std::fs::rename(&source, &destination) {
                Self::show_workspace_file_error(
                    window_handle,
                    format!("无法移动或重命名：{}", err),
                    cx,
                );
                return;
            }
            if let Some(rewritten) = rewritten_disk_markdown
                .as_ref()
                .zip(disk_markdown.as_ref())
                .and_then(|(rewritten, original)| (rewritten != original).then_some(rewritten))
                && let Err(error) = fs::write(&destination, rewritten)
            {
                let rollback_error = std::fs::rename(&destination, &source).err();
                let detail = if let Some(rollback_error) = rollback_error {
                    format!("无法更新图片相对路径：{error}；回滚也失败：{rollback_error}")
                } else {
                    format!("无法更新图片相对路径：{error}")
                };
                Self::show_workspace_file_error(window_handle, detail, cx);
                return;
            }

            let _ = editor.update(cx, move |editor, cx| {
                editor.document_revision = editor.document_revision.wrapping_add(1);
                editor.autosave_task = None;
                for tab in &mut editor.workspace.open_documents {
                    if markdown_move && tab.path == source {
                        if let (Some(source_directory), Some(destination_directory)) = (
                            source_directory.as_deref(),
                            destination_directory.as_deref(),
                        ) {
                            tab.markdown = rewrite_relative_image_targets(
                                &tab.markdown,
                                source_directory,
                                destination_directory,
                            );
                        }
                        if let Some(file_version) = moved_disk_version {
                            tab.file_version = file_version;
                        }
                    }
                    if let Some(path) =
                        remap_moved_path(&tab.path, &source, &destination, source_is_directory)
                    {
                        tab.path = path;
                    }
                }
                let active_markdown = if markdown_move && editor.file_path.as_ref() == Some(&source)
                {
                    Some((editor.serialized_document_text(cx), editor.document_dirty))
                } else {
                    None
                };
                if let Some(path) = editor.file_path.as_ref().and_then(|path| {
                    remap_moved_path(path, &source, &destination, source_is_directory)
                }) {
                    editor.file_path = Some(path);
                    editor.pending_window_title_refresh = true;
                }
                if let Some(path) = editor.workspace.active_document.as_ref().and_then(|path| {
                    remap_moved_path(path, &source, &destination, source_is_directory)
                }) {
                    editor.workspace.active_document = Some(path);
                }
                if let Some(root) = editor.workspace.root.as_ref().and_then(|root| {
                    remap_moved_path(root, &source, &destination, source_is_directory)
                }) {
                    editor.workspace.root = Some(root.clone());
                }
                editor.workspace.selected = match editor.workspace.selected.take() {
                    Some(WorkspaceSelection::File(path)) => {
                        remap_moved_path(&path, &source, &destination, source_is_directory)
                            .map(WorkspaceSelection::File)
                    }
                    Some(WorkspaceSelection::Directory(path)) => {
                        remap_moved_path(&path, &source, &destination, source_is_directory)
                            .map(WorkspaceSelection::Directory)
                    }
                    Some(WorkspaceSelection::WorkspaceRoot(path)) => {
                        remap_moved_path(&path, &source, &destination, source_is_directory)
                            .map(WorkspaceSelection::WorkspaceRoot)
                    }
                    other => other,
                };
                if let Some((markdown, was_dirty)) = active_markdown {
                    if let (Some(source_directory), Some(destination_directory)) = (
                        source_directory.as_deref(),
                        destination_directory.as_deref(),
                    ) {
                        let rewritten = rewrite_relative_image_targets(
                            &markdown,
                            source_directory,
                            destination_directory,
                        );
                        editor.file_version = moved_disk_version;
                        if rewritten != markdown {
                            editor.replace_document_from_markdown(
                                rewritten.clone(),
                                Some(destination.clone()),
                                cx,
                            );
                            editor.file_version = moved_disk_version;
                            editor.document_dirty = was_dirty;
                            if was_dirty {
                                editor.mark_dirty(cx);
                            }
                            if let Some(tab) =
                                editor.workspace.open_documents.iter_mut().find(|tab| {
                                    tab.path == destination && tab.recovery_id == editor.recovery_id
                                })
                            {
                                tab.markdown = rewritten;
                                tab.dirty = editor.document_dirty;
                                tab.file_version = moved_disk_version.unwrap_or_else(|| {
                                    super::persistence::file_content_version(&tab.markdown)
                                });
                            }
                        } else if was_dirty {
                            editor.document_dirty = true;
                            editor.schedule_autosave(cx);
                        }
                    }
                }
                editor.refresh_workspace_tree(cx);
                if editor.document_dirty || editor.has_dirty_workspace_documents() {
                    editor.schedule_autosave(cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn prompt_delete_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(target) = self.selected_workspace_path() else {
            return;
        };
        if self.workspace.root.as_ref() == Some(&target) {
            return;
        }
        let target_is_directory = target.is_dir();
        let strings = cx.global::<crate::i18n::I18nManager>().strings().clone();
        let has_unsaved_changes =
            self.document_dirty
                && self
                    .file_path
                    .as_ref()
                    .is_some_and(|path| path_is_affected(path, &target, target_is_directory))
                || self.workspace.open_documents.iter().any(|tab| {
                    tab.dirty && path_is_affected(&tab.path, &target, target_is_directory)
                });
        let mut detail = format!(
            "{}\n{}",
            strings.workspace_delete_confirm_message,
            target.display()
        );
        if has_unsaved_changes {
            detail.push_str("\n");
            detail.push_str(&strings.workspace_delete_unsaved_message);
        }
        let editor = cx.entity().downgrade();
        let window_handle = window.window_handle();
        let background = cx.background_executor().clone();
        let delete_policy = crate::config::EditorSettings::delete_policy(cx);
        let title = strings.workspace_delete_confirm_title.clone();
        let confirm_label = strings.workspace_delete.clone();
        let cancel_label = strings.open_link_cancel.clone();

        // 删除确认走应用内模态（用户要求：全软件不用系统原生弹窗）。
        self.show_modal(
            ModalSpec {
                title: title.into(),
                detail: Some(detail.into()),
                buttons: vec![confirm_label.into(), cancel_label.into()],
                default_index: 0,
                cancel_index: 1,
            },
            move |choice, _editor, _window, cx| {
                if choice != 0 {
                    return;
                }
        cx.spawn(async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let delete_target = target.clone();
            let delete_policy = delete_policy;
            let result = background.spawn(async move {
                match delete_policy {
                    crate::config::DeletePolicy::Permanent => {
                        permanent_delete(&delete_target, target_is_directory)
                    }
                    crate::config::DeletePolicy::Trash => {
                        move_to_trash(&delete_target, target_is_directory)
                    }
                }
            })
                .await;
            if let Err(err) = result {
                Self::show_workspace_file_error(window_handle, format!("无法删除：{}", err), cx);
                return;
            }

            let target_for_update = target.clone();
            let _ = cx.update_window(
                window_handle,
                move |_view: AnyView, window: &mut Window, cx: &mut App| {
                    let _ = editor.update(cx, |editor, cx| {
                        let active_recovery_id = editor.recovery_id;
                        let active_deleted = editor.file_path.as_ref().is_some_and(|path| {
                            path_is_affected(path, &target_for_update, target_is_directory)
                        });
                        editor.document_revision = editor.document_revision.wrapping_add(1);
                        editor.autosave_task = None;
                        if active_deleted {
                            editor.snapshot_current_document(cx);
                        }
                        let deleted_recovery_ids = editor
                            .workspace
                            .open_documents
                            .iter()
                            .filter(|tab| {
                                path_is_affected(
                                    &tab.path,
                                    &target_for_update,
                                    target_is_directory,
                                )
                            })
                            .map(|tab| tab.recovery_id)
                            .collect::<Vec<_>>();
                        for recovery_id in deleted_recovery_ids {
                            if let Err(error) =
                                crate::config::remove_recovery_snapshot(recovery_id)
                            {
                                eprintln!("failed to remove deleted tab recovery snapshot: {error}");
                            }
                        }
                        let next_tab = editor
                            .workspace
                            .open_documents
                            .iter()
                            .find(|tab| {
                                !path_is_affected(
                                    &tab.path,
                                    &target_for_update,
                                    target_is_directory,
                                )
                            })
                            .cloned();
                        editor.workspace.open_documents.retain(|tab| {
                            !path_is_affected(&tab.path, &target_for_update, target_is_directory)
                        });
                        if editor.workspace.root.as_ref().is_some_and(|root| {
                            path_is_affected(root, &target_for_update, target_is_directory)
                        }) {
                            editor.workspace.root = None;
                            editor.workspace.file_tree = None;
                            editor.workspace.tree_scan_root = None;
                        }
                        editor.workspace.selected = None;
                        if active_deleted {
                            if let Some(tab) = next_tab {
                                editor.recovery_id = tab.recovery_id;
                                let file_version = tab.file_version;
                                editor.recovery_source_path = None;
                                editor.is_recovered_document = false;
                                editor.workspace.active_document = Some(tab.path.clone());
                                if is_code_file(&tab.path) {
                                    editor.replace_document_from_code_source(tab.markdown, tab.path, cx);
                                } else {
                                    editor.replace_document_from_markdown(tab.markdown, Some(tab.path), cx);
                                }
                                editor.document_dirty = tab.dirty;
                                editor.file_version = Some(file_version);
                                window.set_window_edited(tab.dirty);
                            } else {
                                editor.recovery_id = uuid::Uuid::new_v4();
                                editor.recovery_source_path = None;
                                editor.is_recovered_document = false;
                                if let Err(error) =
                                    crate::config::remove_recovery_snapshot(active_recovery_id)
                                {
                                    eprintln!("failed to remove deleted document recovery snapshot: {error}");
                                }
                                editor.workspace.active_document = None;
                                editor.replace_document_from_markdown(String::new(), None, cx);
                                window.set_window_edited(false);
                            }
                        }
                        editor.refresh_workspace_tree(cx);
                        if editor.document_dirty || editor.has_dirty_workspace_documents() {
                            editor.schedule_autosave(cx);
                        }
                        cx.notify();
                    });
                },
            );
        })
        .detach();
            },
            cx,
        );
    }

    fn selected_workspace_path(&self) -> Option<PathBuf> {
        match self.workspace.selected.as_ref()? {
            WorkspaceSelection::Directory(path)
            | WorkspaceSelection::File(path)
            | WorkspaceSelection::WorkspaceRoot(path) => Some(path.clone()),
            WorkspaceSelection::Outline(_) => None,
        }
    }

    pub(super) fn close_workspace_context_menu(&mut self, cx: &mut Context<Self>) {
        if self.workspace.context_menu.take().is_some() {
            cx.notify();
        }
    }

    fn open_workspace_context_menu(
        &mut self,
        position: Point<Pixels>,
        selection: Option<WorkspaceSelection>,
        cx: &mut Context<Self>,
    ) {
        self.dismiss_contextual_overlays(cx);
        let has_target = selection.as_ref().is_some_and(|selection| match selection {
            WorkspaceSelection::File(path) | WorkspaceSelection::Directory(path) => {
                self.workspace.root.as_ref() != Some(path)
            }
            _ => false,
        });
        self.workspace.selected = selection.or_else(|| {
            self.workspace
                .root
                .clone()
                .map(WorkspaceSelection::WorkspaceRoot)
        });
        self.workspace.context_menu = Some(WorkspaceContextMenu {
            position,
            has_target,
        });
        cx.notify();
    }

    fn on_workspace_background_right_click(
        &mut self,
        event: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_workspace_context_menu(event.position, None, cx);
        cx.stop_propagation();
    }

    pub(super) fn render_workspace_context_menu_overlay(
        &self,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let menu = self.workspace.context_menu?;
        let strings = cx.global::<crate::i18n::I18nManager>().strings();
        let mut actions = vec![
            (
                strings.workspace_new_file.clone(),
                WorkspaceMenuAction::NewFile,
            ),
            (
                strings.workspace_new_folder.clone(),
                WorkspaceMenuAction::NewFolder,
            ),
        ];
        if menu.has_target {
            actions.push((
                strings.workspace_rename.clone(),
                WorkspaceMenuAction::Rename,
            ));
            if let Some(WorkspaceSelection::File(path)) = self.workspace.selected.as_ref() {
                if !path.is_dir() {
                    actions.push((
                        strings.workspace_duplicate.clone(),
                        WorkspaceMenuAction::Duplicate,
                    ));
                    actions.push((
                        strings.workspace_copy.clone(),
                        WorkspaceMenuAction::Copy,
                    ));
                }
            }
            actions.push((
                strings.workspace_delete.clone(),
                WorkspaceMenuAction::Delete,
            ));
        }
        actions.push((
            strings.workspace_paste.clone(),
            WorkspaceMenuAction::Paste,
        ));
        let width = 180.0;
        let height = actions.len() as f32 * 32.0 + 8.0;
        let viewport = window.viewport_size();
        let left = f32::from(menu.position.x)
            .min((f32::from(viewport.width) - width - 8.0).max(8.0))
            .max(8.0);
        let top = f32::from(menu.position.y)
            .min((f32::from(viewport.height) - height - 8.0).max(8.0))
            .max(8.0);
        let editor = cx.entity().downgrade();
        let rows = actions
            .into_iter()
            .enumerate()
            .map(|(index, (label, action))| {
                let editor = editor.clone();
                div()
                    .id(("workspace-context-action", index))
                    .h(px(32.0))
                    .px(px(10.0))
                    .flex()
                    .items_center()
                    .rounded(px(5.0))
                    .cursor_pointer()
                    .text_size(px(12.0))
                    .text_color(if matches!(action, WorkspaceMenuAction::Delete) {
                        theme.colors.dialog_danger_button_bg
                    } else {
                        theme.colors.dialog_body
                    })
                    .hover(|this| this.bg(theme.colors.dialog_secondary_button_hover))
                    .child(label)
                    .on_click(move |_, window, cx| {
                        let _ = editor.update(cx, |editor, cx| {
                            editor.workspace.context_menu = None;
                            match action {
                                WorkspaceMenuAction::NewFile => {
                                    editor.prompt_create_workspace_file(window, cx)
                                }
                                WorkspaceMenuAction::NewFolder => {
                                    editor.prompt_create_workspace_folder(window, cx)
                                }
                                WorkspaceMenuAction::Rename => {
                                    editor.prompt_rename_or_move_selected(window, cx)
                                }
                                WorkspaceMenuAction::Duplicate => {
                                    editor.duplicate_selected_file(window, cx)
                                }
                                WorkspaceMenuAction::Copy => {
                                    editor.copy_selected_workspace_file(cx)
                                }
                                WorkspaceMenuAction::Paste => {
                                    editor.paste_into_workspace_tree(cx)
                                }
                                WorkspaceMenuAction::Delete => {
                                    editor.prompt_delete_selected(window, cx)
                                }
                            }
                            cx.notify();
                        });
                        cx.stop_propagation();
                    })
            })
            .collect::<Vec<_>>();
        let close_editor = editor;
        Some(
            div()
                .id("workspace-context-overlay")
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .bottom_0()
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = close_editor
                        .update(cx, |editor, cx| editor.close_workspace_context_menu(cx));
                })
                .child(
                    div()
                        .id("workspace-context-panel")
                        .absolute()
                        .left(px(left))
                        .top(px(top))
                        .w(px(width))
                        .p(px(4.0))
                        .rounded(px(8.0))
                        .border_1()
                        .border_color(theme.colors.dialog_border)
                        .bg(theme.colors.dialog_surface)
                        .shadow_md()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .children(rows),
                )
                .into_any_element(),
        )
    }

    pub(crate) fn toggle_workspace_drawer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // 手动切换后不再保留「贴边滑出」的浮层状态，避免收起时它立刻又冒出来。
        // 收回动画同理：浮层要么被展开的抽屉取代、要么随收起直接消失，都不该
        // 再挂着一个正在滑出的浮层。
        self.sidebar_peek = false;
        self.sidebar_overlay_closing = false;
        if self.workspace.is_open {
            self.workspace.is_open = false;
        } else {
            self.close_menu_bar(cx);
            self.dismiss_contextual_overlays(cx);
            self.workspace.is_open = true;
            self.sync_workspace_models(cx);
            window.activate_window();
        }
        cx.notify();
    }

    /// 收起状态下指针贴到窗口左边缘时的浮层开关。
    ///
    /// 收起后整条侧边栏（窄条 + 面板）都不占布局，正文用满整宽；指针在左边缘
    /// 停留满 dwell（见 `SIDEBAR_PEEK_DWELL`）后整条侧边栏作为浮层带滑入动画
    /// 盖在正文上，指针移开再带滑出动画收回。展开状态下这个开关不生效（那时
    /// 侧边栏本来就常驻）。停留判定在贴边感应区的 hover 处理里。
    ///
    /// 收回动画期间浮层仍挂载，动画播完由定时器卸载；动画中途再次贴边会作废
    /// 那枚定时器、重新播放滑入。
    pub(super) fn set_sidebar_peek(&mut self, peek: bool, cx: &mut Context<Self>) {
        if self.workspace.is_open {
            return;
        }
        if peek {
            // 已经完全滑出时无需重播；正在收回则取消收回、立即重新滑入。
            if !self.sidebar_peek {
                self.sidebar_peek = true;
                // 浮层里展示的还是那几棵树，进入时同步一次，和展开抽屉走同一条路径。
                self.sync_workspace_models(cx);
                cx.notify();
            }
            self.sidebar_overlay_closing = false;
            return;
        }
        if !self.sidebar_peek {
            return;
        }
        self.sidebar_peek = false;
        self.sidebar_collapse_generation = self.sidebar_collapse_generation.wrapping_add(1);
        let generation = self.sidebar_collapse_generation;
        self.sidebar_overlay_closing = true;
        let duration = super::render::SIDEBAR_SLIDE_DURATION;
        cx.spawn(async move |editor, cx| {
            cx.background_executor().timer(duration).await;
            _ = editor.update(cx, |editor, cx| {
                if editor.sidebar_collapse_generation == generation && !editor.sidebar_peek {
                    editor.sidebar_overlay_closing = false;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    pub(crate) fn on_toggle_workspace_action(
        &mut self,
        _: &crate::components::ToggleSidebar,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_workspace_drawer(window, cx);
    }

    pub(super) fn sync_workspace_after_document_path_change(&mut self, cx: &mut Context<Self>) {
        if let Some(path) = self.file_path.clone() {
            let previous = self.workspace.active_document.clone();
            if previous.as_ref() != Some(&path) {
                let markdown = self.serialized_document_text(cx);
                let file_version = self
                    .file_version
                    .unwrap_or_else(|| super::persistence::file_content_version(&markdown));
                let previous_index = previous.as_ref().and_then(|previous| {
                    self.workspace
                        .open_documents
                        .iter()
                        .position(|tab| &tab.path == previous)
                });
                let current_index = self
                    .workspace
                    .open_documents
                    .iter()
                    .position(|tab| tab.path == path);
                if let Some(previous_index) = previous_index {
                    if let Some(current_index) = current_index {
                        self.workspace.open_documents.remove(previous_index);
                        let current_index = if previous_index < current_index {
                            current_index - 1
                        } else {
                            current_index
                        };
                        let tab = &mut self.workspace.open_documents[current_index];
                        tab.recovery_id = self.recovery_id;
                        tab.file_version = file_version;
                        tab.markdown = markdown;
                        tab.dirty = false;
                    } else {
                        let tab = &mut self.workspace.open_documents[previous_index];
                        tab.path = path.clone();
                        tab.recovery_id = self.recovery_id;
                        tab.file_version = file_version;
                        tab.markdown = markdown;
                        tab.dirty = false;
                    }
                } else if let Some(current_index) = current_index {
                    let tab = &mut self.workspace.open_documents[current_index];
                    tab.recovery_id = self.recovery_id;
                    tab.file_version = file_version;
                    tab.markdown = markdown;
                    tab.dirty = false;
                } else {
                    self.workspace.open_documents.push(WorkspaceDocumentTab {
                        path: path.clone(),
                        recovery_id: self.recovery_id,
                        file_version,
                        markdown,
                        dirty: false,
                        preview: false,
                    });
                }
                if self.workspace.selected == previous.map(WorkspaceSelection::File) {
                    self.workspace.selected = Some(WorkspaceSelection::File(path.clone()));
                }
                self.workspace.active_document = Some(path);
            }
        }
        // 只有工作区根目录真的换了才丢树：同一根目录下点开一个文件时把树清空，
        // 会让侧栏在后台重扫的那一帧只剩「…」占位（内容高度≈30px），gpui 的
        // div 会把记住的滚动偏移按新的 scroll_max 夹到 0 并写回，于是长树滚到
        // 下面再点文件就自动置顶（用户报修）。重扫照旧会刷新内容。
        let previous_root = self.workspace.root.clone();
        self.clear_workspace_file_error();
        self.workspace.outline_source = None;
        if self.workspace.root.is_none() {
            self.workspace.root = self.workspace_root_for_current_file();
        }
        if previous_root != self.workspace.root {
            self.workspace.file_tree = None;
            self.workspace.tree_scan_root = None;
        }
        if self.workspace.is_open {
            self.sync_workspace_models(cx);
        }
        self.refresh_document_find_after_edit(cx);
    }

    fn sync_workspace_models(&mut self, cx: &mut Context<Self>) {
        self.sync_workspace_file_tree(cx);
        self.sync_workspace_outline(cx);
        self.ensure_current_document_tab(cx);
    }

    fn ensure_current_document_tab(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.file_path.clone() else {
            return;
        };
        if self.workspace.active_document.as_ref() == Some(&path) {
            return;
        }
        if !self
            .workspace
            .open_documents
            .iter()
            .any(|tab| tab.path == path)
        {
            let markdown = self.serialized_document_text(cx);
            let file_version = self
                .file_version
                .unwrap_or_else(|| super::persistence::file_content_version(&markdown));
            self.workspace.open_documents.push(WorkspaceDocumentTab {
                path: path.clone(),
                recovery_id: self.recovery_id,
                file_version,
                markdown,
                dirty: self.document_dirty,
                preview: false,
            });
        }
        self.workspace.active_document = Some(path);
    }

    pub(super) fn snapshot_current_document(&mut self, cx: &App) {
        let Some(path) = self.file_path.clone() else {
            return;
        };
        let markdown = self.serialized_document_text(cx);
        if let Some(tab) = self
            .workspace
            .open_documents
            .iter_mut()
            .find(|tab| tab.path == path)
        {
            tab.markdown = markdown;
            tab.dirty = self.document_dirty;
        } else {
            self.workspace.open_documents.push(WorkspaceDocumentTab {
                path: path.clone(),
                recovery_id: self.recovery_id,
                file_version: self
                    .file_version
                    .unwrap_or_else(|| super::persistence::file_content_version(&markdown)),
                markdown,
                dirty: self.document_dirty,
                preview: false,
            });
        }
        self.workspace.active_document = Some(path);
    }

    pub(super) fn dirty_workspace_documents(&mut self, cx: &App) -> Vec<WorkspaceAutosaveDocument> {
        self.snapshot_current_document(cx);
        self.workspace
            .open_documents
            .iter()
            .filter(|tab| tab.dirty)
            .map(|tab| WorkspaceAutosaveDocument {
                recovery_id: tab.recovery_id,
                file_version: tab.file_version,
                path: tab.path.clone(),
                markdown: tab.markdown.clone(),
            })
            .collect()
    }

    pub(super) fn mark_workspace_documents_saved(
        &mut self,
        saved: &[WorkspaceAutosaveDocument],
    ) -> bool {
        let mut active_document_saved = false;
        for document in saved {
            let Some(tab) =
                self.workspace.open_documents.iter_mut().find(|tab| {
                    tab.recovery_id == document.recovery_id && tab.path == document.path
                })
            else {
                continue;
            };
            tab.markdown = document.markdown.clone();
            tab.dirty = false;
            tab.file_version = super::persistence::file_content_version(&document.markdown);
            if self.file_path.as_ref() == Some(&tab.path) {
                self.file_version = Some(tab.file_version);
            }
            active_document_saved |= self.workspace.active_document.as_ref() == Some(&tab.path);
        }
        active_document_saved
    }

    pub(super) fn has_dirty_workspace_documents(&self) -> bool {
        self.workspace.open_documents.iter().any(|tab| tab.dirty)
    }

    pub(super) fn has_external_autosave_conflict(&self) -> bool {
        self.workspace.external_change_conflict.is_some()
    }

    /// 记录外部修改冲突：自动保存不得覆盖磁盘上的新内容，直到该文件重新加载。
    pub(super) fn report_external_change_conflict(
        &mut self,
        path: PathBuf,
        detail: String,
        cx: &mut Context<Self>,
    ) {
        self.workspace.external_change_conflict = Some((path, detail.clone()));
        self.report_workspace_file_error(detail, cx);
    }

    /// 清空工作区错误提示；外部修改冲突未解决时保留提示。
    pub(super) fn clear_workspace_file_error(&mut self) {
        if self.workspace.external_change_conflict.is_none() {
            self.workspace.file_error = None;
        }
    }

    /// 该文件重新读盘成功即视为冲突解除。
    pub(super) fn clear_external_change_conflict_for(&mut self, path: &Path) {
        let conflicted = self
            .workspace
            .external_change_conflict
            .as_ref()
            .is_some_and(|(conflict_path, _)| conflict_path == path);
        if conflicted {
            self.workspace.external_change_conflict = None;
            self.workspace.file_error = None;
        }
    }

    pub(super) fn workspace_recovery_ids(&self) -> Vec<uuid::Uuid> {
        self.workspace
            .open_documents
            .iter()
            .map(|tab| tab.recovery_id)
            .collect()
    }

    fn workspace_root_for_current_file(&self) -> Option<PathBuf> {
        self.file_path.as_ref()?.parent().map(Path::to_path_buf)
    }

    pub(super) fn workspace_root_for_image_paste(&self) -> Option<PathBuf> {
        self.workspace.root.clone()
    }

    fn markdown_state_for_path(&self, path: &Path, cx: &App) -> Option<(String, bool, u64)> {
        if self.file_path.as_deref() == Some(path) {
            let markdown = self.serialized_document_text(cx);
            return Some((
                markdown.clone(),
                self.document_dirty,
                self.file_version
                    .unwrap_or_else(|| super::persistence::file_content_version(&markdown)),
            ));
        }
        self.workspace
            .open_documents
            .iter()
            .find(|tab| tab.path == path)
            .map(|tab| (tab.markdown.clone(), tab.dirty, tab.file_version))
    }

    fn sync_workspace_file_tree(&mut self, cx: &mut Context<Self>) {
        self.sync_workspace_file_tree_inner(false, cx);
    }

    /// 扫描工作区目录并更新文件树。扫描在后台线程执行（roadmap D9），
    /// 超大目录不再阻塞首帧；结果按代数校验，过期扫描直接丢弃。
    fn sync_workspace_file_tree_inner(&mut self, force: bool, cx: &mut Context<Self>) {
        let next_root = self
            .workspace
            .root
            .clone()
            .or_else(|| self.workspace_root_for_current_file());
        // 该根已有结果（树或错误）或扫描在途：不重复发起扫描，
        // 否则渲染期每帧都会重启扫描（roadmap D9）。
        if !force
            && self.workspace.root == next_root
            && self.workspace.tree_scan_root == next_root
        {
            self.workspace.selected = self
                .file_path
                .as_ref()
                .map(|path| WorkspaceSelection::File(path.clone()));
            return;
        }

        self.workspace.root = next_root.clone();
        self.clear_workspace_file_error();

        let Some(root) = next_root else {
            self.workspace.file_tree = None;
            self.workspace.selected = None;
            self.workspace.tree_scan_root = None;
            return;
        };

        // Validate the root path
        if root.as_os_str().is_empty() {
            self.workspace.file_error = Some("Invalid workspace path: empty path".to_string());
            self.workspace.file_tree = None;
            self.workspace.selected = None;
            self.workspace.tree_scan_root = None;
            return;
        }

        let tree_sort = crate::config::EditorSettings::tree_sort(cx);
        self.workspace.tree_scan_root = Some(root.clone());
        self.workspace.tree_scan_generation = self.workspace.tree_scan_generation.wrapping_add(1);
        let generation = self.workspace.tree_scan_generation;
        let editor = cx.entity().downgrade();
        let scan_root = root.clone();
        let scan = cx.background_spawn(async move { scan_workspace_dir(&scan_root, tree_sort) });
        // Dropping the previous task cancels a scan that is no longer relevant.
        self.workspace.tree_scan_task = Some(cx.spawn(
            async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
                let result = scan.await;
                editor
                    .update(cx, |editor, cx| {
                        if editor.workspace.tree_scan_generation != generation {
                            return;
                        }
                        match result {
                            Ok(tree) => {
                                editor.workspace.expanded.insert(tree.id.clone());
                                editor.workspace.file_tree = Some(tree);
                                editor.workspace.selected = editor
                                    .file_path
                                    .as_ref()
                                    .map(|path| WorkspaceSelection::File(path.clone()));
                                // 扫描期间发起的工作区搜索此时才有文件列表可用。
                                if editor.workspace.active_tab == WorkspaceTab::Search
                                    && !editor.workspace.search_query.is_empty()
                                {
                                    editor.schedule_workspace_search(cx);
                                }
                            }
                            Err(err) => {
                                editor.workspace.file_error = Some(err.to_string());
                            }
                        }
                        cx.notify();
                    })
                    .ok();
            },
        ));
    }

    fn sync_workspace_outline(&mut self, _cx: &mut Context<Self>) {
        let source = &self.last_stable_source_text;
        if self.workspace.outline_source.as_deref() == Some(source.as_str()) {
            return;
        }

        let outline = build_outline_tree(source);
        prune_outline_state(&mut self.workspace, &outline);
        // Expand headings down to H3 by default so the outline is usable
        // without clicking through every level; users can still collapse.
        expand_outline_to_level(&outline, 2, &mut self.workspace.expanded);
        self.workspace.toc_entries = flatten_outline_entries(&outline);
        self.toc_state_version = self.toc_state_version.wrapping_add(1);
        self.workspace.outline_tree = outline;
        self.workspace.outline_source = Some(source.clone());
    }

    fn set_workspace_tab(&mut self, tab: WorkspaceTab, cx: &mut Context<Self>) {
        let changed = self.workspace.active_tab != tab;
        self.workspace.search_focus_pending = false;
        self.workspace.search_query.clear();
        self.workspace.search_selected_range = 0..0;
        self.workspace.search_marked_range = None;
        self.workspace.search_results.clear();
        self.workspace.document_search_source = None;
        self.workspace.document_active_range = None;
        self.workspace.search_pending = false;
        self.workspace.search_generation = self.workspace.search_generation.wrapping_add(1);
        if changed {
            self.workspace.active_tab = tab;
            self.sync_workspace_models(cx);
            cx.notify();
        }
    }

    fn search_options(&self) -> SearchOptions {
        SearchOptions {
            match_case: self.workspace.search_match_case,
            whole_word: self.workspace.search_whole_word,
            use_regex: self.workspace.search_use_regex,
            fuzzy: self.workspace.search_fuzzy,
        }
    }

    fn schedule_workspace_search(&mut self, cx: &mut Context<Self>) {
        self.workspace.search_generation = self.workspace.search_generation.wrapping_add(1);
        let generation = self.workspace.search_generation;
        self.workspace.search_active_index = None;
        self.workspace.document_search_source = None;
        self.workspace.document_active_range = None;
        let matcher = SearchMatcher::new(self.workspace.search_query.trim(), self.search_options());
        let scope = self.workspace.search_scope;
        let tree = self.workspace.file_tree.clone();
        if matcher.is_empty() || (scope == WorkspaceSearchScope::Workspace && tree.is_none()) {
            self.workspace.search_results.clear();
            self.workspace.search_pending = false;
            self.sync_document_search_highlights(cx);
            cx.notify();
            return;
        }
        // 去抖窗口里保留上一次的结果，新结果落地后再整体替换：清空会让侧栏
        // 先闪成空白再恢复（用户报修，watcher 刷新文件树等任何重新调度都会触发）。
        self.workspace.search_pending = true;
        let editor = cx.entity().downgrade();
        let background = cx.background_executor().clone();
        cx.spawn(async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
            background.timer(Duration::from_millis(120)).await;
            let current = editor
                .update(cx, |editor, _| {
                    editor.workspace.search_generation == generation
                })
                .unwrap_or(false);
            if !current {
                return;
            }
            let (results, document_source) = match scope {
                WorkspaceSearchScope::Workspace => {
                    let Some(tree) = tree else { return };
                    let results = search_workspace_files(&tree, &matcher, 200, &background)
                        .await;
                    (results, None)
                }
                WorkspaceSearchScope::Document => {
                    let Ok((source, path, label)) = editor.update(cx, |editor, cx| {
                        let source = editor.current_document_source(cx);
                        let path = editor.file_path.clone().unwrap_or_default();
                        let label = path
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_else(|| {
                                cx.global::<I18nManager>()
                                    .strings()
                                    .workspace_current_document_label
                                    .clone()
                            });
                        (source, path, label)
                    }) else {
                        return;
                    };
                    let (results, source) = background
                        .spawn(async move {
                            let results = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                                || search_document_source(&source, &matcher, &path, &label, 200),
                            ))
                            .unwrap_or_default();
                            (results, source)
                        })
                        .await;
                    (results, Some(source))
                }
            };
            let _ = editor.update(cx, |editor, cx| {
                if editor.workspace.search_generation == generation {
                    editor.workspace.search_results = results;
                    editor.workspace.document_search_source = document_source;
                    editor.workspace.search_pending = false;
                    editor.sync_document_search_highlights(cx);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Records the current caret location before a programmatic jump
    /// (roadmap E6).
    pub(super) fn push_cursor_location(&mut self, cx: &mut Context<Self>) {
        let snapshot = self.capture_source_selection_snapshot(cx);
        self.cursor_history_back.push(CursorLocation {
            path: self.file_path.clone(),
            range: snapshot.range,
        });
        if self.cursor_history_back.len() > CURSOR_HISTORY_LIMIT {
            self.cursor_history_back.remove(0);
        }
        self.cursor_history_forward.clear();
    }

    /// 拖拽标签落到目标标签上：把被拖标签移动到目标位置（roadmap E3）。
    pub(super) fn move_tab_to_position(
        &mut self,
        from_path: &Path,
        to_path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if from_path == to_path {
            return;
        }
        let Some(from) = self
            .workspace
            .open_documents
            .iter()
            .position(|tab| &tab.path == from_path)
        else {
            return;
        };
        let Some(to) = self
            .workspace
            .open_documents
            .iter()
            .position(|tab| tab.path == to_path)
        else {
            return;
        };
        let tab = self.workspace.open_documents.remove(from);
        let mut insert_at = to;
        if from < to {
            insert_at = insert_at.saturating_sub(1);
        }
        self.workspace.open_documents.insert(insert_at, tab);
        self.persist_session(cx);
        cx.notify();
        let _ = window;
    }

    /// F2 复制为 HTML：把选区（无选区时全文）渲染为 HTML 并写入剪贴板。
    pub(crate) fn copy_as_html(&mut self, cx: &mut Context<Self>) {
        let theme = cx.global::<ThemeManager>().current_arc();
        let markdown = self
            .selected_markdown_text(cx)
            .unwrap_or_else(|| self.current_document_source(cx));
        if markdown.trim().is_empty() {
            return;
        }
        let base_dir = self.file_path.as_ref().and_then(|path| path.parent().map(Path::to_path_buf));
        let title = self
            .file_path
            .as_ref()
            .and_then(|path| path.file_stem().map(|stem| stem.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "Velora".into());
        let html = crate::export::html::render_html_with_base_dir(
            &markdown,
            &theme,
            &title,
            base_dir.as_deref(),
        );
        cx.write_to_clipboard(copy_as_html_clipboard_item(html));
        cx.notify();
    }

    /// ⌥⌘←: return to the previous recorded caret location.
    pub(crate) fn on_cursor_history_back(
        &mut self,
        _: &CursorHistoryBack,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(location) = self.cursor_history_back.pop() else {
            return;
        };
        let snapshot = self.capture_source_selection_snapshot(cx);
        self.cursor_history_forward.push(CursorLocation {
            path: self.file_path.clone(),
            range: snapshot.range,
        });
        self.goto_cursor_location(location, window, cx);
    }

    /// ⌥⌘→: re-apply the most recently undone caret jump.
    pub(crate) fn on_cursor_history_forward(
        &mut self,
        _: &CursorHistoryForward,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(location) = self.cursor_history_forward.pop() else {
            return;
        };
        let snapshot = self.capture_source_selection_snapshot(cx);
        self.cursor_history_back.push(CursorLocation {
            path: self.file_path.clone(),
            range: snapshot.range,
        });
        self.goto_cursor_location(location, window, cx);
    }

    fn goto_cursor_location(
        &mut self,
        location: CursorLocation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let open_in_current = self.file_path.as_deref() == location.path.as_deref();
        if !open_in_current
            && let Some(path) = location.path.clone()
            && path.is_file()
        {
            self.open_workspace_file(path, window, cx);
        }
        self.apply_selection_snapshot_in_current_mode(
            &UndoSelectionSnapshot {
                range: location.range,
                reversed: false,
            },
            cx,
        );
        self.pending_scroll_active_block_into_view = true;
        self.pending_scroll_center_into_view = true;
        self.pending_scroll_recheck_after_layout = true;
        cx.notify();
    }

    fn jump_to_document_search_range(&mut self, range: Range<usize>, cx: &mut Context<Self>) {
        self.push_cursor_location(cx);
        self.apply_selection_snapshot_in_current_mode(
            &UndoSelectionSnapshot {
                range,
                reversed: false,
            },
            cx,
        );
        self.pending_scroll_active_block_into_view = true;
        self.pending_scroll_center_into_view = true;
        self.pending_scroll_recheck_after_layout = true;
        cx.notify();
    }

    pub(crate) fn open_document_find(&mut self, cx: &mut Context<Self>) {
        // Document search scans blocks, so a partially imported huge document
        // must finish importing first (roadmap G8).
        self.flush_pending_materialization(cx);
        self.workspace.is_open = true;
        self.workspace.active_tab = WorkspaceTab::Search;
        self.workspace.search_scope = WorkspaceSearchScope::Document;
        self.workspace.search_selected_range = 0..self.workspace.search_query.len();
        self.workspace.search_marked_range = None;
        self.workspace.search_focus_pending = true;
        self.schedule_workspace_search(cx);
        cx.notify();
    }

    /// Opens the workspace file named `target` (with `.md` appended when
    /// missing); creates it at the workspace root when no match exists
    /// (roadmap C3).
    pub(crate) fn open_wikilink(
        &mut self,
        target: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let needle = target.to_lowercase();
        let found = self.workspace_text_files().into_iter().find(|path| {
            let stem = path
                .file_stem()
                .map(|stem| stem.to_string_lossy().to_lowercase());
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().to_lowercase());
            stem.as_deref() == Some(needle.as_str())
                || name.as_deref() == Some(needle.as_str())
        });
        if let Some(path) = found {
            self.open_workspace_file(path, window, cx);
            return;
        }
        // Create `<target>.md` at the workspace root.
        let path = self
            .workspace
            .root
            .clone()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
            .join(format!("{target}.md"));
        if !path.exists() {
            if let Err(error) = fs::write(&path, format!("# {target}\n")) {
                self.workspace.file_error = Some(error.to_string());
                cx.notify();
                return;
            }
            self.refresh_workspace_tree(cx);
        }
        self.open_workspace_file(path, window, cx);
    }

    /// `#tag` 点击：打开搜索面板并以工作区范围列出同类（roadmap C4）。
    pub(crate) fn open_tag_search(&mut self, query: String, cx: &mut Context<Self>) {
        self.workspace.is_open = true;
        self.workspace.active_tab = WorkspaceTab::Search;
        self.workspace.search_scope = WorkspaceSearchScope::Workspace;
        self.workspace.search_query = query;
        self.workspace.search_selected_range = 0..self.workspace.search_query.len();
        self.workspace.search_marked_range = None;
        self.workspace.search_active_index = None;
        self.workspace.document_active_range = None;
        self.schedule_workspace_search(cx);
        cx.notify();
    }

    pub(crate) fn on_find_in_document(
        &mut self,
        _: &crate::components::FindInDocument,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_document_find(cx);
    }

    pub(crate) fn on_find_next_match(
        &mut self,
        _: &crate::components::FindNextMatch,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.find_next_document_match(false, cx);
    }

    pub(crate) fn on_find_previous_match(
        &mut self,
        _: &crate::components::FindPreviousMatch,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.find_next_document_match(true, cx);
    }

    pub(super) fn refresh_document_find_after_edit(&mut self, cx: &mut Context<Self>) {
        if self.workspace.is_open
            && self.workspace.active_tab == WorkspaceTab::Search
            && self.workspace.search_scope == WorkspaceSearchScope::Document
            && !self.workspace.search_query.trim().is_empty()
        {
            self.schedule_workspace_search(cx);
        }
    }

    pub(super) fn apply_pending_workspace_search_focus(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.search_focus_pending {
            let focus = self
                .workspace
                .search_focus
                .get_or_insert_with(|| cx.focus_handle())
                .clone();
            window.focus(&focus);
            self.workspace.search_focus_pending = false;
        }
    }

    pub(crate) fn find_next_document_match(&mut self, reverse: bool, cx: &mut Context<Self>) {
        if self.workspace.search_scope != WorkspaceSearchScope::Document {
            return;
        }
        let Some(source) = self.workspace.document_search_source.clone() else {
            return;
        };
        let matcher = SearchMatcher::new(self.workspace.search_query.trim(), self.search_options());
        let from = self
            .workspace
            .document_active_range
            .as_ref()
            .map(|range| if reverse { range.start } else { range.end })
            .unwrap_or(if reverse { source.len() } else { 0 });
        let Some(range) =
            find_document_match_from(&source, &matcher, from, reverse).or_else(|| {
                find_document_match_from(
                    &source,
                    &matcher,
                    if reverse { source.len() } else { 0 },
                    reverse,
                )
            })
        else {
            return;
        };
        self.workspace.search_active_index = self
            .workspace
            .search_results
            .iter()
            .position(|hit| hit.source_range.as_ref() == Some(&range));
        self.workspace.document_active_range = Some(range.clone());
        self.jump_to_document_search_range(range, cx);
    }

    /// Replaces the currently active document match (selected via a jump).
    pub(super) fn replace_active_document_match(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(range) = self.workspace.document_active_range.clone() else {
            return false;
        };
        let replacement = self.workspace.replace_query.clone();
        self.apply_selection_snapshot_in_current_mode(
            &UndoSelectionSnapshot {
                range,
                reversed: false,
            },
            cx,
        );
        self.replace_selected_block_text(&replacement, window, cx)
    }

    /// Replaces every current match in the open document, last-first so the
    /// earlier byte offsets stay valid while editing.
    pub(super) fn replace_all_document_matches(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> usize {
        let Some(source) = self.workspace.document_search_source.clone() else {
            return 0;
        };
        let matcher = SearchMatcher::new(self.workspace.search_query.trim(), self.search_options());
        let replacement = self.workspace.replace_query.clone();
        let mut ranges = Vec::new();
        let mut absolute = 0usize;
        for raw_line in source.split_inclusive('\n') {
            let line = raw_line.strip_suffix('\n').unwrap_or(raw_line);
            for found in matcher.find_in_line(line) {
                ranges.push(absolute + found.start..absolute + found.end);
            }
            absolute += raw_line.len();
        }
        let mut replaced = 0usize;
        for range in ranges.iter().rev() {
            self.apply_selection_snapshot_in_current_mode(
                &UndoSelectionSnapshot {
                    range: range.clone(),
                    reversed: false,
                },
                cx,
            );
            if self.replace_selected_block_text(&replacement, window, cx) {
                replaced += 1;
            }
        }
        self.workspace.document_active_range = None;
        replaced
    }

    /// Runs `replace_text_in_range` on the block that currently holds the
    /// selection (set by `apply_selection_snapshot_in_current_mode`). The
    /// target block may be outside the visible window, so it is resolved
    /// through the full-tree location map instead of the visible snapshot.
    fn replace_selected_block_text(
        &mut self,
        replacement: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(entity_id) = self.active_entity_id else {
            return false;
        };
        let Some(block) = self.document.block_entity_at_location(entity_id, cx) else {
            return false;
        };
        block.update(cx, |block, cx| {
            block.replace_text_in_range(None, replacement, window, cx);
        });
        true
    }

    /// Replaces matches across every file in the workspace tree. Open tabs get
    /// their cached markdown rewritten (and marked dirty); the active document
    /// is replaced live through the block editor so undo still works.
    pub(super) fn replace_all_workspace_matches(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> usize {
        let Some(tree) = self.workspace.file_tree.clone() else {
            return 0;
        };
        let matcher = SearchMatcher::new(self.workspace.search_query.trim(), self.search_options());
        let replacement = self.workspace.replace_query.clone();
        let active_path = self.file_path.clone();

        let files = collect_workspace_files(&tree);
        let mut total = 0usize;
        for path in files {
            if Some(&path) == active_path.as_ref() {
                // The active document goes through the live editor path so the
                // change is undoable and stays in sync with the block model.
                let source = self.current_document_source(cx);
                let scoped = SearchMatcher::new(
                    self.workspace.search_query.trim(),
                    self.search_options(),
                );
                let count = count_matches_in_source(&source, &scoped);
                if count > 0 {
                    self.workspace.document_search_source = Some(source);
                    total += self.replace_all_document_matches(window, cx);
                }
                continue;
            }
            let open_tab_index = self
                .workspace
                .open_documents
                .iter()
                .position(|tab| tab.path == path);
            let source = match open_tab_index {
                Some(index) if self.workspace.open_documents[index].dirty => {
                    self.workspace.open_documents[index].markdown.clone()
                }
                _ => match fs::read_to_string(&path) {
                    Ok(source) => source,
                    Err(_) => continue,
                },
            };
            let updated = replace_in_source(&source, &matcher, &replacement);
            if updated == source {
                continue;
            }
            total += count_matches_in_source(&source, &matcher);
            if fs::write(&path, &updated).is_ok() {
                if let Some(index) = open_tab_index {
                    let tab = &mut self.workspace.open_documents[index];
                    tab.markdown = updated;
                    tab.dirty = true;
                    tab.file_version = super::persistence::file_content_version(&tab.markdown);
                }
            } else {
                self.workspace.file_error = Some(format!("无法写入 {}", path.display()));
            }
        }
        if total > 0 {
            self.refresh_workspace_tree(cx);
        }
        total
    }

    fn toggle_workspace_node(&mut self, id: &str, cx: &mut Context<Self>) {
        if !self.workspace.expanded.remove(id) {
            self.workspace.expanded.insert(id.to_string());
        }
        cx.notify();
    }

    /// Creates a `name copy.ext` / `name copy 2.ext` duplicate of the selected
    /// file beside it (roadmap D6).
    pub(crate) fn duplicate_selected_file(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(source) = self.selected_workspace_path() else {
            return;
        };
        if source.is_dir() {
            return;
        }
        let Ok(contents) = fs::read(&source) else {
            return;
        };
        let parent = source.parent().unwrap_or(Path::new(""));
        let candidate = unique_workspace_copy_path(parent, &source);
        if let Err(error) = fs::write(&candidate, contents) {
            self.workspace.file_error = Some(error.to_string());
            cx.notify();
            return;
        }
        self.refresh_workspace_tree(cx);
        cx.notify();
        let _ = window;
    }

    /// 是否仍是空白欢迎态（未打开文件、未编辑、无标签）；用于启动窗口让位
    /// 给 Finder/`open` 的文件事件（roadmap G5）。
    pub(crate) fn is_pristine_startup_window(&self) -> bool {
        self.show_welcome
            && self.file_path.is_none()
            && !self.document_dirty
            && self.workspace_open_document_paths().is_empty()
    }

    /// 已打开文档的路径集合（崩溃恢复合并用，roadmap E10）。
    pub(crate) fn workspace_open_document_paths(&self) -> Vec<PathBuf> {
        self.workspace
            .open_documents
            .iter()
            .map(|tab| tab.path.clone())
            .collect()
    }

    /// 崩溃恢复合并（roadmap E10）：把恢复快照的未保存内容并入已打开的会话标签，
    /// 避免同一文件既出现在会话标签又弹出恢复窗口。快照 id 转移给该标签，保存后
    /// 由既有清理逻辑删除。返回 true 表示已合并。
    pub(crate) fn merge_recovery_snapshot(
        &mut self,
        path: &Path,
        markdown: &str,
        recovery_id: uuid::Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self
            .workspace
            .open_documents
            .iter()
            .any(|tab| tab.path == path)
        {
            return false;
        }
        if self.file_path.as_deref() != Some(path) {
            self.open_workspace_file(path.to_path_buf(), window, cx);
        }
        if self.file_path.as_deref() != Some(path) {
            return false;
        }
        self.replace_document_from_markdown(markdown.to_string(), Some(path.to_path_buf()), cx);
        self.recovery_id = recovery_id;
        self.mark_dirty(cx);
        if let Some(tab) = self
            .workspace
            .open_documents
            .iter_mut()
            .find(|tab| tab.path == path)
        {
            tab.markdown = markdown.to_string();
            tab.dirty = true;
            tab.recovery_id = recovery_id;
        }
        cx.notify();
        true
    }

    /// 测试用：读取会话标签的 (dirty, recovery_id, markdown)。
    #[cfg(test)]
    pub(crate) fn workspace_tab_state_for_test(
        &self,
        path: &Path,
    ) -> Option<(bool, uuid::Uuid, String)> {
        self.workspace
            .open_documents
            .iter()
            .find(|tab| tab.path == path)
            .map(|tab| (tab.dirty, tab.recovery_id, tab.markdown.clone()))
    }

    /// 测试用：按路径设置树选中项（等价于点击该节点）。
    #[cfg(test)]
    pub(crate) fn select_workspace_path_for_test(
        &mut self,
        path: PathBuf,
        cx: &mut Context<Self>,
    ) {
        self.workspace.selected = Some(if path.is_dir() {
            WorkspaceSelection::Directory(path)
        } else {
            WorkspaceSelection::File(path)
        });
        cx.notify();
    }

    /// 树右键「复制」：记录源文件并写入系统剪贴板（roadmap D6）。
    pub(crate) fn copy_selected_workspace_file(&mut self, cx: &mut Context<Self>) {
        let Some(source) = self.selected_workspace_path() else {
            return;
        };
        if source.is_dir() {
            return;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(
            source.to_string_lossy().into_owned(),
        ));
        self.tree_clipboard = Some(source);
        cx.notify();
    }

    /// 树右键「粘贴」：把已复制的文件或剪贴板图片落到目标目录（roadmap D6）。
    pub(crate) fn paste_into_workspace_tree(&mut self, cx: &mut Context<Self>) {
        let Some(target_dir) = self.workspace_paste_target_dir() else {
            return;
        };
        if let Err(error) = fs::create_dir_all(&target_dir) {
            self.workspace.file_error = Some(error.to_string());
            cx.notify();
            return;
        }

        if let Some(source) = self.tree_clipboard.clone().filter(|path| path.is_file()) {
            match fs::read(&source) {
                Ok(contents) => {
                    let candidate = unique_workspace_copy_path(&target_dir, &source);
                    if let Err(error) = fs::write(&candidate, contents) {
                        self.workspace.file_error = Some(error.to_string());
                    } else {
                        self.refresh_workspace_tree(cx);
                    }
                    cx.notify();
                    return;
                }
                Err(error) => {
                    self.workspace.file_error = Some(error.to_string());
                    cx.notify();
                    return;
                }
            }
        }

        let image = cx.read_from_clipboard().and_then(|item| {
            item.entries().iter().find_map(|entry| match entry {
                gpui::ClipboardEntry::Image(image) => Some(image.clone()),
                gpui::ClipboardEntry::String(_) => None,
            })
        });
        let Some(image) = image else {
            self.workspace.file_error = Some(
                cx.global::<crate::i18n::I18nManager>()
                    .strings()
                    .workspace_paste_empty
                    .clone(),
            );
            cx.notify();
            return;
        };

        let file_name = format!(
            "{}-{}.{}",
            crate::config::today_local_date(),
            pasted_image_bytes_hash(&image.bytes),
            clipboard_image_extension(image.format)
        );
        let candidate = unique_workspace_file_name(&target_dir, &file_name);
        if let Err(error) = fs::write(&candidate, &image.bytes) {
            self.workspace.file_error = Some(error.to_string());
        } else {
            self.refresh_workspace_tree(cx);
        }
        cx.notify();
    }

    /// 粘贴目标目录：选中目录用其本身，选中文件用其父目录，否则工作区根。
    fn workspace_paste_target_dir(&self) -> Option<PathBuf> {
        match self.workspace.selected.as_ref() {
            Some(WorkspaceSelection::Directory(path)) => Some(path.clone()),
            Some(WorkspaceSelection::File(path)) => {
                if path.is_dir() {
                    Some(path.clone())
                } else {
                    path.parent().map(Path::to_path_buf)
                }
            }
            Some(WorkspaceSelection::WorkspaceRoot(path)) => Some(path.clone()),
            _ => self.workspace.root.clone(),
        }
    }

    /// Double-clicking an outline heading enters rename mode: the caret jumps
    /// to the heading with its title text selected, so typing replaces it
    /// directly in the document (roadmap C6).
    pub(crate) fn rename_outline_heading(&mut self, line: usize, cx: &mut Context<Self>) {
        let source = self.last_stable_source_text.clone();
        let line_start = source
            .split_inclusive('\n')
            .take(line)
            .map(str::len)
            .sum::<usize>()
            .min(source.len());
        let line_end = source[line_start..]
            .find('\n')
            .map(|offset| line_start + offset)
            .unwrap_or(source.len());
        let line_text = &source[line_start..line_end];
        let marker_len = line_text.chars().take_while(|ch| *ch == '#').count();
        let after_marker = &line_text[marker_len..];
        let spaces = after_marker.len() - after_marker.trim_start().len();
        let title_start = (line_start + marker_len + spaces).min(line_end);
        if !source.is_char_boundary(title_start) {
            return;
        }
        self.jump_to_document_search_range(title_start..line_end, cx);
    }

    /// Clicking an outline heading jumps to that heading and expands it so its
    /// children become visible.
    fn open_outline_node(&mut self, id: String, line: usize, cx: &mut Context<Self>) {
        self.workspace.selected = Some(WorkspaceSelection::Outline(id.clone()));
        self.workspace.expanded.insert(id);
        // The outline is built from `last_stable_source_text`, so compute the
        // heading's byte range against that same snapshot.
        let source = self.last_stable_source_text.clone();
        let line_start = source
            .split_inclusive('\n')
            .take(line)
            .map(str::len)
            .sum::<usize>()
            .min(source.len());
        let line_end = source[line_start..]
            .find('\n')
            .map(|offset| line_start + offset)
            .unwrap_or(source.len());
        // 折叠的标题被点击时先展开，使章节内容可见（roadmap C7）。
        if let Some(heading) = self.heading_block_at_source_line(line, cx) {
            heading.update(cx, |block, _cx| {
                if block.folded {
                    block.folded = false;
                    self.fold_state_version = self.fold_state_version.wrapping_add(1);
                }
            });
        }
        if source.is_char_boundary(line_start) && source.is_char_boundary(line_end) {
            self.jump_to_document_search_range(line_start..line_end, cx);
        } else {
            cx.notify();
        }
    }

    /// Finds the heading block whose source line equals `line`.
    fn heading_block_at_source_line(
        &self,
        line: usize,
        cx: &App,
    ) -> Option<Entity<super::Block>> {
        let (_, ranges) = self.build_source_target_mappings_with_block_ranges(cx);
        let source = self.current_document_source(cx);
        let line_start = source
            .split_inclusive('\n')
            .take(line)
            .map(str::len)
            .sum::<usize>()
            .min(source.len());
        ranges
            .iter()
            .find(|(_, range)| range.contains(&line_start) || range.start == line_start)
            .map(|(entity_id, _)| *entity_id)
            .and_then(|entity_id| self.document.block_entity_at_location(entity_id, cx))
    }

    /// Expands the file tree to the given path so it is visible (roadmap D1).
    pub(super) fn reveal_path_in_tree(&mut self, path: &Path) {
        let Some(root) = self.workspace.root.as_ref() else {
            return;
        };
        let Ok(relative) = path.strip_prefix(root) else {
            return;
        };
        // Directory node ids are `file:{path}` (see file_node_id); expand every
        // ancestor of the target.
        let mut ancestor = root.clone();
        for component in relative.components().take(relative.components().count().saturating_sub(1)) {
            if matches!(component, std::path::Component::Normal(_)) {
                ancestor.push(component.as_os_str());
                self.workspace
                    .expanded
                    .insert(format!("file:{}", ancestor.to_string_lossy()));
            }
        }
    }

    pub(crate) fn open_workspace_file(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_workspace_file_in_mode(path, WorkspaceOpenMode::Pinned, window, cx);
    }

    /// 按「单击预览 / 双击固定」的模式打开工作区文件（用户需求）。
    pub(super) fn open_workspace_file_in_mode(
        &mut self,
        path: PathBuf,
        mode: WorkspaceOpenMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.file_path.as_ref() == Some(&path) {
            // 已是当前文档：双击树节点要把已打开的预览标签升级为固定。
            if mode == WorkspaceOpenMode::Pinned
                && let Some(tab) = self
                    .workspace
                    .open_documents
                    .iter_mut()
                    .find(|tab| tab.path == path && tab.preview)
            {
                tab.preview = false;
                cx.notify();
            }
            return;
        }
        // 单击/双击打开要替换旧的未修改预览标签：预览只在停留期间占据标签栏，
        // 一旦切走就消失（用户需求），已修改的预览保留。这里只记录待删清单，
        // 真正删除放在函数末尾——打开流程中的 snapshot_current_document 会把
        // 旧活动文档推回标签集，提前删会被它再加回来。
        let stale_previews: Vec<PathBuf> = if mode == WorkspaceOpenMode::Activate {
            Vec::new()
        } else {
            self.workspace
                .open_documents
                .iter()
                .filter(|tab| tab.preview && !tab.dirty && tab.path != path)
                .map(|tab| tab.path.clone())
                .collect()
        };
        // Sniff the content, not the extension: dotfiles like .gitignore have
        // no extension but are text, while a .md full of NUL bytes is not
        // renderable. Non-text files still become the active tab; the content
        // area shows a centered placeholder.
        if has_utf16_bom(&path) {
            let strings = cx.global::<I18nManager>().strings().clone();
            self.show_welcome = false;
            self.show_preview_unavailable_with_detail(
                path.clone(),
                Some(strings.encoding_not_supported.clone()),
                window,
                cx,
            );
            return;
        }
        if !is_likely_text_file(&path) {
            self.show_welcome = false;
            self.show_preview_unavailable(path.clone(), window, cx);
            return;
        }
        self.unsupported_preview_path = None;
        self.unsupported_preview_detail = None;
        self.show_welcome = false;
        if self.file_path.is_none() && self.document_dirty {
            self.request_dropped_markdown_replace(path, window, cx);
            return;
        }

        if !self.has_external_autosave_conflict() {
            self.workspace.file_error = None;
        }
        self.snapshot_current_document(cx);
        let cached = self
            .workspace
            .open_documents
            .iter()
            .find(|tab| tab.path == path)
            .cloned();
        let (markdown, dirty, recovery_id, file_version) = if let Some(tab) = cached {
            if tab.dirty {
                (tab.markdown, true, tab.recovery_id, tab.file_version)
            } else {
                match fs::read_to_string(&path) {
                    Ok(markdown) => {
                        let file_version = super::persistence::file_content_version(&markdown);
                        (markdown, false, tab.recovery_id, file_version)
                    }
                    Err(err) => {
                        self.workspace.file_error = Some(err.to_string());
                        cx.notify();
                        return;
                    }
                }
            }
        } else {
            match fs::read_to_string(&path) {
                Ok(markdown) => (
                    markdown.clone(),
                    false,
                    uuid::Uuid::new_v4(),
                    super::persistence::file_content_version(&markdown),
                ),
                Err(err) => {
                    self.workspace.file_error = Some(err.to_string());
                    cx.notify();
                    return;
                }
            }
        };
        // 从磁盘重新读取成功即视为用户接受磁盘内容，该文件的冲突解除。
        if !dirty {
            self.clear_external_change_conflict_for(&path);
        }
        let preview = mode != WorkspaceOpenMode::Pinned;
        if !self
            .workspace
            .open_documents
            .iter()
            .any(|tab| tab.path == path)
        {
            self.workspace.open_documents.push(WorkspaceDocumentTab {
                path: path.clone(),
                recovery_id,
                file_version,
                markdown: markdown.clone(),
                dirty,
                preview,
            });
        }
        if let Some(tab) = self
            .workspace
            .open_documents
            .iter_mut()
            .find(|tab| tab.path == path)
        {
            tab.markdown = markdown.clone();
            tab.dirty = dirty;
            tab.file_version = file_version;
            if mode == WorkspaceOpenMode::Pinned {
                tab.preview = false;
            }
        }
        self.recovery_id = recovery_id;
        self.file_version = Some(file_version);
        self.recovery_source_path = None;
        self.is_recovered_document = false;
        self.workspace.active_document = Some(path.clone());
        self.workspace.selected = Some(WorkspaceSelection::File(path.clone()));
        self.reveal_path_in_tree(&path);
        // Markdown rendering is for .md/.markdown only; every other text file
        // (code, dotfiles, plain text) opens as monospace source text.
        if is_markdown_document(&path) {
            self.replace_document_from_markdown(markdown, Some(path), cx);
        } else {
            self.replace_document_from_code_source(markdown, path, cx);
        }
        self.document_dirty = dirty;
        self.file_version = Some(file_version);
        if dirty || self.has_dirty_workspace_documents() {
            self.schedule_autosave(cx);
        }
        window.set_window_edited(dirty);
        // 此刻打开流程（含旧活动文档的快照回写）已结束，替换掉的未修改预览
        // 标签可以安全移除了。
        if !stale_previews.is_empty() {
            self.workspace
                .open_documents
                .retain(|tab| !stale_previews.contains(&tab.path));
        }
        self.persist_session(cx);
        cx.notify();
    }

    /// Makes the picked file the active tab but shows a centered
    /// "can't preview" placeholder instead of editor content.
    fn show_preview_unavailable(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_preview_unavailable_with_detail(path, None, window, cx);
    }

    pub(crate) fn show_preview_unavailable_with_detail(
        &mut self,
        path: PathBuf,
        detail: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_welcome = false;
        if let Some(existing) = self
            .workspace
            .open_documents
            .iter()
            .position(|tab| tab.path == path)
        {
            self.workspace.open_documents.remove(existing);
        }
        self.workspace.open_documents.push(WorkspaceDocumentTab {
            path: path.clone(),
            recovery_id: uuid::Uuid::new_v4(),
            file_version: 0,
            markdown: String::new(),
            dirty: false,
            preview: false,
        });
        self.workspace.active_document = Some(path.clone());
        self.workspace.selected = Some(WorkspaceSelection::File(path.clone()));
        self.reveal_path_in_tree(&path);
        self.unsupported_preview_detail = detail;
        self.unsupported_preview_path = Some(path);
        self.file_path = None;
        self.document_dirty = false;
        window.set_window_edited(false);
        cx.notify();
    }

    fn open_tab_context_menu(
        &mut self,
        position: Point<Pixels>,
        target_index: usize,
        cx: &mut Context<Self>,
    ) {
        self.dismiss_contextual_overlays(cx);
        self.workspace.tab_context_menu = Some(TabContextMenu {
            position,
            target_index,
        });
        cx.notify();
    }

    pub(super) fn close_tab_context_menu(&mut self, cx: &mut Context<Self>) {
        if self.workspace.tab_context_menu.take().is_some() {
            cx.notify();
        }
    }

    pub(super) fn render_tab_context_menu_overlay(
        &self,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let menu = self.workspace.tab_context_menu?;
        let target = self.workspace.open_documents.get(menu.target_index)?;
        let target_path = target.path.clone();
        let count = self.workspace.open_documents.len();
        let strings = cx.global::<crate::i18n::I18nManager>().strings();
        let mut actions = vec![(strings.tab_close.clone(), TabMenuAction::Close)];
        if count > 1 {
            actions.push((strings.tab_close_others.clone(), TabMenuAction::CloseOthers));
            if menu.target_index > 0 {
                actions.push((strings.tab_close_left.clone(), TabMenuAction::CloseLeft));
            }
            if menu.target_index + 1 < count {
                actions.push((strings.tab_close_right.clone(), TabMenuAction::CloseRight));
            }
            actions.push((strings.tab_close_all.clone(), TabMenuAction::CloseAll));
        }

        let width = 200.0;
        let height = actions.len() as f32 * 32.0 + 8.0;
        let viewport = window.viewport_size();
        let left = f32::from(menu.position.x)
            .min((f32::from(viewport.width) - width - 8.0).max(8.0))
            .max(8.0);
        let top = f32::from(menu.position.y)
            .min((f32::from(viewport.height) - height - 8.0).max(8.0))
            .max(8.0);
        let editor = cx.entity().downgrade();
        let rows = actions
            .into_iter()
            .enumerate()
            .map(|(index, (label, action))| {
                let editor = editor.clone();
                let target_path = target_path.clone();
                div()
                    .id(("tab-context-action", index))
                    .h(px(32.0))
                    .px(px(10.0))
                    .flex()
                    .items_center()
                    .rounded(px(5.0))
                    .cursor_pointer()
                    .text_size(px(12.0))
                    .text_color(theme.colors.dialog_body)
                    .hover(|this| this.bg(theme.colors.dialog_secondary_button_hover))
                    .child(label)
                    .on_click(move |_, window, cx| {
                        let _ = editor.update(cx, |editor, cx| {
                            editor.workspace.tab_context_menu = None;
                            editor.close_workspace_tabs_for_action(&target_path, action, window, cx);
                            cx.notify();
                        });
                        cx.stop_propagation();
                    })
            })
            .collect::<Vec<_>>();
        let close_editor_left = editor.clone();
        let close_editor_right = editor.clone();
        Some(
            div()
                .id("tab-context-overlay")
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .bottom_0()
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = close_editor_left
                        .update(cx, |editor, cx| editor.close_tab_context_menu(cx));
                })
                .on_mouse_down(MouseButton::Right, move |_, _, cx| {
                    let _ = close_editor_right
                        .update(cx, |editor, cx| editor.close_tab_context_menu(cx));
                })
                .child(
                    div()
                        .id("tab-context-panel")
                        .absolute()
                        .left(px(left))
                        .top(px(top))
                        .w(px(width))
                        .p(px(4.0))
                        .rounded(px(8.0))
                        .border_1()
                        .border_color(theme.colors.dialog_border)
                        .bg(theme.colors.dialog_surface)
                        .shadow_md()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .children(rows),
                )
                .into_any_element(),
        )
    }

    /// Closes one tab, prompting before discarding unsaved edits.
    pub(super) fn close_workspace_document(
        &mut self,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_workspace_tabs(std::slice::from_ref(&path.to_path_buf()), window, cx);
    }

    fn close_workspace_tabs_for_action(
        &mut self,
        target: &Path,
        action: TabMenuAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let paths: Vec<PathBuf> = {
            let tabs = &self.workspace.open_documents;
            let Some(index) = tabs.iter().position(|tab| tab.path == target) else {
                return;
            };
            match action {
                TabMenuAction::Close => vec![target.to_path_buf()],
                TabMenuAction::CloseOthers => tabs
                    .iter()
                    .enumerate()
                    .filter(|(tab_index, _)| *tab_index != index)
                    .map(|(_, tab)| tab.path.clone())
                    .collect(),
                TabMenuAction::CloseLeft => tabs[..index]
                    .iter()
                    .map(|tab| tab.path.clone())
                    .collect(),
                TabMenuAction::CloseRight => tabs[index + 1..]
                    .iter()
                    .map(|tab| tab.path.clone())
                    .collect(),
                TabMenuAction::CloseAll => tabs.iter().map(|tab| tab.path.clone()).collect(),
            }
        };
        self.close_workspace_tabs(&paths, window, cx);
    }

    /// Closes the listed tabs. Unsaved edits are confirmed once for the whole
    /// batch; saving writes each tab's cached markdown back to disk.
    fn close_workspace_tabs(
        &mut self,
        paths: &[PathBuf],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if paths.is_empty() {
            return;
        }
        // Capture the live editor content into its tab before deciding what is
        // dirty, so closing the active document sees up-to-date state.
        self.snapshot_current_document(cx);
        self.dismiss_contextual_overlays(cx);

        let closing: Vec<WorkspaceDocumentTab> = self
            .workspace
            .open_documents
            .iter()
            .filter(|tab| paths.contains(&tab.path))
            .cloned()
            .collect();
        if closing.is_empty() {
            return;
        }
        let dirty: Vec<WorkspaceDocumentTab> = closing
            .iter()
            .filter(|tab| tab.dirty)
            .cloned()
            .collect();

        if dirty.is_empty() {
            // Already inside this Editor's update context (the click handler
            // wraps everything in editor.update); re-entering update here
            // would panic.
            self.finish_close_workspace_tabs(&closing, false, window, cx);
            return;
        }

        let strings = cx.global::<crate::i18n::I18nManager>().strings().clone();
        let (message, detail) = if dirty.len() == 1 {
            let name = dirty[0]
                .path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| dirty[0].path.to_string_lossy().into_owned());
            (
                strings.tab_close_dirty_message_one.replace("{name}", &name),
                String::new(),
            )
        } else {
            (
                strings
                    .tab_close_dirty_message_many
                    .replace("{count}", &dirty.len().to_string()),
                String::new(),
            )
        };
        let detail = (!detail.is_empty()).then_some(detail);
        // 关闭多个未保存标签的确认同样走应用内模态（用户要求：不用系统原生弹窗）。
        self.show_modal(
            ModalSpec {
                title: message.into(),
                detail: detail.map(Into::into),
                buttons: vec![
                    strings.unsaved_changes_save_and_close.clone().into(),
                    strings.unsaved_changes_discard_and_close.clone().into(),
                    strings.open_link_cancel.clone().into(),
                ],
                default_index: 0,
                cancel_index: 2,
            },
            move |choice, editor, window, cx| match choice {
                0 => editor.finish_close_workspace_tabs(&closing, true, window, cx),
                1 => editor.finish_close_workspace_tabs(&closing, false, window, cx),
                _ => {}
            },
            cx,
        );
    }

    /// Removes closed tabs from the strip, optionally saving their cached
    /// content first, and activates the nearest remaining neighbour.
    fn finish_close_workspace_tabs(
        &mut self,
        closing: &[WorkspaceDocumentTab],
        save_first: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if save_first {
            for tab in closing {
                if !tab.dirty {
                    continue;
                }
                match std::fs::write(&tab.path, tab.markdown.as_str()) {
                    Ok(()) => {
                        let _ = crate::config::remove_recovery_snapshot(tab.recovery_id);
                    }
                    Err(err) => {
                        self.workspace.file_error = Some(err.to_string());
                    }
                }
            }
        }

        let closing_paths: Vec<PathBuf> = closing.iter().map(|tab| tab.path.clone()).collect();
        let active_was_closed = closing_paths
            .iter()
            .any(|path| self.workspace.active_document.as_ref() == Some(path));
        let first_closed_index = self
            .workspace
            .open_documents
            .iter()
            .position(|tab| closing_paths.contains(&tab.path));

        self.workspace
            .open_documents
            .retain(|tab| !closing_paths.contains(&tab.path));

        if active_was_closed {
            let next_path = first_closed_index.and_then(|index| {
                self.workspace
                    .open_documents
                    .get(index)
                    .or_else(|| {
                        index
                            .checked_sub(1)
                            .and_then(|previous| self.workspace.open_documents.get(previous))
                    })
                    .map(|tab| tab.path.clone())
            });
            // Detach the closing document from the editor first so snapshot /
            // dirty-guard logic inside open_workspace_file cannot resurrect it.
            self.file_path = None;
            self.document_dirty = false;
            if let Some(next_path) = next_path {
                self.open_workspace_file(next_path, window, cx);
            } else {
                self.workspace.active_document = None;
                self.workspace.selected = None;
                self.replace_document_from_markdown(String::new(), None, cx);
                window.set_window_edited(false);
                self.show_welcome = true;
            }
        }
        if self.document_dirty || self.has_dirty_workspace_documents() {
            self.schedule_autosave(cx);
        }
        self.persist_session(cx);
        cx.notify();
    }

    pub(super) fn render_document_tabs(
        &mut self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        self.ensure_current_document_tab(cx);
        if self.workspace.open_documents.is_empty() {
            return None;
        }

        let editor = cx.entity().downgrade();
        let c = &theme.colors;
        let tabs = self
            .workspace
            .open_documents
            .iter()
            .enumerate()
            .map(|(index, tab)| {
                let path = tab.path.clone();
                let click_path = path.clone();
                let middle_click_path = path.clone();
                let drag_from_path = path.clone();
                let drop_target_path = path.clone();
                let close_path = path.clone();
                let active = self.workspace.active_document.as_ref() == Some(&tab.path);
                let dirty = if active {
                    self.document_dirty
                } else {
                    tab.dirty
                };
                let title = tab
                    .path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| tab.path.to_string_lossy().into_owned());
                let tab_editor = editor.clone();
                let context_editor = editor.clone();
                // Markdown glyph only for real markdown files; code files and
                // extension-less dotfiles render as source, binaries show as
                // placeholders — neither is markdown.
                let tab_shows_code_icon = is_code_file(&path)
                    || path.extension().is_none()
                    || self.unsupported_preview_path.as_ref() == Some(&path);
                // The close button carries its own hover state: highlighting
                // the whole tab was indistinguishable from the tab's own
                // hover background, so the X lights up only under the pointer.
                let close_button = div()
                    .id(("document-tab-close", index))
                    .group("doc-tab-close")
                    .w(px(16.0))
                    .h(px(16.0))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(4.0))
                    .hover(|this| {
                        // One step darker than the tab's own hover fill —
                        // neutral, no loud accent colors.
                        this.bg({
                            let mut bg = c.dialog_secondary_button_hover;
                            bg.l = (bg.l - 0.05).max(0.0);
                            bg
                        })
                    })
                    .cursor_pointer()
                    .child({
                        // Faded body color rather than dialog_muted: inactive
                        // tabs show no other text, so the X needs to stand on
                        // its own while staying quieter than the tab title.
                        let mut idle_icon = c.text_default;
                        idle_icon.a *= 0.6;
                        svg()
                            .path(TAB_CLOSE_ICON)
                            .size(px(10.0))
                            .text_color(idle_icon)
                            .group_hover("doc-tab-close", |this| this.text_color(c.text_default))
                    })
                    .on_click({
                        let close_editor = editor.clone();
                        move |_event, window, cx| {
                            let _ = close_editor.update(cx, |editor, cx| {
                                editor.close_workspace_document(&close_path, window, cx);
                            });
                            cx.stop_propagation();
                        }
                    })
                    .into_any_element();
                div()
                    .id(("document-tab", stable_node_hash(&path.to_string_lossy())))
                    .debug_selector(move || format!("document-tab-{index}"))
                    .group("doc-tab")
                    .h_full()
                    .min_w(px(120.0))
                    .max_w(px(220.0))
                    .px(px(10.0))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .relative()
                    .border_r(px(1.0))
                    .border_color(c.dialog_border)
                    .bg(if active {
                        c.editor_background
                    } else {
                        hsla(0.0, 0.0, 0.0, 0.0)
                    })
                    .hover(|this| {
                        this.bg(if active {
                            c.editor_background
                        } else {
                            c.dialog_secondary_button_hover
                        })
                    })
                    .cursor_pointer()
                    .text_size(px(12.0))
                    .text_color(if active {
                        c.text_default
                    } else {
                        c.dialog_muted
                    })
                    .children(active.then(|| {
                        div()
                            .absolute()
                            .bottom_0()
                            .left_0()
                            .right_0()
                            .h(px(2.0))
                            .bg(c.dialog_primary_button_bg)
                    }))
                    .child(
                        div()
                            .w(px(11.0))
                            .text_center()
                            .text_size(px(9.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(if tab_shows_code_icon {
                                c.dialog_muted
                            } else {
                                c.dialog_primary_button_bg
                            })
                            .child(if tab_shows_code_icon { "⌘" } else { "M" }),
                    )
                    .child({
                        // 预览标签用斜体区分（用户需求：单击预览/双击固定）。
                        let mut title_el = div().flex_1().min_w(px(0.0)).truncate();
                        if tab.preview {
                            title_el = title_el.italic();
                        }
                        title_el.child(title)
                    })
                    .children((dirty && !active).then(|| {
                        div()
                            .w(px(7.0))
                            .h(px(7.0))
                            .flex_shrink_0()
                            .rounded(px(4.0))
                            .bg(c.dialog_primary_button_bg)
                    }))
                    .child(close_button)
                    .on_click(move |_event, window, cx| {
                        let _ = tab_editor.update(cx, |editor, cx| {
                            // 点标签栏只是激活：不把预览标签升级为固定（用户需求
                            // 的预览语义：只有双击树节点或产生修改才固定）。
                            editor.open_workspace_file_in_mode(
                                click_path.clone(),
                                WorkspaceOpenMode::Activate,
                                window,
                                cx,
                            );
                        });
                    })
                    .on_drag(
                        TabDrag {
                            from_path: drag_from_path.clone(),
                        },
                        move |drag, _offset, _window, cx| {
                            let label = drag
                                .from_path
                                .file_name()
                                .map(|name| name.to_string_lossy().into_owned())
                                .unwrap_or_default();
                            cx.new(|_| DraggedTabPreview {
                                label: label.into(),
                            })
                        },
                    )
                    .drag_over::<TabDrag>({
                        let drag_hover_path = path.clone();
                        move |style, drag, _window, cx| {
                            if drag.from_path == drag_hover_path {
                                return style;
                            }
                            // 拖拽中的目标位置高亮（roadmap E3）：左侧强调边 +
                            // 悬浮底色，明确落点。
                            let theme = cx.global::<ThemeManager>().current_arc();
                            style
                                .border_l(px(2.0))
                                .border_color(theme.colors.dialog_primary_button_bg)
                                .bg(theme.colors.dialog_secondary_button_hover)
                        }
                    })
                    .on_drop({
                        let drop_editor = editor.clone();
                        move |drag: &TabDrag, window, cx| {
                        let _ = drop_editor.update(cx, |editor, cx| {
                            editor.move_tab_to_position(
                                &drag.from_path,
                                &drop_target_path,
                                window,
                                cx,
                            );
                        });
                        }
                    })
                    .on_mouse_down(MouseButton::Middle, {
                        let middle_editor = editor.clone();
                        move |_event, window, cx| {
                            let _ = middle_editor.update(cx, |editor, cx| {
                                editor.close_workspace_document(&middle_click_path, window, cx);
                            });
                            cx.stop_propagation();
                        }
                    })
                    .on_mouse_down(MouseButton::Right, move |event, _window, cx| {
                        let _ = context_editor.update(cx, |editor, cx| {
                            editor.open_tab_context_menu(event.position, index, cx);
                        });
                        cx.stop_propagation();
                    })
                    .into_any_element()
            })
            .collect::<Vec<_>>();

        Some(
            div()
                .id("document-tabs")
                .h_full()
                .min_w(px(0.0))
                .flex()
                .track_scroll(&self.workspace.tabs_scroll_handle)
                .overflow_x_scroll()
                .children(tabs)
                .into_any_element(),
        )
    }

    pub(super) fn code_tab_active(&self) -> bool {
        self.code_document
    }

    pub(super) fn workspace_breadcrumb(&self) -> String {
        self.file_path
            .as_ref()
            .or(self.recovery_source_path.as_ref())
            .and_then(|path| path.file_name())
            .or_else(|| {
                self.workspace
                    .root
                    .as_ref()
                    .and_then(|path| path.file_name())
            })
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    pub(super) fn active_code_line_count(&self, cx: &App) -> Option<usize> {
        self.code_tab_active().then(|| {
            // P4a：按修订缓存；逐块统计换行片段数，禁止整篇重新序列化
            // （此前每帧 raw_source_text 一次，10 MiB 文档是每帧的灾难）。
            let revision = self.document_revision;
            if let Some((cached_revision, lines)) = self.code_line_count_cache.get()
                && cached_revision == revision
            {
                return lines;
            }
            let mut pieces = 0usize;
            for visible in self.document.visible_blocks() {
                pieces += visible.entity.read(cx).display_text().split('\n').count();
            }
            if let Some(tail) = self.document.pending_tail() {
                pieces += tail.lines.len() - tail.next_line;
            }
            if let Some(tail) = self.document.pending_source() {
                pieces += tail.source.split('\n').count().saturating_sub(1);
            }
            let lines = pieces.max(1);
            self.code_line_count_cache.set(Some((revision, lines)));
            lines
        })
    }

    pub(super) fn current_workspace_panel_width(&self, viewport_width: f32, cx: &App) -> f32 {
        let width = self
            .workspace
            .panel_width
            .unwrap_or_else(|| crate::config::EditorSettings::workspace_sidebar_width(cx) as f32);
        clamp_workspace_panel_width(width, viewport_width)
    }

    fn start_workspace_resize(&mut self, pointer_x: f32, width: f32, cx: &mut Context<Self>) {
        self.workspace.resize_drag = Some(WorkspaceResizeDrag {
            start_x: pointer_x,
            start_width: width,
        });
        cx.notify();
    }

    pub(super) fn on_workspace_resize_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(drag) = self.workspace.resize_drag else {
            return;
        };
        let viewport = f32::from(window.viewport_size().width);
        let width = clamp_workspace_panel_width(
            drag.start_width + f32::from(event.position.x) - drag.start_x,
            viewport,
        );
        self.workspace.panel_width = Some(width);
        cx.notify();
    }

    pub(super) fn on_workspace_resize_mouse_up(
        &mut self,
        _event: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.resize_drag.take().is_some() {
            if let Some(width) = self.workspace.panel_width {
                crate::config::EditorSettings::set_workspace_sidebar_width(
                    cx,
                    width.round() as u16,
                );
            }
            cx.notify();
        }
    }

    pub(super) fn render_workspace_panel(
        &mut self,
        theme: &Theme,
        strings: &I18nStrings,
        panel_width: f32,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.workspace.is_open && !self.sidebar_peek && !self.sidebar_overlay_closing {
            // 侧栏收起也要同步文档大纲：块级 `[TOC]` 的条目来自这里，曾因
            // 「启动不展开侧边栏」回归成空目录（outline 按文档源去重，收起
            // 时每帧只付一次字符串比较）。文件树同步仍留给打开的抽屉。
            // 收回动画期间面板要继续渲染（浮层还在滑出），所以只在完全
            // 静止的收起状态才早退。
            self.sync_workspace_outline(cx);
            return None;
        }

        self.sync_workspace_models(cx);
        let editor = cx.entity().downgrade();
        let resize_editor = editor.clone();
        let c = &theme.colors;
        let d = &theme.dimensions;

        let search_header = (self.workspace.active_tab == WorkspaceTab::Search)
            .then(|| self.render_search_header(theme, strings, window, cx));
        let tree_sort_header = (self.workspace.active_tab == WorkspaceTab::Files)
            .then(|| self.render_tree_filter_and_sort_header(theme, strings, window, cx));
        let body = match self.workspace.active_tab {
            WorkspaceTab::Files => self.render_workspace_files_tree(theme, strings, &editor),
            WorkspaceTab::Search => self.render_search_results(theme, strings, &editor),
            WorkspaceTab::Outline => self.render_workspace_outline_tree(theme, strings, &editor),
        };

        Some(
            div()
                .id("workspace-panel")
                .relative()
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(Self::on_workspace_background_right_click),
                )
                .h_full()
                .w(px(panel_width))
                .flex()
                .flex_col()
                .flex_shrink_0()
                .bg(c.dialog_secondary_button_bg)
                .border_r(px(d.dialog_border_width))
                .border_color(c.dialog_border)
                .children(search_header)
                .children(tree_sort_header)
                .child(
                    div()
                        .id("workspace-panel-scroll")
                        .flex_1()
                        .min_h(px(0.0))
                        .overflow_y_scroll()
                        .track_scroll(&self.workspace.tree_scroll_handle)
                        .px(px(4.0))
                        .py(px(6.0))
                        .child(body),
                )
                .child(
                    div()
                        .id("workspace-resize-handle")
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .right_0()
                        .w(px(6.0))
                        .cursor(CursorStyle::ResizeLeftRight)
                        .hover(|this| this.bg(c.selection))
                        .on_mouse_down(MouseButton::Left, move |event, _, cx| {
                            let _ = resize_editor.update(cx, |editor, cx| {
                                editor.start_workspace_resize(
                                    f32::from(event.position.x),
                                    panel_width,
                                    cx,
                                );
                            });
                            cx.stop_propagation();
                        }),
                )
                .into_any_element(),
        )
    }

    /// 单击 = 预览打开，双击 = 固定打开（用户需求）；键盘触发的点击按固定处理。
    fn tree_click_open_mode(event: &ClickEvent) -> WorkspaceOpenMode {
        match event {
            ClickEvent::Mouse(mouse) if mouse.up.click_count >= 2 => WorkspaceOpenMode::Pinned,
            ClickEvent::Mouse(_) => WorkspaceOpenMode::Preview,
            ClickEvent::Keyboard(_) => WorkspaceOpenMode::Pinned,
        }
    }

    /// 文件树过滤：非空查询时显示匹配文件的扁平列表（roadmap D8）。
    fn render_tree_filter_row(
        &mut self,
        theme: &Theme,
        strings: &I18nStrings,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = &theme.colors;
        let query = self.workspace.tree_filter.clone();
        let focus = self
            .workspace
            .tree_filter_focus
            .get_or_insert_with(|| cx.focus_handle())
            .clone();
        let focus_for_click = focus.clone();
        let editor = cx.entity().downgrade();
        // 聚焦即给出高亮描边并清掉占位符：否则看起来像不可点击的静态文字
        // （用户报修）。
        let focused = focus.is_focused(window);
        div()
            .id("workspace-tree-filter")
            .relative()
            .track_focus(&focus)
            .w_full()
            .h(px(26.0))
            .px(px(8.0))
            .mr(px(6.0))
            .flex()
            .items_center()
            .rounded(px(5.0))
            .border_1()
            .border_color(if focused {
                c.dialog_primary_button_bg
            } else {
                c.dialog_border
            })
            .bg(c.editor_background)
            .text_size(px(11.0))
            .text_color(if query.is_empty() && !focused {
                c.dialog_muted
            } else {
                c.text_default
            })
            .cursor(CursorStyle::IBeam)
            .child(if query.is_empty() {
                if focused {
                    String::new()
                } else {
                    strings.tree_filter_placeholder.clone()
                }
            } else {
                query.clone()
            })
            .on_click({
                let focus = focus_for_click.clone();
                move |_event, window, _cx| window.focus(&focus)
            })
            .on_key_down({
                let editor = editor.clone();
                move |event: &KeyDownEvent, _window, cx| {
                    let _ = editor.update(cx, |editor, cx| {
                        editor.on_tree_filter_key_down(event, cx);
                    });
                }
            })
            .into_any_element()
    }

    /// 过滤框按键：字母/数字追加、退格删除、Esc 清空（roadmap D8）。
    pub(super) fn on_tree_filter_key_down(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let key = event.keystroke.key.clone();
        match key.as_str() {
            "escape" => {
                self.workspace.tree_filter.clear();
                self.workspace.tree_filter_focus = None;
            }
            "backspace" => {
                self.workspace.tree_filter.pop();
            }
            "shift" | "control" | "alt" | "meta" | "capslock" | "tab" | "enter" => return,
            _ => {
                if key.len() == 1 && key.chars().all(|ch| ch.is_ascii_graphic()) {
                    self.workspace.tree_filter.push_str(&key.to_lowercase());
                } else {
                    return;
                }
            }
        }
        cx.notify();
    }

    /// 树头部：排序按钮 + 过滤输入。
    fn render_tree_filter_and_sort_header(
        &mut self,
        theme: &Theme,
        strings: &I18nStrings,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = &theme.colors;
        let filter_row = self.render_tree_filter_row(theme, strings, window, cx);
        let order = match crate::config::EditorSettings::tree_sort(cx) {
            TreeSortPreference::Name => strings.tree_sort_name.clone(),
            TreeSortPreference::ModifiedTime => strings.tree_sort_mtime.clone(),
            TreeSortPreference::Type => strings.tree_sort_type.clone(),
        };
        div()
            .id("workspace-tree-header")
            .w_full()
            .px(px(4.0))
            .py(px(3.0))
            .flex()
            .items_center()
            .gap(px(6.0))
            .child(filter_row)
            .child(
                div()
                    .id("workspace-tree-sort")
                    .px(px(6.0))
                    .h(px(26.0))
                    .flex()
                    .items_center()
                    .rounded(px(5.0))
                    .text_size(px(11.0))
                    .text_color(c.dialog_muted)
                    .cursor_pointer()
                    .hover(|this| {
                        this.text_color(c.text_default)
                            .bg(c.dialog_secondary_button_hover)
                    })
                    .child(format!(
                        "{} · {} ↻",
                        strings.tree_sort_prefix, order
                    ))
                    .on_click(cx.listener(Self::on_cycle_tree_sort)),
            )
            .into_any_element()
    }

    /// VS Code-style search header: query input, collapsible replace input,
    /// option toggles, and a document/workspace scope switch. Rendered above
    /// the result list when the Search tab is active.
    fn render_search_header(
        &mut self,
        theme: &Theme,
        strings: &I18nStrings,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = &theme.colors;
        let editor = cx.entity().downgrade();

        let query_focused = self
            .workspace
            .search_focus
            .as_ref()
            .is_some_and(|focus| focus.is_focused(window));
        let search_input = self.render_search_input(
            "workspace-search-query",
            self.workspace.search_query.clone(),
            strings.workspace_search_placeholder.clone(),
            SearchInputKind::Query,
            query_focused,
            theme,
            cx,
        );
        let replace_visible = self.workspace.replace_visible;
        let replace_input = replace_visible.then(|| {
            let replace_focused = self
                .workspace
                .replace_focus
                .as_ref()
                .is_some_and(|focus| focus.is_focused(window));
            self.render_search_input(
                "workspace-search-replace",
                self.workspace.replace_query.clone(),
                strings.search_replace_placeholder.clone(),
                SearchInputKind::Replace,
                replace_focused,
                theme,
                cx,
            )
        });

        let toggle_editor = editor.clone();
        let toggle_button = div()
            .id("workspace-search-toggle-replace")
            .w(px(24.0))
            .h(px(24.0))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(5.0))
            .hover(|this| this.bg(c.dialog_secondary_button_hover))
            .cursor_pointer()
            .text_color(if replace_visible {
                c.dialog_primary_button_bg
            } else {
                c.dialog_muted
            })
            .child(
                svg()
                    .path(if replace_visible {
                        CHEVRON_DOWN_ICON
                    } else {
                        CHEVRON_RIGHT_ICON
                    })
                    .size(px(14.0))
                    .text_color(if replace_visible {
                        c.dialog_primary_button_bg
                    } else {
                        c.dialog_muted
                    }),
            )
            .tooltip(|_, cx| {
                cx.new(|_| WorkspaceTooltip {
                    label: "显示替换".into(),
                })
                .into()
            })
            .on_click(move |_, _, cx| {
                let _ = toggle_editor.update(cx, |editor, cx| {
                    editor.workspace.replace_visible = !editor.workspace.replace_visible;
                    cx.notify();
                });
            });

        let options_row = self.render_search_options_row(theme, strings, cx);

        let header = div()
            .id("workspace-search-header")
            .w_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .px(px(6.0))
            .pt(px(8.0))
            .pb(px(2.0))
            .border_b(px(1.0))
            .border_color(c.dialog_border)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .child(search_input)
                    .child(toggle_button),
            )
            .children(replace_input)
            .child(options_row);
        header.into_any_element()
    }

    /// Option toggles (case / whole word / regex / fuzzy), the scope switch,
    /// and — for the document scope — the replace action buttons.
    fn render_search_options_row(
        &mut self,
        theme: &Theme,
        strings: &I18nStrings,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = &theme.colors;
        let editor = cx.entity().downgrade();

        let toggle_chip = |editor: &WeakEntity<Self>,
                           id: &'static str,
                           label: String,
                           selected: bool,
                           tooltip: String| {
            let chip_editor = editor.clone();
            div()
                .id(id)
                .px(px(6.0))
                .h(px(22.0))
                .flex()
                .items_center()
                .rounded(px(4.0))
                .border_1()
                .border_color(if selected {
                    c.dialog_primary_button_bg
                } else {
                    c.dialog_border
                })
                .bg(if selected {
                    c.selection
                } else {
                    hsla(0.0, 0.0, 0.0, 0.0)
                })
                .text_size(px(11.0))
                .text_color(if selected {
                    c.dialog_primary_button_bg
                } else {
                    c.dialog_muted
                })
                .cursor_pointer()
                .hover(|this| this.bg(c.dialog_secondary_button_hover))
                .tooltip(move |_, cx| {
                    let tooltip = tooltip.clone();
                    cx.new(|_| WorkspaceTooltip { label: tooltip }).into()
                })
                .child(label)
                .on_click(move |_, _, cx| {
                    let _ = chip_editor.update(cx, |editor, cx| {
                        match id {
                            "workspace-search-case" => {
                                editor.workspace.search_match_case =
                                    !editor.workspace.search_match_case;
                            }
                            "workspace-search-word" => {
                                editor.workspace.search_whole_word =
                                    !editor.workspace.search_whole_word;
                            }
                            "workspace-search-regex" => {
                                editor.workspace.search_use_regex =
                                    !editor.workspace.search_use_regex;
                            }
                            "workspace-search-fuzzy" => {
                                editor.workspace.search_fuzzy = !editor.workspace.search_fuzzy;
                            }
                            _ => {}
                        }
                        editor.schedule_workspace_search(cx);
                        cx.notify();
                    });
                })
        };

        let mut options = div()
            .id("workspace-search-options")
            .w_full()
            .flex()
            .items_center()
            .flex_wrap()
            .gap(px(4.0))
            .child(toggle_chip(
                &editor,
                "workspace-search-case",
                "Aa".to_string(),
                self.workspace.search_match_case,
                strings.search_case_sensitive.clone(),
            ))
            .child(toggle_chip(
                &editor,
                "workspace-search-word",
                "ab".to_string(),
                self.workspace.search_whole_word,
                strings.search_whole_word.clone(),
            ))
            .child(toggle_chip(
                &editor,
                "workspace-search-regex",
                ".*".to_string(),
                self.workspace.search_use_regex,
                strings.search_regex.clone(),
            ))
            .child(toggle_chip(
                &editor,
                "workspace-search-fuzzy",
                strings.search_fuzzy_short.clone(),
                self.workspace.search_fuzzy,
                strings.search_fuzzy.clone(),
            ));

        // Scope switch: current document vs. whole workspace (the latter needs
        // a scanned tree).
        let scope = self.workspace.search_scope;
        let scope_button =
            |editor: &WeakEntity<Self>, id: &'static str, label: String, selected: bool| {
                let scope_editor = editor.clone();
                div()
                    .id(id)
                    .px(px(6.0))
                    .h(px(22.0))
                    .flex()
                    .items_center()
                    .rounded(px(4.0))
                    .bg(if selected {
                        c.selection
                    } else {
                        hsla(0.0, 0.0, 0.0, 0.0)
                    })
                    .text_size(px(11.0))
                    .text_color(if selected {
                        c.dialog_primary_button_bg
                    } else {
                        c.dialog_muted
                    })
                    .cursor_pointer()
                    .hover(|this| this.bg(c.dialog_secondary_button_hover))
                    .child(label)
                    .on_click(move |_, _, cx| {
                        let _ = scope_editor.update(cx, |editor, cx| {
                            editor.workspace.search_scope = match id {
                                "workspace-scope-document" => WorkspaceSearchScope::Document,
                                _ => WorkspaceSearchScope::Workspace,
                            };
                            editor.workspace.search_active_index = None;
                            editor.workspace.document_active_range = None;
                            editor.schedule_workspace_search(cx);
                            cx.notify();
                        });
                    })
            };
        let match_count = self.workspace.search_results.len();
        let count_label = if match_count > 0 && scope == WorkspaceSearchScope::Document {
            Some(
                strings
                    .search_result_count
                    .replace("{n}", &match_count.to_string()),
            )
        } else {
            None
        };

        options = options.child(
            div()
                .id("workspace-search-scope-row")
                .w_full()
                .flex()
                .items_center()
                .gap(px(4.0))
                .child(scope_button(
                    &editor,
                    "workspace-scope-document",
                    strings.search_scope_document.clone(),
                    scope == WorkspaceSearchScope::Document,
                ))
                .child(scope_button(
                    &editor,
                    "workspace-scope-workspace",
                    strings.search_scope_workspace.clone(),
                    scope == WorkspaceSearchScope::Workspace,
                ))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .flex()
                        .justify_end()
                        .children(count_label.map(|label| {
                            div()
                                .text_size(px(11.0))
                                .text_color(c.dialog_muted)
                                .child(label)
                        })),
                ),
        );

        // Replace actions (document scope only for now; workspace replace
        // lives behind the same buttons when the workspace scope is active).
        if self.workspace.replace_visible && !self.workspace.replace_query.is_empty() {
            let replace_editor = editor.clone();
            let replace_all_editor = editor.clone();
            let is_document_scope = scope == WorkspaceSearchScope::Document;
            let replace_row = div()
                .w_full()
                .flex()
                .items_center()
                .justify_end()
                .gap(px(6.0))
                .child(
                    div()
                        .id("workspace-search-replace-current")
                        .px(px(8.0))
                        .h(px(24.0))
                        .flex()
                        .items_center()
                        .rounded(px(5.0))
                        .border_1()
                        .border_color(c.dialog_border)
                        .text_size(px(11.0))
                        .text_color(c.text_default)
                        .cursor_pointer()
                        .hover(|this| this.bg(c.dialog_secondary_button_hover))
                        .child(strings.search_replace_current.clone())
                        .on_click(move |_, window, cx| {
                            let _ = replace_editor.update(cx, |editor, cx| {
                                if is_document_scope {
                                    editor.replace_active_document_match(window, cx);
                                }
                                cx.notify();
                            });
                            cx.stop_propagation();
                        }),
                )
                .child(
                    div()
                        .id("workspace-search-replace-all")
                        .px(px(8.0))
                        .h(px(24.0))
                        .flex()
                        .items_center()
                        .rounded(px(5.0))
                        .border_1()
                        .border_color(c.dialog_border)
                        .text_size(px(11.0))
                        .text_color(c.text_default)
                        .cursor_pointer()
                        .hover(|this| this.bg(c.dialog_secondary_button_hover))
                        .child(strings.search_replace_all.clone())
                        .on_click(move |_, window, cx| {
                            let _ = replace_all_editor.update(cx, |editor, cx| {
                                if is_document_scope {
                                    editor.replace_all_document_matches(window, cx);
                                } else {
                                    let replaced =
                                        editor.replace_all_workspace_matches(window, cx);
                                    if replaced > 0 {
                                        editor.schedule_workspace_search(cx);
                                    }
                                }
                                cx.notify();
                            });
                            cx.stop_propagation();
                        }),
                );
            options = options.child(replace_row);
        }

        options.into_any_element()
    }

    /// Shared single-line input used by the query and replace fields. Clicks
    /// focus the field; key handling and IME route through `SearchInputKind`.
    fn render_search_input(
        &mut self,
        id: &'static str,
        value: String,
        placeholder: String,
        kind: SearchInputKind,
        focused: bool,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = &theme.colors;
        let focus = match kind {
            SearchInputKind::Query => self
                .workspace
                .search_focus
                .get_or_insert_with(|| cx.focus_handle())
                .clone(),
            SearchInputKind::Replace => self
                .workspace
                .replace_focus
                .get_or_insert_with(|| cx.focus_handle())
                .clone(),
        };
        let focus_for_click = focus.clone();
        let focus_for_input = focus.clone();
        let input_editor = cx.entity();
        let editor = cx.entity().downgrade();

        // The placeholder disappears as soon as the field is focused, not
        // just once text is typed.
        let (label, muted) = if !value.is_empty() {
            (value, false)
        } else if focused {
            (String::new(), true)
        } else {
            (placeholder, true)
        };

        div()
            .id(id)
            .relative()
            .track_focus(&focus)
            .flex_1()
            .min_w(px(0.0))
            .h(px(28.0))
            .px(px(8.0))
            .flex()
            .items_center()
            .rounded(px(6.0))
            .border_1()
            .border_color(if focused {
                c.dialog_primary_button_bg
            } else {
                c.dialog_border
            })
            .bg(c.editor_background)
            .text_size(px(12.0))
            .text_color(if muted {
                c.dialog_muted
            } else {
                c.text_default
            })
            .child(label)
            .child(
                canvas(
                    |_, _, _| (),
                    move |bounds, _, window, cx| {
                        window.handle_input(
                            &focus_for_input,
                            ElementInputHandler::new(bounds, input_editor.clone()),
                            cx,
                        );
                    },
                )
                .absolute()
                .top_0()
                .right_0()
                .bottom_0()
                .left_0(),
            )
            .on_click(move |_event, window, _cx| window.focus(&focus_for_click))
            .on_key_down(move |event: &KeyDownEvent, window, cx| {
                let key = event.keystroke.key.to_ascii_lowercase();
                let secondary = event.keystroke.modifiers.secondary();
                match key.as_str() {
                    "escape" => {
                        let _ = editor.update(cx, |editor, cx| match kind {
                            SearchInputKind::Query => {
                                editor.workspace.search_query.clear();
                                editor.workspace.search_selected_range = 0..0;
                                editor.workspace.search_marked_range = None;
                                editor.workspace.active_tab = WorkspaceTab::Files;
                                editor.workspace.search_focus_pending = false;
                                editor.schedule_workspace_search(cx);
                            }
                            SearchInputKind::Replace => {
                                editor.workspace.replace_visible = false;
                            }
                        });
                    }
                    "a" if secondary => {
                        let _ = editor.update(cx, |editor, cx| {
                            match kind {
                                SearchInputKind::Query => {
                                    editor.workspace.search_selected_range =
                                        0..editor.workspace.search_query.len();
                                }
                                SearchInputKind::Replace => {
                                    editor.workspace.replace_selected_range =
                                        0..editor.workspace.replace_query.len();
                                }
                            }
                            cx.notify();
                        });
                    }
                    "backspace" => {
                        let handled = editor.update(cx, |editor, cx| {
                            let (text, selected, marked) = match kind {
                                SearchInputKind::Query => (
                                    editor.workspace.search_query.clone(),
                                    editor.workspace.search_selected_range.clone(),
                                    editor.workspace.search_marked_range.clone(),
                                ),
                                SearchInputKind::Replace => (
                                    editor.workspace.replace_query.clone(),
                                    editor.workspace.replace_selected_range.clone(),
                                    editor.workspace.replace_marked_range.clone(),
                                ),
                            };
                            if marked.is_some() {
                                return false;
                            }
                            let range = if selected.start == selected.end {
                                let before = &text[..selected.start];
                                let start = before
                                    .grapheme_indices(true)
                                    .last()
                                    .map(|(start, _)| start)
                                    .unwrap_or(selected.start);
                                start..selected.start
                            } else {
                                selected
                            };
                            editor.replace_overlay_input_text(kind, range, "", None, false, cx);
                            true
                        });
                        if !matches!(handled, Ok(true)) {
                            return;
                        }
                    }
                    "v" if secondary => {
                        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                            let _ = editor.update(cx, |editor, cx| {
                                let selected = match kind {
                                    SearchInputKind::Query => {
                                        editor.workspace.search_selected_range.clone()
                                    }
                                    SearchInputKind::Replace => {
                                        editor.workspace.replace_selected_range.clone()
                                    }
                                };
                                editor.replace_overlay_input_text(
                                    kind, selected, &text, None, false, cx,
                                );
                            });
                        }
                    }
                    "enter" => {
                        let reverse = event.keystroke.modifiers.shift;
                        let _ = editor.update(cx, |editor, cx| match kind {
                            SearchInputKind::Query => {
                                editor.find_next_document_match(reverse, cx);
                            }
                            SearchInputKind::Replace => {
                                editor.replace_active_document_match(window, cx);
                            }
                        });
                    }
                    _ => return,
                }
                cx.stop_propagation();
            })
            .into_any_element()
    }

    pub(super) fn render_activity_rail(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let c = &theme.colors;
        let editor = cx.entity().downgrade();
        let button = |id: &'static str,
                      icon_path: &'static str,
                      label: &'static str,
                      selected: bool,
                      tab: WorkspaceTab,
                      editor: WeakEntity<Self>| {
            div()
                .id(id)
                .debug_selector(move || id.to_string())
                .w(px(34.0))
                .h(px(34.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(8.0))
                .bg(if selected {
                    c.selection
                } else {
                    c.dialog_secondary_button_bg
                })
                .hover(|this| this.bg(c.dialog_secondary_button_hover))
                .cursor_pointer()
                .child(
                    svg()
                        .path(icon_path)
                        .size(px(18.0))
                        .text_color(if selected {
                            c.dialog_primary_button_bg
                        } else {
                            c.dialog_muted
                        }),
                )
                .tooltip(move |_, cx| {
                    cx.new(|_| WorkspaceTooltip {
                        label: label.into(),
                    })
                    .into()
                })
                .on_click(move |_, _, cx| {
                    let _ = editor.update(cx, |editor, cx| {
                        // 三个按钮一致：已经开在这一页时再点一次就收起侧边栏
                        // （之前只有文件和搜索会收，大纲那个参数写的是 false）。
                        // 手动切换后不要留下「贴边滑出」的状态：收起时它会让浮层
                        // 立刻又冒出来，展开时也不需要它；收回动画同理一并清掉。
                        editor.sidebar_peek = false;
                        editor.sidebar_overlay_closing = false;
                        if editor.workspace.is_open && editor.workspace.active_tab == tab {
                            editor.workspace.is_open = false;
                            cx.notify();
                            return;
                        }
                        editor.workspace.is_open = true;
                        editor.set_workspace_tab(tab, cx);
                        cx.notify();
                    });
                })
        };
        div()
            .id("workspace-activity-rail")
            .w(px(50.0))
            .h_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(10.0))
            .pt(px(12.0))
            .bg(c.dialog_secondary_button_bg)
            .border_r(px(1.0))
            .border_color(c.dialog_border)
            .child(button(
                "activity-files",
                ACTIVITY_FILES_ICON,
                "文件",
                self.workspace.is_open && self.workspace.active_tab == WorkspaceTab::Files,
                WorkspaceTab::Files,
                editor.clone(),
            ))
            .child(button(
                "activity-search",
                ACTIVITY_SEARCH_ICON,
                "搜索",
                self.workspace.is_open && self.workspace.active_tab == WorkspaceTab::Search,
                WorkspaceTab::Search,
                editor.clone(),
            ))
            .child(button(
                "activity-outline",
                ACTIVITY_OUTLINE_ICON,
                "大纲",
                self.workspace.is_open && self.workspace.active_tab == WorkspaceTab::Outline,
                WorkspaceTab::Outline,
                editor,
            ))
            .into_any_element()
    }

    fn render_workspace_files_tree(
        &self,
        theme: &Theme,
        strings: &I18nStrings,
        editor: &WeakEntity<Editor>,
    ) -> AnyElement {
        if self.workspace.root.is_none() {
            return self.render_workspace_empty_state(
                &strings.workspace_no_file_title,
                &strings.workspace_no_file_message,
                theme,
            );
        }

        // 过滤激活：只显示文件名匹配的文件（roadmap D8）。
        let filter = self.workspace.tree_filter.trim().to_lowercase();
        if !filter.is_empty() {
            let mut rows: Vec<AnyElement> = Vec::new();
            let all_files = self.workspace_text_files();
            let editor = editor.clone();
            let mut shown = 0usize;
            for path in all_files {
                let name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                if !name.contains(&filter) {
                    continue;
                }
                shown += 1;
                if shown > 50 {
                    break;
                }
                let label = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let directory = path
                    .parent()
                    .map(|parent| parent.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let row_editor = editor.clone();
                let row_path = path.clone();
                rows.push(
                    div()
                        .id(("tree-filter-hit", shown))
                        .w_full()
                        .px(px(10.0))
                        .h(px(28.0))
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .rounded(px(5.0))
                        .cursor_pointer()
                        .hover(|this| this.bg(theme.colors.dialog_secondary_button_hover))
                        .child(
                            div()
                                .max_w(px(180.0))
                                .min_w(px(0.0))
                                .truncate()
                                .text_size(px(12.0))
                                .text_color(theme.colors.text_default)
                                .child(label),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.0))
                                .truncate()
                                .text_size(px(10.5))
                                .text_color(theme.colors.dialog_muted)
                                .child(directory),
                        )
                        .on_click(move |event, window, cx| {
                            let _ = row_editor.update(cx, |editor, cx| {
                                let mode = Self::tree_click_open_mode(&event);
                                editor.open_workspace_file_in_mode(
                                    row_path.clone(),
                                    mode,
                                    window,
                                    cx,
                                );
                            });
                            let _ = event;
                        })
                        .into_any_element(),
                );
            }
            if rows.is_empty() {
                return self.render_workspace_empty_state(
                    "",
                    &strings.workspace_no_search_results,
                    theme,
                );
            }
            return div()
                .w_full()
                .flex()
                .flex_col()
                .gap(px(1.0))
                .children(rows)
                .into_any_element();
        }

        if let Some(error) = self.workspace.file_error.as_ref() {
            return self.render_workspace_empty_state(
                &strings.workspace_scan_failed_title,
                error,
                theme,
            );
        }

        let Some(root) = self.workspace.file_tree.as_ref() else {
            // 后台扫描进行中（roadmap D9）：与搜索面板一致显示处理中占位，
            // 而不是误报「空文件夹」。
            if self.workspace.tree_scan_root.is_some()
                && self.workspace.tree_scan_root.as_ref() == self.workspace.root.as_ref()
            {
                return div()
                    .p(px(12.0))
                    .text_size(px(14.0))
                    .text_color(theme.colors.dialog_muted)
                    .child("…")
                    .into_any_element();
            }
            return self.render_workspace_empty_state("", &strings.workspace_empty_files, theme);
        };

        div()
            .w_full()
            .flex()
            .flex_col()
            .children(self.render_workspace_nodes(std::slice::from_ref(root), 0, theme, editor))
            .into_any_element()
    }

    fn render_search_results(
        &self,
        theme: &Theme,
        strings: &I18nStrings,
        editor: &WeakEntity<Editor>,
    ) -> AnyElement {
        // 只在还没有任何结果可显示时才用「…」占位：重新搜索期间继续显示上一次
        // 的结果，避免侧栏闪空（用户报修）。
        if self.workspace.search_pending && self.workspace.search_results.is_empty() {
            return div()
                .p(px(12.0))
                .text_size(px(14.0))
                .text_color(theme.colors.dialog_muted)
                .child("…")
                .into_any_element();
        }
        if self.workspace.search_results.is_empty() {
            return self.render_workspace_empty_state(
                "",
                if self.workspace.search_scope == WorkspaceSearchScope::Document {
                    &strings.workspace_no_document_find_results
                } else {
                    &strings.workspace_no_search_results
                },
                theme,
            );
        }
        let c = &theme.colors;
        let is_document_scope = self.workspace.search_scope == WorkspaceSearchScope::Document;
        let mut elements: Vec<AnyElement> = Vec::new();
        let mut current_file: Option<PathBuf> = None;
        for (index, hit) in self.workspace.search_results.iter().enumerate() {
            // Workspace scope groups hits under a file header row; document
            // scope lists matches flat with the file name on each row.
            if !is_document_scope && current_file.as_ref() != Some(&hit.path) {
                current_file = Some(hit.path.clone());
                let file_hit_count = self
                    .workspace
                    .search_results
                    .iter()
                    .filter(|other| other.path == hit.path)
                    .count();
                // 文件头整行可点击并代表该组第一条命中（用户报修：此前文件名
                // 本身点不了，只有它下面一条没有内容的空行能点）。文件名命中
                // 没有行号，点击就只是打开这个文件；内容命中则跳到该处匹配。
                let header_selected = self.workspace.search_active_index == Some(index);
                let header_editor = editor.clone();
                elements.push(
                    div()
                        .id(("workspace-search-file", index))
                        .debug_selector(move || format!("workspace-search-file-{index}"))
                        .w_full()
                        .px(px(6.0))
                        .pt(px(6.0))
                        .pb(px(2.0))
                        .flex()
                        .items_center()
                        .gap(px(4.0))
                        .rounded(px(5.0))
                        .bg(if header_selected {
                            c.selection
                        } else {
                            hsla(0.0, 0.0, 0.0, 0.0)
                        })
                        .cursor_pointer()
                        .hover(|this| this.bg(c.dialog_secondary_button_hover))
                        .child(
                            svg()
                                .path(if is_code_file(&hit.path) {
                                    CODE_ICON
                                } else {
                                    MARKDOWN_ICON
                                })
                                .size(px(13.0))
                                .text_color(c.dialog_primary_button_bg),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.0))
                                .truncate()
                                .text_size(px(11.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(c.text_default)
                                .child(hit.label.clone()),
                        )
                        .child(
                            div()
                                .text_size(px(10.0))
                                .text_color(c.dialog_muted)
                                .child(file_hit_count.to_string()),
                        )
                        .on_click(move |event, window, cx| {
                            if !event.standard_click() {
                                return;
                            }
                            let _ = header_editor.update(cx, |editor, cx| {
                                editor.open_search_hit(index, window, cx);
                            });
                        })
                        .into_any_element(),
                );
            }
            // 文件名命中只由文件头代表：再渲染一行没有行号、没有预览的行
            // 只会得到一条看得见点不着（或看不见）的空条。
            if !is_document_scope && hit.line.is_none() {
                continue;
            }
            let selected = self.workspace.search_active_index == Some(index);
            let hit_editor = editor.clone();
            let show_file_label = is_document_scope;
            let label = hit.label.clone();
            let line_number = hit.line;
            let preview = hit.preview.clone();
            elements.push(
                div()
                    .id(("workspace-search-hit", index))
                    .debug_selector(move || format!("workspace-search-hit-{index}"))
                    .w_full()
                    .pl(px(if is_document_scope { 10.0 } else { 24.0 }))
                    .pr(px(6.0))
                    .py(px(4.0))
                    .flex()
                    .flex_col()
                    .gap(px(1.0))
                    .rounded(px(5.0))
                    .bg(if selected {
                        c.selection
                    } else {
                        hsla(0.0, 0.0, 0.0, 0.0)
                    })
                    .cursor_pointer()
                    .hover(|this| this.bg(c.dialog_secondary_button_hover))
                    .children(show_file_label.then(|| {
                        div()
                            .truncate()
                            .text_size(px(11.0))
                            .text_color(c.text_default)
                            .child(label)
                    }))
                    .children(line_number.map(|line| {
                        div()
                            .flex()
                            .gap(px(6.0))
                            .min_w(px(0.0))
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .text_color(c.dialog_muted)
                                    .child(format!("{line}")),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.0))
                                    .truncate()
                                    .text_size(px(11.0))
                                    .text_color(c.text_default)
                                    .child(preview),
                            )
                    }))
                    .on_click(move |event, window, cx| {
                        if !event.standard_click() {
                            return;
                        }
                        let _ = hit_editor.update(cx, |editor, cx| {
                            editor.open_search_hit(index, window, cx);
                        });
                    })
                    .into_any_element(),
            );
        }
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(1.0))
            .children(elements)
            .into_any_element()
    }

    /// Opens the hit's match: document-scope hits select the byte range in the
    /// live document; workspace-scope hits open the file first, then select
    /// the match using its line/column information.
    fn open_search_hit(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(hit) = self.workspace.search_results.get(index) else {
            return;
        };
        self.workspace.search_active_index = Some(index);
        if let Some(range) = hit.source_range.clone() {
            self.workspace.document_active_range = Some(range.clone());
            self.jump_to_document_search_range(range, cx);
            return;
        }
        let path = hit.path.clone();
        let line = hit.line;
        let match_range = hit.match_range.clone();
        self.open_workspace_file(path.clone(), window, cx);
        if self.file_path.as_ref() == Some(&path)
            && let (Some(line), Some(match_range)) = (line, match_range)
        {
            let source = self.current_document_source(cx);
            let line_start = source
                .split_inclusive('\n')
                .take(line.saturating_sub(1))
                .map(str::len)
                .sum::<usize>()
                .min(source.len());
            let start = (line_start + match_range.start).min(source.len());
            let end = (line_start + match_range.end).min(source.len());
            if source.is_char_boundary(start) && source.is_char_boundary(end) {
                self.jump_to_document_search_range(start..end, cx);
            }
        }
    }

    fn render_workspace_outline_tree(
        &self,
        theme: &Theme,
        strings: &I18nStrings,
        editor: &WeakEntity<Editor>,
    ) -> AnyElement {
        if self.workspace.outline_tree.is_empty() {
            return self.render_workspace_empty_state("", &strings.workspace_empty_outline, theme);
        }

        div()
            .w_full()
            .flex()
            .flex_col()
            .children(self.render_workspace_nodes(&self.workspace.outline_tree, 0, theme, editor))
            .into_any_element()
    }

    fn render_workspace_empty_state(
        &self,
        title: &str,
        message: &str,
        theme: &Theme,
    ) -> AnyElement {
        let c = &theme.colors;
        let t = &theme.typography;
        let title = (!title.is_empty()).then(|| {
            div()
                .text_size(px(t.text_size))
                .font_weight(FontWeight::MEDIUM)
                .text_color(c.text_default)
                .child(title.to_string())
        });

        div()
            .w_full()
            .h_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(8.0))
            .px(px(22.0))
            .text_align(TextAlign::Center)
            .children(title)
            .child(
                div()
                    .text_size(px(t.text_size * 0.9))
                    .line_height(px(t.text_size * t.text_line_height))
                    .text_color(c.dialog_muted)
                    .child(message.to_string()),
            )
            .into_any_element()
    }

    fn render_workspace_nodes(
        &self,
        nodes: &[WorkspaceTreeNode],
        depth: usize,
        theme: &Theme,
        editor: &WeakEntity<Editor>,
    ) -> Vec<AnyElement> {
        let mut elements = Vec::new();
        for node in nodes {
            elements.push(self.render_workspace_node(node, depth, theme, editor));
            if !node.children.is_empty() && self.workspace.expanded.contains(&node.id) {
                elements.extend(self.render_workspace_nodes(
                    &node.children,
                    depth + 1,
                    theme,
                    editor,
                ));
            }
        }
        elements
    }

    fn render_workspace_node(
        &self,
        node: &WorkspaceTreeNode,
        depth: usize,
        theme: &Theme,
        editor: &WeakEntity<Editor>,
    ) -> AnyElement {
        let c = &theme.colors;
        let is_expanded = self.workspace.expanded.contains(&node.id);
        let has_children = !node.children.is_empty();
        let selected = match (&self.workspace.selected, &node.kind) {
            (Some(WorkspaceSelection::Directory(selected)), WorkspaceTreeKind::Directory(path)) => {
                selected == path
            }
            (Some(WorkspaceSelection::File(selected)), WorkspaceTreeKind::MarkdownFile(path)) => {
                selected == path
            }
            (Some(WorkspaceSelection::File(selected)), WorkspaceTreeKind::CodeFile(path)) => {
                selected == path
            }
            (Some(WorkspaceSelection::Outline(selected)), _) => selected == &node.id,
            _ => false,
        };
        let node_id = node.id.clone();
        let click_editor = editor.clone();
        let click_kind = node.kind.clone();
        let context_editor = editor.clone();
        let context_kind = node.kind.clone();
        let arrow_node_id = node.id.clone();
        let arrow_editor = editor.clone();
        let arrow_icon = has_children.then_some(if is_expanded {
            CHEVRON_DOWN_ICON
        } else {
            CHEVRON_RIGHT_ICON
        });

        let icon = match &node.kind {
            WorkspaceTreeKind::Directory(_) => Some((FOLDER_ICON, Hsla::from(rgba(0x4a93d8ff)))),
            WorkspaceTreeKind::MarkdownFile(_) => Some((MARKDOWN_ICON, c.dialog_primary_button_bg)),
            WorkspaceTreeKind::CodeFile(_) => Some((CODE_ICON, c.dialog_muted)),
            WorkspaceTreeKind::OtherFile(_) => Some((GENERIC_FILE_ICON, c.dialog_muted)),
            WorkspaceTreeKind::Heading { .. } => None,
        };

        let label_color = if selected {
            c.text_default
        } else {
            c.dialog_muted
        };

        let mut arrow_el = div()
            .w(px(16.0))
            .h(px(20.0))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .children(
                arrow_icon.map(|path| svg().path(path).size(px(14.0)).text_color(c.dialog_muted)),
            );
        if has_children {
            arrow_el = arrow_el.cursor_pointer().on_mouse_down(
                MouseButton::Left,
                move |_event, _window, cx| {
                    let _ = arrow_editor.update(cx, |editor, cx| {
                        editor.toggle_workspace_node(&arrow_node_id, cx);
                    });
                    cx.stop_propagation();
                },
            );
        }

        // 文件节点悬停显示 大小 · 修改时间（roadmap D7）。
        let tooltip_text = tree_node_tooltip(&node);
        div()
            .id(("workspace-node", stable_node_hash(&node.id)))
            .h(px(WORKSPACE_NODE_HEIGHT))
            .w_full()
            .overflow_hidden()
            .flex()
            .items_center()
            .gap(px(4.0))
            .pl(px(6.0 + depth as f32 * WORKSPACE_NODE_INDENT))
            .pr(px(6.0))
            .rounded(px(4.0))
            .tooltip(move |_, cx| {
                let tooltip_text = tooltip_text.clone();
                cx.new(|_| WorkspaceTooltip { label: tooltip_text }).into()
            })
            .bg(if selected {
                c.selection
            } else {
                hsla(0.0, 0.0, 0.0, 0.0)
            })
            .hover(|this| this.bg(c.dialog_secondary_button_hover))
            .cursor_pointer()
            .child(arrow_el)
            .children(icon.map(|(path, color)| {
                svg()
                    .path(path)
                    .size(px(16.0))
                    .flex_shrink_0()
                    .text_color(color)
                    .into_any_element()
            }))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .overflow_hidden()
                    .truncate()
                    .text_size(px(12.0))
                    .line_height(px(18.0))
                    .text_color(label_color)
                    .child(node.label.clone()),
            )
            .on_mouse_down(MouseButton::Right, move |event, _, cx| {
                let selection = match &context_kind {
                    WorkspaceTreeKind::Directory(path) => {
                        Some(WorkspaceSelection::Directory(path.clone()))
                    }
                    WorkspaceTreeKind::MarkdownFile(path)
                    | WorkspaceTreeKind::CodeFile(path)
                    | WorkspaceTreeKind::OtherFile(path) => {
                        Some(WorkspaceSelection::File(path.clone()))
                    }
                    WorkspaceTreeKind::Heading { .. } => None,
                };
                let _ = context_editor.update(cx, |editor, cx| {
                    editor.open_workspace_context_menu(event.position, selection, cx);
                });
                cx.stop_propagation();
            })
            .on_click(move |event, window, cx| {
                if !event.standard_click() {
                    return;
                }
                let node_id = node_id.clone();
                let click_kind = click_kind.clone();
                let _ = click_editor.update(cx, |editor, cx| match click_kind {
                    WorkspaceTreeKind::Directory(path) => {
                        editor.workspace.selected = Some(WorkspaceSelection::Directory(path));
                        editor.toggle_workspace_node(&node_id, cx);
                    }
                    WorkspaceTreeKind::MarkdownFile(path) => {
                        let mode = Self::tree_click_open_mode(&event);
                        editor.open_workspace_file_in_mode(path, mode, window, cx);
                    }
                    WorkspaceTreeKind::CodeFile(path) => {
                        let mode = Self::tree_click_open_mode(&event);
                        editor.open_workspace_file_in_mode(path, mode, window, cx);
                    }
                    WorkspaceTreeKind::OtherFile(path) => {
                        let mode = Self::tree_click_open_mode(&event);
                        editor.open_workspace_file_in_mode(path, mode, window, cx);
                    }
                    WorkspaceTreeKind::Heading { line, .. } => {
                        if event.click_count() >= 2 {
                            editor.rename_outline_heading(line, cx);
                        } else {
                            editor.open_outline_node(node_id, line, cx);
                        }
                    }
                });
            })
            .into_any_element()
    }
}

/// 永久删除（roadmap H2 的「永久删除」策略）。
pub(crate) fn permanent_delete(target: &Path, is_directory: bool) -> std::io::Result<()> {
    if is_directory {
        std::fs::remove_dir_all(target)
    } else {
        std::fs::remove_file(target)
    }
}

/// Moves a workspace item to the system trash so accidental deletions are
/// recoverable (roadmap D4). Falls back to hard delete where trash semantics
/// are unavailable.
fn move_to_trash(target: &Path, is_directory: bool) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var("HOME").unwrap_or_default();
        if home.is_empty() {
            return fallback_delete(target, is_directory);
        }
        let trash_dir = PathBuf::from(home).join(".Trash");
        std::fs::create_dir_all(&trash_dir)?;
        let name = target
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "deleted".into());
        let stem = target
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| name.clone());
        let extension = target
            .extension()
            .map(|extension| format!(".{}", extension.to_string_lossy()))
            .unwrap_or_default();
        let mut destination = trash_dir.join(&name);
        let mut counter = 1u32;
        while destination.exists() {
            destination = trash_dir.join(format!("{stem} {counter}{extension}"));
            counter += 1;
        }
        std::fs::rename(target, destination)
    }
    #[cfg(not(target_os = "macos"))]
    {
        fallback_delete(target, is_directory)
    }
}

fn fallback_delete(target: &Path, is_directory: bool) -> std::io::Result<()> {
    if is_directory {
        std::fs::remove_dir_all(target)
    } else {
        std::fs::remove_file(target)
    }
}

pub(super) fn is_markdown_file(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.to_string_lossy().eq_ignore_ascii_case("md"))
}

/// 是否按 Markdown 文档打开：仅 `.md` / `.markdown`。所有打开入口（工作区树、
/// 拖拽、命令行）必须共用这一条判定，否则同一文件两条入口行为不一致。
pub(crate) fn is_markdown_document(path: &Path) -> bool {
    path.extension().is_some_and(|extension| {
        let extension = extension.to_string_lossy();
        extension.eq_ignore_ascii_case("md") || extension.eq_ignore_ascii_case("markdown")
    })
}

fn create_workspace_file(path: &Path) -> std::io::Result<()> {
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(path)?;
    let template = crate::config::EditorSettings::new_file_template();
    if !template.is_empty() {
        let date = crate::config::today_local_date();
        use std::io::Write;
        file.write_all(template.replace("{date}", &date).as_bytes())?;
    }
    Ok(())
}

fn create_workspace_folder(path: &Path) -> std::io::Result<()> {
    std::fs::create_dir(path)
}

fn remap_moved_path(
    path: &Path,
    source: &Path,
    destination: &Path,
    source_is_directory: bool,
) -> Option<PathBuf> {
    let suffix = if source_is_directory {
        path.strip_prefix(source).ok()?
    } else if path == source {
        Path::new("")
    } else {
        return None;
    };
    Some(destination.join(suffix))
}

fn inline_image_destination_range(source: &str, range: Range<usize>) -> Option<Range<usize>> {
    let image = source.get(range.clone())?;
    let bytes = image.as_bytes();
    if !image.starts_with("![") {
        return None;
    }

    let mut cursor = 2;
    let mut bracket_depth = 1;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'\\' => cursor = cursor.checked_add(2)?,
            b'[' => {
                bracket_depth += 1;
                cursor += 1;
            }
            b']' => {
                bracket_depth -= 1;
                cursor += 1;
                if bracket_depth == 0 {
                    break;
                }
            }
            _ => cursor += 1,
        }
    }
    if bracket_depth != 0 {
        return None;
    }
    while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
        cursor += 1;
    }
    if bytes.get(cursor) != Some(&b'(') {
        return None;
    }
    cursor += 1;
    while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
        cursor += 1;
    }

    let (start, end) = if bytes.get(cursor) == Some(&b'<') {
        let start = cursor + 1;
        cursor = start;
        while cursor < bytes.len() {
            if bytes[cursor] == b'\\' {
                cursor = cursor.checked_add(2)?;
            } else if bytes[cursor] == b'>' {
                break;
            } else {
                cursor += 1;
            }
        }
        (start, cursor)
    } else {
        let start = cursor;
        let mut parentheses = 0usize;
        while cursor < bytes.len() {
            match bytes[cursor] {
                b'\\' => cursor = cursor.checked_add(2)?,
                b'(' => {
                    parentheses += 1;
                    cursor += 1;
                }
                b')' if parentheses == 0 => break,
                b')' => {
                    parentheses -= 1;
                    cursor += 1;
                }
                byte if byte.is_ascii_whitespace() && parentheses == 0 => break,
                _ => cursor += 1,
            }
        }
        (start, cursor)
    };
    (start < end).then_some((range.start + start)..(range.start + end))
}

fn rewrite_relative_image_destination(
    destination: &str,
    source_directory: &Path,
    destination_directory: &Path,
) -> Option<String> {
    if destination.starts_with("//")
        || destination.starts_with('/')
        || destination.starts_with('#')
        || url::Url::parse(destination).is_ok()
    {
        return None;
    }

    let suffix_start = destination
        .char_indices()
        .find(|(_, character)| matches!(character, '?' | '#'))
        .map_or(destination.len(), |(index, _)| index);
    let (relative_target, suffix) = destination.split_at(suffix_start);
    if relative_target.is_empty() {
        return None;
    }
    let target_path = Path::new(relative_target);
    if target_path.is_absolute() {
        return None;
    }
    let resolved_target = normalize_path(&source_directory.join(target_path));
    let relative_path = relative_path_between(destination_directory, &resolved_target)?;
    let mut relative = relative_path.to_string_lossy().replace('\\', "/");
    if !relative.starts_with("./") && !relative.starts_with("../") {
        relative = format!("./{relative}");
    }
    relative = relative
        .replace('%', "%25")
        .replace(' ', "%20")
        .replace('(', "%28")
        .replace(')', "%29")
        .replace('"', "%22");
    Some(format!("{relative}{suffix}"))
}

fn rewrite_relative_image_targets(
    markdown: &str,
    source_directory: &Path,
    destination_directory: &Path,
) -> String {
    let mut replacements = Vec::new();
    let mut reference_destinations = HashMap::new();
    for (event, range) in Parser::new_ext(markdown, Options::all()).into_offset_iter() {
        let Event::Start(Tag::Image {
            link_type,
            dest_url,
            id,
            ..
        }) = event
        else {
            continue;
        };
        let Some(destination) =
            rewrite_relative_image_destination(&dest_url, source_directory, destination_directory)
        else {
            continue;
        };
        if link_type == LinkType::Inline {
            if let Some(range) = inline_image_destination_range(markdown, range) {
                replacements.push((range, destination));
            }
        } else {
            reference_destinations.insert(normalize_reference_id(&id), destination);
        }
    }

    if !reference_destinations.is_empty() {
        let mut line_offset = 0;
        for line in markdown.split_inclusive('\n') {
            if let Some((id, range)) = reference_definition_target_range(line, line_offset)
                && let Some(destination) = reference_destinations.get(&id)
            {
                replacements.push((range, destination.clone()));
            }
            line_offset += line.len();
        }
    }

    replacements.sort_by(|left, right| right.0.start.cmp(&left.0.start));
    let mut rewritten = markdown.to_string();
    for (range, destination) in replacements {
        rewritten.replace_range(range, &destination);
    }
    rewritten
}

fn normalize_reference_id(id: &str) -> String {
    id.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn reference_definition_target_range(
    line: &str,
    line_offset: usize,
) -> Option<(String, Range<usize>)> {
    let bytes = line.as_bytes();
    let mut cursor = 0;
    while bytes.get(cursor) == Some(&b' ') && cursor < 4 {
        cursor += 1;
    }
    if bytes.get(cursor) != Some(&b'[') {
        return None;
    }
    let id_start = cursor + 1;
    cursor = id_start;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'\\' => cursor = cursor.checked_add(2)?,
            b']' => break,
            _ => cursor += 1,
        }
    }
    if bytes.get(cursor) != Some(&b']') || bytes.get(cursor + 1) != Some(&b':') {
        return None;
    }
    let id = normalize_reference_id(line.get(id_start..cursor)?);
    cursor += 2;
    while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
        cursor += 1;
    }

    let (start, end) = if bytes.get(cursor) == Some(&b'<') {
        let start = cursor + 1;
        cursor = start;
        while cursor < bytes.len() {
            if bytes[cursor] == b'\\' {
                cursor = cursor.checked_add(2)?;
            } else if bytes[cursor] == b'>' {
                break;
            } else {
                cursor += 1;
            }
        }
        (start, cursor)
    } else {
        let start = cursor;
        while cursor < bytes.len() && !bytes[cursor].is_ascii_whitespace() && bytes[cursor] != b')'
        {
            if bytes[cursor] == b'\\' {
                cursor = cursor.checked_add(2)?;
            } else {
                cursor += 1;
            }
        }
        (start, cursor)
    };
    (start < end).then_some((id, (line_offset + start)..(line_offset + end)))
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if !normalized.pop() {
                    normalized.push(component.as_os_str());
                }
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

fn relative_path_between(from: &Path, to: &Path) -> Option<PathBuf> {
    let from = normalize_path(from);
    let to = normalize_path(to);
    if !from.is_absolute() || !to.is_absolute() {
        return None;
    }
    let from_components = from.components().collect::<Vec<_>>();
    let to_components = to.components().collect::<Vec<_>>();
    let mut common = 0;
    while common < from_components.len()
        && common < to_components.len()
        && from_components[common] == to_components[common]
    {
        common += 1;
    }
    if common == 0 {
        return None;
    }

    let mut relative = PathBuf::new();
    for component in &from_components[common..] {
        if matches!(component, std::path::Component::Normal(_)) {
            relative.push("..");
        }
    }
    for component in &to_components[common..] {
        if matches!(
            component,
            std::path::Component::Normal(_) | std::path::Component::ParentDir
        ) {
            relative.push(component.as_os_str());
        }
    }
    Some(relative)
}

fn path_is_affected(path: &Path, target: &Path, target_is_directory: bool) -> bool {
    if target_is_directory {
        path.starts_with(target)
    } else {
        path == target
    }
}

/// Match options for workspace/document search, mirroring VS Code's toggles.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct SearchOptions {
    pub(super) match_case: bool,
    pub(super) whole_word: bool,
    pub(super) use_regex: bool,
    pub(super) fuzzy: bool,
}

/// Compiled search query. Regex compilation failures degrade to a plain
/// substring search so a bad pattern never silently kills search.
#[derive(Clone)]
pub(super) struct SearchMatcher {
    query: String,
    options: SearchOptions,
    regex: Option<regex::Regex>,
}

impl SearchMatcher {
    pub(super) fn new(query: &str, options: SearchOptions) -> Self {
        let regex = if options.use_regex {
            let mut builder = regex::RegexBuilder::new(query);
            builder.case_insensitive(!options.match_case);
            builder.build().ok()
        } else {
            None
        };
        Self {
            query: query.to_string(),
            options,
            regex,
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.query.trim().is_empty() && self.regex.is_none()
    }

    /// Byte ranges of every match inside `line`.
    pub(super) fn find_in_line(&self, line: &str) -> Vec<Range<usize>> {
        if let Some(regex) = self.regex.as_ref() {
            return regex
                .find_iter(line)
                .map(|m| m.start()..m.end())
                .collect();
        }
        if self.options.fuzzy {
            return fuzzy_subsequence_ranges(line, &self.query);
        }
        let mut ranges = if self.options.match_case {
            line.match_indices(&self.query)
                .map(|(start, matched)| start..start + matched.len())
                .collect()
        } else {
            case_insensitive_ranges(line, &self.query)
        };
        if self.options.whole_word {
            ranges.retain(|range| is_word_boundary(line, range));
        }
        ranges
    }

    /// Whether a filename matches (fuzzy subsequence or substring).
    pub(super) fn matches_filename(&self, name: &str) -> bool {
        if self.options.fuzzy {
            return !fuzzy_subsequence_ranges(name, &self.query).is_empty();
        }
        if self.options.match_case {
            name.contains(&self.query)
        } else {
            case_insensitive_contains(name, &self.query)
        }
    }
}

fn case_insensitive_contains(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    if needle.is_ascii() {
        haystack
            .as_bytes()
            .windows(needle.len())
            .any(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
    } else {
        !case_insensitive_ranges(haystack, needle).is_empty()
    }
}

/// Case-insensitive byte ranges for one line. ASCII needles use a fast
/// sliding compare; non-ASCII needles fall back to per-char lowercase
/// comparison (haystack byte offsets stay stable because lowercase folding
/// of a char never splits the position bookkeeping below).
fn case_insensitive_ranges(line: &str, query: &str) -> Vec<Range<usize>> {
    if query.is_empty() || line.is_empty() || line.len() < query.len() {
        return Vec::new();
    }
    if query.is_ascii() {
        let mut ranges = Vec::new();
        let last = line.len() - query.len();
        let bytes = line.as_bytes();
        let first = query.as_bytes()[0];
        let mut start = 0;
        while start <= last {
            // 先比对首字节再展开整窗：不命中位置只做一次单字节大小写不敏感
            // 比较，避免每个位置都比完整窗口。
            if bytes[start].eq_ignore_ascii_case(&first)
                && bytes[start..start + query.len()].eq_ignore_ascii_case(query.as_bytes())
            {
                ranges.push(start..start + query.len());
                start += query.len();
            } else {
                start += 1;
            }
        }
        return ranges;
    }

    let query_chars: Vec<char> = query.to_lowercase().chars().collect();
    if query_chars.is_empty() {
        return Vec::new();
    }
    let mut ranges = Vec::new();
    let char_positions: Vec<(usize, char)> = line.char_indices().collect();
    for start_index in 0..char_positions.len() {
        let mut query_index = 0usize;
        let mut cursor = start_index;
        while cursor < char_positions.len() && query_index < query_chars.len() {
            let (_, line_char) = char_positions[cursor];
            let mut folded = line_char.to_lowercase();
            let matches = match (folded.next(), folded.next()) {
                (Some(first), None) => first == query_chars[query_index],
                _ => line_char == query_chars[query_index],
            };
            if !matches {
                break;
            }
            query_index += 1;
            cursor += 1;
        }
        if query_index == query_chars.len() {
            let start = char_positions[start_index].0;
            let end = if cursor < char_positions.len() {
                char_positions[cursor].0
            } else {
                line.len()
            };
            ranges.push(start..end);
        }
    }
    ranges
}

/// fzf-style subsequence match: every query char must appear in order
/// (case-insensitive); the returned range spans the first to last consumed
/// char so the hit can be selected on jump.
fn fuzzy_subsequence_ranges(line: &str, query: &str) -> Vec<Range<usize>> {
    let query_chars: Vec<char> = query
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .flat_map(|ch| ch.to_lowercase())
        .collect();
    if query_chars.is_empty() || line.is_empty() {
        return Vec::new();
    }
    let positions: Vec<(usize, char)> = line.char_indices().collect();
    let mut ranges = Vec::new();
    for start_index in 0..positions.len() {
        let mut query_index = 0usize;
        let mut cursor = start_index;
        while cursor < positions.len() && query_index < query_chars.len() {
            let (_, line_char) = positions[cursor];
            let folded = line_char.to_lowercase().next().unwrap_or(line_char);
            if folded == query_chars[query_index] {
                query_index += 1;
            }
            cursor += 1;
        }
        if query_index == query_chars.len() {
            let start = positions[start_index].0;
            let end = positions
                .get(cursor)
                .map(|(offset, _)| *offset)
                .unwrap_or(line.len());
            ranges.push(start..end);
        }
    }
    ranges
}

fn is_word_boundary(line: &str, range: &Range<usize>) -> bool {
    let word_char = |ch: char| ch.is_alphanumeric() || ch == '_';
    let before = line[..range.start]
        .chars()
        .next_back()
        .map(word_char)
        .unwrap_or(false);
    let after = line[range.end..]
        .chars()
        .next()
        .map(word_char)
        .unwrap_or(false);
    !before && !after
}

/// 工作区搜索的待扫文件（树序）。`searchable` 为 false 的文件（非文本）只
/// 匹配文件名，不读内容。
#[derive(Clone)]
struct WorkspaceSearchFile {
    path: PathBuf,
    label: String,
    searchable: bool,
}

/// 按树序收集待搜索文件，供并行分片使用。
fn collect_workspace_search_files(root: &WorkspaceTreeNode) -> Vec<WorkspaceSearchFile> {
    let root_path = match &root.kind {
        WorkspaceTreeKind::Directory(path) => path.as_path(),
        _ => return Vec::new(),
    };
    let mut files = Vec::new();
    fn visit(node: &WorkspaceTreeNode, root: &Path, files: &mut Vec<WorkspaceSearchFile>) {
        match &node.kind {
            WorkspaceTreeKind::Directory(_) => {
                for child in &node.children {
                    visit(child, root, files);
                }
            }
            WorkspaceTreeKind::MarkdownFile(path) | WorkspaceTreeKind::CodeFile(path) => {
                files.push(WorkspaceSearchFile {
                    path: path.clone(),
                    label: search_file_label(path, root),
                    searchable: true,
                });
            }
            WorkspaceTreeKind::OtherFile(path) => {
                files.push(WorkspaceSearchFile {
                    path: path.clone(),
                    label: search_file_label(path, root),
                    searchable: false,
                });
            }
            WorkspaceTreeKind::Heading { .. } => {}
        }
    }
    visit(root, root_path, &mut files);
    files
}

fn search_file_label(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

/// 文件内容缓存：上一轮搜过的文件本轮直接走内存，只付一次 stat 的代价校验
/// 是否过期（用户报修：工作区变大后每敲一键都全树重读磁盘，比 VS Code 慢得
/// 多）。容量超限时按最久未用驱逐。
const SEARCH_CACHE_MAX_BYTES: usize = 128 * 1024 * 1024;
const SEARCH_CACHE_MAX_FILE_BYTES: u64 = 20_000_000;

struct SearchContentCacheEntry {
    mtime: std::time::SystemTime,
    contents: std::sync::Arc<str>,
    last_used: std::time::Instant,
}

fn search_content_cache() -> &'static std::sync::Mutex<
    HashMap<PathBuf, SearchContentCacheEntry>,
> {
    static CACHE: std::sync::OnceLock<
        std::sync::Mutex<HashMap<PathBuf, SearchContentCacheEntry>>,
    > = std::sync::OnceLock::new();
    CACHE.get_or_init(|| std::sync::Mutex::new(HashMap::new()))
}

/// 读取文件内容用于搜索：命中缓存（mtime 未变）零拷贝返回；未命中读盘一次
/// 并入缓存。非 UTF-8 文件返回 None（跳过内容搜索）。
fn cached_file_source(path: &Path) -> Option<std::sync::Arc<str>> {
    let metadata = fs::metadata(path).ok()?;
    if metadata.len() > SEARCH_CACHE_MAX_FILE_BYTES {
        return None;
    }
    let mtime = metadata.modified().ok()?;

    {
        let cache = search_content_cache();
        let Ok(cache) = cache.lock() else {
            return None;
        };
        if let Some(entry) = cache.get(path) {
            if entry.mtime == mtime {
                return Some(entry.contents.clone());
            }
        }
    }

    // 读盘不持锁：并行分片时不能让一把缓存锁把所有 worker 串行化。
    let bytes = fs::read(path).ok()?;
    let contents: std::sync::Arc<str> = match String::from_utf8(bytes) {
        Ok(text) => std::sync::Arc::from(text),
        Err(_) => return None,
    };

    if let Ok(mut cache) = search_content_cache().lock() {
        if let Some(existing) = cache.get(path) {
            if existing.mtime == mtime {
                return Some(existing.contents.clone());
            }
        }
        // 粗粒度总量记账：超限就把最久未用的条目逐出，直到放得下。单文件
        // 上限 20MB 远小于总上限，刚插入的条目不会被自己挤掉。
        let inserted = contents.len();
        if inserted <= SEARCH_CACHE_MAX_BYTES {
            while cache.len() * 4 > SEARCH_CACHE_MAX_BYTES
                || cache.values().map(|entry| entry.contents.len()).sum::<usize>()
                    > SEARCH_CACHE_MAX_BYTES - inserted
            {
                let oldest = cache
                    .iter()
                    .min_by_key(|(_, entry)| entry.last_used)
                    .map(|(key, _)| key.clone());
                match oldest {
                    Some(key) => {
                        cache.remove(&key);
                    }
                    None => break,
                }
            }
            cache.insert(
                path.to_path_buf(),
                SearchContentCacheEntry {
                    mtime,
                    contents: contents.clone(),
                    last_used: std::time::Instant::now(),
                },
            );
        }
    }
    Some(contents)
}

/// 单文件搜索：文件名匹配 +（文本文件的）内容匹配。与旧版逐文件逻辑一致。
fn search_single_file(
    file: &WorkspaceSearchFile,
    matcher: &SearchMatcher,
    limit: usize,
    hits: &mut Vec<WorkspaceSearchHit>,
) {
    if hits.len() >= limit {
        return;
    }
    if matcher.matches_filename(&file.label) {
        hits.push(WorkspaceSearchHit {
            path: file.path.clone(),
            label: file.label.clone(),
            line: None,
            match_range: None,
            source_range: None,
            preview: String::new(),
        });
        if hits.len() >= limit {
            return;
        }
    }
    if !file.searchable {
        return;
    }
    let Some(source) = cached_file_source(&file.path) else {
        return;
    };
    let mut file_hits = 0;
    for (index, raw_line) in source.split_inclusive('\n').enumerate() {
        let line = raw_line.strip_suffix('\n').unwrap_or(raw_line);
        let matches = matcher.find_in_line(line);
        if let Some(first) = matches.first() {
            hits.push(WorkspaceSearchHit {
                path: file.path.clone(),
                label: file.label.clone(),
                line: Some(index + 1),
                match_range: Some(first.clone()),
                source_range: None,
                preview: line.trim().chars().take(140).collect(),
            });
            file_hits += 1;
            if file_hits == 3 || hits.len() >= limit {
                break;
            }
        }
    }
}

/// 工作区搜索：文件列表按 CPU 核数分片，在后台线程池并行扫描；结果按分片
/// 顺序合并保持树序稳定。上一轮读过且未变更的文件内容直接命中缓存，不再
/// 逐个重读磁盘（用户报修：大工作区搜索远慢于 VS Code）。
async fn search_workspace_files(
    root: &WorkspaceTreeNode,
    matcher: &SearchMatcher,
    limit: usize,
    background: &gpui::BackgroundExecutor,
) -> Vec<WorkspaceSearchHit> {
    if matcher.is_empty() || limit == 0 {
        return Vec::new();
    }
    let files = collect_workspace_search_files(root);
    if files.is_empty() {
        return Vec::new();
    }
    let workers = std::thread::available_parallelism()
        .map(|parallelism| parallelism.get())
        .unwrap_or(4)
        .min(files.len());
    let chunk_size = files.len().div_ceil(workers);
    let matcher = std::sync::Arc::new(matcher.clone());
    let mut tasks = Vec::new();
    for chunk in files.chunks(chunk_size) {
        let chunk = chunk.to_vec();
        let matcher = matcher.clone();
        tasks.push(background.spawn(async move {
            // 分片体是同步扫描，panic 隔离在这里完成（一个分片炸掉只丢自己的
            // 结果，降级为"无结果"而不是拖垮整个搜索）。
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut hits = Vec::new();
                for file in &chunk {
                    search_single_file(file, &matcher, limit, &mut hits);
                }
                hits
            }))
            .unwrap_or_default()
        }));
    }
    let mut hits = Vec::new();
    for task in tasks {
        if hits.len() >= limit {
            break;
        }
        hits.extend(task.await);
    }
    hits.truncate(limit);
    hits
}

fn search_document_source(
    source: &str,
    matcher: &SearchMatcher,
    path: &Path,
    label: &str,
    limit: usize,
) -> Vec<WorkspaceSearchHit> {
    if matcher.is_empty() || limit == 0 {
        return Vec::new();
    }
    let mut hits = Vec::new();
    let mut absolute = 0usize;
    for (line_index, raw_line) in source.split_inclusive('\n').enumerate() {
        let line = raw_line.strip_suffix('\n').unwrap_or(raw_line);
        for range in matcher.find_in_line(line) {
            hits.push(WorkspaceSearchHit {
                path: path.to_path_buf(),
                label: label.to_string(),
                line: Some(line_index + 1),
                match_range: Some(range.start..range.end),
                source_range: Some(absolute + range.start..absolute + range.end),
                preview: line.trim().chars().take(140).collect(),
            });
            if hits.len() == limit {
                return hits;
            }
        }
        absolute += raw_line.len();
    }
    hits
}

/// Next match at or after `from` (or before, when reversing) across the whole
/// document source, wrapping around once.
fn find_document_match_from(
    source: &str,
    matcher: &SearchMatcher,
    from: usize,
    reverse: bool,
) -> Option<Range<usize>> {
    if matcher.is_empty() {
        return None;
    }
    let mut from = from.min(source.len());
    while !source.is_char_boundary(from) {
        from -= 1;
    }

    let mut ranges = Vec::new();
    let mut absolute = 0usize;
    for raw_line in source.split_inclusive('\n') {
        let line = raw_line.strip_suffix('\n').unwrap_or(raw_line);
        for range in matcher.find_in_line(line) {
            ranges.push(absolute + range.start..absolute + range.end);
        }
        absolute += raw_line.len();
    }
    if ranges.is_empty() {
        return None;
    }
    if reverse {
        ranges
            .iter()
            .rev()
            .find(|range| range.start < from)
            .or_else(|| ranges.last())
            .cloned()
    } else {
        ranges
            .iter()
            .find(|range| range.start >= from)
            .or_else(|| ranges.first())
            .cloned()
    }
}

/// Detects a UTF-16 BOM (LE or BE). Such files are text, but the editor only
/// renders UTF-8 — they get a specific placeholder instead of a parse error
/// (roadmap G2).
fn has_utf16_bom(path: &Path) -> bool {
    use std::io::Read;
    let mut file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(_) => return false,
    };
    let mut head = [0u8; 2];
    match file.read_exact(&mut head) {
        Ok(()) => head == [0xFF, 0xFE] || head == [0xFE, 0xFF],
        Err(_) => false,
    }
}

/// Heuristic text detection (same shape as Git's `is_text`): read up to the
/// first 8 KiB and treat the file as text when it decodes as UTF-8 (lossy
/// covers Latin-1-ish legacy files) and contains no NUL byte — the signature
/// of binary formats.
fn is_likely_text_file(path: &Path) -> bool {
    use std::io::Read;
    let mut file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(_) => return false,
    };
    let mut head = [0u8; 8192];
    let mut read = 0usize;
    loop {
        match file.read(&mut head[read..]) {
            Ok(0) => break,
            Ok(n) => {
                read += n;
                if read == head.len() {
                    break;
                }
            }
            Err(_) => return false,
        }
    }
    let head = &head[..read];
    // UTF-16 BOMs are text but not UTF-8; treat them as previewable anyway
    // since the editor renders UTF-8 only.
    if read >= 2 && (head.starts_with(&[0xFF, 0xFE]) || head.starts_with(&[0xFE, 0xFF])) {
        return true;
    }
    if head.contains(&0) {
        return false;
    }
    match std::str::from_utf8(head) {
        Ok(_) => true,
        // A read window that ends inside a multi-byte character is still a text
        // prefix: `error_len() == None` means the only problem is that the
        // window cut the last character (an 8 KiB window over CJK text hits
        // this constantly). Treating it as binary hid whole documents behind
        // the "can't preview" notice.
        Err(error) => error.error_len().is_none(),
    }
}

pub(super) fn is_code_file(path: &Path) -> bool {
    const CODE_EXTENSIONS: &[&str] = &[
        "c", "cc", "cpp", "cs", "css", "go", "h", "hpp", "html", "java", "js", "json", "jsx", "kt",
        "php", "py", "rb", "rs", "sh", "sql", "swift", "toml", "ts", "tsx", "xml", "yaml", "yml",
        "zsh", "txt", "csv", "log", "ini", "conf", "lock",
    ];

    path.extension().is_some_and(|extension| {
        let extension = extension.to_string_lossy();
        CODE_EXTENSIONS
            .iter()
            .any(|candidate| extension.eq_ignore_ascii_case(candidate))
    })
}

impl WorkspaceTreeNode {
    fn kind_dir(&self) -> bool {
        matches!(self.kind, WorkspaceTreeKind::Directory(_))
    }
}

fn tree_node_path(node: &WorkspaceTreeNode) -> &Path {
    match &node.kind {
        WorkspaceTreeKind::Directory(path)
        | WorkspaceTreeKind::MarkdownFile(path)
        | WorkspaceTreeKind::CodeFile(path)
        | WorkspaceTreeKind::OtherFile(path) => path,
        WorkspaceTreeKind::Heading { .. } => Path::new(""),
    }
}

fn scan_workspace_dir(path: &Path, sort: TreeSortPreference) -> Result<WorkspaceTreeNode> {
    let mut children = Vec::new();
    for entry in
        fs::read_dir(path).with_context(|| format!("failed to read '{}'", path.display()))?
    {
        let entry = entry?;
        let entry_path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            let name = entry.file_name();
            if matches!(
                name.to_str(),
                Some(".git" | "target" | "node_modules" | ".worktrees" | "dist")
            ) {
                continue;
            }
            children.push(scan_workspace_dir(&entry_path, sort)?);
        } else if file_type.is_file() && is_markdown_file(&entry_path) {
            children.push(WorkspaceTreeNode {
                id: file_node_id(&entry_path),
                label: file_label(&entry_path),
                kind: WorkspaceTreeKind::MarkdownFile(entry_path),
                children: Vec::new(),
            });
        } else if file_type.is_file() && is_code_file(&entry_path) {
            children.push(WorkspaceTreeNode {
                id: file_node_id(&entry_path),
                label: file_label(&entry_path),
                kind: WorkspaceTreeKind::CodeFile(entry_path),
                children: Vec::new(),
            });
        } else if file_type.is_file() {
            children.push(WorkspaceTreeNode {
                id: file_node_id(&entry_path),
                label: file_label(&entry_path),
                kind: WorkspaceTreeKind::OtherFile(entry_path),
                children: Vec::new(),
            });
        }
    }

    children.sort_by(|left, right| {
        let left_dir = matches!(left.kind, WorkspaceTreeKind::Directory(_));
        let right_dir = matches!(right.kind, WorkspaceTreeKind::Directory(_));
        // Directories always group first; within a group the preference
        // decides the key (roadmap D2).
        right_dir.cmp(&left_dir).then_with(|| match sort {
            TreeSortPreference::Name => left
                .label
                .to_lowercase()
                .cmp(&right.label.to_lowercase()),
            TreeSortPreference::ModifiedTime => {
                let left_time = fs::metadata(tree_node_path(left))
                    .ok()
                    .and_then(|meta| meta.modified().ok());
                let right_time = fs::metadata(tree_node_path(right))
                    .ok()
                    .and_then(|meta| meta.modified().ok());
                // Newest first; missing metadata sorts last.
                right_time.cmp(&left_time)
            }
            TreeSortPreference::Type => {
                let left_ext = tree_node_path(left)
                    .extension()
                    .map(|e| e.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let right_ext = tree_node_path(right)
                    .extension()
                    .map(|e| e.to_string_lossy().into_owned())
                    .unwrap_or_default();
                left_ext.cmp(&right_ext).then_with(|| {
                    left.label.to_lowercase().cmp(&right.label.to_lowercase())
                })
            }
        })
    });

    Ok(WorkspaceTreeNode {
        id: file_node_id(path),
        label: file_label(path),
        kind: WorkspaceTreeKind::Directory(path.to_path_buf()),
        children,
    })
}

fn search_utf16_to_utf8(text: &str, offset: usize) -> usize {
    let mut utf16 = 0;
    for (byte, ch) in text.char_indices() {
        if utf16 >= offset || utf16 + ch.len_utf16() > offset {
            return byte;
        }
        utf16 += ch.len_utf16();
    }
    text.len()
}

fn search_utf8_to_utf16(text: &str, offset: usize) -> usize {
    let mut byte = offset.min(text.len());
    while !text.is_char_boundary(byte) {
        byte -= 1;
    }
    text[..byte].encode_utf16().count()
}

impl Editor {
    fn active_overlay_input(&self, window: &Window) -> OverlayInputKind {
        if self
            .quick_open
            .as_ref()
            .and_then(|state| state.focus.as_ref())
            .is_some_and(|focus| focus.is_focused(window))
        {
            return OverlayInputKind::QuickOpen;
        }
        if self
            .workspace
            .replace_focus
            .as_ref()
            .is_some_and(|focus| focus.is_focused(window))
        {
            OverlayInputKind::Replace
        } else {
            OverlayInputKind::Query
        }
    }

    fn input_text(&self, kind: OverlayInputKind) -> &str {
        match kind {
            OverlayInputKind::Query => &self.workspace.search_query,
            OverlayInputKind::Replace => &self.workspace.replace_query,
            OverlayInputKind::QuickOpen => self
                .quick_open
                .as_ref()
                .map(|state| state.query.as_str())
                .unwrap_or_default(),
        }
    }

    fn input_selection(&self, kind: OverlayInputKind) -> Range<usize> {
        match kind {
            OverlayInputKind::Query => self.workspace.search_selected_range.clone(),
            OverlayInputKind::Replace => self.workspace.replace_selected_range.clone(),
            OverlayInputKind::QuickOpen => self
                .quick_open
                .as_ref()
                .map(|state| state.selected_range.clone())
                .unwrap_or_default(),
        }
    }

    fn input_marked(&self, kind: OverlayInputKind) -> Option<Range<usize>> {
        match kind {
            OverlayInputKind::Query => self.workspace.search_marked_range.clone(),
            OverlayInputKind::Replace => self.workspace.replace_marked_range.clone(),
            OverlayInputKind::QuickOpen => self
                .quick_open
                .as_ref()
                .and_then(|state| state.marked_range.clone()),
        }
    }

    /// Applies an edit to the focused single-line overlay input (search query,
    /// search replace, or quick switcher) and refreshes what that input drives.
    fn replace_overlay_input_text(
        &mut self,
        kind: impl Into<OverlayInputKind>,
        range: Range<usize>,
        new_text: &str,
        selected_in_inserted: Option<Range<usize>>,
        marked: bool,
        cx: &mut Context<Self>,
    ) {
        let kind = kind.into();
        let (old, was_marked) = match kind {
            OverlayInputKind::Query => (
                self.workspace.search_query.clone(),
                self.workspace.search_marked_range.is_some(),
            ),
            OverlayInputKind::Replace => (
                self.workspace.replace_query.clone(),
                self.workspace.replace_marked_range.is_some(),
            ),
            OverlayInputKind::QuickOpen => (
                self.quick_open
                    .as_ref()
                    .map(|state| state.query.clone())
                    .unwrap_or_default(),
                self.quick_open
                    .as_ref()
                    .is_some_and(|state| state.marked_range.is_some()),
            ),
        };
        let start = range.start.min(old.len());
        let end = range.end.min(old.len()).max(start);
        if !old.is_char_boundary(start) || !old.is_char_boundary(end) {
            return;
        }
        let inserted = new_text.replace(['\r', '\n'], " ");
        let updated = {
            let mut updated = old.clone();
            updated.replace_range(start..end, &inserted);
            updated
        };
        let inserted_end = start + inserted.len();
        let selection = selected_in_inserted
            .map(|selection| {
                start + selection.start.min(inserted.len())
                    ..start + selection.end.min(inserted.len())
            })
            .unwrap_or(inserted_end..inserted_end);
        let marked_range = (marked && !inserted.is_empty()).then_some(start..inserted_end);
        match kind {
            OverlayInputKind::Query => {
                self.workspace.search_query = updated;
                self.workspace.search_selected_range = selection;
                self.workspace.search_marked_range = marked_range;
                if !marked && (self.workspace.search_query != old || was_marked) {
                    self.schedule_workspace_search(cx);
                }
            }
            OverlayInputKind::Replace => {
                self.workspace.replace_query = updated;
                self.workspace.replace_selected_range = selection;
                self.workspace.replace_marked_range = marked_range;
            }
            OverlayInputKind::QuickOpen => {
                if let Some(state) = self.quick_open.as_mut() {
                    state.query = updated;
                    state.selected_range = selection;
                    state.marked_range = marked_range;
                    state.selected = 0;
                }
                if !marked && (self.input_text(kind) != old.as_str() || was_marked) {
                    self.refresh_quick_open_results(cx);
                }
            }
        }
        cx.notify();
    }

    /// Applies an edit to the quick switcher query (roadmap E9).
    pub(super) fn replace_quick_open_input_text(
        &mut self,
        range: Range<usize>,
        new_text: &str,
        selected_in_inserted: Option<Range<usize>>,
        cx: &mut Context<Self>,
    ) {
        self.replace_overlay_input_text(
            OverlayInputKind::QuickOpen,
            range,
            new_text,
            selected_in_inserted,
            false,
            cx,
        );
    }
}

// Only the focused workspace search fields register this handler; document
// blocks keep their own input handlers and IME state. The query and replace
// fields share the editor's handler, routed by which focus handle is active.
impl EntityInputHandler for Editor {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let kind = self.active_overlay_input(window);
        let text = self.input_text(kind);
        let start = search_utf16_to_utf8(text, range.start);
        let end = search_utf16_to_utf8(text, range.end).max(start);
        *actual_range = Some(search_utf8_to_utf16(text, start)..search_utf8_to_utf16(text, end));
        Some(text[start..end].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let kind = self.active_overlay_input(window);
        let text = self.input_text(kind).to_string();
        let range = self.input_selection(kind);
        Some(UTF16Selection {
            range: search_utf8_to_utf16(&text, range.start)..search_utf8_to_utf16(&text, range.end),
            reversed: false,
        })
    }

    fn marked_text_range(
        &self,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        let kind = self.active_overlay_input(window);
        let text = self.input_text(kind).to_string();
        self.input_marked(kind).map(|range| {
            search_utf8_to_utf16(&text, range.start)..search_utf8_to_utf16(&text, range.end)
        })
    }

    fn unmark_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let kind = self.active_overlay_input(window);
        let was_marked = self.input_marked(kind).is_some();
        match kind {
            OverlayInputKind::Query => {
                self.workspace.search_marked_range = None;
            }
            OverlayInputKind::Replace => {
                self.workspace.replace_marked_range = None;
            }
            OverlayInputKind::QuickOpen => {
                if let Some(state) = self.quick_open.as_mut() {
                    state.marked_range = None;
                }
            }
        }
        if was_marked {
            match kind {
                OverlayInputKind::Query => self.schedule_workspace_search(cx),
                OverlayInputKind::Replace => {}
                OverlayInputKind::QuickOpen => self.refresh_quick_open_results(cx),
            }
            cx.notify();
        }
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let kind = self.active_overlay_input(window);
        let query = self.input_text(kind).to_string();
        let range = range
            .map(|range| {
                search_utf16_to_utf8(&query, range.start)..search_utf16_to_utf8(&query, range.end)
            })
            .or_else(|| self.input_marked(kind))
            .unwrap_or_else(|| self.input_selection(kind));
        self.replace_overlay_input_text(kind, range, text, None, false, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        new_text: &str,
        new_selected_range: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let kind = self.active_overlay_input(window);
        let query = self.input_text(kind).to_string();
        let range = range
            .map(|range| {
                search_utf16_to_utf8(&query, range.start)..search_utf16_to_utf8(&query, range.end)
            })
            .or_else(|| self.input_marked(kind))
            .unwrap_or_else(|| self.input_selection(kind));
        let selected = new_selected_range.map(|range| {
            search_utf16_to_utf8(new_text, range.start)..search_utf16_to_utf8(new_text, range.end)
        });
        self.replace_overlay_input_text(kind, range, new_text, selected, true, cx);
    }

    fn bounds_for_range(
        &mut self,
        _range: Range<usize>,
        bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        Some(bounds)
    }

    fn character_index_for_point(
        &mut self,
        _point: Point<Pixels>,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let kind = self.active_overlay_input(window);
        Some(self.input_text(kind).encode_utf16().count())
    }
}

/// Flattens the markdown/code files of a workspace tree in display order.
fn collect_workspace_files(root: &WorkspaceTreeNode) -> Vec<PathBuf> {
    let mut files = Vec::new();
    fn visit(node: &WorkspaceTreeNode, files: &mut Vec<PathBuf>) {
        match &node.kind {
            WorkspaceTreeKind::Directory(_) => {
                for child in &node.children {
                    visit(child, files);
                }
            }
            WorkspaceTreeKind::MarkdownFile(path) | WorkspaceTreeKind::CodeFile(path) => {
                files.push(path.clone());
            }
            // Other files can't be opened, so they are never replacement
            // targets for bulk replace.
            WorkspaceTreeKind::OtherFile(_) => {}
            WorkspaceTreeKind::Heading { .. } => {}
        }
    }
    visit(root, &mut files);
    files
}

/// Number of matches in a whole source string.
fn count_matches_in_source(source: &str, matcher: &SearchMatcher) -> usize {
    let mut count = 0;
    for raw_line in source.split_inclusive('\n') {
        let line = raw_line.strip_suffix('\n').unwrap_or(raw_line);
        count += matcher.find_in_line(line).len();
    }
    count
}

/// Rewrites every match in `source` with `replacement`.
fn replace_in_source(source: &str, matcher: &SearchMatcher, replacement: &str) -> String {
    if matcher.is_empty() {
        return source.to_string();
    }
    let mut updated = String::with_capacity(source.len());
    for raw_line in source.split_inclusive('\n') {
        let line = raw_line.strip_suffix('\n').unwrap_or(raw_line);
        let trailing_newline = raw_line.len() - line.len();
        let mut line_cursor = 0usize;
        for range in matcher.find_in_line(line) {
            if range.start < line_cursor {
                continue;
            }
            updated.push_str(&line[line_cursor..range.start]);
            updated.push_str(replacement);
            line_cursor = range.end;
        }
        updated.push_str(&line[line_cursor..]);
        if trailing_newline > 0 {
            updated.push('\n');
        }
    }
    updated
}

/// Hover tooltip text for a tree node: relative path, size, and modified time
/// for files; just the path for directories (roadmap D7).
fn tree_node_tooltip(node: &WorkspaceTreeNode) -> String {
    let path = match &node.kind {
        WorkspaceTreeKind::Directory(path)
        | WorkspaceTreeKind::MarkdownFile(path)
        | WorkspaceTreeKind::CodeFile(path)
        | WorkspaceTreeKind::OtherFile(path) => path,
        WorkspaceTreeKind::Heading { .. } => return node.label.clone(),
    };
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(_) => return node.label.clone(),
    };
    if node.kind_dir() {
        return node.label.clone();
    }
    let size = metadata.len();
    let size_text = if size >= 1024 * 1024 {
        format!("{:.1} MiB", size as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} KiB", size as f64 / 1024.0)
    };
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| {
            time.duration_since(std::time::UNIX_EPOCH).ok()
        })
        .map(|elapsed| {
            let seconds = elapsed.as_secs();
            chrono_like_date_string(seconds)
        })
        .unwrap_or_default();
    format!("{} · {} · {modified}", node.label, size_text)
}

/// Minimal local-date rendering from a unix timestamp (UTC date, good enough
/// for tooltips without pulling a time-zone database).
fn chrono_like_date_string(seconds: u64) -> String {
    let days = seconds / 86_400;
    // Civil-from-days algorithm (Howard Hinnant) for a UTC date.
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    let seconds_of_day = seconds % 86_400;
    let (hour, minute) = (seconds_of_day / 3600, (seconds_of_day % 3600) / 60);
    format!("{y:04}-{m:02}-{d:02} {hour:02}:{minute:02}")
}

fn file_label(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

fn file_node_id(path: &Path) -> String {
    format!("file:{}", path.to_string_lossy())
}

fn stable_node_hash(id: &str) -> u64 {
    use std::hash::{Hash, Hasher};

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    id.hash(&mut hasher);
    hasher.finish()
}

fn clamp_workspace_panel_width(width: f32, viewport_width: f32) -> f32 {
    let maximum = (viewport_width - 320.0).clamp(180.0, 600.0);
    width.clamp(180.0, maximum)
}

fn prune_outline_state(workspace: &mut WorkspaceState, outline: &[WorkspaceTreeNode]) {
    let mut current_ids = HashSet::new();
    collect_node_ids(outline, &mut current_ids);
    workspace
        .expanded
        .retain(|id| !is_outline_node_id(id) || current_ids.contains(id));

    if matches!(
        &workspace.selected,
        Some(WorkspaceSelection::Outline(id)) if !current_ids.contains(id)
    ) {
        workspace.selected = None;
    }
}

fn collect_node_ids(nodes: &[WorkspaceTreeNode], ids: &mut HashSet<String>) {
    for node in nodes {
        ids.insert(node.id.clone());
        collect_node_ids(&node.children, ids);
    }
}

fn is_outline_node_id(id: &str) -> bool {
    id.starts_with("outline:")
}

/// Marks every heading node at or above `max_level` expanded so the outline
/// opens down to that level (level 2 = H1/H2 expanded, showing H3 leaves).
fn expand_outline_to_level(
    nodes: &[WorkspaceTreeNode],
    max_level: u8,
    expanded: &mut HashSet<String>,
) {
    for node in nodes {
        if let WorkspaceTreeKind::Heading { level, .. } = &node.kind {
            if *level <= max_level {
                expanded.insert(node.id.clone());
            }
        }
        expand_outline_to_level(&node.children, max_level, expanded);
    }
}

/// 把大纲树压平为 `[TOC]` 块用的条目列表（roadmap C2），保持文档顺序。
fn flatten_outline_entries(nodes: &[WorkspaceTreeNode]) -> Vec<TocEntry> {
    fn visit(nodes: &[WorkspaceTreeNode], entries: &mut Vec<TocEntry>) {
        for node in nodes {
            if let WorkspaceTreeKind::Heading { line, level } = node.kind {
                entries.push(TocEntry {
                    level,
                    title: node.label.clone(),
                    line,
                });
            }
            visit(&node.children, entries);
        }
    }
    let mut entries = Vec::new();
    visit(nodes, &mut entries);
    entries
}

fn build_outline_tree(markdown: &str) -> Vec<WorkspaceTreeNode> {
    let mut roots = Vec::new();
    let mut stack: Vec<(u8, Vec<usize>)> = Vec::new();
    let mut fence: Option<(char, usize)> = None;

    for (line_index, line) in markdown.lines().enumerate() {
        let trimmed = line.trim_start();
        if let Some((marker, len)) = fence {
            if is_closing_fence(trimmed, marker, len) {
                fence = None;
            }
            continue;
        }

        if let Some(next_fence) = opening_fence(trimmed) {
            fence = Some(next_fence);
            continue;
        }

        let Some((level, title)) = BlockKind::parse_atx_heading_line(line) else {
            continue;
        };

        while stack
            .last()
            .is_some_and(|(parent_level, _)| *parent_level >= level)
        {
            stack.pop();
        }

        let node = WorkspaceTreeNode {
            id: format!("outline:{line_index}"),
            label: title,
            kind: WorkspaceTreeKind::Heading {
                line: line_index,
                level,
            },
            children: Vec::new(),
        };

        let siblings = if let Some((_, parent_path)) = stack.last() {
            children_at_path_mut(&mut roots, parent_path)
        } else {
            &mut roots
        };
        siblings.push(node);

        let mut node_path = stack
            .last()
            .map(|(_, path)| path.clone())
            .unwrap_or_default();
        node_path.push(siblings.len() - 1);
        stack.push((level, node_path));
    }

    roots
}

fn children_at_path_mut<'a>(
    nodes: &'a mut Vec<WorkspaceTreeNode>,
    path: &[usize],
) -> &'a mut Vec<WorkspaceTreeNode> {
    let mut current = nodes;
    for &index in path {
        current = &mut current[index].children;
    }
    current
}

fn opening_fence(trimmed: &str) -> Option<(char, usize)> {
    let marker = trimmed.chars().next()?;
    if marker != '`' && marker != '~' {
        return None;
    }
    let len = trimmed.chars().take_while(|ch| *ch == marker).count();
    (len >= 3).then_some((marker, len))
}

fn is_closing_fence(trimmed: &str, marker: char, len: usize) -> bool {
    let count = trimmed.chars().take_while(|ch| *ch == marker).count();
    count >= len && trimmed[count..].trim().is_empty()
}

#[cfg(test)]
mod search_bench;

#[cfg(test)]
mod tests {
    use super::{
        Editor, SearchMatcher, SearchOptions, TreeSortPreference, WorkspaceSelection,
        WorkspaceState,
        WorkspaceTreeKind, build_outline_tree, clamp_workspace_panel_width,
        create_workspace_file, create_workspace_folder, find_document_match_from, is_code_file, tree_node_path, has_utf16_bom,
        is_likely_text_file,
        path_is_affected, prune_outline_state, remap_moved_path, rewrite_relative_image_targets,
        scan_workspace_dir, search_document_source, search_utf8_to_utf16, search_utf16_to_utf8,
        search_workspace_files,
    };
    use crate::components::{Block, UndoCaptureKind};
    use gpui::{
        AppContext, ClipboardItem, EntityInputHandler, Modifiers, ScrollDelta, ScrollWheelEvent,
        TestAppContext, TouchPhase, point, px,
    };
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    #[test]
    fn search_offsets_keep_cjk_and_emoji_boundaries() {
        let text = "中😀a";
        assert_eq!(search_utf16_to_utf8(text, 1), "中".len());
        assert_eq!(search_utf16_to_utf8(text, 2), "中".len());
        assert_eq!(search_utf16_to_utf8(text, 3), "中😀".len());
        assert_eq!(search_utf8_to_utf16(text, "中😀".len()), 3);
    }

        #[gpui::test]
    async fn outline_rename_selects_heading_title(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
        });
        let (editor, cx) =
            cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
        editor.update(cx, |editor, cx| {
            editor.replace_document_from_markdown(
                "## Old Title\n\nbody".into(),
                None,
                cx,
            );
            editor.sync_workspace_outline(cx);
            editor.rename_outline_heading(0, cx);
        });
        editor.read_with(cx, |editor, cx| {
            let block = editor.document.first_root().expect("root");
            // 渲染模式标题块的内容坐标即整个标题文本。
            assert_eq!(
                block.read(cx).selected_range.clone(),
                0.."Old Title".len()
            );
        });
    }

    #[test]
    fn utf16_bom_detection() {
        let root = std::env::temp_dir().join(format!("velora-bom-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("create root");

        let le = root.join("le.txt");
        std::fs::write(&le, [0xFF, 0xFE, b'a', 0x00]).expect("write LE");
        assert!(has_utf16_bom(&le));

        let be = root.join("be.txt");
        std::fs::write(&be, [0xFE, 0xFF, 0x00, b'a']).expect("write BE");
        assert!(has_utf16_bom(&be));

        let utf8 = root.join("utf8.txt");
        std::fs::write(&utf8, "plain utf-8 text").expect("write utf8");
        assert!(!has_utf16_bom(&utf8));

        let _ = std::fs::remove_dir_all(root);
    }

    fn plain_matcher(query: &str) -> SearchMatcher {
        SearchMatcher::new(query, SearchOptions::default())
    }

#[gpui::test]
    async fn workspace_search_accepts_unicode_platform_input(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
        });
        let (editor, cx) =
            cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                // 侧栏默认关闭（用户需求）：搜索输入框在抽屉里，先展开再聚焦。
                editor.workspace.is_open = true;
                editor.workspace.active_tab = super::WorkspaceTab::Search;
                let focus = editor
                    .workspace
                    .search_focus
                    .get_or_insert_with(|| cx.focus_handle())
                    .clone();
                window.focus(&focus);
                cx.notify();
            });
            window.draw(cx).clear();
        });
        cx.simulate_input("你好");
        editor.read_with(cx, |editor, _cx| {
            assert_eq!(editor.workspace.search_query, "你好");
            assert_eq!(
                editor.workspace.search_selected_range,
                "你好".len().."你好".len()
            );
        });
        cx.simulate_keystrokes("backspace");
        editor.read_with(cx, |editor, _cx| {
            assert_eq!(editor.workspace.search_query, "你");
        });
        cx.update(|_window, cx| cx.write_to_clipboard(ClipboardItem::new_string("世界".into())));
        cx.simulate_keystrokes("cmd-v");
        editor.read_with(cx, |editor, _cx| {
            assert_eq!(editor.workspace.search_query, "你世界");
        });
    }

    #[gpui::test]
    async fn workspace_search_waits_for_ime_commit_before_searching(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
        });
        let (editor, cx) =
            cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.workspace.active_tab = super::WorkspaceTab::Search;
                let generation = editor.workspace.search_generation;
                editor.replace_and_mark_text_in_range(None, "ni", Some(2..2), window, cx);
                assert_eq!(editor.workspace.search_query, "ni");
                assert_eq!(editor.workspace.search_generation, generation);
                editor.replace_and_mark_text_in_range(None, "你", Some(1..1), window, cx);
                assert_eq!(editor.workspace.search_query, "你");
                assert_eq!(editor.workspace.search_generation, generation);
                editor.replace_text_in_range(None, "你", window, cx);
                assert!(editor.workspace.search_marked_range.is_none());
                assert_eq!(editor.workspace.search_generation, generation + 1);
            });
        });
    }

    /// 一份长于 8 KiB、且第 8192 个字节正好落在三字节汉字中间的 Markdown。
    fn long_cjk_markdown() -> String {
        let mut content = String::from("# 长篇中文文档\n\n");
        while content.len() + "中文内容。".len() <= 8191 {
            content.push_str("中文内容。");
        }
        while content.len() < 8191 {
            content.push('a');
        }
        assert_eq!(content.len(), 8191);
        content.push('中');
        content.push_str("\n\n结尾段落\n");
        content
    }

    #[test]
    fn text_sniffing_accepts_a_prefix_cut_mid_character() {
        // 用户报修：8 KiB 读取窗切在多字节字符中间时，窗口内不是合法 UTF-8，
        // 于是整篇中文文档被判成二进制，只显示「无法使用文本编辑器预览该文件」。
        let root = std::env::temp_dir().join(format!("velora-sniff-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).expect("create dir");
        let path = root.join("长篇.md");
        fs::write(&path, long_cjk_markdown()).expect("write markdown");

        let bytes = fs::read(&path).expect("read back");
        assert!(bytes.len() > 8192, "用例前提：文件要长于读取窗");
        assert!(
            std::str::from_utf8(&bytes[..8192]).is_err(),
            "用例前提：8192 字节必须切在多字节字符中间"
        );
        assert!(is_likely_text_file(&path), "切在多字节字符中间的文本前缀仍是文本");

        let binary = root.join("blob.bin");
        fs::write(&binary, [0u8, 1, 2, 3, 0, 5]).expect("write binary");
        assert!(!is_likely_text_file(&binary), "含 NUL 的文件不该被当成文本");
        let invalid = root.join("invalid.md");
        fs::write(&invalid, [b'a', 0xE5, 0x20, 0x20, 0x20]).expect("write invalid utf8");
        assert!(
            !is_likely_text_file(&invalid),
            "窗口内的非法 UTF-8（非截断）仍应是二进制"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[gpui::test]
    async fn opening_a_large_cjk_markdown_file_shows_the_editor(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
        });
        let root = std::env::temp_dir().join(format!("velora-cjk-open-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).expect("create dir");
        let path = root.join("长篇.md");
        fs::write(&path, long_cjk_markdown()).expect("write markdown");

        let (editor, cx) =
            cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
        editor.update(cx, |editor, cx| {
            editor.set_workspace_root(root.clone(), cx);
        });
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.open_workspace_file(path.clone(), window, cx);
            });
        });
        editor.read_with(cx, |editor, cx| {
            assert!(
                editor.unsupported_preview_path.is_none(),
                "长中文 md 文件应正常打开，不该显示「无法预览」占位"
            );
            assert_eq!(editor.file_path.as_ref(), Some(&path));
            assert!(editor.document.markdown_text(cx).contains("结尾段落"));
        });
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn workspace_scan_includes_markdown_and_code_files() {
        let root =
            std::env::temp_dir().join(format!("velora-workspace-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("nested")).expect("create dirs");
        fs::write(root.join("a.md"), "a").expect("write md");
        fs::write(root.join("a.txt"), "plain text").expect("write txt");
        fs::write(root.join("main.rs"), "fn main() {}").expect("write code");
        fs::write(root.join("nested").join("b.md"), "b").expect("write nested md");

        let tree = scan_workspace_dir(&root, TreeSortPreference::Name).expect("scan tree");
        let labels = tree
            .children
            .iter()
            .map(|node| node.label.as_str())
            .collect::<Vec<_>>();
        assert_eq!(labels, vec!["nested", "a.md", "a.txt", "main.rs"]);
        assert!(matches!(
            tree.children[0].kind,
            WorkspaceTreeKind::Directory(_)
        ));
        assert!(matches!(
            tree.children[1].kind,
            WorkspaceTreeKind::MarkdownFile(_)
        ));
        assert!(matches!(
            tree.children[2].kind,
            WorkspaceTreeKind::CodeFile(_)
        ));
        assert!(matches!(
            tree.children[3].kind,
            WorkspaceTreeKind::CodeFile(_)
        ));

        // Type sort groups files by extension before name (roadmap D2).
        let typed = scan_workspace_dir(&root, TreeSortPreference::Type).expect("scan typed");
        let extension_at = |index: usize| {
            tree_node_path(&typed.children[index])
                .extension()
                .map(|extension| extension.to_string_lossy().into_owned())
        };
        let first = extension_at(0);
        let second = extension_at(1);
        if let (Some(first), Some(second)) = (first.clone(), second) {
            assert!(
                first <= second,
                "extensions not ordered: {first} > {second}"
            );
        }

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn workspace_tree_includes_plain_viewer_code_extensions() {
        let root = std::env::temp_dir().join(format!(
            "velora-workspace-plain-code-test-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).expect("create root");
        for (name, source) in [
            ("query.sql", "select 1;"),
            ("App.swift", "struct App {}"),
            ("Main.kt", "fun main() {}"),
            ("layout.xml", "<root />"),
        ] {
            fs::write(root.join(name), source).expect("write code sample");
        }

        let tree = scan_workspace_dir(&root, TreeSortPreference::Name).expect("scan code workspace");
        assert_eq!(tree.children.len(), 4);
        assert!(
            tree.children
                .iter()
                .all(|node| { matches!(node.kind, WorkspaceTreeKind::CodeFile(_)) })
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn code_file_extensions_are_case_insensitive() {
        for extension in ["rs", "sql", "swift", "kt", "xml"] {
            assert!(is_code_file(Path::new(&format!("source.{extension}"))));
            assert!(is_code_file(Path::new(&format!(
                "source.{}",
                extension.to_ascii_uppercase()
            ))));
        }
        assert!(is_code_file(Path::new("notes.txt")));
    }

    #[gpui::test]
    async fn opening_code_files_keeps_them_in_the_editor(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
        });
        let root =
            std::env::temp_dir().join(format!("velora-code-viewer-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).expect("create test workspace");
        let path = root.join("main.rs");
        fs::write(&path, "fn main() { println!(\"hello\"); }").expect("write code file");
        let plain_path = root.join("query.sql");
        fs::write(&plain_path, "select 1;").expect("write plain code file");
        let crlf_path = root.join("windows.cs");
        fs::write(&crlf_path, "class A {\r\n}\r\n").expect("write CRLF code file");
        let cleanup_root = root.clone();
        cx.on_quit(move || {
            let _ = fs::remove_dir_all(cleanup_root);
        });

        let (editor, cx) =
            cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.open_workspace_file(path.clone(), window, cx)
            });
        });
        cx.update(|window, cx| window.draw(cx).clear());
        cx.run_until_parked();
        editor.read_with(cx, |editor, cx| {
            assert!(editor.code_tab_active());
            assert_eq!(
                editor.document.raw_source_text(cx),
                "fn main() { println!(\"hello\"); }"
            );
            let block = editor.document.first_root().unwrap().read(cx);
            assert!(block.kind().is_code_block());
            assert!(block.code_highlight_result().is_some());
            assert_eq!(editor.workspace.open_documents.len(), 1);
        });
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.open_workspace_file(plain_path.clone(), window, cx)
            });
        });
        cx.update(|window, cx| window.draw(cx).clear());
        cx.run_until_parked();
        editor.read_with(cx, |editor, cx| {
            assert_eq!(editor.workspace.open_documents.len(), 2);
            assert_eq!(editor.workspace.active_document.as_ref(), Some(&plain_path));
            assert_eq!(editor.document.raw_source_text(cx), "select 1;");
        });
        let block = editor.read_with(cx, |editor, _| {
            editor.document.first_root().unwrap().clone()
        });
        cx.update(|window, cx| {
            block.update(cx, |block, cx| {
                block.selected_range = 0..block.visible_len();
                <Block as EntityInputHandler>::replace_text_in_range(
                    block,
                    None,
                    "select 2;",
                    window,
                    cx,
                );
            });
        });
        cx.run_until_parked();
        editor.read_with(cx, |editor, cx| {
            assert_eq!(editor.document.raw_source_text(cx), "select 2;");
        });
        editor.update(cx, |editor, cx| editor.undo_document(cx));
        editor.read_with(cx, |editor, cx| {
            assert!(editor.code_tab_active());
            assert_eq!(editor.document.raw_source_text(cx), "select 1;");
            assert!(
                editor
                    .document
                    .first_root()
                    .unwrap()
                    .read(cx)
                    .kind()
                    .is_code_block()
            );
        });
        editor.update(cx, |editor, cx| editor.redo_document(cx));
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| editor.save_document(window, cx));
        });
        assert_eq!(fs::read_to_string(&plain_path).unwrap(), "select 2;");
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.open_workspace_file(path.clone(), window, cx)
            });
        });
        editor.read_with(cx, |editor, cx| {
            assert_eq!(editor.workspace.open_documents.len(), 2);
            assert_eq!(
                editor.document.raw_source_text(cx),
                "fn main() { println!(\"hello\"); }"
            );
        });
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.open_workspace_file(crlf_path.clone(), window, cx)
            });
        });
        editor.read_with(cx, |editor, cx| {
            assert_eq!(editor.document.raw_source_text(cx), "class A {\n}\n");
        });
        editor.update(cx, |editor, cx| {
            let source = editor.current_document_source(cx);
            let offset = source
                .split_inclusive('\n')
                .take(1)
                .map(str::len)
                .sum::<usize>()
                .min(source.len());
            editor.jump_to_document_search_range(offset..offset, cx);
        });
        editor.read_with(cx, |editor, cx| {
            assert_eq!(
                editor
                    .document
                    .first_root()
                    .unwrap()
                    .read(cx)
                    .selected_range
                    .start,
                "class A {\n".len()
            );
        });
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| editor.save_document(window, cx));
        });
        assert_eq!(
            fs::read_to_string(&crlf_path).unwrap(),
            "class A {\r\n}\r\n"
        );
    }

    #[gpui::test]
    async fn right_click_menu_renders_for_a_workspace_file(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
        });
        let root = std::env::temp_dir().join(format!("velora-menu-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("note.md");
        fs::write(&path, "hello").unwrap();
        cx.on_quit({
            let root = root.clone();
            move || {
                let _ = fs::remove_dir_all(root);
            }
        });
        let (editor, cx) =
            cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
        editor.update(cx, |editor, cx| {
            editor.set_workspace_root(root, cx);
            editor.open_workspace_context_menu(
                point(px(100.0), px(100.0)),
                Some(WorkspaceSelection::File(path)),
                cx,
            );
        });
        cx.update(|window, cx| window.draw(cx).clear());
        editor.read_with(cx, |editor, _| {
            assert!(editor.workspace.context_menu.unwrap().has_target);
        });
    }

    #[gpui::test]
    async fn workspace_search_matches_file_names_and_contents(cx: &mut TestAppContext) {
        let background = cx.executor();
        let root =
            std::env::temp_dir().join(format!("velora-workspace-search-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("src")).expect("create source dir");
        fs::write(
            root.join("README.md"),
            "search term is only in file content",
        )
        .expect("write md");
        fs::write(root.join("src").join("main.rs"), "fn main() {}").expect("write code");
        let tree = scan_workspace_dir(&root, TreeSortPreference::Name).expect("scan tree");

        let matches = search_workspace_files(&tree, &SearchMatcher::new("MAIN", SearchOptions::default()), 200, &background).await;
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].label, "src/main.rs");
        assert_eq!(matches[0].line, None);
        assert_eq!(matches[1].line, Some(1));

        let matches = search_workspace_files(&tree, &SearchMatcher::new("readme", SearchOptions::default()), 200, &background).await;
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].label, "README.md");

        let matches = search_workspace_files(&tree, &SearchMatcher::new("content", SearchOptions::default()), 200, &background).await;
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].label, "README.md");
        assert_eq!(matches[0].line, Some(1));
        assert!(matches[0].preview.contains("content"));
        assert!(search_workspace_files(&tree, &SearchMatcher::new("absent", SearchOptions::default()), 200, &background).await.is_empty());

        let _ = fs::remove_dir_all(root);
    }

    #[gpui::test]
    async fn activity_rail_buttons_all_toggle_the_sidebar(cx: &mut TestAppContext) {
        // 用户要求：文件 / 搜索 / 大纲 三个活动栏按钮都支持「再点一次收起」；
        // 侧边栏收起后整条（含窄条）隐藏，指针贴到窗口左边缘才作为浮层滑出。
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
        });
        let (editor, cx) =
            cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
        cx.update(|window, cx| window.draw(cx).clear());

        // 默认是收起状态：窄条不渲染，正文占满整宽。
        assert!(
            cx.debug_bounds("activity-files").is_none(),
            "收起状态下窄条不应占位"
        );

        for (id, tab) in [
            ("activity-files", super::WorkspaceTab::Files),
            ("activity-search", super::WorkspaceTab::Search),
            ("activity-outline", super::WorkspaceTab::Outline),
        ] {
            // 贴左边缘并停留满 dwell 唤出浮层；滑入动画按真实时间计时
            // （AnimationElement 用 Instant，测试时钟推不动），再等动画播完、
            // 浮层停在 x=0 后按钮位置才可点。
            cx.simulate_mouse_move(
                gpui::point(px(2.0), px(200.0)),
                gpui::MouseButton::Left,
                Modifiers::none(),
            );
            cx.update(|window, cx| window.draw(cx).clear());
            cx.executor().advance_clock(std::time::Duration::from_millis(400));
            cx.run_until_parked();
            cx.update(|window, cx| window.draw(cx).clear());
            std::thread::sleep(std::time::Duration::from_millis(450));
            cx.update(|window, cx| window.draw(cx).clear());
            let bounds = cx
                .debug_bounds(id)
                .unwrap_or_else(|| panic!("贴左边缘后活动栏按钮 {id} 应滑出"));

            // 第一次点：展开并切到这一页。
            cx.simulate_click(bounds.center(), Modifiers::none());
            cx.update(|window, cx| window.draw(cx).clear());
            editor.read_with(cx, |editor, _| {
                assert!(editor.workspace.is_open, "点 {id} 应展开侧边栏");
                assert_eq!(editor.workspace.active_tab, tab, "点 {id} 应切到对应页");
            });
            assert!(
                cx.debug_bounds(id).is_some(),
                "展开后窄条常驻，不该再依赖贴边"
            );

            // 第二次点：收起，整条侧边栏一起隐藏。
            let bounds = cx.debug_bounds(id).expect("展开后按钮应仍可见");
            cx.simulate_click(bounds.center(), Modifiers::none());
            cx.update(|window, cx| window.draw(cx).clear());
            editor.read_with(cx, |editor, _| {
                assert!(!editor.workspace.is_open, "再点 {id} 应收起侧边栏");
            });
            assert!(
                cx.debug_bounds(id).is_none(),
                "收起后窄条应一起隐藏，等指针贴左边缘才滑出"
            );

            // 第三次点：重新展开（同一个按钮能反复切）。同样停留唤出、等滑入
            // 动画播完后再点。
            cx.simulate_mouse_move(
                gpui::point(px(2.0), px(200.0)),
                gpui::MouseButton::Left,
                Modifiers::none(),
            );
            cx.update(|window, cx| window.draw(cx).clear());
            cx.executor().advance_clock(std::time::Duration::from_millis(400));
            cx.run_until_parked();
            cx.update(|window, cx| window.draw(cx).clear());
            std::thread::sleep(std::time::Duration::from_millis(450));
            cx.update(|window, cx| window.draw(cx).clear());
            let bounds = cx
                .debug_bounds(id)
                .unwrap_or_else(|| panic!("第二次贴边后活动栏按钮 {id} 应再次滑出"));
            cx.simulate_click(bounds.center(), Modifiers::none());
            cx.update(|window, cx| window.draw(cx).clear());
            editor.read_with(cx, |editor, _| {
                assert!(editor.workspace.is_open, "第三次点 {id} 应重新展开");
            });

            // 换下一个按钮前回到收起状态，避免上一个按钮的展开状态影响判断。
            editor.update(cx, |editor, _cx| {
                editor.workspace.is_open = false;
                editor.sidebar_peek = false;
                editor.sidebar_overlay_closing = false;
            });
            cx.update(|window, cx| window.draw(cx).clear());
        }
    }

    #[gpui::test]
    async fn collapsed_sidebar_slides_out_only_while_pointer_is_at_left_edge(
        cx: &mut TestAppContext,
    ) {
        // 收起 = 自动隐藏：整条侧边栏不占布局；指针贴左边缘滑出浮层，移开收回。
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
        });
        let (editor, cx) =
            cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
        cx.update(|window, cx| window.draw(cx).clear());

        editor.update(cx, |editor, _| {
            editor.workspace.is_open = false;
        });
        cx.update(|window, cx| window.draw(cx).clear());
        assert!(
            cx.debug_bounds("sidebar-auto-hide-overlay").is_none(),
            "收起状态不该有浮层"
        );
        assert!(
            cx.debug_bounds("activity-files").is_none(),
            "收起状态整条侧边栏（含窄条）不占位"
        );

        // 指针贴到左边缘并停留满 dwell：整条侧边栏（窄条 + 面板）作为浮层出现。
        cx.simulate_mouse_move(
            gpui::point(px(2.0), px(200.0)),
            gpui::MouseButton::Left,
            Modifiers::none(),
        );
        cx.update(|window, cx| window.draw(cx).clear());
        cx.executor().advance_clock(std::time::Duration::from_millis(400));
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear());
        let titlebar_bottom = cx
            .debug_bounds("editor-titlebar")
            .map(|bounds| bounds.origin.y + bounds.size.height)
            .unwrap_or(px(0.0));
        let overlay = cx
            .debug_bounds("sidebar-auto-hide-overlay")
            .expect("贴左边缘应滑出浮层");
        assert!(
            overlay.origin.y >= titlebar_bottom,
            "浮层顶边必须从标题栏下方开始（y = {:?}，标题栏底 = {titlebar_bottom:?}），\
             否则会盖住红绿灯和标签栏",
            overlay.origin.y
        );
        assert!(
            cx.debug_bounds("activity-files").is_some(),
            "浮层里应包含窄条按钮"
        );

        // 指针移到正文：浮层带动画收回。动画期间仍挂载，播完（定时器收尾）才卸载。
        cx.simulate_mouse_move(
            gpui::point(px(700.0), px(200.0)),
            gpui::MouseButton::Left,
            Modifiers::none(),
        );
        // 悬停命中按上一帧的 hitbox 计算，退出事件要下一帧才派发。
        cx.update(|window, cx| window.draw(cx).clear());
        cx.update(|window, cx| window.draw(cx).clear());
        editor.read_with(cx, |editor, _| {
            assert!(!editor.sidebar_peek, "指针移开后贴边状态应结束");
            assert!(
                editor.sidebar_overlay_closing,
                "移开后应先播放收回动画而不是瞬间消失"
            );
        });
        assert!(
            cx.debug_bounds("sidebar-auto-hide-overlay").is_some(),
            "收回动画期间浮层仍在滑出，不该瞬间消失"
        );
        // 动画时长（350ms）走完后，定时器把浮层真正卸载。
        cx.executor().advance_clock(std::time::Duration::from_millis(400));
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear());
        assert!(
            cx.debug_bounds("sidebar-auto-hide-overlay").is_none(),
            "收回动画播完应收起浮层"
        );
    }

    #[gpui::test]
    async fn sidebar_collapse_timer_does_not_cut_a_second_slide_out(
        cx: &mut TestAppContext,
    ) {
        // 快速「贴边 → 移开 → 再贴边 → 再移开」：第一轮收回的定时器到点时，
        // 第二轮收回动画还在播。旧定时器必须因 generation 变化作废，否则会把
        // 第二轮浮层半路砍掉、看起来瞬间消失。
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
        });
        let (editor, cx) =
            cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
        cx.update(|window, cx| window.draw(cx).clear());
        editor.update(cx, |editor, _| {
            editor.workspace.is_open = false;
        });
        cx.update(|window, cx| window.draw(cx).clear());

        // 第一轮：贴边停留唤出，随即移开进入收回动画（定时器在测试时钟
        // T+400+350=750ms 到点）。
        cx.simulate_mouse_move(
            gpui::point(px(2.0), px(200.0)),
            gpui::MouseButton::Left,
            Modifiers::none(),
        );
        cx.update(|window, cx| window.draw(cx).clear());
        cx.executor().advance_clock(std::time::Duration::from_millis(400));
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear());
        cx.simulate_mouse_move(
            gpui::point(px(700.0), px(200.0)),
            gpui::MouseButton::Left,
            Modifiers::none(),
        );
        cx.update(|window, cx| window.draw(cx).clear());
        cx.update(|window, cx| window.draw(cx).clear());
        cx.executor().advance_clock(std::time::Duration::from_millis(100));

        // 第二轮收回。停留判定下真实「再贴边」走不完 dwell 就会被第一轮定时器
        // 赶上，所以像抽屉切换路径那样直写状态置回贴边，再移开触发第二轮
        // （新定时器 T+500+350=850ms 到点）。
        editor.update(cx, |editor, _cx| {
            editor.sidebar_peek = true;
            editor.sidebar_overlay_closing = false;
        });
        cx.update(|window, cx| window.draw(cx).clear());
        cx.simulate_mouse_move(
            gpui::point(px(700.0), px(200.0)),
            gpui::MouseButton::Left,
            Modifiers::none(),
        );
        cx.update(|window, cx| window.draw(cx).clear());
        cx.update(|window, cx| window.draw(cx).clear());

        // 推到 T+800ms：旧定时器（750ms）到点，但 generation 已变，不得动状态；
        // 新定时器（850ms）还没到。
        cx.executor().advance_clock(std::time::Duration::from_millis(300));
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear());
        editor.read_with(cx, |editor, _| {
            assert!(
                editor.sidebar_overlay_closing,
                "旧定时器到点不能终止第二轮收回动画"
            );
        });
        assert!(
            cx.debug_bounds("sidebar-auto-hide-overlay").is_some(),
            "第二轮收回动画应完整播完，不被旧定时器半路砍掉"
        );

        // 推到 T+900ms：新一轮定时器到点，浮层才真正卸载。
        cx.executor().advance_clock(std::time::Duration::from_millis(100));
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear());
        assert!(
            cx.debug_bounds("sidebar-auto-hide-overlay").is_none(),
            "新一轮收回动画播完应收起浮层"
        );
    }

    #[gpui::test]
    async fn sidebar_edge_hover_must_dwell_before_peeking(cx: &mut TestAppContext) {
        // 防误触：贴边必须停留满 dwell 才唤出；扫过左缘不停留不弹，
        // 移开后旧的停留定时器到点也不许再弹。
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
        });
        let (editor, cx) =
            cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
        cx.update(|window, cx| window.draw(cx).clear());
        editor.update(cx, |editor, _| {
            editor.workspace.is_open = false;
        });
        cx.update(|window, cx| window.draw(cx).clear());

        // 贴边但停留不足 dwell（300ms 只推 200ms）：不唤出。
        cx.simulate_mouse_move(
            gpui::point(px(2.0), px(200.0)),
            gpui::MouseButton::Left,
            Modifiers::none(),
        );
        cx.update(|window, cx| window.draw(cx).clear());
        cx.executor().advance_clock(std::time::Duration::from_millis(200));
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear());
        editor.read_with(cx, |editor, _| {
            assert!(!editor.sidebar_peek, "停留不满 dwell 不应唤出浮层");
        });
        assert!(
            cx.debug_bounds("sidebar-auto-hide-overlay").is_none(),
            "停留不满 dwell 不应出现浮层"
        );

        // 移开：挂着的停留定时器被作废，到点也不许再弹。
        cx.simulate_mouse_move(
            gpui::point(px(700.0), px(200.0)),
            gpui::MouseButton::Left,
            Modifiers::none(),
        );
        cx.update(|window, cx| window.draw(cx).clear());
        cx.executor().advance_clock(std::time::Duration::from_millis(400));
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear());
        editor.read_with(cx, |editor, _| {
            assert!(!editor.sidebar_peek, "移开后旧停留定时器不应唤出浮层");
        });
        assert!(
            cx.debug_bounds("sidebar-auto-hide-overlay").is_none(),
            "移开后旧停留定时器不应弹出浮层"
        );
    }

    #[gpui::test]
    async fn workspace_search_cache_picks_up_modified_content(cx: &mut TestAppContext) {
        // 内容缓存：mtime 未变走内存；文件被改写后必须反映新内容（用户报修
        // 的性能优化不能牺牲正确性）。
        let background = cx.executor();
        let root =
            std::env::temp_dir().join(format!("velora-search-cache-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).expect("create dir");
        let note = root.join("note.md");
        fs::write(&note, "alpha only").expect("write");

        let tree = scan_workspace_dir(&root, TreeSortPreference::Name).expect("scan tree");
        let matcher = SearchMatcher::new("alpha", SearchOptions::default());
        assert_eq!(search_workspace_files(&tree, &matcher, 200, &background).await.len(), 1);
        // 第二轮：命中缓存仍能找到。
        assert_eq!(search_workspace_files(&tree, &matcher, 200, &background).await.len(), 1);

        // 改写文件后缓存必须失效。
        fs::write(&note, "beta instead").expect("rewrite");
        // 缓存的失效依据是 mtime，而文件系统的 mtime 粒度可能是一秒甚至更粗：
        // 两次写在几十毫秒内发生时时间戳可能一模一样，缓存就会以为文件没变。
        // 这里显式把 mtime 往后推，让「文件已改」这件事与文件系统粒度无关。
        if let Ok(file) = std::fs::OpenOptions::new().write(true).open(&note) {
            let _ = file.set_modified(
                std::time::SystemTime::now() + std::time::Duration::from_secs(2),
            );
        }
        let tree = scan_workspace_dir(&root, TreeSortPreference::Name).expect("rescan tree");
        let fresh = SearchMatcher::new("beta", SearchOptions::default());
        assert_eq!(
            search_workspace_files(&tree, &fresh, 200, &background).await.len(),
            1,
            "改写后应搜到新内容"
        );
        let stale = SearchMatcher::new("alpha", SearchOptions::default());
        assert!(
            search_workspace_files(&tree, &stale, 200, &background).await.is_empty(),
            "改写后不应再搜到旧内容"
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn document_search_finds_ascii_and_chinese_with_source_ranges() {
        let source = "# Hello\n\n你好 Hello\n";
        let hits = search_document_source(source, &SearchMatcher::new("hello", SearchOptions::default()), Path::new("note.md"), "note.md", 200);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].line, Some(1));
        assert_eq!(hits[1].line, Some(3));
        for hit in &hits {
            let range = hit.source_range.clone().unwrap();
            assert_eq!(&source[range], "Hello");
        }
        let chinese = search_document_source(source, &SearchMatcher::new("你好", SearchOptions::default()), Path::new("note.md"), "note.md", 1);
        assert_eq!(chinese.len(), 1);
        assert_eq!(&source[chinese[0].source_range.clone().unwrap()], "你好");
    }

    #[test]
    fn document_match_navigation_can_search_forward_and_backward() {
        let source = "Alpha 你好 alpha";
        assert_eq!(
            find_document_match_from(source, &plain_matcher("alpha"), 0, false),
            Some(0..5)
        );
        assert_eq!(
            find_document_match_from(source, &plain_matcher("alpha"), 5, false),
            Some(13..18)
        );
        assert_eq!(
            find_document_match_from(source, &plain_matcher("alpha"), source.len(), true),
            Some(13..18)
        );
        assert_eq!(
            find_document_match_from(source, &plain_matcher("你好"), 0, false),
            Some(6..12)
        );
    }

    #[gpui::test]
    async fn document_find_navigates_beyond_sidebar_result_limit(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
        });
        let source = "a ".repeat(250);
        let (editor, cx) = cx.add_window_view(move |_, cx| Editor::from_markdown(cx, source, None));
        editor.update(cx, |editor, cx| {
            editor.open_document_find(cx);
            editor.workspace.search_query = "a".into();
            editor.schedule_workspace_search(cx);
        });
        cx.executor().advance_clock(Duration::from_millis(150));
        cx.run_until_parked();
        editor.update(cx, |editor, cx| {
            assert_eq!(editor.workspace.search_results.len(), 200);
            for _ in 0..201 {
                editor.find_next_document_match(false, cx);
            }
            assert_eq!(editor.workspace.document_active_range, Some(400..401));
            assert_eq!(editor.workspace.search_active_index, None);
            editor.find_next_document_match(true, cx);
            assert_eq!(editor.workspace.document_active_range, Some(398..399));
            assert_eq!(editor.workspace.search_active_index, Some(199));
        });
    }

    #[gpui::test]
    async fn document_find_searches_unsaved_edits_and_selects_matches(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
        });
        let (editor, cx) =
            cx.add_window_view(|_, cx| Editor::from_markdown(cx, "# Alpha\n\nBeta".into(), None));
        editor.update(cx, |editor, cx| {
            let paragraph = editor.document.root_blocks()[1].clone();
            paragraph.update(cx, |paragraph, cx| {
                paragraph.prepare_undo_capture(UndoCaptureKind::CoalescibleText, cx);
                paragraph.replace_text_in_visible_range(4..4, " alpha", None, false, cx);
            });
        });
        cx.run_until_parked();
        editor.update(cx, |editor, cx| {
            editor.open_document_find(cx);
            editor.workspace.search_query = "alpha".into();
            editor.schedule_workspace_search(cx);
        });
        cx.executor().advance_clock(Duration::from_millis(150));
        cx.run_until_parked();
        editor.update(cx, |editor, cx| {
            assert_eq!(editor.workspace.search_results.len(), 2);
            editor.find_next_document_match(false, cx);
            assert_eq!(editor.workspace.search_active_index, Some(0));
            let heading = editor.document.root_blocks()[0].read(cx);
            assert_eq!(heading.selected_range, 0..5);
            editor.find_next_document_match(false, cx);
            assert_eq!(editor.workspace.search_active_index, Some(1));
            let paragraph = editor.document.root_blocks()[1].read(cx);
            assert_eq!(paragraph.selected_range, 5..10);
        });
        editor.update(cx, |editor, cx| {
            let paragraph = editor.document.root_blocks()[1].clone();
            paragraph.update(cx, |paragraph, cx| {
                paragraph.prepare_undo_capture(UndoCaptureKind::CoalescibleText, cx);
                paragraph.replace_text_in_visible_range(5..10, "", None, false, cx);
            });
        });
        cx.executor().advance_clock(Duration::from_millis(150));
        cx.run_until_parked();
        editor.read_with(cx, |editor, _cx| {
            assert_eq!(editor.workspace.search_results.len(), 1);
        });
    }

    #[gpui::test]
    async fn document_find_refreshes_after_switching_tabs(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
        });
        let root = std::env::temp_dir().join(format!(
            "velora-document-find-tabs-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).unwrap();
        let first = root.join("first.md");
        let second = root.join("second.md");
        fs::write(&first, "needle in first").unwrap();
        fs::write(&second, "needle in second").unwrap();
        cx.on_quit({
            let root = root.clone();
            move || {
                let _ = fs::remove_dir_all(root);
            }
        });
        let (editor, cx) =
            cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.set_workspace_root(root, cx);
                editor.open_workspace_file(first.clone(), window, cx);
                editor.open_document_find(cx);
                editor.workspace.search_query = "needle".into();
                editor.schedule_workspace_search(cx);
            });
        });
        cx.executor().advance_clock(Duration::from_millis(150));
        cx.run_until_parked();
        editor.read_with(cx, |editor, _cx| {
            assert_eq!(editor.workspace.search_results.len(), 1);
            assert_eq!(editor.workspace.search_results[0].path, first);
        });
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.open_workspace_file(second.clone(), window, cx);
            });
        });
        cx.executor().advance_clock(Duration::from_millis(150));
        cx.run_until_parked();
        editor.read_with(cx, |editor, _cx| {
            assert_eq!(editor.workspace.search_results.len(), 1);
            assert_eq!(editor.workspace.search_results[0].path, second);
        });
    }

    #[gpui::test]
    async fn cmd_f_opens_current_document_find_in_sidebar(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
            crate::app_menu::init(cx);
        });
        let (editor, cx) = cx.add_window_view(|_window, cx| {
            Editor::from_markdown(cx, "hello\n\nhello".into(), None)
        });
        cx.update(|window, cx| {
            window.activate_window();
            window.draw(cx).clear();
        });
        cx.simulate_keystrokes("cmd-f");
        cx.update(|window, cx| window.draw(cx).clear());
        editor.read_with(cx, |editor, _cx| {
            assert!(editor.workspace.is_open);
            assert!(editor.workspace.active_tab == super::WorkspaceTab::Search);
            assert_eq!(
                editor.workspace.search_scope,
                super::WorkspaceSearchScope::Document
            );
        });
        cx.simulate_input("hello");
        cx.executor().advance_clock(Duration::from_millis(150));
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear());
        editor.read_with(cx, |editor, _cx| {
            assert_eq!(editor.workspace.search_results.len(), 2);
        });
        cx.simulate_keystrokes("enter");
        cx.update(|window, cx| window.draw(cx).clear());
        editor.read_with(cx, |editor, _cx| {
            assert_eq!(editor.workspace.search_active_index, Some(0));
        });
        cx.simulate_keystrokes("cmd-g");
        editor.read_with(cx, |editor, _cx| {
            assert_eq!(editor.workspace.search_active_index, Some(1));
        });
        cx.simulate_keystrokes("cmd-shift-g");
        editor.read_with(cx, |editor, _cx| {
            assert_eq!(editor.workspace.search_active_index, Some(0));
        });
    }

    #[gpui::test]
    async fn workspace_search_returns_content_hits_after_typing(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
        });
        let root =
            std::env::temp_dir().join(format!("velora-content-search-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("notes.md"), "first line\n独特的内容在这里\n").unwrap();
        cx.on_quit({
            let root = root.clone();
            move || {
                let _ = fs::remove_dir_all(root);
            }
        });
        let (editor, cx) =
            cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
        editor.update(cx, |editor, cx| {
            editor.set_workspace_root(root, cx);
            editor.workspace.active_tab = super::WorkspaceTab::Search;
            editor.workspace.search_query = "独特".into();
            editor.schedule_workspace_search(cx);
        });
        cx.executor().advance_clock(Duration::from_millis(150));
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear());
        editor.read_with(cx, |editor, _| {
            assert_eq!(editor.workspace.search_results.len(), 1);
            assert_eq!(editor.workspace.search_results[0].line, Some(2));
            assert_eq!(editor.workspace.search_results[0].label, "notes.md");
        });
    }

    #[gpui::test]
    async fn search_result_file_header_opens_the_file_and_has_no_empty_row(cx: &mut TestAppContext) {
        // 用户报修：搜索结果里文件名本身点不了，只有它下面一条没有内容的空行能点。
        // 原因是文件名命中（line=None）也渲染了一行（py(4) 且无内容），而文件头
        // 完全没有点击处理。现在文件头整行可点并代表该组第一条命中。
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
        });
        let root = std::env::temp_dir().join(format!(
            "velora-search-header-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(root.join("assets")).unwrap();
        // 含 NUL 才能稳定判成不可预览文件（用户截图里的 png 场景）。
        fs::write(root.join("assets").join("velora-banner.png"), [0u8, 1, 2, 3]).unwrap();
        fs::write(root.join("velora-notes.md"), "开头\nvelora 命中行\n").unwrap();
        cx.on_quit({
            let root = root.clone();
            move || {
                let _ = fs::remove_dir_all(root);
            }
        });

        let (editor, cx) =
            cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
        editor.update(cx, |editor, cx| {
            editor.set_workspace_root(root.clone(), cx);
            editor.workspace.is_open = true;
            editor.workspace.active_tab = super::WorkspaceTab::Search;
            editor.workspace.search_query = "velora".into();
            editor.schedule_workspace_search(cx);
        });
        cx.executor().advance_clock(Duration::from_millis(150));
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear());

        let hits = editor.read_with(cx, |editor, _| {
            editor
                .workspace
                .search_results
                .iter()
                .map(|hit| (hit.label.clone(), hit.line))
                .collect::<Vec<_>>()
        });
        assert_eq!(
            hits,
            vec![
                ("assets/velora-banner.png".to_string(), None),
                ("velora-notes.md".to_string(), None),
                ("velora-notes.md".to_string(), Some(2)),
            ],
            "目录在前，文件名命中在前，内容命中带行号"
        );

        // 文件名命中不再有独立空行；内容命中仍然渲染自己的行。
        assert!(
            cx.debug_bounds("workspace-search-hit-0").is_none(),
            "文件名命中不应再渲染一条看不见的空行"
        );
        assert!(
            cx.debug_bounds("workspace-search-hit-1").is_none(),
            "文件名命中不应再渲染一条看不见的空行"
        );
        assert!(cx.debug_bounds("workspace-search-hit-2").is_some());

        let header = cx
            .debug_bounds("workspace-search-file-0")
            .expect("首个文件头应渲染为可点击行");
        assert!(
            header.size.height > px(16.0),
            "点击区应覆盖整行文件头，实测高度 {:?}",
            header.size.height
        );
        assert!(header.size.width > px(60.0));

        cx.simulate_click(header.center(), Modifiers::none());
        cx.update(|window, cx| window.draw(cx).clear());
        // macOS 的 /var 会被打开流程规范化为 /private/var，断言前统一 canonicalize。
        let banner = fs::canonicalize(root.join("assets").join("velora-banner.png"))
            .expect("canonical banner path");
        editor.read_with(cx, |editor, _| {
            assert_eq!(
                editor
                    .unsupported_preview_path
                    .as_ref()
                    .and_then(|path| fs::canonicalize(path).ok()),
                Some(banner.clone()),
                "点文件名应打开该文件（png 走不可预览占位）"
            );
        });

        // 同一组里既有文件名命中又有内容命中时，文件头也负责打开文件。
        let notes_header = cx
            .debug_bounds("workspace-search-file-1")
            .expect("第二个文件头应渲染");
        cx.simulate_click(notes_header.center(), Modifiers::none());
        cx.update(|window, cx| window.draw(cx).clear());
        let notes = fs::canonicalize(root.join("velora-notes.md")).expect("canonical notes path");
        editor.read_with(cx, |editor, _| {
            assert_eq!(
                editor
                    .file_path
                    .as_ref()
                    .and_then(|path| fs::canonicalize(path).ok()),
                Some(notes.clone()),
                "点文件头应打开对应的 Markdown 文件"
            );
            assert!(editor.unsupported_preview_path.is_none());
        });
    }

    #[gpui::test]
    async fn switching_workspace_drops_the_previous_workspaces_tabs(cx: &mut TestAppContext) {
        // 用户报修：切换工作区之后，顶栏还留着上一个工作区的标签。
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
        });
        let root_a = std::env::temp_dir().join(format!("velora-switch-a-{}", uuid::Uuid::new_v4()));
        let root_b = std::env::temp_dir().join(format!("velora-switch-b-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root_a).unwrap();
        fs::create_dir_all(&root_b).unwrap();
        let alpha = root_a.join("alpha.md");
        let beta = root_a.join("beta.md");
        fs::write(&alpha, "# alpha\n").unwrap();
        fs::write(&beta, "# beta\n").unwrap();
        let gamma = root_b.join("gamma.md");
        fs::write(&gamma, "# gamma\n").unwrap();
        cx.on_quit({
            let (root_a, root_b) = (root_a.clone(), root_b.clone());
            move || {
                let _ = fs::remove_dir_all(root_a);
                let _ = fs::remove_dir_all(root_b);
            }
        });

        let (editor, cx) =
            cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.set_workspace_root(root_a.clone(), cx);
            });
            editor.update(cx, |editor, cx| {
                editor.open_workspace_file(alpha.clone(), window, cx);
                editor.open_workspace_file(beta.clone(), window, cx);
            });
        });
        editor.read_with(cx, |editor, _| {
            assert_eq!(editor.workspace.open_documents.len(), 2, "切换前应有两个标签");
        });

        cx.update(|_window, cx| {
            editor.update(cx, |editor, cx| {
                editor.set_workspace_root(root_b.clone(), cx);
            });
        });
        editor.read_with(cx, |editor, _| {
            assert!(
                editor.workspace.open_documents.is_empty(),
                "上一个工作区的标签必须全部收起，实测 {:?}",
                editor.workspace.open_documents.iter().map(|tab| tab.path.clone()).collect::<Vec<_>>()
            );
            assert!(editor.workspace.active_document.is_none());
            assert!(editor.show_welcome, "没有可留的标签时应回到欢迎页");
        });
    }

    #[gpui::test]
    async fn single_click_previews_and_double_click_pins_tabs(cx: &mut TestAppContext) {
        // 用户需求：单击打开为预览标签——切换到其它文件时未修改的预览标签被
        // 替换、不再占据标签栏；双击打开固定常驻；已修改的预览不会被替换。
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
        });
        let root = std::env::temp_dir().join(format!("velora-preview-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let paths: Vec<std::path::PathBuf> = ["a", "b", "c", "d", "e"]
            .iter()
            .map(|name| {
                let path = root.join(format!("{name}.md"));
                fs::write(&path, format!("# {name}\n")).unwrap();
                path
            })
            .collect();
        cx.on_quit({
            let root = root.clone();
            move || {
                let _ = fs::remove_dir_all(root);
            }
        });
        let preview = super::WorkspaceOpenMode::Preview;
        let pinned = super::WorkspaceOpenMode::Pinned;

        let (editor, cx) =
            cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.set_workspace_root(root.clone(), cx);
            });
            editor.update(cx, |editor, cx| {
                editor.open_workspace_file_in_mode(paths[0].clone(), preview, window, cx);
            });
        });
        editor.read_with(cx, |editor, _| {
            let tabs = &editor.workspace.open_documents;
            assert_eq!(tabs.len(), 1);
            assert!(tabs[0].preview, "单击打开的应是预览标签");
        });

        // 单击另一个文件：旧的未修改预览标签被替换。
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.open_workspace_file_in_mode(paths[1].clone(), preview, window, cx);
            });
        });
        editor.read_with(cx, |editor, _| {
            let tabs = &editor.workspace.open_documents;
            assert_eq!(tabs.len(), 1, "切走后未修改的预览标签应被替换");
            assert_eq!(tabs[0].path, paths[1]);
        });

        // 双击打开：固定，之后切走不再被替换。
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.open_workspace_file_in_mode(paths[0].clone(), pinned, window, cx);
            });
        });
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.open_workspace_file_in_mode(paths[2].clone(), preview, window, cx);
            });
        });
        editor.read_with(cx, |editor, _| {
            let tabs = &editor.workspace.open_documents;
            assert_eq!(tabs.len(), 2, "固定标签保留，预览标签只有当前一个");
            assert!(
                tabs.iter().any(|tab| tab.path == paths[0] && !tab.preview),
                "双击打开的标签应为固定"
            );
        });

        // 已修改的预览标签切走后保留。
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.open_workspace_file_in_mode(paths[3].clone(), preview, window, cx);
                if let Some(tab) = editor
                    .workspace
                    .open_documents
                    .iter_mut()
                    .find(|tab| tab.path == paths[3])
                {
                    tab.dirty = true;
                }
            });
            editor.update(cx, |editor, cx| {
                editor.open_workspace_file_in_mode(paths[4].clone(), preview, window, cx);
            });
        });
        editor.read_with(cx, |editor, _| {
            let tabs = &editor.workspace.open_documents;
            assert!(
                tabs.iter().any(|tab| tab.path == paths[3]),
                "已修改的预览标签切走后应保留"
            );
            assert_eq!(tabs.len(), 3);
        });
    }

    #[gpui::test]
    async fn switching_workspace_keeps_inner_tabs_and_reopens_one(cx: &mut TestAppContext) {
        // 新根目录是旧根的子目录：子目录里的标签要留下，且活动标签被收起时
        // 下一帧补开剩下的那个。
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
        });
        let root = std::env::temp_dir().join(format!("velora-switch-inner-{}", uuid::Uuid::new_v4()));
        let inner = root.join("sub");
        fs::create_dir_all(&inner).unwrap();
        let outer = root.join("outer.md");
        let inside = inner.join("inside.md");
        fs::write(&outer, "# outer\n").unwrap();
        fs::write(&inside, "# inside\n").unwrap();
        cx.on_quit({
            let root = root.clone();
            move || {
                let _ = fs::remove_dir_all(root);
            }
        });

        let (editor, cx) =
            cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.set_workspace_root(root.clone(), cx);
            });
            editor.update(cx, |editor, cx| {
                editor.open_workspace_file(inside.clone(), window, cx);
                editor.open_workspace_file(outer.clone(), window, cx);
            });
        });
        cx.update(|_window, cx| {
            editor.update(cx, |editor, cx| {
                editor.set_workspace_root(inner.clone(), cx);
            });
        });
        editor.read_with(cx, |editor, _| {
            let kept = editor
                .workspace
                .open_documents
                .iter()
                .map(|tab| tab.path.clone())
                .collect::<Vec<_>>();
            assert_eq!(kept, vec![inside.clone()], "只应留下新根目录内的标签");
        });
        cx.update(|window, cx| window.draw(cx).clear());
        editor.read_with(cx, |editor, _| {
            // 活动标签被收起后，补开剩下的那个（延后一帧，那时才拿得到 Window）。
            assert_eq!(
                editor.file_path.as_deref(),
                Some(inside.as_path()),
                "应补开新根目录内剩下的标签，file_path={:?} active={:?}",
                editor.file_path,
                editor.workspace.active_document
            );
            assert!(editor.pending_workspace_tab_activation.is_none());
            assert!(!editor.show_welcome, "还有标签时不该回到欢迎页");
        });
    }

    #[gpui::test]
    async fn switching_workspace_saves_dirty_stale_tabs_instead_of_losing_them(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
        });
        let root_a = std::env::temp_dir().join(format!("velora-switch-dirty-a-{}", uuid::Uuid::new_v4()));
        let root_b = std::env::temp_dir().join(format!("velora-switch-dirty-b-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root_a).unwrap();
        fs::create_dir_all(&root_b).unwrap();
        let doc = root_a.join("notes.md");
        fs::write(&doc, "# 原文\n").unwrap();
        cx.on_quit({
            let (root_a, root_b) = (root_a.clone(), root_b.clone());
            move || {
                let _ = fs::remove_dir_all(root_a);
                let _ = fs::remove_dir_all(root_b);
            }
        });

        let (editor, cx) =
            cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.set_workspace_root(root_a.clone(), cx);
            });
            editor.update(cx, |editor, cx| {
                editor.open_workspace_file(doc.clone(), window, cx);
            });
        });
        // 造一个未保存的脏标签。
        editor.update(cx, |editor, _cx| {
            editor.workspace.open_documents[0].dirty = true;
            editor.workspace.open_documents[0].markdown = "# 未保存的修改\n".into();
        });
        cx.update(|_window, cx| {
            editor.update(cx, |editor, cx| {
                editor.set_workspace_root(root_b.clone(), cx);
            });
        });
        editor.read_with(cx, |editor, _| {
            assert!(editor.workspace.open_documents.is_empty(), "脏标签也应被收起");
        });
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            "# 未保存的修改\n",
            "切换工作区不能吞掉未保存的内容"
        );
    }

    #[gpui::test]
    async fn clicking_a_file_keeps_the_tree_scroll_offset(cx: &mut TestAppContext) {
        // 用户报修：长文件树滚到下面后点一个文件，树会刷新并自动置顶。
        // 根因是打开文件时把已扫描的树清成 None，重扫落地前的那一帧侧栏只剩
        // 「…」（内容高度 ≈30px），gpui 的 div 会把记住的偏移按新的 scroll_max
        // 夹到 0 并写回，重扫完成后偏移已经没了。
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
        });
        let root = std::env::temp_dir().join(format!(
            "velora-tree-scroll-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).unwrap();
        for index in 0..80 {
            fs::write(
                root.join(format!("note-{index:02}.md")),
                format!("# note {index}\n"),
            )
            .unwrap();
        }
        cx.on_quit({
            let root = root.clone();
            move || {
                let _ = fs::remove_dir_all(root);
            }
        });

        let (editor, cx) =
            cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
        editor.update(cx, |editor, cx| {
            editor.set_workspace_root(root.clone(), cx);
            editor.workspace.is_open = true;
        });
        cx.run_until_parked();
        // 压矮窗口，让 80 行的树必须滚动才有下方内容。
        cx.update(|window, _cx| window.resize(gpui::size(px(320.0), px(260.0))));
        cx.update(|window, cx| window.draw(cx).clear());

        cx.simulate_event(ScrollWheelEvent {
            position: point(px(60.0), px(200.0)),
            delta: ScrollDelta::Pixels(point(px(0.0), px(-600.0))),
            modifiers: Modifiers::default(),
            touch_phase: TouchPhase::default(),
        });
        cx.update(|window, cx| window.draw(cx).clear());
        let scrolled =
            editor.read_with(cx, |editor, _| editor.workspace.tree_scroll_handle.offset().y);
        assert!(
            scrolled < px(0.0),
            "滚轮应把长树滚下去（gpui 的偏移向下为负），实测 {scrolled:?}"
        );

        let target = root.join("note-70.md");
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.open_workspace_file(target.clone(), window, cx);
            });
        });
        // 重扫落地前的那一帧：树不能被丢掉。
        cx.update(|window, cx| window.draw(cx).clear());
        editor.read_with(cx, |editor, _| {
            assert!(
                editor.workspace.file_tree.is_some(),
                "同一根目录下点开文件不应丢掉已扫描的树"
            );
        });
        let after_click =
            editor.read_with(cx, |editor, _| editor.workspace.tree_scroll_handle.offset().y);
        assert_eq!(after_click, scrolled, "点文件那一刻滚动位置不应被重置");

        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear());
        let settled =
            editor.read_with(cx, |editor, _| editor.workspace.tree_scroll_handle.offset().y);
        assert_eq!(settled, scrolled, "重扫落地后滚动位置仍应保持");
    }

    #[gpui::test]
    async fn re_search_keeps_previous_results_visible(cx: &mut TestAppContext) {
        // 用户报修：点击搜索结果后侧栏闪一下——先空白再恢复。触发重新搜索的来源
        // 很多（watcher 刷新文件树、重新调度等），但闪空的根因是重新搜索一开始就
        // 清空 `search_results`，面板在 120ms 去抖窗口里只剩「…」。
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
        });
        let root = std::env::temp_dir().join(format!(
            "velora-search-keep-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(root.join("assets")).unwrap();
        fs::write(root.join("assets").join("velora-banner.png"), [0u8, 1, 2, 3]).unwrap();
        fs::write(root.join("velora-notes.md"), "开头\nvelora 命中行\n").unwrap();
        cx.on_quit({
            let root = root.clone();
            move || {
                let _ = fs::remove_dir_all(root);
            }
        });

        let (editor, cx) =
            cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
        editor.update(cx, |editor, cx| {
            editor.set_workspace_root(root.clone(), cx);
            editor.workspace.is_open = true;
            editor.workspace.active_tab = super::WorkspaceTab::Search;
            editor.workspace.search_query = "velora".into();
            editor.schedule_workspace_search(cx);
        });
        cx.executor().advance_clock(Duration::from_millis(150));
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear());
        let baseline = editor.read_with(cx, |editor, _| editor.workspace.search_results.len());
        assert_eq!(baseline, 3);

        // 模拟「打开文件后 watcher 触发文件树刷新」：这会重新调度搜索。
        editor.update(cx, |editor, cx| editor.refresh_workspace_tree(cx));
        editor.read_with(cx, |editor, _| {
            assert!(editor.workspace.search_pending, "重新搜索应处于进行中");
            assert_eq!(
                editor.workspace.search_results.len(),
                baseline,
                "重新搜索期间应继续显示旧结果，而不是先清空"
            );
        });
        cx.update(|window, cx| window.draw(cx).clear());
        assert!(
            cx.debug_bounds("workspace-search-file-0").is_some(),
            "重新搜索期间侧栏不应闪成空白"
        );
        assert!(cx.debug_bounds("workspace-search-hit-2").is_some());

        cx.executor().advance_clock(Duration::from_millis(200));
        cx.run_until_parked();
        editor.read_with(cx, |editor, _| {
            assert!(!editor.workspace.search_pending);
            assert_eq!(editor.workspace.search_results.len(), baseline);
        });

        // 查询被清空时必须立刻丢掉旧结果（面板回到空态），不能留着过期结果。
        editor.update(cx, |editor, cx| {
            editor.workspace.search_query.clear();
            editor.schedule_workspace_search(cx);
        });
        editor.read_with(cx, |editor, _| {
            assert!(editor.workspace.search_results.is_empty());
            assert!(!editor.workspace.search_pending);
        });
    }

    #[test]
    fn workspace_create_operations_do_not_overwrite_existing_files() {
        let root =
            std::env::temp_dir().join(format!("velora-workspace-create-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).expect("create root");
        let file = root.join("notes.md");
        fs::write(&file, "keep this").expect("write existing file");

        assert!(create_workspace_file(&file).is_err());
        assert_eq!(
            fs::read_to_string(&file).expect("read existing file"),
            "keep this"
        );

        let new_file = root.join("new.md");
        create_workspace_file(&new_file).expect("create markdown file");
        assert_eq!(fs::read_to_string(&new_file).expect("read new file"), "");

        let folder = root.join("nested");
        create_workspace_folder(&folder).expect("create folder");
        assert!(folder.is_dir());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn moving_a_folder_remaps_open_document_descendants() {
        let source = Path::new("/workspace/old");
        let destination = Path::new("/workspace/new");
        assert_eq!(
            remap_moved_path(
                Path::new("/workspace/old/docs/readme.md"),
                source,
                destination,
                true,
            ),
            Some(PathBuf::from("/workspace/new/docs/readme.md"))
        );
        assert_eq!(
            remap_moved_path(
                Path::new("/workspace/other/readme.md"),
                source,
                destination,
                true,
            ),
            None
        );
        assert_eq!(
            remap_moved_path(
                Path::new("/workspace/old.md"),
                Path::new("/workspace/old.md"),
                Path::new("/workspace/new.md"),
                false,
            ),
            Some(PathBuf::from("/workspace/new.md"))
        );
    }

    #[test]
    fn moving_a_markdown_file_rewrites_relative_inline_image_paths() {
        let markdown = "![diagram](./assets/diagram.png \"Diagram\")\n\n![online](https://example.com/image.png)";
        assert_eq!(
            rewrite_relative_image_targets(
                markdown,
                Path::new("/workspace/docs"),
                Path::new("/workspace/notes"),
            ),
            "![diagram](../docs/assets/diagram.png \"Diagram\")\n\n![online](https://example.com/image.png)"
        );
    }

    #[test]
    fn moving_a_markdown_file_rewrites_reference_image_definitions() {
        let markdown = "![cover][hero]\n\n[hero]: ./assets/cover.png \"Cover\"";
        assert_eq!(
            rewrite_relative_image_targets(
                markdown,
                Path::new("/workspace/docs"),
                Path::new("/workspace/notes"),
            ),
            "![cover][hero]\n\n[hero]: ../docs/assets/cover.png \"Cover\""
        );
    }

    #[test]
    fn deleting_a_folder_matches_only_its_descendants() {
        let folder = Path::new("/workspace/docs");
        assert!(path_is_affected(
            Path::new("/workspace/docs/readme.md"),
            folder,
            true,
        ));
        assert!(!path_is_affected(
            Path::new("/workspace/docs-old/readme.md"),
            folder,
            true,
        ));
        assert!(path_is_affected(
            Path::new("/workspace/readme.md"),
            Path::new("/workspace/readme.md"),
            false,
        ));
    }

    #[test]
    fn outline_tree_skips_headings_inside_fenced_code() {
        let outline = build_outline_tree(
            "# Root\n\n```md\n# ignored\n```\n\n## Child\n\n### Grandchild\n\n# Next",
        );

        assert_eq!(outline.len(), 2);
        assert_eq!(outline[0].label, "Root");
        assert_eq!(outline[0].children[0].label, "Child");
        assert_eq!(outline[0].children[0].children[0].label, "Grandchild");
        assert_eq!(outline[1].label, "Next");
    }

    #[gpui::test]
    async fn outline_tracks_committed_heading_edits(cx: &mut TestAppContext) {
        let editor = cx.new(|cx| Editor::from_markdown(cx, "# Old".into(), None));
        editor.update(cx, |editor, cx| {
            editor.sync_workspace_outline(cx);
            assert_eq!(editor.workspace.outline_tree[0].label, "Old");
            let heading = editor.document.first_root().unwrap().clone();
            heading.update(cx, |heading, cx| {
                heading.prepare_undo_capture(UndoCaptureKind::CoalescibleText, cx);
                heading.replace_text_in_visible_range(0..3, "New", None, false, cx);
            });
        });
        cx.run_until_parked();
        editor.update(cx, |editor, cx| {
            editor.sync_workspace_outline(cx);
            assert_eq!(editor.workspace.outline_tree[0].label, "New");
        });
    }

    #[test]
    fn outline_expansion_state_is_not_auto_populated_and_prunes_stale_ids() {
        let outline = build_outline_tree("# Root\n\n## Child\n\n# Next");
        let mut fresh = WorkspaceState::default();
        prune_outline_state(&mut fresh, &outline);
        assert!(fresh.expanded.is_empty());

        let mut existing = WorkspaceState::default();
        existing.expanded.insert("outline:0".to_string());
        existing.expanded.insert("outline:999".to_string());
        existing
            .expanded
            .insert("workspace-dir:C:/docs".to_string());
        existing.selected = Some(WorkspaceSelection::Outline("outline:999".to_string()));

        prune_outline_state(&mut existing, &outline);

        assert!(existing.expanded.contains("outline:0"));
        assert!(existing.expanded.contains("workspace-dir:C:/docs"));
        assert!(!existing.expanded.contains("outline:999"));
        assert_eq!(existing.selected, None);
    }

    #[test]
    fn workspace_panel_width_stays_within_drag_bounds() {
        assert_eq!(clamp_workspace_panel_width(100.0, 1080.0), 180.0);
        assert_eq!(clamp_workspace_panel_width(320.0, 1080.0), 320.0);
        assert_eq!(clamp_workspace_panel_width(500.0, 720.0), 400.0);
    }
}


