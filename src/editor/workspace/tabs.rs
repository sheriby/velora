use super::*;

/// 「这一篇的外部改动读不出来」那条提示的开头。侧栏那条红字是全工作区共用的，
/// 只有以它开头的那一条由重载流程自己收回——别人写的（保存失败、打不开文件）
/// 不能被一次成功的重载顺手清掉。
const UNREADABLE_EXTERNAL_CHANGE: &str = "读不出外部改动";

/// 读不出来时补复查的间隔：写入方往往还在写（半截的 UTF-16、临时文件还没改名），
/// 太急第二次照样读坏；而很多平台这一次事件就是最后一次（notify 合并、FSEvents
/// 去抖），不补这一次界面就永远停在旧内容上。
const EXTERNAL_CHANGE_RECHECK_DELAY: std::time::Duration = std::time::Duration::from_millis(400);

impl Editor {
    pub(crate) fn on_new_document(
        &mut self,
        _: &crate::components::NewDocument,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        self.new_workspace_document(window, cx);
    }

    pub(crate) fn active_document_path(&self) -> Option<&PathBuf> {
        self.file_path.as_ref().or_else(|| {
            self.workspace
                .active_document
                .as_ref()
                .filter(|path| untitled_document_id(path).is_some())
        })
    }

    pub(crate) fn finish_pending_tab_close(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let pending = std::mem::take(&mut self.pending_tab_close_after_save);
        if pending.is_empty() {
            return false;
        }
        let closing = self
            .workspace
            .open_documents
            .iter()
            .filter(|tab| pending.contains(&tab.recovery_id))
            .cloned()
            .collect::<Vec<_>>();
        self.finish_close_workspace_tabs(&closing, true, window, cx);
        true
    }

    pub(crate) fn restore_untitled_tabs(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) {
        if !paths
            .iter()
            .any(|path| untitled_document_id(path).is_some())
        {
            return;
        }
        let snapshots = match crate::config::read_recovery_snapshots() {
            Ok(snapshots) => snapshots,
            Err(error) => {
                let title = cx
                    .global::<I18nManager>()
                    .strings()
                    .open_failed_title
                    .clone();
                self.show_message_modal(title, error.to_string(), cx);
                return;
            }
        };
        for path in paths {
            let Some(id) = untitled_document_id(path) else {
                continue;
            };
            if self
                .workspace
                .open_documents
                .iter()
                .any(|tab| tab.path == *path)
            {
                continue;
            }
            let snapshot = snapshots.iter().find(|snapshot| snapshot.id == id);
            self.workspace.open_documents.push(WorkspaceDocumentTab {
                path: path.clone(),
                recovery_id: id,
                file_version: 0,
                markdown: snapshot
                    .map(|snapshot| snapshot.markdown.clone())
                    .unwrap_or_default(),
                file_shape: None,
                dirty: snapshot.is_some(),
                preview: false,
                view: None,
            });
        }
    }

    pub(crate) fn restore_workspace_tab_order(
        &mut self,
        paths: &[PathBuf],
        cx: &mut Context<Self>,
    ) {
        self.workspace.open_documents.sort_by_cached_key(|tab| {
            paths
                .iter()
                .position(|path| path == &tab.path)
                .unwrap_or(usize::MAX)
        });
        self.persist_session(cx);
        cx.notify();
    }

    pub(crate) fn has_recovery_document(&self, id: uuid::Uuid, cx: &App) -> bool {
        self.workspace
            .open_documents
            .iter()
            .any(|tab| tab.recovery_id == id)
            || self.split_groups().iter().any(|group| {
                group
                    .read(cx)
                    .workspace
                    .open_documents
                    .iter()
                    .any(|tab| tab.recovery_id == id)
            })
    }

