use super::*;

use crate::config::{SidebarOpenPreference, SidebarPanelPreference};

impl Editor {
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
            sidebar_tab: Some(self.workspace.sidebar_memory_tab.session_key().to_string()),
            sidebar_open: Some(self.workspace.is_open),
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
        self.snapshot_current_document(cx);
        if self.document_dirty && self.file_path.as_ref().is_some_and(|path| !path_is_within_root(&root, path)) {
            let strings = cx.global::<crate::i18n::I18nManager>().strings().clone();
            self.show_modal(
                ModalSpec {
                    title: strings.unsaved_changes_title.into(),
                    detail: Some(strings.unsaved_changes_message.into()),
                    buttons: vec![
                        strings.unsaved_changes_save_and_close.into(),
                        strings.unsaved_changes_discard_and_close.into(),
                        strings.unsaved_changes_cancel.into(),
                    ],
                    default_index: 0, cancel_index: 2,
                },
                move |choice, editor, window, cx| match choice {
                    0 => {
                        if let Some(path) = editor.file_path.clone()
                            && editor.save_to_existing_path(&path, window, cx) {
                            editor.apply_workspace_root(root, cx);
                        }
                    }
                    1 => {
                        editor.autosave_task = None;
                        editor.document_dirty = false;
                        if let Some(path) = &editor.file_path
                            && let Some(tab) = editor.workspace.open_documents.iter_mut().find(|tab| &tab.path == path) {
                            tab.dirty = false;
                        }
                        editor.apply_workspace_root(root, cx);
                    }
                    _ => {}
                },
                cx,
            );
            return;
        }
        self.apply_workspace_root(root, cx);
    }

    fn apply_workspace_root(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        if !self.prune_workspace_tabs_outside_root(&root, cx) {
            return;
        }
        // 恢复该工作区记忆的侧栏宽度（roadmap E7）。宽度按工作区记，侧栏开关与
        // 面板是全局记忆（见下面），两者口径不同。
        let session = crate::config::read_session()
            .map_err(|error| eprintln!("读取上次的会话失败：{error}"))
            .ok();
        if let Some(session) = session.as_ref()
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
        self.workspace.root = Some(root);
        self.workspace.file_tree = None;
        // 打开新文件夹必须重新扫描：清掉缓存结果标记（roadmap D9）。
        self.workspace.tree_scan_root = None;
        // 文件名单（⌘P / 搜索 / 全部替换 / 双链共用）同理：换根重走一次。
        self.workspace.files_on_disk.clear();
        self.workspace.files_on_disk_root = None;
        // 旧根上在飞的按需扫层结果全部作废（drop 即取消）。
        self.workspace.dir_scan_tasks.clear();
        self.clear_workspace_file_error();
        self.workspace.expanded.clear();
        // 侧栏开关与面板是**全局**记忆（用户要求：不按工作区记）：上次关着就关着，
        // 上次挑的是哪个面板就还是它——搜索不参与记忆，所以记忆里只会是非搜索面板。
        // 设置里选「跟随上次」（默认）时听记忆，钉死了（总是打开/总是关闭、文件数/
        // 大纲）就听设置。
        self.apply_sidebar_startup_state(session.as_ref(), cx);
        self.workspace.search_scope = WorkspaceSearchScope::Workspace;
        self.workspace.search_focus_pending = false;
        self.workspace.search_query.clear();
        self.workspace.search_selected_range = 0..0;
        self.workspace.search_marked_range = None;
        self.workspace.search_results.clear();
        self.workspace.document_active_range = None;
        self.workspace.search_pending = false;
        self.workspace.search_generation = self.workspace.search_generation.wrapping_add(1);
        // 基准目录变了（未落盘文档的相对图片目标从「无基准」变「按工作区根」），
        // 块上的图片 runtime 要按新基准重算一次；与每键重建走同一入口。
        self.rebuild_image_runtimes(cx);
        self.sync_workspace_file_tree(cx);
        self.sync_workspace_outline(cx);
        self.ensure_workspace_watcher(cx);
        self.persist_session(cx);
        cx.notify();
    }

    /// 侧栏启动状态（开关与面板）：设置钉死了听设置，否则听会话记忆。
    /// 开文件夹（`apply_workspace_root`）与开窗（`apply_sidebar_startup_state_from_disk`）
    /// 两条启动路径共用这一处判据。
    pub(crate) fn apply_sidebar_startup_state(
        &mut self,
        session: Option<&crate::config::SessionState>,
        cx: &App,
    ) {
        let remembered_tab = session
            .and_then(|session| session.sidebar_tab.as_deref())
            .and_then(WorkspaceTab::from_session_key);
        let startup_tab = match crate::config::EditorSettings::sidebar_panel_on_startup(cx) {
            SidebarPanelPreference::FollowLast => remembered_tab.unwrap_or(WorkspaceTab::Files),
            SidebarPanelPreference::Files => WorkspaceTab::Files,
            SidebarPanelPreference::Outline => WorkspaceTab::Outline,
        };
        self.workspace.sidebar_memory_tab = startup_tab;
        self.workspace.active_tab = startup_tab;
        self.workspace.is_open = match crate::config::EditorSettings::sidebar_open_on_startup(cx) {
            SidebarOpenPreference::FollowLast => session
                .and_then(|session| session.sidebar_open)
                .unwrap_or(true),
            SidebarOpenPreference::Always => true,
            SidebarOpenPreference::Never => false,
        };
    }

    /// 开窗路径用：直接打开文件启动（Windows 关联双击 / macOS 双击）没有
    /// `set_workspace_root` 可依赖，这里自己读会话记忆再落位。
    pub(crate) fn apply_sidebar_startup_state_from_disk(&mut self, cx: &App) {
        let session = crate::config::read_session()
            .map_err(|error| eprintln!("读取上次的会话失败：{error}"))
            .ok();
        self.apply_sidebar_startup_state(session.as_ref(), cx);
    }

    /// 工作区根就绪后启动文件监听；同一根不重复启动（
    /// 隐含根：只打开单个文件时也要监听，否则外部修改不会重载，用户报修）。
    pub(crate) fn ensure_workspace_watcher(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.workspace.root.clone() else {
            return;
        };
        if self.watched_workspace_root.as_ref() == Some(&root) && self.external_watcher.is_some() {
            return;
        }
        self.watched_workspace_root = Some(root.clone());
        // 测试进程里成百上千个窗口各起一个 OS watcher 会把 fd 打爆
        // （Too many open files），“要不要监听”的决策在测试里可断言，
        // OS 侧的监听行为由真实运行验证。
        if cfg!(test) {
            return;
        }
        crate::editor::watcher::start_watching(self, &root, cx);
    }

    /// watcher 事件统一入口：外部文件的增/删/改都走这里。
    pub(crate) fn on_watched_path_changed(&mut self, path: &Path, cx: &mut Context<Self>) {
        self.reload_externally_changed_document(path, cx);
        self.schedule_workspace_tree_refresh(cx);
    }

    /// 合并连续的 watcher 事件，防抖后强制重扫文件树：外部新建/删除/改名
    /// 也要反映到树、⌘P 与工作区搜索的文件列表里（用户报修：外部改动后
    /// 树一直是旧的，点进去报「无法预览」）。
    pub(crate) fn schedule_workspace_tree_refresh(&mut self, cx: &mut Context<Self>) {
        self.workspace.tree_refresh_generation = self
            .workspace
            .tree_refresh_generation
            .wrapping_add(1);
        let generation = self.workspace.tree_refresh_generation;
        cx.spawn(async move |editor, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(250))
                .await;
            editor
                .update(cx, |editor, cx| {
                    if editor.workspace.tree_refresh_generation == generation {
                        editor.refresh_workspace_tree(cx);
                    }
                })
                .ok();
        })
        .detach();
    }

    /// 收起落在新工作区之外的标签；脏标签先写回自己的文件，内容不丢。
    /// 若当前文档被收起：还有标签就延后打开最近的那个（需要 `&mut Window`），
    /// 没有标签就清空文档并回到欢迎页。
    pub(crate) fn prune_workspace_tabs_outside_root(&mut self, root: &Path, cx: &mut Context<Self>) -> bool {
        let stale = self
            .workspace
            .open_documents
            .iter()
            .filter(|tab| !path_is_within_root(root, &tab.path))
            .cloned()
            .collect::<Vec<_>>();
        if stale.is_empty() {
            return true;
        }
        let stale_paths = stale
            .iter()
            .map(|tab| tab.path.clone())
            .collect::<Vec<PathBuf>>();
        for tab in &stale {
            if !tab.dirty {
                continue;
            }
            let bytes = match crate::editor::persistence::tab_write_bytes(&tab.path, &tab.markdown, tab.file_version) {
                Ok(bytes) => bytes,
                Err(error) => {
                    self.report_external_change_conflict(tab.path.clone(), error.to_string(), cx);
                    return false;
                }
            };
            match crate::editor::persistence::write_atomic(
                &tab.path,
                &bytes,
            ) {
                Ok(()) => {
                    let version = crate::editor::persistence::file_content_version(&tab.markdown);
                    self.mark_workspace_document_saved(&tab.path, version, &tab.markdown);
                    if let Err(error) = crate::config::remove_recovery_snapshot(tab.recovery_id) {
                        eprintln!("无法移除已保存标签的恢复快照：{error}");
                    }
                }
                Err(err) => {
                    self.workspace.file_error = Some(format!(
                        "无法保存「{}」：{err}",
                        tab.path.display()
                    ));
                    let title = cx.global::<crate::i18n::I18nManager>().strings().save_failed_title.clone();
                    self.show_message_modal(title, err.to_string(), cx);
                    return false;
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
            return true;
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
        true
    }

    pub(crate) fn selected_workspace_directory(&self) -> Option<PathBuf> {
        match self.workspace.selected.as_ref() {
            Some(WorkspaceSelection::Directory(path)) => Some(path.clone()),
            Some(WorkspaceSelection::File(path)) => path.parent().map(Path::to_path_buf),
            Some(WorkspaceSelection::WorkspaceRoot(path)) => Some(path.clone()),
            _ => self.workspace.root.clone(),
        }
    }

    pub(crate) fn refresh_workspace_tree(&mut self, cx: &mut Context<Self>) {
        // 外部新建 / 删除 / 改名也要刷新文件名单：⌘P、工作区搜索、全部替换与双链
        // 都读这一份，只重扫树会让它们继续拿旧名单。
        self.workspace.files_on_disk_root = None;
        if let Some(root) = self.workspace.root.clone() {
            self.spawn_workspace_files_walk(root, cx);
        }
        // 保留旧树直到新扫描落地，避免侧栏在扫描期间闪空。
        self.sync_workspace_file_tree_inner(true, cx);
        if self.workspace.active_tab == WorkspaceTab::Search
            && !self.workspace.search_query.is_empty()
        {
            self.schedule_workspace_search(cx);
        }
        cx.notify();
    }

    pub(crate) fn show_workspace_file_error(
        window_handle: AnyWindowHandle,
        detail: String,
        cx: &mut AsyncApp,
    ) {
        show_async_message_modal(window_handle, cx, move |strings| {
            (strings.open_failed_title.clone(), detail.clone())
        });
    }

    pub(crate) fn show_external_change_error(
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

    pub(crate) fn show_workspace_save_error(
        window_handle: AnyWindowHandle,
        detail: String,
        cx: &mut AsyncApp,
    ) {
        show_async_message_modal(window_handle, cx, move |strings| {
            (strings.save_failed_title.clone(), detail.clone())
        });
    }

    pub(crate) fn report_workspace_file_error(&mut self, detail: String, cx: &mut Context<Self>) {
        if self.workspace.root.is_none() {
            self.workspace.root = self.workspace_root_for_current_file();
        }
        self.workspace.file_error = Some(detail);
        cx.notify();
    }

}
