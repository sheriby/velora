//! Persistent app preferences and the preferences window.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::Context as _;
use gpui::prelude::FluentBuilder;
use gpui::*;
use serde::{Deserialize, Serialize};

use super::{VeloraConfigDirs, read_recent_files};
use crate::components::{
    ShortcutCategory, ShortcutCommand, ShortcutDefinition, install_keybindings,
    normalize_shortcut_config, normalize_shortcut_keys, resolved_shortcut_keys,
    shortcut_conflict_for, shortcut_definitions, switch::Switch,
};
use crate::i18n::{I18nManager, language_id_for_locale_preferences};
use crate::theme::{Theme, ThemeCatalogEntry, ThemeManager};
use crate::window_chrome::{custom_titlebar_height, render_custom_titlebar, velora_window_options};

const DEFAULT_THEME_ID: &str = "forest";
const DEFAULT_LANGUAGE_ID: &str = "en-US";
const PREFERENCES_VERSION: i64 = 3;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FontPreferences {
    pub(crate) markdown_family: String,
    pub(crate) markdown_size: u16,
    pub(crate) code_family: String,
    pub(crate) code_size: u16,
}

impl Default for FontPreferences {
    fn default() -> Self {
        Self {
            markdown_family: "theme".into(),
            markdown_size: 16,
            code_family: if cfg!(target_os = "windows") {
                "Consolas"
            } else {
                "Menlo"
            }
            .into(),
            code_size: 14,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum WritingWidthPreference {
    #[default]
    Theme,
    Compact,
    Standard,
    Wide,
}

impl WritingWidthPreference {
    fn as_str(self) -> &'static str {
        match self {
            Self::Theme => "theme",
            Self::Compact => "compact",
            Self::Standard => "standard",
            Self::Wide => "wide",
        }
    }

    fn from_str(value: &str) -> Self {
        match value {
            "compact" => Self::Compact,
            "standard" => Self::Standard,
            "wide" => Self::Wide,
            _ => Self::Theme,
        }
    }

    pub(crate) fn max_width(self, theme_width: f32) -> f32 {
        match self {
            Self::Theme => theme_width,
            Self::Compact => 640.0,
            Self::Standard => 760.0,
            Self::Wide => 900.0,
        }
    }
}

/// A user-configurable button shown in the status bar.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct StatusBarButton {
    pub id: String,
    pub label: String,
    pub action_id: String,
}

/// Status bar visibility and component toggles.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct StatusBarPreferences {
    pub enabled: bool,
    pub show_word_count: bool,
    pub show_cursor_position: bool,
    pub show_sidebar_toggle: bool,
    pub show_mode_switch: bool,
    pub custom_buttons: Vec<StatusBarButton>,
}

impl Default for StatusBarPreferences {
    fn default() -> Self {
        Self {
            enabled: true,
            show_word_count: true,
            show_cursor_position: true,
            show_sidebar_toggle: true,
            show_mode_switch: true,
            custom_buttons: Vec::new(),
        }
    }
}

/// Startup document selection stored in `config.toml`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StartupOpenPreference {
    NewFile,
    LastOpenedFile,
}

impl StartupOpenPreference {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::NewFile => "new_file",
            Self::LastOpenedFile => "last_opened_file",
        }
    }

    fn from_str(value: &str) -> Self {
        match value {
            "last_opened_file" => Self::LastOpenedFile,
            _ => Self::NewFile,
        }
    }
}

/// File tree ordering (roadmap D2).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum TreeSortPreference {
    /// Alphabetical by file name, directories first.
    #[default]
    Name,
    /// Most recently modified first, directories first.
    ModifiedTime,
    /// Group by extension then name, directories first.
    Type,
}

impl TreeSortPreference {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::ModifiedTime => "mtime",
            Self::Type => "type",
        }
    }

    fn from_str(value: &str) -> Self {
        match value {
            "mtime" => Self::ModifiedTime,
            "type" => Self::Type,
            _ => Self::Name,
        }
    }
}

/// 外部变更策略（roadmap H2）：默认自动重载未编辑的文档。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ExternalChangePolicy {
    /// 未编辑的文档在外部变更后自动重载（默认）。
    #[default]
    Auto,
    /// 不自动重载；保存冲突时仍走既有提示。
    Manual,
}

impl ExternalChangePolicy {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Manual => "manual",
        }
    }

    fn from_str(value: &str) -> Self {
        match value {
            "manual" => Self::Manual,
            _ => Self::Auto,
        }
    }
}

/// 删除策略（roadmap H2）：默认移入系统废纸篓。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum DeletePolicy {
    /// 移入系统废纸篓，可找回（默认，D4 行为）。
    #[default]
    Trash,
    /// 直接删除，不进废纸篓。
    Permanent,
}

impl DeletePolicy {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Trash => "trash",
            Self::Permanent => "permanent",
        }
    }

    fn from_str(value: &str) -> Self {
        match value {
            "permanent" => Self::Permanent,
            _ => Self::Trash,
        }
    }
}

/// 新窗口的打开位置（用户报修：缺少窗口位置设置）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum WindowOpenPosition {
    /// 用上次关闭时记住的位置与大小（默认，roadmap A2 行为）。
    #[default]
    Remember,
    /// 每次都在主屏居中，并按「默认窗口尺寸」打开。
    Center,
}

impl WindowOpenPosition {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Remember => "remember",
            Self::Center => "center",
        }
    }

    fn from_str(value: &str) -> Self {
        match value {
            "center" => Self::Center,
            _ => Self::Remember,
        }
    }
}

/// Where pasted clipboard images should be stored before inserting Markdown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ImagePasteBehavior {
    None,
    CopyToDocumentFolder,
    CopyToAssetsFolder,
    CopyToNamedAssetsFolder,
}

impl ImagePasteBehavior {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::CopyToDocumentFolder => "copy_to_document_folder",
            Self::CopyToAssetsFolder => "copy_to_assets_folder",
            Self::CopyToNamedAssetsFolder => "copy_to_named_assets_folder",
        }
    }

    fn from_str(value: &str) -> Self {
        match value {
            "copy_to_document_folder" => Self::CopyToDocumentFolder,
            "copy_to_assets_folder" => Self::CopyToAssetsFolder,
            "copy_to_named_assets_folder" => Self::CopyToNamedAssetsFolder,
            _ => Self::None,
        }
    }
}

/// 导出主题（roadmap F3）：跟随当前主题，或固定使用内置浅色/深色主题。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ExportThemePreference {
    /// 沿用当前应用主题（默认，保持既有导出行为）。
    #[default]
    Current,
    /// 内置浅色主题。
    Light,
    /// 内置深色主题。
    Dark,
}

impl ExportThemePreference {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    fn from_str(value: &str) -> Self {
        match value {
            "light" => Self::Light,
            "dark" => Self::Dark,
            _ => Self::Current,
        }
    }
}

/// Last window frame (logical pixels) persisted across launches.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct WindowFrame {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) width: i32,
    pub(crate) height: i32,
}

/// User preferences persisted under the app config directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AppPreferences {
    pub(crate) startup_open: StartupOpenPreference,
    pub(crate) default_language_id: String,
    pub(crate) default_theme_id: String,
    /// HTML/PDF 导出使用的主题（roadmap F3）。
    pub(crate) export_theme: ExportThemePreference,
    pub(crate) show_table_headers: bool,
    /// Typographic quote/dash substitution while typing (default off).
    pub(crate) smart_punctuation: bool,
    /// 外部变更策略（roadmap H2）。
    pub(crate) external_change_policy: ExternalChangePolicy,
    /// 删除策略（roadmap H2）。
    pub(crate) delete_policy: DeletePolicy,
    pub(crate) image_paste_behavior: ImagePasteBehavior,
    pub(crate) fonts: FontPreferences,
    pub(crate) writing_width: WritingWidthPreference,
    pub(crate) workspace_sidebar_width: u16,
    pub(crate) keybindings: BTreeMap<String, Vec<String>>,
    pub(crate) status_bar: StatusBarPreferences,
    /// Debounce before dirty changes are autosaved/recovery-snapshotted (ms).
    pub(crate) autosave_debounce_ms: u64,
    /// File tree ordering: "name" | "mtime" | "type".
    pub(crate) tree_sort: TreeSortPreference,
    pub(crate) new_file_template: String,
    pub(crate) remember_window_bounds: bool,
    pub(crate) window_frame: Option<WindowFrame>,
    /// 新窗口打开位置（roadmap A2 报修补齐的设置项）。
    pub(crate) window_open_position: WindowOpenPosition,
    /// Session-wide text zoom in percent (60..=200).
    pub(crate) zoom_percent: i64,
    /// Default window width when no remembered frame applies.
    pub(crate) default_window_width: i64,
    /// Default window height when no remembered frame applies.
    pub(crate) default_window_height: i64,
}

impl Default for AppPreferences {
    fn default() -> Self {
        Self {
            startup_open: StartupOpenPreference::NewFile,
            default_language_id: DEFAULT_LANGUAGE_ID.into(),
            default_theme_id: DEFAULT_THEME_ID.into(),
            export_theme: ExportThemePreference::Current,
            show_table_headers: true,
            smart_punctuation: false,
            external_change_policy: ExternalChangePolicy::Auto,
            delete_policy: DeletePolicy::Trash,
            image_paste_behavior: ImagePasteBehavior::CopyToAssetsFolder,
            fonts: FontPreferences::default(),
            writing_width: WritingWidthPreference::Theme,
            workspace_sidebar_width: 258,
            keybindings: BTreeMap::new(),
            status_bar: StatusBarPreferences::default(),
            autosave_debounce_ms: 800,
            tree_sort: TreeSortPreference::default(),
            new_file_template: String::new(),
            remember_window_bounds: true,
            window_frame: None,
            window_open_position: WindowOpenPosition::default(),
            zoom_percent: 100,
            default_window_width: 1080,
            default_window_height: 720,
        }
    }
}

/// Status Bar Settings
struct StatusBarSettings {
    status_bar_enabled: bool,
    status_bar_show_word_count: bool,
    status_bar_show_cursor_position: bool,
    status_bar_show_sidebar_toggle: bool,
    status_bar_show_mode_switch: bool,
}

/// Runtime-accessible editor settings mirrored from [`AppPreferences`] so the
/// render path can read them without touching disk. Toggling persists the new
/// value back to the preferences file.
pub struct EditorSettings {
    show_table_headers: bool,
    smart_punctuation: bool,
    external_change_policy: ExternalChangePolicy,
    delete_policy: DeletePolicy,
    status_bar_settings: StatusBarSettings,
    fonts: FontPreferences,
    writing_width: WritingWidthPreference,
    workspace_sidebar_width: u16,
    zoom_percent: i64,
    autosave_debounce_ms: u64,
    tree_sort: TreeSortPreference,
    new_file_template: String,
    default_window_width: i64,
    default_window_height: i64,
    window_open_position: WindowOpenPosition,
}

impl Global for EditorSettings {}

impl EditorSettings {
    pub fn init(cx: &mut App, show_table_headers: bool) {
        let preferences = read_app_preferences().ok();
        let status_bar = preferences
            .as_ref()
            .map(|p| p.status_bar.clone())
            .unwrap_or_default();
        let smart_punctuation = preferences
            .as_ref()
            .map(|p| p.smart_punctuation)
            .unwrap_or(false);
        Self::set_global(cx, show_table_headers, smart_punctuation, &status_bar);
    }

    fn set_global(
        cx: &mut App,
        show_table_headers: bool,
        smart_punctuation: bool,
        status_bar: &StatusBarPreferences,
    ) {
        let fonts = cx
            .try_global::<Self>()
            .map(|settings| settings.fonts.clone())
            .or_else(|| {
                read_app_preferences()
                    .ok()
                    .map(|preferences| preferences.fonts)
            })
            .unwrap_or_default();
        let workspace_sidebar_width = cx
            .try_global::<Self>()
            .map(|settings| settings.workspace_sidebar_width)
            .or_else(|| {
                read_app_preferences()
                    .ok()
                    .map(|preferences| preferences.workspace_sidebar_width)
            })
            .unwrap_or(258);
        let writing_width = cx
            .try_global::<Self>()
            .map(|settings| settings.writing_width)
            .or_else(|| {
                read_app_preferences()
                    .ok()
                    .map(|preferences| preferences.writing_width)
            })
            .unwrap_or_default();
        let zoom_percent = cx
            .try_global::<Self>()
            .map(|settings| settings.zoom_percent)
            .or_else(|| {
                read_app_preferences()
                    .ok()
                    .map(|preferences| preferences.zoom_percent)
            })
            .unwrap_or(100);
        let autosave_debounce_ms = cx
            .try_global::<Self>()
            .map(|settings| settings.autosave_debounce_ms)
            .or_else(|| {
                read_app_preferences()
                    .ok()
                    .map(|preferences| preferences.autosave_debounce_ms)
            })
            .unwrap_or(800);
        let tree_sort = cx
            .try_global::<Self>()
            .map(|settings| settings.tree_sort)
            .or_else(|| {
                read_app_preferences()
                    .ok()
                    .map(|preferences| preferences.tree_sort)
            })
            .unwrap_or_default();
        let new_file_template = cx
            .try_global::<Self>()
            .map(|settings| settings.new_file_template.clone())
            .or_else(|| {
                read_app_preferences()
                    .ok()
                    .map(|preferences| preferences.new_file_template)
            })
            .unwrap_or_default();
        let (default_window_width, default_window_height) = cx
            .try_global::<Self>()
            .map(|settings| {
                (settings.default_window_width, settings.default_window_height)
            })
            .or_else(|| {
                read_app_preferences().ok().map(|preferences| {
                    (preferences.default_window_width, preferences.default_window_height)
                })
            })
            .unwrap_or((1080, 720));
        let window_open_position = cx
            .try_global::<Self>()
            .map(|settings| settings.window_open_position)
            .or_else(|| {
                read_app_preferences()
                    .ok()
                    .map(|preferences| preferences.window_open_position)
            })
            .unwrap_or_default();
        let external_change_policy = cx
            .try_global::<Self>()
            .map(|settings| settings.external_change_policy)
            .or_else(|| {
                read_app_preferences()
                    .ok()
                    .map(|preferences| preferences.external_change_policy)
            })
            .unwrap_or_default();
        let delete_policy = cx
            .try_global::<Self>()
            .map(|settings| settings.delete_policy)
            .or_else(|| {
                read_app_preferences()
                    .ok()
                    .map(|preferences| preferences.delete_policy)
            })
            .unwrap_or_default();
        cx.set_global(Self {
            show_table_headers,
            smart_punctuation,
            external_change_policy,
            delete_policy,
            fonts,
            writing_width,
            workspace_sidebar_width,
            zoom_percent,
            autosave_debounce_ms,
            tree_sort,
            new_file_template,
            default_window_width,
            default_window_height,
            window_open_position,
            status_bar_settings: StatusBarSettings {
                status_bar_enabled: status_bar.enabled,
                status_bar_show_word_count: status_bar.show_word_count,
                status_bar_show_cursor_position: status_bar.show_cursor_position,
                status_bar_show_sidebar_toggle: status_bar.show_sidebar_toggle,
                status_bar_show_mode_switch: status_bar.show_mode_switch,
            },
        });
    }

