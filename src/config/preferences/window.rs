use super::*;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PreferencesNav {
    File,
    Theme,
    Image,
    Ai,
    Shortcuts,
    StatusBar,
    Window,
}

/// 正在编辑/新增的端点草稿:`id = None` 表示新增。字段值长在
/// TextField 实体里(IME 可用),协议与预设是普通字段。
pub(crate) struct AiEndpointDraft {
    pub(super) id: Option<String>,
    pub(super) name: Entity<TextField>,
    pub(super) kind: crate::ai::ProviderKind,
    pub(super) preset_id: String,
    pub(super) base_url: Entity<TextField>,
    pub(super) api_key: Entity<TextField>,
    pub(super) model: Entity<TextField>,
    pub(super) test: Option<AiTestState>,
    pub(super) kind_dropdown_open: bool,
    pub(super) preset_dropdown_open: bool,
}

/// 「测试连接」的状态。
pub(crate) enum AiTestState {
    Running,
    Ok,
    Failed(String),
}

/// Independent preferences window view.
pub(crate) struct PreferencesWindow {
    pub(super) nav: PreferencesNav,
    pub(super) startup_open: StartupOpenPreference,
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
    /// AI 页草稿:端点档案列表 + 翻译目标;编辑中的端点单独长在草稿里。
    pub(super) ai_settings: AiSettings,
    pub(super) saved_ai: AiSettings,
    pub(super) ai_editing: Option<AiEndpointDraft>,
    pub(super) ai_translate_dropdown_open: bool,
    pub(super) saved_tree_sort: TreeSortPreference,
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
            ai_settings: preferences.ai.clone(),
            saved_ai: preferences.ai.clone(),
            ai_editing: None,
            ai_translate_dropdown_open: false,
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

    /// AI 页的草稿值:翻译目标 + 端点列表(编辑中的端点不计入,点了
    /// 「保存端点」才进列表——页面级「待保存」的口径保持单一)。
    pub(crate) fn ai_draft(&self) -> AiSettings {
        AiSettings {
            translate_target: self.ai_settings.translate_target.clone(),
            endpoints: self.ai_settings.endpoints.clone(),
        }
    }

