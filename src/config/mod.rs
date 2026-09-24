//! Shared user-configuration helpers for app preferences and imported packs.
//! 基于 Velotype 修改：配置写入 maksher 独立目录。

use std::path::{Path, PathBuf};

use anyhow::{Context as _, bail};
use directories::ProjectDirs;
use serde_json::{Map, Value};

pub(crate) mod preferences;
mod recovery;

pub(crate) use preferences::{
    EditorSettings, ImagePasteBehavior, StartupOpenPreference, apply_configured_language,
    apply_configured_theme, first_existing_recent_markdown_file, import_language_config_and_select,
    import_theme_config_and_select, load_or_create_app_preferences, open_preferences_window,
    read_app_preferences,
};
pub(crate) use recovery::{
    RecoverySnapshot, read_recovery_snapshots, remove_recovery_snapshot, save_recovery_snapshot,
};

pub(crate) const RECENT_FILES_LIMIT: usize = 20;
pub(crate) const RECENT_WORKSPACES_LIMIT: usize = 10;

/// maksher 的跨平台配置目录。
#[derive(Debug, Clone)]
pub(crate) struct VelotypeConfigDirs {
    root: PathBuf,
}

impl VelotypeConfigDirs {
    /// Resolves the platform-specific app config directory.
    ///
    /// GPUI does not currently expose an app config path, so user-imported
    /// language and theme packs are stored under the OS location returned by
    /// `directories::ProjectDirs`.
    pub(crate) fn from_system() -> anyhow::Result<Self> {
        let dirs = ProjectDirs::from("app", "maksher", "maksher")
            .context("failed to resolve the maksher config directory")?;
        Ok(Self {
            root: dirs.config_dir().to_path_buf(),
        })
    }

    /// Creates a directory set from a caller-provided root for tests.
    #[cfg(test)]
    pub(crate) fn from_root(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub(crate) fn languages_dir(&self) -> PathBuf {
        self.root.join("languages")
    }

    pub(crate) fn themes_dir(&self) -> PathBuf {
        self.root.join("themes")
    }

    pub(crate) fn history_file(&self) -> PathBuf {
        self.root.join(".history")
    }

    pub(crate) fn recent_workspaces_file(&self) -> PathBuf {
        self.root.join("recent-workspaces.txt")
    }

    pub(crate) fn app_config_file(&self) -> PathBuf {
        self.root.join("config.toml")
    }

    pub(crate) fn recovery_dir(&self) -> PathBuf {
        self.root.join("recovery")
    }
}

pub(crate) fn read_recent_files() -> anyhow::Result<Vec<PathBuf>> {
    read_recent_files_with_dirs(&VelotypeConfigDirs::from_system()?)
}

pub(crate) fn record_recent_file(path: &Path) -> anyhow::Result<Vec<PathBuf>> {
    record_recent_file_with_dirs(path, &VelotypeConfigDirs::from_system()?)
}

pub(crate) fn remove_recent_file(path: &Path) -> anyhow::Result<Vec<PathBuf>> {
    remove_recent_file_with_dirs(path, &VelotypeConfigDirs::from_system()?)
}

pub(crate) fn read_recent_workspaces() -> anyhow::Result<Vec<PathBuf>> {
    read_recent_workspaces_with_dirs(&VelotypeConfigDirs::from_system()?)
}

pub(crate) fn record_recent_workspace(path: &Path) -> anyhow::Result<Vec<PathBuf>> {
    record_recent_workspace_with_dirs(path, &VelotypeConfigDirs::from_system()?)
}

fn read_recent_workspaces_with_dirs(dirs: &VelotypeConfigDirs) -> anyhow::Result<Vec<PathBuf>> {
    let path = dirs.recent_workspaces_file();
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => {
            return Err(err).with_context(|| format!("failed to read '{}'", path.display()));
        }
    };
    Ok(normalize_recent_workspaces(text.lines().map(PathBuf::from)))
}

fn record_recent_workspace_with_dirs(
    path: &Path,
    dirs: &VelotypeConfigDirs,
) -> anyhow::Result<Vec<PathBuf>> {
    if !path.is_dir() {
        bail!("workspace path is not a directory: '{}'", path.display());
    }
    let mut paths = read_recent_workspaces_with_dirs(dirs)?;
    paths.retain(|existing| !same_recent_path(existing, path));
    paths.insert(0, path.to_path_buf());
    paths.truncate(RECENT_WORKSPACES_LIMIT);
    let file = dirs.recent_workspaces_file();
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create '{}'", parent.display()))?;
    }
    let content = paths
        .iter()
        .map(|path| path.to_string_lossy())
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&file, content + "\n")
        .with_context(|| format!("failed to write '{}'", file.display()))?;
    Ok(paths)
}