    /// Whether table top rows are styled as headers. Defaults to `true` when
    /// the global has not been installed (e.g. in unit tests).
    pub fn show_table_headers(cx: &App) -> bool {
        cx.try_global::<Self>()
            .map(|settings| settings.show_table_headers)
            .unwrap_or(true)
    }

    /// Installs the settings global without touching the config file. Lets
    /// tests exercise setting-dependent behavior without cross-test file races.
    #[cfg(test)]
    pub(crate) fn install_test_settings(cx: &mut App, smart_punctuation: bool) {
        Self::set_global(
            cx,
            true,
            smart_punctuation,
            &StatusBarPreferences::default(),
        );
    }

    /// Whether typed straight quotes/dashes become typographic forms.
    /// Defaults to `false` when the global has not been installed.
    pub fn smart_punctuation(cx: &App) -> bool {
        cx.try_global::<Self>()
            .map(|settings| settings.smart_punctuation)
            .unwrap_or(false)
    }

    pub(crate) fn fonts(cx: &App) -> FontPreferences {
        cx.try_global::<Self>()
            .map(|settings| settings.fonts.clone())
            .unwrap_or_default()
    }

    pub(crate) fn writing_width(cx: &App) -> WritingWidthPreference {
        cx.try_global::<Self>()
            .map(|settings| settings.writing_width)
            .unwrap_or_default()
    }

    pub(crate) fn workspace_sidebar_width(cx: &App) -> u16 {
        cx.try_global::<Self>()
            .map(|settings| settings.workspace_sidebar_width)
            .unwrap_or(258)
    }

    /// Debounce before autosave/recovery snapshots fire (roadmap G3).
    pub(crate) fn autosave_debounce_ms(cx: &App) -> u64 {
        cx.try_global::<Self>()
            .map(|settings| settings.autosave_debounce_ms)
            .unwrap_or(800)
    }

    /// Template body for new Markdown files, with `{date}` expanded to the
    /// local date (YYYY-MM-DD).
    pub(crate) fn new_file_template() -> String {
        let template = read_app_preferences()
            .map(|preferences| preferences.new_file_template)
            .unwrap_or_default();
        template.replace("{date}", &crate::config::today_local_date())
    }

    pub(crate) fn set_autosave_debounce_ms(cx: &mut App, ms: u64) {
        if cx.try_global::<Self>().is_some() {
            cx.update_global::<Self, _>(|settings, _cx| settings.autosave_debounce_ms = ms);
        }
        if let Err(error) =
            update_app_preferences(|preferences| preferences.autosave_debounce_ms = ms)
        {
            eprintln!("failed to save autosave debounce: {error}");
        }
    }

    /// File tree ordering (roadmap D2).
    pub(crate) fn tree_sort(cx: &App) -> TreeSortPreference {
        cx.try_global::<Self>()
            .map(|settings| settings.tree_sort)
            .unwrap_or_default()
    }

    /// 外部变更策略（roadmap H2）；默认自动重载。
    pub(crate) fn external_change_policy(cx: &App) -> ExternalChangePolicy {
        cx.try_global::<Self>()
            .map(|settings| settings.external_change_policy)
            .unwrap_or_default()
    }

    pub(crate) fn set_external_change_policy(cx: &mut App, policy: ExternalChangePolicy) {
        if cx.try_global::<Self>().is_some() {
            cx.update_global::<Self, _>(|settings, _cx| settings.external_change_policy = policy);
        }
        if let Err(error) =
            update_app_preferences(|preferences| preferences.external_change_policy = policy)
        {
            eprintln!("failed to save external change policy: {error}");
        }
    }

    /// 删除策略（roadmap H2）；默认移入废纸篓。
    pub(crate) fn delete_policy(cx: &App) -> DeletePolicy {
        cx.try_global::<Self>()
            .map(|settings| settings.delete_policy)
            .unwrap_or_default()
    }

    pub(crate) fn set_delete_policy(cx: &mut App, policy: DeletePolicy) {
        if cx.try_global::<Self>().is_some() {
            cx.update_global::<Self, _>(|settings, _cx| settings.delete_policy = policy);
        }
        if let Err(error) =
            update_app_preferences(|preferences| preferences.delete_policy = policy)
        {
            eprintln!("failed to save delete policy: {error}");
        }
    }

    pub(crate) fn set_tree_sort(cx: &mut App, sort: TreeSortPreference) {
        if cx.try_global::<Self>().is_some() {
            cx.update_global::<Self, _>(|settings, _cx| settings.tree_sort = sort);
        }
        if let Err(error) =
            update_app_preferences(|preferences| preferences.tree_sort = sort)
        {
            eprintln!("failed to save tree sort: {error}");
        }
    }

    /// Session-wide text zoom percent (60-200); cached mirror of [window]
    /// zoom_percent.
    pub(crate) fn zoom_percent(cx: &App) -> i64 {
        cx.try_global::<Self>()
            .map(|settings| settings.zoom_percent)
            .unwrap_or(100)
    }

    pub(crate) fn set_zoom_percent(cx: &mut App, percent: i64) {
        if cx.try_global::<Self>().is_some() {
            cx.update_global::<Self, _>(|settings, _cx| settings.zoom_percent = percent);
        }
        if let Err(error) = update_app_preferences(|preferences| {
            preferences.zoom_percent = percent;
        }) {
            eprintln!("failed to save zoom percent: {error}");
        }
    }

    /// Default editor window size used when no remembered frame applies.
    pub(crate) fn default_window_size(cx: &App) -> (i64, i64) {
        cx.try_global::<Self>()
            .map(|settings| (settings.default_window_width, settings.default_window_height))
            .unwrap_or((1080, 720))
    }

    /// 新窗口打开位置（roadmap A2 报修补齐），[window] open_position 的内存镜像。
    pub(crate) fn window_open_position(cx: &App) -> WindowOpenPosition {
        cx.try_global::<Self>()
            .map(|settings| settings.window_open_position)
            .unwrap_or_default()
    }

    pub(crate) fn set_window_open_position(cx: &mut App, position: WindowOpenPosition) {
        if cx.try_global::<Self>().is_some() {
            cx.update_global::<Self, _>(|settings, _cx| {
                settings.window_open_position = position;
            });
        }
        if let Err(error) = update_app_preferences(|preferences| {
            preferences.window_open_position = position;
        }) {
            eprintln!("failed to save window open position: {error}");
        }
    }

    pub(crate) fn set_workspace_sidebar_width(cx: &mut App, width: u16) {
        if cx.try_global::<Self>().is_some() {
            cx.update_global::<Self, _>(|settings, _cx| {
                settings.workspace_sidebar_width = width;
            });
        }
        if let Err(error) =
            update_app_preferences(|preferences| preferences.workspace_sidebar_width = width)
        {
            eprintln!("failed to save workspace sidebar width: {error}");
        }
    }

    pub fn set_show_table_headers(cx: &mut App, show_table_headers: bool) {
        let status_bar = cx
            .try_global::<Self>()
            .map(|s| StatusBarPreferences {
                enabled: s.status_bar_settings.status_bar_enabled,
                show_word_count: s.status_bar_settings.status_bar_show_word_count,
                show_cursor_position: s.status_bar_settings.status_bar_show_cursor_position,
                show_sidebar_toggle: s.status_bar_settings.status_bar_show_sidebar_toggle,
                show_mode_switch: s.status_bar_settings.status_bar_show_mode_switch,
                custom_buttons: Vec::new(),
            })
            .unwrap_or_default();
        Self::set_global(cx, show_table_headers, Self::smart_punctuation(cx), &status_bar);
        match read_app_preferences() {
            Ok(mut preferences) => {
                preferences.show_table_headers = show_table_headers;
                if let Err(err) = save_app_preferences(&preferences) {
                    eprintln!("failed to save table header preference: {err}");
                }
            }
            Err(err) => eprintln!("failed to read table header preference: {err}"),
        }
    }

    pub fn set_smart_punctuation(cx: &mut App, smart_punctuation: bool) {
        let status_bar = cx
            .try_global::<Self>()
            .map(|s| StatusBarPreferences {
                enabled: s.status_bar_settings.status_bar_enabled,
                show_word_count: s.status_bar_settings.status_bar_show_word_count,
                show_cursor_position: s.status_bar_settings.status_bar_show_cursor_position,
                show_sidebar_toggle: s.status_bar_settings.status_bar_show_sidebar_toggle,
                show_mode_switch: s.status_bar_settings.status_bar_show_mode_switch,
                custom_buttons: Vec::new(),
            })
            .unwrap_or_default();
        let show_table_headers = Self::show_table_headers(cx);
        Self::set_global(cx, show_table_headers, smart_punctuation, &status_bar);
        match read_app_preferences() {
            Ok(mut preferences) => {
                preferences.smart_punctuation = smart_punctuation;
                if let Err(err) = save_app_preferences(&preferences) {
                    eprintln!("failed to save smart punctuation preference: {err}");
                }
            }
            Err(err) => eprintln!("failed to read smart punctuation preference: {err}"),
        }
    }

    pub fn status_bar_preferences(cx: &App) -> StatusBarPreferences {
        cx.try_global::<Self>()
            .map(|s| StatusBarPreferences {
                enabled: s.status_bar_settings.status_bar_enabled,
                show_word_count: s.status_bar_settings.status_bar_show_word_count,
                show_cursor_position: s.status_bar_settings.status_bar_show_cursor_position,
                show_sidebar_toggle: s.status_bar_settings.status_bar_show_sidebar_toggle,
                show_mode_switch: s.status_bar_settings.status_bar_show_mode_switch,
                custom_buttons: Vec::new(),
            })
            .unwrap_or_default()
    }
}

