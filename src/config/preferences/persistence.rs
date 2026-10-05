use super::*;

#[derive(Serialize)]
pub(crate) struct PreferencesFile {
    preferences_version: i64,
    startup: StartupPreferencesFile,
    language: LanguagePreferencesFile,
    theme: ThemePreferencesFile,
    export: ExportPreferencesFile,
    editor: EditorPreferencesFile,
    status_bar: StatusBarPreferencesFile,
    window: WindowPreferencesFile,
    keybindings: BTreeMap<String, Vec<String>>,
}

#[derive(Serialize)]
struct StartupPreferencesFile {
    open: String,
}

#[derive(Serialize)]
struct EditorPreferencesFile {
    show_table_headers: bool,
    smart_punctuation: bool,
    external_change_policy: String,
    delete_policy: String,
    image_paste_behavior: String,
    markdown_font_family: String,
    markdown_font_size: u16,
    code_font_family: String,
    code_font_size: u16,
    writing_width: String,
    workspace_sidebar_width: u16,
    autosave_debounce_ms: u64,
    autosave: bool,
    tree_sort: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    new_file_template: String,
}

#[derive(Serialize)]
struct LanguagePreferencesFile {
    default_language_id: String,
}

#[derive(Serialize)]
struct ThemePreferencesFile {
    default_theme_id: String,
}

#[derive(Serialize)]
struct ExportPreferencesFile {
    theme: String,
}

#[derive(Serialize, Deserialize)]
struct WindowFrameFile {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

impl From<WindowFrame> for WindowFrameFile {
    fn from(value: WindowFrame) -> Self {
        Self {
            x: value.x,
            y: value.y,
            width: value.width,
            height: value.height,
        }
    }
}

impl From<WindowFrameFile> for WindowFrame {
    fn from(value: WindowFrameFile) -> Self {
        Self {
            x: value.x,
            y: value.y,
            width: value.width,
            height: value.height,
        }
    }
}

#[derive(Serialize)]
struct WindowPreferencesFile {
    remember_bounds: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    frame: Option<WindowFrameFile>,
    open_position: String,
    zoom_percent: i64,
    default_window_width: i64,
    default_window_height: i64,
}

#[derive(Serialize)]
struct StatusBarPreferencesFile {
    enabled: bool,
    show_word_count: bool,
    show_cursor_position: bool,
    show_sidebar_toggle: bool,
    show_mode_switch: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    custom_buttons: Vec<StatusBarButton>,
}

impl From<&StatusBarPreferences> for StatusBarPreferencesFile {
    fn from(value: &StatusBarPreferences) -> Self {
        Self {
            enabled: value.enabled,
            show_word_count: value.show_word_count,
            show_cursor_position: value.show_cursor_position,
            show_sidebar_toggle: value.show_sidebar_toggle,
            show_mode_switch: value.show_mode_switch,
            custom_buttons: value.custom_buttons.clone(),
        }
    }
}

impl From<&AppPreferences> for PreferencesFile {
    fn from(value: &AppPreferences) -> Self {
        Self {
            preferences_version: PREFERENCES_VERSION,
            startup: StartupPreferencesFile {
                open: value.startup_open.as_str().into(),
            },
            language: LanguagePreferencesFile {
                default_language_id: value.default_language_id.clone(),
            },
            theme: ThemePreferencesFile {
                default_theme_id: value.default_theme_id.clone(),
            },
            export: ExportPreferencesFile {
                theme: value.export_theme.as_str().into(),
            },
            editor: EditorPreferencesFile {
                show_table_headers: value.show_table_headers,
                smart_punctuation: value.smart_punctuation,
                external_change_policy: value.external_change_policy.as_str().into(),
                delete_policy: value.delete_policy.as_str().into(),
                image_paste_behavior: value.image_paste_behavior.as_str().into(),
                markdown_font_family: value.fonts.markdown_family.clone(),
                markdown_font_size: value.fonts.markdown_size,
                code_font_family: value.fonts.code_family.clone(),
                code_font_size: value.fonts.code_size,
                writing_width: value.writing_width.as_str().into(),
                workspace_sidebar_width: value.workspace_sidebar_width,
                autosave_debounce_ms: value.autosave_debounce_ms,
                autosave: value.autosave,
                tree_sort: value.tree_sort.as_str().into(),
                new_file_template: value.new_file_template.clone(),
            },
            status_bar: StatusBarPreferencesFile::from(&value.status_bar),
            window: WindowPreferencesFile {
                remember_bounds: value.remember_window_bounds,
                frame: value.window_frame.map(WindowFrameFile::from),
                open_position: value.window_open_position.as_str().into(),
                zoom_percent: value.zoom_percent,
                default_window_width: value.default_window_width,
                default_window_height: value.default_window_height,
            },
            keybindings: normalize_shortcut_config(&value.keybindings),
        }
    }
}

/// The persisted window frame, if window-bounds remembering is enabled and a
/// usable frame was stored.
pub(crate) fn saved_window_frame() -> anyhow::Result<Option<WindowFrame>> {
    let preferences = read_app_preferences()?;
    Ok(preferences
        .remember_window_bounds
        .then_some(())
        .and_then(|()| preferences.window_frame))
}

/// Persists the window frame for the next launch (no-op when remembering is
/// disabled in preferences).
pub(crate) fn store_window_frame(frame: WindowFrame) -> anyhow::Result<()> {
    update_app_preferences(|preferences| {
        if preferences.remember_window_bounds {
            preferences.window_frame = Some(frame);
        }
    })?;
    Ok(())
}

pub(crate) fn read_app_preferences() -> anyhow::Result<AppPreferences> {
    read_app_preferences_with_dirs(&VeloraConfigDirs::from_system()?)
}

pub(crate) fn read_app_preferences_with_dirs(
    dirs: &VeloraConfigDirs,
) -> anyhow::Result<AppPreferences> {
    let path = dirs.app_config_file();
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(AppPreferences::default());
        }
        Err(err) => {
            return Err(err).with_context(|| format!("failed to read '{}'", path.display()));
        }
    };
    let Ok(value) = toml::from_str::<toml::Value>(&text) else {
        return Ok(AppPreferences::default());
    };

    let (preferences, needs_migration) =
        load_preferences_from_toml_value(&value, DEFAULT_LANGUAGE_ID);
    if needs_migration {
        save_app_preferences_with_dirs(&preferences, dirs)?;
    }
    Ok(preferences)
}

