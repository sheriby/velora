//! Native application menu, app-level actions, and window close routing.
//! 应用名称与命令路径为 velora，更新入口默认隐藏。
//!
//! This module owns menu construction and the actions that operate on the
//! active editor window. The Quit action is routed to the current window so the
//! existing unsaved-changes dialog remains authoritative for that window.

use std::path::{Path, PathBuf};

use anyhow::Context as _;
use futures::channel::oneshot;
use gpui::*;

use crate::commands::{CommandMenu, CommandSpec};
use crate::components::{
    AddLanguageConfig, AddThemeConfig, CheckForUpdates, CloseWindow, ExportHtml, ExportPdf,
    ExportPng, FindInDocument, FindNextMatch, FindPreviousMatch, InstallCliTool, NoRecentFiles,
    NewWindow, OpenCommandPalette, OpenFile, OpenPreferences, OpenRecentFile, PrintDocument,
    QuitApplication, SaveDocument,
    SaveDocumentAs,
    SelectLanguage, SelectTheme, ShowAbout, CopyAsHtml, ToggleFocusMode, ToggleFullscreen,
    ToggleSidebar, ToggleTypewriterMode, ToggleViewMode, UninstallCliTool, ZoomIn, ZoomOut,
    ZoomReset,
};
use crate::config::{
    RecoverySnapshot, apply_configured_language, apply_configured_theme,
    import_language_config_and_select, import_theme_config_and_select, open_preferences_window,
    read_recent_folders, read_session, record_recent_file,
    remove_recent_file,
};
use crate::editor::{Editor, InfoDialogKind};
use crate::export::ExportFormat;
use crate::i18n::{I18nManager, I18nStrings};
use crate::theme::ThemeManager;
use crate::window_chrome::velora_window_options_on_display;

/// Global app-menu state for platform menu lifecycle hooks.
#[derive(Default)]
pub(crate) struct AppMenuState {
    window_closed_subscription: Option<Subscription>,
}

impl Global for AppMenuState {}

fn window_title(file_path: Option<&Path>) -> SharedString {
    if let Some(path) = file_path {
        // OsStr::to_string_lossy returns Cow<str>; calling .to_string() on
        // it always allocates a fresh String, even for the valid-UTF-8 path
        // (the common case). Borrow the Cow directly into format! — its
        // Display impl writes the borrowed bytes straight into the output
        // String, no intermediate allocation.
        format!(
            "Velora - {}",
            path.file_name()
                .map(|name| name.to_string_lossy())
                .unwrap_or_else(|| path.to_string_lossy())
        )
        .into()
    } else {
        SharedString::new("Velora")
    }
}

/// 记住的窗口位置/大小和它所在的显示器。显示器一并以 `display_id` 交给平台：
/// gpui 的 Windows 后端只在 frame 中心点落在目标显示器上时才采用它，否则整块
/// 换成显示器默认 bounds（位置和大小一起丢）。
struct RestoredWindow {
    bounds: Bounds<Pixels>,
    display_id: Option<DisplayId>,
}

/// Opens an editor window for the given Markdown content and optional path.
/// Restores the last window frame when remembering is enabled, clamped so
/// the window stays reachable on the current displays; otherwise centers the
/// default size.
fn restored_window_bounds(cx: &mut App) -> RestoredWindow {
    let (default_w, default_h) = crate::config::EditorSettings::default_window_size(cx);
    let default_size = size(px(default_w as f32), px(default_h as f32));
    let centered = RestoredWindow {
        bounds: Bounds::centered(None, default_size, cx),
        display_id: None,
    };
    // 「打开位置 = 居中打开」时忽略记住的 frame，按默认窗口尺寸居中，
    // 让设置里的尺寸选项真正生效（用户报修：缺窗口位置/大小设置）。
    if crate::config::EditorSettings::window_open_position(cx)
        == crate::config::WindowOpenPosition::Center
    {
        return centered;
    }
    let Some(frame) = crate::config::saved_window_frame().ok().flatten() else {
        return centered;
    };
    let mut bounds = Bounds::new(
        point(px(frame.x as f32), px(frame.y as f32)),
        size(
            px(frame.width as f32).max(px(480.0)),
            px(frame.height as f32).max(px(320.0)),
        ),
    );
    let center = bounds.center();
    // 记忆的 frame 还在某块屏上：原样恢复，并把该屏交给平台。
    if let Some(display) = cx
        .displays()
        .into_iter()
        .find(|display| display.bounds().contains(&center))
    {
        return RestoredWindow {
            bounds,
            display_id: Some(display.id()),
        };
    }
    // 记忆的 frame 不在任何显示器上（副屏拔掉、分辨率变小）：挪回主屏，能整块
    // 放下就整块放下，放不下时至少保证中心点留在屏内（平台按中心点判断可用性）。
    // 尺寸照旧，否则平台会把大小一起换成默认值——「永远打开成默认大小」。
    let Some(display) = cx.primary_display() else {
        return RestoredWindow {
            bounds,
            display_id: None,
        };
    };
    let screen = display.bounds();
    // 能整块放下时就是「整块进屏」，放不下时退化成「中心点留在屏内」。
    let right_edge = f32::from(screen.right()) - f32::from(bounds.size.width);
    let bottom_edge = f32::from(screen.bottom()) - f32::from(bounds.size.height);
    let left = f32::from(screen.left());
    let top = f32::from(screen.top());
    let x = f32::from(bounds.origin.x).clamp(right_edge.min(left), right_edge.max(left));
    let y = f32::from(bounds.origin.y).clamp(bottom_edge.min(top), bottom_edge.max(top));
    bounds.origin = point(px(x), px(y));
    RestoredWindow {
        bounds,
        display_id: Some(display.id()),
    }
}

pub(crate) fn open_editor_window(
    cx: &mut App,
    markdown: String,
    file_path: Option<PathBuf>,
) -> WindowHandle<Editor> {
    let RestoredWindow { bounds, display_id } = restored_window_bounds(cx);
    let title = window_title(file_path.as_deref());
    let handle = cx
        .open_window(
            velora_window_options_on_display(title, bounds, display_id),
            move |_window, cx| cx.new(move |cx| Editor::from_file_source(cx, markdown, file_path)),
        )
        .unwrap();

    handle
        .update(cx, |editor, window, cx| {
            window.activate_window();
            editor.force_install_close_guard(cx, window);
            editor.install_window_frame_recorder(window, cx);
        })
        .expect("newly opened editor window should be updateable");

    handle
}

pub(crate) fn open_recovered_editor_window(cx: &mut App, snapshot: RecoverySnapshot) {
    let recovered_title = cx
        .global::<I18nManager>()
        .strings()
        .recovered_document_title
        .clone();
    let title = format!("Velora - {recovered_title}");
    let RestoredWindow { bounds, display_id } = restored_window_bounds(cx);
    let handle = cx
        .open_window(
            velora_window_options_on_display(title.into(), bounds, display_id),
            move |_window, cx| cx.new(move |cx| Editor::from_recovery(cx, snapshot)),
        )
        .unwrap();
    handle
        .update(cx, |editor, window, cx| {
            window.activate_window();
            editor.force_install_close_guard(cx, window);
            editor.install_window_frame_recorder(window, cx);
        })
        .expect("newly opened recovered document should be updateable");
}

pub(crate) fn open_workspace_window(cx: &mut App, root: PathBuf) -> anyhow::Result<()> {
    let handle = open_editor_window(cx, String::new(), None);
    handle.update(cx, move |editor, _window, cx| {
        editor.set_workspace_root(root, cx);
        editor.show_welcome = true;
    })?;
    Ok(())
}

/// Restores the last session (workspace root + open tabs). Returns `false`
/// when there is nothing to restore so callers can fall back to normal
/// startup (roadmap A4).
pub(crate) fn restore_last_session(cx: &mut App) -> bool {
    let session = match read_session() {
        Ok(session) => session,
        Err(_) => return false,
    };
    let Some(root) = session
        .root
        .as_ref()
        .map(PathBuf::from)
        .filter(|root| root.is_dir())
    else {
        return false;
    };
    let mut tabs: Vec<PathBuf> = session
        .tabs
        .iter()
        .map(PathBuf::from)
        .filter(|path| path.is_file())
        .collect();
    // Reorder so the previously active tab opens last and ends up focused.
    if let Some(active) = session.active.as_ref().map(PathBuf::from) {
        if active.is_file() {
            tabs.retain(|tab| tab != &active);
            tabs.push(active);
        }
    }

    let handle = open_editor_window(cx, String::new(), None);
    // 活动标签先同步打开（决定窗口内容与焦点），其余标签分散到后续事件循环打开，
    // 避免恢复大量标签阻塞首帧（roadmap G5）。
    let mut deferred: Vec<PathBuf> = Vec::new();
    if let Some(active) = tabs.last().cloned() {
        tabs.pop();
        deferred = tabs;
        let _ = handle.update(cx, |editor, window, cx| {
            editor.show_welcome = false;
            editor.set_workspace_root(root, cx);
            editor.open_workspace_file(active, window, cx);
        });
    } else {
        let _ = handle.update(cx, |editor, _window, cx| {
            editor.show_welcome = true;
            editor.set_workspace_root(root, cx);
        });
    }
    if !deferred.is_empty() {
        cx.spawn(async move |cx| {
            for tab in deferred {
                // 让出一帧，保证首帧先渲染。
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(16))
                    .await;
                let _ = handle.update(cx, |editor, window, cx| {
                    editor.open_workspace_file(tab, window, cx);
                });
            }
        })
        .detach();
    }
    true
}