#[derive(Serialize)]
struct PreferencesFile {
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

fn load_preferences_from_toml_value(
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

fn app_preferences_from_toml_value(
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

fn detected_language_id_from_locales<I, S>(locales: I) -> &'static str
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    language_id_for_locale_preferences(locales)
}

fn load_or_create_app_preferences_with_dirs_and_locales<I, S>(
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
        zoom_percent,
        default_window_width,
        default_window_height,
        external_change_policy,
        delete_policy,
        &dirs,
    )
}

#[allow(clippy::too_many_arguments)]
fn save_preferences_from_window_with_dirs(
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

fn update_app_preferences(
    update: impl FnOnce(&mut AppPreferences),
) -> anyhow::Result<AppPreferences> {
    let mut preferences = load_or_create_app_preferences()?;
    update(&mut preferences);
    save_app_preferences(&preferences)?;
    Ok(preferences)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PreferencesNav {
    File,
    Theme,
    Image,
    Shortcuts,
    StatusBar,
    Window,
}

/// Independent preferences window view.
pub(crate) struct PreferencesWindow {
    nav: PreferencesNav,
    startup_open: StartupOpenPreference,
    selected_theme_id: String,
    image_paste_behavior: ImagePasteBehavior,
    fonts: FontPreferences,
    keybindings: BTreeMap<String, Vec<String>>,
    saved_startup_open: StartupOpenPreference,
    saved_theme_id: String,
    saved_image_paste_behavior: ImagePasteBehavior,
    saved_fonts: FontPreferences,
    writing_width: WritingWidthPreference,
    saved_writing_width: WritingWidthPreference,
    saved_keybindings: BTreeMap<String, Vec<String>>,
    theme_options: Vec<ThemeCatalogEntry>,
    focus_handle: FocusHandle,
    startup_dropdown_open: bool,
    theme_dropdown_open: bool,
    image_dropdown_open: bool,
    markdown_font_dropdown_open: bool,
    writing_width_dropdown_open: bool,
    code_font_dropdown_open: bool,
    recording_shortcut: Option<ShortcutCommand>,
    shortcut_error: Option<String>,
    /// 保存失败时在本页顶部内联显示，不弹系统原生对话框（用户要求）。
    save_error: Option<String>,
    /// 右侧内容区的滚动位置（单测用它验「真的能滚」）。
    page_scroll: ScrollHandle,
    tree_sort: TreeSortPreference,
    autosave_debounce_ms: u64,
    remember_window_bounds: bool,
    window_open_position: WindowOpenPosition,
    smart_punctuation: bool,
    zoom_percent: i64,
    default_window_width: i64,
    default_window_height: i64,
    external_change_policy: ExternalChangePolicy,
    delete_policy: DeletePolicy,
    zoom_dropdown_open: bool,
    window_size_dropdown_open: bool,
    window_open_position_dropdown_open: bool,
    external_change_dropdown_open: bool,
    delete_policy_dropdown_open: bool,
    saved_tree_sort: TreeSortPreference,
    saved_autosave_debounce_ms: u64,
    saved_remember_window_bounds: bool,
    saved_window_open_position: WindowOpenPosition,
    saved_smart_punctuation: bool,
    saved_zoom_percent: i64,
    saved_default_window_width: i64,
    saved_default_window_height: i64,
    saved_external_change_policy: ExternalChangePolicy,
    saved_delete_policy: DeletePolicy,
    tree_sort_dropdown_open: bool,
    autosave_dropdown_open: bool,
    status_bar_enabled: bool,
    status_bar_show_word_count: bool,
    status_bar_show_cursor_position: bool,
    status_bar_show_sidebar_toggle: bool,
    status_bar_show_mode_switch: bool,
    saved_status_bar_enabled: bool,
    saved_status_bar_show_word_count: bool,
    saved_status_bar_show_cursor_position: bool,
    saved_status_bar_show_sidebar_toggle: bool,
    saved_status_bar_show_mode_switch: bool,
    system_appearance_subscription: Option<Subscription>,
}

impl PreferencesWindow {
    fn new(
        preferences: AppPreferences,
        theme_options: Vec<ThemeCatalogEntry>,
        cx: &mut Context<Self>,
    ) -> Self {
        let selected_theme_id = if theme_options
            .iter()
            .any(|entry| entry.id == preferences.default_theme_id)
        {
            preferences.default_theme_id
        } else {
            DEFAULT_THEME_ID.into()
        };
        let startup_open = preferences.startup_open;
        let image_paste_behavior = preferences.image_paste_behavior;
        let fonts = preferences.fonts;
        let writing_width = preferences.writing_width;
        let keybindings = preferences.keybindings;
        let tree_sort = preferences.tree_sort;
        let autosave_debounce_ms = preferences.autosave_debounce_ms;
        let remember_window_bounds = preferences.remember_window_bounds;
        let window_open_position = preferences.window_open_position;
        let smart_punctuation = preferences.smart_punctuation;
        let zoom_percent = preferences.zoom_percent;
        let default_window_width = preferences.default_window_width;
        let default_window_height = preferences.default_window_height;
        let external_change_policy = preferences.external_change_policy;
        let delete_policy = preferences.delete_policy;
        Self {
            nav: PreferencesNav::File,
            startup_open,
            selected_theme_id: selected_theme_id.clone(),
            image_paste_behavior,
            fonts: fonts.clone(),
            writing_width,
            keybindings: keybindings.clone(),
            saved_startup_open: startup_open,
            saved_theme_id: selected_theme_id,
            saved_image_paste_behavior: image_paste_behavior,
            saved_fonts: fonts,
            saved_writing_width: writing_width,
            saved_keybindings: keybindings,
            tree_sort,
            autosave_debounce_ms,
            remember_window_bounds,
            window_open_position,
            smart_punctuation,
            zoom_percent,
            default_window_width,
            default_window_height,
            external_change_policy,
            delete_policy,
            zoom_dropdown_open: false,
            window_size_dropdown_open: false,
            window_open_position_dropdown_open: false,
            external_change_dropdown_open: false,
            delete_policy_dropdown_open: false,
            saved_tree_sort: tree_sort,
            saved_autosave_debounce_ms: autosave_debounce_ms,
            saved_remember_window_bounds: remember_window_bounds,
            saved_window_open_position: window_open_position,
            saved_smart_punctuation: smart_punctuation,
            saved_zoom_percent: zoom_percent,
            saved_default_window_width: default_window_width,
            saved_default_window_height: default_window_height,
            saved_external_change_policy: external_change_policy,
            saved_delete_policy: delete_policy,
            tree_sort_dropdown_open: false,
            autosave_dropdown_open: false,
            theme_options,
            focus_handle: cx.focus_handle(),
            startup_dropdown_open: false,
            theme_dropdown_open: false,
            image_dropdown_open: false,
            markdown_font_dropdown_open: false,
            writing_width_dropdown_open: false,
            code_font_dropdown_open: false,
            recording_shortcut: None,
            shortcut_error: None,
            save_error: None,
            page_scroll: ScrollHandle::new(),
            status_bar_enabled: preferences.status_bar.enabled,
            status_bar_show_word_count: preferences.status_bar.show_word_count,
            status_bar_show_cursor_position: preferences.status_bar.show_cursor_position,
            status_bar_show_sidebar_toggle: preferences.status_bar.show_sidebar_toggle,
            status_bar_show_mode_switch: preferences.status_bar.show_mode_switch,
            saved_status_bar_enabled: preferences.status_bar.enabled,
            saved_status_bar_show_word_count: preferences.status_bar.show_word_count,
            saved_status_bar_show_cursor_position: preferences.status_bar.show_cursor_position,
            saved_status_bar_show_sidebar_toggle: preferences.status_bar.show_sidebar_toggle,
            saved_status_bar_show_mode_switch: preferences.status_bar.show_mode_switch,
            system_appearance_subscription: None,
        }
    }

    fn theme_display_name(
        &self,
        entry: &ThemeCatalogEntry,
        strings: &crate::i18n::I18nStrings,
    ) -> String {
        match entry.id.as_str() {
            "system" => strings.preferences_theme_system.clone(),
            "velora-dark" => strings.preferences_theme_dark.clone(),
            "velora-light" => strings.preferences_theme_light.clone(),
            _ => entry.name.clone(),
        }
    }

    fn selected_theme_name(&self, strings: &crate::i18n::I18nStrings) -> String {
        self.theme_options
            .iter()
            .find(|entry| entry.id == self.selected_theme_id)
            .map(|entry| self.theme_display_name(entry, strings))
            .unwrap_or_else(|| strings.preferences_theme_system.clone())
    }

    fn has_unsaved_changes(&self) -> bool {
        self.startup_open != self.saved_startup_open
            || self.selected_theme_id != self.saved_theme_id
            || self.image_paste_behavior != self.saved_image_paste_behavior
            || self.fonts != self.saved_fonts
            || self.writing_width != self.saved_writing_width
            || normalize_shortcut_config(&self.keybindings)
                != normalize_shortcut_config(&self.saved_keybindings)
            || self.status_bar_enabled != self.saved_status_bar_enabled
            || self.status_bar_show_word_count != self.saved_status_bar_show_word_count
            || self.status_bar_show_cursor_position != self.saved_status_bar_show_cursor_position
            || self.status_bar_show_sidebar_toggle != self.saved_status_bar_show_sidebar_toggle
            || self.status_bar_show_mode_switch != self.saved_status_bar_show_mode_switch
            || self.tree_sort != self.saved_tree_sort
            || self.autosave_debounce_ms != self.saved_autosave_debounce_ms
            || self.remember_window_bounds != self.saved_remember_window_bounds
            || self.window_open_position != self.saved_window_open_position
            || self.smart_punctuation != self.saved_smart_punctuation
            || self.zoom_percent != self.saved_zoom_percent
            || self.default_window_width != self.saved_default_window_width
            || self.default_window_height != self.saved_default_window_height
            || self.external_change_policy != self.saved_external_change_policy
            || self.delete_policy != self.saved_delete_policy
    }

    fn toggle_tree_sort_dropdown(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.tree_sort_dropdown_open = !self.tree_sort_dropdown_open;
        cx.notify();
    }

    fn toggle_autosave_dropdown(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.autosave_dropdown_open = !self.autosave_dropdown_open;
        cx.notify();
    }

    fn toggle_external_change_dropdown(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.external_change_dropdown_open = !self.external_change_dropdown_open;
        cx.notify();
    }

    fn toggle_delete_policy_dropdown(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.delete_policy_dropdown_open = !self.delete_policy_dropdown_open;
        cx.notify();
    }

    fn toggle_zoom_dropdown(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.zoom_dropdown_open = !self.zoom_dropdown_open;
        self.window_size_dropdown_open = false;
        cx.notify();
    }

    fn toggle_window_size_dropdown(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.window_size_dropdown_open = !self.window_size_dropdown_open;
        self.zoom_dropdown_open = false;
        self.window_open_position_dropdown_open = false;
        cx.notify();
    }

    fn toggle_window_open_position_dropdown(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.window_open_position_dropdown_open = !self.window_open_position_dropdown_open;
        self.zoom_dropdown_open = false;
        self.window_size_dropdown_open = false;
        cx.notify();
    }

    fn set_nav_file(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.nav = PreferencesNav::File;
        self.startup_dropdown_open = false;
        self.theme_dropdown_open = false;
        self.writing_width_dropdown_open = false;
        self.image_dropdown_open = false;
        self.recording_shortcut = None;
        cx.notify();
    }

    fn set_nav_theme(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.nav = PreferencesNav::Theme;
        self.startup_dropdown_open = false;
        self.theme_dropdown_open = false;
        self.writing_width_dropdown_open = false;
        self.image_dropdown_open = false;
        self.recording_shortcut = None;
        cx.notify();
    }

    fn set_nav_image(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.nav = PreferencesNav::Image;
        self.startup_dropdown_open = false;
        self.theme_dropdown_open = false;
        self.writing_width_dropdown_open = false;
        self.image_dropdown_open = false;
        self.recording_shortcut = None;
        cx.notify();
    }

    fn set_nav_shortcuts(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.nav = PreferencesNav::Shortcuts;
        self.startup_dropdown_open = false;
        self.theme_dropdown_open = false;
        self.writing_width_dropdown_open = false;
        self.image_dropdown_open = false;
        self.shortcut_error = None;
        cx.notify();
    }

    fn set_nav_window(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.nav = PreferencesNav::Window;
        cx.notify();
    }

    fn set_nav_status_bar(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.nav = PreferencesNav::StatusBar;
        self.startup_dropdown_open = false;
        self.theme_dropdown_open = false;
        self.writing_width_dropdown_open = false;
        self.image_dropdown_open = false;
        self.recording_shortcut = None;
        cx.notify();
    }

    fn toggle_startup_dropdown(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.startup_dropdown_open = !self.startup_dropdown_open;
        self.theme_dropdown_open = false;
        self.image_dropdown_open = false;
        cx.notify();
    }

    fn toggle_theme_dropdown(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.theme_dropdown_open = !self.theme_dropdown_open;
        self.writing_width_dropdown_open = false;
        self.startup_dropdown_open = false;
        self.image_dropdown_open = false;
        cx.notify();
    }

    fn toggle_writing_width_dropdown(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.writing_width_dropdown_open = !self.writing_width_dropdown_open;
        self.theme_dropdown_open = false;
        self.markdown_font_dropdown_open = false;
        self.code_font_dropdown_open = false;
        cx.notify();
    }

    fn toggle_markdown_font_dropdown(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.markdown_font_dropdown_open = !self.markdown_font_dropdown_open;
        self.writing_width_dropdown_open = false;
        self.code_font_dropdown_open = false;
        cx.notify();
    }

    fn toggle_code_font_dropdown(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.code_font_dropdown_open = !self.code_font_dropdown_open;
        self.writing_width_dropdown_open = false;
        self.markdown_font_dropdown_open = false;
        cx.notify();
    }

    fn toggle_image_dropdown(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.image_dropdown_open = !self.image_dropdown_open;
        self.startup_dropdown_open = false;
        self.theme_dropdown_open = false;
        cx.notify();
    }

    fn cancel(&mut self, _: &ClickEvent, window: &mut Window, _: &mut Context<Self>) {
        window.remove_window();
    }

    fn on_titlebar_close(
        &mut self,
        event: &ClickEvent,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        if event.standard_click() {
            window.remove_window();
        }
    }

    fn save(&mut self, _: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !self.has_unsaved_changes() {
            return;
        }

        self.save_error = None;
        let preferences = match save_preferences_from_window(
            self.startup_open,
            &self.selected_theme_id,
            self.image_paste_behavior,
            &self.fonts,
            self.writing_width,
            self.keybindings.clone(),
            &StatusBarPreferences {
                enabled: self.status_bar_enabled,
                show_word_count: self.status_bar_show_word_count,
                show_cursor_position: self.status_bar_show_cursor_position,
                show_sidebar_toggle: self.status_bar_show_sidebar_toggle,
                show_mode_switch: self.status_bar_show_mode_switch,
                custom_buttons: Vec::new(),
            },
            self.tree_sort,
            self.autosave_debounce_ms,
            self.remember_window_bounds,
            self.window_open_position,
            self.smart_punctuation,
            self.zoom_percent,
            self.default_window_width,
            self.default_window_height,
            self.external_change_policy,
            self.delete_policy,
        ) {
            Ok(preferences) => preferences,
            Err(err) => {
                let strings = cx.global::<I18nManager>().strings().clone();
                self.save_error = Some(format!(
                    "{}: {}",
                    strings.preferences_save_failed_title,
                    err
                ));
                cx.notify();
                return;
            }
        };

        // 同步新设置到全局缓存并刷新（roadmap H1）。
        EditorSettings::set_tree_sort(cx, self.tree_sort);
        EditorSettings::set_autosave_debounce_ms(cx, self.autosave_debounce_ms);
        EditorSettings::set_smart_punctuation(cx, self.smart_punctuation);
        EditorSettings::set_zoom_percent(cx, self.zoom_percent);
        EditorSettings::set_external_change_policy(cx, self.external_change_policy);
        EditorSettings::set_delete_policy(cx, self.delete_policy);
        EditorSettings::set_window_open_position(cx, self.window_open_position);
        cx.update_global::<EditorSettings, _>(|settings, _cx| {
            settings.default_window_width = self.default_window_width;
            settings.default_window_height = self.default_window_height;
        });
        self.apply_saved_preferences(preferences, window, cx);
    }

    fn apply_saved_preferences(
        &mut self,
        preferences: AppPreferences,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let theme_changed = cx.update_global::<ThemeManager, _>(|theme_manager, _cx| {
            theme_manager.set_theme_by_id(&preferences.default_theme_id)
        });
        if !theme_changed {
            let _ = cx.update_global::<ThemeManager, _>(|theme_manager, _cx| {
                theme_manager.set_theme_by_id(DEFAULT_THEME_ID)
            });
        }
        cx.clear_key_bindings();
        install_keybindings(cx, &preferences.keybindings);
        crate::app_menu::install_menus(cx);
        cx.update_global::<EditorSettings, _>(|settings, _cx| {
            settings.status_bar_settings.status_bar_enabled = preferences.status_bar.enabled;
            settings.status_bar_settings.status_bar_show_word_count =
                preferences.status_bar.show_word_count;
            settings.status_bar_settings.status_bar_show_cursor_position =
                preferences.status_bar.show_cursor_position;
            settings.status_bar_settings.status_bar_show_sidebar_toggle =
                preferences.status_bar.show_sidebar_toggle;
            settings.status_bar_settings.status_bar_show_mode_switch =
                preferences.status_bar.show_mode_switch;
            settings.fonts = preferences.fonts.clone();
            settings.writing_width = preferences.writing_width;
            settings.window_open_position = preferences.window_open_position;
        });
        cx.refresh_windows();
        window.activate_window();
        self.focus_handle.focus(window);
        self.saved_startup_open = self.startup_open;
        self.saved_theme_id = self.selected_theme_id.clone();
        self.saved_image_paste_behavior = self.image_paste_behavior;
        self.saved_fonts = self.fonts.clone();
        self.saved_writing_width = self.writing_width;
        self.saved_keybindings = normalize_shortcut_config(&self.keybindings);
        self.saved_status_bar_enabled = self.status_bar_enabled;
        self.saved_status_bar_show_word_count = self.status_bar_show_word_count;
        self.saved_status_bar_show_cursor_position = self.status_bar_show_cursor_position;
        self.saved_status_bar_show_sidebar_toggle = self.status_bar_show_sidebar_toggle;
        self.saved_status_bar_show_mode_switch = self.status_bar_show_mode_switch;
        self.saved_tree_sort = self.tree_sort;
        self.saved_autosave_debounce_ms = self.autosave_debounce_ms;
        self.saved_remember_window_bounds = self.remember_window_bounds;
        self.saved_window_open_position = self.window_open_position;
        self.saved_smart_punctuation = self.smart_punctuation;
        self.saved_zoom_percent = self.zoom_percent;
        self.saved_default_window_width = self.default_window_width;
        self.saved_default_window_height = self.default_window_height;
        self.saved_external_change_policy = self.external_change_policy;
        self.saved_delete_policy = self.delete_policy;
        cx.notify();
    }

    /// 侧边栏的一项：左对齐、选中时用强调色 + 左侧强调条。
    fn nav_button(
        &self,
        id: &'static str,
        label: String,
        selected: bool,
        theme: &Theme,
        on_click: fn(&mut Self, &ClickEvent, &mut Window, &mut Context<Self>),
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let accent = c.dialog_primary_button_bg;
        div()
            .id(id)
            .debug_selector(move || id.to_string())
            .w_full()
            .h(px(34.0))
            .px(px(10.0))
            .flex()
            .items_center()
            .gap(px(9.0))
            .rounded(px(d.menu_item_radius))
            .cursor_pointer()
            .text_size(px(t.dialog_body_size))
            .font_weight(t.dialog_button_weight.to_font_weight())
            .text_color(if selected {
                c.dialog_title
            } else {
                c.dialog_muted
            })
            .bg(if selected {
                c.selection
            } else {
                c.dialog_surface
            })
            .hover(move |this| {
                this.bg(if selected {
                    c.selection
                } else {
                    c.dialog_secondary_button_hover
                })
            })
            // 左侧强调条：未选中时用背景色占位，保证两种状态文字不左右跳。
            .child(
                div()
                    .w(px(3.0))
                    .h(px(16.0))
                    .flex_shrink_0()
                    .rounded(px(2.0))
                    .bg(if selected { accent } else { c.dialog_surface }),
            )
            .child(label)
            .on_click(cx.listener(on_click))
    }

    /// 页面标题（放在滚动区外，不跟着内容滚）。
    fn page_header(&self, title: String, theme: &Theme) -> AnyElement {
        let c = &theme.colors;
        let t = &theme.typography;
        div()
            .w_full()
            .flex_shrink_0()
            .text_size(px(t.dialog_title_size))
            .font_weight(t.dialog_title_weight.to_font_weight())
            .text_color(c.dialog_title)
            .child(SharedString::from(title))
            .into_any_element()
    }

    /// 一组设置：行装在圆角面板里，行与行之间一条细线。
    fn settings_card(&self, theme: &Theme, rows: Vec<AnyElement>) -> AnyElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let mut column = div().w_full().flex().flex_col();
        for (index, row) in rows.into_iter().enumerate() {
            if index > 0 {
                column = column.child(
                    div()
                        .w_full()
                        .h(px(d.dialog_border_width.max(1.0)))
                        .bg(c.dialog_border),
                );
            }
            column = column.child(row);
        }
        div()
            .w_full()
            .flex_shrink_0()
            .rounded(px(d.dialog_radius))
            .border(px(d.dialog_border_width))
            .border_color(c.dialog_border)
            .bg(c.dialog_surface)
            .overflow_hidden()
            .child(column)
            .into_any_element()
    }

    /// 一行设置：左边标签，右边控件；整行悬停时高亮。
    fn settings_row(
        &self,
        theme: &Theme,
        label: impl Into<SharedString>,
        control: impl IntoElement,
    ) -> AnyElement {
        let c = &theme.colors;
        let t = &theme.typography;
        div()
            .w_full()
            .min_h(px(52.0))
            .px(px(14.0))
            .py(px(8.0))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(16.0))
            .hover(|this| this.bg(c.dialog_secondary_button_hover))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .text_size(px(t.dialog_body_size))
                    .text_color(c.dialog_body)
                    .child(label.into()),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_end()
                    .child(control),
            )
            .into_any_element()
    }

