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

/// 导出落盘前把目标扩展名收口到格式本身：`notes.txt` 导出 HTML 时存成
/// `notes.txt.html`，而不是让 HTML 字节顶着 txt 扩展名。
pub(crate) fn ensure_export_extension(path: PathBuf, format: ExportFormat) -> PathBuf {
    let already_target = path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case(format.extension()));
    if already_target {
        return path;
    }
    // 追加而非替换：`report.v2` 的 `.v2` 是文件名的一部分，不是待覆盖的扩展名。
    match path.file_name() {
        Some(name) => {
            let mut name = name.to_os_string();
            name.push(".");
            name.push(format.extension());
            path.with_file_name(name)
        }
        None => path,
    }
}

#[cfg(test)]
mod export_path_tests {
    use super::{ExportFormat, ensure_export_extension};
    use std::path::PathBuf;

    fn normalized(name: &str, format: ExportFormat) -> String {
        ensure_export_extension(PathBuf::from(name), format)
            .to_string_lossy()
            .into_owned()
    }

    #[test]
    fn export_paths_keep_the_target_extension() {
        assert_eq!(normalized("notes", ExportFormat::Html), "notes.html");
        assert_eq!(normalized("notes.html", ExportFormat::Html), "notes.html");
        assert_eq!(normalized("notes.HTML", ExportFormat::Html), "notes.HTML");
        assert_eq!(normalized("notes.pdf", ExportFormat::Pdf), "notes.pdf");
    }

    #[test]
    fn foreign_extensions_are_appended_not_replaced() {
        // 文件名里的点不一定是扩展名：`report.v2`、`2026.10` 原样保留再追加。
        assert_eq!(
            normalized("notes.txt", ExportFormat::Html),
            "notes.txt.html"
        );
        assert_eq!(normalized("report.v2", ExportFormat::Png), "report.v2.png");
        assert_eq!(normalized("2026.10", ExportFormat::Pdf), "2026.10.pdf");
        assert_eq!(
            normalized("notes.html", ExportFormat::Pdf),
            "notes.html.pdf"
        );
    }
}