    pub(crate) fn register_untitled_document(&mut self, cx: &mut Context<Self>) -> PathBuf {
        if let Some(path) = self
            .workspace
            .active_document
            .as_ref()
            .filter(|path| untitled_document_id(path).is_some())
        {
            return path.clone();
        }
        let number = self
            .workspace
            .open_documents
            .iter()
            .filter(|tab| untitled_document_id(&tab.path).is_some())
            .filter_map(|tab| {
                tab.path
                    .file_name()?
                    .to_str()?
                    .strip_prefix("Untitled-")?
                    .parse::<usize>()
                    .ok()
            })
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        let path = untitled_document_path(self.recovery_id, number);
        let markdown = self.buffer.text();
        self.workspace.open_documents.push(WorkspaceDocumentTab {
            path: path.clone(),
            recovery_id: self.recovery_id,
            file_version: 0,
            markdown: markdown.clone(),
            file_shape: self.buffer.file_shape(),
            dirty: self.document_dirty,
            preview: false,
            view: Some(self.capture_document_view(&markdown, cx)),
        });
        self.workspace.active_document = Some(path.clone());
        path
    }

    pub(crate) fn new_workspace_document(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(group) = self.active_split_editor() {
            let handle = window.window_handle();
            cx.defer(move |cx| {
                if let Err(error) = handle.update(cx, |_, window, cx| {
                    group.update(cx, |editor, cx| editor.new_workspace_document(window, cx))
                }) {
                    eprintln!("在活动分组新建文档失败：{error}");
                }
            });
            return;
        }
        if self.file_path.is_none()
            && self.workspace.active_document.is_none()
            && self.document_dirty
        {
            self.register_untitled_document(cx);
        }
        self.finalize_pending_undo_capture(cx);
        self.snapshot_current_document(cx);
        self.workspace.active_document = None;
        self.replace_document_from_markdown(String::new(), None, cx);
        self.recovery_id = uuid::Uuid::new_v4();
        self.recovery_source_path = None;
        self.is_recovered_document = false;
        self.document_dirty = false;
        self.show_welcome = false;
        self.unsupported_preview_path = None;
        self.unsupported_preview_detail = None;
        self.register_untitled_document(cx);
        self.schedule_autosave(cx);
        self.pending_focus = self
            .active_entity_id
            .or_else(|| self.first_focusable_entity_id(cx));
        self.persist_session(cx);
        cx.notify();
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
    pub(crate) fn reload_externally_changed_document(
        &mut self,
        path: &Path,
        cx: &mut Context<Self>,
    ) {
        if crate::components::Block::has_supported_image_extension(path) {
            // 正文 img(path) 与独立预览使用不同的加载器，缓存必须分别失效。
            gpui::ImageSource::from(path).remove_asset(cx);
            cx.remove_asset::<ImageAssetLoader>(&Resource::Path(path.to_path_buf().into()));
            if self
                .image_preview
                .as_ref()
                .is_some_and(|preview| preview.path == path)
            {
                self.load_image_file_preview(path.to_path_buf(), cx);
            }
            cx.notify();
            return;
        }
        self.reload_externally_changed_document_with_recheck(path, true, cx);
    }

    /// 重载这一篇；`may_recheck` 说这次要不要在读不出来时补一次复查。
    ///
    /// 复查只补一次（第二趟再失败就只留那条提示，不再排第三个计时），否则会跟着
    /// 监听事件滚成一串定时器。
    fn reload_externally_changed_document_with_recheck(
        &mut self,
        path: &Path,
        may_recheck: bool,
        cx: &mut Context<Self>,
    ) {
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
        let document = match crate::editor::encoding::load_document(path) {
            Ok(document) => document,
            Err(error) => {
                // 以前这里是 `let Ok(..) else { return }`：静默吞掉一次读盘失败。
                // 那是「文档在外部修改后没有刷新」的一条真路径——写入方还没写完
                // （半截的 UTF-16、读到 NUL 的字节流）、文件被同步盘换成占位符、
                // 句柄还被上一个写者独占，而很多平台这一次事件就是最后一次，
                // 于是界面永远停在旧内容上，用户连一句为什么都拿不到。
                // 现在：先说清这一趟为什么没刷，再补一次短延时的复查。
                self.note_unreadable_external_change(path, &error, cx);
                if may_recheck {
                    self.schedule_external_change_recheck(path.to_path_buf(), cx);
                }
                return;
            }
        };
        let disk = &document.text;
        // 标签缓存与缓冲区一样存 LF 文本，磁盘上的 CRLF 不是「外部改动」：按规范化
        // 后的版本号比，否则每次监听事件都会把干净文件当成被改了，重新导入一遍。
        let disk_version = crate::editor::persistence::file_content_version(disk);
        if disk_version == crate::editor::persistence::file_content_version(&cached_markdown) {
            self.clear_unreadable_external_change();
            return;
        }
        self.apply_disk_reload(path, document, disk_version, cx);
        self.clear_unreadable_external_change();
        cx.notify();
    }

    /// 读不出外部改动的那条提示（应用内侧栏红字，不用系统原生弹窗）。
    fn note_unreadable_external_change(
        &mut self,
        path: &Path,
        error: &std::io::Error,
        cx: &mut Context<Self>,
    ) {
        self.workspace.file_error = Some(format!(
            "{UNREADABLE_EXTERNAL_CHANGE}「{}」：{error}，界面仍是上一版内容。",
            path.display()
        ));
        cx.notify();
    }

    /// 内容终于读出来了：收回那条提示。只认它自己写的那一句——别处失败
    /// （保存不了、打不开文件）的红字不该被一次成功的重载顺手清掉。
    fn clear_unreadable_external_change(&mut self) {
        if self
            .workspace
            .file_error
            .as_deref()
            .is_some_and(|error| error.starts_with(UNREADABLE_EXTERNAL_CHANGE))
        {
            self.workspace.file_error = None;
        }
    }

    /// 补一次复查：写入方往往还在写，那一瞬间的读失败多半是自己会好的；不补就只能
    /// 停在旧内容上（很多平台的监听事件会合并，之后不再有第二次机会）。
    fn schedule_external_change_recheck(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let editor = cx.entity().downgrade();
        cx.spawn(async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
            cx.background_executor()
                .timer(EXTERNAL_CHANGE_RECHECK_DELAY)
                .await;
            // 更新失败只剩一种可能：这个 Editor 实体已随窗口销毁，没有界面要刷新了。
            if let Err(error) = editor.update(cx, |editor, cx| {
                editor.reload_externally_changed_document_with_recheck(&path, false, cx);
            }) {
                eprintln!("failed to recheck the external change: {error}");
            }
        })
        .detach();
    }

