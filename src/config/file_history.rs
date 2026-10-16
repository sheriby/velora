use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct HistoryPolicy {
    pub(crate) interval_minutes: u64,
    pub(crate) max_entries: usize,
    pub(crate) retention_days: u64,
}

impl Default for HistoryPolicy {
    fn default() -> Self {
        Self {
            interval_minutes: 5,
            max_entries: 30,
            retention_days: 7,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum HistorySource {
    Automatic,
    Manual,
    Exit,
    BeforeRestore,
    Baseline,
    Legacy,
}

#[derive(Clone, Debug)]
pub(crate) struct HistoryVersion {
    pub(crate) path: PathBuf,
    pub(crate) timestamp: SystemTime,
    pub(crate) source: HistorySource,
    sequence: u128,
}

#[derive(Serialize, Deserialize)]
struct Metadata {
    timestamp_nanos: u128,
    source: HistorySource,
}

static HISTORY_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

impl HistoryPolicy {
    pub(crate) fn normalized(self) -> Self {
        Self {
            interval_minutes: self.interval_minutes.min(1440),
            max_entries: self.max_entries.clamp(1, 1000),
            retention_days: self.retention_days.clamp(1, 3650),
        }
    }
}

fn directory(file: &Path) -> std::io::Result<PathBuf> {
    use std::hash::{Hash, Hasher};
    let dirs = super::VeloraConfigDirs::from_system().map_err(std::io::Error::other)?;
    let mut hasher = std::hash::DefaultHasher::new();
    file.hash(&mut hasher);
    Ok(dirs
        .history_root()
        .join(format!("{:016x}", hasher.finish())))
}

fn nanos(time: SystemTime) -> u128 {
    time.duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

fn timestamp(value: u128) -> std::io::Result<SystemTime> {
    let seconds = u64::try_from(value / 1_000_000_000).map_err(std::io::Error::other)?;
    SystemTime::UNIX_EPOCH
        .checked_add(std::time::Duration::new(
            seconds,
            (value % 1_000_000_000) as u32,
        ))
        .ok_or_else(|| std::io::Error::other("历史时间戳超出支持范围"))
}

fn remove_version(version: &HistoryVersion) -> std::io::Result<()> {
    std::fs::remove_file(&version.path)?;
    match std::fs::remove_file(version.path.with_extension("json")) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn load_and_prune(
    file: &Path,
    policy: HistoryPolicy,
    now: SystemTime,
) -> std::io::Result<Vec<HistoryVersion>> {
    let policy = policy.normalized();
    let entries = match std::fs::read_dir(directory(file)?) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut versions = Vec::new();
    for entry in entries {
        let path = entry?.path();
        if path.extension().is_none_or(|extension| extension != "md") {
            continue;
        }
        let Some(stamp) = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .and_then(|stem| stem.parse::<u128>().ok())
        else {
            continue;
        };
        let metadata = match std::fs::read(path.with_extension("json")) {
            Ok(bytes) => {
                serde_json::from_slice::<Metadata>(&bytes).map_err(std::io::Error::other)?
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Metadata {
                timestamp_nanos: stamp,
                source: HistorySource::Legacy,
            },
            Err(error) => return Err(error),
        };
        versions.push(HistoryVersion {
            path,
            timestamp: timestamp(metadata.timestamp_nanos)?,
            source: metadata.source,
            sequence: stamp,
        });
    }
    versions.sort_by(|left, right| {
        right
            .timestamp
            .cmp(&left.timestamp)
            .then_with(|| right.sequence.cmp(&left.sequence))
    });
    let mut kept = Vec::new();
    for version in versions {
        if kept.len() >= policy.max_entries
            || now
                .duration_since(version.timestamp)
                .unwrap_or_default()
                .as_secs()
                >= policy.retention_days.saturating_mul(86400)
        {
            remove_version(&version)?;
        } else {
            kept.push(version);
        }
    }
    Ok(kept)
}

fn write_metadata(path: &Path, source: HistorySource, now: SystemTime) -> std::io::Result<()> {
    let metadata = serde_json::to_string(&Metadata {
        timestamp_nanos: nanos(now),
        source,
    })
    .map_err(std::io::Error::other)?;
    super::write_config_file_atomic(&path.with_extension("json"), &metadata)
}

pub(crate) fn record_snapshot_at(
    file: &Path,
    bytes: &[u8],
    source: HistorySource,
    policy: HistoryPolicy,
    now: SystemTime,
) -> std::io::Result<()> {
    let _guard = HISTORY_LOCK
        .lock()
        .map_err(|_| std::io::Error::other("文件历史锁已损坏"))?;
    let policy = policy.normalized();
    let versions = load_and_prune(file, policy, now)?;
    if source == HistorySource::Automatic {
        if let Some(last) = versions
            .iter()
            .find(|version| version.source != HistorySource::Baseline)
        {
            if now
                .duration_since(last.timestamp)
                .unwrap_or_default()
                .as_secs()
                < policy.interval_minutes.saturating_mul(60)
            {
                return Ok(());
            }
        }
    }
    if let Some(last) = versions.first() {
        if std::fs::read(&last.path)? == bytes {
            if source != HistorySource::Automatic && source != HistorySource::Baseline {
                write_metadata(&last.path, source, now)?;
            }
            return Ok(());
        }
    }
    let folder = directory(file)?;
    std::fs::create_dir_all(&folder)?;
    let mut stamp = versions
        .iter()
        .map(|entry| entry.sequence)
        .max()
        .map_or(nanos(now), |last| nanos(now).max(last.saturating_add(1)));
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    // pending 名称同时作跨进程占位；只在内容和元数据落稳后发布 .md。
    let (path, temporary, mut snapshot) = loop {
        let path = folder.join(format!("{stamp}.md"));
        let temporary = folder.join(format!("{stamp}.pending"));
        match options.open(&temporary) {
            Ok(snapshot) if !path.exists() => break (path, temporary, snapshot),
            Ok(snapshot) => {
                drop(snapshot);
                std::fs::remove_file(&temporary)?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
        stamp = stamp
            .checked_add(1)
            .ok_or_else(|| std::io::Error::other("历史版本序号已耗尽"))?;
    };
    let mut metadata_written = false;
    let result = (|| {
        snapshot.write_all(bytes)?;
        snapshot.sync_all()?;
        drop(snapshot);
        write_metadata(&path, source, now)?;
        metadata_written = true;
        std::fs::rename(&temporary, &path)
    })();
    if let Err(error) = result {
        for leftover in
            std::iter::once(temporary).chain(metadata_written.then(|| path.with_extension("json")))
        {
            if let Err(cleanup_error) = std::fs::remove_file(&leftover)
                && cleanup_error.kind() != std::io::ErrorKind::NotFound
            {
                eprintln!(
                    "清理历史临时文件失败（{}）：{cleanup_error}",
                    leftover.display()
                );
            }
        }
        return Err(error);
    }
    load_and_prune(file, policy, now)?;
    Ok(())
}

pub(crate) fn list_versions(
    file: &Path,
    policy: HistoryPolicy,
    now: SystemTime,
) -> std::io::Result<Vec<HistoryVersion>> {
    let _guard = HISTORY_LOCK
        .lock()
        .map_err(|_| std::io::Error::other("文件历史锁已损坏"))?;
    load_and_prune(file, policy, now)
}

pub(crate) fn capture_baseline(file: &Path) -> std::io::Result<()> {
    let policy = super::read_app_preferences()
        .map(|preferences| preferences.file_history)
        .unwrap_or_default();
    if !list_versions(file, policy, SystemTime::now())?.is_empty() {
        return Ok(());
    }
    match std::fs::read(file) {
        Ok(bytes) => record_snapshot_at(
            file,
            &bytes,
            HistorySource::Baseline,
            policy,
            SystemTime::now(),
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests;
