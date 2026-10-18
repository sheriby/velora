use super::*;
use anyhow::bail;

struct PreparedDocument {
    id: String,
    destination: Destination,
    edits: Vec<crate::ai::ProposedEdit>,
}

enum Destination {
    Current,
    Live(Entity<Editor>),
    Cached {
        editor: Entity<Editor>,
        path: PathBuf,
        version: u64,
        recovery: uuid::Uuid,
        new_document: bool,
    },
}

impl Editor {
    pub(in crate::editor) fn accept_ai_proposals(
        &mut self,
        document: Option<&str>,
        edit: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<()> {
        if !crate::ai::settings::AiFeature::enabled(cx) {
            bail!("{}", cx.global::<crate::i18n::I18nManager>().strings().ai_feature_disabled);
        }
        if self.is_read_only() {
            bail!(
                "{}",
                cx.global::<crate::i18n::I18nManager>()
                    .strings()
                    .ai_error_read_only_accept
            );
        }
        if self.split_workspace.is_some() {
            let group = self.active_split_editor().context("没有活动编辑器")?;
            let session = self.ai_session.take().context("AI 会话不存在")?;
            if session.saved.running {
                self.ai_session = Some(session);
                bail!("请先停止生成，再审阅修改");
            }
            let owner = cx.entity().downgrade();
            let handle = window.window_handle();
            let document = document.map(str::to_owned);
            cx.defer(move |cx| {
                let result = handle.update(cx, |_, window, cx| {
                    let (session, result) = group.update(cx, |editor, cx| {
                        editor.ai_session = Some(session);
                        let result =
                            editor.accept_ai_proposals(document.as_deref(), edit, window, cx);
                        (editor.ai_session.take(), result)
                    });
                    owner.update(cx, |editor, cx| {
                        editor.ai_session = session;
                        if let Err(error) = result {
                            editor.show_message_modal("AI", error.to_string(), cx);
                        }
                        editor.sync_shared_groups(group.entity_id(), cx);
                        cx.notify();
                    })
                });
                match result {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) | Err(error) => eprintln!("应用分屏 AI 修改失败：{error}"),
                }
            });
            return Ok(());
        }
        let session = self.ai_session.as_ref().context("AI 会话不存在")?;
        if session.saved.running {
            bail!("请先停止生成，再审阅修改");
        }
        if session.saved.workspace.read_only {
            bail!("阅读结果不能应用到正文");
        }
        let saved = session.saved.clone();
        let root = saved.workspace.root.as_deref();
        let current_id = ai_document_id(self.file_path.as_deref(), root, self.recovery_id);
        let ids: Vec<_> = saved
            .workspace
            .changes
            .proposals
            .keys()
            .filter(|id| document.is_none_or(|selected| selected == id.as_str()))
            .cloned()
            .collect();
        saved
            .workspace
            .changes
            .validate_review(&ids, edit.is_some())?;
        if ids.is_empty() {
            bail!("没有待接受的修改");
        }
        if edit.is_some() && ids.len() != 1 {
            bail!("请指定要接受的文档");
        }
        let groups = self.ai_editor_groups(cx);
        let mut staged = saved.workspace.changes.clone();
        let mut prepared = Vec::new();
        for id in ids {
            if saved
                .workspace
                .selection_scope
                .as_ref()
                .is_some_and(|scope| scope.document_id != id)
            {
                bail!("本任务只能修改原始选区");
            }
            let proposal = saved
                .workspace
                .changes
                .proposals
                .get(&id)
                .context("修改不存在")?;
            let bound_path = saved.bindings.get(&id).cloned();
            let is_current = id == current_id
                || bound_path
                    .as_ref()
                    .is_some_and(|path| self.active_document_path() == Some(path));
            let destination = if is_current {
                proposal.validate(&self.buffer.text())?;
                self.ai_verify_origin()?;
                if let Some(path) = &self.file_path {
                    self.ai_verify_other_windows(path, window, cx)?;
                }
                Destination::Current
            } else if bound_path.is_none()
                && id.contains(':')
                && let Some(group) = groups.iter().find(|group| {
                    let editor = group.read(cx);
                    editor.split_workspace.is_none()
                        && !editor.code_document
                        && super::super::ai_assistant::ai_document_id(
                            editor.file_path.as_deref(),
                            saved.workspace.root.as_deref(),
                            editor.recovery_id,
                        ) == id
                })
            {
                // untitled:/external: 不落磁盘相对路径，指向窗口里打开的
                // 编辑器组：找到持有它的组直接写缓冲区。已关闭时走不到这里，
                // 由下方相对路径分支的可行动报错兜底。
                if let Some(path) = id.strip_prefix("external:") {
                    self.ai_verify_other_windows(Path::new(path), window, cx)?;
                }
                let group = group.clone();
                group.read_with(cx, |editor, _| -> Result<()> {
                    proposal.validate(&editor.buffer.text())?;
                    editor.ai_verify_origin()
                })?;
                Destination::Live(group)
            } else if bound_path.is_none() && id.contains(':') {
                bail!(
                    "{}：{id}",
                    cx.global::<crate::i18n::I18nManager>()
                        .strings()
                        .ai_error_owner_closed
                );
            } else {
                let path = if let Some(path) = bound_path {
                    path
                } else {
                    // 写入路径直接用校验过的结果：取当前工作区根另算一份，
                    // 任务发起后切换工作区会把新文档落到别的目录。
                    saved.workspace.proposal_path(&id)?
                };
                self.ai_verify_other_windows(&path, window, cx)?;
                if let Some(group) = groups.iter().find(|group| {
                    group.entity_id() != cx.entity_id()
                        && group
                            .read(cx)
                            .active_document_path()
                            .is_some_and(|active| same_path(active, &path))
                }) {
                    let group = group.clone();
                    group.read_with(cx, |editor, _| -> Result<()> {
                        proposal.validate(&editor.buffer.text())?;
                        editor.ai_verify_origin()
                    })?;
                    Destination::Live(group)
                } else {
                    let cached = self.ai_prepare_cached_document(&path).or_else(|| {
                        groups
                            .iter()
                            .filter(|group| group.entity_id() != cx.entity_id())
                            .find_map(|group| group.read(cx).ai_prepare_cached_document(&path))
                    });
                    let is_new = saved.workspace.new_documents.contains(&id)
                        && !saved.bindings.contains_key(&id);
                    if is_new && path.exists() {
                        bail!("新文档目标已存在：{id}");
                    }
                    let (loaded, view, shape, version, recovery) =
                        if let Some((source, view, shape, version, recovery)) = cached {
                            if super::super::workspace::untitled_document_id(&path).is_none() {
                                super::super::persistence::verify_file_version(&path, version)?;
                            }
                            proposal.validate(&source)?;
                            (
                                super::super::encoding::LoadedDocument::from_text(source),
                                view,
                                shape,
                                version,
                                recovery,
                            )
                        } else if is_new {
                            proposal.validate("")?;
                            (
                                super::super::encoding::LoadedDocument::from_text(String::new()),
                                None,
                                None,
                                0,
                                uuid::Uuid::new_v4(),
                            )
                        } else {
                            let loaded = super::super::encoding::load_document(&path)?;
                            let normalized =
                                super::super::encoding::normalize_document_text(&loaded.text);
                            proposal.validate(&normalized)?;
                            let version =
                                super::super::persistence::file_content_version(&normalized);
                            (loaded, None, None, version, uuid::Uuid::new_v4())
                        };
                    let tab_path = if is_new {
                        let name = path
                            .file_name()
                            .context("新文档名称无效")?
                            .to_string_lossy();
                        PathBuf::from(format!("untitled:{recovery}/{name}"))
                    } else {
                        path.clone()
                    };
                    let file_path = super::super::workspace::untitled_document_id(&tab_path)
                        .is_none()
                        .then_some(path);
                    let editor = cx.new(|cx| {
                        let mut editor = Editor::from_loaded_document(cx, loaded, file_path);
                        editor.recovery_id = recovery;
                        editor.file_version = Some(version);
                        if let Some(shape) = shape {
                            editor.buffer.restore_file_shape(shape);
                        }
                        if let Some(view) = view {
                            if view.view_mode != editor.view_mode {
                                editor.toggle_view_mode(cx);
                            }
                            if let Some(state) = view.editing_state {
                                editor.restore_document_editing_state(&state, None, true, true, cx);
                            }
                        }
                        editor
                    });
                    Destination::Cached {
                        editor,
                        path: tab_path,
                        version,
                        recovery,
                        new_document: is_new,
                    }
                }
            };
            let edits = if let Some(index) = edit {
                vec![staged.accept_edit(&id, index, &proposal.base.source)?]
            } else {
                staged.proposals.remove(&id);
                proposal.edits()
            };
            if let Some(scope) = saved
                .workspace
                .selection_scope
                .as_ref()
                .filter(|scope| scope.document_id == id)
            {
                let mut pending = proposal.base.source.to_string();
                for edit in edits.iter().rev() {
                    if pending.get(edit.range.clone()).is_none() {
                        bail!("修改范围已失效");
                    }
                    pending.replace_range(edit.range.clone(), &edit.replacement);
                }
                scope.allowed_in(proposal.base.source.as_ref(), &pending)?;
            }
            prepared.push(PreparedDocument {
                id,
                destination,
                edits,
            });
        }
        let mut bindings = saved.bindings;
        let mut accepted = Vec::new();
        for prepared in prepared {
            let source = match prepared.destination {
                Destination::Current => {
                    self.apply_ai_source_edits(&prepared.edits, cx);
                    self.buffer.text()
                }
                Destination::Live(group) => group.update(cx, |editor, cx| {
                    editor.apply_ai_source_edits(&prepared.edits, cx);
                    editor.buffer.text()
                }),
                Destination::Cached {
                    editor,
                    path,
                    version,
                    recovery,
                    new_document,
                } => {
                    let (source, shape, view, buffer) = editor.update(cx, |editor, cx| {
                        editor.apply_ai_source_edits(&prepared.edits, cx);
                        let source = editor.buffer.text();
                        let view = editor.capture_document_view(&source, cx);
                        (
                            source,
                            editor.buffer.file_shape(),
                            view,
                            editor.buffer.shared_view(),
                        )
                    });
                    self.ai_store_prepared_tab(
                        path.clone(),
                        source.clone(),
                        shape,
                        version,
                        recovery,
                        view.clone(),
                    );
                    let coordinator = cx.entity_id();
                    for group in groups
                        .iter()
                        .filter(|group| group.entity_id() != coordinator)
                    {
                        group.update(cx, |editor, _| {
                            if editor.ai_prepare_cached_document(&path).is_some() {
                                editor.ai_store_prepared_tab(
                                    path.clone(),
                                    source.clone(),
                                    shape,
                                    version,
                                    recovery,
                                    view.clone(),
                                );
                            }
                        });
                    }
                    if let Some(parent) = self.split_parent.clone() {
                        let owner = cx.entity().downgrade();
                        if let Err(error) = parent.update(cx, |editor, _| {
                            editor.adopt_ai_shared_document(
                                path.clone(),
                                buffer,
                                owner,
                                version,
                                recovery,
                            )
                        }) {
                            eprintln!("同步 AI 文档缓存失败：{error}");
                        }
                    }
                    if new_document {
                        bindings.insert(prepared.id.clone(), path);
                    }
                    source
                }
            };
            accepted.push((prepared.id, source, prepared.edits));
        }
        if let Some(session) = self.ai_session.as_mut() {
            session.saved.workspace.changes = staged;
            session.review_dirty = true;
            session.saved.bindings = bindings;
            for (id, source, edits) in accepted {
                if let Some(scope) = session
                    .saved
                    .workspace
                    .selection_scope
                    .as_mut()
                    .filter(|scope| scope.document_id == id)
                {
                    for edit in &edits {
                        scope.range.end = scope
                            .range
                            .end
                            .saturating_sub(edit.range.len())
                            .saturating_add(edit.replacement.len());
                    }
                    scope.version = super::super::persistence::file_content_version(&source);
                }
                session.saved.workspace.new_documents.remove(&id);
                session.saved.workspace.live_documents.insert(id.clone());
                if let Some(snapshot) = session.saved.workspace.documents.get_mut(&id) {
                    snapshot.source = source.into();
                }
            }
        }
        if self
            .ai_session
            .as_ref()
            .is_some_and(|session| session.saved.workspace.changes.proposals.is_empty())
        {
            self.close_ai_review_surface(cx);
        }
        self.schedule_autosave(cx);
        self.persist_ai_session(cx);
        cx.notify();
        Ok(())
    }