    pub(crate) fn has_unsaved_changes(&self, cx: &App) -> bool {
        self.startup_open != self.saved_startup_open
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
            || self.ai_draft() != self.saved_ai
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

    pub(crate) fn set_nav_ai(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.nav = PreferencesNav::Ai;
        self.startup_dropdown_open = false;
        self.theme_dropdown_open = false;
        self.writing_width_dropdown_open = false;
        self.image_dropdown_open = false;
        self.recording_shortcut = None;
        cx.notify();
    }

    /// 正在编辑/新增的端点草稿。
    pub(super) fn ai_editing(&self) -> Option<&AiEndpointDraft> {
        self.ai_editing.as_ref()
    }

    pub(crate) fn start_add_ai_endpoint(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let kind = crate::ai::ProviderKind::ChatCompletions;
        let fields = |placeholder: String, value: &str, cx: &mut Context<Self>| {
            cx.new(|cx| {
                let mut field = TextField::new(placeholder, cx);
                field.set_value(value, cx);
                field
            })
        };
        // 新端点默认套该协议的第一个预设,拿到手即可用。
        let preset = AI_PROVIDER_PRESETS
            .iter()
            .find(|preset| preset.kind == kind)
            .unwrap_or(&AI_PROVIDER_PRESETS[AI_PROVIDER_PRESETS.len() - 1]);
        self.ai_editing = Some(AiEndpointDraft {
            id: None,
            name: fields("".into(), "", cx),
            kind,
            preset_id: preset.id.to_string(),
            base_url: fields("https://api.openai.com/v1".into(), preset.base_url, cx),
            api_key: fields("sk-…".into(), "", cx),
            model: fields("gpt-4o-mini".into(), preset.model, cx),
            test: None,
            kind_dropdown_open: false,
            preset_dropdown_open: false,
        });
        cx.notify();
    }

    pub(crate) fn start_edit_ai_endpoint(
        &mut self,
        index: usize,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let fields = |placeholder: String, value: &str, cx: &mut Context<Self>| {
            cx.new(|cx| {
                let mut field = TextField::new(placeholder, cx);
                field.set_value(value, cx);
                field
            })
        };
        let Some(endpoint) = self.ai_settings.endpoints.get(index) else {
            return;
        };
        // 找得到同名预设就带上,找不到(手改配置)落「自定义」。
        let preset_id = AI_PROVIDER_PRESETS
            .iter()
            .find(|preset| {
                preset.kind == endpoint.kind
                    && !preset.base_url.is_empty()
                    && preset.base_url == endpoint.base_url.trim()
            })
            .map(|preset| preset.id.to_string())
            .unwrap_or_else(|| AI_PROVIDER_CUSTOM_ID.to_string());
        self.ai_editing = Some(AiEndpointDraft {
            id: Some(endpoint.id.clone()),
            name: fields("".into(), &endpoint.name, cx),
            kind: endpoint.kind,
            preset_id,
            base_url: fields(
                "https://api.openai.com/v1".into(),
                &endpoint.base_url,
                cx,
            ),
            api_key: fields("sk-…".into(), &endpoint.api_key, cx),
            model: fields("gpt-4o-mini".into(), &endpoint.model, cx),
            test: None,
            kind_dropdown_open: false,
            preset_dropdown_open: false,
        });
        cx.notify();
    }

    pub(crate) fn cancel_ai_endpoint_edit(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.ai_editing = None;
        cx.notify();
    }

    pub(crate) fn delete_ai_endpoint(
        &mut self,
        index: usize,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if index < self.ai_settings.endpoints.len() {
            self.ai_settings.endpoints.remove(index);
            self.ai_settings.normalize_defaults();
        }
        cx.notify();
    }

    pub(crate) fn set_default_ai_endpoint(
        &mut self,
        index: usize,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for (position, endpoint) in self.ai_settings.endpoints.iter_mut().enumerate() {
            endpoint.is_default = position == index;
        }
        cx.notify();
    }

    pub(crate) fn save_ai_endpoint(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(draft) = self.ai_editing.take() else {
            return;
        };
        let read = |field: &Entity<TextField>| field.read(cx).value().to_string();
        // 编辑沿用原 id(默认位/面板选择不漂移);新增用时间戳级 id。
        let endpoint_id = draft.id.clone().unwrap_or_else(|| {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|elapsed| elapsed.as_nanos())
                .unwrap_or_default();
            format!("ep-{nanos}")
        });
        let mut endpoint = AiEndpointPref {
            id: endpoint_id.clone(),
            name: read(&draft.name),
            kind: draft.kind,
            base_url: read(&draft.base_url),
            api_key: read(&draft.api_key),
            model: read(&draft.model),
            is_default: false,
        };
        match self
            .ai_settings
            .endpoints
            .iter_mut()
            .find(|existing| existing.id == endpoint_id)
        {
            // 替换原位:默认位保持用户之前的选择。
            Some(slot) => {
                let is_default = slot.is_default;
                *slot = endpoint;
                slot.is_default = is_default;
            }
            // 新端点成为默认(用户刚配好它,意图明确)。
            None => {
                endpoint.is_default = true;
                for existing in &mut self.ai_settings.endpoints {
                    existing.is_default = false;
                }
                self.ai_settings.endpoints.push(endpoint);
            }
        }
        self.ai_settings.normalize_defaults();
        cx.notify();
    }

    pub(crate) fn toggle_ai_kind_dropdown(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(draft) = self.ai_editing.as_mut() {
            draft.kind_dropdown_open = !draft.kind_dropdown_open;
            draft.preset_dropdown_open = false;
        }
        cx.notify();
    }

    pub(crate) fn select_ai_kind(
        &mut self,
        kind: crate::ai::ProviderKind,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(draft) = self.ai_editing.as_mut() else {
            return;
        };
        draft.kind = kind;
        draft.kind_dropdown_open = false;
        // 换协议时自动套该协议的第一个预设(拿到手即可用);stub 无预设。
        let preset = AI_PROVIDER_PRESETS
            .iter()
            .find(|preset| preset.kind == kind && preset.id != AI_PROVIDER_CUSTOM_ID);
        if let Some(preset) = preset {
            draft.preset_id = preset.id.to_string();
            draft.base_url.update(cx, |field, cx| {
                field.set_value(preset.base_url, cx)
            });
            draft.model.update(cx, |field, cx| field.set_value(preset.model, cx));
        }
        draft.test = None;
        cx.notify();
    }

