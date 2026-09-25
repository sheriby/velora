//! 保存尚未成功写入文档的恢复快照。

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::VelotypeConfigDirs;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct RecoverySnapshot {
    pub(crate) id: Uuid,
    pub(crate) source_path: Option<PathBuf>,
    pub(crate) markdown: String,
}

pub(crate) fn save_recovery_snapshot(snapshot: &RecoverySnapshot) -> Result<()> {
    save_recovery_snapshot_with_dir(snapshot, &VelotypeConfigDirs::from_system()?.recovery_dir())
}

fn save_recovery_snapshot_with_dir(snapshot: &RecoverySnapshot, dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir)
        .with_context(|| format!("failed to create recovery directory '{}'", dir.display()))?;
    let path = dir.join(format!("{}.json", snapshot.id));
    let temporary_path = dir.join(format!("{}.json.tmp", snapshot.id));
    let contents = serde_json::to_vec(snapshot).context("failed to encode recovery snapshot")?;
    std::fs::write(&temporary_path, contents).with_context(|| {
        format!(
            "failed to write recovery snapshot '{}'",
            temporary_path.display()
        )
    })?;
    std::fs::rename(&temporary_path, &path)
        .with_context(|| format!("failed to install recovery snapshot '{}'", path.display()))?;
    Ok(())
}

pub(crate) fn read_recovery_snapshots() -> Result<Vec<RecoverySnapshot>> {
    read_recovery_snapshots_from_dir(&VelotypeConfigDirs::from_system()?.recovery_dir())
}

fn read_recovery_snapshots_from_dir(dir: &Path) -> Result<Vec<RecoverySnapshot>> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to read recovery directory '{}'", dir.display()));
        }
    };

    let mut snapshots = Vec::new();
    for entry in entries {
        let entry = entry?;
        if entry
            .path()
            .extension()
            .is_none_or(|extension| extension != "json")
        {
            continue;
        }
        let contents = std::fs::read(entry.path()).with_context(|| {
            format!(
                "failed to read recovery snapshot '{}'",
                entry.path().display()
            )
        })?;
        match serde_json::from_slice::<RecoverySnapshot>(&contents) {
            Ok(snapshot) => snapshots.push(snapshot),
            Err(error) => eprintln!(
                "ignoring invalid recovery snapshot '{}': {error}",
                entry.path().display()
            ),
        }
    }
    snapshots.sort_by_key(|snapshot| snapshot.id);
    Ok(snapshots)
}

pub(crate) fn remove_recovery_snapshot(id: Uuid) -> Result<()> {
    remove_recovery_snapshot_from_dir(id, &VelotypeConfigDirs::from_system()?.recovery_dir())
}

fn remove_recovery_snapshot_from_dir(id: Uuid, dir: &Path) -> Result<()> {
    let path = dir.join(format!("{id}.json"));
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error)
            .with_context(|| format!("failed to remove recovery snapshot '{}'", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        RecoverySnapshot, read_recovery_snapshots_from_dir, remove_recovery_snapshot_from_dir,
        save_recovery_snapshot_with_dir,
    };
    use std::path::Path;

    #[test]
    fn recovery_snapshots_round_trip_and_can_be_removed() {
        let root =
            std::env::temp_dir().join(format!("velora-recovery-test-{}", uuid::Uuid::new_v4()));
        let snapshot = RecoverySnapshot {
            id: uuid::Uuid::new_v4(),
            source_path: Some(Path::new("/notes/draft.md").to_path_buf()),
            markdown: "# 恢复内容".to_string(),
        };

        save_recovery_snapshot_with_dir(&snapshot, &root).expect("save snapshot");
        assert_eq!(
            read_recovery_snapshots_from_dir(&root).expect("read snapshots"),
            vec![snapshot.clone()]
        );
        remove_recovery_snapshot_from_dir(snapshot.id, &root).expect("remove snapshot");
        assert!(
            read_recovery_snapshots_from_dir(&root)
                .expect("read snapshots")
                .is_empty()
        );

        let _ = std::fs::remove_dir_all(root);
    }
}
