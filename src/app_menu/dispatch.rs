use super::*;

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
    } else if action.as_any().is::<OpenFolder>() {
        prompt_and_open_folder_with_error_window(cx, current_window);
    } else if action.as_any().is::<ToggleViewMode>() {
        let _ = target.update(cx, |editor, cx| editor.toggle_view_mode_from_ui(cx));
    } else if action.as_any().is::<ToggleFocusMode>() {
        let _ = target.update(cx, |editor, cx| editor.toggle_focus_mode(cx));
    } else if action.as_any().is::<ToggleTypewriterMode>() {
        let _ = target.update(cx, |editor, cx| editor.toggle_typewriter_mode(cx));
    } else if action.as_any().is::<FindInDocument>() {
        let _ = target.update(cx, |editor, cx| editor.open_document_find(cx));
    } else if action.as_any().is::<OpenAiAssistant>() {
        let _ = target.update(cx, |editor, cx| editor.toggle_ai_assistant(window, cx));
    } else if action.as_any().is::<FindNextMatch>() {
        let _ = target.update(cx, |editor, cx| {
            editor.advance_search_match(false, window, cx)
        });
    } else if action.as_any().is::<FindPreviousMatch>() {
        let _ = target.update(cx, |editor, cx| {
            editor.advance_search_match(true, window, cx)
        });
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
    } else if action.as_any().is::<FormatDocument>() {
        let _ = target.update(cx, |editor, cx| editor.format_document(cx));
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