fn normalize_recent_workspaces(paths: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
    let mut normalized: Vec<PathBuf> = Vec::new();
    for path in paths {
        let text = path.to_string_lossy();
        let trimmed = text.trim();
        if trimmed.is_empty() {
            continue;
        }
        let path = PathBuf::from(trimmed);
        if !path.is_dir()
            || normalized
                .iter()
                .any(|existing| same_recent_path(existing, &path))
        {
            continue;
        }
        normalized.push(path);
        if normalized.len() == RECENT_WORKSPACES_LIMIT {
            break;
        }
    }
    normalized
}

pub(crate) fn read_recent_files_with_dirs(
    dirs: &VelotypeConfigDirs,
) -> anyhow::Result<Vec<PathBuf>> {
    let path = dirs.history_file();
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => {
            return Err(err).with_context(|| format!("failed to read '{}'", path.display()));
        }
    };

    Ok(normalize_recent_files(text.lines().map(PathBuf::from)))
}

pub(crate) fn record_recent_file_with_dirs(
    path: &Path,
    dirs: &VelotypeConfigDirs,
) -> anyhow::Result<Vec<PathBuf>> {
    if path.to_string_lossy().trim().is_empty() {
        bail!("recent file path cannot be empty");
    }
    if !is_recordable_recent_file_path(path) {
        return read_recent_files_with_dirs(dirs);
    }

    let mut paths = read_recent_files_with_dirs(dirs)?;
    let path = path.to_path_buf();
    paths.retain(|existing| !same_recent_path(existing, &path));
    paths.insert(0, path);
    paths.truncate(RECENT_FILES_LIMIT);
    write_recent_files_with_dirs(&paths, dirs)?;
    Ok(paths)
}

pub(crate) fn remove_recent_file_with_dirs(
    path: &Path,
    dirs: &VelotypeConfigDirs,
) -> anyhow::Result<Vec<PathBuf>> {
    let mut paths = read_recent_files_with_dirs(dirs)?;
    paths.retain(|existing| !same_recent_path(existing, path));
    write_recent_files_with_dirs(&paths, dirs)?;
    Ok(paths)
}

fn write_recent_files_with_dirs(
    paths: &[PathBuf],
    dirs: &VelotypeConfigDirs,
) -> anyhow::Result<()> {
    let history_file = dirs.history_file();
    let normalized = normalize_recent_files(paths.iter().cloned());
    if normalized.is_empty() {
        match std::fs::remove_file(&history_file) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                return Err(err)
                    .with_context(|| format!("failed to remove '{}'", history_file.display()));
            }
        }
        return Ok(());
    }

    if let Some(parent) = history_file.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create '{}'", parent.display()))?;
    }
    let mut content = String::new();
    for path in normalized {
        content.push_str(&path.to_string_lossy());
        content.push('\n');
    }
    std::fs::write(&history_file, content)
        .with_context(|| format!("failed to write '{}'", history_file.display()))
}

fn normalize_recent_files(paths: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
    let mut normalized: Vec<PathBuf> = Vec::new();
    for path in paths {
        let text = path.to_string_lossy();
        let trimmed = text.trim();
        if trimmed.is_empty() {
            continue;
        }
        let path = PathBuf::from(trimmed);
        if !is_recordable_recent_file_path(&path) {
            continue;
        }
        if normalized
            .iter()
            .any(|existing| same_recent_path(existing, &path))
        {
            continue;
        }
        normalized.push(path);
        if normalized.len() == RECENT_FILES_LIMIT {
            break;
        }
    }
    normalized
}

fn is_recordable_recent_file_path(path: &Path) -> bool {
    let text = path.to_string_lossy();
    if text.trim().is_empty() {
        return false;
    }

    !(is_inside_system_temp_dir(path) && has_velotype_temp_fixture_name(path))
}

fn is_inside_system_temp_dir(path: &Path) -> bool {
    let temp_dir = std::env::temp_dir();
    if cfg!(windows) {
        let path_text = normalize_windows_path_text(path);
        let mut temp_text = normalize_windows_path_text(&temp_dir);
        if !temp_text.ends_with('\\') {
            temp_text.push('\\');
        }
        return path_text.starts_with(&temp_text);
    }

    path.starts_with(temp_dir)
}

fn normalize_windows_path_text(path: &Path) -> String {
    path.to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_ascii_lowercase()
}