pub(crate) fn load_preferences_from_toml_value(
    value: &toml::Value,
    fallback_language_id: &str,
) -> (AppPreferences, bool) {
    let mut preferences = app_preferences_from_toml_value(value, fallback_language_id);
    let version = value
        .get("preferences_version")
        .and_then(toml::Value::as_integer)
        .unwrap_or_default();
    if version >= PREFERENCES_VERSION {
        return (preferences, false);
    }

    if version < 3 && preferences.default_theme_id == "system" {
        // 「system」（跟随系统明暗）是 v2 之前的默认值，多数人没主动选过。默认主题改成
        // Forest 后把它一起带过去，否则老配置看不到任何变化；想跟随系统的可以再选回来。
        preferences.default_theme_id = DEFAULT_THEME_ID.into();
    }

    if version < 1 {
        // 早期配置里的主题标识可能已不存在：认不出来就回到默认主题。
        if let Some(stored) = value
            .get("theme")
            .and_then(|theme| theme.get("default_theme_id"))
            .and_then(toml::Value::as_str)
            && !matches!(stored, "system" | "velora-dark" | "velora-light")
        {
            preferences.default_theme_id = DEFAULT_THEME_ID.into();
        }
        if value
            .get("editor")
            .and_then(|editor| editor.get("image_paste_behavior"))
            .and_then(toml::Value::as_str)
            == Some("none")
        {
            preferences.image_paste_behavior = ImagePasteBehavior::CopyToAssetsFolder;
        }
    }
    if version < 2 && preferences.fonts.markdown_family == ".SystemUIFont" {
        preferences.fonts.markdown_family = "theme".into();
    }
    (preferences, true)
}