    /// 冲突框里按「重载」：放弃本地编辑、读回磁盘那一版。放弃之前先把当前内容
    /// 写成恢复快照——按下这个按钮不该丢字。
    pub(crate) fn reload_conflicted_document(&mut self, path: &Path, cx: &mut Context<Self>) {
        self.stash_local_content_for_recovery(path);
        let document = match crate::editor::encoding::load_document(path) {
            Ok(document) => document,
            Err(err) => {
                self.report_document_load_failure(&err, cx);
                return;
            }
        };
        let disk_version = crate::editor::persistence::file_content_version(&document.text);
        self.apply_disk_reload(path, document, disk_version, cx);
        // 内容与磁盘一致了：脏标记与版本号都要跟上记账，否则圆点继续亮着，
        // 而下一次自动保存又会拿旧的版本号去校验新内容。
        if let Some(tab) = self
            .workspace
            .open_documents
            .iter_mut()
            .find(|tab| tab.path == path)
        {
            tab.dirty = false;
        }
        if self.file_path.as_deref() == Some(path) {
            self.file_version = Some(disk_version);
            self.document_dirty = false;
            self.pending_window_edited = false;
            self.pending_window_unedited = true;
            self.pending_window_title_refresh = true;
        }
        self.clear_external_change_conflict_for(path);
        cx.notify();
    }

    /// 打开失败要让用户看得见：侧栏那条红字只在有工作区时才会出现，而拒绝打开
    /// 一个文件是「用户会问为什么」的事，所以编码类拒绝额外给一个应用内模态
    /// （禁系统原生弹窗）。普通 IO 错误照旧只记侧栏。
    fn report_document_load_failure(&mut self, error: &std::io::Error, cx: &mut Context<Self>) {
        let detail = error.to_string();
        self.workspace.file_error = Some(detail.clone());
        if error.kind() == std::io::ErrorKind::InvalidData {
            let title = cx
                .global::<crate::i18n::I18nManager>()
                .strings()
                .open_failed_title
                .clone();
            self.show_message_modal(title, detail, cx);
        }
        cx.notify();
    }

