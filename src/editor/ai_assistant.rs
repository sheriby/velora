use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use anyhow::{Context as _, Result};
use gpui::*;

use super::Editor;
use crate::ai::runtime::{RunControl, RunEvent, RunOutput, RunRequest};
use crate::ai::session::{
    ChatMessage, SavedSession, SessionKind, TargetSnapshot, ToolCallRecord, ToolCallStatus,
};
use crate::ai::settings::{AiFeature, AiSettings};
use crate::ai::workspace::AiWorkspace;
use crate::ai::{AiCommand, DocumentSnapshot};
use gpui_kit::assets::IconName;
use gpui_kit::component::input::{
    Enter as InputEnter, Input, InputEvent, InputState, Textarea, TextareaState,
};
use gpui_kit::component::{
    Disableable, Sizable,
    button::{Button, ButtonVariants},
    checkbox::Checkbox,
};

mod apply;
mod context;
mod experience;
mod messages;
pub(in crate::editor) mod inline_review;
mod render;
mod source_list;
mod workbench;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum PanelMode {
    Task,
    History,
    Skills,
    Instruction,
    Context,
    Sources,
}

pub(super) struct AiSidebarLayout {
    state: Entity<gpui_kit::component::resizable::ResizableState>,
    focus: FocusHandle,
}

pub(super) struct AiSession {
    document: uuid::Uuid,
    panel_navigation: bool,
    pub(super) saved: SavedSession,
    input: Entity<TextareaState>,
    _input_subscription: Subscription,
    composer_focus: FocusHandle,
    language: Entity<InputState>,
    instruction_name: Entity<InputState>,
    instruction_body: Entity<TextareaState>,
    context_input: Entity<InputState>,
    _field_subscriptions: Vec<Subscription>,
    settings: AiSettings,
    visible: bool,
    expanded: bool,
    /// 浮动窗收起到右下角图标；停靠侧栏不使用。
    collapsed: bool,
    /// 浮动任务窗的用户尺寸；None 表示默认尺寸。
    float_size: Option<Size<Pixels>>,
    /// 右下角缩放的起点与起始尺寸。
    float_resize: Option<(Point<Pixels>, Size<Pixels>)>,
    mode: PanelMode,
    command: AiCommand,
    origin: Point<Pixels>,
    error: Option<String>,
    truncated: bool,
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
    pub(in crate::editor) conversation: Option<Entity<messages::MessageTranscript>>,
    review_open: bool,
    // 标签重载会重建缓冲区，使用稳定的恢复标识区分暂离与主动关闭。
    review_navigation: Option<uuid::Uuid>,
    source_list: Entity<gpui_kit::component::list::ListState<source_list::SourceList>>,
    source_focus_pending: bool,
    history_files: Vec<(PathBuf, String, SessionKind)>,
    history_list: Entity<gpui_kit::component::command::CommandState>,
    history_loading: bool,
    history_focus_pending: bool,
    skills: crate::ai::skills::SkillCatalog,
    context_candidates: Vec<String>,
    settings_saving: bool,
    /// 保留读取失败原因，防止默认配置覆盖仍可修复的原文件。
    settings_read_error: Option<String>,
    // 设置单独编辑，仅在配置版本变化时读盘，避免在每帧渲染里读取配置。
    settings_revision: u64,
    review: Option<experience::ReviewDocument>,
    review_dirty: bool,
}

impl AiSession {
    fn settings_readonly(&self) -> bool {
        self.settings_read_error.is_some()
    }

    fn set_settings_read_error(&mut self, error: Option<String>) {
        if error.is_some() || self.error == self.settings_read_error {
            self.error = error.clone();
        }
        self.settings_read_error = error;
    }

    #[cfg(test)]
    pub(in crate::editor) fn control_for_test(&self) -> RunControl {
        self.control.clone()
    }
}

