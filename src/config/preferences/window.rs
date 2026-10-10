use super::*;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PreferencesNav {
    File,
    Theme,
    Image,
    Shortcuts,
    StatusBar,
    Window,
}

/// Independent preferences window view.
pub(crate) struct PreferencesWindow {
    pub(super) nav: PreferencesNav,
    pub(super) startup_open: StartupOpenPreference,
    pub(super) check_updates_on_startup: bool,
    pub(super) include_prereleases: bool,
    saved_check_updates_on_startup: bool,
    saved_include_prereleases: bool,
    pub(super) selected_theme_id: String,
    pub(super) image_paste_behavior: ImagePasteBehavior,
    pub(super) fonts: FontPreferences,
    pub(super) keybindings: BTreeMap<String, Vec<String>>,
    pub(super) saved_startup_open: StartupOpenPreference,
    pub(super) saved_theme_id: String,
    pub(super) saved_image_paste_behavior: ImagePasteBehavior,
    pub(super) saved_fonts: FontPreferences,
    pub(super) writing_width: WritingWidthPreference,
    pub(super) saved_writing_width: WritingWidthPreference,
    pub(super) saved_keybindings: BTreeMap<String, Vec<String>>,
    pub(super) theme_options: Vec<ThemeCatalogEntry>,
    pub(super) focus_handle: FocusHandle,
    pub(super) startup_dropdown_open: bool,
    pub(super) theme_dropdown_open: bool,
    pub(super) image_dropdown_open: bool,
    pub(super) markdown_font_dropdown_open: bool,
    pub(super) writing_width_dropdown_open: bool,
    pub(super) code_font_dropdown_open: bool,
    pub(super) recording_shortcut: Option<ShortcutCommand>,
    pub(super) shortcut_error: Option<String>,
    /// 保存失败时在本页顶部内联显示，不弹系统原生对话框（用户要求）。
    pub(super) save_error: Option<String>,
    /// 右侧内容区的滚动位置（单测用它验「真的能滚」）。
    pub(super) page_scroll: ScrollHandle,
    pub(super) tree_sort: TreeSortPreference,
    pub(super) sidebar_open: SidebarOpenPreference,
    pub(super) sidebar_open_dropdown_open: bool,
    pub(super) sidebar_panel: SidebarPanelPreference,
    pub(super) sidebar_panel_dropdown_open: bool,
    pub(super) autosave_debounce_ms: u64,
    pub(super) autosave: bool,
    pub(super) remember_window_bounds: bool,
    pub(super) window_open_position: WindowOpenPosition,
    pub(super) smart_punctuation: bool,
    pub(super) zoom_percent: i64,
    pub(super) default_window_width: i64,
    pub(super) default_window_height: i64,
    pub(super) external_change_policy: ExternalChangePolicy,
    pub(super) delete_policy: DeletePolicy,
    pub(super) zoom_dropdown_open: bool,
    pub(super) window_size_dropdown_open: bool,
    pub(super) window_open_position_dropdown_open: bool,
    pub(super) external_change_dropdown_open: bool,
    pub(super) delete_policy_dropdown_open: bool,
    pub(super) saved_tree_sort: TreeSortPreference,
    pub(super) saved_sidebar_open: SidebarOpenPreference,
    pub(super) saved_sidebar_panel: SidebarPanelPreference,
    pub(super) saved_autosave_debounce_ms: u64,
    pub(super) saved_autosave: bool,
    pub(super) saved_remember_window_bounds: bool,
    pub(super) saved_window_open_position: WindowOpenPosition,
    pub(super) saved_smart_punctuation: bool,
    pub(super) saved_zoom_percent: i64,
    pub(super) saved_default_window_width: i64,
    pub(super) saved_default_window_height: i64,
    pub(super) saved_external_change_policy: ExternalChangePolicy,
    pub(super) saved_delete_policy: DeletePolicy,
    pub(super) tree_sort_dropdown_open: bool,
    pub(super) autosave_dropdown_open: bool,
    pub(super) status_bar_enabled: bool,
    pub(super) status_bar_show_word_count: bool,
    pub(super) status_bar_show_cursor_position: bool,
    pub(super) status_bar_show_sidebar_toggle: bool,
    pub(super) status_bar_show_mode_switch: bool,
    pub(super) saved_status_bar_enabled: bool,
    pub(super) saved_status_bar_show_word_count: bool,
    pub(super) saved_status_bar_show_cursor_position: bool,
    pub(super) saved_status_bar_show_sidebar_toggle: bool,
    pub(super) saved_status_bar_show_mode_switch: bool,
    pub(super) system_appearance_subscription: Option<Subscription>,
}