pub(crate) fn open_file_in_new_window(cx: &mut App, path: &Path) -> anyhow::Result<()> {
    let markdown = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read '{}'", path.display()))?;
    open_editor_window(cx, markdown, Some(path.to_path_buf()));
    record_recent_file_and_refresh(path, cx);
    Ok(())
}

fn record_recent_file_and_refresh(path: &Path, cx: &mut App) {
    if let Err(err) = record_recent_file(path) {
        eprintln!("failed to update recent file history: {err}");
        return;
    }
    install_menus(cx);
    cx.refresh_windows();
}

#[cfg(target_os = "macos")]
/// Check whether `/usr/local/bin/velora` is correctly installed for this app.
///
/// Returns `true` only if the symlink exists **and** resolves (directly or via
/// one level of canonicalization) to the currently running executable.
fn is_cli_symlink_current_app() -> bool {
    let link = std::path::Path::new("/usr/local/bin/velora");
    let Ok(target) = std::fs::read_link(link) else {
        return false; // does not exist or not a symlink
    };
    let resolved = if target.is_absolute() {
        // Canonicalize the target itself (may fail if dangling).
        std::fs::canonicalize(&target).unwrap_or(target)
    } else {
        // Relative — resolve from symlink's parent directory.
        link.parent()
            .unwrap_or(std::path::Path::new("/"))
            .join(&target)
            .canonicalize()
            .unwrap_or(target)
    };
    match std::env::current_exe() {
        Ok(exe) => resolved == exe,
        Err(_) => false,
    }
}

#[cfg(any(target_os = "macos", test))]
fn applescript_string_literal(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len() + 2);
    escaped.push('"');
    for ch in value.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            _ => escaped.push(ch),
        }
    }
    escaped.push('"');
    escaped
}

#[cfg(target_os = "macos")]
pub(crate) fn install_cli_tool(cx: &mut App) {
    use std::process::Command;

    let bin_link = "/usr/local/bin/velora";

    let current_exe = match std::env::current_exe() {
        Ok(path) => path,
        Err(err) => {
            show_install_cli_error(cx, &format!("Failed to get executable path: {err}"));
            return;
        }
    };

    // Only allow from a portable .app bundle (e.g. drag-installed to /Applications)
    if !current_exe
        .to_string_lossy()
        .contains(".app/Contents/MacOS/")
    {
        show_install_cli_error(
            cx,
            "Command-line tool installation requires running from an .app bundle.\n\n\
             If the app was installed via the `.pkg` installer,\n\
             the CLI command is configured automatically.",
        );
        return;
    }

    let exe_path = applescript_string_literal(&current_exe.to_string_lossy());
    let link_path = applescript_string_literal(bin_link);
    let script = format!(
        r#"set exePath to {exe_path}
set linkPath to {link_path}
do shell script "rm -f " & quoted form of linkPath & linefeed & "ln -s " & quoted form of exePath & space & quoted form of linkPath with administrator privileges"#
    );

    match Command::new("osascript").arg("-e").arg(&script).output() {
        Ok(output) => {
            if output.status.success() {
                let title = "CLI Command Installed";
                let detail = format!(
                    "Successfully installed! You can now use 'velora' from the terminal:\n\n\
                     \x1b[1mvelora README.md\x1b[0m\n\
                     \x1b[1mvelora file1.md file2.md\x1b[0m\n\n\
                     Location: {bin_link}\n\n\
                     Note: If you move or delete velora.app,\n\
                     the 'velora' command will stop working\n\
                     automatically (no cleanup needed)."
                );
                show_message_on_active_editor(cx, title, &detail);
            } else {
                // User pressed Cancel on the admin password dialog
                // or the link creation failed for another reason.
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let detail = if stderr.contains("User canceled") || stderr.contains("(-128)") {
                    "Installation cancelled.".to_string()
                } else {
                    format!("Installation failed: {stderr}")
                };
                show_install_cli_error(cx, &detail);
            }
        }
        Err(err) => {
            show_install_cli_error(cx, &format!("Failed to run installer: {err}"));
        }
    }
    // Refresh menus so the label changes between Install -> Uninstall.
    install_menus(cx);
}