pub(crate) fn load_or_create_app_preferences() -> anyhow::Result<AppPreferences> {
    let dirs = VeloraConfigDirs::from_system()?;
    load_or_create_app_preferences_with_dirs_and_locales(&dirs, sys_locale::get_locales())
}

pub(crate) fn app_preferences_from_toml_value(
    value: &toml::Value,
    fallback_language_id: &str,
) -> AppPreferences {
    let startup_open = value
        .get("startup")
        .and_then(|startup| startup.get("open"))
        .and_then(|open| open.as_str())
        .map(StartupOpenPreference::from_str)
        .unwrap_or(StartupOpenPreference::NewFile);
    let default_language_id = value
        .get("language")
        .and_then(|language| language.get("default_language_id"))
        .and_then(|id| id.as_str())
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .unwrap_or(fallback_language_id)
        .to_string();
    let default_theme_id = value
        .get("theme")
        .and_then(|theme| theme.get("default_theme_id"))
        .and_then(|id| id.as_str())
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .unwrap_or(DEFAULT_THEME_ID)
        .to_string();
    let export_theme = value
        .get("export")
        .and_then(|export| export.get("theme"))
        .and_then(toml::Value::as_str)
        .map(ExportThemePreference::from_str)
        .unwrap_or_default();
    let keybindings = value
        .get("keybindings")
        .and_then(|keybindings| keybindings.as_table())
        .map(|table| {
            table
                .iter()
                .filter_map(|(key, value)| {
                    let keys = value
                        .as_array()?
                        .iter()
                        .filter_map(|value| value.as_str().map(str::to_string))
                        .collect::<Vec<_>>();
                    Some((key.clone(), keys))
                })
                .collect::<BTreeMap<_, _>>()
        })
        .map(|keybindings| normalize_shortcut_config(&keybindings))
        .unwrap_or_default();

    let show_table_headers = value
        .get("editor")
        .and_then(|editor| editor.get("show_table_headers"))
        .and_then(|value| value.as_bool())
        .unwrap_or(true);
    let smart_punctuation = value
        .get("editor")
        .and_then(|editor| editor.get("smart_punctuation"))
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let external_change_policy = value
        .get("editor")
        .and_then(|editor| editor.get("external_change_policy"))
        .and_then(|value| value.as_str())
        .map(ExternalChangePolicy::from_str)
        .unwrap_or_default();
    let delete_policy = value
        .get("editor")
        .and_then(|editor| editor.get("delete_policy"))
        .and_then(|value| value.as_str())
        .map(DeletePolicy::from_str)
        .unwrap_or_default();
    let image_paste_behavior = value
        .get("editor")
        .and_then(|editor| editor.get("image_paste_behavior"))
        .and_then(|value| value.as_str())
        .map(ImagePasteBehavior::from_str)
        .unwrap_or(ImagePasteBehavior::CopyToAssetsFolder);
    let font_defaults = FontPreferences::default();
    let editor = value.get("editor");
    let font_family = |key: &str, default: &str| {
        editor
            .and_then(|editor| editor.get(key))
            .and_then(toml::Value::as_str)
            .map(str::trim)
            .filter(|family| !family.is_empty())
            .unwrap_or(default)
            .to_string()
    };
    let font_size = |key: &str, default: u16| {
        editor
            .and_then(|editor| editor.get(key))
            .and_then(toml::Value::as_integer)
            .and_then(|size| u16::try_from(size).ok())
            .filter(|size| (10..=36).contains(size))
            .unwrap_or(default)
    };
    let fonts = FontPreferences {
        markdown_family: font_family("markdown_font_family", &font_defaults.markdown_family),
        markdown_size: font_size("markdown_font_size", font_defaults.markdown_size),
        code_family: font_family("code_font_family", &font_defaults.code_family),
        code_size: font_size("code_font_size", font_defaults.code_size),
    };
    let writing_width = editor
        .and_then(|editor| editor.get("writing_width"))
        .and_then(toml::Value::as_str)
        .map(WritingWidthPreference::from_str)
        .unwrap_or_default();
    let workspace_sidebar_width = editor
        .and_then(|editor| editor.get("workspace_sidebar_width"))
        .and_then(toml::Value::as_integer)
        .and_then(|width| u16::try_from(width).ok())
        .filter(|width| (180..=600).contains(width))
        .unwrap_or(258);
    let tree_sort = editor
        .and_then(|editor| editor.get("tree_sort"))
        .and_then(toml::Value::as_str)
        .map(TreeSortPreference::from_str)
        .unwrap_or_default();
    let new_file_template = editor
        .and_then(|editor| editor.get("new_file_template"))
        .and_then(toml::Value::as_str)
        .unwrap_or("")
        .to_string();
    let autosave_debounce_ms = editor
        .and_then(|editor| editor.get("autosave_debounce_ms"))
        .and_then(toml::Value::as_integer)
        .and_then(|ms| u64::try_from(ms).ok())
        .filter(|ms| (100..=30_000).contains(ms))
        .unwrap_or(800);
    let autosave = editor
        .and_then(|editor| editor.get("autosave"))
        .and_then(toml::Value::as_bool)
        .unwrap_or(true);

    let status_bar = value
        .get("status_bar")
        .map(|sb| {
            let enabled = sb.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true);
            let show_word_count = sb
                .get("show_word_count")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let show_cursor_position = sb
                .get("show_cursor_position")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let show_sidebar_toggle = sb
                .get("show_sidebar_toggle")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let show_mode_switch = sb
                .get("show_mode_switch")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let custom_buttons = sb
                .get("custom_buttons")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|item| {
                            let id = item.get("id")?.as_str()?.to_string();
                            let label = item.get("label")?.as_str()?.to_string();
                            Some(StatusBarButton {
                                id,
                                label,
                                action_id: item
                                    .get("action_id")
                                    .and_then(|a| a.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            StatusBarPreferences {
                enabled,
                show_word_count,
                show_cursor_position,
                show_sidebar_toggle,
                show_mode_switch,
                custom_buttons,
            }
        })
        .unwrap_or_default();

    let window = value.get("window");
    let remember_window_bounds = window
        .and_then(|window| window.get("remember_bounds"))
        .and_then(toml::Value::as_bool)
        .unwrap_or(true);
    let window_open_position = window
        .and_then(|window| window.get("open_position"))
        .and_then(toml::Value::as_str)
        .map(WindowOpenPosition::from_str)
        .unwrap_or_default();
    let zoom_percent = window
        .and_then(|window| window.get("zoom_percent"))
        .and_then(toml::Value::as_integer)
        .filter(|percent| (60..=200).contains(percent))
        .unwrap_or(100);
    let default_window_width = window
        .and_then(|window| window.get("default_window_width"))
        .and_then(toml::Value::as_integer)
        .filter(|width| (480..=8_000).contains(width))
        .unwrap_or(1080);
    let default_window_height = window
        .and_then(|window| window.get("default_window_height"))
        .and_then(toml::Value::as_integer)
        .filter(|height| (320..=4_000).contains(height))
        .unwrap_or(720);
    let window_frame = window
        .and_then(|window| window.get("frame"))
        .and_then(|frame| {
            let x = frame.get("x").and_then(toml::Value::as_integer)?;
            let y = frame.get("y").and_then(toml::Value::as_integer)?;
            let width = frame.get("width").and_then(toml::Value::as_integer)?;
            let height = frame.get("height").and_then(toml::Value::as_integer)?;
            Some(WindowFrame {
                x: i32::try_from(x).ok()?,
                y: i32::try_from(y).ok()?,
                width: i32::try_from(width).ok()?,
                height: i32::try_from(height).ok()?,
            })
        })
        .filter(|frame| frame.width > 200 && frame.height > 200);

    AppPreferences {
        startup_open,
        default_language_id,
        default_theme_id,
        export_theme,
        show_table_headers,
        smart_punctuation,
        external_change_policy,
        delete_policy,
        image_paste_behavior,
        fonts,
        writing_width,
        workspace_sidebar_width,
        autosave_debounce_ms,
        autosave,
        tree_sort,
        new_file_template,
        keybindings,
        status_bar,
        remember_window_bounds,
        window_frame,
        window_open_position,
        zoom_percent,
        default_window_width,
        default_window_height,
    }
}

pub(crate) fn detected_language_id_from_locales<I, S>(locales: I) -> &'static str
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    language_id_for_locale_preferences(locales)
}

pub(crate) fn load_or_create_app_preferences_with_dirs_and_locales<I, S>(
    dirs: &VeloraConfigDirs,
    locales: I,
) -> anyhow::Result<AppPreferences>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let detected_language_id = detected_language_id_from_locales(locales);
    let path = dirs.app_config_file();
    let preferences = match std::fs::read_to_string(&path) {
        Ok(text) => toml::from_str::<toml::Value>(&text)
            .map(|value| load_preferences_from_toml_value(&value, detected_language_id).0)
            .unwrap_or_else(|_| AppPreferences {
                default_language_id: detected_language_id.into(),
                ..AppPreferences::default()
            }),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => AppPreferences {
            default_language_id: detected_language_id.into(),
            ..AppPreferences::default()
        },
        Err(err) => {
            return Err(err).with_context(|| format!("failed to read '{}'", path.display()));
        }
    };
    save_app_preferences_with_dirs(&preferences, dirs)?;
    Ok(preferences)
}

pub(crate) fn save_app_preferences(preferences: &AppPreferences) -> anyhow::Result<()> {
    save_app_preferences_with_dirs(preferences, &VeloraConfigDirs::from_system()?)
}

pub(crate) fn save_app_preferences_with_dirs(
    preferences: &AppPreferences,
    dirs: &VeloraConfigDirs,
) -> anyhow::Result<()> {
    let path = dirs.app_config_file();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create '{}'", parent.display()))?;
    }
    let text = toml::to_string_pretty(&PreferencesFile::from(preferences))?;
    crate::config::write_config_file_atomic(&path, &text)
        .with_context(|| format!("failed to write '{}'", path.display()))
}