impl PreferencesWindow {
    pub(crate) fn new(
        preferences: AppPreferences,
        theme_options: Vec<ThemeCatalogEntry>,
        cx: &mut Context<Self>,
    ) -> Self {
        let selected_theme_id = if theme_options
            .iter()
            .any(|entry| entry.id == preferences.default_theme_id)
        {
            preferences.default_theme_id
        } else {
            DEFAULT_THEME_ID.into()
        };
        let startup_open = preferences.startup_open;
        let image_paste_behavior = preferences.image_paste_behavior;
        let fonts = preferences.fonts;
        let writing_width = preferences.writing_width;
        let keybindings = preferences.keybindings;
        let tree_sort = preferences.tree_sort;
        let sidebar_open = preferences.sidebar_open;
        let sidebar_panel = preferences.sidebar_panel;
        let autosave_debounce_ms = preferences.autosave_debounce_ms;
        let autosave = preferences.autosave;
        let remember_window_bounds = preferences.remember_window_bounds;
        let window_open_position = preferences.window_open_position;
        let smart_punctuation = preferences.smart_punctuation;
        let zoom_percent = preferences.zoom_percent;
        let default_window_width = preferences.default_window_width;
        let default_window_height = preferences.default_window_height;
        let external_change_policy = preferences.external_change_policy;
        let delete_policy = preferences.delete_policy;
        Self {
            nav: PreferencesNav::File,
            check_updates_on_startup: preferences.updates.check_on_startup,
            include_prereleases: preferences.updates.include_prereleases,
            saved_check_updates_on_startup: preferences.updates.check_on_startup,
            saved_include_prereleases: preferences.updates.include_prereleases,
            startup_open,
            selected_theme_id: selected_theme_id.clone(),
            image_paste_behavior,
            fonts: fonts.clone(),
            writing_width,
            keybindings: keybindings.clone(),
            saved_startup_open: startup_open,
            saved_theme_id: selected_theme_id,
            saved_image_paste_behavior: image_paste_behavior,
            saved_fonts: fonts,
            saved_writing_width: writing_width,
            saved_keybindings: keybindings,
            tree_sort,
            sidebar_open,
            sidebar_open_dropdown_open: false,
            sidebar_panel,
            sidebar_panel_dropdown_open: false,
            autosave_debounce_ms,
            autosave,
            remember_window_bounds,
            window_open_position,
            smart_punctuation,
            zoom_percent,
            default_window_width,
            default_window_height,
            external_change_policy,
            delete_policy,
            zoom_dropdown_open: false,
            window_size_dropdown_open: false,
            window_open_position_dropdown_open: false,
            external_change_dropdown_open: false,
            delete_policy_dropdown_open: false,
            saved_tree_sort: tree_sort,
            saved_autosave_debounce_ms: autosave_debounce_ms,
            saved_autosave: autosave,
            saved_remember_window_bounds: remember_window_bounds,
            saved_window_open_position: window_open_position,
            saved_smart_punctuation: smart_punctuation,
            saved_zoom_percent: zoom_percent,
            saved_default_window_width: default_window_width,
            saved_default_window_height: default_window_height,
            saved_external_change_policy: external_change_policy,
            saved_delete_policy: delete_policy,
            tree_sort_dropdown_open: false,
            saved_sidebar_open: sidebar_open,
            saved_sidebar_panel: sidebar_panel,
            autosave_dropdown_open: false,
            theme_options,
            focus_handle: cx.focus_handle(),
            startup_dropdown_open: false,
            theme_dropdown_open: false,
            image_dropdown_open: false,
            markdown_font_dropdown_open: false,
            writing_width_dropdown_open: false,
            code_font_dropdown_open: false,
            recording_shortcut: None,
            shortcut_error: None,
            save_error: None,
            page_scroll: ScrollHandle::new(),
            status_bar_enabled: preferences.status_bar.enabled,
            status_bar_show_word_count: preferences.status_bar.show_word_count,
            status_bar_show_cursor_position: preferences.status_bar.show_cursor_position,
            status_bar_show_sidebar_toggle: preferences.status_bar.show_sidebar_toggle,
            status_bar_show_mode_switch: preferences.status_bar.show_mode_switch,
            saved_status_bar_enabled: preferences.status_bar.enabled,
            saved_status_bar_show_word_count: preferences.status_bar.show_word_count,
            saved_status_bar_show_cursor_position: preferences.status_bar.show_cursor_position,
            saved_status_bar_show_sidebar_toggle: preferences.status_bar.show_sidebar_toggle,
            saved_status_bar_show_mode_switch: preferences.status_bar.show_mode_switch,
            system_appearance_subscription: None,
        }
    }

