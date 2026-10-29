use super::*;
use crate::editor::buffer::BufferChange;

impl Editor {
    pub(in crate::editor) fn adopt_ai_shared_document(&mut self, path: PathBuf, buffer: crate::editor::buffer::TextBuffer, owner: WeakEntity<Editor>, version: u64, recovery_id: uuid::Uuid) {
        if let Some(workspace) = self.split_workspace.as_mut() {
            let synced = (buffer.identity(), buffer.revision(), buffer.file_shape());
            workspace.documents.insert(path, super::SharedDocument { buffer, owner, dirty: true, file_version: Some(version), recovery_id, synced });
        }
    }
    pub(in crate::editor) fn remap_split_workspace_paths(
        &mut self,
        source: &Path,
        destination: &Path,
        is_directory: bool,
        cx: &mut Context<Self>,
    ) {
        if let Some(workspace) = self.split_workspace.as_mut() {
            workspace.documents = std::mem::take(&mut workspace.documents)
                .into_iter()
                .map(|(path, document)| {
                    (
                        remap_moved_path(&path, source, destination, is_directory).unwrap_or(path),
                        document,
                    )
                })
                .collect();
        }
        for group in self.split_groups() {
            group.update(cx, |editor, cx| {
                editor.apply_workspace_rename(source, destination, is_directory, cx);
                cx.notify();
            });
        }
    }

    pub(in crate::editor) fn resolve_split_conflict(
        &mut self,
        path: &Path,
        save_as: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.split_workspace.is_none() {
            return false;
        }
        let target = self
            .split_groups()
            .into_iter()
            .find(|group| group.read(cx).file_path.as_deref() == Some(path))
            .or_else(|| self.active_split_editor());
        let Some(target) = target else {
            return false;
        };
        self.activate_split_group(target.entity_id(), cx);
        let host = cx.entity().downgrade();
        let group = target.downgrade();
        let path = path.to_path_buf();
        let handle = window.window_handle();
        cx.defer(move |cx| {
            let result = handle
                .update(cx, |_, window, cx| {
                    group.update(cx, |editor, cx| {
                        if editor.file_path.as_ref() != Some(&path) {
                            editor.open_workspace_file_in_mode(
                                path.clone(),
                                WorkspaceOpenMode::Activate,
                                window,
                                cx,
                            );
                        }
                        if editor.file_path.as_ref() != Some(&path) {
                            return false;
                        }
                        if save_as {
                            editor.request_save_document_as(cx);
                            return false;
                        }
                        editor.reload_conflicted_document(&path, cx);
                        !editor.document_dirty
                    })
                })
                .and_then(|result| result);
            match result {
                Ok(true) => {
                    if let Err(error) = host.update(cx, |editor, cx| {
                        editor.clear_external_change_conflict_for(&path);
                        editor.sync_shared_groups(group.entity_id(), cx);
                        cx.notify();
                    }) {
                        eprintln!("同步分屏重载结果失败：{error}");
                    }
                }
                Ok(false) => {}
                Err(error) => eprintln!("处理分屏文件冲突失败：{error}"),
            }
        });
        true
    }