/// 导出主题偏好（roadmap F3）；读取失败时按「当前主题」导出。
pub(crate) fn export_theme_preference() -> ExportThemePreference {
    read_app_preferences()
        .map(|preferences| preferences.export_theme)
        .unwrap_or_default()
}

pub(crate) fn first_existing_recent_markdown_file() -> Option<PathBuf> {
    let recent_files = read_recent_files().ok()?;
    recent_files.into_iter().find(|path| path.is_file())
}

pub(crate) fn apply_configured_language(cx: &mut App, language_id: &str) -> anyhow::Result<bool> {
    let mut applied = false;
    let changed = cx.update_global::<I18nManager, _>(|i18n_manager, _cx| {
        let changed = i18n_manager.set_language_by_id(language_id);
        applied = changed || i18n_manager.current_language_id() == language_id;
        changed
    });
    if !applied {
        return Ok(false);
    }
    update_app_preferences(|preferences| {
        preferences.default_language_id = language_id.into();
    })?;
    Ok(changed)
}

pub(crate) fn apply_configured_theme(cx: &mut App, theme_id: &str) -> anyhow::Result<bool> {
    let mut applied = false;
    let changed = cx.update_global::<ThemeManager, _>(|theme_manager, _cx| {
        let changed = theme_manager.set_theme_by_id(theme_id);
        applied = changed || theme_manager.current_theme_id() == theme_id;
        changed
    });
    if !applied {
        return Ok(false);
    }
    update_app_preferences(|preferences| {
        preferences.default_theme_id = theme_id.into();
    })?;
    Ok(changed)
}