    fn ai_editor_groups(&self, cx: &App) -> Vec<Entity<Editor>> {
        self.split_parent
            .as_ref()
            .and_then(|parent| parent.read_with(cx, |editor, _| editor.split_groups()).ok())
            .unwrap_or_else(|| self.split_groups())
    }

    fn ai_verify_origin(&self) -> Result<()> {
        if let (Some(path), Some(version)) = (&self.file_path, self.file_version) {
            super::super::persistence::verify_file_version(path, version)?;
        }
        Ok(())
    }

    fn ai_verify_other_windows(&self, path: &Path, window: &Window, cx: &App) -> Result<()> {
        for handle in cx
            .windows()
            .into_iter()
            .filter(|handle| handle.window_id() != window.window_handle().window_id())
        {
            let Some(handle) = crate::ui::window_content::<Editor>(handle, cx) else {
                continue;
            };
            let conflict = handle.read_with(cx, |editor, cx| {
                let dirty = |editor: &Editor| {
                    (editor
                        .file_path
                        .as_ref()
                        .is_some_and(|current| same_path(current, path))
                        && editor.document_dirty)
                        || editor
                            .ai_cached_document_sources()
                            .iter()
                            .any(|(candidate, _)| {
                                same_path(candidate, path)
                                    && editor
                                        .cached_tab_content_for_path(candidate, cx)
                                        .is_some_and(|(_, dirty)| dirty)
                            })
                };
                dirty(editor)
                    || editor
                        .split_groups()
                        .iter()
                        .any(|group| dirty(group.read(cx)))
            })?;
            if conflict {
                bail!("其他窗口正在编辑此文档，请先处理冲突：{}", path.display());
            }
        }
        Ok(())
    }

    pub(in crate::editor) fn apply_ai_source_edits(
        &mut self,
        edits: &[crate::ai::ProposedEdit],
        cx: &mut Context<Self>,
    ) {
        self.prepare_undo_capture(crate::components::UndoCaptureKind::NonCoalescible, cx);
        let mut landmark = None;
        for edit in edits.iter().rev() {
            let applied = self.buffer.edit(edit.range.clone(), &edit.replacement);
            // 逆序落笔，最后一轮是文档序最靠前的编辑：接受后光标落在首处
            // 修改之后，而不是随整树重建退回文档开头。
            landmark = Some(applied.new_range.end);
            self.record_buffer_edit(applied);
        }
        self.rebuild_document_from_buffer(cx);
        if let Some(landmark) = landmark {
            self.jump_to_document_search_range(landmark..landmark, cx);
        }
        self.mark_dirty_written_back(cx);
        self.finalize_pending_undo_capture(cx);
    }
}

fn same_path(left: &Path, right: &Path) -> bool {
    left == right
        || match (left.canonicalize(), right.canonicalize()) {
            (Ok(left), Ok(right)) => left == right,
            _ => false,
        }
}
