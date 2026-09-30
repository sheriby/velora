use super::*;
use super::command_menus::{command_menu_items, command_spec, file_menu_items};

pub(crate) fn build_menus(
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
pub(super) fn editor_window_for_folder_open(cx: &App) -> Option<WindowHandle<Editor>> {
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

pub(super) fn prompt_and_open_files(cx: &mut App) {
    let error_window = cx.active_window();
    prompt_and_open_files_with_error_window(cx, error_window);
}

pub(super) fn prompt_and_open_folder(cx: &mut App) {
    let error_window = cx.active_window();
    prompt_and_open_folder_with_error_window(cx, error_window);
}

/// 对话框要挑什么。Windows 的 `FOS_PICKFOLDERS` 只能「文件或文件夹」二选一
/// （见 gpui 的 `can_select_mixed_files_and_dirs`），所以两个入口分开。
#[derive(Clone, Copy)]
enum PathPromptKind {
    Files,
    Folders,
}

pub(super) fn prompt_and_open_files_with_error_window(cx: &mut App, error_window: Option<AnyWindowHandle>) {
    prompt_and_open_paths(cx, error_window, PathPromptKind::Files);
}

pub(super) fn prompt_and_open_folder_with_error_window(cx: &mut App, error_window: Option<AnyWindowHandle>) {
    prompt_and_open_paths(cx, error_window, PathPromptKind::Folders);
}

fn prompt_and_open_paths(
    cx: &mut App,
    error_window: Option<AnyWindowHandle>,
    kind: PathPromptKind,
) {
    // 这个字符串是对话框确定铵钮上的字，保持短；增删字前先想一下它在原生窗口里的宽度。
    let prompt_title = match kind {
        PathPromptKind::Files => cx
            .global::<I18nManager>()
            .strings()
            .open_markdown_files_prompt
            .clone(),
        PathPromptKind::Folders => cx
            .global::<I18nManager>()
            .strings()
            .open_folder_prompt
            .clone(),
    };
    // 起始目录＝当前工作区根（或当前文件所在目录）。见 `PathPromptOptions::directory`：
    // 不指定时 Windows 壳层会回到它记住的上次位置，可能是一个已不可达的网络位置。
    let start_dir = editor_window_for_folder_open(cx).and_then(|window| {
        window
            .update(cx, |editor, _window, _cx| editor.open_dialog_start_dir())
            .ok()
            .flatten()
    });
    // 能同时选文件和文件夹的平台（macOS）保留混合选择；Windows 上文件入口
    // 必须是纯文件，否则 `FOS_PICKFOLDERS` 会让对话框只能选文件夹（用户报修）。
    let (files, directories) = match kind {
        PathPromptKind::Files => (true, cx.can_select_mixed_files_and_dirs()),
        PathPromptKind::Folders => (false, true),
    };
    let prompt = cx.prompt_for_paths(PathPromptOptions {
        files,
        directories,
        multiple: true,
        prompt: Some(prompt_title.into()),
        directory: start_dir,
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

pub(super) fn prompt_and_import_language_config(cx: &mut App) {
    let error_window = cx.active_window();
    prompt_and_import_language_config_with_error_window(cx, error_window);
}

pub(super) fn prompt_and_import_language_config_with_error_window(
    cx: &mut App,
    error_window: Option<AnyWindowHandle>,
) {
    let prompt_title = cx
        .global::<I18nManager>()
        .strings()
        .add_language_config_prompt
        .clone();
    let start_dir = config_dialog_start_dir(|dirs| dirs.languages_dir());
    let prompt = cx.prompt_for_paths(PathPromptOptions {
        files: true,
        directories: false,
        multiple: false,
        prompt: Some(prompt_title.into()),
        directory: start_dir,
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

pub(super) fn prompt_and_import_theme_config(cx: &mut App) {
    let error_window = cx.active_window();
    prompt_and_import_theme_config_with_error_window(cx, error_window);
}

pub(super) fn prompt_and_import_theme_config_with_error_window(
    cx: &mut App,
    error_window: Option<AnyWindowHandle>,
) {
    let prompt_title = cx
        .global::<I18nManager>()
        .strings()
        .add_theme_config_prompt
        .clone();
    let start_dir = config_dialog_start_dir(|dirs| dirs.themes_dir());
    let prompt = cx.prompt_for_paths(PathPromptOptions {
        files: true,
        directories: false,
        multiple: false,
        prompt: Some(prompt_title.into()),
        directory: start_dir,
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
    cx.on_action(|_: &OpenFolder, cx| {
        dispatch_menu_action(&OpenFolder, cx);
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
    cx.on_action(|_: &FileHistory, cx| {
        dispatch_menu_action(&FileHistory, cx);
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