    /// 页面顶部的错误条（保存失败、快捷键冲突）。
    fn error_banner(&self, theme: &Theme, message: String) -> AnyElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        div()
            .w_full()
            .flex_shrink_0()
            .px(px(12.0))
            .py(px(8.0))
            .rounded(px((d.dialog_radius - 4.0).max(4.0)))
            .border(px(d.dialog_border_width))
            .border_color(c.dialog_danger_button_bg)
            .bg(c.dialog_surface)
            .text_size(px(t.dialog_body_size))
            .text_color(c.dialog_danger_button_bg)
            .child(SharedString::from(message))
            .into_any_element()
    }

    fn dropdown_button(
        id: &'static str,
        label: String,
        theme: &Theme,
        on_click: fn(&mut Self, &ClickEvent, &mut Window, &mut Context<Self>),
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        div()
            .id(id)
            .w(px(200.0))
            .h(px(32.0))
            .px(px(10.0))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(8.0))
            .rounded(px(d.menu_item_radius))
            .border(px(d.dialog_border_width))
            .border_color(c.dialog_border)
            .bg(c.dialog_secondary_button_bg)
            .hover(|this| this.bg(c.dialog_secondary_button_hover))
            .cursor_pointer()
            .text_size(px(t.dialog_body_size))
            .text_color(c.dialog_body)
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .truncate()
                    .child(SharedString::from(label)),
            )
            .child(
                svg()
                    .path("icon/workspace/chevron-down.svg")
                    .size(px(10.0))
                    .text_color(c.dialog_muted),
            )
            .on_click(cx.listener(on_click))
    }

    fn dropdown_item(
        id: impl Into<ElementId>,
        label: String,
        selected: bool,
        theme: &Theme,
        on_click: impl Fn(&mut Self, &ClickEvent, &mut Window, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        div()
            .id(id)
            .w(px(200.0))
            .min_h(px(30.0))
            .px(px(10.0))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(8.0))
            .rounded(px(d.menu_item_radius))
            .cursor_pointer()
            .bg(if selected {
                c.selection
            } else {
                c.dialog_secondary_button_bg
            })
            .hover(|this| this.bg(c.dialog_secondary_button_hover))
            .text_size(px(t.dialog_body_size))
            .text_color(if selected {
                c.dialog_title
            } else {
                c.dialog_body
            })
            .child(SharedString::from(label))
            .when(selected, |this| {
                this.child(
                    svg()
                        .path("icon/workspace/check.svg")
                        .size(px(10.0))
                        .text_color(c.dialog_primary_button_bg),
                )
            })
            .on_click(cx.listener(on_click))
    }

    fn theme_dropdown_item(
        index: usize,
        label: String,
        selected: bool,
        preview: (Hsla, Hsla, Hsla),
        theme: &Theme,
        on_click: impl Fn(&mut Self, &ClickEvent, &mut Window, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let (surface, text, accent) = preview;
        div()
            .id(("preferences-theme-option", index))
            .w(px(200.0))
            .min_h(px(38.0))
            .px(px(10.0))
            .flex()
            .items_center()
            .gap(px(10.0))
            .rounded(px(d.menu_item_radius))
            .cursor_pointer()
            .bg(if selected {
                c.selection
            } else {
                c.dialog_secondary_button_bg
            })
            .hover(|this| this.bg(c.dialog_secondary_button_hover))
            .text_size(px(t.dialog_body_size))
            .text_color(c.dialog_body)
            .child(
                div()
                    .w(px(36.0))
                    .h(px(24.0))
                    .px(px(5.0))
                    .flex_shrink_0()
                    .flex()
                    .flex_col()
                    .justify_center()
                    .gap(px(3.0))
                    .rounded(px(4.0))
                    .border(px(1.0))
                    .border_color(c.dialog_border)
                    .bg(surface)
                    .child(div().w(px(20.0)).h(px(3.0)).rounded(px(2.0)).bg(text))
                    .child(div().w(px(12.0)).h(px(3.0)).rounded(px(2.0)).bg(accent)),
            )
            .child(div().flex_1().min_w(px(0.0)).truncate().child(SharedString::from(label)))
            .when(selected, |this| {
                this.child(
                    svg()
                        .path("icon/workspace/check.svg")
                        .size(px(10.0))
                        .text_color(c.dialog_primary_button_bg),
                )
            })
            .on_click(cx.listener(on_click))
    }

    fn render_startup_page(
        &self,
        theme: &Theme,
        strings: &crate::i18n::I18nStrings,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = match self.startup_open {
            StartupOpenPreference::NewFile => strings.preferences_startup_new_file.clone(),
            StartupOpenPreference::LastOpenedFile => {
                strings.preferences_startup_last_opened_file.clone()
            }
        };
        let mut dropdown = div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(Self::dropdown_button(
                "preferences-startup-dropdown",
                selected,
                theme,
                Self::toggle_startup_dropdown,
                cx,
            ));
        if self.startup_dropdown_open {
            let new_file_label = strings.preferences_startup_new_file.clone();
            let last_file_label = strings.preferences_startup_last_opened_file.clone();
            dropdown = dropdown
                .child(Self::dropdown_item(
                    "preferences-startup-new-file",
                    new_file_label,
                    self.startup_open == StartupOpenPreference::NewFile,
                    theme,
                    |this, _, _, cx| {
                        this.startup_open = StartupOpenPreference::NewFile;
                        this.startup_dropdown_open = false;
                        cx.notify();
                    },
                    cx,
                ))
                .child(Self::dropdown_item(
                    "preferences-startup-last-opened-file",
                    last_file_label,
                    self.startup_open == StartupOpenPreference::LastOpenedFile,
                    theme,
                    |this, _, _, cx| {
                        this.startup_open = StartupOpenPreference::LastOpenedFile;
                        this.startup_dropdown_open = false;
                        cx.notify();
                    },
                    cx,
                ));
        }
        let tree_sort_selected = match self.tree_sort {
            TreeSortPreference::Name => strings.tree_sort_name.clone(),
            TreeSortPreference::ModifiedTime => strings.tree_sort_mtime.clone(),
            TreeSortPreference::Type => strings.tree_sort_type.clone(),
        };
        let mut tree_sort_dropdown = div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(Self::dropdown_button(
                "preferences-tree-sort-dropdown",
                tree_sort_selected,
                theme,
                Self::toggle_tree_sort_dropdown,
                cx,
            ));
        if self.tree_sort_dropdown_open {
            let options = [
                (TreeSortPreference::Name, strings.tree_sort_name.clone()),
                (TreeSortPreference::ModifiedTime, strings.tree_sort_mtime.clone()),
                (TreeSortPreference::Type, strings.tree_sort_type.clone()),
            ];
            for (index, (sort, label)) in options.into_iter().enumerate() {
                let is_selected = self.tree_sort == sort;
                let sort_value = sort;
                tree_sort_dropdown = tree_sort_dropdown.child(
                    Self::dropdown_item(
                        gpui::SharedString::from(format!("preferences-tree-sort-{index}")),
                        label,
                        is_selected,
                        theme,
                        move |this, _, _, cx| {
                            this.tree_sort = sort_value;
                            this.tree_sort_dropdown_open = false;
                            cx.notify();
                        },
                        cx,
                    ),
                );
            }
        }

        let debounce_selected = format!("{} ms", self.autosave_debounce_ms);
        let mut debounce_dropdown = div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(Self::dropdown_button(
                "preferences-autosave-dropdown",
                debounce_selected.clone().into(),
                theme,
                Self::toggle_autosave_dropdown,
                cx,
            ));
        if self.autosave_dropdown_open {
            for ms in [800u64, 2000u64, 5000u64] {
                let is_selected = self.autosave_debounce_ms == ms;
                debounce_dropdown = debounce_dropdown.child(
                    Self::dropdown_item(
                        gpui::SharedString::from(format!("preferences-autosave-{ms}")),
                        format!("{ms} ms"),
                        is_selected,
                        theme,
                        move |this, _, _, cx| {
                            this.autosave_debounce_ms = ms;
                            this.autosave_dropdown_open = false;
                            cx.notify();
                        },
                        cx,
                    ),
                );
            }
        }

        let external_change_selected = match self.external_change_policy {
            ExternalChangePolicy::Auto => strings.preferences_external_change_auto.clone(),
            ExternalChangePolicy::Manual => strings.preferences_external_change_manual.clone(),
        };
        let mut external_change_dropdown = div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(Self::dropdown_button(
                "preferences-external-change-dropdown",
                external_change_selected,
                theme,
                Self::toggle_external_change_dropdown,
                cx,
            ));
        if self.external_change_dropdown_open {
            for (policy, label) in [
                (
                    ExternalChangePolicy::Auto,
                    strings.preferences_external_change_auto.clone(),
                ),
                (
                    ExternalChangePolicy::Manual,
                    strings.preferences_external_change_manual.clone(),
                ),
            ] {
                let is_selected = self.external_change_policy == policy;
                external_change_dropdown = external_change_dropdown.child(Self::dropdown_item(
                    gpui::SharedString::from(format!(
                        "preferences-external-change-{}",
                        policy.as_str()
                    )),
                    label,
                    is_selected,
                    theme,
                    move |this, _, _, cx| {
                        this.external_change_policy = policy;
                        this.external_change_dropdown_open = false;
                        cx.notify();
                    },
                    cx,
                ));
            }
        }

        let delete_policy_selected = match self.delete_policy {
            DeletePolicy::Trash => strings.preferences_delete_policy_trash.clone(),
            DeletePolicy::Permanent => strings.preferences_delete_policy_permanent.clone(),
        };
        let mut delete_policy_dropdown = div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(Self::dropdown_button(
                "preferences-delete-policy-dropdown",
                delete_policy_selected,
                theme,
                Self::toggle_delete_policy_dropdown,
                cx,
            ));
        if self.delete_policy_dropdown_open {
            for (policy, label) in [
                (
                    DeletePolicy::Trash,
                    strings.preferences_delete_policy_trash.clone(),
                ),
                (
                    DeletePolicy::Permanent,
                    strings.preferences_delete_policy_permanent.clone(),
                ),
            ] {
                let is_selected = self.delete_policy == policy;
                delete_policy_dropdown = delete_policy_dropdown.child(Self::dropdown_item(
                    gpui::SharedString::from(format!(
                        "preferences-delete-policy-{}",
                        policy.as_str()
                    )),
                    label,
                    is_selected,
                    theme,
                    move |this, _, _, cx| {
                        this.delete_policy = policy;
                        this.delete_policy_dropdown_open = false;
                        cx.notify();
                    },
                    cx,
                ));
            }
        }

        let smart_punctuation_toggle =
            crate::components::switch::Switch::new("preferences-smart-punctuation")
                .checked(self.smart_punctuation)
                .on_click(cx.listener(|this, _event, _window, cx| {
                    this.smart_punctuation = !this.smart_punctuation;
                    cx.notify();
                }));

        self.settings_card(
            theme,
            vec![
                self.settings_row(theme, strings.preferences_startup_option.clone(), dropdown),
                self.settings_row(
                    theme,
                    strings.preferences_file_tree_sort.clone(),
                    tree_sort_dropdown,
                ),
                self.settings_row(
                    theme,
                    strings.preferences_file_autosave_debounce.clone(),
                    debounce_dropdown,
                ),
                self.settings_row(
                    theme,
                    strings.preferences_smart_punctuation.clone(),
                    smart_punctuation_toggle,
                ),
                self.settings_row(
                    theme,
                    strings.preferences_file_external_change.clone(),
                    external_change_dropdown,
                ),
                self.settings_row(
                    theme,
                    strings.preferences_file_delete_policy.clone(),
                    delete_policy_dropdown,
                ),
            ],
        )
    }

    fn render_theme_page(
        &self,
        theme: &Theme,
        strings: &crate::i18n::I18nStrings,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut dropdown = div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(Self::dropdown_button(
                "preferences-theme-dropdown",
                self.selected_theme_name(strings),
                theme,
                Self::toggle_theme_dropdown,
                cx,
            ));
        if self.theme_dropdown_open {
            let mut list = div()
                .id("preferences-theme-dropdown-list")
                .flex()
                .flex_col()
                .gap(px(4.0))
                .max_h(px(240.0))
                .overflow_y_scroll();

            for (index, entry) in self.theme_options.clone().into_iter().enumerate() {
                let selected = entry.id == self.selected_theme_id;
                let name = self.theme_display_name(&entry, strings);
                let preview = cx
                    .global::<ThemeManager>()
                    .preview_colors(&entry.id)
                    .unwrap_or((
                        theme.colors.editor_background,
                        theme.colors.text_default,
                        theme.colors.text_link,
                    ));
                list = list.child(Self::theme_dropdown_item(
                    index,
                    name,
                    selected,
                    preview,
                    theme,
                    move |this, _, _, cx| {
                        this.selected_theme_id = entry.id.clone();
                        this.theme_dropdown_open = false;
                        cx.notify();
                    },
                    cx,
                ));
            }
            dropdown = dropdown.child(list);
        }
        let chinese = cx.global::<I18nManager>().current_language_id() == "zh-CN";
        let width_label = |width| match (chinese, width) {
            (true, WritingWidthPreference::Theme) => "跟随主题",
            (true, WritingWidthPreference::Compact) => "紧凑 · 640 px",
            (true, WritingWidthPreference::Standard) => "标准 · 760 px",
            (true, WritingWidthPreference::Wide) => "宽敞 · 900 px",
            (false, WritingWidthPreference::Theme) => "Follow Theme",
            (false, WritingWidthPreference::Compact) => "Compact · 640 px",
            (false, WritingWidthPreference::Standard) => "Standard · 760 px",
            (false, WritingWidthPreference::Wide) => "Wide · 900 px",
        };
        let mut writing_width = div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(Self::dropdown_button(
                "preferences-writing-width",
                width_label(self.writing_width).into(),
                theme,
                Self::toggle_writing_width_dropdown,
                cx,
            ));
        if self.writing_width_dropdown_open {
            for (index, width) in [
                WritingWidthPreference::Theme,
                WritingWidthPreference::Compact,
                WritingWidthPreference::Standard,
                WritingWidthPreference::Wide,
            ]
            .into_iter()
            .enumerate()
            {
                writing_width = writing_width.child(Self::dropdown_item(
                    ("preferences-writing-width-option", index),
                    width_label(width).into(),
                    self.writing_width == width,
                    theme,
                    move |this, _, _, cx| {
                        this.writing_width = width;
                        this.writing_width_dropdown_open = false;
                        cx.notify();
                    },
                    cx,
                ));
            }
        }
        let markdown_font_options = [
            "theme",
            ".SystemUIFont",
            "PingFang SC",
            "Noto Sans CJK SC",
            "Georgia",
        ];
        let code_font_options = if cfg!(target_os = "windows") {
            ["Consolas", "Courier New", "Cascadia Code"]
        } else {
            ["Menlo", "Monaco", "SF Mono"]
        };
        let markdown_font_label = match self.fonts.markdown_family.as_str() {
            "theme" => "跟随主题".to_string(),
            ".SystemUIFont" => "系统字体".to_string(),
            _ => self.fonts.markdown_family.clone(),
        };
        let mut markdown_font = div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(Self::dropdown_button(
                "preferences-markdown-font",
                markdown_font_label,
                theme,
                Self::toggle_markdown_font_dropdown,
                cx,
            ));
        if self.markdown_font_dropdown_open {
            for (index, family) in markdown_font_options.into_iter().enumerate() {
                markdown_font = markdown_font.child(Self::dropdown_item(
                    ("preferences-markdown-font-option", index),
                    match family {
                        "theme" => "跟随主题".into(),
                        ".SystemUIFont" => "系统字体".into(),
                        _ => family.into(),
                    },
                    self.fonts.markdown_family == family,
                    theme,
                    move |this, _, _, cx| {
                        this.fonts.markdown_family = family.into();
                        this.markdown_font_dropdown_open = false;
                        cx.notify();
                    },
                    cx,
                ));
            }
        }
        let mut code_font = div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(Self::dropdown_button(
                "preferences-code-font",
                self.fonts.code_family.clone(),
                theme,
                Self::toggle_code_font_dropdown,
                cx,
            ));
        if self.code_font_dropdown_open {
            for (index, family) in code_font_options.into_iter().enumerate() {
                code_font = code_font.child(Self::dropdown_item(
                    ("preferences-code-font-option", index),
                    family.into(),
                    self.fonts.code_family == family,
                    theme,
                    move |this, _, _, cx| {
                        this.fonts.code_family = family.into();
                        this.code_font_dropdown_open = false;
                        cx.notify();
                    },
                    cx,
                ));
            }
        }
        // 两张卡片：主题一张，字体与排版一张。页面滚动交给外层容器，
        // 这里不再自己开一个 max_h + overflow 的滚动区（嵌套滚动会互抢滚轮）。
        let theme_rows = vec![self.settings_row(
            theme,
            strings.preferences_local_theme.clone(),
            dropdown,
        )];
        let typography_rows = vec![
            self.settings_row(
                theme,
                if chinese {
                    "写作列宽".to_string()
                } else {
                    "Writing Width".to_string()
                },
                writing_width,
            ),
            self.settings_row(theme, "Markdown 字体".to_string(), markdown_font),
            self.font_size_row("Markdown 字号", self.fonts.markdown_size, true, theme, cx),
            self.settings_row(theme, "代码等宽字体".to_string(), code_font),
            self.font_size_row("代码字号", self.fonts.code_size, false, theme, cx),
        ];
        div()
            .w_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap(px(14.0))
            .child(self.settings_card(theme, theme_rows))
            .child(self.settings_card(theme, typography_rows))
            .into_any_element()
    }

    fn font_size_row(
        &self,
        label: &str,
        size: u16,
        markdown: bool,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = &theme.colors;
        let button = |id: &'static str, sign: &'static str, delta: i16, cx: &mut Context<Self>| {
            div()
                .id(id)
                .w(px(36.0))
                .h(px(32.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(6.0))
                .border_1()
                .border_color(c.dialog_border)
                .bg(c.dialog_secondary_button_bg)
                .hover(|this| this.bg(c.dialog_secondary_button_hover))
                .cursor_pointer()
                .child(sign)
                .on_click(cx.listener(move |this, _, _, cx| {
                    let current = if markdown {
                        &mut this.fonts.markdown_size
                    } else {
                        &mut this.fonts.code_size
                    };
                    *current = ((*current as i16 + delta).clamp(10, 36)) as u16;
                    cx.notify();
                }))
        };
        self.settings_row(
            theme,
            label.to_string(),
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(button(
                    if markdown {
                        "markdown-font-smaller"
                    } else {
                        "code-font-smaller"
                    },
                    "−",
                    -1,
                    cx,
                ))
                .child(div().w(px(52.0)).text_center().child(format!("{size} px")))
                .child(button(
                    if markdown {
                        "markdown-font-larger"
                    } else {
                        "code-font-larger"
                    },
                    "+",
                    1,
                    cx,
                )),
        )
    }

    fn image_paste_behavior_label(
        behavior: ImagePasteBehavior,
        strings: &crate::i18n::I18nStrings,
    ) -> String {
        match behavior {
            ImagePasteBehavior::None => strings.preferences_image_paste_none.clone(),
            ImagePasteBehavior::CopyToDocumentFolder => strings
                .preferences_image_paste_copy_to_document_folder
                .clone(),
            ImagePasteBehavior::CopyToAssetsFolder => strings
                .preferences_image_paste_copy_to_assets_folder
                .clone(),
            ImagePasteBehavior::CopyToNamedAssetsFolder => strings
                .preferences_image_paste_copy_to_named_assets_folder
                .clone(),
        }
    }

    fn render_image_page(
        &self,
        theme: &Theme,
        strings: &crate::i18n::I18nStrings,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let options = [
            ImagePasteBehavior::None,
            ImagePasteBehavior::CopyToDocumentFolder,
            ImagePasteBehavior::CopyToAssetsFolder,
            ImagePasteBehavior::CopyToNamedAssetsFolder,
        ];
        let mut dropdown = div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(Self::dropdown_button(
                "preferences-image-dropdown",
                Self::image_paste_behavior_label(self.image_paste_behavior, strings),
                theme,
                Self::toggle_image_dropdown,
                cx,
            ));
        if self.image_dropdown_open {
            for (index, behavior) in options.into_iter().enumerate() {
                let selected = behavior == self.image_paste_behavior;
                let label = Self::image_paste_behavior_label(behavior, strings);
                dropdown = dropdown.child(Self::dropdown_item(
                    ("preferences-image-option", index),
                    label,
                    selected,
                    theme,
                    move |this, _, _, cx| {
                        this.image_paste_behavior = behavior;
                        this.image_dropdown_open = false;
                        cx.notify();
                    },
                    cx,
                ));
            }
        }
        self.settings_card(
            theme,
            vec![self.settings_row(
                theme,
                strings.preferences_image_insert_behavior.clone(),
                dropdown,
            )],
        )
    }

    fn shortcut_category_label(
        category: ShortcutCategory,
        strings: &crate::i18n::I18nStrings,
    ) -> String {
        match category {
            ShortcutCategory::File => strings.preferences_shortcuts_group_file.clone(),
            ShortcutCategory::Edit => strings.preferences_shortcuts_group_edit.clone(),
            ShortcutCategory::Navigation => strings.preferences_shortcuts_group_navigation.clone(),
            ShortcutCategory::Formatting => strings.preferences_shortcuts_group_formatting.clone(),
            ShortcutCategory::Block => strings.preferences_shortcuts_group_block.clone(),
            ShortcutCategory::Other => strings.preferences_shortcuts_group_other.clone(),
        }
    }

    fn shortcut_command_label(
        command: ShortcutCommand,
        strings: &crate::i18n::I18nStrings,
    ) -> String {
        match command {
            ShortcutCommand::Newline => strings.preferences_shortcut_newline.clone(),
            ShortcutCommand::DeleteBack => strings.preferences_shortcut_delete_back.clone(),
            ShortcutCommand::Delete => strings.preferences_shortcut_delete.clone(),
            ShortcutCommand::WordDeleteBack => {
                strings.preferences_shortcut_word_delete_back.clone()
            }
            ShortcutCommand::WordDeleteForward => {
                strings.preferences_shortcut_word_delete_forward.clone()
            }
            ShortcutCommand::FocusPrev => strings.preferences_shortcut_focus_prev.clone(),
            ShortcutCommand::FocusNext => strings.preferences_shortcut_focus_next.clone(),
            ShortcutCommand::MoveLeft => strings.preferences_shortcut_move_left.clone(),
            ShortcutCommand::MoveRight => strings.preferences_shortcut_move_right.clone(),
            ShortcutCommand::WordMoveLeft => strings.preferences_shortcut_word_move_left.clone(),
            ShortcutCommand::WordMoveRight => strings.preferences_shortcut_word_move_right.clone(),
            ShortcutCommand::Home => strings.preferences_shortcut_home.clone(),
            ShortcutCommand::End => strings.preferences_shortcut_end.clone(),
            ShortcutCommand::BlockUp => strings.preferences_shortcut_block_up.clone(),
            ShortcutCommand::BlockDown => strings.preferences_shortcut_block_down.clone(),
            ShortcutCommand::PageUp => strings.preferences_shortcut_page_up.clone(),
            ShortcutCommand::PageDown => strings.preferences_shortcut_page_down.clone(),
            ShortcutCommand::JumpToTop => strings.preferences_shortcut_jump_to_top.clone(),
            ShortcutCommand::JumpToBottom => strings.preferences_shortcut_jump_to_bottom.clone(),
            ShortcutCommand::SelectLeft => strings.preferences_shortcut_select_left.clone(),
            ShortcutCommand::SelectRight => strings.preferences_shortcut_select_right.clone(),
            ShortcutCommand::WordSelectLeft => {
                strings.preferences_shortcut_word_select_left.clone()
            }
            ShortcutCommand::WordSelectRight => {
                strings.preferences_shortcut_word_select_right.clone()
            }
            ShortcutCommand::SelectHome => strings.preferences_shortcut_select_home.clone(),
            ShortcutCommand::SelectEnd => strings.preferences_shortcut_select_end.clone(),
            ShortcutCommand::SelectAll => strings.preferences_shortcut_select_all.clone(),
            ShortcutCommand::Copy => strings.preferences_shortcut_copy.clone(),
            ShortcutCommand::Cut => strings.preferences_shortcut_cut.clone(),
            ShortcutCommand::Paste => strings.preferences_shortcut_paste.clone(),
            ShortcutCommand::Undo => strings.preferences_shortcut_undo.clone(),
            ShortcutCommand::Redo => strings.preferences_shortcut_redo.clone(),
            ShortcutCommand::BoldSelection => strings.preferences_shortcut_bold_selection.clone(),
            ShortcutCommand::ItalicSelection => {
                strings.preferences_shortcut_italic_selection.clone()
            }
            ShortcutCommand::UnderlineSelection => {
                strings.preferences_shortcut_underline_selection.clone()
            }
            ShortcutCommand::CodeSelection => strings.preferences_shortcut_code_selection.clone(),
            ShortcutCommand::IndentBlock => strings.preferences_shortcut_indent_block.clone(),
            ShortcutCommand::OutdentBlock => strings.preferences_shortcut_outdent_block.clone(),
            ShortcutCommand::ExitCodeBlock => strings.preferences_shortcut_exit_code_block.clone(),
            ShortcutCommand::SaveDocument => strings.preferences_shortcut_save_document.clone(),
            ShortcutCommand::SaveDocumentAs => {
                strings.preferences_shortcut_save_document_as.clone()
            }
            ShortcutCommand::FileHistory => strings.menu_file_history.clone(),
            ShortcutCommand::PrintDocument => strings.menu_print.clone(),
            ShortcutCommand::NewWindow => strings.preferences_shortcut_new_window.clone(),
            ShortcutCommand::OpenFile => strings.preferences_shortcut_open_file.clone(),
            ShortcutCommand::QuitApplication => {
                strings.preferences_shortcut_quit_application.clone()
            }
            ShortcutCommand::CloseWindow => strings.preferences_shortcut_close_window.clone(),
            ShortcutCommand::DismissTransientUi => {
                strings.preferences_shortcut_dismiss_transient_ui.clone()
            }
            ShortcutCommand::ToggleViewMode => {
                strings.preferences_shortcut_toggle_view_mode.clone()
            }
            ShortcutCommand::FindInDocument => {
                strings.preferences_shortcut_find_in_document.clone()
            }
            ShortcutCommand::FindNextMatch => strings.preferences_shortcut_find_next_match.clone(),
            ShortcutCommand::FindPreviousMatch => {
                strings.preferences_shortcut_find_previous_match.clone()
            }
            ShortcutCommand::ToggleSidebar => {
                strings.preferences_shortcut_toggle_sidebar.clone()
            }
            ShortcutCommand::ToggleFullscreen => {
                strings.preferences_shortcut_toggle_fullscreen.clone()
            }
        }
    }

    fn format_template(template: &str, key: &str, value: &str) -> String {
        template.replace(key, value)
    }

    fn begin_recording_shortcut(
        &mut self,
        command: ShortcutCommand,
        _: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.recording_shortcut = Some(command);
        self.shortcut_error = None;
        window.focus(&self.focus_handle);
        cx.notify();
    }

    fn reset_shortcut(
        &mut self,
        command: ShortcutCommand,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(definition) = shortcut_definitions()
            .iter()
            .find(|definition| definition.command == command)
        {
            self.keybindings.remove(definition.id);
        }
        if self.recording_shortcut == Some(command) {
            self.recording_shortcut = None;
        }
        self.shortcut_error = None;
        cx.notify();
    }

    fn capture_shortcut_key(
        &mut self,
        event: &KeyDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(command) = self.recording_shortcut else {
            return;
        };
        cx.stop_propagation();
        if event.is_held {
            return;
        }

        let key = event.keystroke.unparse();
        if key == "escape" {
            self.recording_shortcut = None;
            self.shortcut_error = None;
            cx.notify();
            return;
        }

        let Some(keys) = normalize_shortcut_keys(std::slice::from_ref(&key)) else {
            let strings = cx.global::<I18nManager>().strings();
            self.shortcut_error = Some(Self::format_template(
                &strings.preferences_shortcut_invalid_template,
                "{shortcut}",
                &key,
            ));
            cx.notify();
            return;
        };

        if let Some(conflict) = shortcut_conflict_for(command, &keys, &self.keybindings) {
            let strings = cx.global::<I18nManager>().strings();
            let label = Self::shortcut_command_label(conflict.command, strings);
            self.shortcut_error = Some(Self::format_template(
                &strings.preferences_shortcut_conflict_template,
                "{command}",
                &label,
            ));
            cx.notify();
            return;
        }

        if let Some(definition) = shortcut_definitions()
            .iter()
            .find(|definition| definition.command == command)
        {
            let defaults = definition
                .default_keys
                .iter()
                .map(|key| key.to_string())
                .collect::<Vec<_>>();
            if keys == defaults {
                self.keybindings.remove(definition.id);
            } else {
                self.keybindings.insert(definition.id.to_string(), keys);
            }
        }
        self.recording_shortcut = None;
        self.shortcut_error = None;
        cx.notify();
    }

    fn shortcut_chip(label: &str, theme: &Theme) -> impl IntoElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        div()
            .min_w(px(58.0))
            .h(px(24.0))
            .px(px(8.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px((d.menu_item_radius - 1.0).max(0.0)))
            .border(px(d.dialog_border_width))
            .border_color(c.dialog_border)
            .bg(c.code_bg)
            .text_size(px((t.dialog_body_size - 1.0).max(10.0)))
            .text_color(c.code_text)
            .child(SharedString::from(label.to_string()))
    }

    fn shortcut_action_button(
        id: impl Into<ElementId>,
        label: String,
        theme: &Theme,
        on_click: impl Fn(&mut Self, &ClickEvent, &mut Window, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        div()
            .id(id)
            .h(px(28.0))
            .px(px(10.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px((d.dialog_radius - 5.0).max(0.0)))
            .border(px(d.dialog_border_width))
            .border_color(c.dialog_border)
            .bg(c.dialog_secondary_button_bg)
            .hover(|this| this.bg(c.dialog_secondary_button_hover))
            .cursor_pointer()
            .text_size(px((t.dialog_button_size - 1.0).max(10.0)))
            .font_weight(t.dialog_button_weight.to_font_weight())
            .text_color(c.dialog_secondary_button_text)
            .child(label)
            .on_click(cx.listener(on_click))
    }

    fn render_shortcut_row(
        &self,
        definition: ShortcutDefinition,
        theme: &Theme,
        strings: &crate::i18n::I18nStrings,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let c = &theme.colors;
        let t = &theme.typography;
        let is_recording = self.recording_shortcut == Some(definition.command);
        let keys = resolved_shortcut_keys(&self.keybindings, definition.command);
        let label = Self::shortcut_command_label(definition.command, strings);
        let command = definition.command;

        let mut chips = div().flex().flex_wrap().gap(px(6.0));
        if is_recording {
            chips = chips.child(Self::shortcut_chip(
                &strings.preferences_shortcut_recording,
                theme,
            ));
        } else {
            for key in keys {
                chips = chips.child(Self::shortcut_chip(&key, theme));
            }
        }

        div()
            .w_full()
            .min_h(px(48.0))
            .px(px(14.0))
            .py(px(6.0))
            .flex()
            .items_center()
            .gap(px(12.0))
            .hover(|this| this.bg(c.dialog_secondary_button_hover))
            .child(
                div()
                    .w(px(150.0))
                    .flex_shrink_0()
                    .text_size(px(t.dialog_body_size))
                    .text_color(c.dialog_body)
                    .child(label),
            )
            .child(div().flex_1().min_w(px(0.0)).child(chips))
            .child(
                div()
                    .flex_shrink_0()
                    .flex()
                    .gap(px(6.0))
                    .child(Self::shortcut_action_button(
                        ("preferences-shortcut-record", definition.command as u32),
                        strings.preferences_shortcut_record.clone(),
                        theme,
                        move |this, event, window, cx| {
                            this.begin_recording_shortcut(command, event, window, cx)
                        },
                        cx,
                    ))
                    .child(Self::shortcut_action_button(
                        ("preferences-shortcut-reset", definition.command as u32),
                        strings.preferences_shortcut_reset.clone(),
                        theme,
                        move |this, event, window, cx| {
                            this.reset_shortcut(command, event, window, cx)
                        },
                        cx,
                    )),
            )
    }

    fn render_shortcuts_page(
        &self,
        theme: &Theme,
        strings: &crate::i18n::I18nStrings,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = &theme.colors;
        let t = &theme.typography;

        let categories = [
            ShortcutCategory::File,
            ShortcutCategory::Edit,
            ShortcutCategory::Navigation,
            ShortcutCategory::Formatting,
            ShortcutCategory::Block,
            ShortcutCategory::Other,
        ];

        // 每个分类一张卡片，分类名用弱化的小字放在卡片上方；整页滚动交给外层容器。
        let mut page = div().w_full().flex_shrink_0().flex().flex_col().gap(px(14.0));
        for category in categories {
            let rows = shortcut_definitions()
                .iter()
                .copied()
                .filter(|definition| definition.category == category)
                .map(|definition| {
                    self.render_shortcut_row(definition, theme, strings, cx)
                        .into_any_element()
                })
                .collect::<Vec<_>>();
            if rows.is_empty() {
                continue;
            }
            page = page.child(
                div()
                    .w_full()
                    .flex_shrink_0()
                    .flex()
                    .flex_col()
                    .gap(px(6.0))
                    .child(
                        div()
                            .px(px(2.0))
                            .text_size(px((t.dialog_body_size - 1.0).max(10.0)))
                            .font_weight(t.dialog_button_weight.to_font_weight())
                            .text_color(c.dialog_muted)
                            .child(Self::shortcut_category_label(category, strings)),
                    )
                    .child(self.settings_card(theme, rows)),
            );
        }
        page.into_any_element()
    }

    fn render_window_page(
        &self,
        theme: &Theme,
        strings: &crate::i18n::I18nStrings,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let zoom_selected = format!("{}%", self.zoom_percent);
        let mut zoom_dropdown = div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(Self::dropdown_button(
                "preferences-zoom-dropdown",
                zoom_selected.into(),
                theme,
                Self::toggle_zoom_dropdown,
                cx,
            ));
        if self.zoom_dropdown_open {
            for percent in [80i64, 90, 100, 110, 125, 150] {
                let is_selected = self.zoom_percent == percent;
                zoom_dropdown = zoom_dropdown.child(Self::dropdown_item(
                    gpui::SharedString::from(format!("preferences-zoom-{percent}")),
                    format!("{percent}%"),
                    is_selected,
                    theme,
                    move |this, _, _, cx| {
                        this.zoom_percent = percent;
                        this.zoom_dropdown_open = false;
                        cx.notify();
                    },
                    cx,
                ));
            }
        }

        let size_selected = format!(
            "{} × {}",
            self.default_window_width, self.default_window_height
        );
        let mut size_dropdown = div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(Self::dropdown_button(
                "preferences-window-size-dropdown",
                size_selected.into(),
                theme,
                Self::toggle_window_size_dropdown,
                cx,
            ));
        if self.window_size_dropdown_open {
            for (width, height) in [(900i64, 600i64), (1080, 720), (1280, 800), (1440, 900)] {
                let is_selected =
                    self.default_window_width == width && self.default_window_height == height;
                size_dropdown = size_dropdown.child(Self::dropdown_item(
                    gpui::SharedString::from(format!("preferences-window-size-{width}x{height}")),
                    format!("{width} × {height}"),
                    is_selected,
                    theme,
                    move |this, _, _, cx| {
                        this.default_window_width = width;
                        this.default_window_height = height;
                        this.window_size_dropdown_open = false;
                        cx.notify();
                    },
                    cx,
                ));
            }
        }

        let open_position_selected = match self.window_open_position {
            WindowOpenPosition::Remember => {
                strings.preferences_window_open_position_remember.clone()
            }
            WindowOpenPosition::Center => strings.preferences_window_open_position_center.clone(),
        };
        let mut open_position_dropdown = div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(Self::dropdown_button(
                "preferences-window-open-position-dropdown",
                open_position_selected,
                theme,
                Self::toggle_window_open_position_dropdown,
                cx,
            ));
        if self.window_open_position_dropdown_open {
            for (position, label) in [
                (
                    WindowOpenPosition::Remember,
                    strings.preferences_window_open_position_remember.clone(),
                ),
                (
                    WindowOpenPosition::Center,
                    strings.preferences_window_open_position_center.clone(),
                ),
            ] {
                let is_selected = self.window_open_position == position;
                open_position_dropdown = open_position_dropdown.child(Self::dropdown_item(
                    gpui::SharedString::from(format!(
                        "preferences-window-open-position-{}",
                        position.as_str()
                    )),
                    label,
                    is_selected,
                    theme,
                    move |this, _, _, cx| {
                        this.window_open_position = position;
                        this.window_open_position_dropdown_open = false;
                        cx.notify();
                    },
                    cx,
                ));
            }
        }

        let remember_toggle = crate::components::switch::Switch::new("preferences-remember-window")
            .checked(self.remember_window_bounds)
            .on_click(cx.listener(|this, _event, _window, cx| {
                this.remember_window_bounds = !this.remember_window_bounds;
                cx.notify();
            }));

        self.settings_card(
            theme,
            vec![
                self.settings_row(theme, strings.preferences_window_zoom.clone(), zoom_dropdown),
                self.settings_row(
                    theme,
                    strings.preferences_window_default_size.clone(),
                    size_dropdown,
                ),
                self.settings_row(
                    theme,
                    strings.preferences_window_open_position.clone(),
                    open_position_dropdown,
                ),
                self.settings_row(
                    theme,
                    strings.preferences_window_remember_bounds.clone(),
                    remember_toggle,
                ),
            ],
        )
    }

    fn render_status_bar_page(
        &self,
        theme: &Theme,
        strings: &crate::i18n::I18nStrings,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // 状态栏开关：五个都在同一张卡片里（侧栏按钮、模式切换之前只存着没入口）。
        let switch = |id: &'static str,
                      checked: bool,
                      on_click: fn(&mut Self, &ClickEvent, &mut Window, &mut Context<Self>),
                      cx: &mut Context<Self>| {
            Switch::new(id).checked(checked).on_click(cx.listener(on_click))
        };

        self.settings_card(
            theme,
            vec![
                self.settings_row(
                    theme,
                    strings.preferences_status_bar_enabled.clone(),
                    switch("preferences-status-bar-enabled", self.status_bar_enabled, |this, _, _, cx| {
                        this.status_bar_enabled = !this.status_bar_enabled;
                        cx.notify();
                    }, cx),
                ),
                self.settings_row(
                    theme,
                    strings.preferences_status_bar_show_word_count.clone(),
                    switch("preferences-status-bar-word-count", self.status_bar_show_word_count, |this, _, _, cx| {
                        this.status_bar_show_word_count = !this.status_bar_show_word_count;
                        cx.notify();
                    }, cx),
                ),
                self.settings_row(
                    theme,
                    strings.preferences_status_bar_show_cursor_position.clone(),
                    switch("preferences-status-bar-cursor-position", self.status_bar_show_cursor_position, |this, _, _, cx| {
                        this.status_bar_show_cursor_position = !this.status_bar_show_cursor_position;
                        cx.notify();
                    }, cx),
                ),
                self.settings_row(
                    theme,
                    strings.preferences_status_bar_show_sidebar_toggle.clone(),
                    switch("preferences-status-bar-sidebar-toggle", self.status_bar_show_sidebar_toggle, |this, _, _, cx| {
                        this.status_bar_show_sidebar_toggle = !this.status_bar_show_sidebar_toggle;
                        cx.notify();
                    }, cx),
                ),
                self.settings_row(
                    theme,
                    strings.preferences_status_bar_show_mode_switch.clone(),
                    switch("preferences-status-bar-mode-switch", self.status_bar_show_mode_switch, |this, _, _, cx| {
                        this.status_bar_show_mode_switch = !this.status_bar_show_mode_switch;
                        cx.notify();
                    }, cx),
                ),
            ],
        )
    }
}

impl Render for PreferencesWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.system_appearance_subscription.is_none() {
            self.system_appearance_subscription = Some(cx.observe_window_appearance(
                window,
                |_preferences, window, cx| {
                    let appearance = window.appearance();
                    cx.update_global::<ThemeManager, _>(|manager, _cx| {
                        manager.set_system_appearance(appearance)
                    });
                    cx.refresh_windows();
                },
            ));
        }
        let theme = cx.global::<ThemeManager>().current().clone();
        let strings = cx.global::<I18nManager>().strings().clone();
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let can_save = self.has_unsaved_changes();
        let window_title =
            SharedString::from(format!("Velora - {}", strings.preferences_window_title));
        window.set_window_title(window_title.as_ref());
        let titlebar_height = custom_titlebar_height(window, d);

        let content = {
            let page_title = match self.nav {
                PreferencesNav::File => strings.preferences_nav_file.clone(),
                PreferencesNav::Theme => strings.preferences_nav_theme.clone(),
                PreferencesNav::Image => strings.preferences_nav_image.clone(),
                PreferencesNav::Shortcuts => strings.preferences_nav_shortcuts.clone(),
                PreferencesNav::StatusBar => strings.preferences_nav_status_bar.clone(),
                PreferencesNav::Window => strings.preferences_nav_window.clone(),
            };

            // 侧边栏：左对齐 + 选中强调条（旧版把标签右对齐地堆在 30% 宽的栏里，
            // 看起来像没有设计）。
            let nav_items: [(&'static str, String, bool, fn(&mut Self, &ClickEvent, &mut Window, &mut Context<Self>)); 6] = [
                (
                    "preferences-nav-file",
                    strings.preferences_nav_file.clone(),
                    self.nav == PreferencesNav::File,
                    Self::set_nav_file,
                ),
                (
                    "preferences-nav-theme",
                    strings.preferences_nav_theme.clone(),
                    self.nav == PreferencesNav::Theme,
                    Self::set_nav_theme,
                ),
                (
                    "preferences-nav-image",
                    strings.preferences_nav_image.clone(),
                    self.nav == PreferencesNav::Image,
                    Self::set_nav_image,
                ),
                (
                    "preferences-nav-shortcuts",
                    strings.preferences_nav_shortcuts.clone(),
                    self.nav == PreferencesNav::Shortcuts,
                    Self::set_nav_shortcuts,
                ),
                (
                    "preferences-nav-status-bar",
                    strings.preferences_nav_status_bar.clone(),
                    self.nav == PreferencesNav::StatusBar,
                    Self::set_nav_status_bar,
                ),
                (
                    "preferences-nav-window",
                    strings.preferences_nav_window.clone(),
                    self.nav == PreferencesNav::Window,
                    Self::set_nav_window,
                ),
            ];
            let mut sidebar = div()
                .w(px(196.0))
                .h_full()
                .flex_shrink_0()
                .px(px(12.0))
                .py(px(16.0))
                .flex()
                .flex_col()
                .gap(px(2.0))
                .border_r(px(d.dialog_border_width))
                .border_color(c.dialog_border);
            for (id, label, selected, handler) in nav_items {
                sidebar = sidebar.child(self.nav_button(id, label, selected, &theme, handler, cx));
            }

            // 页面内容：错误条 + 卡片；内容列 flex_shrink_0、高度按内容算，
            // 这样外层 overflow_y_scroll 才有东西可滚（旧版套了一层 flex_1，
            // 高度被压成视口高度，于是「文件」和「窗口」两页永远滚不动）。
            let mut page_column = div().w_full().flex_shrink_0().flex().flex_col().gap(px(14.0));
            if let Some(error) = self.save_error.clone() {
                page_column = page_column.child(self.error_banner(&theme, error));
            }
            if let Some(error) = self.shortcut_error.clone() {
                page_column = page_column.child(self.error_banner(&theme, error));
            }
            page_column = page_column.child(match self.nav {
                PreferencesNav::File => self.render_startup_page(&theme, &strings, cx),
                PreferencesNav::Theme => self.render_theme_page(&theme, &strings, cx),
                PreferencesNav::Image => self.render_image_page(&theme, &strings, cx),
                PreferencesNav::Shortcuts => self.render_shortcuts_page(&theme, &strings, cx),
                PreferencesNav::StatusBar => self.render_status_bar_page(&theme, &strings, cx),
                PreferencesNav::Window => self.render_window_page(&theme, &strings, cx),
            });

            let footer = div()
                .w_full()
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_end()
                .gap(px(d.dialog_button_gap))
                .child(
                    div()
                        .id("preferences-cancel")
                        .h(px(d.dialog_button_height))
                        .px(px(d.dialog_button_padding_x))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px((d.dialog_radius - 4.0).max(0.0)))
                        .border(px(d.dialog_border_width))
                        .border_color(c.dialog_border)
                        .bg(c.dialog_secondary_button_bg)
                        .hover(|this| this.bg(c.dialog_secondary_button_hover))
                        .cursor_pointer()
                        .text_size(px(t.dialog_button_size))
                        .font_weight(t.dialog_button_weight.to_font_weight())
                        .text_color(c.dialog_secondary_button_text)
                        .child(strings.preferences_cancel.clone())
                        .on_click(cx.listener(Self::cancel)),
                )
                .child(
                    div()
                        .id("preferences-save")
                        .h(px(d.dialog_button_height))
                        .px(px(d.dialog_button_padding_x))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px((d.dialog_radius - 4.0).max(0.0)))
                        .border(px(if can_save { 0.0 } else { d.dialog_border_width }))
                        .border_color(c.dialog_border)
                        .bg(if can_save {
                            c.dialog_primary_button_bg
                        } else {
                            c.dialog_secondary_button_bg
                        })
                        .hover(move |this| {
                            if can_save {
                                this.bg(c.dialog_primary_button_hover)
                            } else {
                                this.bg(c.dialog_secondary_button_bg)
                            }
                        })
                        .when(can_save, |this| this.cursor_pointer())
                        .text_size(px(t.dialog_button_size))
                        .font_weight(t.dialog_button_weight.to_font_weight())
                        .text_color(if can_save {
                            c.dialog_primary_button_text
                        } else {
                            c.dialog_secondary_button_text
                        })
                        .child(strings.preferences_save.clone())
                        .on_click(cx.listener(Self::save)),
                );

            div()
                .size_full()
                .pt(px(titlebar_height))
                .flex()
                .key_context("Preferences")
                .track_focus(&self.focus_handle)
                .on_key_down(cx.listener(Self::capture_shortcut_key))
                .bg(c.editor_background)
                .text_color(c.dialog_body)
                .child(sidebar)
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .h_full()
                        .flex()
                        .justify_center()
                        .child(
                            div()
                                .w_full()
                                .max_w(px(660.0))
                                .h_full()
                                .px(px(24.0))
                                .py(px(18.0))
                                .flex()
                                .flex_col()
                                .gap(px(14.0))
                                .child(self.page_header(page_title, &theme))
                                .child(
                                    div()
                                        .id("preferences-page-scroll")
                                        .debug_selector(|| "preferences-page-scroll".to_string())
                                        .track_scroll(&self.page_scroll)
                                        .w_full()
                                        .flex_1()
                                        .min_h(px(0.0))
                                        .overflow_y_scroll()
                                        .flex()
                                        .flex_col()
                                        .child(page_column),
                                )
                                .child(footer),
                        ),
                )
        };

        let root = div()
            .size_full()
            .relative()
            .bg(c.editor_background)
            .child(content);

        if let Some(titlebar) = render_custom_titlebar(
            "preferences-titlebar",
            window_title,
            None,
            None,
            &theme,
            window,
            cx,
            Self::on_titlebar_close,
        ) {
            root.child(titlebar)
        } else {
            root
        }
    }
}

