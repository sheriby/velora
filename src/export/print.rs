//! 打印（roadmap F4）：把导出 HTML 渲染为临时 PDF 并交给系统预览/打印。
//!
//! 打印复用导出管线：先把当前文档写成导出 HTML（主线程，包含 F3 主题配置），
//! 再在后台线程注入打印版式、用 Chromium 渲染 PDF，最后交给系统命令打开。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Context as _;
use uuid::Uuid;

/// 打印流程的中间 HTML 路径（主线程写出，后台线程渲染后删除）。
pub(crate) fn print_temp_html_path() -> PathBuf {
    temp_path("html")
}

/// 交给系统预览/打印的临时 PDF 路径。
pub(crate) fn print_temp_pdf_path() -> PathBuf {
    temp_path("pdf")
}

fn temp_path(extension: &str) -> PathBuf {
    std::env::temp_dir().join(format!("velora-print-{}.{extension}", Uuid::new_v4()))
}

/// 打开 PDF 的系统命令：macOS 交给 Preview，其余平台交给默认程序。
#[cfg(target_os = "macos")]
pub(crate) fn print_open_command(pdf_path: &Path) -> Command {
    let mut command = Command::new("open");
    command.arg("-a").arg("Preview").arg(pdf_path);
    command
}

#[cfg(target_os = "linux")]
pub(crate) fn print_open_command(pdf_path: &Path) -> Command {
    let mut command = Command::new("xdg-open");
    command.arg(pdf_path);
    command
}

#[cfg(target_os = "windows")]
pub(crate) fn print_open_command(pdf_path: &Path) -> Command {
    let mut command = Command::new("cmd");
    command.arg("/C").arg("start").arg("").arg(pdf_path);
    command
}

/// 调用系统命令打开生成的 PDF。
pub(crate) fn open_pdf_in_system_viewer(pdf_path: &Path) -> anyhow::Result<()> {
    run_open_command(print_open_command(pdf_path))
}

fn run_open_command(mut command: Command) -> anyhow::Result<()> {
    let program = command.get_program().to_string_lossy().into_owned();
    let status = command
        .status()
        .with_context(|| format!("failed to run '{program}'"))?;
    if !status.success() {
        anyhow::bail!("'{program}' failed with {status}");
    }
    Ok(())
}

/// 读取导出 HTML、渲染为 PDF 写入临时文件，并交给系统打印/预览。
///
/// 返回生成的 PDF 路径；中间 HTML 文件在读取后删除。
pub(crate) fn print_pdf_from_export_html(html_path: &Path) -> anyhow::Result<PathBuf> {
    let html = fs::read_to_string(html_path)
        .with_context(|| format!("failed to read '{}' for printing", html_path.display()))?;
    let _ = fs::remove_file(html_path);

    let pdf = crate::export::pdf::render_pdf_from_html(&crate::export::html::prepare_print_html(
        &html,
    ))?;
    let pdf_path = print_temp_pdf_path();
    fs::write(&pdf_path, pdf).with_context(|| format!("failed to write '{}'", pdf_path.display()))?;
    open_pdf_in_system_viewer(&pdf_path)?;
    Ok(pdf_path)
}

#[cfg(test)]
mod tests {
    use super::{
        print_open_command, print_temp_html_path, print_temp_pdf_path, run_open_command,
    };
    use std::process::Command;

    #[test]
    fn print_temp_paths_are_unique_and_typed() {
        let pdf = print_temp_pdf_path();
        let other = print_temp_pdf_path();

        assert_eq!(pdf.extension().and_then(|ext| ext.to_str()), Some("pdf"));
        assert!(
            pdf.starts_with(std::env::temp_dir()),
            "print PDF should live in the system temp dir"
        );
        assert!(
            pdf.file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("velora-print-"))
        );
        assert_ne!(pdf, other);
        assert_eq!(
            print_temp_html_path()
                .extension()
                .and_then(|ext| ext.to_str()),
            Some("html")
        );
    }

    #[test]
    fn open_command_reports_spawn_failure() {
        let error = run_open_command(Command::new("velora-nonexistent-viewer"))
            .expect_err("missing command should fail");

        assert!(
            error.to_string().contains("velora-nonexistent-viewer"),
            "unexpected error: {error}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn open_command_reports_non_zero_exit() {
        let error = run_open_command(Command::new("false")).expect_err("false must fail");

        assert!(error.to_string().contains("failed"), "unexpected error: {error}");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn print_open_command_uses_preview_on_macos() {
        let path = print_temp_pdf_path();
        let command = print_open_command(&path);
        let args = command
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect::<Vec<_>>();

        assert_eq!(command.get_program().to_string_lossy(), "open");
        assert_eq!(
            args,
            vec![
                "-a".to_string(),
                "Preview".to_string(),
                path.display().to_string()
            ]
        );
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn print_open_command_uses_platform_default_viewer() {
        let command = print_open_command(&print_temp_pdf_path());

        #[cfg(target_os = "linux")]
        assert_eq!(command.get_program().to_string_lossy(), "xdg-open");
        #[cfg(target_os = "windows")]
        assert_eq!(command.get_program().to_string_lossy(), "cmd");
    }
}
