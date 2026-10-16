use super::{HistoryPolicy, HistorySource, list_versions, record_snapshot_at};
use std::time::{Duration, SystemTime};

fn fixture(check: impl FnOnce(&std::path::Path)) {
    let root = std::env::temp_dir().join(format!("velora-history-policy-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("夹具");
    let _config = crate::config::override_test_config_root(root.join("config"));
    check(&root.join("notes.md"));
    std::fs::remove_dir_all(root).expect("清理夹具");
}

fn time(seconds: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000 + seconds)
}

#[test]
fn automatic_history_observes_the_configured_minimum_interval() {
    fixture(|path| {
        let policy = HistoryPolicy::default();
        record_snapshot_at(path, b"A", HistorySource::Automatic, policy, time(0)).expect("首份");
        record_snapshot_at(path, b"B", HistorySource::Automatic, policy, time(299))
            .expect("不足五分钟");
        assert_eq!(
            list_versions(path, policy, time(299)).expect("历史").len(),
            1
        );
        record_snapshot_at(path, b"C", HistorySource::Automatic, policy, time(300))
            .expect("五分钟");
        let versions = list_versions(path, policy, time(300)).expect("历史");
        assert_eq!(versions.len(), 2);
        assert_eq!(std::fs::read(&versions[0].path).expect("最新"), b"C");
    });
}

#[test]
fn history_retains_thirty_versions_and_prunes_expired_snapshots() {
    fixture(|path| {
        let policy = HistoryPolicy::default();
        for index in 0..35 {
            record_snapshot_at(
                path,
                index.to_string().as_bytes(),
                HistorySource::Manual,
                policy,
                time(index),
            )
            .expect("记录");
        }
        assert_eq!(
            list_versions(path, policy, time(35)).expect("历史").len(),
            30
        );
        record_snapshot_at(path, b"new", HistorySource::Manual, policy, time(8 * 86400))
            .expect("七天后");
        assert_eq!(
            list_versions(path, policy, time(8 * 86400))
                .expect("清理过期")
                .len(),
            1
        );
    });
}

#[test]
fn manual_checkpoint_is_immediate_and_promotes_identical_automatic_content() {
    fixture(|path| {
        let policy = HistoryPolicy::default();
        record_snapshot_at(path, b"A", HistorySource::Automatic, policy, time(0)).expect("自动");
        record_snapshot_at(path, b"A", HistorySource::Manual, policy, time(1)).expect("手动检查点");
        let versions = list_versions(path, policy, time(1)).expect("历史");
        assert_eq!(versions.len(), 1, "相同内容不复制整份文件");
        assert_eq!(versions[0].source, HistorySource::Manual);
        record_snapshot_at(path, b"B", HistorySource::Manual, policy, time(2)).expect("新的检查点");
        assert_eq!(list_versions(path, policy, time(2)).expect("历史").len(), 2);
    });
}

#[test]
fn history_preferences_are_persisted_with_the_requested_defaults() {
    fixture(|path| {
        crate::config::load_or_create_app_preferences().expect("创建配置");
        let text = std::fs::read_to_string(path.parent().expect("目录").join("config/config.toml"))
            .expect("配置文件");
        let value: toml::Value = toml::from_str(&text).expect("配置格式");
        assert_eq!(
            value
                .get("file_history")
                .and_then(|history| history.get("interval_minutes"))
                .and_then(toml::Value::as_integer),
            Some(5)
        );
        assert_eq!(
            value
                .get("file_history")
                .and_then(|history| history.get("max_entries"))
                .and_then(toml::Value::as_integer),
            Some(30)
        );
        assert_eq!(
            value
                .get("file_history")
                .and_then(|history| history.get("retention_days"))
                .and_then(toml::Value::as_integer),
            Some(7)
        );
    });
}

#[test]
fn history_policy_custom_values_roundtrip_and_invalid_bounds_are_normalized() {
    fixture(|_| {
        let mut preferences = crate::config::preferences::AppPreferences::default();
        preferences.file_history = HistoryPolicy {
            interval_minutes: 12,
            max_entries: 4,
            retention_days: 2,
        };
        let dirs = crate::config::VeloraConfigDirs::from_system().expect("配置目录");
        crate::config::preferences::save_app_preferences_with_dirs(&preferences, &dirs)
            .expect("保存配置");
        let restored = crate::config::read_app_preferences().expect("读取配置");
        assert_eq!(restored.file_history, preferences.file_history);
        let source = std::fs::read_to_string(dirs.app_config_file())
            .expect("配置")
            .replace("max_entries = 4", "max_entries = 0")
            .replace("retention_days = 2", "retention_days = 99999");
        std::fs::write(dirs.app_config_file(), source).expect("边界配置");
        let restored = crate::config::read_app_preferences().expect("读取配置");
        assert_eq!(restored.file_history.max_entries, 1);
        assert_eq!(restored.file_history.retention_days, 3650);
    });
}

#[test]
fn history_prunes_data_and_metadata_at_the_exact_retention_boundary() {
    fixture(|path| {
        let policy = HistoryPolicy {
            max_entries: 2,
            retention_days: 1,
            ..Default::default()
        };
        record_snapshot_at(path, b"A", HistorySource::Manual, policy, time(0)).expect("版本");
        let old = list_versions(path, policy, time(0))
            .expect("历史")
            .remove(0)
            .path;
        assert_eq!(
            list_versions(path, policy, time(86399))
                .expect("未过期")
                .len(),
            1
        );
        assert!(
            list_versions(path, policy, time(86400))
                .expect("到期")
                .is_empty()
        );
        assert!(!old.exists());
        assert!(!old.with_extension("json").exists());
        for index in 0..4 {
            record_snapshot_at(
                path,
                index.to_string().as_bytes(),
                HistorySource::Manual,
                policy,
                time(90000 + index),
            )
            .expect("版本");
        }
        let kept = list_versions(path, policy, time(90004)).expect("历史");
        assert_eq!(kept.len(), 2);
        assert_eq!(
            std::fs::read_dir(kept[0].path.parent().expect("目录"))
                .expect("快照目录")
                .count(),
            4,
            "内容和元数据要一起清理"
        );
    });
}

#[test]
fn legacy_history_without_metadata_remains_readable() {
    fixture(|path| {
        use std::hash::{Hash, Hasher};
        let dirs = crate::config::VeloraConfigDirs::from_system().expect("配置目录");
        let mut hasher = std::hash::DefaultHasher::new();
        path.hash(&mut hasher);
        let folder = dirs
            .history_root()
            .join(format!("{:016x}", hasher.finish()));
        std::fs::create_dir_all(&folder).expect("旧版历史目录");
        std::fs::write(
            folder.join(format!(
                "{}.md",
                time(0)
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .expect("时间戳")
                    .as_nanos()
            )),
            "旧版内容",
        )
        .expect("旧版历史");
        let versions = list_versions(path, Default::default(), time(1)).expect("兼容旧版");
        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0].source, HistorySource::Legacy);
        assert_eq!(
            std::fs::read_to_string(&versions[0].path).expect("旧内容"),
            "旧版内容"
        );
    });
}

