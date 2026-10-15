use super::*;
use super::super::{DocumentBlockState, DocumentEditingState, DocumentRowLayout, ViewMode};

impl Editor {
    pub(crate) fn capture_document_editing_state(&self, cx: &App) -> DocumentEditingState {
        let tables: HashSet<_> = self
            .table_cells
            .values()
            .map(|binding| binding.table_block.entity_id())
            .collect();
        let blocks = self
            .document
            .visible_blocks()
            .iter()
            .enumerate()
            .filter_map(|(visible_index, entry)| {
                let metadata = self.document.row_spacing_at(visible_index);
                if metadata.heading_level.is_none()
                    && metadata.source_document_lines == 0
                    && !tables.contains(&entry.entity.entity_id())
                {
                    return None;
                }
                let block = entry.entity.read(cx);
                if !block.folded
                    && block.expanded_long_lines.is_empty()
                    && block.table_container_width.is_none()
                    && block.column_layout_memo.is_none()
                {
                    return None;
                }
                Some(DocumentBlockState {
                    visible_index,
                    kind: block.kind(),
                    folded: block.folded,
                    expanded_long_lines: block.expanded_long_lines.clone(),
                    table_container_width: block.table_container_width.map(f32::from),
                    column_layout_memo: block.column_layout_memo.clone(),
                })
            })
            .collect();
        DocumentEditingState {
            undo_history: self.undo_history.clone(),
            redo_history: self.redo_history.clone(),
            pending_undo_capture: self.pending_undo_capture.clone(),
            blocks,
        }
    }

    pub(crate) fn restore_document_editing_state(
        &mut self,
        state: &DocumentEditingState,
        layout: Option<&DocumentRowLayout>,
        same_projection: bool,
        restore_history: bool,
        cx: &mut Context<Self>,
    ) {
        let layout_matches = layout.is_some_and(|layout| {
            let theme = cx.global::<ThemeManager>().derived_theme(cx);
            let viewport_width = f32::from(self.scroll_handle.bounds().size.width).max(1.0);
            let width = if self.code_document || self.view_mode == ViewMode::Source {
                (viewport_width - theme.dimensions.editor_padding * 1.5).max(1.0)
            } else {
                Self::writing_column_width(viewport_width, &theme.dimensions, cx)
            };
            layout.width == width && std::sync::Arc::ptr_eq(&layout.theme, &theme)
        });
        let mut restored_fold = false;
        if same_projection {
            for saved in &state.blocks {
                let Some(entry) = self.document.visible_blocks().get(saved.visible_index) else {
                    continue;
                };
                if entry.entity.read(cx).kind() != saved.kind {
                    continue;
                }
                restored_fold |= saved.folded;
                entry.entity.update(cx, |block, cx| {
                    block.folded = saved.folded;
                    block.expanded_long_lines = saved.expanded_long_lines.clone();
                    block.shape_memo = None;
                    if layout_matches {
                        block.table_container_width = saved.table_container_width.map(px);
                        block.column_layout_memo = saved.column_layout_memo.clone();
                    } else if saved.table_container_width.is_some() {
                        let viewport_width = f32::from(self.scroll_handle.bounds().size.width);
                        if viewport_width > 0.0 {
                            let theme = cx.global::<ThemeManager>().derived_theme(cx);
                            // 输入变了要重测，但首轮仍应按正文视口而非整个窗口分列。
                            block.table_container_width = Some(px(block
                                .table_container_width_for_viewport(
                                    viewport_width,
                                    &theme.dimensions,
                                    cx,
                                )));
                        }
                    }
                    cx.notify();
                });
            }
        }
        if restored_fold {
            self.fold_state_version = self.fold_state_version.wrapping_add(1);
        }
        if restore_history {
            self.undo_history = state.undo_history.clone();
            self.redo_history = state.redo_history.clone();
            self.pending_undo_capture = state.pending_undo_capture.clone();
            self.finalize_pending_undo_capture(cx);
            // 切换标签结束连续输入组，回来的新输入不能并入离开前的编辑。
            if let Some(entry) = self.undo_history.last_mut()
                && matches!(
                    entry.kind,
                    crate::components::UndoCaptureKind::CoalescibleText
                        | crate::components::UndoCaptureKind::ImeComposition
                )
            {
                entry.kind = crate::components::UndoCaptureKind::NonCoalescible;
            }
        }
    }

