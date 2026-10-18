use super::workspace::AiWorkspace;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ChatMessage {
    pub role: String,
    pub text: String,
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
        let mut writes = writes()
            .lock()
            .map_err(|_| anyhow::anyhow!("AI 会话写入状态不可用"))?;
        if writes
            .get(path)
            .is_some_and(|sequence| *sequence > self.sequence)
        {
            return Ok(());
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        crate::config::write_config_file_atomic(path, &serde_json::to_string(self)?)?;
        writes.insert(path.to_path_buf(), self.sequence);
        Ok(())
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

#[cfg(test)]
mod tests;
