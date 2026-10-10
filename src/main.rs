//! velora - a block-based Markdown editor built with GPUI.
//! 应用入口与命令名称为 velora。
//!
//! Reads file paths from command-line arguments and opens one GPUI window per
//! file. With no arguments, a single empty window is created.

#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
// editor/workspace 的测试模块类型层级较深，默认 128 会爆递归限制。
#![recursion_limit = "256"]

use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[cfg(target_os = "macos")]
use futures::{StreamExt, channel::mpsc};
use gpui::*;

mod app_identity;
mod app_menu;
mod commands;
mod components;
mod config;
mod editor;
mod export;
mod file_url;
mod i18n;
mod net;
mod theme;
mod window_chrome;

use app_menu::{
    init as init_app_menu, open_editor_window, open_editor_window_from_document,
    open_workspace_window, restore_last_session,
};
use components::init_with_keybindings as init_editor;
#[cfg(target_os = "macos")]
use file_url::parse_file_url;
use i18n::I18nManager;
use theme::ThemeManager;

/// Applies the velora.png artwork as the macOS Dock icon. Packaged builds
/// already carry it through the bundle's icns; this covers bare `cargo run`
/// launches where no bundle icon exists.
#[cfg(target_os = "macos")]
// objc 0.2's msg_send/class macros expand cfg(cargo-clippy) checks that
// newer rustc flags as unexpected cfg values; the lint fires inside the
// macro, so silence it at this function.
#[allow(unexpected_cfgs)]
fn apply_dock_icon() {
    use objc::class;
    use objc::msg_send;
    use objc::sel;
    use objc::sel_impl;

    let bytes = include_bytes!("../assets/icon/velora.png");
    unsafe {
        let data: *mut objc::runtime::Object = msg_send![class!(NSData),
            dataWithBytes: bytes.as_ptr() as *const std::ffi::c_void
            length: bytes.len()
        ];
        if data.is_null() {
            return;
        }
        let alloc: *mut objc::runtime::Object = msg_send![class!(NSImage), alloc];
        let image: *mut objc::runtime::Object = msg_send![alloc, initWithData: data];
        if image.is_null() {
            return;
        }
        let app: *mut objc::runtime::Object =
            msg_send![class!(NSApplication), sharedApplication];
        if app.is_null() {
            return;
        }
        let _: () = msg_send![app, setApplicationIconImage: image];
    }
}

struct VeloraAssets;

fn open_startup_window(cx: &mut App, startup_open: config::StartupOpenPreference) {
    if startup_open == config::StartupOpenPreference::LastOpenedFile
        && let Some(path) = config::first_existing_recent_markdown_file()
    {
        match crate::editor::encoding::load_document(&path) {
            Ok(document) => {
                open_editor_window_from_document(cx, document, Some(path));
                return;
            }
            Err(err) => {
                eprintln!(
                    "failed to read last opened file '{}': {err}",
                    path.display()
                );
            }
        }
    }

    let handle = open_editor_window(cx, String::new(), None);
    handle
        .update(cx, |editor, _window, _cx| {
            editor.show_welcome = true;
        })
        .ok();
}

fn restore_recovery_windows(cx: &mut App, restored: &AtomicBool) {
    if restored.swap(true, Ordering::SeqCst) {
        return;
    }
    let snapshots = match config::read_recovery_snapshots() {
        Ok(snapshots) => snapshots,
        Err(error) => {
            eprintln!("failed to read recovery snapshots: {error}");
            return;
        }
    };
    for snapshot in snapshots {
        if snapshot
            .source_path
            .as_ref()
            .and_then(|path| crate::editor::encoding::read_document_string(path).ok())
            .is_some_and(|markdown| markdown == snapshot.markdown)
        {
            if let Err(error) = config::remove_recovery_snapshot(snapshot.id) {
                eprintln!("failed to remove completed recovery snapshot: {error}");
            }
            continue;
        }
        // 会话里已打开同一文件时，把未保存内容合并进该标签而不是另开窗口
        // （roadmap E10）。
        if let Some(path) = snapshot.source_path.clone()
            && merge_snapshot_into_open_session(cx, &path, &snapshot.markdown, snapshot.id)
        {
            continue;
        }
        app_menu::open_recovered_editor_window(cx, snapshot);
    }
}