    /// 把刚读到的磁盘内容交还给这一篇：标签缓存、块树与原始字节一起换。
    /// 干净重载与「冲突后选重载」共用这一处，区别只在调用方要不要跳过脏检查。
    fn apply_disk_reload(
        &mut self,
        path: &Path,
        document: crate::editor::encoding::LoadedDocument,
        disk_version: u64,
        cx: &mut Context<Self>,
    ) {
        let is_active = self.file_path.as_deref() == Some(path);
        let disk = document.text;
        if let Some(tab) = self
            .workspace
            .open_documents
            .iter_mut()
            .find(|tab| tab.path == path)
        {
            tab.markdown = disk.clone();
            tab.file_version = disk_version;
        }
        if is_active {
            let path = path.to_path_buf();
            // 还是这一篇：现场当场记下再交回去，模式、视口、光标都不动。
            let view = self.capture_document_view(&self.buffer.text(), cx);
            if is_markdown_file(&path) {
                self.restore_document_from_markdown(disk, path, Some(view), cx);
            } else {
                self.restore_document_from_code_source(disk, path, Some(view), cx);
            }
            // 重载换掉了整个缓冲区：原始字节与文件形状必须跟着接上，否则重载之后
            // 的第一次保存就把 CRLF/GB18030 全文件洗成 LF/UTF-8。
            self.attach_file_origin(document.raw);
        }
    }

    /// 这一篇当前的内容（活动文档或后台标签）先落一份恢复快照。
    fn stash_local_content_for_recovery(&self, path: &Path) {
        let (markdown, id) = if self.file_path.as_deref() == Some(path) {
            (self.document_text_for_save(), self.recovery_id)
        } else {
            let Some(tab) = self
                .workspace
                .open_documents
                .iter()
                .find(|tab| tab.path == path)
            else {
                return;
            };
            (tab.markdown.clone(), tab.recovery_id)
        };
        let snapshot = crate::config::RecoverySnapshot {
            id,
            source_path: Some(path.to_path_buf()),
            markdown,
        };
        if let Err(error) = crate::config::save_recovery_snapshot(&snapshot) {
            eprintln!("failed to stash the discarded edits: {error}");
        }
    }

    /// Cached markdown + dirty state for an open document path (watcher).
    pub(crate) fn cached_tab_content_for_path(
        &self,
        path: &Path,
        _cx: &App,
    ) -> Option<(String, bool)> {
        // AI 接受路径存的是规范化路径，标签与当前文档可能是原始写法；
        // 与 1072 行的标签查找同一条 canonical_path 判据，不做字符串直等。
        let target = crate::file_url::canonical_path(path);
        if self
            .file_path
            .as_deref()
            .is_some_and(|current| crate::file_url::canonical_path(current) == target)
        {
            return Some((self.document_text_for_save(), self.document_dirty));
        }
        self.workspace
            .open_documents
            .iter()
            .find(|tab| crate::file_url::canonical_path(&tab.path) == target)
            .map(|tab| (tab.markdown.clone(), tab.dirty))
    }

