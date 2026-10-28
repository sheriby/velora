//! 保存尚未成功写入文档的恢复快照。

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::VeloraConfigDirs;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct RecoverySnapshot {
    pub(crate) id: Uuid,
    pub(crate) source_path: Option<PathBuf>,
    pub(crate) markdown: String,
}

pub(crate) fn save_recovery_snapshot(snapshot: &RecoverySnapshot) -> Result<()> {
    save_recovery_snapshot_with_dir(snapshot, &VeloraConfigDirs::from_system()?.recovery_dir())
}

pub(crate) fn save_recovery_snapshot_with_dir(snapshot: &RecoverySnapshot, dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir)
        .with_context(|| format!("failed to create recovery directory '{}'", dir.display()))?;
    let path = dir.join(format!("{}.json", snapshot.id));
    let contents = serde_json::to_vec(snapshot).context("failed to encode recovery snapshot")?;
    // 单点判据 write_config_bytes_atomic：临时文件**建出就收紧 0600** 再写内容、
    // sync_all、rename。旧实现先以 umask 默认权限把未保存正文全文写完才 chmod，
    // 多用户机器上有一段同机其他用户可读的窗口。
    crate::config::write_config_bytes_atomic(&path, &contents)
        .with_context(|| format!("failed to install recovery snapshot '{}'", path.display()))?;
    Ok(())
}