    pub(crate) fn theme_display_name(
        &self,
        entry: &ThemeCatalogEntry,
        strings: &crate::i18n::I18nStrings,
    ) -> String {
        match entry.id.as_str() {
            "system" => strings.preferences_theme_system.clone(),
            "velora-dark" => strings.preferences_theme_dark.clone(),
            "velora-light" => strings.preferences_theme_light.clone(),
            _ => entry.name.clone(),
        }
    }

    pub(crate) fn selected_theme_name(&self, strings: &crate::i18n::I18nStrings) -> String {
        self.theme_options
            .iter()
            .find(|entry| entry.id == self.selected_theme_id)
            .map(|entry| self.theme_display_name(entry, strings))
            .unwrap_or_else(|| strings.preferences_theme_system.clone())
    }

    pub(crate) fn has_unsaved_changes(&self) -> bool {
        self.check_updates_on_startup != self.saved_check_updates_on_startup
            || self.include_prereleases != self.saved_include_prereleases
            || self.startup_open != self.saved_startup_open
            || self.selected_theme_id != self.saved_theme_id
            || self.image_paste_behavior != self.saved_image_paste_behavior
            || self.fonts != self.saved_fonts
            || self.writing_width != self.saved_writing_width
            || normalize_shortcut_config(&self.keybindings)
                != normalize_shortcut_config(&self.saved_keybindings)
            || self.status_bar_enabled != self.saved_status_bar_enabled
            || self.status_bar_show_word_count != self.saved_status_bar_show_word_count
            || self.status_bar_show_cursor_position != self.saved_status_bar_show_cursor_position
            || self.status_bar_show_sidebar_toggle != self.saved_status_bar_show_sidebar_toggle
            || self.status_bar_show_mode_switch != self.saved_status_bar_show_mode_switch
            || self.tree_sort != self.saved_tree_sort
            || self.sidebar_open != self.saved_sidebar_open
            || self.sidebar_panel != self.saved_sidebar_panel
            || self.autosave_debounce_ms != self.saved_autosave_debounce_ms
            || self.autosave != self.saved_autosave
            || self.remember_window_bounds != self.saved_remember_window_bounds
            || self.window_open_position != self.saved_window_open_position
            || self.smart_punctuation != self.saved_smart_punctuation
            || self.zoom_percent != self.saved_zoom_percent
            || self.default_window_width != self.saved_default_window_width
            || self.default_window_height != self.saved_default_window_height
            || self.external_change_policy != self.saved_external_change_policy
            || self.delete_policy != self.saved_delete_policy
    }

    pub(crate) fn toggle_sidebar_open_dropdown(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sidebar_open_dropdown_open = !self.sidebar_open_dropdown_open;
        cx.notify();
    }

    pub(crate) fn toggle_sidebar_panel_dropdown(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sidebar_panel_dropdown_open = !self.sidebar_panel_dropdown_open;
        cx.notify();
    }

    pub(crate) fn toggle_tree_sort_dropdown(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.tree_sort_dropdown_open = !self.tree_sort_dropdown_open;
        cx.notify();
    }

    pub(crate) fn toggle_autosave_dropdown(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.autosave_dropdown_open = !self.autosave_dropdown_open;
        cx.notify();
    }

    pub(crate) fn toggle_external_change_dropdown(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.external_change_dropdown_open = !self.external_change_dropdown_open;
        cx.notify();
    }

    pub(crate) fn toggle_delete_policy_dropdown(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.delete_policy_dropdown_open = !self.delete_policy_dropdown_open;
        cx.notify();
    }

    pub(crate) fn toggle_zoom_dropdown(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.zoom_dropdown_open = !self.zoom_dropdown_open;
        self.window_size_dropdown_open = false;
        cx.notify();
    }

    pub(crate) fn toggle_window_size_dropdown(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.window_size_dropdown_open = !self.window_size_dropdown_open;
        self.zoom_dropdown_open = false;
        self.window_open_position_dropdown_open = false;
        cx.notify();
    }

    pub(crate) fn toggle_window_open_position_dropdown(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.window_open_position_dropdown_open = !self.window_open_position_dropdown_open;
        self.zoom_dropdown_open = false;
        self.window_size_dropdown_open = false;
        cx.notify();
    }

