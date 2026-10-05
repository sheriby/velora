use super::*;

impl PreferencesWindow {
    pub(crate) fn shortcut_category_label(
        category: ShortcutCategory,
        strings: &crate::i18n::I18nStrings,
    ) -> String {
        match category {
            ShortcutCategory::File => strings.preferences_shortcuts_group_file.clone(),
            ShortcutCategory::Edit => strings.preferences_shortcuts_group_edit.clone(),
            ShortcutCategory::Navigation => strings.preferences_shortcuts_group_navigation.clone(),
            ShortcutCategory::Formatting => strings.preferences_shortcuts_group_formatting.clone(),
            ShortcutCategory::Block => strings.preferences_shortcuts_group_block.clone(),
            ShortcutCategory::Other => strings.preferences_shortcuts_group_other.clone(),
        }
    }

    pub(crate) fn shortcut_command_label(
        command: ShortcutCommand,
        strings: &crate::i18n::I18nStrings,
    ) -> String {
        match command {
            ShortcutCommand::Newline => strings.preferences_shortcut_newline.clone(),
            ShortcutCommand::DeleteBack => strings.preferences_shortcut_delete_back.clone(),
            ShortcutCommand::Delete => strings.preferences_shortcut_delete.clone(),
            ShortcutCommand::WordDeleteBack => {
                strings.preferences_shortcut_word_delete_back.clone()
            }
            ShortcutCommand::WordDeleteForward => {
                strings.preferences_shortcut_word_delete_forward.clone()
            }
            ShortcutCommand::FocusPrev => strings.preferences_shortcut_focus_prev.clone(),
            ShortcutCommand::FocusNext => strings.preferences_shortcut_focus_next.clone(),
            ShortcutCommand::MoveLeft => strings.preferences_shortcut_move_left.clone(),
            ShortcutCommand::MoveRight => strings.preferences_shortcut_move_right.clone(),
            ShortcutCommand::WordMoveLeft => strings.preferences_shortcut_word_move_left.clone(),
            ShortcutCommand::WordMoveRight => strings.preferences_shortcut_word_move_right.clone(),
            ShortcutCommand::Home => strings.preferences_shortcut_home.clone(),
            ShortcutCommand::End => strings.preferences_shortcut_end.clone(),
            ShortcutCommand::BlockUp => strings.preferences_shortcut_block_up.clone(),
            ShortcutCommand::BlockDown => strings.preferences_shortcut_block_down.clone(),
            ShortcutCommand::PageUp => strings.preferences_shortcut_page_up.clone(),
            ShortcutCommand::PageDown => strings.preferences_shortcut_page_down.clone(),
            ShortcutCommand::JumpToTop => strings.preferences_shortcut_jump_to_top.clone(),
            ShortcutCommand::JumpToBottom => strings.preferences_shortcut_jump_to_bottom.clone(),
            ShortcutCommand::SelectLeft => strings.preferences_shortcut_select_left.clone(),
            ShortcutCommand::SelectRight => strings.preferences_shortcut_select_right.clone(),
            ShortcutCommand::WordSelectLeft => {
                strings.preferences_shortcut_word_select_left.clone()
            }
            ShortcutCommand::WordSelectRight => {
                strings.preferences_shortcut_word_select_right.clone()
            }
            ShortcutCommand::SelectHome => strings.preferences_shortcut_select_home.clone(),
            ShortcutCommand::SelectEnd => strings.preferences_shortcut_select_end.clone(),
            ShortcutCommand::SelectAll => strings.preferences_shortcut_select_all.clone(),
            ShortcutCommand::Copy => strings.preferences_shortcut_copy.clone(),
            ShortcutCommand::Cut => strings.preferences_shortcut_cut.clone(),
            ShortcutCommand::Paste => strings.preferences_shortcut_paste.clone(),
            ShortcutCommand::Undo => strings.preferences_shortcut_undo.clone(),
            ShortcutCommand::Redo => strings.preferences_shortcut_redo.clone(),
            ShortcutCommand::BoldSelection => strings.preferences_shortcut_bold_selection.clone(),
            ShortcutCommand::ItalicSelection => {
                strings.preferences_shortcut_italic_selection.clone()
            }
            ShortcutCommand::UnderlineSelection => {
                strings.preferences_shortcut_underline_selection.clone()
            }
            ShortcutCommand::CodeSelection => strings.preferences_shortcut_code_selection.clone(),
            ShortcutCommand::IndentBlock => strings.preferences_shortcut_indent_block.clone(),
            ShortcutCommand::OutdentBlock => strings.preferences_shortcut_outdent_block.clone(),
            ShortcutCommand::ExitCodeBlock => strings.preferences_shortcut_exit_code_block.clone(),
            ShortcutCommand::SaveDocument => strings.preferences_shortcut_save_document.clone(),
            ShortcutCommand::SaveDocumentAs => {
                strings.preferences_shortcut_save_document_as.clone()
            }
            ShortcutCommand::FormatDocument => {
                strings.preferences_shortcut_format_document.clone()
            }
            ShortcutCommand::FileHistory => strings.menu_file_history.clone(),
            ShortcutCommand::PrintDocument => strings.menu_print.clone(),
            ShortcutCommand::NewWindow => strings.preferences_shortcut_new_window.clone(),
            ShortcutCommand::OpenFile => strings.preferences_shortcut_open_file.clone(),
            ShortcutCommand::QuitApplication => {
                strings.preferences_shortcut_quit_application.clone()
            }
            ShortcutCommand::CloseWindow => strings.preferences_shortcut_close_window.clone(),
            ShortcutCommand::DismissTransientUi => {
                strings.preferences_shortcut_dismiss_transient_ui.clone()
            }
            ShortcutCommand::ToggleViewMode => {
                strings.preferences_shortcut_toggle_view_mode.clone()
            }
            ShortcutCommand::FindInDocument => {
                strings.preferences_shortcut_find_in_document.clone()
            }
            ShortcutCommand::OpenAiAssistant => {
                strings.preferences_shortcut_open_ai_assistant.clone()
            }
            ShortcutCommand::FindNextMatch => strings.preferences_shortcut_find_next_match.clone(),
            ShortcutCommand::FindPreviousMatch => {
                strings.preferences_shortcut_find_previous_match.clone()
            }
            ShortcutCommand::ToggleSidebar => {
                strings.preferences_shortcut_toggle_sidebar.clone()
            }
            ShortcutCommand::ToggleFullscreen => {
                strings.preferences_shortcut_toggle_fullscreen.clone()
            }
        }
    }

