//! Document export helpers for HTML, PNG, and PDF output.
//!
//! Export starts from the same Markdown text used by document saving. The
//! module owns format-specific rendering so editor code only chooses paths and
//! supplies the current theme.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, anyhow};
use uuid::Uuid;

use crate::config::{ExportThemePreference, export_theme_preference};
use crate::theme::Theme;

pub(crate) mod html;
mod pdf;
mod png;
pub(crate) mod print;

/// Export target selected from the app menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ExportFormat {
    /// Full HTML document with embedded theme CSS.
    Html,
    /// PDF bytes rendered from the themed HTML document.
    Pdf,
    /// Single long PNG image of the whole document（roadmap F5）。
    Png,
}

impl ExportFormat {
    /// File extension used for save-dialog defaults.
    pub(crate) fn extension(self) -> &'static str {
        match self {
            Self::Html => "html",
            Self::Pdf => "pdf",
            Self::Png => "png",
        }
    }
}

/// 临时导出 HTML 文件；Drop 时清理，保证浏览器渲染路径不残留临时文件。
pub(super) struct TempHtmlFile {
    path: PathBuf,
}

impl TempHtmlFile {
    pub(super) fn create(html: &str) -> anyhow::Result<Self> {
        let path = unique_temp_path("velora-export").with_extension("html");
        fs::write(&path, html)
            .with_context(|| format!("failed to write temporary HTML '{}'", path.display()))?;
        Ok(Self { path })
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempHtmlFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// 生成不与其它导出任务冲突的临时路径。
pub(super) fn unique_temp_path(prefix: &str) -> PathBuf {
    std::env::temp_dir().join(format!("{prefix}-{}", Uuid::new_v4()))
}

/// 把本地路径转成 Chromium 可打开的 `file://` URL。
pub(super) fn file_url_from_path(path: &Path) -> anyhow::Result<url::Url> {
    url::Url::from_file_path(path)
        .map_err(|_| anyhow!("failed to convert '{}' to a file URL", path.display()))
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

/// 把整篇文档渲染为单张 PNG 长图（roadmap F5）。
pub(crate) fn render_png(
    markdown: &str,
    theme: &Theme,
    title: &str,
    base_path: Option<&Path>,
) -> anyhow::Result<Vec<u8>> {
    let theme = resolve_export_theme(theme);
    png::render_png(markdown, &theme, title, base_path)
}
