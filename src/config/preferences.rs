//! Persistent app preferences and the preferences window.

pub(super) use std::collections::BTreeMap;
pub(super) use std::path::PathBuf;

pub(super) use anyhow::Context as _;
pub(super) use gpui::prelude::FluentBuilder;
pub(super) use gpui::*;
pub(super) use serde::{Deserialize, Serialize};

pub(super) use super::{VeloraConfigDirs, read_recent_files};
pub(super) use crate::components::{
    ShortcutCategory, ShortcutCommand, ShortcutDefinition, install_keybindings,
    normalize_shortcut_config, normalize_shortcut_keys, resolved_shortcut_keys,
    shortcut_conflict_for, shortcut_definitions, switch::Switch,
};
pub(super) use crate::i18n::{I18nManager, language_id_for_locale_preferences};
pub(super) use crate::theme::{Theme, ThemeCatalogEntry, ThemeManager};
pub(super) use crate::window_chrome::{custom_titlebar_height, render_custom_titlebar, velora_window_options};

const DEFAULT_THEME_ID: &str = "forest";
const DEFAULT_LANGUAGE_ID: &str = "en-US";
const PREFERENCES_VERSION: i64 = 3;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FontPreferences {
    pub(crate) ui_family: String,
    pub(crate) ui_size: u16,
    pub(crate) markdown_family: String,
    pub(crate) markdown_size: u16,
    pub(crate) code_family: String,
    pub(crate) code_size: u16,
}

impl Default for FontPreferences {
    fn default() -> Self {
        Self {
            ui_family: ".SystemUIFont".into(),
            ui_size: 14,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FontRole {
    Ui,
    Body,
    Code,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum WritingWidthPreference {
    /// 主题自己那条上限说了算。
    Theme,
    /// 默认档：按窗口比例，窄窗用 px 兜底。
    #[default]
    Standard,
    Compact,
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
            "theme" => Self::Theme,
            "compact" => Self::Compact,
            "wide" => Self::Wide,
            // 认不出来的值（存量文件被手改、以后删档）落到默认档。
            _ => Self::default(),
        }
    }

    /// 正文列宽。`available_width` 是视口减掉两侧编辑器内边距的可用宽，
    /// `theme_centered_width` 是主题那套居中宽度（`Editor::centered_column_width`）。
    ///
    /// 定值在高分屏上只占窗口一小截：3840 宽的屏幕上 760px 不到两成（用户报修）。
    /// 除「跟随主题」外都改成占可用宽度的比例，px 退成下限——窗口窄到比例算出来
    /// 比下限还小时取下限，与定值时代的表现一致；窗口再宽就按比例长。
    pub(crate) fn column_width(
        self,
        available_width: f32,
        theme_centered_width: f32,
        theme_max_width: f32,
    ) -> f32 {
        let available = available_width.max(1.0);
        let (ratio, floor) = match self {
            Self::Theme => return theme_centered_width.min(theme_max_width).max(1.0),
            Self::Compact => (0.50, 640.0),
            Self::Standard => (0.62, 760.0),
            Self::Wide => (0.75, 900.0),
        };
        (available * ratio).max(floor).min(available)
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

/// 启动时侧边栏开关（用户要求：默认跟随上次，可选钉死）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum SidebarOpenPreference {
    /// 上次关着就关着。
    #[default]
    FollowLast,
    Always,
    Never,
}

impl SidebarOpenPreference {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::FollowLast => "follow_last",
            Self::Always => "always",
            Self::Never => "never",
        }
    }

    fn from_str(value: &str) -> Self {
        match value {
            "always" => Self::Always,
            "never" => Self::Never,
            _ => Self::FollowLast,
        }
    }
}

/// 启动时侧边栏显示哪个面板（默认跟随上次）。搜索不参与记忆，所以不在选项里。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum SidebarPanelPreference {
    #[default]
    FollowLast,
    Files,
    Outline,
}