    pub(crate) fn format_template(template: &str, key: &str, value: &str) -> String {
        template.replace(key, value)
    }

    pub(crate) fn begin_recording_shortcut(
        &mut self,
        command: ShortcutCommand,
        _: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.recording_shortcut = Some(command);
        self.shortcut_error = None;
        window.focus(&self.focus_handle);
        cx.notify();
    }

    pub(crate) fn reset_shortcut(
        &mut self,
        command: ShortcutCommand,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(definition) = shortcut_definitions()
            .iter()
            .find(|definition| definition.command == command)
        {
            self.keybindings.remove(definition.id);
        }
        if self.recording_shortcut == Some(command) {
            self.recording_shortcut = None;
        }
        self.shortcut_error = None;
        cx.notify();
    }

    pub(crate) fn capture_shortcut_key(
        &mut self,
        event: &KeyDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(command) = self.recording_shortcut else {
            return;
        };
        cx.stop_propagation();
        if event.is_held {
            return;
        }

        let key = event.keystroke.unparse();
        if key == "escape" {
            self.recording_shortcut = None;
            self.shortcut_error = None;
            cx.notify();
            return;
        }

        let Some(keys) = normalize_shortcut_keys(std::slice::from_ref(&key)) else {
            let strings = cx.global::<I18nManager>().strings();
            self.shortcut_error = Some(Self::format_template(
                &strings.preferences_shortcut_invalid_template,
                "{shortcut}",
                &key,
            ));
            cx.notify();
            return;
        };

        if let Some(conflict) = shortcut_conflict_for(command, &keys, &self.keybindings) {
            let strings = cx.global::<I18nManager>().strings();
            let label = Self::shortcut_command_label(conflict.command, strings);
            self.shortcut_error = Some(Self::format_template(
                &strings.preferences_shortcut_conflict_template,
                "{command}",
                &label,
            ));
            cx.notify();
            return;
        }

        if let Some(definition) = shortcut_definitions()
            .iter()
            .find(|definition| definition.command == command)
        {
            let defaults = definition
                .default_keys
                .iter()
                .map(|key| key.to_string())
                .collect::<Vec<_>>();
            if keys == defaults {
                self.keybindings.remove(definition.id);
            } else {
                self.keybindings.insert(definition.id.to_string(), keys);
            }
        }
        self.recording_shortcut = None;
        self.shortcut_error = None;
        cx.notify();
    }

