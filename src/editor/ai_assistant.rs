use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use anyhow::{Context as _, Result};
use gpui::*;

use super::Editor;
use crate::ai::runtime::{RunControl, RunEvent, RunOutput, RunRequest};
use crate::ai::session::{ChatMessage, SavedSession, SessionKind, TargetSnapshot};
use crate::ai::settings::{AiFeature, AiSettings};
use crate::ai::workspace::AiWorkspace;
use crate::ai::{AiCommand, DocumentSnapshot};
use crate::components::TextField;

mod apply;
mod context;
mod experience;
pub(in crate::editor) mod inline_review;
mod render;
mod workbench;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum PanelMode {
    Task,
    History,
    Skills,
    Instruction,
    Context,
    Options,
}

pub(super) struct AiSession {
    document: uuid::Uuid,
    panel_navigation: bool,
    pub(super) saved: SavedSession,
    input: Entity<TextField>,
    language: Entity<TextField>,
    instruction_name: Entity<TextField>,
    instruction_body: Entity<TextField>,
    context_input: Entity<TextField>,
    settings: AiSettings,
    visible: bool,
    expanded: bool,
    mode: PanelMode,
    command: AiCommand,
    origin: Point<Pixels>,
    error: Option<String>,
    activity: String,
    generation: u64,
    control: RunControl,
    task: Option<Task<()>>,
    stream_index: Option<usize>,
    scroll: ScrollHandle,
    review_focus: FocusHandle,
    panel_focus: FocusHandle,
    panel_bounds: std::rc::Rc<std::cell::Cell<Bounds<Pixels>>>,
    panel_drag: Option<Point<Pixels>>,
    pub(in crate::editor) conversation: Option<Entity<Editor>>,
    review_open: bool,
    // 标签重载会重建缓冲区，使用稳定的恢复标识区分暂离与主动关闭。
    review_navigation: Option<uuid::Uuid>,
    history_files: Vec<(PathBuf, String, SessionKind)>,
    skills: crate::ai::skills::SkillCatalog,
    context_candidates: Vec<String>,
    settings_saving: bool,
    // 设置单独编辑，仅在配置版本变化时读盘，避免在每帧渲染里读取配置。
    settings_revision: u64,
    review: Option<experience::ReviewDocument>,
    review_dirty: bool,
}

impl AiSession {
    #[cfg(test)]
    pub(in crate::editor) fn control_for_test(&self) -> RunControl {
        self.control.clone()
    }
}

impl Drop for AiSession {
    fn drop(&mut self) {
        self.control.cancel.cancel();
        if self.saved.messages.is_empty() && self.saved.workspace.changes.proposals.is_empty() {
            return;
        }
        self.saved.interrupted |= self.saved.running;
        self.saved.running = false;
        self.saved.sequence = self.saved.sequence.saturating_add(1);
        if let Err(error) = self.saved.path().and_then(|path| self.saved.save_at(&path)) {
            eprintln!("关闭窗口时保存 AI 会话失败：{error}");
        }
    }
}

enum WorkerEvent {
    Runtime(RunEvent),
    Refresh {
        id: String,
        response: mpsc::SyncSender<std::result::Result<Option<DocumentSnapshot>, String>>,
    },
    Finished(std::result::Result<RunOutput, String>),
}

impl Editor {
    pub(in crate::editor) fn sync_ai_availability(&mut self, cx: &mut Context<Self>) {
        if self.ai_result_preview
            || self.split_parent.is_some()
            || crate::ai::settings::AiFeature::enabled(cx)
        {
            return;
        }
        if self.ai_session.is_some() {
            self.stop_ai_task(cx);
            drop(self.ai_session.take());
        }
        self.ai_task_sessions.clear();
        self.ai_surface_visible.set(false);
    }

    pub(in crate::editor) fn dismiss_ai_panel(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if let Some(session) = self
            .ai_session
            .as_mut()
            .filter(|session| session.visible && session.mode != PanelMode::Task)
        {
            session.mode = PanelMode::Task;
            session
                .input
                .update(cx, |input, cx| input.focus(window, cx));
            cx.notify();
            return true;
        }
        let focused = self.ai_interaction_has_focus(window, cx);
        if focused {
            if self
                .ai_session
                .as_ref()
                .is_some_and(|session| session.visible)
            {
                self.hide_ai_panel(cx);
            } else if let Some(session) = self.ai_session.as_mut() {
                session.review_open = false;
                cx.notify();
            }
        }
        focused
    }

    fn hide_ai_panel(&mut self, cx: &mut Context<Self>) {
        if let Some(session) = self.ai_session.as_mut() {
            session.visible = false;
            session.panel_drag = None;
        }
        if let Some(group) = self.active_split_editor() {
            group.update(cx, |editor, cx| {
                editor.pending_focus = editor.active_entity_id;
                cx.notify();
            });
        } else {
            self.restore_focus_after_overlay(cx);
        }
        cx.notify();
    }
    fn ai_session_mut(&mut self, id: &str) -> Option<&mut AiSession> {
        self.ai_session
            .as_mut()
            .filter(|session| session.saved.id == id)
            .or_else(|| {
                self.ai_task_sessions
                    .iter_mut()
                    .find(|session| session.saved.id == id)
            })
    }

    fn activate_ai_document_session(&mut self, document: uuid::Uuid) {
        if self
            .ai_session
            .as_ref()
            .is_some_and(|session| session.document == document)
        {
            return;
        }
        if let Some(index) = self
            .ai_task_sessions
            .iter()
            .rposition(|session| session.document == document)
        {
            let session = self.ai_task_sessions.remove(index);
            if let Some(previous) = self.ai_session.replace(session) {
                self.ai_task_sessions.push(previous);
            }
        }
    }

    pub(in crate::editor) fn sync_ai_task_navigation(&mut self, cx: &App) {
        if self.ai_result_preview || self.split_parent.is_some() {
            return;
        }
        let current = self.ai_review_navigation_origin(cx);
        if self.ai_session.as_ref().is_some_and(|session| {
            session.expanded && session.saved.kind == SessionKind::Conversation
        }) {
            return;
        }
        if self
            .ai_session
            .as_ref()
            .is_some_and(|session| session.document != current)
        {
            self.suspend_ai_review_for_navigation(
                self.ai_session
                    .as_ref()
                    .map_or(current, |session| session.document),
            );
        }
        self.activate_ai_document_session(current);
        if let Some(session) = self.ai_session.as_mut()
            && session.document == current
            && session.panel_navigation
        {
            session.visible = true;
            session.panel_navigation = false;
        }
    }