impl SidebarPanelPreference {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::FollowLast => "follow_last",
            Self::Files => "files",
            Self::Outline => "outline",
        }
    }

    fn from_str(value: &str) -> Self {
        match value {
            "files" => Self::Files,
            "outline" => Self::Outline,
            _ => Self::FollowLast,
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct UpdatePreferences {
    pub(crate) check_on_startup: bool,
    pub(crate) include_prereleases: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub(crate) ignored_version: String,
}
impl Default for UpdatePreferences {
    fn default() -> Self { Self { check_on_startup: true, include_prereleases: false, ignored_version: String::new() } }
}

/// User preferences persisted under the app config directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AppPreferences {
    pub(crate) updates: UpdatePreferences,
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
    /// 自动落盘开关（默认开）。关掉后编辑内容只写恢复快照，真文件只有
    /// ⌘S 与关闭时保存才动；外部改动的检测不受它影响。
    pub(crate) autosave: bool,
    /// File tree ordering: "name" | "mtime" | "type".
    pub(crate) tree_sort: TreeSortPreference,
    /// 启动时侧边栏开关与面板：默认都跟随上次（见会话里的记忆）。
    pub(crate) sidebar_open: SidebarOpenPreference,
    pub(crate) sidebar_panel: SidebarPanelPreference,
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
            updates: UpdatePreferences::default(),
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
            writing_width: WritingWidthPreference::default(),
            workspace_sidebar_width: 258,
            keybindings: BTreeMap::new(),
            status_bar: StatusBarPreferences::default(),
            autosave_debounce_ms: 800,
            autosave: true,
            tree_sort: TreeSortPreference::default(),
            sidebar_open: SidebarOpenPreference::default(),
            sidebar_panel: SidebarPanelPreference::default(),
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
    updates: UpdatePreferences,
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
    autosave: bool,
    tree_sort: TreeSortPreference,
    sidebar_open: SidebarOpenPreference,
    sidebar_panel: SidebarPanelPreference,
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
        let autosave = cx
            .try_global::<Self>()
            .map(|settings| settings.autosave)
            .or_else(|| {
                read_app_preferences()
                    .ok()
                    .map(|preferences| preferences.autosave)
            })
            .unwrap_or(true);
        let tree_sort = cx
            .try_global::<Self>()
            .map(|settings| settings.tree_sort)
            .or_else(|| {
                read_app_preferences()
                    .ok()
                    .map(|preferences| preferences.tree_sort)
            })
            .unwrap_or_default();
        let sidebar_open = cx
            .try_global::<Self>()
            .map(|settings| settings.sidebar_open)
            .or_else(|| {
                read_app_preferences()
                    .ok()
                    .map(|preferences| preferences.sidebar_open)
            })
            .unwrap_or_default();
        let sidebar_panel = cx
            .try_global::<Self>()
            .map(|settings| settings.sidebar_panel)
            .or_else(|| {
                read_app_preferences()
                    .ok()
                    .map(|preferences| preferences.sidebar_panel)
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
        let updates = cx.try_global::<Self>().map(|settings| settings.updates.clone())
            .or_else(|| read_app_preferences().ok().map(|preferences| preferences.updates)).unwrap_or_default();
        cx.set_global(Self {
            updates,
            show_table_headers,
            smart_punctuation,
            external_change_policy,
            delete_policy,
            fonts,
            writing_width,
            workspace_sidebar_width,
            zoom_percent,
            autosave_debounce_ms,
            autosave,
            tree_sort,
            sidebar_open,
            sidebar_panel,
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

    pub(crate) fn updates(cx: &App) -> UpdatePreferences {
        cx.try_global::<Self>()
            .map(|settings| settings.updates.clone())
            .unwrap_or_default()
    }
    pub(crate) fn set_updates_in_memory(cx: &mut App, updates: UpdatePreferences) {
        if cx.try_global::<Self>().is_none() {
            Self::init(cx, true);
        }
        cx.update_global::<Self, _>(|settings, _| settings.updates = updates);
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

    /// 测试专用：只改内存里的自动保存开关，不碰 config.toml（用例之间共用
    /// 一份配置目录，落盘会互相覆盖）。
    #[cfg(test)]
    pub(crate) fn set_autosave_in_memory(autosave: bool, cx: &mut App) {
        cx.update_global::<Self, _>(|settings, _cx| settings.autosave = autosave);
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

    /// 生效的正文字号与代码字号：用户字号设置 × 界面缩放。
    ///
    /// 需要按用户设置绘制文本的地方都从这里取字号。文档块与编辑器外壳
    /// 曾经各自拼一半（块只套字号、漏掉缩放），界面缩放看起来完全没反应。
    pub(crate) fn scaled_font_sizes(cx: &App) -> (f32, f32) {
        let fonts = Self::fonts(cx);
        let zoom = Self::zoom_percent(cx) as f32 / 100.0;
        (
            (fonts.markdown_size as f32 * zoom).max(1.0),
            (fonts.code_size as f32 * zoom).max(1.0),
        )
    }

    /// 字体偏好的渲染副本：字号已乘界面缩放，字体族不变。
    ///
    /// 表格按字号估算列宽与换行，必须用与绘制一致的字号，否则缩放后列宽对不上。
    pub(crate) fn scaled_fonts(cx: &App) -> FontPreferences {
        let mut fonts = Self::fonts(cx);
        let (text_size, code_size) = Self::scaled_font_sizes(cx);
        fonts.markdown_size = text_size.round().clamp(1.0, u16::MAX as f32) as u16;
        fonts.code_size = code_size.round().clamp(1.0, u16::MAX as f32) as u16;
        fonts
    }

    /// 把「字号设置 + 界面缩放」套到一份渲染用主题排版：正文、代码与各级标题
    /// 同比例缩放，标题层级不变。文档块与编辑器外壳共用这一个派生。
    pub(crate) fn apply_scaled_typography(cx: &App, theme: &mut crate::theme::Theme) {
        let fonts = Self::fonts(cx);
        let zoom = Self::zoom_percent(cx) as f32 / 100.0;
        let t = &mut theme.typography;
        t.text_size = (fonts.markdown_size as f32 * zoom).max(1.0);
        t.code_size = (fonts.code_size as f32 * zoom).max(1.0);
        let heading_scale =
            fonts.markdown_size as f32 / FontPreferences::default().markdown_size as f32 * zoom;
        t.h1_size *= heading_scale;
        t.h2_size *= heading_scale;
        t.h3_size *= heading_scale;
        t.h4_size *= heading_scale;
        t.h5_size *= heading_scale;
        t.h6_size *= heading_scale;
    }

    pub(crate) fn apply_ui_typography(cx: &App, theme: &mut crate::theme::Theme) {
        let ui_size = Self::fonts(cx).ui_size as f32;
        let scale = ui_size / theme.typography.dialog_body_size.max(1.0);
        let typography = &mut theme.typography;
        typography.dialog_body_size = ui_size;
        typography.dialog_title_size *= scale;
        typography.dialog_button_size *= scale;
        let dimensions = &mut theme.dimensions;
        dimensions.menu_text_size *= scale;
        dimensions.menu_bar_height *= scale.max(1.0);
        dimensions.menu_bar_button_height *= scale.max(1.0);
        dimensions.menu_item_height *= scale.max(1.0);
        dimensions.view_mode_toggle_text_size *= scale;
        dimensions.status_bar_text_size *= scale;
        dimensions.status_bar_height *= scale.max(1.0);
        dimensions.dialog_button_height *= scale.max(1.0);
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

    /// 自动落盘开关（默认开）。关掉只是不写真文件——恢复快照与外部改动检测照跑。
    pub(crate) fn autosave(cx: &App) -> bool {
        cx.try_global::<Self>()
            .map(|settings| settings.autosave)
            .unwrap_or(true)
    }

    pub(crate) fn set_autosave(cx: &mut App, autosave: bool) {
        if cx.try_global::<Self>().is_some() {
            cx.update_global::<Self, _>(|settings, _cx| settings.autosave = autosave);
        }
        if let Err(error) = update_app_preferences(|preferences| preferences.autosave = autosave) {
            eprintln!("failed to save autosave switch: {error}");
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

    /// 启动时侧边栏开关（默认跟随上次）。
    pub(crate) fn sidebar_open_on_startup(cx: &App) -> SidebarOpenPreference {
        cx.try_global::<Self>()
            .map(|settings| settings.sidebar_open)
            .unwrap_or_default()
    }

    pub(crate) fn set_sidebar_open_on_startup(cx: &mut App, value: SidebarOpenPreference) {
        if cx.try_global::<Self>().is_some() {
            cx.update_global::<Self, _>(|settings, _cx| settings.sidebar_open = value);
        }
        if let Err(error) =
            update_app_preferences(|preferences| preferences.sidebar_open = value)
        {
            eprintln!("failed to save sidebar open preference: {error}");
        }
    }

    /// 测试用：在内存里钉死两个侧栏启动设置，不碰磁盘。
    #[cfg(test)]
    pub(crate) fn set_sidebar_startup_in_memory(
        cx: &mut App,
        open: SidebarOpenPreference,
        panel: SidebarPanelPreference,
    ) {
        if cx.try_global::<Self>().is_none() {
            Self::init(cx, true);
        }
        cx.update_global::<Self, _>(|settings, _| {
            settings.sidebar_open = open;
            settings.sidebar_panel = panel;
        });
    }

    /// 启动时侧边栏显示哪个面板（默认跟随上次）。
    pub(crate) fn sidebar_panel_on_startup(cx: &App) -> SidebarPanelPreference {
        cx.try_global::<Self>()
            .map(|settings| settings.sidebar_panel)
            .unwrap_or_default()
    }

    pub(crate) fn set_sidebar_panel_on_startup(cx: &mut App, value: SidebarPanelPreference) {
        if cx.try_global::<Self>().is_some() {
            cx.update_global::<Self, _>(|settings, _cx| settings.sidebar_panel = value);
        }
        if let Err(error) =
            update_app_preferences(|preferences| preferences.sidebar_panel = value)
        {
            eprintln!("failed to save sidebar panel preference: {error}");
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


pub(crate) use persistence::*;
pub(crate) use render::open_preferences_window;
#[cfg(test)]
pub(crate) use render::{open_preferences_window_with_size, open_preferences_window_with_state};
pub(crate) use window::{PreferencesNav, PreferencesWindow};

mod pages_general;
mod pages_shortcuts_window;
mod persistence;
mod render;
mod widgets;
mod window;

#[cfg(test)]
mod tests;
