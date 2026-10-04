use super::*;

impl Editor {
    pub(crate) fn sync_workspace_after_document_path_change(&mut self, cx: &mut Context<Self>) {
        if let Some(path) = self.file_path.clone() {
            let previous = self.workspace.active_document.clone();
            if previous.as_ref() != Some(&path) {
                let markdown = self.document_text_for_save();
                let file_version = self
                    .file_version
                    .unwrap_or_else(|| crate::editor::persistence::file_content_version(&markdown));
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
        self.workspace.outline_stale = true;
        if self.workspace.root.is_none() {
            self.workspace.root = self.workspace_root_for_current_file();
        }
        // 隐含根（只打开单个文件）也要起监听，否则外部修改永远不会重载。
        self.ensure_workspace_watcher(cx);
        if previous_root != self.workspace.root {
            self.workspace.file_tree = None;
            self.workspace.tree_scan_root = None;
        }
        if self.workspace.is_open {
            self.sync_workspace_models(cx);
        }
        self.refresh_document_find_after_edit(cx);
    }

    pub(crate) fn sync_workspace_models(&mut self, cx: &mut Context<Self>) {
        self.sync_workspace_file_tree(cx);
        self.sync_workspace_outline(cx);
        self.ensure_current_document_tab(cx);
    }

    pub(crate) fn ensure_current_document_tab(&mut self, _cx: &mut Context<Self>) {
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
            let markdown = self.document_text_for_save();
            let file_version = self
                .file_version
                .unwrap_or_else(|| crate::editor::persistence::file_content_version(&markdown));
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

    pub(crate) fn snapshot_current_document(&mut self, _cx: &App) {
        let Some(path) = self.file_path.clone() else {
            return;
        };
        let markdown = self.document_text_for_save();
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
                    .unwrap_or_else(|| crate::editor::persistence::file_content_version(&markdown)),
                markdown,
                dirty: self.document_dirty,
                preview: false,
            });
        }
        self.workspace.active_document = Some(path);
    }

    pub(crate) fn dirty_workspace_documents(
        &mut self,
        cx: &App,
    ) -> Vec<WorkspaceAutosaveDocument> {
        self.snapshot_current_document(cx);
        // 活动文档的落盘字节直接取自缓冲区；后台标签退写文本（见 struct 注释）。
        let active_bytes = self.document_bytes_for_save();
        let active_path = self.file_path.clone();
        self.workspace
            .open_documents
            .iter()
            .filter(|tab| tab.dirty)
            .map(|tab| WorkspaceAutosaveDocument {
                recovery_id: tab.recovery_id,
                file_version: tab.file_version,
                path: tab.path.clone(),
                markdown: tab.markdown.clone(),
                bytes: (active_path.as_ref() == Some(&tab.path)).then(|| active_bytes.clone()),
            })
            .collect()
    }

    pub(crate) fn mark_workspace_documents_saved(
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
            tab.file_version = crate::editor::persistence::file_content_version(&document.markdown);
            if self.file_path.as_ref() == Some(&tab.path) {
                self.file_version = Some(tab.file_version);
            }
            active_document_saved |= self.workspace.active_document.as_ref() == Some(&tab.path);
        }
        active_document_saved
    }

    pub(crate) fn has_dirty_workspace_documents(&self) -> bool {
        self.workspace.open_documents.iter().any(|tab| tab.dirty)
    }

    pub(crate) fn has_external_autosave_conflict(&self) -> bool {
        self.workspace.external_change_conflict.is_some()
    }

    /// 手动保存成功后同步工作区标签：版本号、内容与脏标记都要跟上刚落盘的
    /// 文件，否则下一次自动保存会拿旧版本去校验新内容，把自己的保存误报成
    /// 外部修改（用户报修）。
    pub(crate) fn mark_workspace_document_saved(
        &mut self,
        path: &Path,
        file_version: u64,
        markdown: &str,
    ) {
        if let Some(tab) = self
            .workspace
            .open_documents
            .iter_mut()
            .find(|tab| tab.path == path)
        {
            tab.file_version = file_version;
            tab.markdown = markdown.to_string();
            tab.dirty = false;
        }
    }

    /// 记录外部修改冲突：自动保存不得覆盖磁盘上的新内容，直到该文件重新加载。
    pub(crate) fn report_external_change_conflict(
        &mut self,
        path: PathBuf,
        detail: String,
        cx: &mut Context<Self>,
    ) {
        self.workspace.external_change_conflict = Some((path, detail.clone()));
        self.report_workspace_file_error(detail, cx);
    }

    /// 清空工作区错误提示；外部修改冲突未解决时保留提示。
    pub(crate) fn clear_workspace_file_error(&mut self) {
        if self.workspace.external_change_conflict.is_none() {
            self.workspace.file_error = None;
        }
    }

    /// 该文件重新读盘成功即视为冲突解除。
    pub(crate) fn clear_external_change_conflict_for(&mut self, path: &Path) {
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

    pub(crate) fn workspace_recovery_ids(&self) -> Vec<uuid::Uuid> {
        self.workspace
            .open_documents
            .iter()
            .map(|tab| tab.recovery_id)
            .collect()
    }

    pub(crate) fn workspace_root_for_current_file(&self) -> Option<PathBuf> {
        self.file_path.as_ref()?.parent().map(Path::to_path_buf)
    }

    pub(crate) fn workspace_root_for_image_paste(&self) -> Option<PathBuf> {
        self.workspace.root.clone()
    }

    pub(crate) fn markdown_state_for_path(
        &self,
        path: &Path,
        _cx: &App,
    ) -> Option<(String, bool, u64)> {
        if self.file_path.as_deref() == Some(path) {
            let markdown = self.document_text_for_save();
            return Some((
                markdown.clone(),
                self.document_dirty,
                self.file_version
                    .unwrap_or_else(|| crate::editor::persistence::file_content_version(&markdown)),
            ));
        }
        self.workspace
            .open_documents
            .iter()
            .find(|tab| tab.path == path)
            .map(|tab| (tab.markdown.clone(), tab.dirty, tab.file_version))
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

}