#[cfg(target_os = "macos")]
pub(crate) fn uninstall_cli_tool(cx: &mut App) {
    use std::process::Command;

    let bin_link = "/usr/local/bin/velora";

    if !is_cli_symlink_current_app() {
        show_install_cli_error(cx, "CLI command is not installed for this app.");
        return;
    }

    let link_path = applescript_string_literal(bin_link);
    let script = format!(
        r#"set linkPath to {link_path}
do shell script "rm -f " & quoted form of linkPath with administrator privileges"#
    );

    match Command::new("osascript").arg("-e").arg(&script).output() {
        Ok(output) => {
            if output.status.success() {
                show_message_on_active_editor(
                    cx,
                    "CLI Command Uninstalled",
                    "CLI command has been removed successfully.",
                );
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let detail = if stderr.contains("User canceled") || stderr.contains("(-128)") {
                    "Uninstall cancelled.".to_string()
                } else {
                    format!("Uninstall failed: {stderr}")
                };
                show_install_cli_error(cx, &detail);
            }
        }
        Err(err) => {
            show_install_cli_error(cx, &format!("Failed to run uninstaller: {err}"));
        }
    }
    // Refresh menus so the label changes between Install -> Uninstall.
    install_menus(cx);
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn install_cli_tool(cx: &mut App) {
    show_install_cli_error(
        cx,
        "Command-line tool installation is only available on macOS.",
    );
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn uninstall_cli_tool(cx: &mut App) {
    show_install_cli_error(
        cx,
        "Command-line tool uninstallation is only available on macOS.",
    );
}

fn show_install_cli_error(cx: &mut App, detail: &str) {
    show_message_on_active_editor(cx, "Install Command-Line Tool Failed", detail);
}

pub(crate) fn record_recent_file_from_editor(path: &Path, cx: &mut App) {
    record_recent_file_and_refresh(path, cx);
}

fn show_window_prompt(window: Option<AnyWindowHandle>, title: &str, detail: &str, cx: &mut App) {
    // 应用内模态，不用系统原生弹窗（用户要求）。
    show_message_on_active_editor_in(window, title, detail, cx);
}

/// 在指定（或当前）编辑器窗口里弹应用内模态提示；拿不到编辑器窗口时退回 stderr。
fn show_message_on_active_editor_in(
    window: Option<AnyWindowHandle>,
    title: &str,
    detail: &str,
    cx: &mut App,
) {
    let Some(handle) = window.and_then(|window| window.downcast::<Editor>()) else {
        eprintln!("{title}: {detail}");
        return;
    };
    let title = title.to_string();
    let detail = detail.to_string();
    let _ = handle.update(cx, move |editor, _window, cx| {
        editor.show_message_modal(title.clone(), detail.clone(), cx);
    });
}

fn show_message_on_active_editor(cx: &mut App, title: &str, detail: &str) {
    show_message_on_active_editor_in(cx.active_window(), title, detail, cx);
}

fn with_active_editor<R>(
    cx: &mut App,
    update: impl FnOnce(&mut Editor, &mut Window, &mut Context<Editor>) -> R,
) -> Option<R> {
    let window = cx.active_window()?.downcast::<Editor>()?;
    window.update(cx, update).ok()
}

fn show_info_dialog_on_active_editor(cx: &mut App, kind: InfoDialogKind) {
    let _ = with_active_editor(cx, move |editor, _window, cx| {
        editor.show_info_dialog(kind, cx);
    });
}

fn request_update_check_on_active_editor(cx: &mut App) {
    let _ = with_active_editor(cx, |editor, window, cx| {
        editor.request_check_updates(window, cx);
    });
}

/// roadmap F4 打印：先把当前文档写成临时导出 HTML（含 F3 主题配置），
/// 再在后台线程渲染为临时 PDF 并交给系统预览/打印，避免阻塞 UI。
fn print_document(editor: &Editor, window: &mut Window, cx: &mut Context<Editor>) {
    let html_path = crate::export::print::print_temp_html_path();
    if let Err(err) = editor.export_document_to_path(ExportFormat::Html, &html_path, cx) {
        let title = cx
            .global::<I18nManager>()
            .strings()
            .export_failed_title
            .clone();
        let editor = cx.entity();
        let _ = window;
        let _ = editor.update(cx, |editor, cx| {
            editor.show_message_modal(title, err.to_string(), cx);
        });
        return;
    }

    let editor_entity = cx.entity();
    cx.spawn(async move |_this: WeakEntity<Editor>, cx: &mut AsyncApp| {
        let (sender, receiver) = oneshot::channel();
        let spawn_result = std::thread::Builder::new()
            .name("velora-print".to_string())
            .spawn(move || {
                let result = crate::export::print::print_pdf_from_export_html(&html_path)
                    .map(|path| path.to_string_lossy().into_owned())
                    .map_err(|err| err.to_string());
                let _ = sender.send(result);
            });

        if let Err(err) = spawn_result {
            let detail = format!("failed to start print task: {err}");
            show_export_error(editor_entity.clone(), cx, &detail);
            return;
        }

        let result = receiver
            .await
            .unwrap_or_else(|_| Err("print task stopped before reporting a result".into()));
        if let Err(detail) = result {
            show_export_error(editor_entity.clone(), cx, &detail);
        }
    })
    .detach();
}

/// 打印/导出失败提示走应用内模态（用户要求：全软件不用系统原生弹窗）。
fn show_export_error(editor: Entity<Editor>, cx: &mut AsyncApp, detail: &str) {
    let detail = detail.to_string();
    let _ = editor.update(cx, move |editor, cx| {
        let title = cx
            .global::<I18nManager>()
            .strings()
            .export_failed_title
            .clone();
        editor.show_message_modal(title, detail, cx);
    });
}

fn recent_folders_for_menu() -> Vec<PathBuf> {
    match read_recent_folders() {
        Ok(paths) => paths,
        Err(err) => {
            eprintln!("failed to read recent folder history: {err}");
            Vec::new()
        }
    }
}

/// 「打开最近」的条目 = 最近打开过的工作区（文件夹），不含单个文件。
/// 文件历史仍供其它界面使用，但这个菜单只列工作区（用户要求）。
fn recent_menu_entries(folders: &[PathBuf]) -> Vec<PathBuf> {
    let mut entries: Vec<PathBuf> = Vec::new();
    for path in folders {
        if !entries.contains(path) {
            entries.push(path.clone());
        }
        if entries.len() == 15 {
            break;
        }
    }
    entries
}

/// Top few recent workspaces for the welcome page.
pub(crate) fn welcome_recent_entries() -> Vec<PathBuf> {
    recent_menu_entries(&recent_folders_for_menu())
        .into_iter()
        .take(5)
        .collect()
}

fn open_recent_file(cx: &mut App, path: PathBuf) {
    let error_window = cx.active_window();
    // Re-enter the app context asynchronously before touching window handles.
    // Dispatching directly inside the native-menu action stack hands us stale
    // handles ("window not found"), while the 文件 → 打开文件 flow — which
    // resolves its selection inside an AsyncApp::update — works fine.
    cx.spawn(async move |cx| {
        let _ = cx.update(move |cx| {
            open_recent_file_with_error_window(cx, path, error_window);
        });
    })
    .detach();
}

fn open_recent_file_with_error_window(
    cx: &mut App,
    path: PathBuf,
    error_window: Option<AnyWindowHandle>,
) {
    // Folders route to the working-set flow before the file-existence check,
    // otherwise every folder entry would be reported as "missing file".
    if path.is_dir() {
        open_recent_folder(cx, path, error_window);
        return;
    }
    if !path.is_file() {
        if let Err(err) = remove_recent_file(&path) {
            eprintln!("failed to remove missing recent file: {err}");
        }
        install_menus(cx);
        cx.refresh_windows();
        let strings = cx.global::<I18nManager>().strings().clone();
        let detail = strings
            .recent_file_missing_message_template
            .replace("{path}", &path.to_string_lossy());
        show_window_prompt(
            error_window,
            &strings.recent_file_missing_title,
            &detail,
            cx,
        );
        return;
    }

    // Recent files open in the focused editor window by default — same as
    // 文件 → 打开文件 — so the workspace keeps working in one window. A new
    // window is only spawned when none exists to receive the file.
    if let Some(handle) = editor_window_for_folder_open(cx) {
        let _ = handle.update(cx, |editor, window, cx| {
            editor.open_workspace_file(path, window, cx);
        });
        return;
    }
    if let Err(err) = open_file_in_new_window(cx, &path) {
        let title = cx
            .global::<I18nManager>()
            .strings()
            .open_failed_title
            .clone();
        show_window_prompt(error_window, &title, &err.to_string(), cx);
    }
}

fn open_recent_folder(cx: &mut App, path: PathBuf, error_window: Option<AnyWindowHandle>) {
    // 打开最近 replaces the focused window's working set outright — no
    // confirmation dialog. A fresh window is only spawned when none exists.
    if let Some(handle) = editor_window_for_folder_open(cx) {
        let _ = handle.update(cx, |editor, _window, cx| {
            editor.set_workspace_root(path, cx);
        });
        return;
    }
    if let Err(err) = open_workspace_window(cx, path) {
        let title = cx
            .global::<I18nManager>()
            .strings()
            .open_failed_title
            .clone();
        show_window_prompt(error_window, &title, &err.to_string(), cx);
    }
}

fn is_editor_scoped_menu_action(action: &dyn Action) -> bool {
    action.as_any().is::<SaveDocument>()
        || action.as_any().is::<SaveDocumentAs>()
        || action.as_any().is::<ExportHtml>()
        || action.as_any().is::<ExportPdf>()
        || action.as_any().is::<ExportPng>()
        || action.as_any().is::<PrintDocument>()
        || action.as_any().is::<QuitApplication>()
        || action.as_any().is::<CloseWindow>()
        || action.as_any().is::<CheckForUpdates>()
        || action.as_any().is::<ShowAbout>()
        || action.as_any().is::<InstallCliTool>()
        || action.as_any().is::<UninstallCliTool>()
        || action.as_any().is::<ToggleSidebar>()
        || action.as_any().is::<ToggleFullscreen>()
        || action.as_any().is::<ToggleViewMode>()
        || action.as_any().is::<ToggleFocusMode>()
        || action.as_any().is::<ToggleTypewriterMode>()
        || action.as_any().is::<FindInDocument>()
        || action.as_any().is::<FindNextMatch>()
        || action.as_any().is::<FindPreviousMatch>()
}

fn is_window_context_menu_action(action: &dyn Action) -> bool {
    action.as_any().is::<NewWindow>()
        || action.as_any().is::<OpenFile>()
        || action.as_any().is::<OpenPreferences>()
        || action.as_any().is::<OpenRecentFile>()
        || action.as_any().is::<NoRecentFiles>()
        || action.as_any().is::<AddLanguageConfig>()
        || action.as_any().is::<AddThemeConfig>()
        || action.as_any().is::<InstallCliTool>()
        || action.as_any().is::<UninstallCliTool>()
        || is_editor_scoped_menu_action(action)
}

fn current_window_candidates(cx: &mut App) -> Vec<AnyWindowHandle> {
    let mut candidates = Vec::new();
    let mut push_unique = |window: AnyWindowHandle| {
        if candidates
            .iter()
            .all(|candidate: &AnyWindowHandle| candidate.window_id() != window.window_id())
        {
            candidates.push(window);
        }
    };

    if let Some(window) = cx.active_window() {
        push_unique(window);
    }
    if let Some(windows) = cx.window_stack() {
        for window in windows {
            push_unique(window);
        }
    }
    for window in cx.windows() {
        push_unique(window);
    }

    candidates
}

fn request_close_editor_window(window: AnyWindowHandle, cx: &mut App) -> bool {
    let Some(window) = window.downcast::<Editor>() else {
        return false;
    };

    window
        .update(cx, |editor, window, cx| {
            editor.request_close_current_window(window, cx);
        })
        .is_ok()
}

fn request_close_current_editor_window(cx: &mut App) {
    let candidates = current_window_candidates(cx);
    if candidates.is_empty() {
        cx.quit();
        return;
    }

    for window in candidates {
        if request_close_editor_window(window, cx) {
            return;
        }
    }
}

pub(crate) fn request_quit_application(cx: &mut App) {
    // 退出常由窗口内的操作触发（⌘Q 按键、应用内菜单点击），此时该窗口正被借用，
    // 任何对该窗口的 `window.update` 都会失败并让退出静默中断；延后到本轮更新
    // 结束后再执行，旧窗口已放回，逐窗口询问与落盘都能正常进行。
    cx.defer(perform_quit_application);
}

fn perform_quit_application(cx: &mut App) {
    let candidates = current_window_candidates(cx);
    if candidates.is_empty() {
        cx.quit();
        return;
    }

    for window in candidates {
        let Some(window) = window.downcast::<Editor>() else {
            continue;
        };

        // 允许关闭的窗口在 on_window_should_close 里顺手落盘 frame（roadmap A2），
        // 因此退出路径不需要再扫一遍窗口。
        let should_close = window
            .update(cx, |editor, window, cx| {
                editor.persist_session(cx);
                editor.on_window_should_close(window, cx)
            })
            .unwrap_or(false);
        if !should_close {
            return;
        }
    }

    cx.quit();
}

/// Executes one of the app-menu actions against the current application state.
pub(crate) fn dispatch_menu_action(action: &dyn Action, cx: &mut App) {
    if action.as_any().is::<NewWindow>() {
        open_editor_window(cx, String::new(), None);
    } else if action.as_any().is::<OpenFile>() {
        prompt_and_open_files(cx);
    } else if action.as_any().is::<CopyAsHtml>() {
        let _ = with_active_editor(cx, |editor, _window, cx| editor.copy_as_html(cx));
    } else if action.as_any().is::<OpenCommandPalette>() {
        let _ = with_active_editor(cx, |editor, window, cx| {
            editor.toggle_command_palette(window, cx);
        });
    } else if action.as_any().is::<ToggleViewMode>() {
        let _ = with_active_editor(cx, |editor, _, cx| editor.toggle_view_mode_from_ui(cx));
    } else if action.as_any().is::<ToggleFocusMode>() {
        let _ = with_active_editor(cx, |editor, _, cx| editor.toggle_focus_mode(cx));
    } else if action.as_any().is::<ToggleTypewriterMode>() {
        let _ = with_active_editor(cx, |editor, _, cx| editor.toggle_typewriter_mode(cx));
    } else if action.as_any().is::<FindInDocument>() {
        let _ = with_active_editor(cx, |editor, _, cx| editor.open_document_find(cx));
    } else if action.as_any().is::<FindNextMatch>() {
        let _ = with_active_editor(cx, |editor, _, cx| {
            editor.find_next_document_match(false, cx)
        });
    } else if action.as_any().is::<FindPreviousMatch>() {
        let _ = with_active_editor(cx, |editor, _, cx| {
            editor.find_next_document_match(true, cx)
        });
    } else if action.as_any().is::<OpenPreferences>() {
        open_preferences_window(cx);
    } else if let Some(action) = action.as_any().downcast_ref::<OpenRecentFile>() {
        open_recent_file(cx, PathBuf::from(&action.path));
    } else if action.as_any().is::<NoRecentFiles>() {
    } else if action.as_any().is::<AddLanguageConfig>() {
        prompt_and_import_language_config(cx);
    } else if action.as_any().is::<AddThemeConfig>() {
        prompt_and_import_theme_config(cx);
    } else if action.as_any().is::<SaveDocument>() {
        let _ = with_active_editor(cx, |editor, window, cx| editor.save_document(window, cx));
    } else if action.as_any().is::<SaveDocumentAs>() {
        let _ = with_active_editor(cx, |editor, window, cx| editor.save_document_as(window, cx));
    } else if action.as_any().is::<ExportHtml>() {
        let _ = with_active_editor(cx, |editor, window, cx| {
            editor.export_document_via_prompt(ExportFormat::Html, window, cx)
        });
    } else if action.as_any().is::<ExportPdf>() {
        let _ = with_active_editor(cx, |editor, window, cx| {
            editor.export_document_via_prompt(ExportFormat::Pdf, window, cx)
        });
    } else if action.as_any().is::<ExportPng>() {
        let _ = with_active_editor(cx, |editor, window, cx| {
            editor.export_document_via_prompt(ExportFormat::Png, window, cx)
        });
    } else if action.as_any().is::<PrintDocument>() {
        let _ = with_active_editor(cx, |editor, window, cx| {
            print_document(editor, window, cx);
        });
    } else if let Some(action) = action.as_any().downcast_ref::<SelectTheme>() {
        match apply_configured_theme(cx, &action.theme_id) {
            Ok(changed) => {
                if changed {
                    install_menus(cx);
                    cx.refresh_windows();
                }
            }
            Err(err) => {
                let title = cx
                    .global::<I18nManager>()
                    .strings()
                    .preferences_save_failed_title
                    .clone();
                show_window_prompt(cx.active_window(), &title, &err.to_string(), cx);
            }
        }
    } else if let Some(action) = action.as_any().downcast_ref::<SelectLanguage>() {
        match apply_configured_language(cx, &action.language_id) {
            Ok(changed) => {
                if changed {
                    install_menus(cx);
                    cx.refresh_windows();
                }
            }
            Err(err) => {
                let title = cx
                    .global::<I18nManager>()
                    .strings()
                    .preferences_save_failed_title
                    .clone();
                show_window_prompt(cx.active_window(), &title, &err.to_string(), cx);
            }
        }
    } else if action.as_any().is::<CheckForUpdates>() {
        request_update_check_on_active_editor(cx);
    } else if action.as_any().is::<ShowAbout>() {
        show_info_dialog_on_active_editor(cx, InfoDialogKind::About);
    } else if action.as_any().is::<InstallCliTool>() {
        install_cli_tool(cx);
    } else if action.as_any().is::<UninstallCliTool>() {
        uninstall_cli_tool(cx);
    } else if action.as_any().is::<ToggleSidebar>() {
        let _ = with_active_editor(cx, |editor, window, cx| {
            editor.toggle_workspace_drawer(window, cx);
        });
    } else if action.as_any().is::<ToggleFullscreen>() {
        let _ = with_active_editor(cx, |_editor, window, _cx| {
            window.toggle_fullscreen();
            window.refresh();
        });
    } else if action.as_any().is::<ZoomIn>() {
        let _ = with_active_editor(cx, |editor, _window, cx| editor.zoom_by(10, cx));
    } else if action.as_any().is::<ZoomOut>() {
        let _ = with_active_editor(cx, |editor, _window, cx| editor.zoom_by(-10, cx));
    } else if action.as_any().is::<ZoomReset>() {
        let _ = with_active_editor(cx, |editor, _window, cx| editor.zoom_reset(cx));
    } else if action.as_any().is::<QuitApplication>() {
        request_quit_application(cx);
    } else if action.as_any().is::<CloseWindow>() {
        request_close_current_editor_window(cx);
    }
}

/// Executes a menu action against a specific editor when the action is
/// editor-scoped, falling back to app-wide behavior for global actions.
pub(crate) fn dispatch_menu_action_for_editor(
    action: &dyn Action,
    target: &WeakEntity<Editor>,
    window: &mut Window,
    cx: &mut App,
) {
    if !is_window_context_menu_action(action) {
        let deferred_action = action.boxed_clone();
        cx.defer(move |cx| {
            dispatch_menu_action(deferred_action.as_ref(), cx);
        });
        return;
    }

    window.activate_window();
    let current_window = Some(window.window_handle());

    if action.as_any().is::<NewWindow>() {
        open_editor_window(cx, String::new(), None);
    } else if action.as_any().is::<OpenFile>() {
        prompt_and_open_files_with_error_window(cx, current_window);
    } else if action.as_any().is::<ToggleViewMode>() {
        let _ = target.update(cx, |editor, cx| editor.toggle_view_mode_from_ui(cx));
    } else if action.as_any().is::<ToggleFocusMode>() {
        let _ = target.update(cx, |editor, cx| editor.toggle_focus_mode(cx));
    } else if action.as_any().is::<ToggleTypewriterMode>() {
        let _ = target.update(cx, |editor, cx| editor.toggle_typewriter_mode(cx));
    } else if action.as_any().is::<FindInDocument>() {
        let _ = target.update(cx, |editor, cx| editor.open_document_find(cx));
    } else if action.as_any().is::<FindNextMatch>() {
        let _ = target.update(cx, |editor, cx| editor.find_next_document_match(false, cx));
    } else if action.as_any().is::<FindPreviousMatch>() {
        let _ = target.update(cx, |editor, cx| editor.find_next_document_match(true, cx));
    } else if action.as_any().is::<OpenPreferences>() {
        open_preferences_window(cx);
    } else if let Some(action) = action.as_any().downcast_ref::<OpenRecentFile>() {
        open_recent_file_with_error_window(cx, PathBuf::from(&action.path), current_window);
    } else if action.as_any().is::<NoRecentFiles>() {
    } else if action.as_any().is::<AddLanguageConfig>() {
        prompt_and_import_language_config_with_error_window(cx, current_window);
    } else if action.as_any().is::<AddThemeConfig>() {
        prompt_and_import_theme_config_with_error_window(cx, current_window);
    } else if action.as_any().is::<SaveDocument>() {
        let _ = target.update(cx, |editor, cx| editor.request_save_document(cx));
    } else if action.as_any().is::<SaveDocumentAs>() {
        let _ = target.update(cx, |editor, cx| editor.request_save_document_as(cx));
    } else if action.as_any().is::<ExportHtml>() {
        let _ = target.update(cx, |editor, cx| {
            editor.export_document_via_prompt(ExportFormat::Html, window, cx);
        });
    } else if action.as_any().is::<ExportPdf>() {
        let _ = target.update(cx, |editor, cx| {
            editor.export_document_via_prompt(ExportFormat::Pdf, window, cx);
        });
    } else if action.as_any().is::<ExportPng>() {
        let _ = target.update(cx, |editor, cx| {
            editor.export_document_via_prompt(ExportFormat::Png, window, cx);
        });
    } else if action.as_any().is::<PrintDocument>() {
        let _ = target.update(cx, |editor, cx| {
            print_document(editor, window, cx);
        });
    } else if action.as_any().is::<QuitApplication>() {
        request_quit_application(cx);
    } else if action.as_any().is::<CloseWindow>() {
        let _ = target.update(cx, |editor, cx| {
            editor.request_close_current_window(window, cx);
        });
    } else if action.as_any().is::<CheckForUpdates>() {
        let _ = target.update(cx, |editor, cx| {
            editor.request_check_updates(window, cx);
        });
    } else if action.as_any().is::<ShowAbout>() {
        let _ = target.update(cx, |editor, cx| {
            editor.show_info_dialog(InfoDialogKind::About, cx)
        });
    } else if action.as_any().is::<InstallCliTool>() {
        install_cli_tool(cx);
    } else if action.as_any().is::<UninstallCliTool>() {
        uninstall_cli_tool(cx);
    } else if action.as_any().is::<ToggleSidebar>() {
        let _ = target.update(cx, |editor, cx| {
            editor.toggle_workspace_drawer(window, cx);
        });
    } else if action.as_any().is::<ToggleFullscreen>() {
        window.toggle_fullscreen();
        window.refresh();
    }
}

/// 由命令注册表构造某个菜单的动作条目（含分隔线，roadmap H5）。
fn command_menu_items(strings: &I18nStrings, menu: CommandMenu) -> Vec<MenuItem> {
    let mut items = Vec::new();
    for spec in crate::commands::commands_for(menu) {
        if spec.separator_before && !items.is_empty() {
            items.push(MenuItem::separator());
        }
        items.push(command_menu_item(strings, spec));
    }
    items
}

fn command_menu_item(strings: &I18nStrings, spec: &CommandSpec) -> MenuItem {
    MenuItem::Action {
        name: spec.label(strings).into(),
        action: spec.boxed_action(),
        os_action: None,
    }
}

/// 按 id 取注册命令（非 macOS 折叠应用菜单时用）。
#[cfg(not(target_os = "macos"))]
fn command_spec(id: &str) -> &'static CommandSpec {
    crate::commands::commands()
        .iter()
        .find(|spec| spec.id == id)
        .expect("command registry should keep the id")
}

/// 文件菜单：注册表 File 分组，并把「打开最近」子菜单接在「打开文件」之后。
///
/// 非 macOS 没有应用菜单，App 分组的偏好设置与退出并入文件菜单
/// （顺序沿用既有版本：偏好设置紧跟最近打开，退出在最后）。
fn file_menu_items(strings: &I18nStrings, recent_items: Vec<MenuItem>) -> Vec<MenuItem> {
    let mut items = Vec::new();
    let mut recent_items = Some(recent_items);
    for spec in crate::commands::commands_for(CommandMenu::File) {
        if spec.separator_before && !items.is_empty() {
            items.push(MenuItem::separator());
        }
        items.push(command_menu_item(strings, spec));
        if spec.id == "open_file" {
            if let Some(recent_items) = recent_items.take() {
                items.push(MenuItem::submenu(Menu {
                    name: strings.menu_open_recent_file.clone().into(),
                    items: recent_items,
                }));
            }
            #[cfg(not(target_os = "macos"))]
            items.push(command_menu_item(strings, command_spec("preferences")));
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        items.push(MenuItem::separator());
        items.push(command_menu_item(strings, command_spec("quit")));
    }
    items
}

fn build_menus(
    theme_manager: &ThemeManager,
    i18n_manager: &I18nManager,
    recent_files: &[PathBuf],
) -> Vec<Menu> {
    let current_theme_id = theme_manager.current_theme_id().to_string();
    let current_language_id = i18n_manager.current_language_id().to_string();
    let strings = i18n_manager.strings().clone();
    let mut theme_items = theme_manager
        .available_themes()
        .iter()
        .map(|entry| {
            let name = match entry.id.as_str() {
                "system" => strings.preferences_theme_system.clone(),
                "velora-dark" => strings.preferences_theme_dark.clone(),
                "velora-light" => strings.preferences_theme_light.clone(),
                _ => entry.name.clone(),
            };
            let label = if entry.id.as_str() == current_theme_id {
                format!("\u{2713} {name}")
            } else {
                name
            };
            MenuItem::action(
                label,
                SelectTheme {
                    theme_id: entry.id.to_string(),
                },
            )
        })
        .collect::<Vec<_>>();
    theme_items.push(MenuItem::separator());
    theme_items.push(MenuItem::action(
        strings.menu_add_theme_config.clone(),
        AddThemeConfig,
    ));

    let mut language_items = i18n_manager
        .available_languages()
        .iter()
        .map(|entry| {
            let name = entry.name.to_string();
            let label = if entry.id.as_str() == current_language_id {
                format!("\u{2713} {name}")
            } else {
                name
            };
            MenuItem::action(
                label,
                SelectLanguage {
                    language_id: entry.id.to_string(),
                },
            )
        })
        .collect::<Vec<_>>();
    language_items.push(MenuItem::separator());
    language_items.push(MenuItem::action(
        strings.menu_add_language_config.clone(),
        AddLanguageConfig,
    ));

    let recent_items = if recent_files.is_empty() {
        vec![MenuItem::action(
            strings.menu_no_recent_files.clone(),
            NoRecentFiles,
        )]
    } else {
        recent_files
            .iter()
            .map(|path| {
                // into_owned on a Cow<str> reuses the Cow::Owned variant
                // (no copy) when the OS string is valid UTF-8 — the common
                // case — and only allocates for the lossy fallback. The
                // previous .to_string_lossy().to_string() always allocated.
                let label = path.to_string_lossy().into_owned();
                MenuItem::action(label.clone(), OpenRecentFile { path: label })
            })
            .collect()
    };

    #[cfg(target_os = "macos")]
    let initial_menus = {
        // On macOS, the first menu is the app menu (macOS overrides its title
        // with the app name). File operations go in a separate "File" menu to
        // match standard macOS conventions.
        vec![
            Menu {
                name: "Velora".into(),
                items: command_menu_items(&strings, CommandMenu::App),
            },
            Menu {
                name: strings.menu_file.clone().into(),
                items: file_menu_items(&strings, recent_items),
            },
        ]
    };

    #[cfg(not(target_os = "macos"))]
    let initial_menus = {
        vec![Menu {
            name: strings.menu_file.clone().into(),
            items: file_menu_items(&strings, recent_items),
        }]
    };

    #[cfg(target_os = "macos")]
    let help_items = {
        // Show different menu item depending on whether CLI is already
        // installed pointing to the current app.  Only portable
        // installations (drag-installed .app bundles) need this —
        // pkg-installed apps manage the symlink via postinstall.
        let cli_installed = is_cli_symlink_current_app();
        let mut items = Vec::new();
        if cli_installed {
            items.push(MenuItem::action(
                SharedString::new(strings.menu_uninstall_cli_tool.as_str()),
                UninstallCliTool,
            ));
        } else {
            items.push(MenuItem::action(
                SharedString::new(strings.menu_install_cli_tool.as_str()),
                InstallCliTool,
            ));
        }
        items.push(MenuItem::separator());
        items.extend(command_menu_items(&strings, CommandMenu::Help));
        items
    };
    #[cfg(not(target_os = "macos"))]
    let help_items = command_menu_items(&strings, CommandMenu::Help);

    let mut menus = initial_menus;
    menus.extend([
        Menu {
            name: strings.menu_export.clone().into(),
            items: command_menu_items(&strings, CommandMenu::Export),
        },
        Menu {
            name: strings.menu_language.clone().into(),
            items: language_items,
        },
        Menu {
            name: strings.menu_theme.clone().into(),
            items: theme_items,
        },
        Menu {
            name: if current_language_id == "zh-CN" {
                "视图"
            } else {
                "View"
            }
            .into(),
            items: command_menu_items(&strings, CommandMenu::View),
        },
        Menu {
            name: strings.menu_help.clone().into(),
            items: help_items,
        },
    ]);
    menus
}

pub(crate) fn install_menus(cx: &mut App) {
    let recent_entries = recent_menu_entries(&recent_folders_for_menu());
    let menus = build_menus(
        cx.global::<ThemeManager>(),
        cx.global::<I18nManager>(),
        &recent_entries,
    );
    cx.set_menus(menus);
}

/// Finds the editor window that should receive a picked folder: the active
/// window when it is an editor, otherwise the most recent editor window.
fn editor_window_for_folder_open(cx: &App) -> Option<WindowHandle<Editor>> {
    let mut candidates = Vec::new();
    if let Some(window) = cx.active_window() {
        candidates.push(window);
    }
    if let Some(stack) = cx.window_stack() {
        candidates.extend(stack);
    }
    candidates.extend(cx.windows());
    candidates
        .into_iter()
        .find_map(|window| window.downcast::<Editor>())
}

/// Opens the in-app folder-destination dialog on the target window; the
/// overlay itself renders from `Editor::pending_folder_choice`.
fn prompt_folder_destination(cx: &mut App, target: WindowHandle<Editor>, folder: PathBuf) {
    let _ = target.update(cx, |editor, _window, cx| {
        editor.pending_folder_choice = Some(folder);
        cx.notify();
    });
}

fn prompt_and_open_files(cx: &mut App) {
    let error_window = cx.active_window();
    prompt_and_open_files_with_error_window(cx, error_window);
}

fn prompt_and_open_files_with_error_window(cx: &mut App, error_window: Option<AnyWindowHandle>) {
    let prompt_title = cx
        .global::<I18nManager>()
        .strings()
        .open_markdown_files_prompt
        .clone();
    let prompt = cx.prompt_for_paths(PathPromptOptions {
        files: true,
        directories: true,
        multiple: true,
        prompt: Some(prompt_title.into()),
    });

    cx.spawn(async move |cx| match prompt.await {
        Ok(Ok(Some(paths))) => {
            let _ = cx.update(move |cx| {
                let target = editor_window_for_folder_open(cx);
                for path in paths {
                    if path.is_dir() {
                        // A folder replaces the current working set or opens
                        // fresh — VS Code style — so the current window is
                        // never silently re-rooted.
                        match target.as_ref() {
                            Some(handle) => {
                                prompt_folder_destination(cx, *handle, path);
                            }
                            None => {
                                let _ = open_workspace_window(cx, path);
                            }
                        }
                        continue;
                    }
                    if let Some(handle) = &target {
                        let _ = handle.update(cx, |editor, window, cx| {
                            editor.open_workspace_file(path.clone(), window, cx);
                        });
                        continue;
                    }
                    if let Err(err) = open_file_in_new_window(cx, &path) {
                        let title = cx
                            .global::<I18nManager>()
                            .strings()
                            .open_failed_title
                            .clone();
                        show_window_prompt(error_window, &title, &err.to_string(), cx);
                    }
                }
            });
        }
        Ok(Err(err)) => {
            let detail = err.to_string();
            let _ = cx.update(move |cx| {
                let title = cx
                    .global::<I18nManager>()
                    .strings()
                    .open_failed_title
                    .clone();
                show_window_prompt(error_window, &title, &detail, cx);
            });
        }
        Ok(Ok(None)) | Err(_) => {}
    })
    .detach();
}

fn prompt_and_import_language_config(cx: &mut App) {
    let error_window = cx.active_window();
    prompt_and_import_language_config_with_error_window(cx, error_window);
}

fn prompt_and_import_language_config_with_error_window(
    cx: &mut App,
    error_window: Option<AnyWindowHandle>,
) {
    let prompt_title = cx
        .global::<I18nManager>()
        .strings()
        .add_language_config_prompt
        .clone();
    let prompt = cx.prompt_for_paths(PathPromptOptions {
        files: true,
        directories: false,
        multiple: false,
        prompt: Some(prompt_title.into()),
    });

    cx.spawn(async move |cx| match prompt.await {
        Ok(Ok(Some(paths))) => {
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let _ = cx.update(move |cx| {
                let result = import_language_config_and_select(cx, &path);
                match result {
                    Ok(_) => {
                        install_menus(cx);
                        cx.refresh_windows();
                    }
                    Err(err) => {
                        let title = cx
                            .global::<I18nManager>()
                            .strings()
                            .config_import_failed_title
                            .clone();
                        show_window_prompt(error_window, &title, &err.to_string(), cx);
                    }
                }
            });
        }
        Ok(Err(err)) => {
            let detail = err.to_string();
            let _ = cx.update(move |cx| {
                let title = cx
                    .global::<I18nManager>()
                    .strings()
                    .config_import_failed_title
                    .clone();
                show_window_prompt(error_window, &title, &detail, cx);
            });
        }
        Ok(Ok(None)) | Err(_) => {}
    })
    .detach();
}

fn prompt_and_import_theme_config(cx: &mut App) {
    let error_window = cx.active_window();
    prompt_and_import_theme_config_with_error_window(cx, error_window);
}

fn prompt_and_import_theme_config_with_error_window(
    cx: &mut App,
    error_window: Option<AnyWindowHandle>,
) {
    let prompt_title = cx
        .global::<I18nManager>()
        .strings()
        .add_theme_config_prompt
        .clone();
    let prompt = cx.prompt_for_paths(PathPromptOptions {
        files: true,
        directories: false,
        multiple: false,
        prompt: Some(prompt_title.into()),
    });

    cx.spawn(async move |cx| match prompt.await {
        Ok(Ok(Some(paths))) => {
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let _ = cx.update(move |cx| {
                let result = import_theme_config_and_select(cx, &path);
                match result {
                    Ok(_) => {
                        install_menus(cx);
                        cx.refresh_windows();
                    }
                    Err(err) => {
                        let title = cx
                            .global::<I18nManager>()
                            .strings()
                            .config_import_failed_title
                            .clone();
                        show_window_prompt(error_window, &title, &err.to_string(), cx);
                    }
                }
            });
        }
        Ok(Err(err)) => {
            let detail = err.to_string();
            let _ = cx.update(move |cx| {
                let title = cx
                    .global::<I18nManager>()
                    .strings()
                    .config_import_failed_title
                    .clone();
                show_window_prompt(error_window, &title, &detail, cx);
            });
        }
        Ok(Ok(None)) | Err(_) => {}
    })
    .detach();
}

fn handle_window_closed(cx: &mut App) {
    if cx.windows().is_empty() {
        cx.quit();
    }
}

/// Installs menu state, action handlers, and the native menu bar.
pub(crate) fn init(cx: &mut App) {
    cx.set_global(AppMenuState::default());
    let subscription = cx.on_window_closed(handle_window_closed);
    cx.global_mut::<AppMenuState>().window_closed_subscription = Some(subscription);

    cx.on_action(|_: &NewWindow, cx| {
        dispatch_menu_action(&NewWindow, cx);
    });
    cx.on_action(|_: &OpenFile, cx| {
        dispatch_menu_action(&OpenFile, cx);
    });
    cx.on_action(|_: &ToggleViewMode, cx| {
        dispatch_menu_action(&ToggleViewMode, cx);
    });
    cx.on_action(|_: &ToggleFocusMode, cx| {
        dispatch_menu_action(&ToggleFocusMode, cx);
    });
    cx.on_action(|_: &FindInDocument, cx| {
        dispatch_menu_action(&FindInDocument, cx);
    });
    cx.on_action(|_: &FindNextMatch, cx| {
        dispatch_menu_action(&FindNextMatch, cx);
    });
    cx.on_action(|_: &FindPreviousMatch, cx| {
        dispatch_menu_action(&FindPreviousMatch, cx);
    });
    cx.on_action(|_: &ToggleTypewriterMode, cx| {
        dispatch_menu_action(&ToggleTypewriterMode, cx);
    });
    cx.on_action(|_: &OpenPreferences, cx| {
        dispatch_menu_action(&OpenPreferences, cx);
    });
    cx.on_action(|action: &OpenRecentFile, cx| {
        dispatch_menu_action(action, cx);
    });
    cx.on_action(|_: &NoRecentFiles, cx| {
        dispatch_menu_action(&NoRecentFiles, cx);
    });
    cx.on_action(|_: &AddLanguageConfig, cx| {
        dispatch_menu_action(&AddLanguageConfig, cx);
    });
    cx.on_action(|_: &AddThemeConfig, cx| {
        dispatch_menu_action(&AddThemeConfig, cx);
    });
    cx.on_action(|_: &SaveDocument, cx| {
        dispatch_menu_action(&SaveDocument, cx);
    });
    cx.on_action(|_: &SaveDocumentAs, cx| {
        dispatch_menu_action(&SaveDocumentAs, cx);
    });
    cx.on_action(|_: &ExportHtml, cx| {
        dispatch_menu_action(&ExportHtml, cx);
    });
    cx.on_action(|_: &ExportPdf, cx| {
        dispatch_menu_action(&ExportPdf, cx);
    });
    cx.on_action(|_: &ExportPng, cx| {
        dispatch_menu_action(&ExportPng, cx);
    });
    cx.on_action(|_: &PrintDocument, cx| {
        dispatch_menu_action(&PrintDocument, cx);
    });
    cx.on_action(|action: &SelectTheme, cx| {
        dispatch_menu_action(action, cx);
    });
    cx.on_action(|action: &SelectLanguage, cx| {
        dispatch_menu_action(action, cx);
    });
    cx.on_action(|_: &CheckForUpdates, cx| {
        dispatch_menu_action(&CheckForUpdates, cx);
    });
    cx.on_action(|_: &ShowAbout, cx| {
        dispatch_menu_action(&ShowAbout, cx);
    });
    cx.on_action(|_: &CopyAsHtml, cx| {
        dispatch_menu_action(&CopyAsHtml, cx);
    });
    cx.on_action(|_: &OpenCommandPalette, cx| {
        dispatch_menu_action(&OpenCommandPalette, cx);
    });
    cx.on_action(|_: &ZoomIn, cx| {
        dispatch_menu_action(&ZoomIn, cx);
    });
    cx.on_action(|_: &ZoomOut, cx| {
        dispatch_menu_action(&ZoomOut, cx);
    });
    cx.on_action(|_: &ZoomReset, cx| {
        dispatch_menu_action(&ZoomReset, cx);
    });
    cx.on_action(|_: &ToggleSidebar, cx| {
        dispatch_menu_action(&ToggleSidebar, cx);
    });
    cx.on_action(|_: &ToggleFullscreen, cx| {
        dispatch_menu_action(&ToggleFullscreen, cx);
    });
    cx.on_action(|_: &QuitApplication, cx| {
        dispatch_menu_action(&QuitApplication, cx);
    });
    cx.on_action(|_: &CloseWindow, cx| {
        dispatch_menu_action(&CloseWindow, cx);
    });

    install_menus(cx);
    cx.activate(true);
}

#[cfg(test)]
mod tests {
    use super::{applescript_string_literal, build_menus, recent_menu_entries};
    use crate::components::{
        AddLanguageConfig, AddThemeConfig, CheckForUpdates, CloseWindow, CopyAsHtml, ExportHtml,
        ExportPdf, ExportPng, NewWindow, NoRecentFiles, OpenFile, OpenPreferences, OpenRecentFile,
        PrintDocument, QuitApplication,
        SaveDocument, SelectLanguage, SelectTheme, ShowAbout,
    };
    use crate::i18n::I18nManager;
    use crate::theme::ThemeManager;
    use gpui::MenuItem;
    use std::path::PathBuf;

    fn action_name(item: &MenuItem) -> &str {
        match item {
            MenuItem::Action { name, .. } => name.as_ref(),
            _ => panic!("expected action menu item"),
        }
    }

    fn submenu(item: &MenuItem) -> &gpui::Menu {
        match item {
            MenuItem::Submenu(menu) => menu,
            _ => panic!("expected submenu item"),
        }
    }

    #[test]
    fn applescript_string_literal_escapes_special_characters() {
        assert_eq!(
            applescript_string_literal(r#"/Applications/velora "Test".app/Contents/MacOS/velora"#),
            r#""/Applications/velora \"Test\".app/Contents/MacOS/velora""#
        );
        assert_eq!(
            applescript_string_literal(r#"/Applications/O'Brien\velora.app"#),
            r#""/Applications/O'Brien\\velora.app""#
        );
    }

    // On macOS the menu bar is: [velora app menu, File, Export, Language, Theme, View, Help]
    // On other platforms:       [File, Export, Language, Theme, View, Help]
    #[cfg(target_os = "macos")]
    const EXPORT_IDX: usize = 2;
    #[cfg(not(target_os = "macos"))]
    const EXPORT_IDX: usize = 1;

    #[cfg(target_os = "macos")]
    const LANGUAGE_IDX: usize = 3;
    #[cfg(not(target_os = "macos"))]
    const LANGUAGE_IDX: usize = 2;

    #[cfg(target_os = "macos")]
    const THEME_IDX: usize = 4;
    #[cfg(not(target_os = "macos"))]
    const THEME_IDX: usize = 3;

    #[cfg(target_os = "macos")]
    const VIEW_IDX: usize = 5;
    #[cfg(not(target_os = "macos"))]
    const VIEW_IDX: usize = 4;

    #[cfg(target_os = "macos")]
    const HELP_IDX: usize = 6;
    #[cfg(not(target_os = "macos"))]
    const HELP_IDX: usize = 5;

    #[test]
    fn build_menus_uses_english_fallback_by_default() {
        let theme_manager = ThemeManager::default();
        let i18n_manager = I18nManager::default();
        let menus = build_menus(&theme_manager, &i18n_manager, &[]);

        let menu_names = menus
            .iter()
            .map(|menu| menu.name.to_string())
            .collect::<Vec<_>>();

        #[cfg(target_os = "macos")]
        assert_eq!(
            menu_names,
            vec![
                "Velora",
                "File",
                "Export",
                "Language",
                "Theme",
                "View",
                "Help"
            ]
        );
        #[cfg(not(target_os = "macos"))]
        assert_eq!(
            menu_names,
            vec![
                "File",
                "Export",
                "Language",
                "Theme",
                "View",
                "Help"
            ]
        );

        // New Window belongs with file operations on macOS and remains the
        // first File menu item on other platforms.
        #[cfg(target_os = "macos")]
        assert_eq!(action_name(&menus[1].items[0]), "New Window");
        #[cfg(not(target_os = "macos"))]
        assert_eq!(action_name(&menus[0].items[0]), "New Window");

        // Open Recent File submenu location differs by platform.
        #[cfg(target_os = "macos")]
        assert_eq!(
            submenu(&menus[1].items[3]).name.to_string(),
            "Open Recent"
        );
        #[cfg(not(target_os = "macos"))]
        assert_eq!(
            submenu(&menus[0].items[3]).name.to_string(),
            "Open Recent"
        );

        // Close Window is colocated with New Window in the File menu.
        #[cfg(target_os = "macos")]
        assert_eq!(action_name(&menus[1].items[1]), "Close Window");
        #[cfg(not(target_os = "macos"))]
        assert_eq!(action_name(&menus[0].items[1]), "Close Window");

        // Preferences location differs by platform.
        #[cfg(target_os = "macos")]
        assert_eq!(action_name(&menus[0].items[0]), "Preferences");
        #[cfg(not(target_os = "macos"))]
        assert_eq!(action_name(&menus[0].items[4]), "Preferences");

        assert_eq!(action_name(&menus[EXPORT_IDX].items[0]), "HTML");
        assert_eq!(action_name(&menus[EXPORT_IDX].items[1]), "PDF");
        assert_eq!(action_name(&menus[EXPORT_IDX].items[2]), "Image (PNG)");
        assert_eq!(action_name(&menus[EXPORT_IDX].items[3]), "Print…");
        assert_eq!(action_name(&menus[EXPORT_IDX].items[4]), "Copy as HTML");
        assert_eq!(action_name(&menus[LANGUAGE_IDX].items[0]), "简体中文");
        assert_eq!(
            action_name(&menus[LANGUAGE_IDX].items[1]),
            "\u{2713} English"
        );
        assert_eq!(action_name(&menus[VIEW_IDX].items[0]), "Toggle Sidebar");
        assert_eq!(action_name(&menus[VIEW_IDX].items[1]), "Toggle Full Screen");
        assert_eq!(action_name(&menus[VIEW_IDX].items[3]), "Toggle View Mode");
        assert_eq!(action_name(&menus[VIEW_IDX].items[4]), "Toggle Focus Mode");
        assert_eq!(
            action_name(&menus[VIEW_IDX].items[5]),
            "Toggle Typewriter Mode"
        );
        assert_eq!(
            action_name(&menus[VIEW_IDX].items[7]),
            "Command Palette…"
        );
        assert_eq!(action_name(&menus[VIEW_IDX].items[8]), "Find in Document…");
        assert_eq!(action_name(&menus[VIEW_IDX].items[9]), "Find Next");
        assert_eq!(action_name(&menus[VIEW_IDX].items[10]), "Find Previous");
    }

    #[test]
    fn build_menus_uses_chinese_language_when_selected() {
        let theme_manager = ThemeManager::default();
        let i18n_manager = I18nManager::new_with_language_id("zh-CN");
        let menus = build_menus(&theme_manager, &i18n_manager, &[]);

        #[cfg(target_os = "macos")]
        assert_eq!(
            submenu(&menus[1].items[3]).name.to_string(),
            i18n_manager.strings().menu_open_recent_file.as_str()
        );
        #[cfg(not(target_os = "macos"))]
        assert_eq!(
            submenu(&menus[0].items[3]).name.to_string(),
            i18n_manager.strings().menu_open_recent_file.as_str()
        );

        let menu_names = menus
            .iter()
            .map(|menu| menu.name.to_string())
            .collect::<Vec<_>>();

        #[cfg(target_os = "macos")]
        assert_eq!(
            menu_names,
            vec![
                "Velora",
                "文件",
                "导出",
                "语言",
                "主题",
                "视图",
                "帮助"
            ]
        );
        #[cfg(not(target_os = "macos"))]
        assert_eq!(
            menu_names,
            vec!["文件", "导出", "语言", "主题", "视图", "帮助"]
        );

        #[cfg(target_os = "macos")]
        assert_eq!(action_name(&menus[1].items[0]), "新建窗口");
        #[cfg(not(target_os = "macos"))]
        assert_eq!(action_name(&menus[0].items[0]), "新建窗口");
        assert_eq!(action_name(&menus[EXPORT_IDX].items[0]), "HTML");
        assert_eq!(action_name(&menus[EXPORT_IDX].items[1]), "PDF");
        assert_eq!(action_name(&menus[EXPORT_IDX].items[2]), "图片（PNG 长图）");
        assert_eq!(action_name(&menus[EXPORT_IDX].items[3]), "打印…");
        assert_eq!(action_name(&menus[EXPORT_IDX].items[4]), "复制为 HTML");
        assert_eq!(
            action_name(&menus[LANGUAGE_IDX].items[0]),
            "\u{2713} 简体中文"
        );
        assert_eq!(action_name(&menus[LANGUAGE_IDX].items[1]), "English");
        assert_eq!(action_name(&menus[VIEW_IDX].items[0]), "切换侧边栏");
        assert_eq!(action_name(&menus[VIEW_IDX].items[1]), "切换全屏");
        assert_eq!(action_name(&menus[VIEW_IDX].items[3]), "切换视图模式");
        assert_eq!(action_name(&menus[VIEW_IDX].items[4]), "切换专注模式");
        assert_eq!(action_name(&menus[VIEW_IDX].items[5]), "切换打字机模式");
        assert_eq!(action_name(&menus[VIEW_IDX].items[7]), "命令面板…");
        assert_eq!(action_name(&menus[VIEW_IDX].items[8]), "查找当前文档…");
        assert_eq!(action_name(&menus[VIEW_IDX].items[9]), "查找下一个");
        assert_eq!(action_name(&menus[VIEW_IDX].items[10]), "查找上一个");
    }

    #[test]
    fn export_menu_items_dispatch_export_actions() {
        let theme_manager = ThemeManager::default();
        let i18n_manager = I18nManager::default();
        let menus = build_menus(&theme_manager, &i18n_manager, &[]);

        match &menus[EXPORT_IDX].items[0] {
            MenuItem::Action { action, .. } => {
                assert!(action.as_any().is::<ExportHtml>());
            }
            _ => panic!("expected export html action item"),
        }

        match &menus[EXPORT_IDX].items[1] {
            MenuItem::Action { action, .. } => {
                assert!(action.as_any().is::<ExportPdf>());
            }
            _ => panic!("expected export pdf action item"),
        }

        // roadmap F5：PNG 长图菜单项排在 PDF 之后。
        match &menus[EXPORT_IDX].items[2] {
            MenuItem::Action { action, .. } => {
                assert!(action.as_any().is::<ExportPng>());
            }
            _ => panic!("expected export png action item"),
        }

        // roadmap F4：打印菜单项存在且分发 PrintDocument，复制为 HTML 顺延到第 5 项。
        match &menus[EXPORT_IDX].items[3] {
            MenuItem::Action { action, .. } => {
                assert!(action.as_any().is::<PrintDocument>());
            }
            _ => panic!("expected print action item"),
        }
        match &menus[EXPORT_IDX].items[4] {
            MenuItem::Action { action, .. } => {
                assert!(action.as_any().is::<CopyAsHtml>());
            }
            _ => panic!("expected copy as html action item"),
        }
    }

    /// roadmap H5：菜单与命令面板同源于命令注册表——逐条对照视图菜单
    /// （文案 + 动作 + 分隔线位置），多一条少一条都会失败。
    #[test]
    fn view_menu_matches_the_command_registry() {
        let theme_manager = ThemeManager::default();
        let i18n_manager = I18nManager::default();
        let menus = build_menus(&theme_manager, &i18n_manager, &[]);
        let strings = i18n_manager.strings();
        let items = &menus[VIEW_IDX].items;

        let mut index = 0;
        for spec in crate::commands::commands_for(crate::commands::CommandMenu::View) {
            if spec.separator_before && index > 0 {
                assert!(
                    matches!(items[index], MenuItem::Separator),
                    "{} 前应有分隔线",
                    spec.id
                );
                index += 1;
            }
            match items.get(index) {
                Some(MenuItem::Action { name, action, .. }) => {
                    assert_eq!(name.as_ref(), spec.label(strings).as_str(), "{} 文案", spec.id);
                    assert!(
                        action.as_ref().partial_eq(spec.boxed_action().as_ref()),
                        "{} 动作类型不一致",
                        spec.id
                    );
                }
                _ => panic!("视图菜单缺少条目 {}", spec.id),
            }
            index += 1;
        }
        assert_eq!(index, items.len(), "视图菜单存在注册表之外的条目");
    }

    #[test]
    fn language_menu_items_dispatch_select_language_actions() {
        let theme_manager = ThemeManager::default();
        let i18n_manager = I18nManager::default();
        let menus = build_menus(&theme_manager, &i18n_manager, &[]);

        match &menus[LANGUAGE_IDX].items[0] {
            MenuItem::Action { action, .. } => {
                let action = action
                    .as_any()
                    .downcast_ref::<SelectLanguage>()
                    .expect("language item should dispatch SelectLanguage");
                assert_eq!(action.language_id, "zh-CN");
            }
            _ => panic!("expected language action item"),
        }
    }

    #[test]
    fn recent_files_submenu_uses_empty_state_when_history_is_empty() {
        let theme_manager = ThemeManager::default();
        let i18n_manager = I18nManager::default();
        let menus = build_menus(&theme_manager, &i18n_manager, &[]);

        // On macOS: File menu is index 1, Open Recent is item 3 within it.
        // On other platforms: File menu is index 0, Open Recent is item 3.
        #[cfg(target_os = "macos")]
        let recent_menu = submenu(&menus[1].items[3]);
        #[cfg(not(target_os = "macos"))]
        let recent_menu = submenu(&menus[0].items[3]);

        assert_eq!(recent_menu.name.to_string(), "Open Recent");
        assert_eq!(recent_menu.items.len(), 1);
        assert_eq!(action_name(&recent_menu.items[0]), "No Recent Files or Folders");
        match &recent_menu.items[0] {
            MenuItem::Action { action, .. } => {
                assert!(action.as_any().is::<NoRecentFiles>());
            }
            _ => panic!("expected empty recent-file action item"),
        }
    }

    #[test]
    fn recent_menu_entries_list_workspaces_only() {
        // 用户要求：「打开最近」只允许出现工作区（文件夹），不允许出现文件。
        let folders = vec![PathBuf::from("/work/alpha"), PathBuf::from("/work/beta")];
        assert_eq!(recent_menu_entries(&folders), folders);
        let duplicated = vec![PathBuf::from("/work/a"), PathBuf::from("/work/a")];
        assert_eq!(recent_menu_entries(&duplicated), vec![PathBuf::from("/work/a")]);
        let many: Vec<PathBuf> = (0..20).map(|index| PathBuf::from(format!("/w/{index}"))).collect();
        assert_eq!(recent_menu_entries(&many).len(), 15);
    }

    #[test]
    fn recent_menu_never_reads_the_file_history() {
        // 结构守卫：菜单与工作区两个入口都只吃 recent-folders；一旦有人把文件
        // 历史接回「打开最近」，这里就会读到文件历史的读取函数与旧的双表合并。
        let source = include_str!("app_menu.rs");
        // 断言文本里不能出现被搜索的字面量，所以拼出来。
        let file_history_reader = concat!("read_recent_", "files");
        let legacy_merge = concat!("merged_", "recent_entries");
        assert!(
            !source.contains(file_history_reader),
            "app_menu.rs 不应再读文件历史（「打开最近」只列工作区）"
        );
        assert!(
            !source.contains(legacy_merge),
            "旧的「文件+文件夹交错合并」应整体移除"
        );
    }

    #[test]
    fn recent_files_submenu_dispatches_path_actions() {
        let theme_manager = ThemeManager::default();
        let i18n_manager = I18nManager::default();
        let recent_files = vec![
            PathBuf::from(r"C:\docs\one.md"),
            PathBuf::from(r"D:\notes\two.markdown"),
        ];
        let menus = build_menus(&theme_manager, &i18n_manager, &recent_files);

        #[cfg(target_os = "macos")]
        let recent_menu = submenu(&menus[1].items[3]);
        #[cfg(not(target_os = "macos"))]
        let recent_menu = submenu(&menus[0].items[3]);

        assert_eq!(recent_menu.items.len(), 2);
        assert_eq!(action_name(&recent_menu.items[0]), r"C:\docs\one.md");
        match &recent_menu.items[0] {
            MenuItem::Action { action, .. } => {
                let action = action
                    .as_any()
                    .downcast_ref::<OpenRecentFile>()
                    .expect("recent file should dispatch OpenRecentFile");
                assert_eq!(action.path, r"C:\docs\one.md");
            }
            _ => panic!("expected recent-file action item"),
        }
    }

    #[test]
    fn fallback_menu_routes_window_context_actions_without_app_defer() {
        assert!(super::is_window_context_menu_action(&NewWindow));
        assert!(super::is_window_context_menu_action(&OpenFile));
        assert!(super::is_window_context_menu_action(&OpenPreferences));
        assert!(super::is_window_context_menu_action(&OpenRecentFile {
            path: "notes.md".into(),
        }));
        assert!(super::is_window_context_menu_action(&NoRecentFiles));
        assert!(super::is_window_context_menu_action(&AddLanguageConfig));
        assert!(super::is_window_context_menu_action(&AddThemeConfig));
        assert!(super::is_window_context_menu_action(&SaveDocument));
        assert!(super::is_window_context_menu_action(&QuitApplication));
        assert!(super::is_window_context_menu_action(&CloseWindow));
        assert!(!super::is_window_context_menu_action(&SelectTheme {
            theme_id: "velora-dark".into(),
        }));
        assert!(!super::is_window_context_menu_action(&SelectLanguage {
            language_id: "en-US".into(),
        }));
    }

    #[test]
    fn config_import_items_are_bottom_menu_actions() {
        let theme_manager = ThemeManager::default();
        let i18n_manager = I18nManager::default();
        let menus = build_menus(&theme_manager, &i18n_manager, &[]);

        let language_items = &menus[LANGUAGE_IDX].items;
        assert!(matches!(
            language_items[language_items.len() - 2],
            MenuItem::Separator
        ));
        assert_eq!(
            action_name(&language_items[language_items.len() - 1]),
            "Add Language Config"
        );
        match &language_items[language_items.len() - 1] {
            MenuItem::Action { action, .. } => {
                assert!(action.as_any().is::<AddLanguageConfig>());
            }
            _ => panic!("expected add language config action item"),
        }

        let theme_items = &menus[THEME_IDX].items;
        assert_eq!(action_name(&theme_items[0]), "Follow System");
        assert_eq!(action_name(&theme_items[1]), "\u{2713} Dark");
        assert_eq!(action_name(&theme_items[2]), "Light");
        assert!(matches!(
            theme_items[theme_items.len() - 2],
            MenuItem::Separator
        ));
        assert_eq!(
            action_name(&theme_items[theme_items.len() - 1]),
            "Add Theme Config"
        );
        match &theme_items[1] {
            MenuItem::Action { action, .. } => {
                assert!(action.as_any().is::<SelectTheme>());
            }
            _ => panic!("expected select theme action item"),
        }
        match &theme_items[theme_items.len() - 1] {
            MenuItem::Action { action, .. } => {
                assert!(action.as_any().is::<AddThemeConfig>());
            }
            _ => panic!("expected add theme config action item"),
        }
    }

    #[test]
    fn theme_menu_marks_selected_builtin_light_theme() {
        let mut theme_manager = ThemeManager::default();
        assert!(theme_manager.set_theme_by_id("velora-light"));
        let i18n_manager = I18nManager::default();
        let menus = build_menus(&theme_manager, &i18n_manager, &[]);
        let theme_items = &menus[THEME_IDX].items;

        assert_eq!(action_name(&theme_items[0]), "Follow System");
        assert_eq!(action_name(&theme_items[1]), "Dark");
        assert_eq!(action_name(&theme_items[2]), "\u{2713} Light");
        match &theme_items[2] {
            MenuItem::Action { action, .. } => {
                let action = action
                    .as_any()
                    .downcast_ref::<SelectTheme>()
                    .expect("light theme item should dispatch SelectTheme");
                assert_eq!(action.theme_id, "velora-light");
            }
            _ => panic!("expected light theme action item"),
        }
    }

    #[test]
    fn help_menu_omits_upstream_update_action() {
        let theme_manager = ThemeManager::default();
        let i18n_manager = I18nManager::default();
        let menus = build_menus(&theme_manager, &i18n_manager, &[]);
        let help_items = &menus[HELP_IDX].items;

        assert!(help_items.iter().all(|item| match item {
            MenuItem::Action { action, .. } => !action.as_any().is::<CheckForUpdates>(),
            _ => true,
        }));
    }

    #[test]
    #[cfg(not(target_os = "macos"))]
    fn help_menu_contains_about_only() {
        let theme_manager = ThemeManager::default();
        let i18n_manager = I18nManager::default();
        let menus = build_menus(&theme_manager, &i18n_manager, &[]);
        let help_items = &menus[HELP_IDX].items;

        assert_eq!(help_items.len(), 1);
        match &help_items[0] {
            MenuItem::Action { action, .. } => {
                assert!(action.as_any().is::<ShowAbout>());
            }
            _ => panic!("expected about action item"),
        }
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn help_menu_contains_cli_and_about_on_macos() {
        let theme_manager = ThemeManager::default();
        let i18n_manager = I18nManager::default();
        let menus = build_menus(&theme_manager, &i18n_manager, &[]);
        let help_items = &menus[HELP_IDX].items;

        // 安装或卸载命令、分隔线、关于
        assert_eq!(help_items.len(), 3);
        assert!(matches!(help_items[1], MenuItem::Separator));
        match &help_items[2] {
            MenuItem::Action { action, .. } => {
                assert!(action.as_any().is::<ShowAbout>());
            }
            _ => panic!("expected about action item"),
        }
    }
}