impl Drop for AiSession {
    fn drop(&mut self) {
        self.control.cancel.cancel();
        // 会话可能带着全部文档快照，同步序列化+fsync 会把 UI 线程按体积卡住；
        // 后台线程落盘，退出前由 flush_pending_saves 收尾。正文已 Arc 化，
        // 这里的克隆只是句柄表。
        crate::ai::session::SavedSession::save_in_background(
            self.saved.clone(),
            "关闭窗口时保存 AI 会话失败",
        );
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

/// 轨迹只留最近这些条；完整对话在 history 里，界面轨迹不承担归档。
const AI_TOOL_TRACE_LIMIT: usize = 200;
/// 思考文本只用于界面展示，超过这个字符数就不再追加。
const AI_THINKING_LIMIT: usize = 20_000;

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
        let focused = self.ai_interaction_has_focus(window, cx);
        // 模式回切与关闭都要看焦点：右键菜单/命令面板等瞬态浮层开着时，
        // Esc 应先关最上层浮层，而不是被面板抢先吃掉。
        if focused
            && let Some(session) = self
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
        if focused {
            if self
                .ai_session
                .as_ref()
                .is_some_and(|session| session.visible)
            {
                self.hide_ai_panel(cx);
            } else if self
                .ai_session
                .as_ref()
                .is_some_and(|session| session.review_open)
            {
                self.close_ai_review_surface(cx);
            }
        }
        focused
    }

    /// 暂存任务设上限：每份都持有全文快照与输入/预览实体，只增不减会让
    /// 长会话工作流的内存无界增长；淘汰即落盘（Drop 走后台保存）。
    fn stash_ai_session(&mut self, previous: AiSession) {
        const AI_TASK_SESSION_LIMIT: usize = 6;
        self.ai_task_sessions.push(previous);
        while self.ai_task_sessions.len() > AI_TASK_SESSION_LIMIT {
            // 运行中的任务不能因暂存上限被丢弃：Drop 会取消它的 task。
            // 全在运行时宁可暂时超出上限。
            let Some(index) = self
                .ai_task_sessions
                .iter()
                .position(|session| !session.saved.running)
            else {
                break;
            };
            self.ai_task_sessions.remove(index);
        }
    }

    /// 关闭原位审阅并把焦点归还正文：审阅面卸载后原聚焦句柄成为孤儿，
    /// 按键既进不了正文也不再触发正文快捷键，直到用户手动点击。
    fn close_ai_review_surface(&mut self, cx: &mut Context<Self>) {
        if let Some(session) = self.ai_session.as_mut() {
            session.review_open = false;
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
                self.stash_ai_session(previous);
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

    fn add_ai_context(
        &mut self,
        path: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
                    .update(cx, |input, cx| input.set_value("", window, cx));
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
        if self
            .ai_session
            .as_ref()
            .is_some_and(|session| session.saved.workspace.changes.proposals.is_empty())
        {
            self.close_ai_review_surface(cx);
        }
        self.persist_ai_session(cx);
        cx.notify();
    }

    fn retry_ai_task(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
                    .update(cx, |input, cx| input.set_value(&prompt, window, cx));
            }
        }
        if let Some(session) = self.ai_session.as_mut() {
            // 纯快捷指令（只有自动指令一轮）重试仍然整轮重跑；一旦有追问，
            // 按对话回滚失败轮，保住之前的结果。
            if session.saved.kind == SessionKind::QuickCommand
                && session.saved.messages.len() <= 2
            {
                session.saved.history.clear();
                session.saved.messages.clear();
                session.saved.workspace.discard_proposals();
            } else {
                // 对话重试前回滚失败轮，发送后恰好重新追加一次指令。
                session.saved.rollback_failed_conversation_round();
            }
        }
        self.refresh_ai_conversation(cx);
        self.send_ai_task(window, cx);
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
        let session_id = session.saved.id.clone();
        let root = session.saved.workspace.root.clone();
        let bound = session.saved.bindings.get(&record.path).cloned();
        let groups = self.split_groups();
        let destination = bound
            .clone()
            .or_else(|| {
                self.ai_cached_source_path_for_id(&record.path, root.as_deref(), bound.as_deref())
            })
            .or_else(|| {
                groups.iter().find_map(|group| {
                    group.read(cx).ai_cached_source_path_for_id(
                        &record.path,
                        root.as_deref(),
                        bound.as_deref(),
                    )
                })
            });
        let target = groups
            .into_iter()
            .find(|group| {
                let editor = group.read(cx);
                ai_document_id(
                    editor.file_path.as_deref(),
                    root.as_deref(),
                    editor.recovery_id,
                ) == record.path
                    || bound
                        .as_ref()
                        .is_some_and(|path| editor.active_document_path() == Some(path))
            })
            .or_else(|| self.active_split_editor());
        if let Some(session) = self.ai_session.as_mut() {
            session.review_open = false;
            session.review_navigation = None;
        }
        if let Some(group) = target {
            if self
                .active_split_editor()
                .is_none_or(|active| active.entity_id() != group.entity_id())
            {
                self.activate_split_group(group.entity_id(), cx);
            }
            let handle = window.window_handle();
            let host = cx.entity().downgrade();
            cx.defer(move |cx| {
                if let Err(error) = handle.update(cx, |_, window, cx| {
                    let current = match host.read_with(cx, |editor, _| {
                        !editor.modal_is_open()
                            && editor.info_dialog.is_none()
                            && editor.ai_session.as_ref().is_some_and(|session| {
                                session.saved.id == session_id && session.mode == PanelMode::Sources
                            })
                    }) {
                        Ok(current) => current,
                        Err(error) => {
                            eprintln!("引用定位的宿主已关闭：{error}");
                            return;
                        }
                    };
                    if !current || gpui_kit::base::active_focus_trap(window, cx).is_some() {
                        return;
                    }
                    group.update(cx, |editor, cx| {
                        if !editor.modal_is_open() && editor.info_dialog.is_none() {
                            editor.navigate_ai_source(record, root, destination, window, cx);
                        }
                    });
                }) {
                    eprintln!("定位 AI 引用失败：{error}");
                }
            });
        } else {
            self.navigate_ai_source(record, root, destination, window, cx);
        }
    }