    pub(crate) fn toggle_ai_preset_dropdown(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(draft) = self.ai_editing.as_mut() {
            draft.preset_dropdown_open = !draft.preset_dropdown_open;
            draft.kind_dropdown_open = false;
        }
        cx.notify();
    }

    pub(crate) fn select_ai_preset(
        &mut self,
        preset_id: String,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(draft) = self.ai_editing.as_mut() else {
            return;
        };
        let Some(preset) = AI_PROVIDER_PRESETS
            .iter()
            .find(|preset| preset.id == preset_id)
        else {
            return;
        };
        draft.preset_id = preset.id.to_string();
        if !preset.base_url.is_empty() {
            draft.base_url.update(cx, |field, cx| {
                field.set_value(preset.base_url, cx)
            });
            draft.model.update(cx, |field, cx| field.set_value(preset.model, cx));
        }
        draft.preset_dropdown_open = false;
        draft.test = None;
        cx.notify();
    }

    /// 「测试连接」:发一个最小请求;stub 立即成功。结果回填草稿,
    /// 不经磁盘、不动端点列表。
    pub(crate) fn test_ai_endpoint(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(draft) = self.ai_editing.as_ref() else {
            return;
        };
        let read = |field: &Entity<TextField>| field.read(cx).value().trim().to_string();
        let endpoint = crate::ai::AiEndpointConfig {
            kind: draft.kind,
            base_url: read(&draft.base_url),
            api_key: read(&draft.api_key),
            model: read(&draft.model),
        };
        let Some(draft) = self.ai_editing.as_mut() else {
            return;
        };
        draft.test = Some(AiTestState::Running);
        let handle = cx.entity().downgrade();
        cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let result = std::thread::spawn(move || {
                crate::ai::test_endpoint(&crate::ai::default_client(), &endpoint)
            })
            .join();
            let _ = this.update(cx, |window, cx| {
                let Some(draft) = window.ai_editing.as_mut() else {
                    return;
                };
                draft.test = Some(match result {
                    Ok(Ok(_reply)) => AiTestState::Ok,
                    Ok(Err(error)) => AiTestState::Failed(match error {
                        crate::ai::AiRequestError::Network(detail) => detail,
                        crate::ai::AiRequestError::Protocol(detail) => detail,
                        crate::ai::AiRequestError::Http { message, .. } => message,
                        crate::ai::AiRequestError::Cancelled => "cancelled".to_string(),
                    }),
                    Err(_join) => AiTestState::Failed("test task panicked".to_string()),
                });
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(crate) fn toggle_ai_translate_dropdown(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.ai_translate_dropdown_open = !self.ai_translate_dropdown_open;
        cx.notify();
    }

    pub(crate) fn select_ai_translate_target(
        &mut self,
        target_id: String,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.ai_settings.translate_target = target_id;
        self.ai_translate_dropdown_open = false;
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
        if !self.has_unsaved_changes(cx) {
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
        EditorSettings::set_autosave_debounce_ms(cx, self.autosave_debounce_ms);
        EditorSettings::set_smart_punctuation(cx, self.smart_punctuation);
        EditorSettings::set_autosave(cx, self.autosave);
        EditorSettings::set_zoom_percent(cx, self.zoom_percent);
        EditorSettings::set_external_change_policy(cx, self.external_change_policy);
        EditorSettings::set_delete_policy(cx, self.delete_policy);
        EditorSettings::set_window_open_position(cx, self.window_open_position);
        let ai = self.ai_draft();
        EditorSettings::set_ai(cx, ai);
        cx.update_global::<EditorSettings, _>(|settings, _cx| {
            settings.default_window_width = self.default_window_width;
            settings.default_window_height = self.default_window_height;
        });
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
        self.saved_remember_window_bounds = self.remember_window_bounds;
        self.saved_window_open_position = self.window_open_position;
        self.saved_smart_punctuation = self.smart_punctuation;
        self.saved_zoom_percent = self.zoom_percent;
        self.saved_default_window_width = self.default_window_width;
        self.saved_default_window_height = self.default_window_height;
        self.saved_external_change_policy = self.external_change_policy;
        self.saved_delete_policy = self.delete_policy;
        let ai = self.ai_draft();
        self.saved_ai = ai;
        cx.notify();
    }
}
