//! Native application menu, app-level actions, and window close routing.
//! 应用名称与命令路径为 velora，更新入口默认隐藏。
//!
//! This module owns menu construction and the actions that operate on the
//! active editor window. The Quit action is routed to the current window so the
//! existing unsaved-changes dialog remains authoritative for that window.

pub(super) use std::path::{Path, PathBuf};

pub(super) use anyhow::Context as _;
pub(super) use futures::channel::oneshot;
pub(super) use gpui::*;

pub(super) use crate::commands::{CommandMenu, CommandSpec};
pub(super) use crate::components::{
    AddLanguageConfig, AddThemeConfig, CheckForUpdates, CloseTab, CloseWindow, ExportHtml,
    ExportPdf,
    ExportPng, FindInDocument, FindNextMatch, FindPreviousMatch, InstallCliTool, NoRecentFiles,
    NewWindow, OpenCommandPalette, OpenFile, OpenFolder, OpenPreferences, OpenRecentFile, PrintDocument,
    FormatDocument, QuitApplication, SaveDocument,
    FileHistory,
    SaveDocumentAs,
    SelectLanguage, SelectTheme, ShowAbout, CopyAsHtml, ToggleFocusMode, ToggleFullscreen,
    ToggleSidebar, ToggleTypewriterMode, ToggleViewMode, UninstallCliTool, ZoomIn, ZoomOut,
    ZoomReset,
};
use crate::config::{
    RecoverySnapshot, apply_configured_language, apply_configured_theme,
    config_dialog_start_dir,
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
/// Restores the last window frame when remembering is enabled; centers the
/// window (keeping the remembered size) when the open position is set to
/// center; only falls back to the configured default size when no frame was
/// remembered.
fn restored_window_bounds(cx: &mut App) -> RestoredWindow {
    let (default_w, default_h) = crate::config::EditorSettings::default_window_size(cx);
    let default_size = size(px(default_w as f32), px(default_h as f32));
    let frame = crate::config::saved_window_frame().ok().flatten();
    let frame_size = frame.map(|frame| {
        size(
            px(frame.width as f32).max(px(480.0)),
            px(frame.height as f32).max(px(320.0)),
        )
    });
    // 「打开位置 = 居中打开」只改位置：大小仍用记住的 frame，「默认窗口尺寸」
    // 只在没有记住 frame 时生效。之前这个模式把大小一起换成默认值，用户设了它
    // 就永远开成默认大小——大小记忆看起来像坏了。
    if crate::config::EditorSettings::window_open_position(cx)
        == crate::config::WindowOpenPosition::Center
    {
        return RestoredWindow {
            bounds: Bounds::centered(None, frame_size.unwrap_or(default_size), cx),
            display_id: None,
        };
    }
    let centered = RestoredWindow {
        bounds: Bounds::centered(None, default_size, cx),
        display_id: None,
    };
    let Some(frame) = frame else {
        return centered;
    };
    let mut bounds = Bounds::new(
        point(px(frame.x as f32), px(frame.y as f32)),
        frame_size.expect("frame size was computed above"),
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
    open_editor_window_from_document(
        cx,
        crate::editor::encoding::LoadedDocument::from_text(markdown),
        file_path,
    )
}

/// 从一次真实读盘开窗：原始字节跟着进编辑器，「打开后没编辑就保存」才可能
/// 一个字节都不改。只有文本可用时（新建窗口、测试）走 `open_editor_window`。
pub(crate) fn open_editor_window_from_document(
    cx: &mut App,
    document: crate::editor::encoding::LoadedDocument,
    file_path: Option<PathBuf>,
) -> WindowHandle<Editor> {
    let RestoredWindow { bounds, display_id } = restored_window_bounds(cx);
    let title = window_title(file_path.as_deref());
    let handle = cx
        .open_window(
            velora_window_options_on_display(title, bounds, display_id),
            move |_window, cx| cx.new(move |cx| Editor::from_loaded_document(cx, document, file_path)),
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
    let document = crate::editor::encoding::load_document(path)
        .with_context(|| format!("failed to read '{}'", path.display()))?;
    open_editor_window_from_document(cx, document, Some(path.to_path_buf()));
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
pub(crate) fn applescript_string_literal(value: &str) -> String {
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
pub(crate) fn recent_menu_entries(folders: &[PathBuf]) -> Vec<PathBuf> {
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
        || action.as_any().is::<FileHistory>()
        || action.as_any().is::<ExportHtml>()
        || action.as_any().is::<ExportPdf>()
        || action.as_any().is::<ExportPng>()
        || action.as_any().is::<PrintDocument>()
        || action.as_any().is::<QuitApplication>()
        || action.as_any().is::<CloseTab>()
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

pub(crate) fn is_window_context_menu_action(action: &dyn Action) -> bool {
    action.as_any().is::<NewWindow>()
        || action.as_any().is::<OpenFile>()
        || action.as_any().is::<OpenFolder>()
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
        if crate::editor::install_pending_update(cx) {
            cx.quit();
        }
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

    if crate::editor::install_pending_update(cx) {
        cx.quit();
    }
}

/// Executes one of the app-menu actions against the current application state.
pub(crate) fn dispatch_menu_action(action: &dyn Action, cx: &mut App) {
    if action.as_any().is::<NewWindow>() {
        open_editor_window(cx, String::new(), None);
    } else if action.as_any().is::<OpenFile>() {
        prompt_and_open_files(cx);
    } else if action.as_any().is::<OpenFolder>() {
        prompt_and_open_folder(cx);
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
        let _ = with_active_editor(cx, |editor, window, cx| {
            editor.advance_search_match(false, window, cx)
        });
    } else if action.as_any().is::<FindPreviousMatch>() {
        let _ = with_active_editor(cx, |editor, window, cx| {
            editor.advance_search_match(true, window, cx)
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

mod build_menus;
mod command_menus;
mod dispatch;
pub(crate) use dispatch::dispatch_menu_action_for_editor;

pub(super) use build_menus::*;
// test-only re-export (build_menus tests alias it)
#[cfg(test)]
pub(crate) use build_menus::build_menus as build_menus_impl;
pub(crate) use build_menus::init;

#[cfg(test)]
mod tests;

