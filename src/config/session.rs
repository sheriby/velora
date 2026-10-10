//! Last editor session persistence: workspace root + open tab set.

use anyhow::Context as _;
use serde::{Deserialize, Serialize};

use super::VeloraConfigDirs;

/// Last editor session: the workspace root and the open tab set, restored on
/// launch (roadmap A4).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub(crate) struct SessionState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) root: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) tabs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) active: Option<String>,
    /// Sidebar width remembered for this workspace root (roadmap E7).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) sidebar_width: Option<u16>,
    /// 侧栏记忆（全局，不按工作区）：上次挑的面板（搜索不参与，所以只会是
    /// `files` / `outline` / `backlinks` / `tags`）与上次是开着还是关着。
    /// 用户要求：想一直用大纲，不要每次启动都回到文件数。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) sidebar_tab: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) sidebar_open: Option<bool>,
}

pub(crate) fn read_session() -> anyhow::Result<SessionState> {
    read_session_with_dirs(&VeloraConfigDirs::from_system()?)
}

pub(crate) fn save_session(session: &SessionState) -> anyhow::Result<()> {
    save_session_with_dirs(session, &VeloraConfigDirs::from_system()?)
}

fn read_session_with_dirs(dirs: &VeloraConfigDirs) -> anyhow::Result<SessionState> {
    let path = session_file(dirs);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(SessionState::default())
        }
        Err(err) => {
            return Err(err).with_context(|| format!("failed to read '{}'", path.display()));
        }
    };
    serde_json::from_str(&text).with_context(|| format!("failed to parse '{}'", path.display()))
}

fn save_session_with_dirs(
    session: &SessionState,
    dirs: &VeloraConfigDirs,
) -> anyhow::Result<()> {
    let path = session_file(dirs);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create '{}'", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(session)?;
    crate::config::write_config_file_atomic(&path, &(text + "\n"))
        .with_context(|| format!("failed to write '{}'", path.display()))
}

fn session_file(dirs: &VeloraConfigDirs) -> std::path::PathBuf {
    dirs.root.join("session.json")
}

#[cfg(test)]
mod tests {
    use super::{SessionState, read_session_with_dirs, save_session_with_dirs};
    use crate::config::VeloraConfigDirs;

    #[test]
    fn session_write_leaves_no_partial_or_temp_files() {
        // 审查发现：session.json / config.toml 是直接覆写，半写文件会被读取端
        // 当成「没有配置」静默回退默认值（等于丢用户设置）；原子写不应留临时文件。
        let root =
            std::env::temp_dir().join(format!("velora-session-atomic-{}", uuid::Uuid::new_v4()));
        let dirs = VeloraConfigDirs::from_root(&root);
        let session = SessionState {
            root: Some("/tmp/workspace".into()),
            tabs: vec!["/tmp/workspace/a.md".into()],
            active: Some("/tmp/workspace/a.md".into()),
            sidebar_width: Some(280),
            sidebar_tab: Some("files".into()),
            sidebar_open: Some(false),
        };
        save_session_with_dirs(&session, &dirs).expect("save session");
        let leftovers: Vec<String> = std::fs::read_dir(&dirs.root)
            .expect("read config dir")
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "原子写不应留下临时文件：{leftovers:?}");
        assert_eq!(
            read_session_with_dirs(&dirs).expect("read session").sidebar_width,
            Some(280),
            "原子写不能丢内容"
        );
        assert_eq!(
            read_session_with_dirs(&dirs)
                .expect("read session")
                .sidebar_tab
                .as_deref(),
            Some("files"),
            "侧栏面板也要跟着会话走盘"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn session_roundtrips_through_disk() {
        let root =
            std::env::temp_dir().join(format!("velora-session-{}", uuid::Uuid::new_v4()));
        let dirs = VeloraConfigDirs::from_root(&root);
        let session = SessionState {
            root: Some("/tmp/workspace".into()),
            tabs: vec!["/tmp/workspace/a.md".into(), "/tmp/workspace/b.md".into()],
            active: Some("/tmp/workspace/b.md".into()),
            sidebar_width: Some(300),
            sidebar_tab: Some("outline".into()),
            sidebar_open: Some(true),
        };
        save_session_with_dirs(&session, &dirs).expect("save session");
        let loaded = read_session_with_dirs(&dirs).expect("read session");
        assert_eq!(loaded, session);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn missing_session_file_reads_as_empty() {
        let dirs = VeloraConfigDirs::from_root(
            std::env::temp_dir().join(format!("velora-session-empty-{}", uuid::Uuid::new_v4())),
        );
        let loaded = read_session_with_dirs(&dirs).expect("read session");
        assert_eq!(loaded, SessionState::default());
    }
}