    pub(crate) fn set_nav_file(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.nav = PreferencesNav::File;
        self.startup_dropdown_open = false;
        self.theme_dropdown_open = false;
        self.writing_width_dropdown_open = false;
        self.image_dropdown_open = false;
        self.recording_shortcut = None;
        cx.notify();
    }

    pub(crate) fn set_nav_theme(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.nav = PreferencesNav::Theme;
        self.startup_dropdown_open = false;
        self.theme_dropdown_open = false;
        self.writing_width_dropdown_open = false;
        self.image_dropdown_open = false;
        self.recording_shortcut = None;
        cx.notify();
    }

    pub(crate) fn set_nav_image(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.nav = PreferencesNav::Image;
        self.startup_dropdown_open = false;
        self.theme_dropdown_open = false;
        self.writing_width_dropdown_open = false;
        self.image_dropdown_open = false;
        self.recording_shortcut = None;
        cx.notify();
    }

    pub(crate) fn set_nav_shortcuts(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.nav = PreferencesNav::Shortcuts;
        self.startup_dropdown_open = false;
        self.theme_dropdown_open = false;
        self.writing_width_dropdown_open = false;
        self.image_dropdown_open = false;
        self.shortcut_error = None;
        cx.notify();
    }

    pub(crate) fn set_nav_window(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.nav = PreferencesNav::Window;
        cx.notify();
    }

    pub(crate) fn set_nav_status_bar(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.nav = PreferencesNav::StatusBar;
        self.startup_dropdown_open = false;
        self.theme_dropdown_open = false;
        self.writing_width_dropdown_open = false;
        self.image_dropdown_open = false;
        self.recording_shortcut = None;
        cx.notify();
    }

    pub(crate) fn toggle_startup_dropdown(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.startup_dropdown_open = !self.startup_dropdown_open;
        self.theme_dropdown_open = false;
        self.image_dropdown_open = false;
        cx.notify();
    }

    pub(crate) fn toggle_theme_dropdown(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.theme_dropdown_open = !self.theme_dropdown_open;
        self.writing_width_dropdown_open = false;
        self.startup_dropdown_open = false;
        self.image_dropdown_open = false;
        cx.notify();
    }

    pub(crate) fn toggle_writing_width_dropdown(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.writing_width_dropdown_open = !self.writing_width_dropdown_open;
        self.theme_dropdown_open = false;
        self.markdown_font_dropdown_open = false;
        self.code_font_dropdown_open = false;
        cx.notify();
    }

    pub(crate) fn toggle_markdown_font_dropdown(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.markdown_font_dropdown_open = !self.markdown_font_dropdown_open;
        self.writing_width_dropdown_open = false;
        self.code_font_dropdown_open = false;
        cx.notify();
    }

    pub(crate) fn toggle_code_font_dropdown(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.code_font_dropdown_open = !self.code_font_dropdown_open;
        self.writing_width_dropdown_open = false;
        self.markdown_font_dropdown_open = false;
        cx.notify();
    }

    pub(crate) fn toggle_image_dropdown(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.image_dropdown_open = !self.image_dropdown_open;
        self.startup_dropdown_open = false;
        self.theme_dropdown_open = false;
        cx.notify();
    }

    pub(crate) fn cancel(&mut self, _: &ClickEvent, window: &mut Window, _: &mut Context<Self>) {
        window.remove_window();
    }

    pub(crate) fn on_titlebar_close(
        &mut self,
        event: &ClickEvent,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        if event.standard_click() {
            window.remove_window();
        }
    }