    pub(crate) fn shortcut_chip(label: &str, theme: &Theme) -> impl IntoElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        div()
            .min_w(px(58.0))
            .h(px(24.0))
            .px(px(8.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px((d.menu_item_radius - 1.0).max(0.0)))
            .border(px(d.dialog_border_width))
            .border_color(c.dialog_border)
            .bg(c.code_bg)
            .text_size(px((t.dialog_body_size - 1.0).max(10.0)))
            .text_color(c.code_text)
            .child(SharedString::from(label.to_string()))
    }

    pub(crate) fn shortcut_action_button(
        id: impl Into<ElementId>,
        label: String,
        theme: &Theme,
        on_click: impl Fn(&mut Self, &ClickEvent, &mut Window, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        div()
            .id(id)
            .h(px(28.0))
            .px(px(10.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px((d.dialog_radius - 5.0).max(0.0)))
            .border(px(d.dialog_border_width))
            .border_color(c.dialog_border)
            .bg(c.dialog_secondary_button_bg)
            .hover(|this| this.bg(c.dialog_secondary_button_hover))
            .cursor_pointer()
            .text_size(px((t.dialog_button_size - 1.0).max(10.0)))
            .font_weight(t.dialog_button_weight.to_font_weight())
            .text_color(c.dialog_secondary_button_text)
            .child(label)
            .on_click(cx.listener(on_click))
    }

    pub(crate) fn render_shortcut_row(
        &self,
        definition: ShortcutDefinition,
        theme: &Theme,
        strings: &crate::i18n::I18nStrings,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let c = &theme.colors;
        let t = &theme.typography;
        let is_recording = self.recording_shortcut == Some(definition.command);
        let keys = resolved_shortcut_keys(&self.keybindings, definition.command);
        let label = Self::shortcut_command_label(definition.command, strings);
        let command = definition.command;

        let mut chips = div().flex().flex_wrap().gap(px(6.0));
        if is_recording {
            chips = chips.child(Self::shortcut_chip(
                &strings.preferences_shortcut_recording,
                theme,
            ));
        } else {
            for key in keys {
                chips = chips.child(Self::shortcut_chip(&key, theme));
            }
        }

        div()
            .w_full()
            .min_h(px(48.0))
            .px(px(14.0))
            .py(px(6.0))
            .flex()
            .items_center()
            .gap(px(12.0))
            .hover(|this| this.bg(c.dialog_secondary_button_hover))
            .child(
                div()
                    .w(px(150.0))
                    .flex_shrink_0()
                    .text_size(px(t.dialog_body_size))
                    .text_color(c.dialog_body)
                    .child(label),
            )
            .child(div().flex_1().min_w(px(0.0)).child(chips))
            .child(
                div()
                    .flex_shrink_0()
                    .flex()
                    .gap(px(6.0))
                    .child(Self::shortcut_action_button(
                        ("preferences-shortcut-record", definition.command as u32),
                        strings.preferences_shortcut_record.clone(),
                        theme,
                        move |this, event, window, cx| {
                            this.begin_recording_shortcut(command, event, window, cx)
                        },
                        cx,
                    ))
                    .child(Self::shortcut_action_button(
                        ("preferences-shortcut-reset", definition.command as u32),
                        strings.preferences_shortcut_reset.clone(),
                        theme,
                        move |this, event, window, cx| {
                            this.reset_shortcut(command, event, window, cx)
                        },
                        cx,
                    )),
            )
    }

    pub(crate) fn render_shortcuts_page(
        &self,
        theme: &Theme,
        strings: &crate::i18n::I18nStrings,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = &theme.colors;
        let t = &theme.typography;

        let categories = [
            ShortcutCategory::File,
            ShortcutCategory::Edit,
            ShortcutCategory::Navigation,
            ShortcutCategory::Formatting,
            ShortcutCategory::Block,
            ShortcutCategory::Other,
        ];

        // 每个分类一张卡片，分类名用弱化的小字放在卡片上方；整页滚动交给外层容器。
        let mut page = div().w_full().flex_shrink_0().flex().flex_col().gap(px(14.0));
        for category in categories {
            let rows = shortcut_definitions()
                .iter()
                .copied()
                .filter(|definition| definition.category == category)
                .map(|definition| {
                    self.render_shortcut_row(definition, theme, strings, cx)
                        .into_any_element()
                })
                .collect::<Vec<_>>();
            if rows.is_empty() {
                continue;
            }
            page = page.child(
                div()
                    .w_full()
                    .flex_shrink_0()
                    .flex()
                    .flex_col()
                    .gap(px(6.0))
                    .child(
                        div()
                            .px(px(2.0))
                            .text_size(px((t.dialog_body_size - 1.0).max(10.0)))
                            .font_weight(t.dialog_button_weight.to_font_weight())
                            .text_color(c.dialog_muted)
                            .child(Self::shortcut_category_label(category, strings)),
                    )
                    .child(self.settings_card(theme, rows)),
            );
        }
        page.into_any_element()
    }

    pub(crate) fn render_window_page(
        &self,
        theme: &Theme,
        strings: &crate::i18n::I18nStrings,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let zoom_selected = format!("{}%", self.zoom_percent);
        let mut zoom_dropdown = div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(Self::dropdown_button(
                "preferences-zoom-dropdown",
                zoom_selected.into(),
                theme,
                Self::toggle_zoom_dropdown,
                cx,
            ));
        if self.zoom_dropdown_open {
            for percent in [80i64, 90, 100, 110, 125, 150] {
                let is_selected = self.zoom_percent == percent;
                zoom_dropdown = zoom_dropdown.child(Self::dropdown_item(
                    gpui::SharedString::from(format!("preferences-zoom-{percent}")),
                    format!("{percent}%"),
                    is_selected,
                    theme,
                    move |this, _, _, cx| {
                        this.zoom_percent = percent;
                        this.zoom_dropdown_open = false;
                        cx.notify();
                    },
                    cx,
                ));
            }
        }

        let size_selected = format!(
            "{} × {}",
            self.default_window_width, self.default_window_height
        );
        let mut size_dropdown = div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(Self::dropdown_button(
                "preferences-window-size-dropdown",
                size_selected.into(),
                theme,
                Self::toggle_window_size_dropdown,
                cx,
            ));
        if self.window_size_dropdown_open {
            for (width, height) in [(900i64, 600i64), (1080, 720), (1280, 800), (1440, 900)] {
                let is_selected =
                    self.default_window_width == width && self.default_window_height == height;
                size_dropdown = size_dropdown.child(Self::dropdown_item(
                    gpui::SharedString::from(format!("preferences-window-size-{width}x{height}")),
                    format!("{width} × {height}"),
                    is_selected,
                    theme,
                    move |this, _, _, cx| {
                        this.default_window_width = width;
                        this.default_window_height = height;
                        this.window_size_dropdown_open = false;
                        cx.notify();
                    },
                    cx,
                ));
            }
        }

        let open_position_selected = match self.window_open_position {
            WindowOpenPosition::Remember => {
                strings.preferences_window_open_position_remember.clone()
            }
            WindowOpenPosition::Center => strings.preferences_window_open_position_center.clone(),
        };
        let mut open_position_dropdown = div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(Self::dropdown_button(
                "preferences-window-open-position-dropdown",
                open_position_selected,
                theme,
                Self::toggle_window_open_position_dropdown,
                cx,
            ));
        if self.window_open_position_dropdown_open {
            for (position, label) in [
                (
                    WindowOpenPosition::Remember,
                    strings.preferences_window_open_position_remember.clone(),
                ),
                (
                    WindowOpenPosition::Center,
                    strings.preferences_window_open_position_center.clone(),
                ),
            ] {
                let is_selected = self.window_open_position == position;
                open_position_dropdown = open_position_dropdown.child(Self::dropdown_item(
                    gpui::SharedString::from(format!(
                        "preferences-window-open-position-{}",
                        position.as_str()
                    )),
                    label,
                    is_selected,
                    theme,
                    move |this, _, _, cx| {
                        this.window_open_position = position;
                        this.window_open_position_dropdown_open = false;
                        cx.notify();
                    },
                    cx,
                ));
            }
        }

        let remember_toggle = crate::components::switch::Switch::new("preferences-remember-window")
            .checked(self.remember_window_bounds)
            .on_click(cx.listener(|this, _event, _window, cx| {
                this.remember_window_bounds = !this.remember_window_bounds;
                cx.notify();
            }));

        self.settings_card(
            theme,
            vec![
                self.settings_row(theme, strings.preferences_window_zoom.clone(), zoom_dropdown),
                self.settings_row(
                    theme,
                    strings.preferences_window_default_size.clone(),
                    size_dropdown,
                ),
                self.settings_row(
                    theme,
                    strings.preferences_window_open_position.clone(),
                    open_position_dropdown,
                ),
                self.settings_row(
                    theme,
                    strings.preferences_window_remember_bounds.clone(),
                    remember_toggle,
                ),
            ],
        )
    }

    pub(crate) fn render_status_bar_page(
        &self,
        theme: &Theme,
        strings: &crate::i18n::I18nStrings,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // 状态栏开关：五个都在同一张卡片里（侧栏按钮、模式切换之前只存着没入口）。
        let switch = |id: &'static str,
                      checked: bool,
                      on_click: fn(&mut Self, &ClickEvent, &mut Window, &mut Context<Self>),
                      cx: &mut Context<Self>| {
            Switch::new(id).checked(checked).on_click(cx.listener(on_click))
        };

        self.settings_card(
            theme,
            vec![
                self.settings_row(
                    theme,
                    strings.preferences_status_bar_enabled.clone(),
                    switch("preferences-status-bar-enabled", self.status_bar_enabled, |this, _, _, cx| {
                        this.status_bar_enabled = !this.status_bar_enabled;
                        cx.notify();
                    }, cx),
                ),
                self.settings_row(
                    theme,
                    strings.preferences_status_bar_show_word_count.clone(),
                    switch("preferences-status-bar-word-count", self.status_bar_show_word_count, |this, _, _, cx| {
                        this.status_bar_show_word_count = !this.status_bar_show_word_count;
                        cx.notify();
                    }, cx),
                ),
                self.settings_row(
                    theme,
                    strings.preferences_status_bar_show_cursor_position.clone(),
                    switch("preferences-status-bar-cursor-position", self.status_bar_show_cursor_position, |this, _, _, cx| {
                        this.status_bar_show_cursor_position = !this.status_bar_show_cursor_position;
                        cx.notify();
                    }, cx),
                ),
                self.settings_row(
                    theme,
                    strings.preferences_status_bar_show_sidebar_toggle.clone(),
                    switch("preferences-status-bar-sidebar-toggle", self.status_bar_show_sidebar_toggle, |this, _, _, cx| {
                        this.status_bar_show_sidebar_toggle = !this.status_bar_show_sidebar_toggle;
                        cx.notify();
                    }, cx),
                ),
                self.settings_row(
                    theme,
                    strings.preferences_status_bar_show_mode_switch.clone(),
                    switch("preferences-status-bar-mode-switch", self.status_bar_show_mode_switch, |this, _, _, cx| {
                        this.status_bar_show_mode_switch = !this.status_bar_show_mode_switch;
                        cx.notify();
                    }, cx),
                ),
            ],
        )
    }
}