pub(crate) fn read_recovery_snapshots() -> Result<Vec<RecoverySnapshot>> {
    read_recovery_snapshots_from_dir(&VeloraConfigDirs::from_system()?.recovery_dir())
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
    let mut failures = 0usize;
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                eprintln!("读取恢复目录条目失败 '{}': {error}", dir.display());
                failures += 1;
                continue;
            }
        };
        if entry
            .path()
            .extension()
            .is_none_or(|extension| extension != "json")
        {
            continue;
        }
        let contents = match std::fs::read(entry.path()) {
            Ok(contents) => contents,
            Err(error) => {
                // 单条目读失败（权限被改、被杀毒/同步盘独占）不再一票否决：
                // 那会把本次启动的全部未保存草稿一起放弃，而且坏文件不删的
                // 话，每次启动都在同一个地方失败。记日志、隔离、继续读其余的。
                eprintln!(
                    "读取恢复快照失败 '{}'：{error}（本次跳过，改名隔离）",
                    entry.path().display()
                );
                failures += 1;
                quarantine_unreadable_snapshot(&entry.path());
                continue;
            }
        };
        match serde_json::from_slice::<RecoverySnapshot>(&contents) {
            Ok(snapshot) => snapshots.push((snapshot, entry.metadata().ok().and_then(|metadata| metadata.modified().ok()))),
            Err(error) => eprintln!(
                "ignoring invalid recovery snapshot '{}': {error}",
                entry.path().display()
            ),
        }
    }
    if snapshots.is_empty() && failures > 0 {
        return Err(anyhow::anyhow!(
            "恢复目录 '{}' 里的 {failures} 个快照全部读取失败，本次启动没有可恢复的草稿",
            dir.display()
        ));
    }
    // 恢复顺序按保存时间走，最新的先恢复：id 是纯随机 UUID，按 id 排序得到的
    // 顺序与崩溃前的时间线无关。快照文件的 mtime 就是最后一次
    // 自动保存的时刻；读不到 mtime 的按 epoch 兜底排最后，同刻的按 id 稳定。
    snapshots.sort_by(|(left, left_time), (right, right_time)| {
        (*right_time)
            .unwrap_or(std::time::UNIX_EPOCH)
            .cmp(&(*left_time).unwrap_or(std::time::UNIX_EPOCH))
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(snapshots.into_iter().map(|(snapshot, _)| snapshot).collect())
}

/// 反复读不出来的快照改名 `.bad` 隔离：不删（未保存正文还在，可人工抢救），
/// 但下一次启动不再把它当快照重试，恢复链路不会被一个坏文件永久卡死。
fn quarantine_unreadable_snapshot(path: &Path) {
    let mut quarantined = path.as_os_str().to_os_string();
    quarantined.push(".bad");
    if let Err(error) = std::fs::rename(path, &quarantined) {
        eprintln!("隔离损坏的恢复快照 '{}' 失败：{error}", path.display());
    }
}

pub(crate) fn remove_recovery_snapshot(id: Uuid) -> Result<()> {
    remove_recovery_snapshot_from_dir(id, &VeloraConfigDirs::from_system()?.recovery_dir())
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

    fn test_snapshot() -> RecoverySnapshot {
        RecoverySnapshot {
            id: uuid::Uuid::new_v4(),
            source_path: Some(Path::new("/notes/draft.md").to_path_buf()),
            markdown: "# 恢复内容".to_string(),
        }
    }

    #[test]
    fn recovery_snapshots_round_trip_and_can_be_removed() {
        let root =
            std::env::temp_dir().join(format!("velora-recovery-test-{}", uuid::Uuid::new_v4()));
        let snapshot = test_snapshot();

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

    /// 一个读不出来的条目（权限被改、被杀毒独占，这里用「目录顶着 .json 名字」
    /// 这个三平台都成立的读失败形态）不能拖垮整批恢复：其余草稿照常恢复，
    /// 坏条目被改名 `.bad` 隔离，下次启动不再挡路。
    #[test]
    fn one_unreadable_entry_does_not_abandon_the_rest() {
        let root = std::env::temp_dir().join(format!(
            "velora-recovery-unreadable-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let readable = test_snapshot();
        save_recovery_snapshot_with_dir(&readable, &root).expect("save snapshot");

        let broken = root.join(format!("{}.json", uuid::Uuid::new_v4()));
        std::fs::create_dir(&broken).unwrap();

        let snapshots = read_recovery_snapshots_from_dir(&root)
            .expect("一个坏条目不能让整目录的恢复失败");
        assert_eq!(
            snapshots,
            vec![readable],
            "其余快照必须照常读出来"
        );
        let quarantined = broken.with_extension("json.bad");
        assert!(!broken.exists(), "坏条目要被隔离改名，不能挡下一次启动");
        assert!(quarantined.exists(), "隔离是改名不是删除，内容还在");

        let _ = std::fs::remove_dir_all(root);
    }

    /// 恢复顺序按快照的保存时间走，最新的先恢复；按 UUID 排序得到的是随机序
    /// （id 是纯随机 UUID），与崩溃前的时间线无关。
    #[test]
    fn snapshots_restore_newest_first() {
        let root =
            std::env::temp_dir().join(format!("velora-recovery-order-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();

        let mut older = test_snapshot();
        older.id = uuid::Uuid::new_v4();
        let mut newer = test_snapshot();
        newer.id = uuid::Uuid::new_v4();
        // 让「UUID 排序」与「时间排序」必然打架：旧草稿拿一个大 id。
        if older.id < newer.id {
            std::mem::swap(&mut older.id, &mut newer.id);
        }
        save_recovery_snapshot_with_dir(&older, &root).unwrap();
        save_recovery_snapshot_with_dir(&newer, &root).unwrap();
        // 显式拨 mtime：文件系统时间戳粒度可能吃掉两次快速写入的先后。
        let now = std::time::SystemTime::now();
        for (path, time) in [
            (root.join(format!("{}.json", older.id)), now - std::time::Duration::from_secs(60)),
            (root.join(format!("{}.json", newer.id)), now),
        ] {
            std::fs::OpenOptions::new()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(time)
                .unwrap();
        }

        let snapshots = read_recovery_snapshots_from_dir(&root).expect("read snapshots");
        assert_eq!(
            snapshots.iter().map(|snapshot| snapshot.id).collect::<Vec<_>>(),
            vec![newer.id, older.id],
            "最新的草稿要排在前面"
        );

        let _ = std::fs::remove_dir_all(root);
    }
}