/// 关闭仍是空白欢迎态的启动窗口（未打开文件、未编辑、无标签），
/// 供 Finder/`open` 文件事件到达时让位（roadmap G5）。
#[cfg(target_os = "macos")]
fn close_pristine_startup_windows(cx: &mut App) {
    let handles = cx.windows();
    for handle in handles {
        let Some(editor) = handle.downcast::<components::Editor>() else {
            continue;
        };
        let pristine = editor
            .update(cx, |editor, window, _cx| {
                let pristine = editor.is_pristine_startup_window();
                if pristine {
                    window.remove_window();
                }
                pristine
            })
            .unwrap_or(false);
        if pristine {
            return;
        }
    }
}

/// 把恢复快照并入已打开该文件路径的会话窗口（roadmap E10）。
fn merge_snapshot_into_open_session(
    cx: &mut App,
    path: &std::path::Path,
    markdown: &str,
    recovery_id: uuid::Uuid,
) -> bool {
    for handle in cx.windows() {
        let Some(editor) = handle.downcast::<components::Editor>() else {
            continue;
        };
        let merged = editor
            .update(cx, |editor, window, cx| {
                editor
                    .workspace_open_document_paths()
                    .iter()
                    .any(|open| open == path)
                    && editor.merge_recovery_snapshot(path, markdown, recovery_id, window, cx)
            })
            .unwrap_or(false);
        if merged {
            return true;
        }
    }
    false
}

impl AssetSource for VeloraAssets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        match path {
            "icon/workspace/folder.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/workspace/folder.svg"
            )))),
            "icon/workspace/activity-files.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/workspace/activity-files.svg"
            )))),
            "icon/workspace/activity-search.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/workspace/activity-search.svg"
            )))),
            "icon/workspace/activity-outline.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/workspace/activity-outline.svg"
            )))),
            "icon/workspace/activity-backlinks.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/workspace/activity-backlinks.svg"
            )))),
            "icon/workspace/activity-tags.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/workspace/activity-tags.svg"
            )))),
            "icon/workspace/chevron-right.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/workspace/chevron-right.svg"
            )))),
            "icon/workspace/chevron-down.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/workspace/chevron-down.svg"
            )))),
            "icon/workspace/markdown.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/workspace/markdown.svg"
            )))),
            "icon/workspace/code.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/workspace/code.svg"
            )))),
            "icon/workspace/view-source.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/workspace/view-source.svg"
            )))),
            "icon/workspace/open-folder.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/workspace/open-folder.svg"
            )))),
            "icon/workspace/new-file.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/workspace/new-file.svg"
            )))),
            "icon/workspace/copy.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/workspace/copy.svg"
            )))),
            "icon/workspace/replace.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/workspace/replace.svg"
            )))),
            "icon/workspace/replace-all.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/workspace/replace-all.svg"
            )))),
            "icon/workspace/check.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/workspace/check.svg"
            )))),
            "icon/workspace/new-folder.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/workspace/new-folder.svg"
            )))),
            "icon/workspace/rename.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/workspace/rename.svg"
            )))),
            "icon/workspace/delete.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/workspace/delete.svg"
            )))),
            "icon/workspace/tab-close.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/workspace/tab-close.svg"
            )))),
            "icon/workspace/generic-file.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/workspace/generic-file.svg"
            )))),
            "icon/editor/paragraph.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/editor/paragraph.svg"
            )))),
            "icon/editor/link.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/editor/link.svg"
            )))),
            "icon/editor/format.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/editor/format.svg"
            )))),
            "icon/editor/insert.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/editor/insert.svg"
            )))),
            "icon/editor/undo.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/editor/undo.svg"
            )))),
            "icon/editor/redo.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/editor/redo.svg"
            )))),
            "icon/editor/cut.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/editor/cut.svg"
            )))),
            "icon/editor/copy.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/editor/copy.svg"
            )))),
            "icon/editor/paste.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/editor/paste.svg"
            )))),
            "icon/editor/paste-plain.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/editor/paste-plain.svg"
            )))),
            "icon/editor/copy-markdown.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/editor/copy-markdown.svg"
            )))),
            "icon/editor/copy-html.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/editor/copy-html.svg"
            )))),
            "icon/editor/toggle-source.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/editor/toggle-source.svg"
            )))),
            "icon/velora.png" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/velora.png"
            )))),
            "icon/titlebar/chrome-close.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/titlebar/chrome-close.svg"
            )))),
            "icon/titlebar/chrome-minimize.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/titlebar/chrome-minimize.svg"
            )))),
            "icon/titlebar/chrome-maximize.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/titlebar/chrome-maximize.svg"
            )))),
            "icon/titlebar/chrome-restore.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/titlebar/chrome-restore.svg"
            )))),
            "icon/titlebar/menu-hamburger.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/titlebar/menu-hamburger.svg"
            )))),
            _ => Ok(None),
        }
    }

    fn list(&self, _path: &str) -> gpui::Result<Vec<SharedString>> {
        Ok(Vec::new())
    }
}