    pub(crate) fn sync_workspace_after_document_path_change(&mut self, cx: &mut Context<Self>) {
        if let Some(path) = self.file_path.clone() {
            let previous = self.workspace.active_document.clone();
            if previous.as_ref() != Some(&path) {
                let markdown = self.document_text_for_save();
                let file_version = self
                    .file_version
                    .unwrap_or_else(|| crate::editor::persistence::file_content_version(&markdown));
                // 路径换了但显示的还是这一篇：这份现场归它，四个分支都按此刻的记录，
                // 不给旧标签留下「和当前文档对不上」的机会。
                let view = self.capture_document_view(&markdown, cx);
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
                        tab.view = Some(view.clone());
                    } else {
                        let tab = &mut self.workspace.open_documents[previous_index];
                        tab.path = path.clone();
                        tab.recovery_id = self.recovery_id;
                        tab.file_version = file_version;
                        tab.markdown = markdown;
                        tab.dirty = false;
                        tab.view = Some(view.clone());
                    }
                } else if let Some(current_index) = current_index {
                    let tab = &mut self.workspace.open_documents[current_index];
                    tab.recovery_id = self.recovery_id;
                    tab.file_version = file_version;
                    tab.markdown = markdown;
                    tab.dirty = false;
                    tab.view = Some(view.clone());
                } else {
                    self.workspace.open_documents.push(WorkspaceDocumentTab {
                        path: path.clone(),
                        recovery_id: self.recovery_id,
                        file_version,
                        markdown,
                        file_shape: self.buffer.file_shape(),
                        dirty: false,
                        preview: false,
                        view: Some(view),
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
        // 下面再点文件就自动置顶。重扫照旧会刷新内容。
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
        self.load_expanded_workspace_dirs(cx);
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
                file_shape: self.buffer.file_shape(),
                dirty: self.document_dirty,
                preview: false,
                view: None,
            });
        }
        self.workspace.active_document = Some(path);
    }

    /// 把活动文档的标签从「预览」转成固定（用户需求：编辑过就不再是临时窗口）。
    ///
    /// 调用点是 `document_dirty` 从 false 翻 true 的那一次（`finish_dirty`），所以一次
    /// 编辑会话只走一遍。转了固定之后，切走时它不再算「可替换的预览」，标签与内容都留着。
    pub(crate) fn pin_active_preview_tab(&mut self) {
        let Some(path) = self.file_path.as_deref() else {
            return;
        };
        if let Some(tab) = self
            .workspace
            .open_documents
            .iter_mut()
            .find(|tab| tab.path == path)
        {
            tab.preview = false;
        }
    }

    /// 记下这一篇的内容和阅读现场，写进它的标签：离开这一篇的入口用它（换标签、关
    /// 闭、外部改动重载），切回来按 `WorkspaceDocumentTab::view` 交还。
    pub(crate) fn snapshot_current_document(&mut self, cx: &App) {
        let markdown = self.document_text_for_save();
        let view = self.capture_document_view(&markdown, cx);
        self.write_current_document_tab(markdown, Some(view));
    }

    /// 只把这一篇的内容与脏位写进标签，不采阅读现场：自动保存用它。采一次要付
    /// 全文哈希、每行一条目的行测量、逐个块读实体和撤销栈的深拷贝，而这些在自动保
    /// 存之后没有任何人读（读它的是换标签那一步，那一步自己会重新采）。
    pub(crate) fn sync_current_document_content(&mut self) {
        let markdown = self.document_text_for_save();
        self.write_current_document_tab(markdown, None);
    }

    fn write_current_document_tab(&mut self, markdown: String, view: Option<DocumentView>) {
        let Some(path) = self.file_path.clone() else {
            return;
        };
        let file_shape = self.buffer.file_shape();
        let dirty = self.document_dirty;
        if let Some(tab) = self
            .workspace
            .open_documents
            .iter_mut()
            .find(|tab| tab.path == path)
        {
            tab.markdown = markdown;
            tab.file_shape = file_shape;
            tab.dirty = dirty;
            if let Some(view) = view {
                tab.view = Some(view);
            }
        } else {
            let file_version = self.file_version.unwrap_or_else(|| {
                crate::editor::persistence::file_content_version(&markdown)
            });
            self.workspace.open_documents.push(WorkspaceDocumentTab {
                path: path.clone(),
                recovery_id: self.recovery_id,
                file_version,
                markdown,
                file_shape,
                dirty,
                preview: false,
                view,
            });
        }
        self.workspace.active_document = Some(path);
    }

