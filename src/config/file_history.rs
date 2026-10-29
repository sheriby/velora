use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

mod delta;

pub(crate) const MAX_SNAPSHOT_BYTES: usize = 1024 * 1024;
const MAX_DELTA_DEPTH: usize = 16;

pub(crate) fn supports_document(file: &Path, bytes: u64) -> bool {
    crate::file_url::is_markdown_document(file) && bytes <= MAX_SNAPSHOT_BYTES as u64
}

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

fn lock_history() -> std::io::Result<(std::sync::MutexGuard<'static, ()>, std::fs::File)> {
    let guard = HISTORY_LOCK
        .lock()
        .map_err(|_| std::io::Error::other("文件历史锁已损坏"))?;
    let folder = super::VeloraConfigDirs::from_system()
        .map_err(std::io::Error::other)?
        .history_root();
    std::fs::create_dir_all(&folder)?;
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(folder.join(".lock"))?;
    file.lock()?;
    Ok((guard, file))
}

pub(crate) fn read_snapshot_bytes(path: &Path) -> std::io::Result<Vec<u8>> {
    let _guard = lock_history()?;
    read_snapshot_with_depth(path, 0).map(|(bytes, _)| bytes)
}

fn read_limited(path: &Path) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(MAX_SNAPSHOT_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_SNAPSHOT_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "历史版本超过 1 MiB",
        ));
    }
    Ok(bytes)
}

fn read_snapshot_with_depth(path: &Path, level: usize) -> std::io::Result<(Vec<u8>, usize)> {
    let payload = read_limited(path)?;
    if path
        .extension()
        .is_none_or(|extension| extension != "delta")
    {
        return Ok((payload, 0));
    }
    if level >= MAX_DELTA_DEPTH {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "历史增量链超过上限",
        ));
    }
    let patch = delta::Delta::decode(&payload)?;
    let sequence = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .and_then(|stem| stem.parse::<u128>().ok());
    if sequence.is_none_or(|sequence| patch.base_sequence >= sequence) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "历史增量的基准序号无效",
        ));
    }
    let folder = path
        .parent()
        .ok_or_else(|| std::io::Error::other("历史目录无效"))?;
    let full = folder.join(format!("{}.md", patch.base_sequence));
    let base = match std::fs::metadata(&full) {
        Ok(_) => full,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            folder.join(format!("{}.delta", patch.base_sequence))
        }
        Err(error) => return Err(error),
    };
    let (bytes, depth) = read_snapshot_with_depth(&base, level + 1)?;
    Ok((patch.apply(&bytes, MAX_SNAPSHOT_BYTES)?, depth + 1))
}

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
    if version
        .path
        .extension()
        .is_some_and(|extension| extension == "md")
    {
        match std::fs::remove_file(version.path.with_extension("delta")) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
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
        let entry = entry?;
        let path = entry.path();
        if path
            .extension()
            .is_none_or(|extension| extension != "md" && extension != "delta")
        {
            continue;
        }
        if entry.metadata()?.len() > MAX_SNAPSHOT_BYTES as u64 {
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
        if path
            .extension()
            .is_some_and(|extension| extension == "delta")
            && path.with_extension("md").exists()
        {
            continue;
        }
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
    let mut removed = Vec::new();
    for version in versions {
        if kept.len() >= policy.max_entries
            || now
                .duration_since(version.timestamp)
                .unwrap_or_default()
                .as_secs()
                >= policy.retention_days.saturating_mul(86400)
        {
            removed.push(version);
        } else {
            kept.push(version);
        }
    }
    let retained: std::collections::HashSet<_> =
        kept.iter().map(|version| version.sequence).collect();
    if !removed.is_empty() {
        for version in &mut kept {
            if version
                .path
                .extension()
                .is_some_and(|extension| extension == "delta")
            {
                let payload = read_limited(&version.path)?;
                let patch = delta::Delta::decode(&payload)?;
                if !retained.contains(&patch.base_sequence) {
                    let (bytes, _) = read_snapshot_with_depth(&version.path, 0)?;
                    let full = version.path.with_extension("md");
                    // 新基准先原子落盘，再移除旧增量及祖先；中途退出仍有可读版本。
                    super::write_config_bytes_atomic(&full, &bytes)?;
                    std::fs::remove_file(&version.path)?;
                    version.path = full;
                }
            }
        }
    }
    for version in removed {
        remove_version(&version)?;
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
    if !supports_document(file, bytes.len() as u64) {
        return Ok(());
    }
    let _guard = lock_history()?;
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
    let previous = versions
        .first()
        .map(|last| read_snapshot_with_depth(&last.path, 0))
        .transpose()?;
    if let (Some(last), Some((previous, _))) = (versions.first(), previous.as_ref()) {
        if previous == bytes {
            if source != HistorySource::Automatic && source != HistorySource::Baseline {
                write_metadata(&last.path, source, now)?;
            }
            return Ok(());
        }
    }
    let patch = match (versions.first(), previous.as_ref()) {
        (Some(last), Some((before, depth))) if *depth < MAX_DELTA_DEPTH => {
            delta::encode(last.sequence, before, bytes)
        }
        _ => None,
    };
    let extension = if patch.is_some() { "delta" } else { "md" };
    let payload = patch.as_deref().unwrap_or(bytes);
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
    // pending 名称同时作跨进程占位；内容和元数据落稳后再发布版本。
    let (path, temporary, mut snapshot) = loop {
        let path = folder.join(format!("{stamp}.{extension}"));
        let temporary = folder.join(format!("{stamp}.pending"));
        match options.open(&temporary) {
            Ok(snapshot)
                if !folder.join(format!("{stamp}.md")).exists()
                    && !folder.join(format!("{stamp}.delta")).exists() =>
            {
                break (path, temporary, snapshot);
            }
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
        snapshot.write_all(payload)?;
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
    if !supports_document(file, 0) {
        return Ok(Vec::new());
    }
    let _guard = lock_history()?;
    load_and_prune(file, policy, now)
}

/// `baseline` 是调用方刚刚为版本校验读过的那份文件内容：保存链路同一次提交里
/// 已经为校验完整读过盘，这里再读一遍是同一份字节的第二遍。`None` 才自己读。
pub(crate) fn capture_baseline_with(file: &Path, baseline: Option<&[u8]>) -> std::io::Result<()> {
    if !supports_document(file, 0) {
        return Ok(());
    }
    match std::fs::metadata(file) {
        Ok(metadata) if !supports_document(file, metadata.len()) => return Ok(()),
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    }
    let policy = super::read_app_preferences()
        .map(|preferences| preferences.file_history)
        .unwrap_or_default();
    if !list_versions(file, policy, SystemTime::now())?.is_empty() {
        return Ok(());
    }
    if let Some(bytes) = baseline {
        return record_snapshot_at(
            file,
            bytes,
            HistorySource::Baseline,
            policy,
            SystemTime::now(),
        );
    }
    match read_limited(file) {
        Ok(bytes) => record_snapshot_at(
            file,
            &bytes,
            HistorySource::Baseline,
            policy,
            SystemTime::now(),
        ),
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::InvalidData
            ) =>
        {
            Ok(())
        }
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests;