    fn navigate_ai_source(
        &mut self,
        record: crate::ai::workspace::SourceRecord,
        root: Option<PathBuf>,
        destination: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let current_id =
            ai_document_id(self.file_path.as_deref(), root.as_deref(), self.recovery_id);
        if current_id != record.path
            && destination
                .as_ref()
                .is_none_or(|path| self.active_document_path() != Some(path))
        {
            let path = if let Some(path) = destination {
                path
            } else {
                let Some(root) = root.as_ref() else {
                    self.show_message_modal(
                        "AI",
                        cx.global::<crate::i18n::I18nManager>()
                            .strings()
                            .ai_error_reference_closed
                            .clone(),
                        cx,
                    );
                    return;
                };
                match root.join(&record.path).canonicalize() {
                    Ok(path) if path.starts_with(root) => crate::file_url::canonical_path(&path),
                    _ => {
                        self.show_message_modal(
                            "AI",
                            cx.global::<crate::i18n::I18nManager>()
                                .strings()
                                .ai_error_reference_missing
                                .clone(),
                            cx,
                        );
                        return;
                    }
                }
            };
            self.open_workspace_file_in_mode(
                path.clone(),
                super::workspace::WorkspaceOpenMode::Pinned,
                window,
                cx,
            );
            if ai_document_id(self.file_path.as_deref(), root.as_deref(), self.recovery_id)
                != record.path
                && self.active_document_path() != Some(&path)
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
        self.open_ai_assistant_with_layout(AiCommand::Ask, expanded, window, cx);
        cx.notify();
    }

    fn show_ai_history(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.ai_session.as_mut() else {
            return;
        };
        session.mode = PanelMode::History;
        session.history_files.clear();
        session.history_loading = true;
        session.history_focus_pending = true;
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
                    session.history_loading = false;
                    session.history_focus_pending = true;
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
            session.collapsed = false;
            session.panel_navigation = false;
            session.mode = PanelMode::Task;
            if let Some(previous) = self.ai_session.replace(session) {
                self.stash_ai_session(previous);
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
        cx.spawn_in(window, async move |owner, cx| {
            let loaded = load.await;
            if let Err(error) =
                owner.update_in(cx, |editor, window, cx| {
                    // 磁盘加载在途时会话可能已被新任务暂存；暂存会话不递增
                    // generation，按 id 回退搜索会命中它并把待审状态整体替换掉。
                    // 只认仍在前台的同一会话，否则丢弃这次加载。
                    if let Some(session) = editor.ai_session.as_mut().filter(|session| {
                        session.saved.id == id && session.generation == generation
                    }) {
                        match loaded {
                            Ok(saved) => {
                                session.saved = saved;
                                session.expanded = true;
                                session.visible = true;
                                session.collapsed = false;
                                session.panel_navigation = false;
                                session.command = session
                                    .saved
                                    .prompt_profile
                                    .as_ref()
                                    .map_or(AiCommand::Ask, |profile| profile.command);
                                session
                                    .input
                                    .update(cx, |input, cx| input.set_value("", window, cx));
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
                })
            {
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
                session.error = Some(cx.global::<crate::i18n::I18nManager>().strings().ai_error_instruction_incomplete.clone());
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
    pub(crate) fn on_toggle_ai_sidebar(
        &mut self,
        _: &crate::components::ToggleAiSidebar,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !AiFeature::enabled(cx) {
            return;
        }
        if let Some(parent) = self.split_parent.clone() {
            let handle = window.window_handle();
            cx.defer(move |cx| {
                match handle.update(cx, |_, window, cx| {
                    parent.update(cx, |editor, cx| {
                        editor.on_toggle_ai_sidebar(&crate::components::ToggleAiSidebar, window, cx)
                    })
                }) {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) | Err(error) => eprintln!("切换宿主 AI 侧栏失败：{error}"),
                }
            });
            return;
        }
        if self
            .ai_session
            .as_ref()
            .is_some_and(|session| session.visible && session.expanded)
        {
            self.hide_ai_panel(cx);
        } else if let Some(session) = self.ai_session.as_mut() {
            session.visible = true;
            session.expanded = true;
            session.collapsed = false;
            session.panel_drag = None;
            session.panel_navigation = false;
            session.mode = PanelMode::Task;
            if session.saved.kind == SessionKind::Conversation {
                session
                    .input
                    .update(cx, |input, cx| input.focus(window, cx));
            } else {
                session.panel_focus.focus(window, cx);
            }
            cx.notify();
        } else {
            self.open_ai_assistant_with_layout(AiCommand::Ask, true, window, cx);
        }
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
        self.open_ai_assistant_with_layout(command, false, window, cx);
    }

    fn open_ai_assistant_with_layout(
        &mut self,
        command: AiCommand,
        expanded: bool,
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
                        editor.open_ai_assistant_with_layout(command, expanded, window, cx)
                    })
                }) {
                    eprintln!("在活动分屏打开 AI 失败：{error}");
                }
            });
            return;
        }
        if command.requires_selection() && !self.has_text_selection(cx) {
            self.show_message_modal("AI", cx.global::<crate::i18n::I18nManager>().strings().ai_error_no_selection.clone(), cx);
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
            source: source.into(),
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
        self.context_menu_popup = None;
        self.selection_toolbar = None;
        let owner_document = self.recovery_id;
        if let Some(parent) = self.split_parent.clone() {
            let handle = window.window_handle();
            cx.defer(move |cx| {
                match handle.update(cx, |_, window, cx| {
                    parent.update(cx, |editor, cx| {
                        editor.open_ai_target(
                            command,
                            expanded,
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
            expanded,
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
        expanded: bool,
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
        // 读取失败时面板以默认配置继续可用，但禁止一切写回：否则下一笔
        // 保存会把损坏但可救的原文件整体重置成近空配置。
        let settings_read_error = error.clone();
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
        let input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder(strings.ai_custom_placeholder.clone())
                .auto_grow(1, 4)
                .submit_on_enter(true)
        });
        let input_subscription = cx.subscribe(&input, |editor, input, event: &InputEvent, cx| {
            if editor
                .ai_session
                .as_ref()
                .is_some_and(|session| session.input.entity_id() == input.entity_id())
                && matches!(
                    event,
                    InputEvent::Change | InputEvent::Focus | InputEvent::Blur
                )
            {
                cx.notify();
            }
        });
        let mut directories = Vec::new();
        if let Ok(path) = crate::ai::settings::settings_path() {
            directories.push((path.with_file_name("ai").join("skills"), "user"));
        }
        if let Some(root) = &root {
            directories.push((root.join(".velora").join("skills"), "workspace"));
        }
        if let Some(previous) = self.ai_session.take() {
            self.stash_ai_session(previous);
        }
        let language = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("English / 简体中文")
                .default_value(strings.ai_panel_translate_default.clone())
        });
        let instruction_name = cx.new(|cx| InputState::new(window, cx));
        let context_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("@notes.md / @\"folder name\""));
        let instruction_body = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder("{{selection}} · {{document}} · {{references}}")
                .auto_grow(4, 8)
        });
        let mut field_subscriptions = Vec::new();
        for field in [&language, &instruction_name, &context_input] {
            field_subscriptions.push(cx.subscribe(field, |_, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            }));
        }
        field_subscriptions.push(cx.subscribe(
            &instruction_body,
            |_, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            },
        ));
        let source_owner = cx.entity().downgrade();
        let source_list = cx.new(|cx| gpui_kit::component::list::ListState::new(source_list::SourceList::new(source_owner), window, cx));
        self.ai_session = Some(AiSession {
            document: owner_document,
            panel_navigation: false,
            saved,
            input,
            _input_subscription: input_subscription,
            composer_focus: cx.focus_handle(),
            language,
            instruction_name,
            context_input,
            instruction_body,
            _field_subscriptions: field_subscriptions,
            settings,
            visible: true,
            expanded,
            collapsed: false,
            float_size: None,
            float_resize: None,
            mode: PanelMode::Task,
            command,
            origin,
            settings_read_error,
            error,
            truncated: false,
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
            source_list,
            source_focus_pending: true,
            history_files: Vec::new(),
            history_list: cx.new(|cx| gpui_kit::component::command::CommandState::new(window, cx)),
            history_loading: false,
            history_focus_pending: false,
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
                    window,
                    cx,
                );
            }
            input.focus(window, cx);
        });
        let configured = !session.settings.endpoints.is_empty();
        if configured && !matches!(command, AiCommand::Ask | AiCommand::Translate) {
            self.send_ai_task(window, cx);
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
            if editor.split_workspace.is_none() {
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
                && (ai_document_id(editor.file_path.as_deref(), root, editor.recovery_id) == id
                    || bound.is_some_and(|path| {
                        editor
                            .active_document_path()
                            .is_some_and(|active| active == path)
                    })))
            .then(|| DocumentSnapshot {
                id: id.to_owned(),
                source: editor.buffer.text().into(),
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
                    source: source.into(),
                });
            }
        }
        self.ai_cached_source_for_id(id, root, bound)
            .map(|source| DocumentSnapshot {
                id: id.to_owned(),
                source: source.into(),
            })
    }

    pub(super) fn send_ai_task(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
        let mut editor_context = self.ai_editor_context(cx);
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
                session.error = Some(
                    cx.global::<crate::i18n::I18nManager>()
                        .strings()
                        .ai_error_return_to_document
                        .clone(),
                );
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
        // 只从用户实际输入里提取 @ 引用：选区/资料正文里的 @ 不能变成新资料。
        let mut references = session.saved.references.clone();
        match crate::ai::context::explicit_references(&raw_instruction) {
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
        if let Err(error) = session.saved.workspace.set_scope(&references, target_id) {
            session.error = Some(error.to_string());
            cx.notify();
            return;
        }
        session.saved.references = references;
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
            session.error = Some(
                cx.global::<crate::i18n::I18nManager>()
                    .strings()
                    .ai_error_no_selection
                    .clone(),
            );
            cx.notify();
            return;
        }
        let active_skills = match session.skills.active_skill_references(
            &session.settings.disabled_skills,
            &raw_instruction,
        ) {
            Ok(skills) => skills,
            Err(error) => {
                session.error = Some(error);
                cx.notify();
                return;
            }
        };
        if !active_skills.is_empty() && let Some(context) = editor_context.as_object_mut() {
            context.insert("active_skills".into(), serde_json::json!(active_skills));
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
                text: instruction.into(),
                thinking: "".into(),
            });
            session
                .input
                .update(cx, |input, cx| input.set_value("", window, cx));
            self.refresh_ai_conversation(cx);
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
            session.error = Some(
                cx.global::<crate::i18n::I18nManager>()
                    .strings()
                    .ai_error_document_excluded
                    .clone(),
            );
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
        let enabled_skills = session.skills.enabled(&session.settings.disabled_skills);
        let skills = enabled_skills.prompt_catalog();
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
        let active_skills = match enabled_skills
            .active_skill_references(&session.settings.disabled_skills, &raw_instruction)
        {
            Ok(skills) => skills,
            Err(error) => {
                session.error = Some(error);
                cx.notify();
                return;
            }
        };
        if !active_skills.is_empty() && let Some(context) = editor_context.as_object_mut() {
            context.insert("active_skills".into(), serde_json::json!(active_skills));
        }
        if let Some(context) = editor_context.as_object_mut() {
            if let Some(scope) = &session.saved.workspace.selection_scope {
                context.insert(
                    "write_scope".into(),
                    serde_json::json!({
                        "document_id":scope.document_id,
                        "byte_range":{"start":scope.range.start,"end":scope.range.end},
                        "preserve_all_other_bytes":true,
                    }),
                );
            }
            if session.command == AiCommand::Translate {
                context.insert(
                    "target_language".into(),
                    session.language.read(cx).value().as_str().into(),
                );
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
        session.truncated = false;
        session.stream_index = None;
        session.saved.messages.push(ChatMessage {
            role: "user".into(),
            text: instruction.into(),
            thinking: "".into(),
        });
        session
            .input
            .update(cx, |input, cx| input.set_value("", window, cx));
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
        workspace.invalidate_file_index();
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
        self.refresh_ai_conversation(cx);
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
        // 文本增量先攒成本批再落进消息：Arc 正文逐事件重建会把流式回复放大成平方级拷贝。
        let mut stream_delta = String::new();
        for event in remaining {
            match event {
                WorkerEvent::Runtime(RunEvent::AssistantStart) => {
                    session.saved.messages.push(ChatMessage {
                        role: "assistant".into(),
                        text: String::new().into(),
                        thinking: "".into(),
                    });
                    session.stream_index = session.saved.messages.len().checked_sub(1);
                }
                WorkerEvent::Runtime(RunEvent::Text(text)) => stream_delta.push_str(&text),
                WorkerEvent::Runtime(RunEvent::Thinking(text)) => {
                    if let Some(message) = session
                        .stream_index
                        .and_then(|index| session.saved.messages.get_mut(index))
                    {
                        let mut combined = String::with_capacity(message.thinking.len() + text.len());
                        combined.push_str(&message.thinking);
                        combined.push_str(&text);
                        // 思考只服务界面展示，超长部分直接截断，不随会话落盘放大。
                        if combined.char_indices().nth(AI_THINKING_LIMIT).is_some() {
                            let end = combined
                                .char_indices()
                                .nth(AI_THINKING_LIMIT)
                                .map(|(end, _)| end)
                                .unwrap_or(combined.len());
                            combined.truncate(end);
                        }
                        message.thinking = combined.into();
                    }
                }
                WorkerEvent::Runtime(RunEvent::Activity(activity)) => session.activity = activity,
                WorkerEvent::Runtime(RunEvent::ActivityDone) => session.activity.clear(),
                WorkerEvent::Runtime(RunEvent::ToolCallStart {
                    id,
                    name,
                    arguments,
                }) => {
                    let message_index = session
                        .stream_index
                        .unwrap_or_else(|| session.saved.messages.len().saturating_sub(1));
                    session.saved.tool_calls.push(ToolCallRecord {
                        message_index,
                        id,
                        name,
                        arguments,
                        status: ToolCallStatus::Running,
                        result: String::new(),
                    });
                    if session.saved.tool_calls.len() > AI_TOOL_TRACE_LIMIT {
                        let excess = session.saved.tool_calls.len() - AI_TOOL_TRACE_LIMIT;
                        session.saved.tool_calls.drain(..excess);
                    }
                }
                WorkerEvent::Runtime(RunEvent::ToolCallEnd {
                    id,
                    name,
                    ok,
                    result,
                }) => {
                    let status = if ok {
                        ToolCallStatus::Succeeded
                    } else {
                        ToolCallStatus::Failed
                    };
                    if let Some(record) = session
                        .saved
                        .tool_calls
                        .iter_mut()
                        .rev()
                        .find(|record| record.id == id)
                    {
                        record.status = status;
                        record.result = result;
                    } else {
                        let message_index = session
                            .stream_index
                            .unwrap_or_else(|| session.saved.messages.len().saturating_sub(1));
                        session.saved.tool_calls.push(ToolCallRecord {
                            message_index,
                            id,
                            name,
                            arguments: String::new(),
                            status,
                            result,
                        });
                    }
                    // 并行工具调用时按仍在运行的最后一项恢复活动文案，不能一律清空。
                    session.activity = session
                        .saved
                        .tool_calls
                        .iter()
                        .rev()
                        .find(|record| record.status == ToolCallStatus::Running)
                        .map(|record| record.name.clone())
                        .unwrap_or_default();
                }
                WorkerEvent::Runtime(RunEvent::Checkpoint { workspace, history }) => {
                    session.saved.workspace = *workspace;
                    session.review_dirty = true;
                    session.saved.history = history;
                    checkpoint = true;
                }
                WorkerEvent::Finished(result) => {
                    session.saved.running = false;
                    let strings = cx
                        .global::<crate::i18n::I18nManager>()
                        .strings()
                        .clone();
                    match result {
                        Ok(output) => {
                            session.truncated = output
                                .error
                                .as_deref()
                                == Some(crate::ai::error_codes::LENGTH);
                            session.saved.history = output.history;
                            session.error = output
                                .error
                                .map(|error| crate::ai::error_text(&error, &strings));
                        }
                        Err(error) => session.error = Some(error),
                    }
                    if session.error.is_some() {
                        session.saved.fail_in_flight_tools();
                        session.activity.clear();
                    }
                    finished = true;
                }
                WorkerEvent::Refresh { .. } => {}
            }
        }
        if !stream_delta.is_empty() {
            if let Some(message) = session
                .stream_index
                .and_then(|index| session.saved.messages.get_mut(index))
            {
                let mut combined =
                    String::with_capacity(message.text.len() + stream_delta.len());
                combined.push_str(&message.text);
                combined.push_str(&stream_delta);
                message.text = combined.into();
            }
        }
        if finished {
            self.refresh_ai_preview(cx);
        } else {
            self.refresh_ai_conversation(cx);
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
            session.saved.fail_in_flight_tools();
            session.activity.clear();
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
        if let Some(conversation) = &session.conversation {
            let show_thinking = session.settings.show_thinking;
            conversation.update(cx, |view, cx| view.sync(&session.saved, show_thinking, cx));
            return;
        }
        if session.saved.messages.is_empty() {
            return;
        }
        let owner = cx.weak_entity();
        let show_thinking = session.settings.show_thinking;
        let preview =
            cx.new(|cx| messages::MessageTranscript::new(owner, &session.saved, show_thinking, cx));
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
                    crate::ui::window_content::<crate::config::preferences::PreferencesWindow>(window, cx)
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
                Ok(settings) => {
                    session.settings = settings;
                    session.set_settings_read_error(None);
                }
                Err(error) => {
                    session.set_settings_read_error(Some(error.to_string()));
                }
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
        if session.settings_readonly() {
            session.set_settings_read_error(Some(
                cx.global::<crate::i18n::I18nManager>()
                    .strings()
                    .ai_error_settings_readonly
                    .clone(),
            ));
            cx.notify();
            return;
        }
        let id = session.saved.id.clone();
        let baseline = session.settings.clone();
        session.settings_saving = true;
        let save = cx.background_spawn(async move {
            // 面板持有的是打开时的设置快照；偏好页或别的窗口可能已经写过。
            // 整文件覆盖前先比磁盘，避免把别人的修改静默回滚。
            if let Ok(disk) = AiSettings::load()
                && disk != baseline
            {
                anyhow::bail!("AI 设置已在其他窗口更新，请重新打开助手后再保存");
            }
            settings.save_at(&crate::ai::settings::settings_path()?)?;
            Ok::<_, anyhow::Error>(settings)
        });
        cx.spawn(async move |owner, cx| {
            let result = save.await;
            let bumped = result.is_ok();
            if let Err(error) = owner.update(cx, |editor, cx| {
                if bumped {
                    // 面板写入也推进版本号：其他窗口的会话与打开着的偏好页
                    // 按版本重载，不再持旧快照把这笔写入回滚掉。
                    crate::ai::settings::AiFeature::bump_revision(cx);
                }
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

/// 工作区内文件的相对路径（一律 `/` 分隔）；不在工作区内返回 None。
/// 文档 id 与上下文里的 path 共用这一处换算：给模型看的路径必须和工具
/// 接受的参数同一口径，两处各算一次必然漂移。
pub(in crate::editor) fn ai_workspace_relative_path(path: &Path, root: Option<&Path>) -> Option<String> {
    let root = root?;
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let relative = canonical.strip_prefix(root).ok()?;
    (!relative.as_os_str().is_empty()).then(|| relative.to_string_lossy().replace('\\', "/"))
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
        if let Some(relative) = ai_workspace_relative_path(path, root) {
            return relative;
        }
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        return format!(
            "external:{}",
            canonical.to_string_lossy().replace('\\', "/")
        );
    }
    format!("untitled:{recovery}")
}

#[cfg(test)]
mod tests;