fn open_preferences_window_with_state(
    cx: &mut App,
    preferences: AppPreferences,
    theme_options: Vec<ThemeCatalogEntry>,
    title: String,
) -> WindowHandle<PreferencesWindow> {
    open_preferences_window_with_size(
        cx,
        preferences,
        theme_options,
        title,
        size(px(880.0), px(620.0)),
    )
}

/// 同 [`open_preferences_window_with_state`]，但窗口尺寸由调用方定（单测用它开一个
/// 矮窗口，验证内容超出视口时真的能滚——测试平台不支持 resize）。
fn open_preferences_window_with_size(
    cx: &mut App,
    preferences: AppPreferences,
    theme_options: Vec<ThemeCatalogEntry>,
    title: String,
    window_size: Size<Pixels>,
) -> WindowHandle<PreferencesWindow> {
    let bounds = Bounds::centered(None, window_size, cx);
    let window_title = SharedString::from(format!("Velora - {title}"));
    let handle = cx
        .open_window(
            velora_window_options(window_title, bounds),
            move |_window, cx| {
                cx.new(move |cx| PreferencesWindow::new(preferences, theme_options, cx))
            },
        )
        .expect("preferences window should open");

    handle
        .update(cx, |preferences, window, _cx| {
            window.activate_window();
            preferences.focus_handle.focus(window);
        })
        .expect("newly opened preferences window should be updateable");

    handle
}