fn has_velotype_temp_fixture_name(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| {
            let name = name.to_ascii_lowercase();
            name.starts_with("velotype-drop-") || name.starts_with("velotypre-drop-")
        })
        .unwrap_or(false)
}

fn same_recent_path(left: &Path, right: &Path) -> bool {
    if cfg!(windows) {
        left.to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy())
    } else {
        left == right
    }
}

pub(crate) fn is_supported_config_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| {
            extension.eq_ignore_ascii_case("json") || extension.eq_ignore_ascii_case("jsonc")
        })
        .unwrap_or(false)
}

pub(crate) fn read_json_or_jsonc(path: &Path) -> anyhow::Result<Value> {
    if !is_supported_config_file(path) {
        bail!("configuration files must use the .json or .jsonc extension");
    }

    let text = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read '{}'", path.display()))?;
    let parsed = if path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.eq_ignore_ascii_case("jsonc"))
        .unwrap_or(false)
    {
        parse_jsonc_value(&text)?
    } else {
        serde_json::from_str(&text)?
    };
    Ok(parsed)
}

pub(crate) fn parse_jsonc_value(text: &str) -> anyhow::Result<Value> {
    let stripped = strip_jsonc_comments(text)?;
    Ok(serde_json::from_str(&stripped)?)
}

pub(crate) fn strip_jsonc_comments(input: &str) -> anyhow::Result<String> {
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    let mut in_string = false;
    let mut escaped = false;

    while let Some(ch) = chars.next() {
        if in_string {
            output.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        if ch == '"' {
            in_string = true;
            output.push(ch);
            continue;
        }

        if ch == '/' {
            match chars.peek().copied() {
                Some('/') => {
                    chars.next();
                    for next in chars.by_ref() {
                        if next == '\n' {
                            output.push('\n');
                            break;
                        }
                    }
                    continue;
                }
                Some('*') => {
                    chars.next();
                    let mut closed = false;
                    let mut previous = '\0';
                    for next in chars.by_ref() {
                        if next == '\n' {
                            output.push('\n');
                        }
                        if previous == '*' && next == '/' {
                            closed = true;
                            break;
                        }
                        previous = next;
                    }
                    if !closed {
                        bail!("unterminated block comment in JSONC file");
                    }
                    continue;
                }
                _ => {}
            }
        }

        output.push(ch);
    }

    Ok(output)
}

pub(crate) fn sanitize_config_file_stem(value: &str) -> String {
    let mut output = String::new();
    let mut last_was_separator = false;
    for ch in value.trim().chars() {
        if ch.is_whitespace() {
            if !last_was_separator && !output.is_empty() {
                output.push('_');
                last_was_separator = true;
            }
        } else if ch.is_alphanumeric() || matches!(ch, '-' | '_' | '.') {
            output.push(ch);
            last_was_separator = false;
        }
    }

    let output = output.trim_matches(['_', '.']).to_string();
    if output.is_empty() {
        "custom".into()
    } else {
        output
    }
}

pub(crate) fn prune_empty_json_values(value: &mut Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(text) => text.trim().is_empty(),
        Value::Array(items) => {
            items.retain_mut(|item| !prune_empty_json_values(item));
            items.is_empty()
        }
        Value::Object(object) => {
            object.retain(|_, item| !prune_empty_json_values(item));
            object.is_empty()
        }
        Value::Bool(_) | Value::Number(_) => false,
    }
}

pub(crate) fn merge_non_empty_json_values(base: &mut Value, patch: &Value) {
    if is_empty_json_value(patch) {
        return;
    }

    match (base, patch) {
        (Value::Object(base_object), Value::Object(patch_object)) => {
            for (key, patch_value) in patch_object {
                if is_empty_json_value(patch_value) {
                    continue;
                }
                match base_object.get_mut(key) {
                    Some(base_value) => merge_non_empty_json_values(base_value, patch_value),
                    None => {
                        base_object.insert(key.clone(), patch_value.clone());
                    }
                }
            }
        }
        (base_value, patch_value) => {
            *base_value = patch_value.clone();
        }
    }
}

pub(crate) fn object_without_empty_values(mut object: Map<String, Value>) -> Map<String, Value> {
    object.retain(|_, value| !prune_empty_json_values(value));
    object
}

