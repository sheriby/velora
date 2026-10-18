use super::workspace::AiWorkspace;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ChatMessage {
    pub role: String,
    /// 对话正文用 `Arc<str>`：渲染每帧引用历史消息，逐字节克隆会随轮数线性放大。
    pub text: std::sync::Arc<str>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum SessionKind {
    #[default]
    Conversation,
    QuickCommand,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct SavedSession {
    #[serde(default)]
    pub kind: SessionKind,
    #[serde(default)]
    pub prompt_profile: Option<super::instructions::PromptProfile>,
    pub format: u32,
    #[serde(default)]
    pub sequence: u64,
    pub id: String,
    pub workspace: AiWorkspace,
    pub history: Vec<Value>,
    pub messages: Vec<ChatMessage>,
    pub interrupted: bool,
    pub running: bool,
    #[serde(default)]
    pub target: Option<TargetSnapshot>,
    #[serde(default)]
    pub endpoint_id: String,
    #[serde(default)]
    pub bindings: std::collections::BTreeMap<String, PathBuf>,
    #[serde(default)]
    pub references: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct TargetSnapshot {
    pub document_id: String,
    pub path: Option<PathBuf>,
    pub range: std::ops::Range<usize>,
    pub identity: u64,
    #[serde(default)]
    pub selection: String,
    #[serde(default)]
    pub version: u64,
}

impl SavedSession {
    pub fn new(workspace: AiWorkspace) -> Self {
        Self {
            kind: SessionKind::Conversation,
            prompt_profile: None,
            format: 1,
            sequence: 0,
            id: uuid::Uuid::new_v4().to_string(),
            workspace,
            history: Vec::new(),
            messages: Vec::new(),
            interrupted: false,
            running: false,
            target: None,
            endpoint_id: String::new(),
            bindings: Default::default(),
            references: Vec::new(),
        }
    }
    pub fn save_at(&self, path: &Path) -> Result<()> {
        // 序列化与建目录在锁外：大会话的 JSON 组装不该占着全局写序锁阻塞
        // 其他会话的写盘判定。锁内的检查+写入仍须成对，保住同路径写的顺序。
        let contents = serde_json::to_string(self)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut writes = writes()
            .lock()
            .map_err(|_| anyhow::anyhow!("AI 会话写入状态不可用"))?;
        if writes
            .get(path)
            .is_some_and(|sequence| *sequence > self.sequence)
        {
            return Ok(());
        }
        crate::config::write_config_file_atomic(path, &contents)?;
        writes.insert(path.to_path_buf(), self.sequence);
        Ok(())
    }

    /// 销毁路径的后台落盘：Drop 里没有执行器可用，同步写会把整份 JSON 的
    /// 序列化与 fsync 放到 UI 线程上。句柄登记进进程级表，应用退出时由
    /// `flush_pending_saves` 等它写完。
    pub fn save_in_background(mut saved: SavedSession, context: &'static str) {
        if saved.messages.is_empty() && saved.workspace.changes.proposals.is_empty() {
            return;
        }
        saved.interrupted |= saved.running;
        saved.running = false;
        saved.sequence = saved.sequence.saturating_add(1);
        let path = match saved.path() {
            Ok(path) => path,
            Err(error) => {
                eprintln!("{context}：确定会话文件路径失败：{error}");
                return;
            }
        };
        match std::thread::Builder::new()
            .name("velora-ai-session-save".into())
            .spawn(move || {
                if let Err(error) = saved.save_at(&path) {
                    eprintln!("{context}：{error}");
                }
            })
        {
            Ok(handle) => {
                if let Ok(mut saves) = pending_saves().lock() {
                    saves.retain(|handle: &std::thread::JoinHandle<()>| !handle.is_finished());
                    saves.push(handle);
                }
            }
            Err(error) => eprintln!("{context}：启动保存线程失败：{error}"),
        }
    }
    pub fn delete_at(path: &Path) -> Result<()> {
        let mut writes = writes()
            .lock()
            .map_err(|_| anyhow::anyhow!("AI 会话写入状态不可用"))?;
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        writes.insert(path.to_path_buf(), u64::MAX);
        Ok(())
    }

    pub fn load_at(path: &Path, workspace: Option<&Path>) -> Result<Self> {
        if path.metadata()?.len() > 256 * 1024 * 1024 {
            bail!("会话文件超过读取预算");
        }
        let mut session: Self = serde_json::from_slice(&std::fs::read(path)?)?;
        if session.format != 1 {
            bail!("不支持的 AI 会话版本：{}", session.format);
        }
        let expected = workspace.map(std::fs::canonicalize).transpose()?;
        if session.workspace.root != expected {
            bail!("此会话属于另一个工作区");
        }
        if session.workspace.read_only || session.workspace.selection_scope.is_some() {
            session.kind = SessionKind::QuickCommand;
        }
        session.interrupted |= session.running;
        session.running = false;
        Ok(session)
    }

    pub fn directory(workspace: Option<&Path>) -> Result<PathBuf> {
        use sha2::{Digest, Sha256};
        let key = workspace
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|| "untitled".into());
        let hash = format!("{:x}", Sha256::digest(key.as_bytes()));
        Ok(super::settings::settings_path()?
            .with_file_name("ai")
            .join("sessions")
            .join(hash))
    }

    pub fn path(&self) -> Result<PathBuf> {
        let id = uuid::Uuid::parse_str(&self.id).context("会话标识无效")?;
        Ok(Self::directory(self.workspace.root.as_deref())?.join(format!("{id}.json")))
    }
}

fn writes() -> &'static std::sync::Mutex<std::collections::BTreeMap<PathBuf, u64>> {
    static WRITES: std::sync::OnceLock<std::sync::Mutex<std::collections::BTreeMap<PathBuf, u64>>> =
        std::sync::OnceLock::new();
    WRITES.get_or_init(Default::default)
}

fn pending_saves() -> &'static std::sync::Mutex<Vec<std::thread::JoinHandle<()>>> {
    static PENDING: std::sync::OnceLock<std::sync::Mutex<Vec<std::thread::JoinHandle<()>>>> =
        std::sync::OnceLock::new();
    PENDING.get_or_init(Default::default)
}

/// 应用退出前等待所有后台会话写线程完成，避免最后一轮状态丢失。
pub fn flush_pending_saves() {
    if let Ok(mut saves) = pending_saves().lock() {
        for handle in saves.drain(..) {
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
mod tests;