pub(crate) fn open_preferences_window(cx: &mut App) -> WindowHandle<PreferencesWindow> {
    let preferences = match read_app_preferences() {
        Ok(preferences) => preferences,
        Err(err) => {
            eprintln!("failed to read app preferences: {err}");
            AppPreferences::default()
        }
    };
    let theme_options = cx.global::<ThemeManager>().available_themes().to_vec();
    let title = cx
        .global::<I18nManager>()
        .strings()
        .preferences_window_title
        .clone();
    open_preferences_window_with_state(cx, preferences, theme_options, title)
}

#[cfg(test)]
mod tests {
    use super::{
        AppPreferences, DeletePolicy, EditorSettings, ExportThemePreference,
        ExternalChangePolicy, FontPreferences, ImagePasteBehavior, PreferencesNav,
        StartupOpenPreference, StatusBarPreferences, TreeSortPreference, WindowOpenPosition,
        WritingWidthPreference,
        load_or_create_app_preferences_with_dirs_and_locales, open_preferences_window_with_size,
        open_preferences_window_with_state,
        read_app_preferences_with_dirs, save_app_preferences_with_dirs,
        save_preferences_from_window_with_dirs,
    };
    use crate::config::VeloraConfigDirs;
    use crate::i18n::I18nManager;
    use crate::theme::{ThemeCatalogEntry, ThemeManager};
    use gpui::TestAppContext;
    use gpui::px;
    use std::collections::BTreeMap;