    fn ai_review_navigation_origin(&self, cx: &App) -> uuid::Uuid {
        self.active_split_editor()
            .map_or(self.recovery_id, |group| group.read(cx).recovery_id)
    }

    fn suspend_ai_review_for_navigation(&mut self, origin: uuid::Uuid) {
        if let Some(session) = self
            .ai_session
            .as_mut()
            .filter(|session| session.document == origin)
        {
            if !session.expanded || session.saved.kind != SessionKind::Conversation {
                session.panel_navigation |= session.visible && session.expanded;
                session.visible = false;
                session.panel_drag = None;
            }
            if session.review_open
                || (session.review.is_none()
                    && (session.saved.running
                        || !session.saved.workspace.changes.proposals.is_empty())
                    && session.review_navigation.is_none())
            {
                session.review_navigation = Some(origin);
            }
            session.review_open = false;
        }
    }

    pub(super) fn sync_ai_review_navigation(&mut self, cx: &App) {
        let Some(destination) = self
            .ai_session
            .as_ref()
            .and_then(|session| session.review_navigation)
        else {
            return;
        };
        let current = self.ai_review_navigation_origin(cx);
        if let Some(session) = self.ai_session.as_mut() {
            if session.review.is_none()
                && !session.saved.running
                && session.saved.workspace.changes.proposals.is_empty()
            {
                session.review_navigation = None;
            } else if destination == current {
                session.review_navigation = None;
                session.review_open = session.review.is_some();
            } else {
                session.review_open = false;
            }
        }
    }

    pub(in crate::editor) fn leave_ai_review_for_document_navigation(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        let origin = self.ai_review_navigation_origin(cx);
        self.suspend_ai_review_for_navigation(origin);
        if !self.split_initializing
            && let Some(parent) = self.split_parent.clone()
        {
            cx.defer(move |cx| {
                if let Err(error) = parent.update(cx, |editor, cx| {
                    editor.suspend_ai_review_for_navigation(origin);
                    cx.notify();
                }) {
                    eprintln!("暂存宿主审阅状态失败：{error}");
                }
            });
        }
        cx.notify();
    }