    /// Current workspace root, if set.
    pub(crate) fn workspace_root_path(&self) -> Option<&Path> {
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

    /// All markdown/code files of the workspace, for the quick switcher.
    ///
    /// 读的是全局功能首次使用后后台走盘填的名单（`spawn_workspace_files_walk`），不再从侧栏那棵
    /// 树收集：树只加载展开过的层，拿它当名单会让范围随展开状态漂移。
    pub(crate) fn workspace_text_files(&self) -> Vec<PathBuf> {
        self.text_files_on_disk()
    }

    pub(crate) fn workspace_openable_files(&self) -> Vec<PathBuf> {
        self.workspace
            .files_on_disk
            .iter()
            .filter(|path| {
                is_markdown_file(path)
                    || is_code_file(path)
                    || crate::components::Block::has_supported_image_extension(path)
            })
            .cloned()
            .collect()
    }

    /// 工作区里可作替换目标 / 双链候选的文本文件（Markdown + 代码）。
    pub(crate) fn text_files_on_disk(&self) -> Vec<PathBuf> {
        self.workspace
            .files_on_disk
            .iter()
            .filter(|path| is_markdown_file(path) || is_code_file(path))
            .cloned()
            .collect()
    }

    pub(crate) fn set_workspace_tab(&mut self, tab: WorkspaceTab, cx: &mut Context<Self>) {
        if self.workspace.active_tab == tab {
            return;
        }
        self.workspace.search_focus_pending = false;
        self.workspace.search_select_all_pending = false;
        self.workspace.search_pending = false;
        self.workspace.search_generation = self.workspace.search_generation.wrapping_add(1);
        self.workspace.active_tab = tab;
        if tab == WorkspaceTab::Search {
            self.schedule_workspace_search(cx);
        }
        self.sync_workspace_models(cx);
        self.sync_document_search_highlights(cx);
        // 搜索不参与记忆（用户要求）：只在真的换了别的面板时更新记忆值并落盘。
        if tab != WorkspaceTab::Search {
            self.workspace.sidebar_memory_tab = tab;
            self.persist_session(cx);
        }
        cx.notify();
    }

    /// Records the current caret location before a programmatic jump
    /// (roadmap E6).
    pub(crate) fn push_cursor_location(&mut self, cx: &mut Context<Self>) {
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

    /// 按落点半区把标签插到目标之前或之后。
    pub(crate) fn move_tab_to_position(
        &mut self,
        from_path: &Path,
        to_path: &Path,
        after: bool,
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
        let mut insert_at = to + usize::from(after);
        if from < insert_at {
            insert_at = insert_at.saturating_sub(1);
        }
        self.workspace.open_documents.insert(insert_at, tab);
        self.persist_session(cx);
        cx.notify();
        let _ = window;
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

    pub(crate) fn open_workspace_file(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_workspace_file_in_mode(path, WorkspaceOpenMode::Pinned, window, cx);
    }

    /// 按「单击预览 / 双击固定」的模式打开工作区文件（用户需求）。
    pub(crate) fn open_workspace_file_in_mode(
        &mut self,
        path: PathBuf,
        mode: WorkspaceOpenMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.leave_ai_review_for_document_navigation(cx);
        if let Some(group) = self.active_split_editor() {
            let handle = window.window_handle();
            cx.defer(move |cx| {
                if let Err(error) = handle.update(cx, |_, window, cx| group.update(cx, |editor, cx| {
                    editor.open_workspace_file_in_mode(path, mode, window, cx);
                })) { eprintln!("在活动分屏打开文件失败：{error}"); }
            });
            return;
        }
        let untitled = untitled_document_id(&path).is_some();
        if self.active_document_path() == Some(&path) {
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
        // Sniff the content, not the extension: dotfiles like .gitignore have
        // no extension but are text, while a .md full of NUL bytes is not
        // renderable. Non-text files still become the active tab; the content
        // area shows a centered placeholder.
        //
        // 这里以前有一趟 `has_utf16_bom` 直接拒开：那时代码没有 UTF-16 解码器，
        // 只能声明「编码不支持」。解码/编码现在是对称的（`FileShape` 带 BOM 与
        // 行尾一起走），能不能读交给唯一的读盘漏斗 `encoding::load_document` 判定，
        // 入口不再各自猜编码——否则支持的编码越加越多，这里的白名单越漏。
        let image_file = !untitled && crate::components::Block::is_supported_local_image_path(&path);
        if !untitled && !image_file && !is_likely_text_file(&path) {
            self.show_welcome = false;
            self.show_preview_unavailable(path.clone(), window, cx);
            return;
        }
        self.unsupported_preview_path = None;
        self.unsupported_preview_detail = None;
        self.show_welcome = false;
        if self.file_path.is_none() && self.document_dirty
            && self.workspace.active_document.as_ref().is_none_or(|path| untitled_document_id(path).is_none()) {
            self.request_dropped_markdown_replace(path, window, cx);
            return;
        }

        if !self.has_external_autosave_conflict() {
            self.workspace.file_error = None;
        }
        self.snapshot_current_document(cx);
        // 单击/双击打开要替换掉「没改过」的预览标签：预览只在停留期间占标签栏，一切走
        // 就消失（用户需求）。这份清单必须在 `snapshot_current_document` **之后**算——
        // 活动标签的 `dirty` 只在那一步才写回，早算就还是打开时的 false，刚被改脏的这一篇
        // 会被当成干净预览销毁。置脏的正规入口 `finish_dirty` 已就地转正，这里的先后是给
        // 绕开它的入口（`prompts.rs:283`、`mod.rs:1820` 直接赋 `document_dirty`）兜底。
        // 只记录清单、真正删除放在函数末尾：打开流程会把旧活动文档推回标签集，提前删会
        // 被它再加回来。
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
        let shared = self.shared_document_for_open(&path, cx);
        let mut cached = self
            .workspace
            .open_documents
            .iter()
            .find(|tab| tab.path == path)
            .cloned();
        if let Some((buffer, dirty, version, recovery_id)) = &shared {
            if let Some(tab) = cached.as_mut() {
                tab.markdown = buffer.text(); tab.dirty = *dirty; tab.recovery_id = *recovery_id;
                tab.file_shape = buffer.file_shape();
                if let Some(version) = version { tab.file_version = *version; }
            } else {
                cached = Some(WorkspaceDocumentTab { path: path.clone(), recovery_id: *recovery_id,
                    file_version: version.unwrap_or(0), markdown: buffer.text(), file_shape: buffer.file_shape(),
                    dirty: *dirty, preview: false, view: None });
            }
        }
        if untitled && cached.is_none() {
            self.restore_untitled_tabs(std::slice::from_ref(&path), cx);
            cached = self.workspace.open_documents.iter().find(|tab| tab.path == path).cloned();
            if cached.is_none() { return; }
        }
        // 这个标签上次离开时的阅读现场。取不到就是本次会话第一次读这篇，按打开新
        // 文档的口径走（渲染态、文档顶部）。
        let cached_view = cached.as_ref().and_then(|tab| tab.view.clone());
        let cached_file_shape = cached.as_ref().filter(|tab| tab.dirty).and_then(|tab| tab.file_shape);
        // `raw` 是本次读盘拿到的原始字节；脏标签的内容来自内存而不是磁盘，
        // 那种情况没有「原样写回」的依据，留空。
        let (markdown, raw, dirty, recovery_id, file_version) = if image_file {
            // 图片标签只存路径，二进制内容不能进入文本缓冲区或自动保存快照。
            let recovery_id = cached
                .as_ref()
                .map(|tab| tab.recovery_id)
                .unwrap_or_else(uuid::Uuid::new_v4);
            (String::new(), Vec::new(), false, recovery_id, 0)
        } else if let Some(tab) = cached {
            if untitled || tab.dirty || shared.is_some() {
                (tab.markdown, Vec::new(), tab.dirty, tab.recovery_id, tab.file_version)
            } else {
                match crate::editor::encoding::load_document(&path) {
                    Ok(document) => {
                        let crate::editor::encoding::LoadedDocument { raw, text } = document;
                        let file_version = crate::editor::persistence::file_content_version(&text);
                        (text, raw, false, tab.recovery_id, file_version)
                    }
                    Err(err) => {
                        self.report_document_load_failure(&err, cx);
                        return;
                    }
                }
            }
        } else {
            match crate::editor::encoding::load_document(&path) {
                Ok(document) => {
                    let crate::editor::encoding::LoadedDocument { raw, text } = document;
                    let file_version =
                        crate::editor::persistence::file_content_version(&text);
                    (text, raw, false, uuid::Uuid::new_v4(), file_version)
                }
                Err(err) => {
                    self.report_document_load_failure(&err, cx);
                    return;
                }
            }
        };
        // 从磁盘重新读取成功即视为用户接受磁盘内容，该文件的冲突解除。
        if !dirty {
            self.clear_external_change_conflict_for(&path);
        }
        let preview = !untitled && mode != WorkspaceOpenMode::Pinned;
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
                file_shape: cached_file_shape,
                dirty,
                preview,
                view: None,
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
        if !untitled {
            self.workspace.selected = Some(WorkspaceSelection::File(path.clone()));
            self.reveal_path_in_tree(&path);
        }
        // Markdown rendering is for .md/.markdown only; every other text file
        // (code, dotfiles, plain text) opens as monospace source text.
        if image_file {
            self.replace_document_from_markdown(String::new(), Some(path.clone()), cx);
            self.load_image_file_preview(path.clone(), cx);
        } else if untitled {
            let kind = cached_view.map(crate::editor::file_drop::ImportKind::Restore).unwrap_or(crate::editor::file_drop::ImportKind::Open);
            self.replace_document_content(markdown, None, None, kind, cx);
        } else if is_markdown_document(&path) {
            self.restore_document_from_markdown(markdown, path.clone(), cached_view, cx);
        } else {
            self.restore_document_from_code_source(markdown, path.clone(), cached_view, cx);
        }
        self.attach_file_origin(raw);
        if let Some(shape) = cached_file_shape {
            self.buffer.restore_file_shape(shape);
        }
        if let Some((buffer, _, _, recovery_id)) = shared {
            self.buffer = buffer;
            self.recovery_id = recovery_id;
            self.split_projection_revision = self.buffer.revision();
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
    pub(crate) fn show_preview_unavailable(
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
        self.image_preview = None;
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
            file_shape: None,
            dirty: false,
            preview: false,
            view: None,
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
    /// Closes one tab, prompting before discarding unsaved edits.
    pub(crate) fn close_workspace_document(
        &mut self,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_workspace_tabs(std::slice::from_ref(&path.to_path_buf()), window, cx);
    }

    /// 「关闭标签页」命令：关掉当前这一页。当前文档没有对应标签页（未保存的新文档、
    /// 恢复中的快照）时什么也不做——不顺手关窗口（误按一次不该把整窗口带走）。
    pub(crate) fn close_active_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.active_document_path().cloned() else {
            return;
        };
        self.close_workspace_document(&path, window, cx);
    }

    pub(crate) fn close_workspace_tabs_for_action(
        &mut self,
        target: &Path,
        action: TabMenuAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let TabMenuAction::Split(direction) = action {
            self.request_split_document(cx.entity().downgrade(), target.to_path_buf(), direction, true, window, cx);
            return;
        }
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
                TabMenuAction::Split(_) => Vec::new(),
            }
        };
        self.close_workspace_tabs(&paths, window, cx);
    }

    /// Closes the listed tabs. Unsaved edits are confirmed once for the whole
    /// batch; saving writes each tab's cached markdown back to disk.
    pub(crate) fn close_workspace_tabs(
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
        self.refresh_shared_tab_snapshots(cx);
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
            .filter(|tab| tab.dirty && !self.shared_tab_has_other_view(&tab.path, cx))
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

    /// 另存为的目标正好是**另一篇**已打开标签的文件时的拦阻。
    ///
    /// 盘上那一份归那一页：它可能带着未保存的修改（脏标记、恢复快照都在它名下），
    /// 我们的字节直接盖上去等于把那一页的未保存内容静默丢掉，随后
    /// `sync_workspace_after_document_path_change` 还会把它的脏状态抹成干净。干净的那
    /// 一页也不能绕开版本闸门——按它记的盘上版本核一次再写。
    pub(crate) fn ensure_save_as_target_is_free(&self, path: &Path) -> anyhow::Result<()> {
        let target = crate::file_url::canonical_path(path);
        let Some(other) = self.workspace.open_documents.iter().find(|tab| {
            tab.recovery_id != self.recovery_id
                && crate::file_url::canonical_path(&tab.path) == target
        }) else {
            return Ok(());
        };
        if other.dirty {
            anyhow::bail!(
                "「{}」已在另一个标签里打开，且有未保存的修改：先保存或关掉它，再另存到这个文件名",
                path.display()
            );
        }
        crate::editor::persistence::verify_file_version(&other.path, other.file_version)?;
        Ok(())
    }

    /// Removes closed tabs from the strip, optionally saving their cached
    /// content first, and activates the nearest remaining neighbour.
    pub(crate) fn finish_close_workspace_tabs(
        &mut self,
        closing: &[WorkspaceDocumentTab],
        save_first: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if save_first {
            if self.has_marked_document_text(cx) {
                // 组合还没落定：这一趟不写盘也不关标签，等它结束（收尾那一步会按
                // `pending_save` 再存一次）。与其余四个保存入口同一道闸门——拿正在
                // 组合的正文落盘，写出去的是半个词。
                self.pending_save = true;
                cx.notify();
                return;
            }
            if let Some(tab) = closing.iter().find(|tab| tab.dirty && untitled_document_id(&tab.path).is_some() && !self.shared_tab_has_other_view(&tab.path, cx)) {
                self.pending_tab_close_after_save = closing.iter().map(|tab| tab.recovery_id).collect();
                self.open_workspace_file_in_mode(tab.path.clone(), WorkspaceOpenMode::Activate, window, cx);
                self.save_document(window, cx);
                return;
            }
            for tab in closing {
                if !tab.dirty || self.shared_tab_has_other_view(&tab.path, cx) {
                    continue;
                }
                // 活动文档的正文在缓冲区里，而 `closing` 是弹框那一刻的标签快照：
                // 模态不抢焦点，用户能边打字边点「保存并关闭」，拿快照落盘就把那一
                // 步之后打的字丢在盘外（脏标记与恢复快照又被清掉，退出即丢字）。
                // 这一页走与其他保存入口同一条路：当前缓冲区字节 + 版本闸门 + 提交
                // 前现势校验。
                if self.file_path.as_ref() == Some(&tab.path)
                    && self.recovery_id == tab.recovery_id
                {
                    if !self.save_to_existing_path(&tab.path, window, cx) {
                        return;
                    }
                    continue;
                }
                let bytes = match crate::editor::persistence::tab_write_bytes(&tab.path, &tab.markdown, tab.file_version) {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        self.report_external_change_conflict(tab.path.clone(), error.to_string(), cx);
                        return;
                    }
                };
                match crate::editor::persistence::write_atomic(
                    &tab.path,
                    &bytes,
                ) {
                    Ok(()) => {
                        let version = crate::editor::persistence::file_content_version(&tab.markdown);
                        self.mark_workspace_document_saved(&tab.path, version, &tab.markdown);
                        if !self.record_saved_history(&tab.path, &bytes, crate::config::file_history::HistorySource::Exit, cx) {
                            return;
                        }
                        if self.file_path.as_ref() == Some(&tab.path) {
                            self.document_dirty = false;
                            self.file_version = Some(version);
                            window.set_window_edited(false);
                        }
                        if let Err(error) = crate::config::remove_recovery_snapshot(tab.recovery_id) {
                            eprintln!("无法移除已保存标签的恢复快照：{error}");
                        }
                    }
                    Err(err) => {
                        self.workspace.file_error = Some(err.to_string());
                        let title = cx.global::<crate::i18n::I18nManager>().strings().save_failed_title.clone();
                        self.show_message_modal(title, err.to_string(), cx);
                        return;
                    }
                }
            }
        }

        let closing_paths: Vec<PathBuf> = closing.iter().map(|tab| tab.path.clone()).collect();
        self.preserve_shared_history_before_close(&closing_paths, cx);
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

        for tab in closing {
            if untitled_document_id(&tab.path).is_some() && !self.shared_tab_has_other_view(&tab.path, cx) {
                if let Err(error) = crate::config::remove_recovery_snapshot(tab.recovery_id) {
                    eprintln!("清理已关闭草稿失败：{error}");
                }
            }
        }

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
            self.workspace.active_document = None;
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
}