pub(crate) fn import_language_config_and_select(
    cx: &mut App,
    path: impl AsRef<std::path::Path>,
) -> anyhow::Result<String> {
    let imported_id = cx.update_global::<I18nManager, _>(|i18n_manager, _cx| {
        i18n_manager.import_language_config(path)
    })?;
    update_app_preferences(|preferences| {
        preferences.default_language_id = imported_id.clone();
    })?;
    Ok(imported_id)
}

pub(crate) fn import_theme_config_and_select(
    cx: &mut App,
    path: impl AsRef<std::path::Path>,
) -> anyhow::Result<String> {
    let imported_id = cx.update_global::<ThemeManager, _>(|theme_manager, _cx| {
        theme_manager.import_theme_config(path)
    })?;
    update_app_preferences(|preferences| {
        preferences.default_theme_id = imported_id.clone();
    })?;
    Ok(imported_id)
}

pub(crate) fn save_preferences_from_window(
    startup_open: StartupOpenPreference,
    default_theme_id: &str,
    image_paste_behavior: ImagePasteBehavior,
    fonts: &FontPreferences,
    writing_width: WritingWidthPreference,
    keybindings: BTreeMap<String, Vec<String>>,
    status_bar: &StatusBarPreferences,
    tree_sort: TreeSortPreference,
    autosave_debounce_ms: u64,
    remember_window_bounds: bool,
    window_open_position: WindowOpenPosition,
    smart_punctuation: bool,
    autosave: bool,
    zoom_percent: i64,
    default_window_width: i64,
    default_window_height: i64,
    external_change_policy: ExternalChangePolicy,
    delete_policy: DeletePolicy,
) -> anyhow::Result<AppPreferences> {
    let dirs = VeloraConfigDirs::from_system()?;
    save_preferences_from_window_with_dirs(
        startup_open,
        default_theme_id,
        image_paste_behavior,
        fonts,
        writing_width,
        keybindings,
        status_bar,
        tree_sort,
        autosave_debounce_ms,
        remember_window_bounds,
        window_open_position,
        smart_punctuation,
        autosave,
        zoom_percent,
        default_window_width,
        default_window_height,
        external_change_policy,
        delete_policy,
        &dirs,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn save_preferences_from_window_with_dirs(
    startup_open: StartupOpenPreference,
    default_theme_id: &str,
    image_paste_behavior: ImagePasteBehavior,
    fonts: &FontPreferences,
    writing_width: WritingWidthPreference,
    keybindings: BTreeMap<String, Vec<String>>,
    status_bar: &StatusBarPreferences,
    tree_sort: TreeSortPreference,
    autosave_debounce_ms: u64,
    remember_window_bounds: bool,
    window_open_position: WindowOpenPosition,
    smart_punctuation: bool,
    autosave: bool,
    zoom_percent: i64,
    default_window_width: i64,
    default_window_height: i64,
    external_change_policy: ExternalChangePolicy,
    delete_policy: DeletePolicy,
    dirs: &VeloraConfigDirs,
) -> anyhow::Result<AppPreferences> {
    let mut preferences =
        load_or_create_app_preferences_with_dirs_and_locales(dirs, sys_locale::get_locales())?;
    preferences.startup_open = startup_open;
    preferences.default_theme_id = default_theme_id.into();
    preferences.image_paste_behavior = image_paste_behavior;
    preferences.fonts = fonts.clone();
    preferences.writing_width = writing_width;
    preferences.tree_sort = tree_sort;
    preferences.autosave_debounce_ms = autosave_debounce_ms;
    preferences.remember_window_bounds = remember_window_bounds;
    preferences.window_open_position = window_open_position;
    preferences.smart_punctuation = smart_punctuation;
    preferences.autosave = autosave;
    preferences.external_change_policy = external_change_policy;
    preferences.delete_policy = delete_policy;
    preferences.zoom_percent = zoom_percent.clamp(60, 200);
    preferences.default_window_width = default_window_width.clamp(480, 4096);
    preferences.default_window_height = default_window_height.clamp(360, 4096);
    preferences.keybindings = normalize_shortcut_config(&keybindings);
    preferences.status_bar = status_bar.clone();
    save_app_preferences_with_dirs(&preferences, dirs)?;
    Ok(preferences)
}

pub(crate) fn update_app_preferences(
    update: impl FnOnce(&mut AppPreferences),
) -> anyhow::Result<AppPreferences> {
    let mut preferences = load_or_create_app_preferences()?;
    update(&mut preferences);
    save_app_preferences(&preferences)?;
    Ok(preferences)
}