    fn show_ai_context(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self
            .ai_session
            .as_mut()
            .filter(|session| !session.saved.running)
        else {
            return;
        };
        session.mode = PanelMode::Context;
        let id = session.saved.id.clone();
        let mut workspace = session.saved.workspace.clone();
        workspace.scope.clear();
        let load = cx.background_spawn(async move {
            let listing = workspace.call("list_documents", &serde_json::json!({}))?;
            Ok::<_, anyhow::Error>(
                listing
                    .get("documents")
                    .and_then(serde_json::Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(serde_json::Value::as_str)
                    .map(str::to_owned)
                    .collect::<Vec<_>>(),
            )
        });
        cx.spawn(async move |owner, cx| {
            let result = load.await;
            if let Err(error) = owner.update(cx, |editor, cx| {
                if let Some(session) = editor.ai_session_mut(&id) {
                    match result {
                        Ok(paths) => session.context_candidates = paths,
                        Err(error) => session.error = Some(error.to_string()),
                    }
                    cx.notify();
                }
            }) {
                eprintln!("更新 AI 资料列表失败：{error}");
            }
        })
        .detach();
        cx.notify();
    }

    fn add_ai_context(&mut self, path: Option<String>, cx: &mut Context<Self>) {
        let Some(session) = self.ai_session.as_mut() else {
            return;
        };
        let typed = session.context_input.read(cx).value().trim().to_owned();
        let paths = if let Some(path) = path {
            if session.saved.references.contains(&path) {
                session
                    .saved
                    .references
                    .retain(|selected| selected != &path);
                cx.notify();
                return;
            }
            Ok(vec![path])
        } else {
            crate::ai::context::explicit_references(&if typed.starts_with('@') {
                typed
            } else {
                format!("@\"{typed}\"")
            })
        };
        match paths {
            Ok(paths) => {
                for path in paths {
                    if !session.saved.references.contains(&path) {
                        session.saved.references.push(path);
                    }
                }
                session
                    .context_input
                    .update(cx, |input, cx| input.set_value("", cx));
            }
            Err(error) => session.error = Some(error.to_string()),
        }
        cx.notify();
    }
    fn apply_ai_review(
        &mut self,
        document: Option<String>,
        edit: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Err(error) = self.accept_ai_proposals(document.as_deref(), edit, window, cx) {
            self.show_message_modal("AI", error.to_string(), cx);
        }
    }

    fn reject_ai_review(&mut self, id: &str, edit: Option<usize>, cx: &mut Context<Self>) {
        if let Some(session) = self.ai_session.as_mut() {
            if session.saved.running {
                return;
            }
            session.review_dirty = true;
            if let Some(index) = edit {
                if let Err(error) = session.saved.workspace.changes.reject_edit(id, index) {
                    session.error = Some(error.to_string());
                }
            } else {
                session.saved.workspace.changes.invalidate_dependents(id);
                session.saved.workspace.changes.proposals.remove(id);
            }
            if !session.saved.workspace.changes.proposals.contains_key(id)
                && session.saved.workspace.new_documents.remove(id)
            {
                session.saved.workspace.documents.remove(id);
            }
        }
        if let Some(session) = self
            .ai_session
            .as_mut()
            .filter(|session| session.saved.workspace.changes.proposals.is_empty())
        {
            session.review_open = false;
        }
        self.persist_ai_session(cx);
        cx.notify();
    }

    fn retry_ai_task(&mut self, cx: &mut Context<Self>) {
        if let Some(session) = self.ai_session.as_mut() {
            if let Some(prompt) = session
                .saved
                .messages
                .iter()
                .rev()
                .find(|message| message.role == "user")
                .map(|message| message.text.clone())
            {
                session
                    .input
                    .update(cx, |input, cx| input.set_value(&prompt, cx));
            }
        }
        if let Some(session) = self.ai_session.as_mut()
            && session.saved.kind == SessionKind::QuickCommand
        {
            session.saved.history.clear();
            session.saved.messages.clear();
            session.saved.workspace.discard_proposals();
        }
        self.send_ai_task(cx);
    }

    fn open_ai_source(
        &mut self,
        record: crate::ai::workspace::SourceRecord,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.ai_session.as_ref() else {
            return;
        };
        let root = session.saved.workspace.root.clone();
        if let Some(session) = self.ai_session.as_mut() {
            session.review_open = false;
            session.review_navigation = None;
        }
        if let Some(group) = self.active_split_editor() {
            let handle = window.window_handle();
            cx.defer(move |cx| {
                if let Err(error) = handle.update(cx, |_, window, cx| {
                    group.update(cx, |editor, cx| {
                        editor.navigate_ai_source(record, root, window, cx)
                    })
                }) {
                    eprintln!("定位 AI 引用失败：{error}");
                }
            });
        } else {
            self.navigate_ai_source(record, root, window, cx);
        }
    }

    fn navigate_ai_source(
        &mut self,
        record: crate::ai::workspace::SourceRecord,
        root: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let current_id =
            ai_document_id(self.file_path.as_deref(), root.as_deref(), self.recovery_id);
        if current_id != record.path {
            let Some(root) = root else {
                self.show_message_modal("AI", "引用的未保存文档已关闭", cx);
                return;
            };
            match root.join(&record.path).canonicalize() {
                Ok(path) if path.starts_with(&root) => self.open_workspace_file_in_mode(
                    crate::file_url::canonical_path(&path),
                    super::workspace::WorkspaceOpenMode::Pinned,
                    window,
                    cx,
                ),
                _ => {
                    self.show_message_modal("AI", "引用文档已不存在或超出工作区", cx);
                    return;
                }
            }
            if ai_document_id(self.file_path.as_deref(), Some(&root), self.recovery_id)
                != record.path
            {
                return;
            }
        }
        match record.locate(&self.buffer.text()) {
            Ok(range) => self.jump_to_document_search_range(range, cx),
            Err(error) => self.show_message_modal("AI", error.to_string(), cx),
        }
    }

    fn delete_ai_history(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .ai_session
            .as_ref()
            .and_then(|session| session.saved.path().ok())
            .as_ref()
            == Some(&path)
        {
            self.new_ai_task(window, cx);
        }
        self.ai_task_sessions
            .retain(|session| session.saved.path().ok().as_ref() != Some(&path));
        let delete = cx.background_spawn(async move { SavedSession::delete_at(&path) });
        cx.spawn(async move |owner, cx| {
            let result = delete.await;
            if let Err(error) = owner.update(cx, |editor, cx| match result {
                Ok(()) => editor.show_ai_history(cx),
                Err(error) => {
                    if let Some(session) = editor.ai_session.as_mut() {
                        session.error = Some(error.to_string());
                    }
                    cx.notify();
                }
            }) {
                eprintln!("清理 AI 会话失败：{error}");
            }
        })
        .detach();
    }

    fn new_ai_task(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let expanded = self
            .ai_session
            .as_ref()
            .is_some_and(|session| session.expanded);
        self.open_ai_assistant(AiCommand::Ask, window, cx);
        if let Some(session) = self.ai_session.as_mut() {
            session.expanded = expanded;
        }
        cx.notify();
    }

    fn show_ai_history(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.ai_session.as_mut() else {
            return;
        };
        session.mode = PanelMode::History;
        let id = session.saved.id.clone();
        let root = session.saved.workspace.root.clone();
        let retained = self
            .ai_task_sessions
            .iter()
            .rev()
            .filter(|session| session.saved.workspace.root == root)
            .filter_map(|session| {
                let path = match session.saved.path() {
                    Ok(path) => path,
                    Err(error) => {
                        eprintln!("读取 AI 任务路径失败：{error}");
                        return None;
                    }
                };
                let title = session
                    .saved
                    .messages
                    .iter()
                    .find(|message| message.role == "user")
                    .map(|message| message.text.chars().take(60).collect::<String>())
                    .unwrap_or_else(|| session.input.read(cx).value().chars().take(60).collect());
                Some((
                    path,
                    if title.is_empty() {
                        session.saved.id.clone()
                    } else {
                        title
                    },
                    session.saved.kind,
                ))
            })
            .collect::<Vec<_>>();
        let load = cx.background_spawn(async move {
            let directory = SavedSession::directory(root.as_deref())?;
            if !directory.exists() {
                return Ok(retained);
            }
            let mut files = std::fs::read_dir(directory)?
                .map(|entry| entry.map(|entry| entry.path()))
                .collect::<std::io::Result<Vec<_>>>()?;
            files.retain(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "json")
            });
            files.sort_by_key(|path| {
                std::cmp::Reverse(
                    path.metadata()
                        .and_then(|metadata| metadata.modified())
                        .ok(),
                )
            });
            files.retain(|path| !retained.iter().any(|(retained, _, _)| retained == path));
            files.truncate(20);
            let mut summaries = retained;
            for path in files {
                let (title, kind) = match SavedSession::load_at(&path, root.as_deref()) {
                    Ok(saved) => (
                        saved
                            .messages
                            .iter()
                            .find(|message| message.role == "user")
                            .map(|message| message.text.chars().take(60).collect::<String>())
                            .filter(|title| !title.is_empty())
                            .unwrap_or_else(|| saved.id.clone()),
                        saved.kind,
                    ),
                    Err(error) => (
                        format!(
                            "{} · {error}",
                            path.file_stem().unwrap_or_default().to_string_lossy()
                        ),
                        SessionKind::Conversation,
                    ),
                };
                summaries.push((path, title, kind));
            }
            Ok::<_, anyhow::Error>(summaries)
        });
        cx.spawn(async move |owner, cx| {
            let result = load.await;
            if let Err(error) = owner.update(cx, |editor, cx| {
                if let Some(session) = editor.ai_session_mut(&id) {
                    match result {
                        Ok(files) => session.history_files = files,
                        Err(error) => session.error = Some(error.to_string()),
                    }
                    cx.notify();
                }
            }) {
                eprintln!("读取 AI 会话列表失败：{error}");
            }
        })
        .detach();
        cx.notify();
    }

    fn restore_ai_history(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self
            .ai_task_sessions
            .iter()
            .position(|session| session.saved.path().ok().as_ref() == Some(&path))
        {
            let destination =
                self.ai_task_sessions
                    .get(index)
                    .filter(|session| session.saved.kind == SessionKind::QuickCommand)
                    .and_then(|session| {
                        session.saved.target.as_ref().and_then(|target| {
                            target.path.clone().or_else(|| {
                                session.saved.bindings.get(&target.document_id).cloned()
                            })
                        })
                    });
            if let Some(destination) = destination {
                self.open_workspace_file(destination, window, cx);
            }
            let mut session = self.ai_task_sessions.remove(index);
            session.visible = true;
            session.expanded = true;
            session.panel_navigation = false;
            session.mode = PanelMode::Task;
            if let Some(previous) = self.ai_session.replace(session) {
                self.ai_task_sessions.push(previous);
            }
            cx.notify();
            return;
        }
        self.stop_ai_task(cx);
        let Some(session) = self.ai_session.as_ref() else {
            return;
        };
        let id = session.saved.id.clone();
        let generation = session.generation;
        let root = session.saved.workspace.root.clone();
        let load =
            cx.background_spawn(async move { SavedSession::load_at(&path, root.as_deref()) });
        cx.spawn(async move |owner, cx| {
            let loaded = load.await;
            if let Err(error) = owner.update(cx, |editor, cx| {
                if let Some(session) = editor
                    .ai_session_mut(&id)
                    .filter(|session| session.generation == generation)
                {
                    match loaded {
                        Ok(saved) => {
                            session.saved = saved;
                            session.expanded = true;
                            session.visible = true;
                            session.panel_navigation = false;
                            session.command = session
                                .saved
                                .prompt_profile
                                .as_ref()
                                .map_or(AiCommand::Ask, |profile| profile.command);
                            session
                                .input
                                .update(cx, |input, cx| input.set_value("", cx));
                            session.mode = PanelMode::Task;
                            session.error = None;
                            session.stream_index = None;
                            session.conversation = None;
                            session.review = None;
                            session.review_navigation = None;
                            session.review_dirty = true;
                        }
                        Err(error) => session.error = Some(error.to_string()),
                    }
                    editor.refresh_ai_preview(cx);
                    cx.notify();
                }
            }) {
                eprintln!("恢复 AI 会话失败：{error}");
            }
        })
        .detach();
    }

    fn save_ai_instruction(&mut self, cx: &mut Context<Self>) {
        if let Some(session) = self.ai_session.as_mut() {
            let name = session.instruction_name.read(cx).value().trim().to_owned();
            let prompt = session.instruction_body.read(cx).value().trim().to_owned();
            if name.is_empty() || prompt.is_empty() {
                session.error = Some("请输入指令名称和内容".into());
            } else {
                let mut settings = session.settings.clone();
                if let Some(instruction) = settings
                    .instructions
                    .iter_mut()
                    .find(|instruction| instruction.name == name)
                {
                    instruction.prompt = prompt;
                } else {
                    settings
                        .instructions
                        .push(crate::ai::settings::CustomInstruction { name, prompt });
                }
                self.update_ai_settings(settings, cx);
            }
        }
        cx.notify();
    }
    pub(crate) fn on_open_ai_assistant(
        &mut self,
        _: &crate::components::OpenAiAssistant,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(parent) = self.split_parent.clone() {
            let handle = window.window_handle();
            cx.defer(move |cx| {
                match handle.update(cx, |_, window, cx| {
                    parent.update(cx, |editor, cx| {
                        editor.on_open_ai_assistant(
                            &crate::components::OpenAiAssistant,
                            window,
                            cx,
                        );
                    })
                }) {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) | Err(error) => eprintln!("切换宿主 AI 面板失败：{error}"),
                }
            });
            return;
        }
        self.open_ai_assistant(AiCommand::Ask, window, cx);
    }

    pub(crate) fn open_ai_assistant(
        &mut self,
        command: AiCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !crate::ai::settings::AiFeature::enabled(cx) {
            return;
        }
        if let Some(group) = self.active_split_editor() {
            let handle = window.window_handle();
            cx.defer(move |cx| {
                if let Err(error) = handle.update(cx, |_, window, cx| {
                    group.update(cx, |editor, cx| {
                        editor.open_ai_assistant(command, window, cx)
                    })
                }) {
                    eprintln!("在活动分屏打开 AI 失败：{error}");
                }
            });
            return;
        }
        if self.code_document {
            return;
        }
        if command.requires_selection() && !self.has_text_selection(cx) {
            self.show_message_modal("AI", "请先选择需要处理的文字", cx);
            return;
        }
        self.overlay_focus_restore_target = self.focused_edit_target_entity_id(window, cx);
        let root = self.workspace_root_path().map(PathBuf::from).or_else(|| {
            self.file_path
                .as_ref()
                .and_then(|path| path.parent().map(PathBuf::from))
        });
        let root = root.and_then(|root| root.canonicalize().ok());
        let source = self.buffer.text();
        let range = self.capture_source_selection_snapshot(cx).range;
        let target = TargetSnapshot {
            document_id: ai_document_id(
                self.file_path.as_deref(),
                root.as_deref(),
                self.recovery_id,
            ),
            path: self.file_path.clone(),
            selection: source.get(range.clone()).unwrap_or_default().to_owned(),
            version: super::persistence::file_content_version(&source),
            range,
            identity: self.buffer.identity(),
        };
        let document = DocumentSnapshot {
            id: target.document_id.clone(),
            source,
        };
        let origin = self
            .selection_screen_bounds(cx)
            .map(|bounds| point(bounds.left(), bounds.bottom() + px(12.0)))
            .unwrap_or_else(|| {
                point(
                    (window.viewport_size().width - px(620.0)) * 0.5,
                    (window.viewport_size().height - px(220.0)) * 0.45,
                )
            });
        self.context_menu = None;
        self.selection_toolbar = None;
        let owner_document = self.recovery_id;
        if let Some(parent) = self.split_parent.clone() {
            let handle = window.window_handle();
            cx.defer(move |cx| {
                match handle.update(cx, |_, window, cx| {
                    parent.update(cx, |editor, cx| {
                        editor.open_ai_target(
                            command,
                            owner_document,
                            target,
                            document,
                            root,
                            origin,
                            window,
                            cx,
                        )
                    })
                }) {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) | Err(error) => eprintln!("打开分屏 AI 助手失败：{error}"),
                }
            });
            return;
        }
        self.open_ai_target(
            command,
            owner_document,
            target,
            document,
            root,
            origin,
            window,
            cx,
        );
    }

    fn open_ai_target(
        &mut self,
        command: AiCommand,
        owner_document: uuid::Uuid,
        target: TargetSnapshot,
        document: DocumentSnapshot,
        root: Option<PathBuf>,
        origin: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.window_handle = Some(window.window_handle());
        let mut workspace = match AiWorkspace::new(root.clone(), vec![document]) {
            Ok(workspace) => workspace,
            Err(error) => {
                self.show_message_modal("AI", error.to_string(), cx);
                return;
            }
        };
        workspace
            .live_documents
            .extend(self.ai_window_live_ids(root.as_deref(), cx));
        let loaded = AiSettings::load();
        let error = loaded.as_ref().err().map(ToString::to_string);
        let settings = loaded.unwrap_or_default();
        workspace.excluded_paths = settings.excluded_paths.clone();
        let mut saved = SavedSession::new(workspace);
        saved.kind = if command == AiCommand::Ask {
            SessionKind::Conversation
        } else {
            SessionKind::QuickCommand
        };
        saved.target = Some(target);
        saved.endpoint_id = settings
            .active_endpoint()
            .map(|endpoint| endpoint.id.clone())
            .unwrap_or_default();
        let strings = cx.global::<crate::i18n::I18nManager>().strings().clone();
        let owner = cx.entity().downgrade();
        let input = cx.new(|cx| {
            TextField::new(strings.ai_custom_placeholder.clone(), cx)
                .multiline(1, 4)
                .on_enter(move |_, _, cx| {
                    let owner = owner.clone();
                    let input_id = cx.entity_id();
                    cx.defer(move |cx| {
                        if let Err(error) = owner.update(cx, |editor, cx| {
                            if editor
                                .ai_session
                                .as_ref()
                                .is_some_and(|session| session.input.entity_id() == input_id)
                            {
                                editor.send_ai_task(cx);
                            }
                        }) {
                            eprintln!("发送 AI 指令失败：{error}");
                        }
                    });
                })
        });
        let mut directories = Vec::new();
        if let Ok(path) = crate::ai::settings::settings_path() {
            directories.push((path.with_file_name("ai").join("skills"), "user"));
        }
        if let Some(root) = &root {
            directories.push((root.join(".velora").join("skills"), "workspace"));
        }
        if let Some(previous) = self.ai_session.take() {
            self.ai_task_sessions.push(previous);
        }
        self.ai_session = Some(AiSession {
            document: owner_document,
            panel_navigation: false,
            saved,
            input,
            language: cx.new(|cx| {
                let mut field = TextField::new("English / 简体中文", cx);
                field.set_value("English", cx);
                field
            }),
            instruction_name: cx.new(|cx| TextField::new(strings.ai_panel_instruction_name, cx)),
            context_input: cx.new(|cx| TextField::new("@notes.md / @\"folder name\"", cx)),
            instruction_body: cx
                .new(|cx| TextField::new("{{selection}} · {{document}} · {{references}}", cx)),
            settings,
            visible: true,
            expanded: false,
            mode: PanelMode::Task,
            command,
            origin,
            error,
            activity: String::new(),
            generation: 0,
            control: RunControl::default(),
            task: None,
            stream_index: None,
            scroll: ScrollHandle::new(),
            review_focus: cx.focus_handle(),
            panel_focus: cx.focus_handle(),
            panel_bounds: Default::default(),
            panel_drag: None,
            conversation: None,
            review_open: false,
            review_navigation: None,
            history_files: Vec::new(),
            skills: crate::ai::skills::SkillCatalog::load(&directories),
            context_candidates: Vec::new(),
            settings_saving: false,
            settings_revision: AiFeature::revision(cx),
            review: None,
            review_dirty: true,
        });
        let Some(session) = self.ai_session.as_mut() else {
            return;
        };
        session.visible = true;
        session.panel_navigation = false;
        if command != AiCommand::Ask {
            session.saved.workspace.read_only = command.is_reading_task();
        }
        if command != AiCommand::Ask {
            session.saved.workspace.selection_scope =
                if command.edits_selection() {
                    session.saved.target.as_ref().map(|target| {
                        crate::ai::workspace::SelectionScope {
                            document_id: target.document_id.clone(),
                            range: target.range.clone(),
                            version: target.version,
                        }
                    })
                } else {
                    None
                };
        }
        session.command = command;
        session.origin = origin;
        session.mode = PanelMode::Task;
        let input = session.input.clone();
        input.update(cx, |input, cx| {
            if command != AiCommand::Ask {
                input.set_value(
                    &command.label(cx.global::<crate::i18n::I18nManager>().strings()),
                    cx,
                );
            }
            input.focus(window, cx);
        });
        let configured = !session.settings.endpoints.is_empty();
        if configured && !matches!(command, AiCommand::Ask | AiCommand::Translate) {
            self.send_ai_task(cx);
        }
        self.pending_focus = None;
        cx.notify();
    }

    fn ai_window_live_ids(
        &self,
        root: Option<&Path>,
        cx: &App,
    ) -> std::collections::BTreeSet<String> {
        let mut documents = std::collections::BTreeSet::new();
        let mut collect = |editor: &Editor| {
            for (path, recovery) in editor.ai_cached_document_paths() {
                documents.insert(ai_document_id(Some(&path), root, recovery));
            }
            if editor.split_workspace.is_none() && !editor.code_document {
                documents.insert(ai_document_id(
                    editor.file_path.as_deref(),
                    root,
                    editor.recovery_id,
                ));
            }
        };
        collect(self);
        for group in self.split_groups() {
            collect(group.read(cx));
        }
        documents
    }

    fn ai_current_snapshot(&self, id: &str, cx: &App) -> Option<DocumentSnapshot> {
        let session = self.ai_session.as_ref()?;
        let root = session.saved.workspace.root.as_deref();
        let bound = session.saved.bindings.get(id).map(PathBuf::as_path);
        let current = |editor: &Editor| {
            (editor.split_workspace.is_none()
                && !editor.code_document
                && (ai_document_id(editor.file_path.as_deref(), root, editor.recovery_id) == id
                    || bound.is_some_and(|path| {
                        editor
                            .active_document_path()
                            .is_some_and(|active| active == path)
                    })))
            .then(|| DocumentSnapshot {
                id: id.to_owned(),
                source: editor.buffer.text(),
            })
        };
        if let Some(snapshot) = current(self) {
            return Some(snapshot);
        }
        let groups = self.split_groups();
        for group in &groups {
            if let Some(snapshot) = current(group.read(cx)) {
                return Some(snapshot);
            }
        }
        for group in &groups {
            if let Some(source) = group.read(cx).ai_cached_source_for_id(id, root, bound) {
                return Some(DocumentSnapshot {
                    id: id.to_owned(),
                    source,
                });
            }
        }
        self.ai_cached_source_for_id(id, root, bound)
            .map(|source| DocumentSnapshot {
                id: id.to_owned(),
                source,
            })
    }

    pub(super) fn send_ai_task(&mut self, cx: &mut Context<Self>) {
        if !crate::ai::settings::AiFeature::enabled(cx) {
            self.sync_ai_availability(cx);
            return;
        }
        if self
            .ai_session
            .as_ref()
            .is_some_and(|session| session.mode != PanelMode::Task || session.settings_saving)
        {
            return;
        }
        if self.ai_session.as_ref().is_some_and(|session| {
            session.saved.kind == SessionKind::QuickCommand && !session.saved.messages.is_empty()
        }) {
            return;
        }
        let editor_context = self.ai_editor_context(cx);
        if self.ai_session.as_ref().is_some_and(|session| {
            !(session.expanded && session.saved.kind == SessionKind::Conversation)
                && (session.document != self.ai_review_navigation_origin(cx)
                    || session.saved.target.as_ref().is_some_and(|target| {
                        editor_context
                            .get("active_tab")
                            .and_then(|tab| tab.get("document_id"))
                            .and_then(serde_json::Value::as_str)
                            != Some(target.document_id.as_str())
                            && session
                                .saved
                                .bindings
                                .get(&target.document_id)
                                .is_none_or(|path| {
                                    editor_context
                                        .get("active_tab")
                                        .and_then(|tab| tab.get("path"))
                                        != Some(&serde_json::json!(path))
                                })
                    }))
        }) {
            if let Some(session) = self.ai_session.as_mut() {
                session.error = Some("请回到任务所属文档继续，或重新发起任务".into());
            }
            cx.notify();
            return;
        }
        let ui_language = cx
            .global::<crate::i18n::I18nManager>()
            .current_language_id()
            .to_owned();
        let live_ids = self
            .ai_session
            .as_ref()
            .map(|session| self.ai_window_live_ids(session.saved.workspace.root.as_deref(), cx))
            .unwrap_or_default();
        let known = self
            .ai_session
            .as_ref()
            .map(|session| {
                session
                    .saved
                    .workspace
                    .documents
                    .keys()
                    .chain(session.saved.bindings.keys())
                    .cloned()
                    .collect::<std::collections::BTreeSet<_>>()
            })
            .unwrap_or_default();
        let refreshed: Vec<_> = known
            .into_iter()
            .filter_map(|id| self.ai_current_snapshot(&id, cx))
            .collect();
        let Some(session) = self.ai_session.as_mut() else {
            return;
        };
        if !session.saved.running {
            session.saved.workspace.live_documents = live_ids;
            for snapshot in refreshed {
                session
                    .saved
                    .workspace
                    .live_documents
                    .insert(snapshot.id.clone());
                session
                    .saved
                    .workspace
                    .documents
                    .insert(snapshot.id.clone(), snapshot);
            }
        }
        let raw_instruction = session.input.read(cx).value().trim().to_owned();
        let target_id = session
            .saved
            .target
            .as_ref()
            .map(|target| target.document_id.as_str())
            .unwrap_or_default();
        let selection = session
            .saved
            .target
            .as_ref()
            .map(|target| target.selection.as_str())
            .unwrap_or_default();
        let instruction = crate::ai::context::expand_instruction(
            &raw_instruction,
            selection,
            target_id,
            &session.saved.references,
        );
        if instruction.is_empty() {
            return;
        }
        if session.command.requires_selection()
            && session
                .saved
                .target
                .as_ref()
                .is_none_or(|target| target.range.is_empty())
        {
            session.error = Some("请先选择需要处理的文字".into());
            cx.notify();
            return;
        }
        if session.saved.running {
            match session.control.steering.lock() {
                Ok(mut queue) => queue.push(crate::ai::context::task_prompt(
                    &instruction,
                    &editor_context,
                    None,
                    &session.saved.references,
                    false,
                    None,
                )),
                Err(error) => {
                    session.error = Some(error.to_string());
                    cx.notify();
                    return;
                }
            }
            session.saved.messages.push(ChatMessage {
                role: "user".into(),
                text: instruction,
            });
            session
                .input
                .update(cx, |input, cx| input.set_value("", cx));
            cx.notify();
            return;
        }
        match AiSettings::load() {
            Ok(settings) => session.settings = settings,
            Err(error) => {
                session.error = Some(error.to_string());
                cx.notify();
                return;
            }
        }
        session.saved.workspace.excluded_paths = session.settings.excluded_paths.clone();
        let excluded_target = session
            .saved
            .target
            .as_ref()
            .is_some_and(|target| session.saved.workspace.is_excluded(&target.document_id));
        let excluded_active = editor_context
            .get("active_tab")
            .and_then(|tab| tab.get("document_id"))
            .and_then(serde_json::Value::as_str)
            .is_some_and(|id| session.saved.workspace.is_excluded(id));
        if excluded_target || excluded_active {
            session.error = Some("当前文档已从 AI 资料中排除".into());
            cx.notify();
            return;
        }
        let Some(endpoint) = session
            .settings
            .endpoints
            .iter()
            .find(|endpoint| endpoint.id == session.saved.endpoint_id)
            .or_else(|| session.settings.active_endpoint())
            .cloned()
        else {
            self.open_ai_settings(cx);
            return;
        };
        let model = match endpoint.request_config() {
            Ok(model) => model,
            Err(error) => {
                session.error = Some(crate::ai::settings::error_message(
                    &error,
                    cx.global::<crate::i18n::I18nManager>().strings(),
                ));
                cx.notify();
                return;
            }
        };
        let mut references = session.saved.references.clone();
        match crate::ai::context::explicit_references(&instruction) {
            Ok(paths) => {
                for path in paths {
                    if !references.contains(&path) {
                        references.push(path);
                    }
                }
            }
            Err(error) => {
                session.error = Some(error.to_string());
                cx.notify();
                return;
            }
        }
        let primary = session
            .saved
            .target
            .as_ref()
            .map(|target| target.document_id.as_str())
            .unwrap_or_default();
        if let Err(error) = session.saved.workspace.set_scope(&references, primary) {
            session.error = Some(error.to_string());
            cx.notify();
            return;
        }
        session.saved.references = references;
        let skills = session
            .skills
            .enabled(&session.settings.disabled_skills)
            .prompt_catalog();
        // 首次发送后固定系统与快捷指令；设置变化不能改写多轮请求的已发送前缀。
        let profile = session.saved.prompt_profile.get_or_insert_with(|| {
            crate::ai::instructions::PromptProfile::capture(
                session.command,
                &ui_language,
                &session.settings,
                &skills,
            )
        });
        let mut editor_context = editor_context;
        if let Some(context) = editor_context.as_object_mut() {
            if let Some(scope) = &session.saved.workspace.selection_scope {
                context.insert("write_scope".into(), serde_json::json!({
                    "document_id":scope.document_id,
                    "byte_range":{"start":scope.range.start,"end":scope.range.end},
                    "preserve_all_other_bytes":true,
                }));
            }
            if session.command == AiCommand::Translate {
                context.insert("target_language".into(), session.language.read(cx).value().into());
            }
        }
        let prompt = crate::ai::context::task_prompt(
            &instruction,
            &editor_context,
            session.saved.target.as_ref(),
            &session.saved.references,
            session.saved.history.is_empty(),
            profile.quick_instruction.as_deref(),
        );
        session.generation = session.generation.wrapping_add(1);
        let generation = session.generation;
        let session_id = session.saved.id.clone();
        session.control.cancel.cancel();
        session.control = RunControl::default();
        session.saved.running = true;
        session.saved.interrupted = false;
        session.error = None;
        session.stream_index = None;
        session.saved.messages.push(ChatMessage {
            role: "user".into(),
            text: instruction,
        });
        session
            .input
            .update(cx, |input, cx| input.set_value("", cx));
        session.saved.workspace.cursor_target = (|| {
            Some(crate::ai::workspace::CursorTarget {
                document_id: editor_context
                    .get("active_tab")?
                    .get("document_id")?
                    .as_str()?
                    .to_owned(),
                version: editor_context.get("active_tab")?.get("version")?.as_u64()?,
                offset: usize::try_from(
                    editor_context.get("cursor")?.get("byte_offset")?.as_u64()?,
                )
                .ok()?,
            })
        })();
        let mut workspace = session.saved.workspace.clone();
        workspace.skills = session.skills.enabled(&session.settings.disabled_skills);
        workspace.excluded_paths = session.settings.excluded_paths.clone();
        let control = session.control.clone();
        let (sender, receiver) = mpsc::channel();
        workspace.refresh.callback = Some(Arc::new({
            let sender = sender.clone();
            let control = control.clone();
            move |id| {
                let (response, receive) = mpsc::sync_channel(1);
                sender
                    .send(WorkerEvent::Refresh {
                        id: id.to_owned(),
                        response,
                    })
                    .map_err(|_| anyhow::anyhow!("编辑器窗口已经关闭"))?;
                let started = std::time::Instant::now();
                loop {
                    if control.cancel.is_cancelled() {
                        anyhow::bail!("任务已停止");
                    }
                    match receive.recv_timeout(Duration::from_millis(50)) {
                        Ok(response) => return response.map_err(anyhow::Error::msg),
                        Err(mpsc::RecvTimeoutError::Timeout)
                            if started.elapsed() < Duration::from_secs(10) => {}
                        Err(error) => return Err(anyhow::anyhow!("读取编辑器文档失败：{error}")),
                    }
                }
            }
        }));
        let workspace = Arc::new(Mutex::new(workspace));
        let request = RunRequest {
            model,
            api_key: String::new(),
            history: session.saved.history.clone(),
            prompt,
            max_turns: 32,
            timeout: Duration::from_secs(600),
            system: profile.system.clone(),
        };
        let background = cx.background_spawn(async move {
            let result = (|| -> Result<RunOutput> {
                let mut request = request;
                request.api_key = endpoint.api_key.trim().to_owned();
                crate::ai::runtime::run_blocking(request, workspace, control.clone(), |event| {
                    if sender.send(WorkerEvent::Runtime(event)).is_err() {
                        control.cancel.cancel();
                    }
                })
            })()
            .map_err(|error| error.to_string());
            if sender.send(WorkerEvent::Finished(result)).is_err() {
                control.cancel.cancel();
            }
        });
        session.task = Some(cx.spawn(async move |owner, cx| {
            loop {
                let events: Vec<_> = receiver.try_iter().take(128).collect();
                let mut finished = false;
                if !events.is_empty() {
                    match owner.update(cx, |editor, cx| {
                        editor.receive_ai_session_events(&session_id, generation, events, cx)
                    }) {
                        Ok(done) => finished = done,
                        Err(_) => break,
                    }
                }
                if finished {
                    break;
                }
                cx.background_executor()
                    .timer(Duration::from_millis(24))
                    .await;
            }
            background.await;
        }));
        self.persist_ai_session(cx);
        cx.notify();
    }

    fn receive_ai_session_events(
        &mut self,
        id: &str,
        generation: u64,
        events: Vec<WorkerEvent>,
        cx: &mut Context<Self>,
    ) -> bool {
        if self
            .ai_session
            .as_ref()
            .is_some_and(|session| session.saved.id == id)
        {
            return self.receive_ai_events(generation, events, cx);
        }
        let Some(index) = self
            .ai_task_sessions
            .iter()
            .position(|session| session.saved.id == id)
        else {
            return true;
        };
        let session = self.ai_task_sessions.remove(index);
        // 后台任务继续读取各标签的实时缓冲区，但结果只回写它自己的会话。
        let previous = self.ai_session.replace(session);
        let finished = self.receive_ai_events(generation, events, cx);
        if let Some(session) = self.ai_session.take() {
            self.ai_task_sessions.insert(index, session);
        }
        self.ai_session = previous;
        finished
    }

    fn receive_ai_events(
        &mut self,
        generation: u64,
        events: Vec<WorkerEvent>,
        cx: &mut Context<Self>,
    ) -> bool {
        let mut remaining = Vec::new();
        for event in events {
            if let WorkerEvent::Refresh { id, response } = event {
                let current = self
                    .ai_session
                    .as_ref()
                    .is_some_and(|session| session.generation == generation);
                let snapshot = if current {
                    Ok(self.ai_current_snapshot(&id, cx))
                } else {
                    Err("任务已停止".into())
                };
                if response.send(snapshot).is_err() && current {
                    if let Some(session) = self.ai_session.as_ref() {
                        session.control.cancel.cancel();
                    }
                }
            } else {
                remaining.push(event);
            }
        }
        let Some(session) = self
            .ai_session
            .as_mut()
            .filter(|session| session.generation == generation)
        else {
            return true;
        };
        let mut finished = false;
        let mut checkpoint = false;
        for event in remaining {
            match event {
                WorkerEvent::Runtime(RunEvent::AssistantStart) => {
                    session.saved.messages.push(ChatMessage {
                        role: "assistant".into(),
                        text: String::new(),
                    });
                    session.stream_index = session.saved.messages.len().checked_sub(1);
                }
                WorkerEvent::Runtime(RunEvent::Text(text)) => {
                    if let Some(message) = session
                        .stream_index
                        .and_then(|index| session.saved.messages.get_mut(index))
                    {
                        message.text.push_str(&text);
                    }
                }
                WorkerEvent::Runtime(RunEvent::Activity(activity)) => session.activity = activity,
                WorkerEvent::Runtime(RunEvent::Checkpoint { workspace, history }) => {
                    session.saved.workspace = *workspace;
                    session.review_dirty = true;
                    session.saved.history = history;
                    checkpoint = true;
                }
                WorkerEvent::Finished(result) => {
                    session.saved.running = false;
                    match result {
                        Ok(output) => {
                            session.saved.history = output.history;
                            session.error = output.error;
                        }
                        Err(error) => session.error = Some(error),
                    }
                    finished = true;
                }
                WorkerEvent::Refresh { .. } => {}
            }
        }
        if finished {
            self.refresh_ai_preview(cx);
        }
        if checkpoint || finished {
            self.persist_ai_session(cx);
        }
        cx.notify();
        finished
    }

    pub(super) fn stop_ai_task(&mut self, cx: &mut Context<Self>) {
        if let Some(session) = self.ai_session.as_mut() {
            session.control.cancel.cancel();
            session.saved.interrupted |= session.saved.running;
            session.saved.running = false;
            session.generation = session.generation.wrapping_add(1);
            session.task = None;
        }
        self.refresh_ai_preview(cx);
        self.persist_ai_session(cx);
        cx.notify();
    }

    fn persist_ai_session(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.ai_session.as_mut() else {
            return;
        };
        if session.saved.messages.is_empty() && session.saved.workspace.changes.proposals.is_empty()
        {
            return;
        }
        session.saved.sequence = session.saved.sequence.wrapping_add(1);
        let saved = session.saved.clone();
        let id = saved.id.clone();
        let write =
            cx.background_spawn(async move { saved.path().and_then(|path| saved.save_at(&path)) });
        cx.spawn(async move |owner, cx| {
            if let Err(error) = write.await {
                let message = error.to_string();
                if let Err(context_error) = owner.update(cx, |editor, cx| {
                    if let Some(session) = editor.ai_session_mut(&id) {
                        session.error = Some(message.clone());
                        cx.notify();
                    }
                }) {
                    eprintln!("保存 AI 会话失败：{message}；窗口已关闭：{context_error}");
                }
            }
        })
        .detach();
    }

    pub(in crate::editor) fn refresh_ai_preview(&mut self, cx: &mut Context<Self>) {
        if let Some(session) = self.ai_session.as_mut() {
            session.review_dirty = true;
        }
        self.refresh_ai_conversation(cx);
    }

    fn refresh_ai_conversation(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.ai_session.as_ref() else {
            return;
        };
        let mut text = String::new();
        for message in &session.saved.messages {
            if message.text.is_empty() {
                continue;
            }
            if !text.is_empty() {
                text.push_str("\n\n");
            }
            if message.role == "user" {
                for line in message.text.lines() {
                    text.push_str("> ");
                    text.push_str(line);
                    text.push('\n');
                }
            } else {
                text.push_str(&message.text);
            }
        }
        if text.is_empty()
            || session
                .conversation
                .as_ref()
                .is_some_and(|preview| preview.read(cx).buffer.text() == text)
        {
            return;
        }
        let base = session
            .saved
            .workspace
            .root
            .as_ref()
            .map(|root| root.join(".ai-response.md"));
        let window_handle = self.window_handle;
        let preview = cx.new(|cx| {
            let mut editor = Editor::from_markdown(cx, text, base);
            editor.reading_mode.set(true);
            editor.ai_result_preview = true;
            editor.ai_compact_preview = true;
            editor.window_handle = window_handle;
            editor
        });
        if let Some(session) = self.ai_session.as_mut() {
            session.conversation = Some(preview);
        }
    }

    pub(super) fn open_ai_settings(&mut self, cx: &mut Context<Self>) {
        if let Some(session) = self.ai_session.as_mut() {
            session.mode = PanelMode::Task;
        }
        cx.defer(|cx| {
            let handle = cx
                .windows()
                .into_iter()
                .find_map(|window| {
                    window.downcast::<crate::config::preferences::PreferencesWindow>()
                })
                .unwrap_or_else(|| crate::config::open_preferences_window(cx));
            if let Err(error) = handle.update(cx, |preferences, window, cx| {
                window.activate_window();
                preferences.set_nav_ai(&ClickEvent::default(), window, cx);
            }) {
                eprintln!("打开 AI 偏好设置失败：{error}");
            }
        });
        cx.notify();
    }

    pub(in crate::editor) fn sync_ai_settings(&mut self, cx: &mut Context<Self>) {
        let revision = AiFeature::revision(cx);
        if let Some(session) = self.ai_session.as_mut()
            && session.settings_revision != revision
        {
            session.settings_revision = revision;
            match AiSettings::load() {
                Ok(settings) => session.settings = settings,
                Err(error) => session.error = Some(error.to_string()),
            }
        }
    }

    fn update_ai_settings(&mut self, settings: AiSettings, cx: &mut Context<Self>) {
        let Some(session) = self
            .ai_session
            .as_mut()
            .filter(|session| !session.settings_saving)
        else {
            return;
        };
        let id = session.saved.id.clone();
        session.settings_saving = true;
        let save = cx.background_spawn(async move {
            settings.save_at(&crate::ai::settings::settings_path()?)?;
            Ok::<_, anyhow::Error>(settings)
        });
        cx.spawn(async move |owner, cx| {
            let result = save.await;
            if let Err(error) = owner.update(cx, |editor, cx| {
                if let Some(session) = editor.ai_session_mut(&id) {
                    session.settings_saving = false;
                    match result {
                        Ok(settings) => session.settings = settings,
                        Err(error) => session.error = Some(error.to_string()),
                    }
                    cx.notify();
                }
            }) {
                eprintln!("更新 AI 设置失败：{error}");
            }
        })
        .detach();
        cx.notify();
    }

    fn reload_ai_skills(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.ai_session.as_ref() else {
            return;
        };
        let id = session.saved.id.clone();
        let root = session.saved.workspace.root.clone();
        let load = cx.background_spawn(async move {
            let mut directories = vec![(
                crate::ai::settings::settings_path()?
                    .with_file_name("ai")
                    .join("skills"),
                "user",
            )];
            if let Some(root) = root {
                directories.push((root.join(".velora/skills"), "workspace"));
            }
            Ok::<_, anyhow::Error>(crate::ai::skills::SkillCatalog::load(&directories))
        });
        cx.spawn(async move |owner, cx| {
            let result = load.await;
            if let Err(error) = owner.update(cx, |editor, cx| {
                if let Some(session) = editor.ai_session_mut(&id) {
                    match result {
                        Ok(skills) => session.skills = skills,
                        Err(error) => session.error = Some(error.to_string()),
                    }
                    cx.notify();
                }
            }) {
                eprintln!("重新加载 AI 任务说明失败：{error}");
            }
        })
        .detach();
    }
}

pub(in crate::editor) fn ai_document_id(
    path: Option<&Path>,
    root: Option<&Path>,
    recovery: uuid::Uuid,
) -> String {
    if let Some(path) = path {
        if let Some(id) = super::workspace::untitled_document_id(path) {
            return format!("untitled:{id}");
        }
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        if let Some(relative) = root.and_then(|root| canonical.strip_prefix(root).ok()) {
            return relative.to_string_lossy().replace('\\', "/");
        }
        return format!(
            "external:{}",
            canonical.to_string_lossy().replace('\\', "/")
        );
    }
    format!("untitled:{recovery}")
}

#[cfg(test)]
mod tests;