    pub(crate) fn dirty_workspace_documents(&mut self) -> Vec<WorkspaceAutosaveDocument> {
        self.sync_current_document_content();
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

    pub(crate) fn workspace_save_document_is_open(&self, id: uuid::Uuid, path: Option<&Path>) -> bool {
        (self.recovery_id == id && self.file_path.as_deref() == path)
            || self.workspace.open_documents.iter().any(|tab| {
                tab.recovery_id == id && Some(tab.path.as_path()) == path
            })
    }

    pub(crate) fn workspace_save_snapshot_is_current(&self, id: uuid::Uuid, path: &Path, text: &str) -> bool {
        if !self.workspace_save_document_is_open(id, Some(path)) {
            return false;
        }
        if self.file_path.as_deref() == Some(path) {
            self.document_dirty && self.buffer.matches_text(text)
        } else {
            self.workspace.open_documents.iter().any(|tab| {
                tab.recovery_id == id && tab.path == path && tab.dirty && tab.markdown == text
            })
        }
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
    /// 外部修改。
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
        self.workspace.external_change_conflict = Some((path.clone(), detail.clone()));
        self.report_workspace_file_error(detail, cx);
        // 只留一条红字是不够的：这一篇既不能继续写（会盖掉外部改动），用户又不知道
        // 该干什么。冲突一被记下就给一个能解除它的框（用户需求：重载 / 另存为）。
        self.show_external_change_conflict_modal(path, cx);
    }

    /// 外部改动冲突的解除入口。「重载」放弃本地编辑（先存恢复快照），「另存为」
    /// 保住当前内容写到别处；Esc /「继续编辑」什么都不做，冲突状态留着，
    /// 于是自动保存继续暂停、红字继续在。
    pub(crate) fn show_external_change_conflict_modal(
        &mut self,
        path: PathBuf,
        cx: &mut Context<Self>,
    ) {
        let strings = cx.global::<crate::i18n::I18nManager>().strings().clone();
        self.show_modal(
            ModalSpec {
                title: strings.external_change_title.clone().into(),
                detail: Some(
                    format!("{}\n\n{}", strings.external_change_message, path.display()).into(),
                ),
                buttons: vec![
                    strings.external_change_reload.clone().into(),
                    strings.external_change_save_as.clone().into(),
                    strings.unsaved_changes_cancel.clone().into(),
                ],
                default_index: 0,
                cancel_index: 2,
            },
            move |choice, editor, window, cx| match choice {
                0 => editor.reload_conflicted_document(&path, cx),
                1 => {
                    // 另存为作用于活动文档：冲突在后台标签上时先把它切到台前，
                    // 否则存出去的是另一篇。
                    if editor.file_path.as_deref() != Some(&path) {
                        editor.open_workspace_file_in_mode(
                            path.clone(),
                            WorkspaceOpenMode::Activate,
                            window,
                            cx,
                        );
                    }
                    editor.request_save_document_as(cx);
                }
                _ => {}
            },
            cx,
        );
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
        // 整篇渲染（LaTeX/mermaid 转 SVG、本地图读盘转 base64）是 CPU 密集，
        // 与 HTML/PDF/PNG 导出同属后台活：前台同步跑会冻结窗口（评审报告
        // 维度四建议 6）。剪贴板写回必须回主线程。
        let weak_editor = cx.entity().downgrade();
        let worker = cx.background_spawn(async move {
            crate::export::html::render_html_with_base_dir(
                &markdown,
                &theme,
                &title,
                base_dir.as_deref(),
            )
        });
        self.copy_html_task = Some(cx.spawn(
            async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
                let html = worker.await;
                if weak_editor
                    .update(cx, |_editor, cx| {
                        cx.write_to_clipboard(copy_as_html_clipboard_item(html));
                        cx.notify();
                    })
                    .is_err()
                {
                    // 编辑器实体已消失（窗口关掉）：剪贴板没处写，丢弃即可；
                    // 渲染 API 不带失败分支（内部错误都退化成可见占位），
                    // 没有需要送应用内模态的失败路径。
                }
            },
        ));
    }

    /// 「复制为 Markdown」：把选区的源码文本（无选区时整篇）原样写进剪贴板。
    /// 与「复制」两条不同的地方在内容来源：那一条走 gpui 自己的选区拷贝，
    /// 拿的是渲染之后的可见文本（`**加粗**` 只剩「加粗」）；这一条拿的是文件里的那几个字节。
    pub(crate) fn copy_as_markdown(&mut self, cx: &mut Context<Self>) -> bool {
        let markdown = self
            .selected_markdown_text(cx)
            .unwrap_or_else(|| self.current_document_source(cx));
        if markdown.is_empty() {
            return false;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(markdown));
        cx.notify();
        true
    }

}
