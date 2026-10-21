use super::*;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Sizable as _,
    button::{Button, ButtonVariants},
    form::Field,
    kbd::Kbd,
};

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
            // 借命令在面板里的那条文案：偏好页与命令面板认的是同一个名字。
            ShortcutCommand::SelectDocument => strings.command_select_document.clone(),
            ShortcutCommand::Copy => strings.preferences_shortcut_copy.clone(),
            ShortcutCommand::Cut => strings.preferences_shortcut_cut.clone(),
            ShortcutCommand::Paste => strings.preferences_shortcut_paste.clone(),
            // 这几条借命令自己在菜单上的那条文案：同一个概念在两个地方出现两次，
            // 用同一份字符串才不会出现「菜单叫这个、偏好页叫那个」。
            ShortcutCommand::PasteAsPlainText => strings.context_menu_paste_as_plain_text.clone(),
            ShortcutCommand::CopyAsMarkdown => strings.context_menu_copy_as_markdown.clone(),
            ShortcutCommand::CopyAsPlainText => {
                strings.context_menu_copy_as_plain_text.clone()
            }
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
            ShortcutCommand::StrikethroughSelection => {
                strings.format_strikethrough.clone()
            }
            ShortcutCommand::SuperscriptSelection => {
                strings.format_superscript.clone()
            }
            ShortcutCommand::SubscriptSelection => strings.format_subscript.clone(),
            ShortcutCommand::HighlightSelection => strings.format_highlight.clone(),
            ShortcutCommand::ClearFormatSelection => strings.format_clear.clone(),
            ShortcutCommand::LinkSelection => strings.insert_link.clone(),
            ShortcutCommand::InsertImage => strings.insert_image.clone(),
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
            ShortcutCommand::NewDocument => strings.preferences_shortcut_new_document.clone(),
            ShortcutCommand::OpenFile => strings.preferences_shortcut_open_file.clone(),
            ShortcutCommand::QuitApplication => {
                strings.preferences_shortcut_quit_application.clone()
            }
            ShortcutCommand::SplitEditorLeft => strings.split_editor_left.clone(),
            ShortcutCommand::SplitEditorRight => strings.split_editor_right.clone(),
            ShortcutCommand::SplitEditorUp => strings.split_editor_up.clone(),
            ShortcutCommand::SplitEditorDown => strings.split_editor_down.clone(),
            ShortcutCommand::CloseTab => strings.preferences_shortcut_close_tab.clone(),
            ShortcutCommand::CloseWindow => strings.preferences_shortcut_close_window.clone(),
            ShortcutCommand::DismissTransientUi => {
                strings.preferences_shortcut_dismiss_transient_ui.clone()
            }
            ShortcutCommand::ToggleReadingMode => strings.command_toggle_reading_mode.clone(),
            ShortcutCommand::ToggleViewMode => {
                strings.preferences_shortcut_toggle_view_mode.clone()
            }
            ShortcutCommand::FindInDocument => {
                strings.preferences_shortcut_find_in_document.clone()
            }
            ShortcutCommand::OpenAiAssistant => strings.preferences_shortcut_open_ai_assistant.clone(),
            ShortcutCommand::ToggleAiSidebar => strings.ai_sidebar.clone(),
            ShortcutCommand::AiPolish => format!("AI {}", strings.ai_action_polish),
            ShortcutCommand::AiTranslate => format!("AI {}", strings.ai_action_translate),
            ShortcutCommand::AiSummarize => format!("AI {}", strings.ai_action_summarize),
            ShortcutCommand::AiExplain => format!("AI {}", strings.ai_action_explain),
            ShortcutCommand::AiExpand => format!("AI {}", strings.ai_action_expand),
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
        window.focus(&self.focus_handle, cx);
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

    pub(super) fn preferred_shortcut(keys: &[String], macos: bool) -> Option<&String> {
        keys.iter()
            .find(|key| {
                Keystroke::parse(key).is_ok_and(|keystroke| {
                    if macos {
                        keystroke.modifiers.platform
                    } else {
                        keystroke.modifiers.control && !keystroke.modifiers.platform
                    }
                })
            })
            .or_else(|| {
                keys.iter().find(|key| {
                    Keystroke::parse(key).is_ok_and(|keystroke| {
                        !keystroke.modifiers.platform
                            && (!macos || (keystroke.modifiers.alt && !keystroke.modifiers.control))
                    })
                })
            })
            .or_else(|| keys.first())
    }

    pub(super) fn shortcut_display_stroke(stroke: Keystroke) -> Keystroke {
        // Kit 的 macOS 显示把 delete 与 backspace 合成了同一个符号；绑定仍保留原键名。
        #[cfg(target_os = "macos")]
        if stroke.key == "delete" {
            let mut display_stroke = stroke;
            display_stroke.key = "⌦".into();
            return display_stroke;
        }
        stroke
    }

    pub(crate) fn render_shortcut_row(
        &self,
        definition: ShortcutDefinition,
        keybindings: &BTreeMap<String, Vec<String>>,
        strings: &crate::i18n::I18nStrings,
        cx: &mut Context<Self>,
    ) -> Field {
        let is_recording = self.recording_shortcut == Some(definition.command);
        let keys = resolved_shortcut_keys(keybindings, definition.command);
        let is_custom = keybindings.contains_key(definition.id);
        let label = Self::shortcut_command_label(definition.command, strings);
        let command = definition.command;
        let mut keys_view = div().min_w_0().flex().flex_wrap().justify_end().gap_1();
        if is_recording {
            keys_view = keys_view.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().primary)
                    .child(strings.preferences_shortcut_recording.clone()),
            );
        } else {
            let displayed = if is_custom {
                keys
            } else {
                Self::preferred_shortcut(&keys, cfg!(target_os = "macos"))
                    .into_iter()
                    .cloned()
                    .collect()
            };
            if displayed.is_empty() {
                keys_view =
                    keys_view.child(div().text_color(cx.theme().muted_foreground).child("—"));
            }
            for key in displayed {
                keys_view = match Keystroke::parse(&key) {
                    Ok(stroke) => keys_view.child(Kbd::new(Self::shortcut_display_stroke(stroke))),
                    Err(error) => {
                        eprintln!("显示快捷键失败：{error}");
                        keys_view.child(key)
                    }
                };
            }
        }
        let record = Button::new(("preferences-shortcut-record", command as u32))
            .debug_selector(move || format!("preferences-shortcut-record-{}", command as u32))
            .ghost()
            .small()
            .label(strings.preferences_shortcut_record.clone())
            .accessibility_label(format!("{}: {label}", strings.preferences_shortcut_record))
            .on_click(cx.listener(move |this, event, window, cx| {
                this.begin_recording_shortcut(command, event, window, cx);
            }));
        let reset = Button::new(("preferences-shortcut-reset", command as u32))
            .debug_selector(move || format!("preferences-shortcut-reset-{}", command as u32))
            .ghost()
            .small()
            .label(strings.preferences_shortcut_reset.clone())
            .accessibility_label(format!("{}: {label}", strings.preferences_shortcut_reset))
            .disabled(!is_custom && !is_recording)
            .on_click(cx.listener(move |this, event, window, cx| {
                this.reset_shortcut(command, event, window, cx);
            }));
        let controls = div()
            .w_full()
            .min_w_0()
            .flex()
            .flex_wrap()
            .items_center()
            .justify_end()
            .gap_2()
            .child(keys_view)
            .child(div().flex_none().flex().gap_1().child(record).child(reset));
        let field = Self::preference_row(label, controls);
        if is_recording {
            if let Some(error) = self.shortcut_error.clone() {
                return field.description_fn(move |_, cx| {
                    div()
                        .text_sm()
                        .text_color(cx.theme().danger)
                        .child(error.clone())
                });
            }
            return field.description(strings.preferences_shortcuts_hint.clone());
        }
        field
    }

    pub(crate) fn render_shortcuts_page(
        &self,
        theme: &Theme,
        strings: &crate::i18n::I18nStrings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let keybindings = normalize_shortcut_config(&self.keybindings);
        let categories = [
            (ShortcutCategory::File, "preferences-shortcuts-file"),
            (ShortcutCategory::Edit, "preferences-shortcuts-edit"),
            (
                ShortcutCategory::Navigation,
                "preferences-shortcuts-navigation",
            ),
            (
                ShortcutCategory::Formatting,
                "preferences-shortcuts-formatting",
            ),
            (ShortcutCategory::Block, "preferences-shortcuts-block"),
            (ShortcutCategory::Other, "preferences-shortcuts-other"),
        ];
        let mut page = div().w_full().flex_shrink_0().flex().flex_col().gap_6();
        for (category, id) in categories {
            let fields = shortcut_definitions()
                .iter()
                .copied()
                .filter(|definition| definition.category == category)
                .map(|definition| self.render_shortcut_row(definition, &keybindings, strings, cx))
                .collect::<Vec<_>>();
            if !fields.is_empty() {
                page = page.child(Self::preference_group(
                    id,
                    Self::shortcut_category_label(category, strings),
                    fields,
                    theme,
                    window,
                    rems(18.0),
                ));
            }
        }
        page.into_any_element()
    }

    pub(crate) fn render_window_page(
        &mut self,
        theme: &Theme,
        strings: &crate::i18n::I18nStrings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use super::controls::PreferenceChoice;
        let zoom = self.choice_field(
            "preferences-zoom-dropdown",
            strings.preferences_window_zoom.clone(),
            self.zoom_percent.to_string(),
            |preferences| {
                let mut values = vec![80i64, 90, 100, 110, 125, 150];
                if !values.contains(&preferences.zoom_percent) {
                    values.push(preferences.zoom_percent);
                    values.sort_unstable();
                }
                values
                    .into_iter()
                    .map(|percent| {
                        PreferenceChoice::new(
                            percent.to_string(),
                            format!("{percent}%"),
                            format!("preferences-zoom-{percent}"),
                        )
                    })
                    .collect()
            },
            false,
            window,
            cx,
            |preferences, value, _, _| {
                if let Ok(percent) = value.parse() {
                    preferences.zoom_percent = percent;
                }
            },
        );
        let size = self.choice_field(
            "preferences-window-size-dropdown",
            strings.preferences_window_default_size.clone(),
            format!(
                "{}x{}",
                self.default_window_width, self.default_window_height
            ),
            |preferences| {
                let mut values = vec![(900i64, 600i64), (1080, 720), (1280, 800), (1440, 900)];
                let current = (
                    preferences.default_window_width,
                    preferences.default_window_height,
                );
                if !values.contains(&current) {
                    values.push(current);
                    values.sort_unstable();
                }
                values
                    .into_iter()
                    .map(|(width, height)| {
                        PreferenceChoice::new(
                            format!("{width}x{height}"),
                            format!("{width} × {height}"),
                            format!("preferences-window-size-{width}x{height}"),
                        )
                    })
                    .collect()
            },
            false,
            window,
            cx,
            |preferences, value, _, _| {
                if let Some((width, height)) = value.split_once('x') {
                    if let (Ok(width), Ok(height)) = (width.parse(), height.parse()) {
                        preferences.default_window_width = width;
                        preferences.default_window_height = height;
                    }
                }
            },
        );
        let position_choices = vec![
            PreferenceChoice::new(
                "remember",
                strings.preferences_window_open_position_remember.clone(),
                "preferences-window-open-position-remember",
            ),
            PreferenceChoice::new(
                "center",
                strings.preferences_window_open_position_center.clone(),
                "preferences-window-open-position-center",
            ),
        ];
        let position = self.choice_field(
            "preferences-window-open-position-dropdown",
            strings.preferences_window_open_position.clone(),
            self.window_open_position.as_str(),
            |_| position_choices,
            false,
            window,
            cx,
            |preferences, value, _, _| {
                preferences.window_open_position = if value == "center" {
                    WindowOpenPosition::Center
                } else {
                    WindowOpenPosition::Remember
                };
            },
        );
        let remember = Self::boolean_field(
            "preferences-remember-window",
            strings.preferences_window_remember_bounds.clone(),
            self.remember_window_bounds,
            cx,
            |preferences, value| preferences.remember_window_bounds = value,
        );
        Self::preference_group(
            "preferences-window-fields",
            String::new(),
            vec![
                Self::preference_row(strings.preferences_window_zoom.clone(), zoom),
                Self::preference_row(strings.preferences_window_default_size.clone(), size),
                Self::preference_row(strings.preferences_window_open_position.clone(), position),
                Self::preference_row(strings.preferences_window_remember_bounds.clone(), remember),
            ],
            theme,
            window,
            PREFERENCE_CHOICE_WIDTH,
        )
    }

    pub(crate) fn render_status_bar_page(
        &self,
        theme: &Theme,
        strings: &crate::i18n::I18nStrings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        Self::preference_group(
            "preferences-status-bar-fields",
            String::new(),
            vec![
                Self::preference_row(
                    strings.preferences_status_bar_enabled.clone(),
                    Self::boolean_field(
                        "preferences-status-bar-enabled",
                        strings.preferences_status_bar_enabled.clone(),
                        self.status_bar_enabled,
                        cx,
                        |preferences, value| preferences.status_bar_enabled = value,
                    ),
                ),
                Self::preference_row(
                    strings.preferences_status_bar_show_word_count.clone(),
                    Self::boolean_field(
                        "preferences-status-bar-word-count",
                        strings.preferences_status_bar_show_word_count.clone(),
                        self.status_bar_show_word_count,
                        cx,
                        |preferences, value| preferences.status_bar_show_word_count = value,
                    ),
                ),
                Self::preference_row(
                    strings.preferences_status_bar_show_cursor_position.clone(),
                    Self::boolean_field(
                        "preferences-status-bar-cursor-position",
                        strings.preferences_status_bar_show_cursor_position.clone(),
                        self.status_bar_show_cursor_position,
                        cx,
                        |preferences, value| preferences.status_bar_show_cursor_position = value,
                    ),
                ),
                Self::preference_row(
                    strings.preferences_status_bar_show_sidebar_toggle.clone(),
                    Self::boolean_field(
                        "preferences-status-bar-sidebar-toggle",
                        strings.preferences_status_bar_show_sidebar_toggle.clone(),
                        self.status_bar_show_sidebar_toggle,
                        cx,
                        |preferences, value| preferences.status_bar_show_sidebar_toggle = value,
                    ),
                ),
                Self::preference_row(
                    strings.preferences_status_bar_show_mode_switch.clone(),
                    Self::boolean_field(
                        "preferences-status-bar-mode-switch",
                        strings.preferences_status_bar_show_mode_switch.clone(),
                        self.status_bar_show_mode_switch,
                        cx,
                        |preferences, value| preferences.status_bar_show_mode_switch = value,
                    ),
                ),
            ],
            theme,
            window,
            rems(3.0),
        )
    }
}