#[test]
fn simultaneous_history_snapshots_are_unique_and_newest_first() {
    fixture(|path| {
        for content in ["第一条", "第二条", "第三条"] {
            record_snapshot_at(
                path,
                content.as_bytes(),
                HistorySource::Manual,
                Default::default(),
                time(0),
            )
            .expect("同一时刻保存");
        }
        let versions = list_versions(path, Default::default(), time(0)).expect("历史");
        let contents: Vec<_> = versions
            .iter()
            .map(|version| std::fs::read_to_string(&version.path).expect("版本"))
            .collect();
        assert_eq!(
            contents,
            ["第三条", "第二条", "第一条"],
            "低精度时钟下也必须稳定地按保存顺序排列"
        );
    });
}

#[cfg(unix)]
#[test]
fn history_content_and_metadata_are_private_from_creation() {
    use std::os::unix::fs::PermissionsExt;
    fixture(|path| {
        record_snapshot_at(
            path,
            b"secret",
            HistorySource::Manual,
            Default::default(),
            time(0),
        )
        .expect("记录");
        let version = list_versions(path, Default::default(), time(0))
            .expect("历史")
            .remove(0)
            .path;
        for path in [version.clone(), version.with_extension("json")] {
            assert_eq!(
                std::fs::metadata(path).expect("权限").permissions().mode() & 0o777,
                0o600
            );
        }
    });
}

#[test]
fn failed_history_publish_does_not_expose_a_partial_snapshot() {
    fixture(|path| {
        let folder = super::directory(path).expect("目录");
        std::fs::create_dir_all(&folder).expect("历史目录");
        let stamp = super::nanos(time(0));
        let metadata = folder.join(format!("{stamp}.json"));
        std::fs::create_dir(&metadata).expect("模拟元数据写入失败");
        assert!(
            record_snapshot_at(
                path,
                b"incomplete",
                HistorySource::Manual,
                Default::default(),
                time(0)
            )
            .is_err()
        );
        let published = std::fs::read_dir(&folder)
            .expect("历史目录")
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "md")
            })
            .count();
        assert_eq!(published, 0, "写入失败不能暴露半成品历史");
        std::fs::remove_dir(metadata).expect("解除故障");
        assert!(
            list_versions(path, Default::default(), time(0))
                .expect("历史")
                .is_empty()
        );
    });
}