    fn init_preferences_test_app(cx: &mut TestAppContext) {
        cx.update(|cx| {
            I18nManager::init_with_language_id(cx, "en-US");
            ThemeManager::init_with_theme_id(cx, "velora-dark");
            crate::components::init(cx);
            EditorSettings::init(cx, true);
        });
    }

    fn default_theme_options() -> Vec<ThemeCatalogEntry> {
        vec![
            ThemeCatalogEntry {
                id: "system".into(),
                name: "System".into(),
            },
            ThemeCatalogEntry {
                id: "velora-dark".into(),
                name: "Velora".into(),
            },
            ThemeCatalogEntry {
                id: "velora-light".into(),
                name: "Velora Light".into(),
            },
        ]
    }

    #[test]
    fn missing_preferences_file_returns_defaults() {
        let root = std::env::temp_dir().join(format!(
            "velora-preferences-missing-{}",
            uuid::Uuid::new_v4()
        ));
        let dirs = VeloraConfigDirs::from_root(&root);
        let preferences =
            read_app_preferences_with_dirs(&dirs).expect("missing preferences should load");
        assert_eq!(preferences, AppPreferences::default());
        assert_eq!(preferences.default_theme_id, "forest");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn migrates_legacy_default_theme_and_image_paste_behavior_once() {
        let root = std::env::temp_dir().join(format!(
            "velora-preferences-migration-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).expect("temp root should exist");
        let dirs = VeloraConfigDirs::from_root(&root);
        std::fs::write(
            dirs.app_config_file(),
            r#"
                [theme]
                default_theme_id = "old-unknown-theme"

                [editor]
                image_paste_behavior = "none"
            "#,
        )
        .expect("legacy preferences should be written");

        let preferences =
            read_app_preferences_with_dirs(&dirs).expect("legacy preferences should migrate");
        assert_eq!(preferences.default_theme_id, "forest");
        assert_eq!(
            preferences.image_paste_behavior,
            ImagePasteBehavior::CopyToAssetsFolder
        );
        let migrated_text =
            std::fs::read_to_string(dirs.app_config_file()).expect("config should be migrated");
        assert!(migrated_text.contains("preferences_version = 3"));
        assert!(migrated_text.contains("default_theme_id = \"forest\""));
        assert!(migrated_text.contains("image_paste_behavior = \"copy_to_assets_folder\""));

        let current_preferences = migrated_text
            .replace(
                "default_theme_id = \"forest\"",
                "default_theme_id = \"velora-dark\"",
            )
            .replace(
                "image_paste_behavior = \"copy_to_assets_folder\"",
                "image_paste_behavior = \"none\"",
            );
        std::fs::write(dirs.app_config_file(), current_preferences)
            .expect("current explicit preferences should be written");
        let preferences = read_app_preferences_with_dirs(&dirs)
            .expect("versioned preferences should load without migration");
        assert_eq!(preferences.default_theme_id, "velora-dark");
        assert_eq!(preferences.image_paste_behavior, ImagePasteBehavior::None);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn version_two_default_system_theme_migrates_to_forest() {
        let root = std::env::temp_dir().join(format!(
            "velora-preferences-theme-default-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).expect("temp root should exist");
        let dirs = VeloraConfigDirs::from_root(&root);
        std::fs::write(
            dirs.app_config_file(),
            r#"
                preferences_version = 2

                [theme]
                default_theme_id = "system"

                [editor]
                markdown_font_family = "PingFang SC"
            "#,
        )
        .expect("v2 preferences should be written");

        let preferences = read_app_preferences_with_dirs(&dirs).expect("v2 preferences should load");
        // 「system」是旧默认值，跟着新默认主题走；顺手确认其它设置没被这次迁移碰掉。
        assert_eq!(preferences.default_theme_id, "forest");
        assert_eq!(preferences.fonts.markdown_family, "PingFang SC");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn explicitly_chosen_theme_is_not_overwritten_by_default_theme() {
        let root = std::env::temp_dir().join(format!(
            "velora-preferences-theme-explicit-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).expect("temp root should exist");
        let dirs = VeloraConfigDirs::from_root(&root);
        std::fs::write(
            dirs.app_config_file(),
            r#"
                preferences_version = 2

                [theme]
                default_theme_id = "velora-light"
            "#,
        )
        .expect("v2 preferences should be written");

        let preferences = read_app_preferences_with_dirs(&dirs).expect("v2 preferences should load");
        assert_eq!(preferences.default_theme_id, "velora-light");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn version_one_preferences_follow_theme_font_without_resetting_choices() {
        let value: toml::Value = toml::from_str(
            r#"
                preferences_version = 1

                [theme]
                default_theme_id = "velora-dark"

                [editor]
                image_paste_behavior = "none"
                markdown_font_family = ".SystemUIFont"
            "#,
        )
        .unwrap();
        let (preferences, migrated) = super::load_preferences_from_toml_value(&value, "en-US");
        assert!(migrated);
        assert_eq!(preferences.default_theme_id, "velora-dark");
        assert_eq!(preferences.image_paste_behavior, ImagePasteBehavior::None);
        assert_eq!(preferences.fonts.markdown_family, "theme");
    }

    #[test]
    fn partial_or_invalid_preferences_fall_back_by_field() {
        let root = std::env::temp_dir().join(format!(
            "velora-preferences-partial-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).expect("temp root should exist");
        let dirs = VeloraConfigDirs::from_root(&root);
        std::fs::write(
            dirs.app_config_file(),
            r#"
                [startup]
                open = "not-valid"

                [theme]
                default_theme_id = "velora-light"
            "#,
        )
        .expect("preferences should be written");

        let preferences =
            read_app_preferences_with_dirs(&dirs).expect("partial preferences should load");
        assert_eq!(preferences.startup_open, StartupOpenPreference::NewFile);
        assert_eq!(preferences.default_language_id, "en-US");
        assert_eq!(preferences.default_theme_id, "velora-light");
        assert_eq!(preferences.export_theme, ExportThemePreference::Current);
        assert!(!preferences.smart_punctuation);
        assert_eq!(preferences.external_change_policy, ExternalChangePolicy::Auto);
        assert_eq!(preferences.delete_policy, DeletePolicy::Trash);
        assert_eq!(preferences.writing_width, WritingWidthPreference::Theme);
        assert_eq!(
            preferences.image_paste_behavior,
            ImagePasteBehavior::CopyToAssetsFolder
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn writing_width_presets_keep_theme_default_and_explicit_sizes() {
        assert_eq!(WritingWidthPreference::Theme.max_width(700.0), 700.0);
        assert_eq!(WritingWidthPreference::Compact.max_width(700.0), 640.0);
        assert_eq!(WritingWidthPreference::Standard.max_width(700.0), 760.0);
        assert_eq!(WritingWidthPreference::Wide.max_width(700.0), 900.0);
        assert_eq!(
            WritingWidthPreference::from_str("unknown"),
            WritingWidthPreference::Theme
        );
    }

    #[test]
    fn invalid_image_paste_behavior_falls_back_to_none() {
        let root = std::env::temp_dir().join(format!(
            "velora-preferences-image-invalid-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).expect("temp root should exist");
        let dirs = VeloraConfigDirs::from_root(&root);
        std::fs::write(
            dirs.app_config_file(),
            r#"
                [editor]
                image_paste_behavior = "somewhere-dangerous"
            "#,
        )
        .expect("preferences should be written");

        let preferences = read_app_preferences_with_dirs(&dirs).expect("preferences should load");
        assert_eq!(preferences.image_paste_behavior, ImagePasteBehavior::None);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn damaged_preferences_file_returns_defaults() {
        let root = std::env::temp_dir().join(format!(
            "velora-preferences-damaged-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).expect("temp root should exist");
        let dirs = VeloraConfigDirs::from_root(&root);
        std::fs::write(dirs.app_config_file(), "not = [valid")
            .expect("preferences should be written");

        let preferences =
            read_app_preferences_with_dirs(&dirs).expect("damaged preferences should load");
        assert_eq!(preferences, AppPreferences::default());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn saves_and_reads_preferences() {
        let root = std::env::temp_dir().join(format!(
            "velora-preferences-save-{}",
            uuid::Uuid::new_v4()
        ));
        let dirs = VeloraConfigDirs::from_root(&root);
        let preferences = AppPreferences {
            startup_open: StartupOpenPreference::LastOpenedFile,
            default_language_id: "zh-CN".into(),
            default_theme_id: "velora-light".into(),
            export_theme: ExportThemePreference::Dark,
            show_table_headers: false,
            smart_punctuation: true,
            external_change_policy: ExternalChangePolicy::Manual,
            delete_policy: DeletePolicy::Permanent,
            image_paste_behavior: ImagePasteBehavior::CopyToAssetsFolder,
            fonts: FontPreferences {
                markdown_family: "PingFang SC".into(),
                markdown_size: 18,
                code_family: "Menlo".into(),
                code_size: 13,
            },
            writing_width: WritingWidthPreference::Wide,
            workspace_sidebar_width: 320,
            keybindings: BTreeMap::new(),
            status_bar: StatusBarPreferences::default(),
            autosave_debounce_ms: 800,
            tree_sort: TreeSortPreference::default(),
            new_file_template: String::new(),
            remember_window_bounds: true,
            window_frame: None,
            window_open_position: WindowOpenPosition::Center,
            zoom_percent: 100,
            default_window_width: 1080,
            default_window_height: 720,
        };

        save_app_preferences_with_dirs(&preferences, &dirs)
            .expect("preferences should save to config.toml");
        let loaded = read_app_preferences_with_dirs(&dirs).expect("preferences should read back");
        assert_eq!(loaded, preferences);
        assert!(loaded.smart_punctuation);
        assert_eq!(loaded.external_change_policy, ExternalChangePolicy::Manual);
        assert_eq!(loaded.delete_policy, DeletePolicy::Permanent);
        let text =
            std::fs::read_to_string(dirs.app_config_file()).expect("config.toml should exist");
        assert!(text.contains("external_change_policy = \"manual\""));
        assert!(text.contains("delete_policy = \"permanent\""));

        let text =
            std::fs::read_to_string(dirs.app_config_file()).expect("config.toml should exist");
        assert!(text.contains("remember_bounds = true"));
        assert!(text.contains("open_position = \"center\""));
        assert!(text.contains("open = \"last_opened_file\""));
        assert!(text.contains("default_language_id = \"zh-CN\""));
        assert!(text.contains("default_theme_id = \"velora-light\""));
        assert!(text.contains("show_table_headers = false"));
        assert!(text.contains("markdown_font_family = \"PingFang SC\""));
        assert!(text.contains("code_font_size = 13"));
        assert!(text.contains("writing_width = \"wide\""));
        assert!(text.contains("workspace_sidebar_width = 320"));
        assert!(text.contains("image_paste_behavior = \"copy_to_assets_folder\""));
        // roadmap F3：[export] theme 随其他偏好一起持久化。
        assert!(text.contains("[export]"));
        assert!(text.contains("theme = \"dark\""));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn missing_preferences_file_is_created_with_detected_language() {
        let root = std::env::temp_dir().join(format!(
            "velora-preferences-create-{}",
            uuid::Uuid::new_v4()
        ));
        let dirs = VeloraConfigDirs::from_root(&root);
        let preferences = load_or_create_app_preferences_with_dirs_and_locales(&dirs, ["zh-HK"])
            .expect("preferences should be created");
        assert_eq!(preferences.default_language_id, "zh-CN");
        assert!(dirs.app_config_file().exists());
        let text =
            std::fs::read_to_string(dirs.app_config_file()).expect("config.toml should exist");
        assert!(text.contains("remember_bounds = true"));
        assert!(text.contains("[language]"));
        assert!(text.contains("default_language_id = \"zh-CN\""));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn legacy_preferences_are_normalized_with_language() {
        let root = std::env::temp_dir().join(format!(
            "velora-preferences-legacy-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).expect("temp root should exist");
        let dirs = VeloraConfigDirs::from_root(&root);
        std::fs::write(
            dirs.app_config_file(),
            r#"
                [startup]
                open = "last_opened_file"

                [theme]
                default_theme_id = "velora-light"
            "#,
        )
        .expect("legacy preferences should be written");

        let preferences = load_or_create_app_preferences_with_dirs_and_locales(&dirs, ["en-GB"])
            .expect("legacy preferences should normalize");
        assert_eq!(
            preferences.startup_open,
            StartupOpenPreference::LastOpenedFile
        );
        assert_eq!(preferences.default_language_id, "en-US");
        assert_eq!(preferences.default_theme_id, "velora-light");
        // 老配置没有 open_position 键时按「记住上次位置」处理。
        assert_eq!(preferences.window_open_position, WindowOpenPosition::Remember);
        let text =
            std::fs::read_to_string(dirs.app_config_file()).expect("config.toml should exist");
        assert!(text.contains("remember_bounds = true"));
        assert!(text.contains("open_position = \"remember\""));
        assert!(text.contains("[language]"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn saving_preferences_window_preserves_language() {
        let root = std::env::temp_dir().join(format!(
            "velora-preferences-window-{}",
            uuid::Uuid::new_v4()
        ));
        let dirs = VeloraConfigDirs::from_root(&root);
        let preferences = AppPreferences {
            startup_open: StartupOpenPreference::NewFile,
            smart_punctuation: false,
            external_change_policy: ExternalChangePolicy::Auto,
            delete_policy: DeletePolicy::Trash,
            default_language_id: "zh-CN".into(),
            default_theme_id: "velora-dark".into(),
            export_theme: ExportThemePreference::Dark,
            show_table_headers: true,
            image_paste_behavior: ImagePasteBehavior::None,
            fonts: FontPreferences::default(),
            writing_width: WritingWidthPreference::Theme,
            workspace_sidebar_width: 258,
            keybindings: BTreeMap::new(),
            status_bar: StatusBarPreferences::default(),
            autosave_debounce_ms: 800,
            tree_sort: TreeSortPreference::default(),
            new_file_template: String::new(),
            remember_window_bounds: true,
            window_frame: None,
            window_open_position: WindowOpenPosition::default(),
            zoom_percent: 100,
            default_window_width: 1080,
            default_window_height: 720,
        };
        save_app_preferences_with_dirs(&preferences, &dirs)
            .expect("preferences should save to config.toml");

        let saved = save_preferences_from_window_with_dirs(
            StartupOpenPreference::LastOpenedFile,
            "velora-light",
            ImagePasteBehavior::CopyToNamedAssetsFolder,
            &FontPreferences::default(),
            WritingWidthPreference::Compact,
            BTreeMap::from([("save_document".to_string(), vec!["ctrl-alt-s".to_string()])]),
            &StatusBarPreferences::default(),
            TreeSortPreference::Name,
            800,
            true,
            WindowOpenPosition::Center,
            false,
            110,
            1280,
            800,
            ExternalChangePolicy::Manual,
            DeletePolicy::Permanent,
            &dirs,
        )
        .expect("window preferences should save");
        assert_eq!(saved.tree_sort, TreeSortPreference::Name);
        assert_eq!(saved.autosave_debounce_ms, 800);
        assert!(saved.remember_window_bounds);
        assert_eq!(saved.zoom_percent, 110);
        assert_eq!(saved.default_window_width, 1280);
        assert_eq!(saved.default_window_height, 800);
        assert_eq!(saved.external_change_policy, ExternalChangePolicy::Manual);
        assert_eq!(saved.delete_policy, DeletePolicy::Permanent);
        assert_eq!(saved.default_language_id, "zh-CN");
        assert_eq!(saved.startup_open, StartupOpenPreference::LastOpenedFile);
        assert_eq!(saved.default_theme_id, "velora-light");
        assert_eq!(saved.writing_width, WritingWidthPreference::Compact);
        assert_eq!(
            saved.image_paste_behavior,
            ImagePasteBehavior::CopyToNamedAssetsFolder
        );
        assert_eq!(
            saved.keybindings.get("save_document"),
            Some(&vec!["ctrl-alt-s".to_string()])
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[gpui::test]
    async fn preferences_pages_render_inside_a_scroll_container(cx: &mut TestAppContext) {
        // 用户报修（Windows）：偏好设置「文件」页内容超出窗口高度时无法滚动。
        // 页面内容必须挂在 overflow_y_scroll 容器里。
        init_preferences_test_app(cx);
        let handle = cx.update(|cx| {
            open_preferences_window_with_state(
                cx,
                AppPreferences::default(),
                default_theme_options(),
                "Preferences".into(),
            )
        });
        cx.run_until_parked();

        let mut preferences_cx = gpui::VisualTestContext::from_window(handle.into(), cx);

        handle
            .update(cx, |preferences, _window, cx| {
                preferences.nav = PreferencesNav::File;
                cx.notify();
            })
            .expect("preferences window should update");
        preferences_cx.run_until_parked();
        assert!(
            preferences_cx.debug_bounds("preferences-page-scroll").is_some(),
            "偏好设置「文件」页应挂在可滚动容器里"
        );

        handle
            .update(cx, |preferences, _window, cx| {
                preferences.nav = PreferencesNav::Shortcuts;
                cx.notify();
            })
            .expect("preferences window should update");
        preferences_cx.run_until_parked();
        assert!(
            preferences_cx.debug_bounds("preferences-page-scroll").is_some(),
            "快捷键页也应挂在滚动容器里"
        );
    }

    #[gpui::test]
    async fn preferences_pages_really_scroll_when_content_overflows(cx: &mut TestAppContext) {
        // 用户报修（两次）：偏好设置「文件」「窗口」两页内容超出窗口高度时滚不动。
        // 旧的测试只验了「挂了一个 overflow 容器」——容器在但高度被上一层的 flex_1
        // 压成视口高度，于是根本滚不动。这里用滚动句柄验 max_offset。
        init_preferences_test_app(cx);
        // 窗口开矮一点：内容必然超出视口（测试平台不支持 resize，只能一开始就开小）。
        let handle = cx.update(|cx| {
            open_preferences_window_with_size(
                cx,
                AppPreferences::default(),
                default_theme_options(),
                "Preferences".into(),
                gpui::size(px(880.0), px(320.0)),
            )
        });
        cx.run_until_parked();

        let mut preferences_cx = gpui::VisualTestContext::from_window(handle.into(), cx);
        preferences_cx.run_until_parked();

        for nav in [PreferencesNav::File, PreferencesNav::Window] {
            handle
                .update(&mut preferences_cx, |preferences, _window, cx| {
                    preferences.nav = nav;
                    cx.notify();
                })
                .expect("preferences window should update");
            preferences_cx.run_until_parked();

            let max_offset = handle
                .update(&mut preferences_cx, |preferences, _window, _cx| {
                    preferences.page_scroll.max_offset()
                })
                .expect("preferences window should update");
            assert!(
                max_offset.height > px(0.0),
                "「{nav:?}」页内容超出窗口时必须能滚，实测 max_offset {max_offset:?}"
            );

            // 侧边栏在最左边（旧版把标签堆在 30% 宽的栏里且右对齐）。
            let nav_bounds = preferences_cx
                .debug_bounds("preferences-nav-file")
                .expect("侧边栏第一项应渲染");
            assert!(
                nav_bounds.origin.x < px(200.0),
                "侧边栏应贴在窗口左侧，实测 x = {:?}",
                nav_bounds.origin.x
            );
        }
    }

    #[gpui::test]
    async fn window_page_exposes_zoom_and_default_size_controls(cx: &mut TestAppContext) {
        // roadmap H1 批次二：偏好设置「窗口」分组页（缩放 + 默认窗口尺寸）；
        // 用户报修补齐：打开位置下拉与「记住窗口位置与大小」开关也放在这一页。
        init_preferences_test_app(cx);
        let handle = cx.update(|cx| {
            open_preferences_window_with_state(
                cx,
                AppPreferences::default(),
                default_theme_options(),
                "Preferences".into(),
            )
        });
        cx.run_until_parked();

        handle
            .update(cx, |preferences, _window, cx| {
                assert_eq!(preferences.zoom_percent, 100);
                assert_eq!(preferences.default_window_width, 1080);
                assert_eq!(preferences.default_window_height, 720);
                assert_eq!(
                    preferences.window_open_position,
                    WindowOpenPosition::Remember
                );

                // 切到窗口页并展开三个下拉（渲染路径由窗口自身的绘制触发）。
                preferences.nav = PreferencesNav::Window;
                preferences.zoom_dropdown_open = true;
                preferences.window_size_dropdown_open = true;
                preferences.window_open_position_dropdown_open = true;
                cx.notify();

                // 改动进入未保存状态并可通过保存路径持久化。
                preferences.zoom_percent = 125;
                preferences.default_window_width = 1280;
                preferences.default_window_height = 800;
                preferences.window_open_position = WindowOpenPosition::Center;
                assert!(preferences.has_unsaved_changes());
            })
            .expect("preferences window should update");
    }

    #[gpui::test]
    async fn preferences_window_activates_and_focuses_on_open(cx: &mut TestAppContext) {
        init_preferences_test_app(cx);

        let handle = cx.update(|cx| {
            open_preferences_window_with_state(
                cx,
                AppPreferences::default(),
                default_theme_options(),
                "Preferences".into(),
            )
        });
        cx.run_until_parked();

        let active_window = cx.update(|cx| cx.active_window().expect("window should be active"));
        assert_eq!(active_window.window_id(), handle.window_id());
        assert!(
            handle
                .update(cx, |preferences, window, _cx| preferences
                    .focus_handle
                    .is_focused(window))
                .expect("preferences window should be updateable")
        );
        assert!(
            !handle
                .update(cx, |preferences, _window, _cx| preferences
                    .has_unsaved_changes())
                .expect("preferences window should be updateable")
        );
    }

    #[gpui::test]
    async fn preferences_dirty_state_tracks_draft_changes(cx: &mut TestAppContext) {
        init_preferences_test_app(cx);

        let handle = cx.update(|cx| {
            open_preferences_window_with_state(
                cx,
                AppPreferences::default(),
                default_theme_options(),
                "Preferences".into(),
            )
        });
        cx.run_until_parked();

        handle
            .update(cx, |preferences, _window, _cx| {
                assert!(!preferences.has_unsaved_changes());
                preferences.startup_open = StartupOpenPreference::LastOpenedFile;
                assert!(preferences.has_unsaved_changes());
                preferences.startup_open = StartupOpenPreference::NewFile;
                assert!(!preferences.has_unsaved_changes());

                preferences.image_paste_behavior = ImagePasteBehavior::CopyToDocumentFolder;
                assert!(preferences.has_unsaved_changes());
                preferences.image_paste_behavior = ImagePasteBehavior::CopyToAssetsFolder;
                assert!(!preferences.has_unsaved_changes());

                preferences
                    .keybindings
                    .insert("save_document".into(), vec!["ctrl-alt-s".into()]);
                assert!(preferences.has_unsaved_changes());
            })
            .expect("preferences window should be updateable");
    }

    #[gpui::test]
    async fn applying_saved_preferences_keeps_window_open_and_focused(cx: &mut TestAppContext) {
        init_preferences_test_app(cx);

        let handle = cx.update(|cx| {
            open_preferences_window_with_state(
                cx,
                AppPreferences::default(),
                default_theme_options(),
                "Preferences".into(),
            )
        });
        cx.run_until_parked();

        handle
            .update(cx, |preferences, window, cx| {
                preferences.startup_open = StartupOpenPreference::LastOpenedFile;
                assert!(preferences.has_unsaved_changes());
                let saved = AppPreferences {
                    startup_open: StartupOpenPreference::LastOpenedFile,
                    ..AppPreferences::default()
                };
                preferences.apply_saved_preferences(saved, window, cx);
            })
            .expect("preferences window should be updateable");
        cx.run_until_parked();

        assert_eq!(cx.update(|cx| cx.windows().len()), 1);
        let active_window = cx.update(|cx| cx.active_window().expect("window should be active"));
        assert_eq!(active_window.window_id(), handle.window_id());
        assert!(
            handle
                .update(cx, |preferences, window, _cx| preferences
                    .focus_handle
                    .is_focused(window))
                .expect("preferences window should remain updateable")
        );
        assert!(
            !handle
                .update(cx, |preferences, _window, _cx| preferences
                    .has_unsaved_changes())
                .expect("preferences window should remain updateable")
        );
    }
}