fn is_empty_json_value(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(text) => text.trim().is_empty(),
        Value::Array(items) => items.iter().all(is_empty_json_value),
        Value::Object(object) => object.values().all(is_empty_json_value),
        Value::Bool(_) | Value::Number(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        RECENT_FILES_LIMIT, RECENT_WORKSPACES_LIMIT, VelotypeConfigDirs, parse_jsonc_value,
        prune_empty_json_values, read_recent_files_with_dirs, read_recent_workspaces_with_dirs,
        record_recent_file_with_dirs, record_recent_workspace_with_dirs,
        remove_recent_file_with_dirs, sanitize_config_file_stem, strip_jsonc_comments,
    };
    use serde_json::json;
    use std::path::{Path, PathBuf};

    #[test]
    fn jsonc_comments_are_stripped_without_touching_strings() {
        let text = r#"
        {
            // line comment
            "url": "https://example.com/a//b",
            "text": "/* not a comment */",
            /* block comment */
            "value": 1
        }
        "#;

        let parsed = parse_jsonc_value(text).expect("jsonc should parse");
        assert_eq!(parsed["url"], "https://example.com/a//b");
        assert_eq!(parsed["text"], "/* not a comment */");
        assert_eq!(parsed["value"], 1);
        assert!(strip_jsonc_comments(text).is_ok());
    }

    #[test]
    fn recent_workspaces_are_deduplicated_and_most_recent_first() {
        let root = std::env::temp_dir().join(format!(
            "maksher-recent-workspaces-{}",
            uuid::Uuid::new_v4()
        ));
        let first = root.join("first");
        let second = root.join("second");
        std::fs::create_dir_all(&first).expect("create first workspace");
        std::fs::create_dir_all(&second).expect("create second workspace");
        let dirs = VelotypeConfigDirs::from_root(root.join("config"));

        assert_eq!(
            record_recent_workspace_with_dirs(&first, &dirs).expect("record first"),
            vec![first.clone()]
        );
        assert_eq!(
            record_recent_workspace_with_dirs(&second, &dirs).expect("record second"),
            vec![second.clone(), first.clone()]
        );
        assert_eq!(
            record_recent_workspace_with_dirs(&first, &dirs).expect("move first to front"),
            vec![first.clone(), second.clone()]
        );
        assert_eq!(
            read_recent_workspaces_with_dirs(&dirs).expect("read recent"),
            vec![first, second]
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn recent_workspaces_respect_the_limit() {
        let root = std::env::temp_dir().join(format!(
            "maksher-recent-workspaces-limit-{}",
            uuid::Uuid::new_v4()
        ));
        let dirs = VelotypeConfigDirs::from_root(root.join("config"));
        let mut expected = Vec::new();

        for index in 0..RECENT_WORKSPACES_LIMIT + 1 {
            let path = root.join(format!("workspace-{index}"));
            std::fs::create_dir_all(&path).expect("create workspace");
            let recent = record_recent_workspace_with_dirs(&path, &dirs).expect("record workspace");
            expected.insert(0, path);
            expected.truncate(RECENT_WORKSPACES_LIMIT);
            assert_eq!(recent, expected);
        }

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn empty_values_are_pruned_recursively() {
        let mut value = json!({
            "name": "",
            "colors": {
                "text_default": null,
                "selection": "#fff"
            },
            "items": ["", null]
        });

        assert!(!prune_empty_json_values(&mut value));
        assert_eq!(value, json!({ "colors": { "selection": "#fff" } }));
    }

    #[test]
    fn config_file_stems_are_sanitized() {
        assert_eq!(
            sanitize_config_file_stem("My Theme / Blue"),
            "My_Theme_Blue"
        );
        assert_eq!(sanitize_config_file_stem("  ...  "), "custom");
    }

    #[test]
    fn missing_recent_history_file_returns_empty_list() {
        let root = std::env::temp_dir().join(format!("velotype-config-{}", uuid::Uuid::new_v4()));
        let dirs = VelotypeConfigDirs::from_root(&root);

        assert!(read_recent_files_with_dirs(&dirs).unwrap().is_empty());
        assert!(!dirs.history_file().exists());

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn empty_recent_history_write_does_not_create_file() {
        let root = std::env::temp_dir().join(format!("velotype-config-{}", uuid::Uuid::new_v4()));
        let dirs = VelotypeConfigDirs::from_root(&root);

        super::write_recent_files_with_dirs(&[], &dirs).unwrap();

        assert!(!dirs.history_file().exists());

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn blank_recent_file_path_is_rejected() {
        let root = std::env::temp_dir().join(format!("velotype-config-{}", uuid::Uuid::new_v4()));
        let dirs = VelotypeConfigDirs::from_root(&root);

        assert!(record_recent_file_with_dirs(Path::new("   "), &dirs).is_err());
        assert!(!dirs.history_file().exists());

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn recent_history_filters_empty_lines_and_deduplicates() {
        let root = std::env::temp_dir().join(format!("velotype-config-{}", uuid::Uuid::new_v4()));
        let dirs = VelotypeConfigDirs::from_root(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            dirs.history_file(),
            "  \nC:\\one.md\n\nC:\\two.md\nC:\\one.md\n",
        )
        .unwrap();

        let paths = read_recent_files_with_dirs(&dirs).unwrap();
        assert_eq!(
            paths,
            vec![PathBuf::from("C:\\one.md"), PathBuf::from("C:\\two.md")]
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn recent_history_filters_legacy_velotype_temp_fixture_paths() {
        let root = std::env::temp_dir().join(format!("velotype-config-{}", uuid::Uuid::new_v4()));
        let dirs = VelotypeConfigDirs::from_root(&root);
        let fixture_path = std::env::temp_dir().join(format!(
            "velotype-drop-save-replace-{}-123.md",
            std::process::id()
        ));
        let real_path = PathBuf::from("C:\\notes\\real.md");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            dirs.history_file(),
            format!("{}\n{}\n", fixture_path.display(), real_path.display()),
        )
        .unwrap();

        let paths = read_recent_files_with_dirs(&dirs).unwrap();
        assert_eq!(paths, vec![real_path]);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn recording_velotype_temp_fixture_path_is_noop() {
        let root = std::env::temp_dir().join(format!("velotype-config-{}", uuid::Uuid::new_v4()));
        let dirs = VelotypeConfigDirs::from_root(&root);
        let fixture_path = std::env::temp_dir().join(format!(
            "velotype-drop-dirty-discard-{}-123.md",
            std::process::id()
        ));

        assert!(
            record_recent_file_with_dirs(&fixture_path, &dirs)
                .unwrap()
                .is_empty()
        );
        assert!(!dirs.history_file().exists());

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn ordinary_temp_markdown_file_can_still_be_recorded() {
        let root = std::env::temp_dir().join(format!("velotype-config-{}", uuid::Uuid::new_v4()));
        let dirs = VelotypeConfigDirs::from_root(&root);
        let path = std::env::temp_dir().join(format!("manual-note-{}.md", std::process::id()));

        let paths = record_recent_file_with_dirs(&path, &dirs).unwrap();

        assert_eq!(paths, vec![path]);
        assert!(dirs.history_file().exists());

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn recording_recent_file_moves_it_to_front_and_truncates() {
        let root = std::env::temp_dir().join(format!("velotype-config-{}", uuid::Uuid::new_v4()));
        let dirs = VelotypeConfigDirs::from_root(&root);

        for index in 0..(RECENT_FILES_LIMIT + 2) {
            record_recent_file_with_dirs(&PathBuf::from(format!("file-{index}.md")), &dirs)
                .unwrap();
        }
        record_recent_file_with_dirs(&PathBuf::from("file-3.md"), &dirs).unwrap();

        let paths = read_recent_files_with_dirs(&dirs).unwrap();
        assert_eq!(paths.len(), RECENT_FILES_LIMIT);
        assert_eq!(paths[0], PathBuf::from("file-3.md"));
        assert_eq!(
            paths
                .iter()
                .filter(|path| path.as_path() == Path::new("file-3.md"))
                .count(),
            1
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn removing_recent_file_persists_history_without_it() {
        let root = std::env::temp_dir().join(format!("velotype-config-{}", uuid::Uuid::new_v4()));
        let dirs = VelotypeConfigDirs::from_root(&root);
        record_recent_file_with_dirs(&PathBuf::from("one.md"), &dirs).unwrap();
        record_recent_file_with_dirs(&PathBuf::from("two.md"), &dirs).unwrap();

        let paths = remove_recent_file_with_dirs(&PathBuf::from("one.md"), &dirs).unwrap();

        assert_eq!(paths, vec![PathBuf::from("two.md")]);
        assert_eq!(
            read_recent_files_with_dirs(&dirs).unwrap(),
            vec![PathBuf::from("two.md")]
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn removing_last_recent_file_deletes_history_file() {
        let root = std::env::temp_dir().join(format!("velotype-config-{}", uuid::Uuid::new_v4()));
        let dirs = VelotypeConfigDirs::from_root(&root);
        let path = PathBuf::from("only.md");
        record_recent_file_with_dirs(&path, &dirs).unwrap();
        assert!(dirs.history_file().exists());

        let paths = remove_recent_file_with_dirs(&path, &dirs).unwrap();

        assert!(paths.is_empty());
        assert!(!dirs.history_file().exists());
        assert!(read_recent_files_with_dirs(&dirs).unwrap().is_empty());

        let _ = std::fs::remove_dir_all(root);
    }
}