/// 启动计时（roadmap G5）：设 VELORA_STARTUP_TIMING=1 时把各阶段耗时打到 stderr。
fn startup_timing_enabled() -> bool {
    std::env::var_os("VELORA_STARTUP_TIMING").is_some_and(|value| value != "0")
}

fn log_startup_phase(start: std::time::Instant, phase: &str) {
    if startup_timing_enabled() {
        eprintln!(
            "[startup] {phase}: {:.1}ms",
            start.elapsed().as_secs_f64() * 1000.0
        );
    }
}

fn main() {
    let startup_start = std::time::Instant::now();
    let args: Vec<String> = std::env::args().collect();

    // Parse command-line arguments
    let mut detach = false;
    let mut input_paths = Vec::new();

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--version" | "-v" => {
                println!("velora {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            "--help" | "-h" => {
                println!(
                    "velora {} - A block-based Markdown editor",
                    env!("CARGO_PKG_VERSION")
                );
                println!();
                println!("USAGE:");
                println!("    velora [OPTIONS] [FILES...]");
                println!();
                println!("OPTIONS:");
                println!("    -v, --version    Print version information");
                println!("    -h, --help       Print this help message");
                println!("    -d, --detach     Launch in background (non-blocking)");
                println!();
                println!("FILES:");
                println!("    One or more markdown files to open. If no files are specified,");
                println!("    opens an empty document.");
                return;
            }
            "--detach" | "-d" => {
                detach = true;
            }
            option if option.starts_with('-') => {
                eprintln!("Unknown option: {}", option);
                std::process::exit(1);
            }
            path => {
                input_paths.push(PathBuf::from(path));
            }
        }
        i += 1;
    }

    #[cfg(not(target_os = "macos"))]
    let _ = detach;

    // On macOS, detach from terminal if requested
    // TODO: Other platforms may also need to be adapted
    #[cfg(target_os = "macos")]
    if detach {
        use std::process::Command;

        // Re-launch the application in the background without the --detach flag
        let exe_path = std::env::current_exe().expect("Failed to get executable path");
        let non_detach_args: Vec<String> = args
            .iter()
            .filter(|arg| *arg != "--detach" && *arg != "-d")
            .cloned()
            .collect();

        Command::new(exe_path)
            .args(&non_detach_args[1..])
            .spawn()
            .expect("Failed to detach process");

        return;
    }

    #[cfg(target_os = "macos")]
    let (open_file_tx, mut open_file_rx) = mpsc::unbounded::<PathBuf>();
    #[cfg(target_os = "macos")]
    let open_file_requested = Arc::new(AtomicBool::new(false));

    let app = Application::new().with_assets(VeloraAssets);

    #[cfg(target_os = "macos")]
    {
        let open_file_requested_for_callback = open_file_requested.clone();
        app.on_open_urls(move |urls| {
            for url in urls {
                let Some(path) = parse_file_url(&url) else {
                    continue;
                };
                open_file_requested_for_callback.store(true, Ordering::SeqCst);
                let _ = open_file_tx.unbounded_send(path);
            }
        });
    }

    app.run(move |cx: &mut App| {
        #[cfg(target_os = "macos")]
        apply_dock_icon();
        log_startup_phase(startup_start, "app.run entered");
        let preferences = config::load_or_create_app_preferences().unwrap_or_else(|err| {
            eprintln!("failed to initialize app preferences: {err}");
            Default::default()
        });
        log_startup_phase(startup_start, "preferences loaded");
        I18nManager::init_with_language_id(cx, &preferences.default_language_id);
        ThemeManager::init_with_theme_id(cx, &preferences.default_theme_id);
        config::EditorSettings::init(cx, preferences.show_table_headers);
        log_startup_phase(startup_start, "i18n/theme/settings ready");
        net::install_http_client(cx);
        init_editor(cx, &preferences.keybindings);
        log_startup_phase(startup_start, "editor installed");
        // 菜单栏在首帧之后再安装：macOS 菜单不需要在窗口出现前就绪，
        // 延后一个事件循环省下菜单构建时间（roadmap G5）。
        cx.spawn(async move |cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(16))
                .await;
            let _ = cx.update(|cx| init_app_menu(cx));
        })
        .detach();
        let recovery_windows_restored = Arc::new(AtomicBool::new(false));

        #[cfg(target_os = "macos")]
        let recovery_windows_restored_for_open = recovery_windows_restored.clone();
        #[cfg(target_os = "macos")]
        cx.spawn(async move |cx| {
            while let Some(path) = open_file_rx.next().await {
                let recovery_windows_restored = recovery_windows_restored_for_open.clone();
                let _ = cx.update(move |cx| {
                    close_pristine_startup_windows(cx);
                    if let Err(err) = app_menu::open_file_in_new_window(cx, &path) {
                        eprintln!("failed to open '{}': {err}", path.display());
                        app_menu::report_open_failure(cx, &err);
                    }
                    restore_recovery_windows(cx, &recovery_windows_restored);
                });
            }
        })
        .detach();

        if input_paths.is_empty() {
            #[cfg(target_os = "macos")]
            {
                // 直接开窗（roadmap G5：不再等 150ms 的 open-file 宽限期）；
                // 若随后收到 Finder/`open` 的文件事件，先关掉这个未被使用的
                // 启动窗口再打开目标文件，语义与原来的宽限期一致。
                let startup_open = preferences.startup_open;
                if !restore_last_session(cx) {
                    open_startup_window(cx, startup_open);
                }
                restore_recovery_windows(cx, &recovery_windows_restored);
                log_startup_phase(startup_start, "first window opened");
            }

            #[cfg(not(target_os = "macos"))]
            {
                if !restore_last_session(cx) {
                    open_startup_window(cx, preferences.startup_open);
                }
                restore_recovery_windows(cx, &recovery_windows_restored);
                log_startup_phase(startup_start, "first window opened");
            }

            return;
        }

        for path in &input_paths {
            let absolute_path = if path.is_absolute() {
                path.clone()
            } else {
                match std::env::current_dir() {
                    Ok(cwd) => cwd.join(path),
                    Err(_) => path.clone(),
                }
            };

            if absolute_path.is_dir() {
                if let Err(err) = open_workspace_window(cx, absolute_path) {
                    eprintln!("failed to open workspace: {err}");
                }
                continue;
            }

            // 读盘失败（含「编码不能无损写回」这一类拒绝）时**不能**把这个路径交给
            // 一份空文档：按那个路径保存就是把用户的文件覆盖成空白，和报修第 1 条
            // 是同一类破坏。于是开一份未命名空文档，理由走应用内模态（禁系统原生弹窗）。
            let (document, open_path, failure) =
                match crate::editor::encoding::load_document(&absolute_path) {
                    Ok(document) => {
                        if let Err(err) = config::record_recent_file(&absolute_path) {
                            eprintln!("failed to update recent file history: {err}");
                        }
                        (document, Some(absolute_path), None)
                    }
                    Err(err) => {
                        eprintln!("failed to open '{}': {err}", absolute_path.display());
                        (
                            crate::editor::encoding::LoadedDocument::from_text(String::new()),
                            None,
                            Some(err.to_string()),
                        )
                    }
                };
            let handle = open_editor_window_from_document(cx, document, open_path);
            if let Some(detail) = failure {
                let title = cx
                    .global::<I18nManager>()
                    .strings()
                    .open_failed_title
                    .clone();
                let _ = handle.update(cx, |editor, _window, cx| {
                    editor.show_message_modal(title, detail, cx);
                });
            }
        }
        restore_recovery_windows(cx, &recovery_windows_restored);
        app_menu::install_menus(cx);
        cx.refresh_windows();
        log_startup_phase(startup_start, "first window opened");
    });
}
