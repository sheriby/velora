use super::*;

impl Editor {
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
        let Ok(document) = crate::editor::encoding::load_document(path) else {
            return;
        };
        let disk = document.text;
        // 标签缓存与缓冲区一样存 LF 文本，磁盘上的 CRLF 不是「外部改动」：按规范化
        // 后的版本号比，否则每次监听事件都会把干净文件当成被改了，重新导入一遍。
        let disk_version = crate::editor::persistence::file_content_version(&disk);
        if disk_version == crate::editor::persistence::file_content_version(&cached_markdown) {
            return;
        }
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
            let view = self.capture_document_view(cx);
            if is_markdown_file(&path) {
                self.restore_document_from_markdown(disk, path, Some(view), cx);
            } else {
                self.restore_document_from_code_source(disk, path, Some(view), cx);
            }
            // 重载换掉了整个缓冲区：原始字节与文件形状必须跟着接上，否则重载之后
            // 的第一次保存就把 CRLF/GB18030 全文件洗成 LF/UTF-8。
            self.attach_file_origin(document.raw);
        }
        cx.notify();
    }

    /// Cached markdown + dirty state for an open document path (watcher).
    pub(crate) fn cached_tab_content_for_path(
        &self,
        path: &Path,
        _cx: &App,
    ) -> Option<(String, bool)> {
        if self.file_path.as_deref() == Some(path) {
            return Some((self.document_text_for_save(), self.document_dirty));
        }
        self.workspace
            .open_documents
            .iter()
            .find(|tab| &tab.path == path)
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

    /// All markdown/code files of the workspace tree, for the quick switcher.
    pub(crate) fn workspace_text_files(&self) -> Vec<PathBuf> {
        self.workspace
            .file_tree
            .as_ref()
            .map(|tree| collect_workspace_files(tree))
            .unwrap_or_default()
    }

    pub(crate) fn set_workspace_tab(&mut self, tab: WorkspaceTab, cx: &mut Context<Self>) {
        let changed = self.workspace.active_tab != tab;
        self.workspace.search_focus_pending = false;
        self.workspace.search_query.clear();
        self.workspace.search_selected_range = 0..0;
        self.workspace.search_marked_range = None;
        self.workspace.search_results.clear();
        self.workspace.document_active_range = None;
        self.workspace.search_pending = false;
        self.workspace.search_generation = self.workspace.search_generation.wrapping_add(1);
        if changed {
            self.workspace.active_tab = tab;
            self.sync_workspace_models(cx);
            cx.notify();
        }
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

    /// 拖拽标签落到目标标签上：把被拖标签移动到目标位置（roadmap E3）。
    pub(crate) fn move_tab_to_position(
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
        let cached = self
            .workspace
            .open_documents
            .iter()
            .find(|tab| tab.path == path)
            .cloned();
        // 这个标签上次离开时的阅读现场。取不到就是本次会话第一次读这篇，按打开新
        // 文档的口径走（渲染态、文档顶部）。
        let cached_view = cached.as_ref().and_then(|tab| tab.view.clone());
        // `raw` 是本次读盘拿到的原始字节；脏标签的内容来自内存而不是磁盘，
        // 那种情况没有「原样写回」的依据，留空。
        let (markdown, raw, dirty, recovery_id, file_version) = if let Some(tab) = cached {
            if tab.dirty {
                (tab.markdown, Vec::new(), true, tab.recovery_id, tab.file_version)
            } else {
                match crate::editor::encoding::load_document(&path) {
                    Ok(document) => {
                        let crate::editor::encoding::LoadedDocument { raw, text } = document;
                        let file_version = crate::editor::persistence::file_content_version(&text);
                        (text, raw, false, tab.recovery_id, file_version)
                    }
                    Err(err) => {
                        self.workspace.file_error = Some(err.to_string());
                        cx.notify();
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
        self.workspace.selected = Some(WorkspaceSelection::File(path.clone()));
        self.reveal_path_in_tree(&path);
        // Markdown rendering is for .md/.markdown only; every other text file
        // (code, dotfiles, plain text) opens as monospace source text.
        if is_markdown_document(&path) {
            self.restore_document_from_markdown(markdown, path.clone(), cached_view, cx);
        } else {
            self.restore_document_from_code_source(markdown, path.clone(), cached_view, cx);
        }
        self.attach_file_origin(raw);
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

    pub(crate) fn close_workspace_tabs_for_action(
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
    pub(crate) fn finish_close_workspace_tabs(
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
                // 已知限制：后台标签没有缓冲区，`tab.markdown` 是切换时存下的
                // LF 文本——这里写出去会把 CRLF/GB18030 洗成 LF/UTF-8。修法是让
                // tab 快照携带字节与 FileShape（独立工作项，见 FIXPLAN B2）。
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
}
