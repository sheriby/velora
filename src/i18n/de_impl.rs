use super::*;

use super::de::I18nStringsDe;
impl I18nStringsDe {
    pub(crate) fn into_strings(self, defaults: I18nStrings) -> I18nStrings {
        I18nStrings {
            dirty_title_marker: self
                .dirty_title_marker
                .unwrap_or(defaults.dirty_title_marker),
            recovered_document_title: self
                .recovered_document_title
                .unwrap_or(defaults.recovered_document_title),
            unsaved_changes_title: self
                .unsaved_changes_title
                .unwrap_or(defaults.unsaved_changes_title),
            unsaved_changes_message: self
                .unsaved_changes_message
                .unwrap_or(defaults.unsaved_changes_message),
            unsaved_changes_save_and_close: self
                .unsaved_changes_save_and_close
                .unwrap_or(defaults.unsaved_changes_save_and_close),
            unsaved_changes_discard_and_close: self
                .unsaved_changes_discard_and_close
                .unwrap_or(defaults.unsaved_changes_discard_and_close),
            unsaved_changes_cancel: self
                .unsaved_changes_cancel
                .unwrap_or(defaults.unsaved_changes_cancel),
            drop_replace_title: self
                .drop_replace_title
                .unwrap_or(defaults.drop_replace_title),
            drop_replace_message: self
                .drop_replace_message
                .unwrap_or(defaults.drop_replace_message),
            drop_replace_save_and_replace: self
                .drop_replace_save_and_replace
                .unwrap_or(defaults.drop_replace_save_and_replace),
            drop_replace_discard_and_replace: self
                .drop_replace_discard_and_replace
                .unwrap_or(defaults.drop_replace_discard_and_replace),
            drop_replace_cancel: self
                .drop_replace_cancel
                .unwrap_or(defaults.drop_replace_cancel),
            drop_no_markdown_file_message: self
                .drop_no_markdown_file_message
                .unwrap_or(defaults.drop_no_markdown_file_message),
            info_dialog_ok: self.info_dialog_ok.unwrap_or(defaults.info_dialog_ok),
            help_check_updates_title: self
                .help_check_updates_title
                .unwrap_or(defaults.help_check_updates_title),
            help_check_updates_message: self
                .help_check_updates_message
                .unwrap_or(defaults.help_check_updates_message),
            update_available_title: self
                .update_available_title
                .unwrap_or(defaults.update_available_title),
            update_available_message_template: self
                .update_available_message_template
                .unwrap_or(defaults.update_available_message_template),
            update_up_to_date_title: self
                .update_up_to_date_title
                .unwrap_or(defaults.update_up_to_date_title),
            update_up_to_date_message_template: self
                .update_up_to_date_message_template
                .unwrap_or(defaults.update_up_to_date_message_template),
            update_failed_title: self
                .update_failed_title
                .unwrap_or(defaults.update_failed_title),
            update_failed_message_template: self
                .update_failed_message_template
                .unwrap_or(defaults.update_failed_message_template),
            update_open_release: self
                .update_open_release
                .unwrap_or(defaults.update_open_release),
            update_later: self.update_later.unwrap_or(defaults.update_later),
            help_about_title: self.help_about_title.unwrap_or(defaults.help_about_title),
            help_about_message: self
                .help_about_message
                .unwrap_or(defaults.help_about_message),
            help_about_github_label: self
                .help_about_github_label
                .unwrap_or(defaults.help_about_github_label),
            help_about_star_message: self
                .help_about_star_message
                .unwrap_or(defaults.help_about_star_message),
            menu_file: self.menu_file.unwrap_or(defaults.menu_file),
            menu_export: self.menu_export.unwrap_or(defaults.menu_export),
            menu_language: self.menu_language.unwrap_or(defaults.menu_language),
            menu_theme: self.menu_theme.unwrap_or(defaults.menu_theme),
            menu_help: self.menu_help.unwrap_or(defaults.menu_help),
            menu_add_language_config: self
                .menu_add_language_config
                .unwrap_or(defaults.menu_add_language_config),
            menu_add_theme_config: self
                .menu_add_theme_config
                .unwrap_or(defaults.menu_add_theme_config),
            menu_new_window: self.menu_new_window.unwrap_or(defaults.menu_new_window),
            menu_close_tab: self.menu_close_tab.unwrap_or(defaults.menu_close_tab),
            menu_close_window: self.menu_close_window.unwrap_or(defaults.menu_close_window),
            menu_open_file: self.menu_open_file.unwrap_or(defaults.menu_open_file),
            menu_open_folder: self.menu_open_folder.unwrap_or(defaults.menu_open_folder),
            menu_open_recent_file: self
                .menu_open_recent_file
                .unwrap_or(defaults.menu_open_recent_file),
            menu_preferences: self.menu_preferences.unwrap_or(defaults.menu_preferences),
            menu_no_recent_files: self
                .menu_no_recent_files
                .unwrap_or(defaults.menu_no_recent_files),
            menu_save: self.menu_save.unwrap_or(defaults.menu_save),
            menu_save_as: self.menu_save_as.unwrap_or(defaults.menu_save_as),
            menu_file_history: self.menu_file_history.unwrap_or(defaults.menu_file_history),
            menu_format_document: self
                .menu_format_document
                .unwrap_or(defaults.menu_format_document),
            image_reveal_in_file_manager: self
                .image_reveal_in_file_manager
                .unwrap_or(defaults.image_reveal_in_file_manager),
            image_copy_address: self
                .image_copy_address
                .unwrap_or(defaults.image_copy_address),
            image_scale: self.image_scale.unwrap_or(defaults.image_scale),
            file_history_empty: self
                .file_history_empty
                .unwrap_or(defaults.file_history_empty),
            menu_quit: self.menu_quit.unwrap_or(defaults.menu_quit),
            menu_export_html: self.menu_export_html.unwrap_or(defaults.menu_export_html),
            menu_export_pdf: self.menu_export_pdf.unwrap_or(defaults.menu_export_pdf),
            menu_export_png: self.menu_export_png.unwrap_or(defaults.menu_export_png),
            menu_print: self.menu_print.unwrap_or(defaults.menu_print),
            menu_open_command_palette: self
                .menu_open_command_palette
                .unwrap_or(defaults.menu_open_command_palette),
            menu_check_updates: self
                .menu_check_updates
                .unwrap_or(defaults.menu_check_updates),
            menu_about: self.menu_about.unwrap_or(defaults.menu_about),
            menu_install_cli_tool: self
                .menu_install_cli_tool
                .unwrap_or(defaults.menu_install_cli_tool),
            menu_uninstall_cli_tool: self
                .menu_uninstall_cli_tool
                .unwrap_or(defaults.menu_uninstall_cli_tool),
            open_markdown_files_prompt: self
                .open_markdown_files_prompt
                .unwrap_or(defaults.open_markdown_files_prompt),
            open_folder_prompt: self
                .open_folder_prompt
                .unwrap_or(defaults.open_folder_prompt),
            add_language_config_prompt: self
                .add_language_config_prompt
                .unwrap_or(defaults.add_language_config_prompt),
            add_theme_config_prompt: self
                .add_theme_config_prompt
                .unwrap_or(defaults.add_theme_config_prompt),
            open_failed_title: self.open_failed_title.unwrap_or(defaults.open_failed_title),
            recent_file_missing_title: self
                .recent_file_missing_title
                .unwrap_or(defaults.recent_file_missing_title),
            recent_file_missing_message_template: self
                .recent_file_missing_message_template
                .unwrap_or(defaults.recent_file_missing_message_template),
            save_failed_title: self.save_failed_title.unwrap_or(defaults.save_failed_title),
            external_change_title: self
                .external_change_title
                .unwrap_or(defaults.external_change_title),
            external_change_message: self
                .external_change_message
                .unwrap_or(defaults.external_change_message),
            external_change_reload: self
                .external_change_reload
                .unwrap_or(defaults.external_change_reload),
            external_change_save_as: self
                .external_change_save_as
                .unwrap_or(defaults.external_change_save_as),
            export_failed_title: self
                .export_failed_title
                .unwrap_or(defaults.export_failed_title),
            image_paste_failed_title: self
                .image_paste_failed_title
                .unwrap_or(defaults.image_paste_failed_title),
            config_import_failed_title: self
                .config_import_failed_title
                .unwrap_or(defaults.config_import_failed_title),
            preferences_window_title: self
                .preferences_window_title
                .unwrap_or(defaults.preferences_window_title),
            preferences_nav_file: self
                .preferences_nav_file
                .unwrap_or(defaults.preferences_nav_file),
            preferences_nav_theme: self
                .preferences_nav_theme
                .unwrap_or(defaults.preferences_nav_theme),
            preferences_nav_image: self
                .preferences_nav_image
                .unwrap_or(defaults.preferences_nav_image),
            preferences_nav_shortcuts: self
                .preferences_nav_shortcuts
                .unwrap_or(defaults.preferences_nav_shortcuts),
            preferences_startup_option: self
                .preferences_startup_option
                .unwrap_or(defaults.preferences_startup_option),
            preferences_startup_new_file: self
                .preferences_startup_new_file
                .unwrap_or(defaults.preferences_startup_new_file),
            preferences_startup_last_opened_file: self
                .preferences_startup_last_opened_file
                .unwrap_or(defaults.preferences_startup_last_opened_file),
            preferences_local_theme: self
                .preferences_local_theme
                .unwrap_or(defaults.preferences_local_theme),
            preferences_theme_system: self
                .preferences_theme_system
                .unwrap_or(defaults.preferences_theme_system),
            preferences_theme_dark: self
                .preferences_theme_dark
                .unwrap_or(defaults.preferences_theme_dark),
            preferences_theme_light: self
                .preferences_theme_light
                .unwrap_or(defaults.preferences_theme_light),
            preferences_image_insert_behavior: self
                .preferences_image_insert_behavior
                .unwrap_or(defaults.preferences_image_insert_behavior),
            preferences_image_paste_none: self
                .preferences_image_paste_none
                .unwrap_or(defaults.preferences_image_paste_none),
            preferences_image_paste_copy_to_document_folder: self
                .preferences_image_paste_copy_to_document_folder
                .unwrap_or(defaults.preferences_image_paste_copy_to_document_folder),
            preferences_image_paste_copy_to_assets_folder: self
                .preferences_image_paste_copy_to_assets_folder
                .unwrap_or(defaults.preferences_image_paste_copy_to_assets_folder),
            preferences_image_paste_copy_to_named_assets_folder: self
                .preferences_image_paste_copy_to_named_assets_folder
                .unwrap_or(defaults.preferences_image_paste_copy_to_named_assets_folder),
            preferences_save: self.preferences_save.unwrap_or(defaults.preferences_save),
            preferences_cancel: self
                .preferences_cancel
                .unwrap_or(defaults.preferences_cancel),
            preferences_save_failed_title: self
                .preferences_save_failed_title
                .unwrap_or(defaults.preferences_save_failed_title),
            preferences_shortcuts_group_file: self
                .preferences_shortcuts_group_file
                .unwrap_or(defaults.preferences_shortcuts_group_file),
            preferences_shortcuts_group_edit: self
                .preferences_shortcuts_group_edit
                .unwrap_or(defaults.preferences_shortcuts_group_edit),
            preferences_shortcuts_group_navigation: self
                .preferences_shortcuts_group_navigation
                .unwrap_or(defaults.preferences_shortcuts_group_navigation),
            preferences_shortcuts_group_formatting: self
                .preferences_shortcuts_group_formatting
                .unwrap_or(defaults.preferences_shortcuts_group_formatting),
            preferences_shortcuts_group_block: self
                .preferences_shortcuts_group_block
                .unwrap_or(defaults.preferences_shortcuts_group_block),
            preferences_shortcuts_group_other: self
                .preferences_shortcuts_group_other
                .unwrap_or(defaults.preferences_shortcuts_group_other),
            preferences_shortcut_record: self
                .preferences_shortcut_record
                .unwrap_or(defaults.preferences_shortcut_record),
            preferences_shortcut_reset: self
                .preferences_shortcut_reset
                .unwrap_or(defaults.preferences_shortcut_reset),
            preferences_shortcut_recording: self
                .preferences_shortcut_recording
                .unwrap_or(defaults.preferences_shortcut_recording),
            preferences_shortcut_conflict_template: self
                .preferences_shortcut_conflict_template
                .unwrap_or(defaults.preferences_shortcut_conflict_template),
            preferences_shortcut_invalid_template: self
                .preferences_shortcut_invalid_template
                .unwrap_or(defaults.preferences_shortcut_invalid_template),
            preferences_shortcut_newline: self
                .preferences_shortcut_newline
                .unwrap_or(defaults.preferences_shortcut_newline),
            preferences_shortcut_delete_back: self
                .preferences_shortcut_delete_back
                .unwrap_or(defaults.preferences_shortcut_delete_back),
            preferences_shortcut_delete: self
                .preferences_shortcut_delete
                .unwrap_or(defaults.preferences_shortcut_delete),
            preferences_shortcut_word_delete_back: self
                .preferences_shortcut_word_delete_back
                .unwrap_or(defaults.preferences_shortcut_word_delete_back),
            preferences_shortcut_word_delete_forward: self
                .preferences_shortcut_word_delete_forward
                .unwrap_or(defaults.preferences_shortcut_word_delete_forward),
            preferences_shortcut_focus_prev: self
                .preferences_shortcut_focus_prev
                .unwrap_or(defaults.preferences_shortcut_focus_prev),
            preferences_shortcut_focus_next: self
                .preferences_shortcut_focus_next
                .unwrap_or(defaults.preferences_shortcut_focus_next),
            preferences_shortcut_move_left: self
                .preferences_shortcut_move_left
                .unwrap_or(defaults.preferences_shortcut_move_left),
            preferences_shortcut_move_right: self
                .preferences_shortcut_move_right
                .unwrap_or(defaults.preferences_shortcut_move_right),
            preferences_shortcut_word_move_left: self
                .preferences_shortcut_word_move_left
                .unwrap_or(defaults.preferences_shortcut_word_move_left),
            preferences_shortcut_word_move_right: self
                .preferences_shortcut_word_move_right
                .unwrap_or(defaults.preferences_shortcut_word_move_right),
            preferences_shortcut_home: self
                .preferences_shortcut_home
                .unwrap_or(defaults.preferences_shortcut_home),
            preferences_shortcut_end: self
                .preferences_shortcut_end
                .unwrap_or(defaults.preferences_shortcut_end),
            preferences_shortcut_block_up: self
                .preferences_shortcut_block_up
                .unwrap_or(defaults.preferences_shortcut_block_up),
            preferences_shortcut_block_down: self
                .preferences_shortcut_block_down
                .unwrap_or(defaults.preferences_shortcut_block_down),
            preferences_shortcut_page_up: self
                .preferences_shortcut_page_up
                .unwrap_or(defaults.preferences_shortcut_page_up),
            preferences_shortcut_page_down: self
                .preferences_shortcut_page_down
                .unwrap_or(defaults.preferences_shortcut_page_down),
            preferences_shortcut_jump_to_top: self
                .preferences_shortcut_jump_to_top
                .unwrap_or(defaults.preferences_shortcut_jump_to_top),
            preferences_shortcut_jump_to_bottom: self
                .preferences_shortcut_jump_to_bottom
                .unwrap_or(defaults.preferences_shortcut_jump_to_bottom),
            preferences_shortcut_select_left: self
                .preferences_shortcut_select_left
                .unwrap_or(defaults.preferences_shortcut_select_left),
            preferences_shortcut_select_right: self
                .preferences_shortcut_select_right
                .unwrap_or(defaults.preferences_shortcut_select_right),
            preferences_shortcut_word_select_left: self
                .preferences_shortcut_word_select_left
                .unwrap_or(defaults.preferences_shortcut_word_select_left),
            preferences_shortcut_word_select_right: self
                .preferences_shortcut_word_select_right
                .unwrap_or(defaults.preferences_shortcut_word_select_right),
            preferences_shortcut_select_home: self
                .preferences_shortcut_select_home
                .unwrap_or(defaults.preferences_shortcut_select_home),
            preferences_shortcut_select_end: self
                .preferences_shortcut_select_end
                .unwrap_or(defaults.preferences_shortcut_select_end),
            preferences_shortcut_select_all: self
                .preferences_shortcut_select_all
                .unwrap_or(defaults.preferences_shortcut_select_all),
            preferences_shortcut_copy: self
                .preferences_shortcut_copy
                .unwrap_or(defaults.preferences_shortcut_copy),
            preferences_shortcut_cut: self
                .preferences_shortcut_cut
                .unwrap_or(defaults.preferences_shortcut_cut),
            preferences_shortcut_paste: self
                .preferences_shortcut_paste
                .unwrap_or(defaults.preferences_shortcut_paste),
            preferences_shortcut_undo: self
                .preferences_shortcut_undo
                .unwrap_or(defaults.preferences_shortcut_undo),
            preferences_shortcut_redo: self
                .preferences_shortcut_redo
                .unwrap_or(defaults.preferences_shortcut_redo),
            preferences_shortcut_bold_selection: self
                .preferences_shortcut_bold_selection
                .unwrap_or(defaults.preferences_shortcut_bold_selection),
            preferences_shortcut_italic_selection: self
                .preferences_shortcut_italic_selection
                .unwrap_or(defaults.preferences_shortcut_italic_selection),
            preferences_shortcut_underline_selection: self
                .preferences_shortcut_underline_selection
                .unwrap_or(defaults.preferences_shortcut_underline_selection),
            preferences_shortcut_code_selection: self
                .preferences_shortcut_code_selection
                .unwrap_or(defaults.preferences_shortcut_code_selection),
            format_bold: self.format_bold.unwrap_or(defaults.format_bold),
            format_italic: self.format_italic.unwrap_or(defaults.format_italic),
            format_underline: self.format_underline.unwrap_or(defaults.format_underline),
            format_code: self.format_code.unwrap_or(defaults.format_code),
            format_highlight: self.format_highlight.unwrap_or(defaults.format_highlight),
            format_strikethrough: self
                .format_strikethrough
                .unwrap_or(defaults.format_strikethrough),
            format_superscript: self
                .format_superscript
                .unwrap_or(defaults.format_superscript),
            format_subscript: self.format_subscript.unwrap_or(defaults.format_subscript),
            preferences_shortcut_indent_block: self
                .preferences_shortcut_indent_block
                .unwrap_or(defaults.preferences_shortcut_indent_block),
            preferences_shortcut_outdent_block: self
                .preferences_shortcut_outdent_block
                .unwrap_or(defaults.preferences_shortcut_outdent_block),
            preferences_shortcut_exit_code_block: self
                .preferences_shortcut_exit_code_block
                .unwrap_or(defaults.preferences_shortcut_exit_code_block),
            preferences_shortcut_save_document: self
                .preferences_shortcut_save_document
                .unwrap_or(defaults.preferences_shortcut_save_document),
            preferences_shortcut_save_document_as: self
                .preferences_shortcut_save_document_as
                .unwrap_or(defaults.preferences_shortcut_save_document_as),
            preferences_shortcut_format_document: self
                .preferences_shortcut_format_document
                .unwrap_or(defaults.preferences_shortcut_format_document),
            preferences_shortcut_new_window: self
                .preferences_shortcut_new_window
                .unwrap_or(defaults.preferences_shortcut_new_window),
            preferences_shortcut_open_file: self
                .preferences_shortcut_open_file
                .unwrap_or(defaults.preferences_shortcut_open_file),
            preferences_shortcut_quit_application: self
                .preferences_shortcut_quit_application
                .unwrap_or(defaults.preferences_shortcut_quit_application),
            preferences_shortcut_close_tab: self
                .preferences_shortcut_close_tab
                .unwrap_or(defaults.preferences_shortcut_close_tab),
            preferences_shortcut_close_window: self
                .preferences_shortcut_close_window
                .unwrap_or(defaults.preferences_shortcut_close_window),
            preferences_shortcut_dismiss_transient_ui: self
                .preferences_shortcut_dismiss_transient_ui
                .unwrap_or(defaults.preferences_shortcut_dismiss_transient_ui),
            preferences_shortcut_toggle_view_mode: self
                .preferences_shortcut_toggle_view_mode
                .unwrap_or(defaults.preferences_shortcut_toggle_view_mode),
            preferences_shortcut_find_in_document: self
                .preferences_shortcut_find_in_document
                .unwrap_or(defaults.preferences_shortcut_find_in_document),
            preferences_shortcut_find_next_match: self
                .preferences_shortcut_find_next_match
                .unwrap_or(defaults.preferences_shortcut_find_next_match),
            preferences_shortcut_find_previous_match: self
                .preferences_shortcut_find_previous_match
                .unwrap_or(defaults.preferences_shortcut_find_previous_match),
            preferences_shortcut_toggle_sidebar: self
                .preferences_shortcut_toggle_sidebar
                .unwrap_or(defaults.preferences_shortcut_toggle_sidebar),
            preferences_shortcut_toggle_fullscreen: self
                .preferences_shortcut_toggle_fullscreen
                .unwrap_or(defaults.preferences_shortcut_toggle_fullscreen),
            workspace_panel_title: self
                .workspace_panel_title
                .unwrap_or(defaults.workspace_panel_title),
            workspace_tab_files: self
                .workspace_tab_files
                .unwrap_or(defaults.workspace_tab_files),
            workspace_backlinks_no_document: self
                .workspace_backlinks_no_document
                .unwrap_or(defaults.workspace_backlinks_no_document),
            workspace_backlinks_empty: self
                .workspace_backlinks_empty
                .unwrap_or(defaults.workspace_backlinks_empty),
            workspace_tags_empty: self
                .workspace_tags_empty
                .unwrap_or(defaults.workspace_tags_empty),
            workspace_tab_outline: self
                .workspace_tab_outline
                .unwrap_or(defaults.workspace_tab_outline),
            workspace_tab_recent: self
                .workspace_tab_recent
                .unwrap_or(defaults.workspace_tab_recent),
            workspace_search_placeholder: self
                .workspace_search_placeholder
                .unwrap_or(defaults.workspace_search_placeholder),
            workspace_document_find_placeholder: self
                .workspace_document_find_placeholder
                .unwrap_or(defaults.workspace_document_find_placeholder),
            workspace_current_document_label: self
                .workspace_current_document_label
                .unwrap_or(defaults.workspace_current_document_label),
            workspace_no_search_results: self
                .workspace_no_search_results
                .unwrap_or(defaults.workspace_no_search_results),
            workspace_no_document_find_results: self
                .workspace_no_document_find_results
                .unwrap_or(defaults.workspace_no_document_find_results),
            workspace_new_file: self
                .workspace_new_file
                .unwrap_or(defaults.workspace_new_file),
            workspace_new_generic_file: self.workspace_new_generic_file.unwrap_or(defaults.workspace_new_generic_file),
            workspace_reveal_in_file_manager: self.workspace_reveal_in_file_manager.unwrap_or(defaults.workspace_reveal_in_file_manager),
            workspace_copy_absolute_path: self.workspace_copy_absolute_path.unwrap_or(defaults.workspace_copy_absolute_path),
            workspace_copy_relative_path: self.workspace_copy_relative_path.unwrap_or(defaults.workspace_copy_relative_path),
            workspace_copy_file_name: self.workspace_copy_file_name.unwrap_or(defaults.workspace_copy_file_name),
            workspace_invalid_name: self.workspace_invalid_name.unwrap_or(defaults.workspace_invalid_name),
            workspace_name_exists: self.workspace_name_exists.unwrap_or(defaults.workspace_name_exists),
            workspace_new_folder: self
                .workspace_new_folder
                .unwrap_or(defaults.workspace_new_folder),
            workspace_rename: self.workspace_rename.unwrap_or(defaults.workspace_rename),
            workspace_delete: self.workspace_delete.unwrap_or(defaults.workspace_delete),
            workspace_delete_confirm_title: self
                .workspace_delete_confirm_title
                .unwrap_or(defaults.workspace_delete_confirm_title),
            workspace_delete_confirm_message: self
                .workspace_delete_confirm_message
                .unwrap_or(defaults.workspace_delete_confirm_message),
            workspace_delete_unsaved_message: self
                .workspace_delete_unsaved_message
                .unwrap_or(defaults.workspace_delete_unsaved_message),
            workspace_no_file_title: self
                .workspace_no_file_title
                .unwrap_or(defaults.workspace_no_file_title),
            workspace_no_file_message: self
                .workspace_no_file_message
                .unwrap_or(defaults.workspace_no_file_message),
            workspace_empty_files: self
                .workspace_empty_files
                .unwrap_or(defaults.workspace_empty_files),
            workspace_empty_outline: self
                .workspace_empty_outline
                .unwrap_or(defaults.workspace_empty_outline),
            workspace_scan_failed_title: self
                .workspace_scan_failed_title
                .unwrap_or(defaults.workspace_scan_failed_title),
            search_replace_placeholder: self
                .search_replace_placeholder
                .unwrap_or(defaults.search_replace_placeholder),
            search_case_sensitive: self
                .search_case_sensitive
                .unwrap_or(defaults.search_case_sensitive),
            search_whole_word: self.search_whole_word.unwrap_or(defaults.search_whole_word),
            search_regex: self.search_regex.unwrap_or(defaults.search_regex),
            search_regex_hint: self.search_regex_hint.unwrap_or(defaults.search_regex_hint),
            search_invalid_pattern: self
                .search_invalid_pattern
                .unwrap_or(defaults.search_invalid_pattern),
            search_fuzzy: self.search_fuzzy.unwrap_or(defaults.search_fuzzy),
            search_fuzzy_short: self
                .search_fuzzy_short
                .unwrap_or(defaults.search_fuzzy_short),
            search_scope_document: self
                .search_scope_document
                .unwrap_or(defaults.search_scope_document),
            search_scope_workspace: self
                .search_scope_workspace
                .unwrap_or(defaults.search_scope_workspace),
            search_replace_current: self
                .search_replace_current
                .unwrap_or(defaults.search_replace_current),
            search_replace_all: self
                .search_replace_all
                .unwrap_or(defaults.search_replace_all),
            search_result_count: self
                .search_result_count
                .unwrap_or(defaults.search_result_count),
            dialog_ok: self.dialog_ok.unwrap_or(defaults.dialog_ok),
            workspace_folder_choice_title: self
                .workspace_folder_choice_title
                .unwrap_or(defaults.workspace_folder_choice_title),
            workspace_replace_current_button: self
                .workspace_replace_current_button
                .unwrap_or(defaults.workspace_replace_current_button),
            workspace_open_new_window_button: self
                .workspace_open_new_window_button
                .unwrap_or(defaults.workspace_open_new_window_button),
            workspace_preview_unavailable_message: self
                .workspace_preview_unavailable_message
                .unwrap_or(defaults.workspace_preview_unavailable_message),
            encoding_not_supported: self
                .encoding_not_supported
                .unwrap_or(defaults.encoding_not_supported),
            welcome_tagline: self.welcome_tagline.unwrap_or(defaults.welcome_tagline),
            welcome_new_document: self
                .welcome_new_document
                .unwrap_or(defaults.welcome_new_document),
            welcome_open: self.welcome_open.unwrap_or(defaults.welcome_open),
            welcome_recent: self.welcome_recent.unwrap_or(defaults.welcome_recent),
            welcome_shortcut_hint: self
                .welcome_shortcut_hint
                .unwrap_or(defaults.welcome_shortcut_hint),
            workspace_folder_entry_label: self
                .workspace_folder_entry_label
                .unwrap_or(defaults.workspace_folder_entry_label),
            quick_open_placeholder: self
                .quick_open_placeholder
                .unwrap_or(defaults.quick_open_placeholder),
            quick_open_no_results: self
                .quick_open_no_results
                .unwrap_or(defaults.quick_open_no_results),
            tree_sort_prefix: self.tree_sort_prefix.unwrap_or(defaults.tree_sort_prefix),
            tree_sort_name: self.tree_sort_name.unwrap_or(defaults.tree_sort_name),
            tree_sort_mtime: self.tree_sort_mtime.unwrap_or(defaults.tree_sort_mtime),
            tree_sort_type: self.tree_sort_type.unwrap_or(defaults.tree_sort_type),
            command_palette_placeholder: self
                .command_palette_placeholder
                .unwrap_or(defaults.command_palette_placeholder),
            command_toggle_focus_mode: self
                .command_toggle_focus_mode
                .unwrap_or(defaults.command_toggle_focus_mode),
            command_toggle_typewriter_mode: self
                .command_toggle_typewriter_mode
                .unwrap_or(defaults.command_toggle_typewriter_mode),
            command_toggle_sidebar: self
                .command_toggle_sidebar
                .unwrap_or(defaults.command_toggle_sidebar),
            command_find_in_document: self
                .command_find_in_document
                .unwrap_or(defaults.command_find_in_document),
            command_find_next: self.command_find_next.unwrap_or(defaults.command_find_next),
            command_find_previous: self
                .command_find_previous
                .unwrap_or(defaults.command_find_previous),
            command_toggle_view_mode: self
                .command_toggle_view_mode
                .unwrap_or(defaults.command_toggle_view_mode),
            command_zoom_in: self.command_zoom_in.unwrap_or(defaults.command_zoom_in),
            command_zoom_out: self.command_zoom_out.unwrap_or(defaults.command_zoom_out),
            command_zoom_reset: self
                .command_zoom_reset
                .unwrap_or(defaults.command_zoom_reset),
            preferences_file_tree_sort: self
                .preferences_file_tree_sort
                .unwrap_or(defaults.preferences_file_tree_sort),
            preferences_file_autosave_debounce: self
                .preferences_file_autosave_debounce
                .unwrap_or(defaults.preferences_file_autosave_debounce),
            preferences_autosave: self
                .preferences_autosave
                .unwrap_or(defaults.preferences_autosave),
            preferences_window_remember_bounds: self
                .preferences_window_remember_bounds
                .unwrap_or(defaults.preferences_window_remember_bounds),
            preferences_file_external_change: self
                .preferences_file_external_change
                .unwrap_or(defaults.preferences_file_external_change),
            preferences_external_change_auto: self
                .preferences_external_change_auto
                .unwrap_or(defaults.preferences_external_change_auto),
            preferences_external_change_manual: self
                .preferences_external_change_manual
                .unwrap_or(defaults.preferences_external_change_manual),
            preferences_file_delete_policy: self
                .preferences_file_delete_policy
                .unwrap_or(defaults.preferences_file_delete_policy),
            preferences_delete_policy_trash: self
                .preferences_delete_policy_trash
                .unwrap_or(defaults.preferences_delete_policy_trash),
            preferences_delete_policy_permanent: self
                .preferences_delete_policy_permanent
                .unwrap_or(defaults.preferences_delete_policy_permanent),
            preferences_smart_punctuation: self
                .preferences_smart_punctuation
                .unwrap_or(defaults.preferences_smart_punctuation),
            status_bar_long_block_source: self
                .status_bar_long_block_source
                .unwrap_or(defaults.status_bar_long_block_source),
            menu_copy_as_html: self.menu_copy_as_html.unwrap_or(defaults.menu_copy_as_html),
            workspace_duplicate: self
                .workspace_duplicate
                .unwrap_or(defaults.workspace_duplicate),
            workspace_copy: self.workspace_copy.unwrap_or(defaults.workspace_copy),
            workspace_paste: self.workspace_paste.unwrap_or(defaults.workspace_paste),
            workspace_paste_empty: self
                .workspace_paste_empty
                .unwrap_or(defaults.workspace_paste_empty),
            hover_footnote_prefix: self
                .hover_footnote_prefix
                .unwrap_or(defaults.hover_footnote_prefix),
            hover_target_exists: self
                .hover_target_exists
                .unwrap_or(defaults.hover_target_exists),
            hover_target_missing: self
                .hover_target_missing
                .unwrap_or(defaults.hover_target_missing),
            tree_filter_placeholder: self
                .tree_filter_placeholder
                .unwrap_or(defaults.tree_filter_placeholder),
            workspace_open_unsupported_message: self
                .workspace_open_unsupported_message
                .unwrap_or(defaults.workspace_open_unsupported_message),
            tab_close: self.tab_close.unwrap_or(defaults.tab_close),
            tab_close_others: self.tab_close_others.unwrap_or(defaults.tab_close_others),
            tab_close_left: self.tab_close_left.unwrap_or(defaults.tab_close_left),
            tab_close_right: self.tab_close_right.unwrap_or(defaults.tab_close_right),
            tab_close_all: self.tab_close_all.unwrap_or(defaults.tab_close_all),
            tab_close_dirty_title: self
                .tab_close_dirty_title
                .unwrap_or(defaults.tab_close_dirty_title),
            tab_close_dirty_message_one: self
                .tab_close_dirty_message_one
                .unwrap_or(defaults.tab_close_dirty_message_one),
            tab_close_dirty_message_many: self
                .tab_close_dirty_message_many
                .unwrap_or(defaults.tab_close_dirty_message_many),
            open_link_title: self.open_link_title.unwrap_or(defaults.open_link_title),
            open_link_open: self.open_link_open.unwrap_or(defaults.open_link_open),
            open_link_cancel: self.open_link_cancel.unwrap_or(defaults.open_link_cancel),
            view_mode_source: self.view_mode_source.unwrap_or(defaults.view_mode_source),
            view_mode_switch_to_source: self
                .view_mode_switch_to_source
                .unwrap_or(defaults.view_mode_switch_to_source),
            view_mode_rendered: self
                .view_mode_rendered
                .unwrap_or(defaults.view_mode_rendered),
            view_mode_switch_to_rendered: self
                .view_mode_switch_to_rendered
                .unwrap_or(defaults.view_mode_switch_to_rendered),
            source_mode_fallback_message: self
                .source_mode_fallback_message
                .unwrap_or(defaults.source_mode_fallback_message),
            context_menu_toggle_source_view: self
                .context_menu_toggle_source_view
                .unwrap_or(defaults.context_menu_toggle_source_view),
            context_menu_undo: self.context_menu_undo.unwrap_or(defaults.context_menu_undo),
            context_menu_redo: self.context_menu_redo.unwrap_or(defaults.context_menu_redo),
            context_menu_cut: self.context_menu_cut.unwrap_or(defaults.context_menu_cut),
            context_menu_copy: self.context_menu_copy.unwrap_or(defaults.context_menu_copy),
            context_menu_paste_as_plain_text: self
                .context_menu_paste_as_plain_text
                .unwrap_or(defaults.context_menu_paste_as_plain_text),
            context_menu_copy_as_markdown: self
                .context_menu_copy_as_markdown
                .unwrap_or(defaults.context_menu_copy_as_markdown),
            context_menu_paste: self
                .context_menu_paste
                .unwrap_or(defaults.context_menu_paste),
            context_menu_format: self
                .context_menu_format
                .unwrap_or(defaults.context_menu_format),
            context_menu_paragraph: self
                .context_menu_paragraph
                .unwrap_or(defaults.context_menu_paragraph),
            context_menu_insert: self
                .context_menu_insert
                .unwrap_or(defaults.context_menu_insert),
            context_menu_table: self
                .context_menu_table
                .unwrap_or(defaults.context_menu_table),
            paragraph_heading1: self
                .paragraph_heading1
                .unwrap_or(defaults.paragraph_heading1),
            paragraph_heading2: self
                .paragraph_heading2
                .unwrap_or(defaults.paragraph_heading2),
            paragraph_heading3: self
                .paragraph_heading3
                .unwrap_or(defaults.paragraph_heading3),
            paragraph_heading4: self
                .paragraph_heading4
                .unwrap_or(defaults.paragraph_heading4),
            paragraph_heading5: self
                .paragraph_heading5
                .unwrap_or(defaults.paragraph_heading5),
            paragraph_heading6: self
                .paragraph_heading6
                .unwrap_or(defaults.paragraph_heading6),
            paragraph_normal_text: self
                .paragraph_normal_text
                .unwrap_or(defaults.paragraph_normal_text),
            paragraph_bullet_list: self
                .paragraph_bullet_list
                .unwrap_or(defaults.paragraph_bullet_list),
            paragraph_numbered_list: self
                .paragraph_numbered_list
                .unwrap_or(defaults.paragraph_numbered_list),
            paragraph_task_list: self
                .paragraph_task_list
                .unwrap_or(defaults.paragraph_task_list),
            paragraph_quote: self.paragraph_quote.unwrap_or(defaults.paragraph_quote),
            paragraph_code_block: self
                .paragraph_code_block
                .unwrap_or(defaults.paragraph_code_block),
            insert_link: self.insert_link.unwrap_or(defaults.insert_link),
            format_clear: self.format_clear.unwrap_or(defaults.format_clear),
            format_clear_unavailable: self
                .format_clear_unavailable
                .unwrap_or(defaults.format_clear_unavailable),
            insert_image: self.insert_image.unwrap_or(defaults.insert_image),
            insert_image_prompt: self
                .insert_image_prompt
                .unwrap_or(defaults.insert_image_prompt),
            insert_math_block: self.insert_math_block.unwrap_or(defaults.insert_math_block),
            insert_formula: self.insert_formula.unwrap_or(defaults.insert_formula),
            formula_editor_apply: self.formula_editor_apply.unwrap_or(defaults.formula_editor_apply),
            formula_editor_cancel: self
                .formula_editor_cancel
                .unwrap_or(defaults.formula_editor_cancel),
            formula_editor_hint: self.formula_editor_hint.unwrap_or(defaults.formula_editor_hint),
            formula_editor_placeholder: self
                .formula_editor_placeholder
                .unwrap_or(defaults.formula_editor_placeholder),
            formula_editor_preview_empty: self
                .formula_editor_preview_empty
                .unwrap_or(defaults.formula_editor_preview_empty),
            latex_category_greek: self
                .latex_category_greek
                .unwrap_or(defaults.latex_category_greek),
            latex_category_operators: self
                .latex_category_operators
                .unwrap_or(defaults.latex_category_operators),
            latex_category_arrows: self
                .latex_category_arrows
                .unwrap_or(defaults.latex_category_arrows),
            latex_category_structures: self
                .latex_category_structures
                .unwrap_or(defaults.latex_category_structures),
            latex_category_functions: self
                .latex_category_functions
                .unwrap_or(defaults.latex_category_functions),
            latex_category_symbols: self
                .latex_category_symbols
                .unwrap_or(defaults.latex_category_symbols),
            insert_separator: self.insert_separator.unwrap_or(defaults.insert_separator),
            insert_toc: self.insert_toc.unwrap_or(defaults.insert_toc),
            insert_front_matter: self
                .insert_front_matter
                .unwrap_or(defaults.insert_front_matter),
            table_axis_align_column_left: self
                .table_axis_align_column_left
                .unwrap_or(defaults.table_axis_align_column_left),
            table_axis_align_column_center: self
                .table_axis_align_column_center
                .unwrap_or(defaults.table_axis_align_column_center),
            table_axis_align_column_right: self
                .table_axis_align_column_right
                .unwrap_or(defaults.table_axis_align_column_right),
            table_axis_move_column_left: self
                .table_axis_move_column_left
                .unwrap_or(defaults.table_axis_move_column_left),
            table_axis_move_column_right: self
                .table_axis_move_column_right
                .unwrap_or(defaults.table_axis_move_column_right),
            table_axis_delete_column: self
                .table_axis_delete_column
                .unwrap_or(defaults.table_axis_delete_column),
            table_axis_move_row_up: self
                .table_axis_move_row_up
                .unwrap_or(defaults.table_axis_move_row_up),
            table_axis_move_row_down: self
                .table_axis_move_row_down
                .unwrap_or(defaults.table_axis_move_row_down),
            table_axis_delete_row: self
                .table_axis_delete_row
                .unwrap_or(defaults.table_axis_delete_row),
            table_header_row: self.table_header_row.unwrap_or(defaults.table_header_row),
            table_insert_title: self
                .table_insert_title
                .unwrap_or(defaults.table_insert_title),
            table_insert_description: self
                .table_insert_description
                .unwrap_or(defaults.table_insert_description),
            table_insert_body_rows: self
                .table_insert_body_rows
                .unwrap_or(defaults.table_insert_body_rows),
            table_insert_columns: self
                .table_insert_columns
                .unwrap_or(defaults.table_insert_columns),
            table_insert_cancel: self
                .table_insert_cancel
                .unwrap_or(defaults.table_insert_cancel),
            table_insert_confirm: self
                .table_insert_confirm
                .unwrap_or(defaults.table_insert_confirm),
            image_placeholder: self.image_placeholder.unwrap_or(defaults.image_placeholder),
            image_loading_without_alt: self
                .image_loading_without_alt
                .unwrap_or(defaults.image_loading_without_alt),
            image_loading_with_alt_template: self
                .image_loading_with_alt_template
                .unwrap_or(defaults.image_loading_with_alt_template),
            image_load_failed: self.image_load_failed.unwrap_or(defaults.image_load_failed),
            code_language_placeholder: self
                .code_language_placeholder
                .unwrap_or(defaults.code_language_placeholder),
            code_copy_button: self.code_copy_button.unwrap_or(defaults.code_copy_button),
            status_bar_files: self.status_bar_files.unwrap_or(defaults.status_bar_files),
            status_bar_mode_source: self
                .status_bar_mode_source
                .unwrap_or(defaults.status_bar_mode_source),
            status_bar_mode_rendered: self
                .status_bar_mode_rendered
                .unwrap_or(defaults.status_bar_mode_rendered),
            status_bar_word_count_suffix: self
                .status_bar_word_count_suffix
                .unwrap_or(defaults.status_bar_word_count_suffix),
            status_bar_reading_time_suffix: self
                .status_bar_reading_time_suffix
                .unwrap_or(defaults.status_bar_reading_time_suffix),
            preferences_nav_status_bar: self
                .preferences_nav_status_bar
                .unwrap_or(defaults.preferences_nav_status_bar),
            preferences_nav_window: self
                .preferences_nav_window
                .unwrap_or(defaults.preferences_nav_window),
            preferences_window_zoom: self
                .preferences_window_zoom
                .unwrap_or(defaults.preferences_window_zoom),
            preferences_window_default_size: self
                .preferences_window_default_size
                .unwrap_or(defaults.preferences_window_default_size),
            preferences_window_open_position: self
                .preferences_window_open_position
                .unwrap_or(defaults.preferences_window_open_position),
            preferences_window_open_position_remember: self
                .preferences_window_open_position_remember
                .unwrap_or(defaults.preferences_window_open_position_remember),
            preferences_window_open_position_center: self
                .preferences_window_open_position_center
                .unwrap_or(defaults.preferences_window_open_position_center),
            preferences_status_bar_enabled: self
                .preferences_status_bar_enabled
                .unwrap_or(defaults.preferences_status_bar_enabled),
            preferences_status_bar_show_word_count: self
                .preferences_status_bar_show_word_count
                .unwrap_or(defaults.preferences_status_bar_show_word_count),
            preferences_status_bar_show_cursor_position: self
                .preferences_status_bar_show_cursor_position
                .unwrap_or(defaults.preferences_status_bar_show_cursor_position),
            preferences_status_bar_show_sidebar_toggle: self
                .preferences_status_bar_show_sidebar_toggle
                .unwrap_or(defaults.preferences_status_bar_show_sidebar_toggle),
            preferences_status_bar_show_mode_switch: self
                .preferences_status_bar_show_mode_switch
                .unwrap_or(defaults.preferences_status_bar_show_mode_switch),
        }
    }
}