    pub(crate) fn save(&mut self, _: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !self.has_unsaved_changes() {
            return;
        }

        self.save_error = None;
        let preferences = match save_preferences_from_window(
            self.startup_open,
            &self.selected_theme_id,
            self.image_paste_behavior,
            &self.fonts,
            self.writing_width,
            self.keybindings.clone(),
            &StatusBarPreferences {
                enabled: self.status_bar_enabled,
                show_word_count: self.status_bar_show_word_count,
                show_cursor_position: self.status_bar_show_cursor_position,
                show_sidebar_toggle: self.status_bar_show_sidebar_toggle,
                show_mode_switch: self.status_bar_show_mode_switch,
                custom_buttons: Vec::new(),
            },
            self.tree_sort,
            self.sidebar_open,
            self.sidebar_panel,
            self.autosave_debounce_ms,
            self.remember_window_bounds,
            self.window_open_position,
            self.smart_punctuation,
            self.autosave,
            self.zoom_percent,
            self.default_window_width,
            self.default_window_height,
            self.external_change_policy,
            self.delete_policy,
            self.check_updates_on_startup,
            self.include_prereleases,
        ) {
            Ok(preferences) => preferences,
            Err(err) => {
                let strings = cx.global::<I18nManager>().strings().clone();
                self.save_error = Some(format!(
                    "{}: {}",
                    strings.preferences_save_failed_title,
                    err
                ));
                cx.notify();
                return;
            }
        };

        // 同步新设置到全局缓存并刷新（roadmap H1）。
        EditorSettings::set_tree_sort(cx, self.tree_sort);
        EditorSettings::set_sidebar_open_on_startup(cx, self.sidebar_open);
        EditorSettings::set_sidebar_panel_on_startup(cx, self.sidebar_panel);
        EditorSettings::set_autosave_debounce_ms(cx, self.autosave_debounce_ms);
        EditorSettings::set_smart_punctuation(cx, self.smart_punctuation);
        EditorSettings::set_autosave(cx, self.autosave);
        EditorSettings::set_zoom_percent(cx, self.zoom_percent);
        EditorSettings::set_external_change_policy(cx, self.external_change_policy);
        EditorSettings::set_delete_policy(cx, self.delete_policy);
        EditorSettings::set_window_open_position(cx, self.window_open_position);
        cx.update_global::<EditorSettings, _>(|settings, _cx| {
            settings.default_window_width = self.default_window_width;
            settings.default_window_height = self.default_window_height;
        });
        EditorSettings::set_updates_in_memory(cx, preferences.updates.clone());
        self.apply_saved_preferences(preferences, window, cx);
    }

    pub(crate) fn apply_saved_preferences(
        &mut self,
        preferences: AppPreferences,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let theme_changed = cx.update_global::<ThemeManager, _>(|theme_manager, _cx| {
            theme_manager.set_theme_by_id(&preferences.default_theme_id)
        });
        if !theme_changed {
            let _ = cx.update_global::<ThemeManager, _>(|theme_manager, _cx| {
                theme_manager.set_theme_by_id(DEFAULT_THEME_ID)
            });
        }
        cx.clear_key_bindings();
        install_keybindings(cx, &preferences.keybindings);
        crate::app_menu::install_menus(cx);
        cx.update_global::<EditorSettings, _>(|settings, _cx| {
            settings.status_bar_settings.status_bar_enabled = preferences.status_bar.enabled;
            settings.status_bar_settings.status_bar_show_word_count =
                preferences.status_bar.show_word_count;
            settings.status_bar_settings.status_bar_show_cursor_position =
                preferences.status_bar.show_cursor_position;
            settings.status_bar_settings.status_bar_show_sidebar_toggle =
                preferences.status_bar.show_sidebar_toggle;
            settings.status_bar_settings.status_bar_show_mode_switch =
                preferences.status_bar.show_mode_switch;
            settings.fonts = preferences.fonts.clone();
            settings.writing_width = preferences.writing_width;
            settings.window_open_position = preferences.window_open_position;
        });
        cx.refresh_windows();
        window.activate_window();
        self.focus_handle.focus(window);
        self.saved_startup_open = self.startup_open;
        self.saved_theme_id = self.selected_theme_id.clone();
        self.saved_image_paste_behavior = self.image_paste_behavior;
        self.saved_fonts = self.fonts.clone();
        self.saved_writing_width = self.writing_width;
        self.saved_keybindings = normalize_shortcut_config(&self.keybindings);
        self.saved_status_bar_enabled = self.status_bar_enabled;
        self.saved_status_bar_show_word_count = self.status_bar_show_word_count;
        self.saved_status_bar_show_cursor_position = self.status_bar_show_cursor_position;
        self.saved_status_bar_show_sidebar_toggle = self.status_bar_show_sidebar_toggle;
        self.saved_status_bar_show_mode_switch = self.status_bar_show_mode_switch;
        self.saved_tree_sort = self.tree_sort;
        self.saved_autosave_debounce_ms = self.autosave_debounce_ms;
        self.saved_autosave = self.autosave;
        self.saved_check_updates_on_startup = self.check_updates_on_startup;
        self.saved_include_prereleases = self.include_prereleases;
        self.saved_remember_window_bounds = self.remember_window_bounds;
        self.saved_window_open_position = self.window_open_position;
        self.saved_smart_punctuation = self.smart_punctuation;
        self.saved_zoom_percent = self.zoom_percent;
        self.saved_default_window_width = self.default_window_width;
        self.saved_default_window_height = self.default_window_height;
        self.saved_external_change_policy = self.external_change_policy;
        self.saved_delete_policy = self.delete_policy;
        cx.notify();
    }
}
