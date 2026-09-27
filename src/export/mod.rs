//! Document export helpers for HTML and PDF output.
//!
//! Export starts from the same Markdown text used by document saving. The
//! module owns format-specific rendering so editor code only chooses paths and
//! supplies the current theme.

use std::path::Path;

use crate::config::{ExportThemePreference, export_theme_preference};
use crate::theme::Theme;

pub(crate) mod html;
mod pdf;
pub(crate) mod print;

/// Export target selected from the app menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ExportFormat {
    /// Full HTML document with embedded theme CSS.
    Html,
    /// PDF bytes rendered from the themed HTML document.
    Pdf,
}

impl ExportFormat {
    /// File extension used for save-dialog defaults.
    pub(crate) fn extension(self) -> &'static str {
        match self {
            Self::Html => "html",
            Self::Pdf => "pdf",
        }
    }
}

/// 把导出主题偏好映射为具体主题（roadmap F3）。纯函数，便于单测断言。
pub(crate) fn resolve_export_theme_choice(
    current: &Theme,
    choice: ExportThemePreference,
) -> Theme {
    match choice {
        ExportThemePreference::Current => current.clone(),
        ExportThemePreference::Light => Theme::light_theme(),
        ExportThemePreference::Dark => Theme::default_theme(),
    }
}

/// 按 `[export] theme` 配置解析实际导出主题。
pub(crate) fn resolve_export_theme(current: &Theme) -> Theme {
    resolve_export_theme_choice(current, export_theme_preference())
}

/// 导出 HTML，并应用 `[export] theme` 主题配置。
pub(crate) fn render_html_with_base_dir(
    markdown: &str,
    theme: &Theme,
    title: &str,
    base_dir: Option<&Path>,
) -> String {
    let theme = resolve_export_theme(theme);
    html::render_html_with_base_dir(markdown, &theme, title, base_dir)
}

/// Renders themed PDF bytes for the current document Markdown.
pub(crate) fn render_pdf(
    markdown: &str,
    theme: &Theme,
    title: &str,
    base_path: Option<&Path>,
) -> anyhow::Result<Vec<u8>> {
    let theme = resolve_export_theme(theme);
    pdf::render_pdf(markdown, &theme, title, base_path)
}
