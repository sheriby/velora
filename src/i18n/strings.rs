use super::*;

/// All localisable UI strings for the editor.
#[derive(Debug, Clone, Serialize)]
pub struct I18nStrings {
    /// Marker prepended to the window title when the document is dirty.
    pub dirty_title_marker: String,
    /// Label used for a document restored from an unexpected exit.
    pub recovered_document_title: String,
    /// Title of the unsaved-changes dialog.
    pub unsaved_changes_title: String,
    /// Body message of the unsaved-changes dialog.
    pub unsaved_changes_message: String,
    /// Label for the "save and close" button.
    pub unsaved_changes_save_and_close: String,
    /// Label for the "discard and close" button.
    pub unsaved_changes_discard_and_close: String,
    /// Label for the "keep editing" button.
    pub unsaved_changes_cancel: String,
    /// Title of the dropped-file replacement dialog.
    pub drop_replace_title: String,
    /// Body message of the dropped-file replacement dialog.
    pub drop_replace_message: String,
    /// Label for saving before replacing the current document.
    pub drop_replace_save_and_replace: String,
    /// Label for replacing the current document without saving.
    pub drop_replace_discard_and_replace: String,
    /// Label for cancelling a dropped-file replacement.
    pub drop_replace_cancel: String,
    /// Prompt detail shown when no supported Markdown file was dropped.
    pub drop_no_markdown_file_message: String,
    /// Label for dismissing simple informational dialogs.
    pub info_dialog_ok: String,
    /// Title of the placeholder update-check dialog.
    pub help_check_updates_title: String,
    /// Body text shown while an update check is running.
    pub help_check_updates_message: String,
    /// Title shown when a newer version is available.
    pub update_available_title: String,
    /// Message template for newer-version prompts. Supports `{current}` and `{latest}`.
    pub update_available_message_template: String,
    /// Title shown when the running app is already current.
    pub update_up_to_date_title: String,
    /// Message template for up-to-date prompts. Supports `{current}` and `{latest}`.
    pub update_up_to_date_message_template: String,
    /// Title shown when an update check fails.
    pub update_failed_title: String,
    /// Message template for update-check failures. Supports `{error}`.
    pub update_failed_message_template: String,
    /// Button label for opening the GitHub Releases page.
    pub update_open_release: String,
    /// Button label for dismissing an available-update prompt.
    pub update_later: String,
    /// Title of the About dialog.
    pub help_about_title: String,
    /// Supplemental About dialog text shown below the app name and version.
    pub help_about_message: String,
    /// Label for the project repository link in the About dialog.
    pub help_about_github_label: String,
    /// Star request shown in the About dialog.
    pub help_about_star_message: String,
    /// Top-level File menu label.
    pub menu_file: String,
    /// Top-level Export menu label.
    pub menu_export: String,
    /// Top-level Language menu label.
    pub menu_language: String,
    /// Top-level Theme menu label.
    pub menu_theme: String,
    /// Top-level Help menu label.
    pub menu_help: String,
    /// Language menu item for importing a custom language pack.
    pub menu_add_language_config: String,
    /// Theme menu item for importing a custom theme pack.
    pub menu_add_theme_config: String,
    /// File menu item for opening a new window.
    pub menu_new_window: String,
    /// File menu item for closing the current window.
    pub menu_close_window: String,
    /// File menu item for opening Markdown files.
    pub menu_open_file: String,
    /// File menu item for opening a folder as a workspace.
    pub menu_open_folder: String,
    /// File menu item for opening a recent file submenu.
    pub menu_open_recent_file: String,
    /// File menu item for opening app preferences.
    pub menu_preferences: String,
    /// Placeholder item shown when no recent files are recorded.
    pub menu_no_recent_files: String,
    /// File menu item for saving the current document.
    pub menu_save: String,
    /// File menu item for saving the current document to a new path.
    pub menu_save_as: String,
    /// 文件历史菜单项与浮层标题。
    pub menu_file_history: String,
    /// 显式「格式化文档」菜单项与命令面板条目。
    pub menu_format_document: String,
    /// 图片右键菜单：在文件管理器中显示。
    pub image_reveal_in_file_manager: String,
    /// 图片右键菜单：复制图片地址。
    pub image_copy_address: String,
    /// 历史浮层空态（该文件还没有保存版本）。
    pub file_history_empty: String,
    /// File menu item for quitting the app.
    pub menu_quit: String,
    /// Export menu item for writing an HTML document.
    pub menu_export_html: String,
    /// Export menu item for writing a PDF document.
    pub menu_export_pdf: String,
    /// Export menu item for writing the whole document as one long PNG image.
    pub menu_export_png: String,
    /// Export menu item for printing the document through the system print/preview.
    pub menu_print: String,
    /// View menu / command palette entry for opening the command palette.
    pub menu_open_command_palette: String,
    /// Help menu item for checking updates.
    pub menu_check_updates: String,
    /// Help menu item for showing About information.
    pub menu_about: String,
    /// Help menu item for installing the CLI tool (symlink to /usr/local/bin).
    pub menu_install_cli_tool: String,
    /// Help menu item for uninstalling the CLI tool.
    pub menu_uninstall_cli_tool: String,
    /// Native file-dialog prompt for opening files.
    pub open_markdown_files_prompt: String,
    /// Native file-dialog prompt for opening a folder.
    pub open_folder_prompt: String,
    /// Native file-dialog prompt for importing a language pack.
    pub add_language_config_prompt: String,
    /// Native file-dialog prompt for importing a theme pack.
    pub add_theme_config_prompt: String,
    /// Title of the open-file failure prompt.
    pub open_failed_title: String,
    /// Title shown when a recent file path no longer exists.
    pub recent_file_missing_title: String,
    /// Message template for missing recent files. Supports `{path}`.
    pub recent_file_missing_message_template: String,
    /// Title of the save failure prompt.
    pub save_failed_title: String,
    /// Title of the prompt shown when a file changes outside the app.
    pub external_change_title: String,
    /// Message shown when a file changes outside the app.
    pub external_change_message: String,
    /// 外部改动冲突框：放弃本地编辑、读回磁盘那一版。
    pub external_change_reload: String,
    /// 外部改动冲突框：把当前编辑内容另存到新路径。
    pub external_change_save_as: String,
    /// Title of the export failure prompt.
    pub export_failed_title: String,
    /// Title of the image-paste failure prompt.
    pub image_paste_failed_title: String,
    /// Title of the custom configuration import failure prompt.
    pub config_import_failed_title: String,
    /// Preferences window title.
    pub preferences_window_title: String,
    /// File preferences navigation label.
    pub preferences_nav_file: String,
    /// Theme preferences navigation label.
    pub preferences_nav_theme: String,
    /// Image preferences navigation label.
    pub preferences_nav_image: String,
    /// Shortcut preferences navigation label.
    pub preferences_nav_shortcuts: String,
    /// Startup option field label.
    pub preferences_startup_option: String,
    /// Startup option for creating a new Markdown document.
    pub preferences_startup_new_file: String,
    /// Startup option for opening the last opened Markdown document.
    pub preferences_startup_last_opened_file: String,
    /// Theme preference field label.
    pub preferences_local_theme: String,
    /// System theme selection label.
    pub preferences_theme_system: String,
    /// Dark theme selection label.
    pub preferences_theme_dark: String,
    /// Light theme selection label.
    pub preferences_theme_light: String,
    /// Image paste behavior field label.
    pub preferences_image_insert_behavior: String,
    pub preferences_image_paste_none: String,
    pub preferences_image_paste_copy_to_document_folder: String,
    pub preferences_image_paste_copy_to_assets_folder: String,
    pub preferences_image_paste_copy_to_named_assets_folder: String,
    /// Save button label in the preferences window.
    pub preferences_save: String,
    /// Cancel button label in the preferences window.
    pub preferences_cancel: String,
    /// Title shown when preferences cannot be saved.
    pub preferences_save_failed_title: String,
    pub preferences_shortcuts_group_file: String,
    pub preferences_shortcuts_group_edit: String,
    pub preferences_shortcuts_group_navigation: String,
    pub preferences_shortcuts_group_formatting: String,
    pub preferences_shortcuts_group_block: String,
    pub preferences_shortcuts_group_other: String,
    pub preferences_shortcut_record: String,
    pub preferences_shortcut_reset: String,
    pub preferences_shortcut_recording: String,
    pub preferences_shortcut_conflict_template: String,
    pub preferences_shortcut_invalid_template: String,
    pub preferences_shortcut_newline: String,
    pub preferences_shortcut_delete_back: String,
    pub preferences_shortcut_delete: String,
    pub preferences_shortcut_word_delete_back: String,
    pub preferences_shortcut_word_delete_forward: String,
    pub preferences_shortcut_focus_prev: String,
    pub preferences_shortcut_focus_next: String,
    pub preferences_shortcut_move_left: String,
    pub preferences_shortcut_move_right: String,
    pub preferences_shortcut_word_move_left: String,
    pub preferences_shortcut_word_move_right: String,
    pub preferences_shortcut_home: String,
    pub preferences_shortcut_end: String,
    pub preferences_shortcut_block_up: String,
    pub preferences_shortcut_block_down: String,
    pub preferences_shortcut_page_up: String,
    pub preferences_shortcut_page_down: String,
    pub preferences_shortcut_jump_to_top: String,
    pub preferences_shortcut_jump_to_bottom: String,
    pub preferences_shortcut_select_left: String,
    pub preferences_shortcut_select_right: String,
    pub preferences_shortcut_word_select_left: String,
    pub preferences_shortcut_word_select_right: String,
    pub preferences_shortcut_select_home: String,
    pub preferences_shortcut_select_end: String,
    pub preferences_shortcut_select_all: String,
    pub preferences_shortcut_copy: String,
    pub preferences_shortcut_cut: String,
    pub preferences_shortcut_paste: String,
    pub preferences_shortcut_undo: String,
    pub preferences_shortcut_redo: String,
    pub preferences_shortcut_bold_selection: String,
    pub preferences_shortcut_italic_selection: String,
    pub preferences_shortcut_underline_selection: String,
    pub preferences_shortcut_code_selection: String,
    /// 加粗的名字（快捷键页与右键菜单共用）。
    pub format_bold: String,
    /// 斜体的名字。
    pub format_italic: String,
    /// 下划线的名字。
    pub format_underline: String,
    /// 行内代码的名字。
    pub format_code: String,
    /// 标记文本的名字。
    pub format_highlight: String,
    /// 删除线的名字（快捷键页与选中菜单、右键菜单共用）。
    pub format_strikethrough: String,
    /// 上标的名字。
    pub format_superscript: String,
    /// 下标的名字。
    pub format_subscript: String,
    pub preferences_shortcut_indent_block: String,
    pub preferences_shortcut_outdent_block: String,
    pub preferences_shortcut_exit_code_block: String,
    pub preferences_shortcut_save_document: String,
    pub preferences_shortcut_save_document_as: String,
    pub preferences_shortcut_format_document: String,
    pub preferences_shortcut_new_window: String,
    pub preferences_shortcut_open_file: String,
    pub preferences_shortcut_quit_application: String,
    pub preferences_shortcut_close_window: String,
    pub preferences_shortcut_dismiss_transient_ui: String,
    pub preferences_shortcut_toggle_view_mode: String,
    pub preferences_shortcut_find_in_document: String,
    pub preferences_shortcut_find_next_match: String,
    pub preferences_shortcut_find_previous_match: String,
    pub preferences_shortcut_toggle_sidebar: String,
    /// 快捷键名：切换全屏（roadmap A6）。
    pub preferences_shortcut_toggle_fullscreen: String,
    /// Workspace drawer Files tab.
    pub workspace_panel_title: String,
    pub workspace_tab_files: String,
    /// Workspace drawer Outline tab.
    pub workspace_tab_outline: String,
    /// Backlinks panel empty state: no document open.
    pub workspace_backlinks_no_document: String,
    /// Backlinks panel empty state: nothing links here.
    pub workspace_backlinks_empty: String,
    /// Tags panel empty state.
    pub workspace_tags_empty: String,
    /// Workspace drawer recent roots tab.
    pub workspace_tab_recent: String,
    /// Placeholder for filtering workspace files by name.
    pub workspace_search_placeholder: String,
    pub workspace_document_find_placeholder: String,
    pub workspace_current_document_label: String,
    /// Empty state when the filename filter has no matches.
    pub workspace_no_search_results: String,
    pub workspace_no_document_find_results: String,
    /// Workspace action for creating a Markdown file.
    pub workspace_new_file: String,
    /// Workspace action for creating a folder.
    pub workspace_new_folder: String,
    /// Workspace action for renaming or moving the selected item.
    pub workspace_rename: String,
    /// Workspace action for deleting the selected item.
    pub workspace_delete: String,
    /// Confirmation title for deleting a workspace item.
    pub workspace_delete_confirm_title: String,
    /// Confirmation message for deleting a workspace item.
    pub workspace_delete_confirm_message: String,
    /// Warning shown when deleting workspace content with unsaved edits.
    pub workspace_delete_unsaved_message: String,
    /// Title shown when no Markdown file path is available for workspace mode.
    pub workspace_no_file_title: String,
    /// Message shown when no Markdown file path is available for workspace mode.
    pub workspace_no_file_message: String,
    /// Message shown when a workspace directory has no visible Markdown files.
    pub workspace_empty_files: String,
    /// Message shown when the current document has no headings.
    pub workspace_empty_outline: String,
    /// Title shown when the workspace file tree cannot be scanned.
    pub workspace_scan_failed_title: String,
    /// Placeholder for the search panel's replace input.
    pub search_replace_placeholder: String,
    /// Toggle tooltip: distinguish upper and lower case.
    pub search_case_sensitive: String,
    /// Toggle tooltip: match whole words only.
    pub search_whole_word: String,
    /// Toggle tooltip: interpret the query as a regular expression.
    pub search_regex: String,
    /// 正则按钮的补充说明：跨行怎么写、`.` 为什么跨不了行。
    pub search_regex_hint: String,
    /// 搜索框下方那一行的开头，后面接匹配引擎交回的原始诊断。
    pub search_invalid_pattern: String,
    /// Toggle tooltip: fuzzy (subsequence) matching.
    pub search_fuzzy: String,
    /// Short chip label for fuzzy matching.
    pub search_fuzzy_short: String,
    /// Scope switch: search the current document.
    pub search_scope_document: String,
    /// Scope switch: search every file in the workspace.
    pub search_scope_workspace: String,
    /// Replace button: replace the current match.
    pub search_replace_current: String,
    /// Replace button: replace every match.
    pub search_replace_all: String,
    /// Result count template; `{n}` is replaced.
    pub search_result_count: String,
    /// Generic OK button label.
    pub dialog_ok: String,
    /// Title for choosing where a picked folder opens.
    pub workspace_folder_choice_title: String,
    /// Button: replace the current window's working set with the folder.
    pub workspace_replace_current_button: String,
    /// Button: open the picked folder in a new window.
    pub workspace_open_new_window_button: String,
    /// Center placeholder for files the text editor can't preview.
    pub workspace_preview_unavailable_message: String,
    /// Message for files using an encoding the editor can't render yet.
    pub encoding_not_supported: String,
    /// Welcome page tagline under the app name.
    pub welcome_tagline: String,
    /// Welcome page primary action.
    pub welcome_new_document: String,
    /// Welcome page secondary action.
    pub welcome_open: String,
    /// Welcome page recent-entries heading.
    pub welcome_recent: String,
    /// Welcome page keyboard hint footer.
    pub welcome_shortcut_hint: String,
    /// Marker label for folder entries in lists.
    pub workspace_folder_entry_label: String,
    /// Quick switcher input placeholder.
    pub quick_open_placeholder: String,
    /// Quick switcher empty-state row.
    pub quick_open_no_results: String,
    /// File tree sort control prefix.
    pub tree_sort_prefix: String,
    /// File tree sort option: by name.
    pub tree_sort_name: String,
    /// File tree sort option: by modification time.
    pub tree_sort_mtime: String,
    /// File tree sort option: by type.
    pub tree_sort_type: String,
    /// Command palette input placeholder.
    pub command_palette_placeholder: String,
    /// Command label: toggle focus mode.
    pub command_toggle_focus_mode: String,
    /// Command label: toggle typewriter mode.
    pub command_toggle_typewriter_mode: String,
    /// Command label: toggle sidebar.
    pub command_toggle_sidebar: String,
    /// Command label: find in document.
    pub command_find_in_document: String,
    /// Command label: find next match.
    pub command_find_next: String,
    /// Command label: find previous match.
    pub command_find_previous: String,
    /// Command label: toggle source/rendered view.
    pub command_toggle_view_mode: String,
    /// Command palette entry for zooming the interface in.
    pub command_zoom_in: String,
    /// Command palette entry for zooming the interface out.
    pub command_zoom_out: String,
    /// Command palette entry for resetting the interface zoom.
    pub command_zoom_reset: String,
    /// 文件页：文件树排序行标签。
    pub preferences_file_tree_sort: String,
    /// 文件页：自动保存间隔行标签。
    pub preferences_file_autosave_debounce: String,
    /// 文件页：自动保存开关行标签。
    pub preferences_autosave: String,
    /// 窗口页：记住窗口位置与大小开关标签。
    pub preferences_window_remember_bounds: String,
    pub preferences_file_external_change: String,
    pub preferences_external_change_auto: String,
    pub preferences_external_change_manual: String,
    pub preferences_file_delete_policy: String,
    pub preferences_delete_policy_trash: String,
    pub preferences_delete_policy_permanent: String,
    pub preferences_smart_punctuation: String,
    pub status_bar_long_block_source: String,
    /// Export menu item: copy rendered HTML source to clipboard.
    pub menu_copy_as_html: String,
    /// Workspace action: duplicate the selected file.
    pub workspace_duplicate: String,
    pub workspace_copy: String,
    pub workspace_paste: String,
    pub workspace_paste_empty: String,
    /// Hover tooltip prefix for footnote references.
    pub hover_footnote_prefix: String,
    /// Hover tooltip marker: local target exists.
    pub hover_target_exists: String,
    /// Hover tooltip marker: local target missing.
    pub hover_target_missing: String,
    /// 文件树过滤输入占位。
    pub tree_filter_placeholder: String,
    /// Message shown when picking or clicking a file type Velora can't open.
    pub workspace_open_unsupported_message: String,
    /// Tab context menu item for closing the tab.
    pub tab_close: String,
    /// Tab context menu item for closing all other tabs.
    pub tab_close_others: String,
    /// Tab context menu item for closing tabs to the left.
    pub tab_close_left: String,
    /// Tab context menu item for closing tabs to the right.
    pub tab_close_right: String,
    /// Tab context menu item for closing every tab.
    pub tab_close_all: String,
    /// Confirmation title when closing a tab with unsaved edits.
    pub tab_close_dirty_title: String,
    /// Confirmation message when one tab has unsaved edits.
    pub tab_close_dirty_message_one: String,
    /// Confirmation message template when several tabs have unsaved edits.
    pub tab_close_dirty_message_many: String,
    /// Title of the link-opening confirmation prompt.
    pub open_link_title: String,
    /// Confirm button for the link-opening prompt.
    pub open_link_open: String,
    /// Cancel button for the link-opening prompt.
    pub open_link_cancel: String,
    /// Compact label shown when rendered mode can switch to source mode.
    pub view_mode_source: String,
    /// Hover label shown when rendered mode can switch to source mode.
    pub view_mode_switch_to_source: String,
    /// Compact label shown when source mode can switch to rendered mode.
    pub view_mode_rendered: String,
    /// Hover label shown when source mode can switch to rendered mode.
    pub view_mode_switch_to_rendered: String,
    /// Explains why a document with ambiguous extensions stays in source mode.
    pub source_mode_fallback_message: String,
    /// 文本右键菜单：切换源码模式的菜单项标签。
    pub context_menu_toggle_source_view: String,
    /// 文本右键菜单：撤销。
    pub context_menu_undo: String,
    /// 文本右键菜单：重做。
    pub context_menu_redo: String,
    /// 文本右键菜单：剪切。
    pub context_menu_cut: String,
    /// 文本右键菜单：拷贝。
    pub context_menu_copy: String,
    /// 文本右键菜单：粘贴。
    pub context_menu_paste: String,
    /// 文本右键菜单：格式二级子菜单。
    pub context_menu_format: String,
    /// 文本右键菜单：段落二级子菜单。
    pub context_menu_paragraph: String,
    /// Root context-menu insert label.
    pub context_menu_insert: String,
    /// Insert submenu item for tables.
    pub context_menu_table: String,
    /// 段落样式：一级标题。
    pub paragraph_heading1: String,
    /// 段落样式：二级标题。
    pub paragraph_heading2: String,
    /// 段落样式：三级标题。
    pub paragraph_heading3: String,
    /// 段落样式：四级标题。
    pub paragraph_heading4: String,
    /// 段落样式：五级标题。
    pub paragraph_heading5: String,
    /// 段落样式：六级标题。
    pub paragraph_heading6: String,
    /// 段落样式：正文。
    pub paragraph_normal_text: String,
    /// 段落样式：无序列表。
    pub paragraph_bullet_list: String,
    /// 段落样式：有序列表。
    pub paragraph_numbered_list: String,
    /// 段落样式：任务列表。
    pub paragraph_task_list: String,
    pub paragraph_quote: String,
    /// Table-axis menu item for left-aligning a column.
    pub table_axis_align_column_left: String,
    /// Table-axis menu item for center-aligning a column.
    pub table_axis_align_column_center: String,
    /// Table-axis menu item for right-aligning a column.
    pub table_axis_align_column_right: String,
    /// Table-axis menu item for moving a column left.
    pub table_axis_move_column_left: String,
    /// Table-axis menu item for moving a column right.
    pub table_axis_move_column_right: String,
    /// Table-axis menu item for deleting a column.
    pub table_axis_delete_column: String,
    /// Table-axis menu item for moving a row up.
    pub table_axis_move_row_up: String,
    /// Table-axis menu item for moving a row down.
    pub table_axis_move_row_down: String,
    /// Table-axis menu item for deleting a row.
    pub table_axis_delete_row: String,
    /// Table header-row menu item that toggles header styling on the top row.
    pub table_header_row: String,
    /// Title of the table-insert dialog.
    pub table_insert_title: String,
    /// Body text of the table-insert dialog.
    pub table_insert_description: String,
    /// Label for table body rows in the table-insert dialog.
    pub table_insert_body_rows: String,
    /// Label for table columns in the table-insert dialog.
    pub table_insert_columns: String,
    /// Cancel button in the table-insert dialog.
    pub table_insert_cancel: String,
    /// Confirm button in the table-insert dialog.
    pub table_insert_confirm: String,
    /// Placeholder label for rendered images without alt text.
    pub image_placeholder: String,
    /// Loading label for rendered images without alt text.
    pub image_loading_without_alt: String,
    /// Loading label template for rendered images with alt text; `{alt}` is replaced.
    pub image_loading_with_alt_template: String,
    /// Label shown when an image fails to load.
    pub image_load_failed: String,
    /// Placeholder shown in the code-block language input when no language is set.
    pub code_language_placeholder: String,
    /// 代码块「复制代码」按钮文案（roadmap B9）。
    pub code_copy_button: String,
    /// Label for the sidebar/files toggle button in the status bar.
    pub status_bar_files: String,
    /// Label for source mode in the status bar mode switch.
    pub status_bar_mode_source: String,
    /// Label for rendered mode in the status bar mode switch.
    pub status_bar_mode_rendered: String,
    /// Suffix shown after the word count number.
    pub status_bar_word_count_suffix: String,
    /// Suffix shown after the estimated reading time in minutes (roadmap B8).
    pub status_bar_reading_time_suffix: String,
    /// Nav label for the status bar preferences tab.
    pub preferences_nav_status_bar: String,
    pub preferences_nav_window: String,
    pub preferences_window_zoom: String,
    pub preferences_window_default_size: String,
    /// 窗口页：新窗口打开位置标签。
    pub preferences_window_open_position: String,
    /// 窗口页：打开位置「记住上次位置」。
    pub preferences_window_open_position_remember: String,
    /// 窗口页：打开位置「每次居中」。
    pub preferences_window_open_position_center: String,
    /// Label for the status bar enabled toggle.
    pub preferences_status_bar_enabled: String,
    /// Label for the word count toggle.
    pub preferences_status_bar_show_word_count: String,
    /// Label for the cursor position toggle.
    pub preferences_status_bar_show_cursor_position: String,
    /// Label for the sidebar toggle visibility.
    pub preferences_status_bar_show_sidebar_toggle: String,
    /// Label for the mode switch visibility.
    pub preferences_status_bar_show_mode_switch: String,
}