    pub(in crate::editor) fn reload_hidden_shared_document(
        &mut self,
        path: &Path,
        cx: &mut Context<Self>,
    ) {
        if crate::config::EditorSettings::external_change_policy(cx)
            == crate::config::ExternalChangePolicy::Manual
            || crate::components::Block::has_supported_image_extension(path)
        {
            return;
        }
        let groups = self.split_groups();
        if groups
            .iter()
            .any(|group| group.read(cx).file_path.as_deref() == Some(path))
        {
            return;
        }
        let Some(document) = self
            .split_workspace
            .as_mut()
            .and_then(|workspace| workspace.documents.get_mut(path))
        else {
            return;
        };
        if document.dirty
            || groups.iter().any(|group| {
                group
                    .read(cx)
                    .workspace
                    .open_documents
                    .iter()
                    .any(|tab| tab.path == path && tab.dirty)
            })
        {
            return;
        }
        let loaded = match crate::editor::encoding::load_document(path) {
            Ok(loaded) => loaded,
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
        let text = loaded.text.replace("\r\n", "\n").replace('\r', "\n");
        if document.buffer.matches_text(&text) {
            return;
        }
        let version = crate::editor::persistence::file_content_version(&text);
        let mut buffer = crate::editor::buffer::TextBuffer::from_text(&text);
        let shape = crate::editor::buffer::FileShape::detect(&loaded.raw);
        buffer.set_file_origin(loaded.raw, shape);
        document.buffer = buffer.shared_view();
        document.file_version = Some(version);
        for group in groups {
            group.update(cx, |editor, _| {
                if let Some(tab) = editor
                    .workspace
                    .open_documents
                    .iter_mut()
                    .find(|tab| tab.path == path)
                {
                    tab.markdown = text.clone();
                    tab.file_shape = Some(shape);
                    tab.file_version = version;
                    if let Some(view) = &mut tab.view {
                        view.editing_state = None;
                        view.row_layout = None;
                    }
                }
            });
        }
    }

    pub(in crate::editor) fn ensure_other_groups_save_as_target_is_free(
        &self,
        path: &Path,
        cx: &Context<Self>,
    ) -> anyhow::Result<()> {
        let Some(parent) = &self.split_parent else {
            return Ok(());
        };
        let current = cx.entity_id();
        let target = crate::file_url::canonical_path(path);
        let other_tabs = parent.read_with(cx, |editor, cx| {
            editor
                .split_groups()
                .into_iter()
                .filter(|group| group.entity_id() != current)
                .flat_map(|group| group.read(cx).workspace.open_documents.clone())
                .collect::<Vec<_>>()
        })?;
        for tab in other_tabs.iter().filter(|tab| {
            tab.recovery_id != self.recovery_id
                && crate::file_url::canonical_path(&tab.path) == target
        }) {
            anyhow::ensure!(
                !tab.dirty,
                "「{}」已在另一个编辑器组打开，且有未保存的修改，请先保存或关闭它",
                path.display()
            );
            crate::editor::persistence::verify_file_version(&tab.path, tab.file_version)?;
        }
        Ok(())
    }

    pub(in crate::editor) fn has_dirty_split_documents(&self, cx: &App) -> bool {
        self.split_groups()
            .iter()
            .any(|group| group.read(cx).document_dirty)
            || self.split_workspace.as_ref().is_some_and(|workspace| {
                workspace.documents.values().any(|document| document.dirty)
            })
    }

    pub(in crate::editor) fn has_untitled_documents(&self, cx: &App) -> bool {
        self.workspace
            .open_documents
            .iter()
            .any(|tab| untitled_document_id(&tab.path).is_some())
            || self.split_groups().iter().any(|group| {
                group
                    .read(cx)
                    .workspace
                    .open_documents
                    .iter()
                    .any(|tab| untitled_document_id(&tab.path).is_some())
            })
    }

    pub(in crate::editor) fn is_untitled_recovery_open(&self, id: uuid::Uuid, cx: &App) -> bool {
        let matches = |editor: &Editor| {
            editor
                .workspace
                .open_documents
                .iter()
                .any(|tab| tab.recovery_id == id && untitled_document_id(&tab.path).is_some())
                || (editor.active_document_path().is_none()
                    && editor.document_dirty
                    && editor.recovery_id == id)
        };
        if self.split_workspace.is_some() {
            self.split_groups()
                .iter()
                .any(|group| matches(group.read(cx)))
        } else {
            matches(self)
        }
    }

    pub(in crate::editor) fn untitled_recovery_snapshots(
        &self,
        cx: &App,
    ) -> Vec<crate::config::RecoverySnapshot> {
        let mut snapshots = HashMap::new();
        let mut collect = |editor: &Editor| {
            for tab in &editor.workspace.open_documents {
                if untitled_document_id(&tab.path).is_none() {
                    continue;
                }
                let markdown = self
                    .split_workspace
                    .as_ref()
                    .and_then(|workspace| workspace.documents.get(&tab.path))
                    .map(|document| document.buffer.text())
                    .unwrap_or_else(|| {
                        if editor.active_document_path() == Some(&tab.path) {
                            editor.buffer.text()
                        } else {
                            tab.markdown.clone()
                        }
                    });
                snapshots.insert(
                    tab.recovery_id,
                    crate::config::RecoverySnapshot {
                        id: tab.recovery_id,
                        source_path: None,
                        markdown,
                    },
                );
            }
            if editor.split_parent.is_some()
                && editor.file_path.is_none()
                && editor.document_dirty
                && editor.workspace.active_document.is_none()
            {
                snapshots.insert(
                    editor.recovery_id,
                    crate::config::RecoverySnapshot {
                        id: editor.recovery_id,
                        source_path: editor.recovery_source_path.clone(),
                        markdown: editor.buffer.text(),
                    },
                );
            }
        };
        collect(self);
        for group in self.split_groups() {
            collect(group.read(cx));
        }
        snapshots.into_values().collect()
    }

    pub(in crate::editor) fn save_untitled_split_before_close(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.collect_split_tabs(cx);
        self.snapshot_current_document(cx);
        if self.split_workspace.is_none() {
            if let Some(path) = self
                .workspace
                .open_documents
                .iter()
                .find(|tab| tab.dirty && untitled_document_id(&tab.path).is_some())
                .map(|tab| tab.path.clone())
            {
                if self.active_document_path() == Some(&path) {
                    return false;
                }
                self.open_workspace_file_in_mode(path, WorkspaceOpenMode::Activate, window, cx);
                self.pending_close_after_save = true;
                self.pending_save = true;
                cx.notify();
                return true;
            }
            return false;
        }
        let Some(group) = self.split_groups().into_iter().find(|group| {
            let editor = group.read(cx);
            (editor.file_path.is_none() && editor.document_dirty)
                || editor
                    .workspace
                    .open_documents
                    .iter()
                    .any(|tab| tab.dirty && untitled_document_id(&tab.path).is_some())
        }) else {
            return false;
        };
        self.activate_split_group(group.entity_id(), cx);
        self.hide_unsaved_changes_dialog(cx);
        let handle = window.window_handle();
        cx.defer(move |cx| {
            if let Err(error) = handle.update(cx, |_, window, cx| {
                group.update(cx, |editor, cx| {
                    let path = editor
                        .workspace
                        .open_documents
                        .iter()
                        .find(|tab| tab.dirty && untitled_document_id(&tab.path).is_some())
                        .map(|tab| tab.path.clone());
                    if let Some(path) = path {
                        editor.open_workspace_file_in_mode(
                            path,
                            WorkspaceOpenMode::Activate,
                            window,
                            cx,
                        );
                    }
                    editor.pending_close_after_save = true;
                    editor.save_document(window, cx);
                });
            }) {
                eprintln!("保存未命名分屏文档失败：{error}");
            }
        });
        true
    }

    pub(in crate::editor) fn split_document_bytes_for_save(&self, path: &Path) -> Option<Vec<u8>> {
        self.split_workspace
            .as_ref()?
            .documents
            .get(path)
            .map(|document| document.buffer.file_bytes())
    }

    pub(in crate::editor) fn split_save_snapshot_is_current(
        &self,
        path: &Path,
        text: &str,
    ) -> Option<bool> {
        let workspace = self.split_workspace.as_ref()?;
        Some(
            workspace
                .documents
                .get(path)
                .is_some_and(|document| document.dirty && document.buffer.matches_text(text)),
        )
    }

    pub(in crate::editor) fn propagate_split_saves(
        &mut self,
        saved: &[WorkspaceAutosaveDocument],
        cx: &mut Context<Self>,
    ) {
        if self.split_workspace.is_none() {
            return;
        }
        let groups = self.split_groups();
        for saved in saved {
            let version = crate::editor::persistence::file_content_version(&saved.markdown);
            if let Some(document) = self
                .split_workspace
                .as_mut()
                .and_then(|workspace| workspace.documents.get_mut(&saved.path))
            {
                if !document.buffer.matches_text(&saved.markdown) {
                    continue;
                }
                document.dirty = false;
                document.file_version = Some(version);
            }
            for group in &groups {
                group.update(cx, |editor, cx| {
                    let mut changed = false;
                    if let Some(tab) = editor
                        .workspace
                        .open_documents
                        .iter_mut()
                        .find(|tab| tab.path == saved.path)
                    {
                        tab.dirty = false;
                        tab.file_version = version;
                        tab.markdown = saved.markdown.clone();
                        changed = true;
                    }
                    if editor.active_document_path() == Some(&saved.path) {
                        editor.document_dirty = false;
                        editor.file_version = Some(version);
                    }
                    if changed {
                        cx.notify();
                    }
                });
            }
        }
    }

    pub(super) fn transfer_shared_history(&mut self, group: EntityId, cx: &mut Context<Self>) {
        let Some(target) = self
            .split_groups()
            .into_iter()
            .find(|editor| editor.entity_id() == group)
        else {
            return;
        };
        let Some(path) = target.read(cx).active_document_path().cloned() else {
            return;
        };
        let Some(owner) = self
            .split_workspace
            .as_ref()
            .and_then(|workspace| workspace.documents.get(&path))
            .and_then(|document| document.owner.upgrade())
        else {
            return;
        };
        if owner.entity_id() == group {
            return;
        }
        let history = owner.update(cx, |editor, cx| {
            editor.autosave_task = None;
            if editor.active_document_path() == Some(&path) {
                editor.finalize_pending_undo_capture(cx);
                Some((
                    editor.undo_history.clone(),
                    editor.redo_history.clone(),
                    editor.pending_undo_capture.clone(),
                ))
            } else {
                editor
                    .workspace
                    .open_documents
                    .iter()
                    .find(|tab| tab.path == path)
                    .and_then(|tab| tab.view.as_ref())
                    .and_then(|view| view.editing_state.as_ref())
                    .map(|state| {
                        (
                            state.undo_history.clone(),
                            state.redo_history.clone(),
                            state.pending_undo_capture.clone(),
                        )
                    })
            }
        });
        if let Some((undo, redo, pending)) = history {
            target.update(cx, |editor, cx| {
                editor.undo_history = undo;
                editor.redo_history = redo;
                editor.pending_undo_capture = pending;
                editor.finalize_pending_undo_capture(cx);
            });
        }
        if let Some(document) = self
            .split_workspace
            .as_mut()
            .and_then(|workspace| workspace.documents.get_mut(&path))
        {
            document.owner = target.downgrade();
        }
    }

    pub(in crate::editor) fn sync_shared_groups(
        &mut self,
        changed: EntityId,
        cx: &mut Context<Self>,
    ) {
        let groups = self.split_groups();
        if let Some(source) = groups.iter().find(|group| group.entity_id() == changed) {
            let untitled_changed = source.update(cx, |editor, _| {
                if editor.file_path.is_none()
                    && editor.split_projection_revision != editor.buffer.revision()
                {
                    editor.split_projection_revision = editor.buffer.revision();
                    true
                } else {
                    false
                }
            });
            if untitled_changed {
                self.document_revision = self.document_revision.wrapping_add(1);
            }
        }
        if self
            .split_workspace
            .as_ref()
            .is_some_and(|workspace| workspace.active == changed)
        {
            self.transfer_shared_history(changed, cx);
        }
        let Some(workspace) = self.split_workspace.as_mut() else {
            return;
        };
        let mut documents = std::mem::take(&mut workspace.documents);
        if let Some(source) = groups.iter().find(|group| group.entity_id() == changed) {
            if let Some(path) = source.read(cx).active_document_path().cloned() {
                let identity = source.read(cx).buffer.identity();
                let renamed = documents
                    .iter()
                    .find(|(previous, document)| {
                        **previous != path
                            && document.buffer.identity() == identity
                            && !source
                                .read(cx)
                                .workspace
                                .open_documents
                                .iter()
                                .any(|tab| tab.path == **previous)
                    })
                    .map(|(path, _)| path.clone());
                if let Some(previous) = renamed {
                    self.clear_external_change_conflict_for(&previous);
                    if let Some(document) = documents.remove(&previous) {
                        documents.insert(path.clone(), document);
                    }
                    for group in groups.iter().filter(|group| group.entity_id() != changed) {
                        group.update(cx, |editor, cx| {
                            for tab in &mut editor.workspace.open_documents {
                                if tab.path == previous {
                                    tab.path = path.clone();
                                }
                            }
                            if editor.active_document_path() == Some(&previous) {
                                editor.file_path =
                                    untitled_document_id(&path).is_none().then(|| path.clone());
                                if previous.parent() != path.parent() {
                                    editor.rebuild_image_runtimes(cx);
                                }
                            }
                            if editor.workspace.active_document.as_ref() == Some(&previous) {
                                editor.workspace.active_document = Some(path.clone());
                            }
                            cx.notify();
                        });
                    }
                }
                if !documents.contains_key(&path) {
                    let buffer = source.update(cx, |editor, _| editor.buffer.shared_view());
                    documents.insert(path, SharedDocument::new(buffer, source, cx));
                }
            }
        }
        for (path, document) in &mut documents {
            let changes = document.buffer.shared_changes();
            let writer = changes.last().map(|change| change.origin);
            let source = groups
                .iter()
                .find(|group| {
                    group.read(cx).active_document_path() == Some(path)
                        && writer.map_or(group.entity_id() == changed, |writer| {
                            group.read(cx).buffer.shared_writer_id() == writer
                        })
                })
                .cloned();
            if let Some(source) = &source {
                if writer.is_some() {
                    document.owner = source.downgrade();
                }
                if source.read(cx).buffer.identity() != document.buffer.identity() {
                    let same = source.read(cx).buffer.same_content(&document.buffer);
                    if same {
                        if let Some(buffer) = document.buffer.shared_handle() {
                            source.update(cx, |editor, _| editor.buffer = buffer);
                        }
                    } else {
                        document.buffer =
                            source.update(cx, |editor, _| editor.buffer.shared_view());
                        document.owner = source.downgrade();
                    }
                }
            }
            let state = document.owner.read_with(cx, |editor, _| {
                if editor.active_document_path() == Some(path) {
                    Some((editor.document_dirty, editor.file_version))
                } else {
                    editor
                        .workspace
                        .open_documents
                        .iter()
                        .find(|tab| tab.path == *path)
                        .map(|tab| (tab.dirty, Some(tab.file_version)))
                }
            });
            if let Ok(Some((dirty, version))) = &state {
                document.dirty = *dirty;
                document.file_version = *version;
            }
            let changes = document.buffer.shared_changes();
            let revision = document.buffer.revision();
            let identity = document.buffer.identity();
            let current = (identity, revision, document.buffer.file_shape());
            if document.synced != current {
                document.synced = current;
                self.document_revision = self.document_revision.wrapping_add(1);
            }
            for group in &groups {
                let active = group.read(cx).active_document_path() == Some(path);
                if !active {
                    group.update(cx, |editor, _| {
                        if let Some(tab) = editor
                            .workspace
                            .open_documents
                            .iter_mut()
                            .find(|tab| tab.path == *path)
                        {
                            tab.dirty = document.dirty;
                            tab.recovery_id = document.recovery_id;
                            if let Some(version) = document.file_version {
                                tab.file_version = version;
                            }
                        }
                    });
                    continue;
                }
                group.update(cx, |editor, cx| {
                    let mut refreshed = false;
                    if editor.buffer.identity() != identity {
                        let selection = editor.capture_source_selection_snapshot(cx);
                        if let Some(buffer) = document.buffer.shared_handle() {
                            editor.buffer = buffer;
                        }
                        editor.rebuild_document_from_buffer(cx);
                        editor.apply_selection_snapshot_in_current_mode(&selection, cx);
                        if !editor.split_active {
                            editor.pending_focus = None;
                            editor.pending_scroll_active_block_into_view = false;
                        }
                        refreshed = true;
                    } else if source
                        .as_ref()
                        .is_none_or(|source| source.entity_id() != group.entity_id())
                        && editor.split_projection_revision < revision
                    {
                        editor.apply_shared_projection_changes(&changes, cx);
                        refreshed = true;
                    }
                    editor.split_projection_revision = revision;
                    if editor.document_dirty != document.dirty
                        || editor.file_version != document.file_version
                    {
                        refreshed = true;
                    }
                    editor.recovery_id = document.recovery_id;
                    editor.document_dirty = document.dirty;
                    editor.file_version = document.file_version;
                    if let Some(tab) = editor
                        .workspace
                        .open_documents
                        .iter_mut()
                        .find(|tab| tab.path == *path)
                    {
                        tab.dirty = document.dirty;
                        tab.recovery_id = document.recovery_id;
                    }
                    if refreshed {
                        editor.document_revision = editor.document_revision.wrapping_add(1);
                        cx.notify();
                    }
                });
            }
            document.buffer.clear_shared_changes();
        }
        documents.retain(|path, _| {
            groups.iter().any(|group| {
                group
                    .read(cx)
                    .workspace
                    .open_documents
                    .iter()
                    .any(|tab| &tab.path == path)
            })
        });
        if let Some(workspace) = self.split_workspace.as_mut() {
            workspace.documents = documents;
        }
    }

    fn apply_shared_projection_changes(
        &mut self,
        changes: &[BufferChange],
        cx: &mut Context<Self>,
    ) {
        let changes: Vec<_> = changes
            .iter()
            .filter(|change| change.revision > self.split_projection_revision)
            .cloned()
            .collect();
        if changes.is_empty() {
            return;
        }
        // 共享字节已被另一视图修改，旧块的映射此时不能再切新缓冲区。
        let mut selection = self.last_selection_snapshot.clone();
        for change in &changes {
            let delta = change.new_len as i64 - change.old_range.len() as i64;
            let map = |offset: usize| {
                if offset <= change.old_range.start {
                    offset
                } else if offset >= change.old_range.end {
                    (offset as i64 + delta).max(0) as usize
                } else {
                    change.old_range.start + change.new_len
                }
            };
            selection.range = map(selection.range.start)..map(selection.range.end);
        }
        // 分屏重投影要尽量保留原块实体（复用叶子），撤销/重做那条路则不要，
        // 其余口径（受影响根的判定、区间平移、引用注册表刷新）只有一处实现。
        let applied = self.reproject_buffer_changes_regionally(&changes, true, cx);
        if !applied {
            self.rebuild_document_from_buffer(cx);
        }
        // 只同步发生变化的视图状态，后台分屏不能抢走当前输入焦点。
        self.apply_selection_snapshot_in_current_mode(&selection, cx);
        if !self.split_active {
            self.pending_focus = None;
            self.pending_scroll_active_block_into_view = false;
        }
        self.last_selection_snapshot = selection;
        self.last_selection_snapshot_source = None;
        self.split_projection_revision = self.buffer.revision();
    }
}
