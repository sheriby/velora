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
            // 借命令在面板里的那条文案：偏好页与命令面板认的是同一个名字。
            ShortcutCommand::SelectDocument => strings.command_select_document.clone(),
            ShortcutCommand::Copy => strings.preferences_shortcut_copy.clone(),
            ShortcutCommand::Cut => strings.preferences_shortcut_cut.clone(),
            ShortcutCommand::Paste => strings.preferences_shortcut_paste.clone(),
            // 这几条借命令自己在菜单上的那条文案：同一个概念在两个地方出现两次，
            // 用同一份字符串才不会出现「菜单叫这个、偏好页叫那个」。
            ShortcutCommand::PasteAsPlainText => strings.context_menu_paste_as_plain_text.clone(),
            ShortcutCommand::CopyAsMarkdown => strings.context_menu_copy_as_markdown.clone(),
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

    pub(crate) fn shortcut_chip(label: &str, recording: bool, theme: &Theme) -> impl IntoElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        div()
            .min_w(px(42.0))
            .min_h(px(t.ui_text_size(26.0).max(26.0)))
            .px(px(9.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(d.menu_item_radius.min(5.0)))
            .border(px(d.dialog_border_width))
            .border_color(if recording {
                c.dialog_primary_button_bg
            } else {
                c.dialog_border
            })
            .bg(if recording {
                c.selection
            } else {
                c.dialog_secondary_button_bg
            })
            .text_size(px(t.ui_text_size(12.0)))
            .font_weight(FontWeight::MEDIUM)
            .text_color(c.dialog_body)
            .child(SharedString::from(label.to_string()))
    }

    pub(crate) fn shortcut_action_button(
        id: impl Into<ElementId>,
        label: String,
        enabled: bool,
        theme: &Theme,
        on_click: impl Fn(&mut Self, &ClickEvent, &mut Window, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let c = &theme.colors;
        let t = &theme.typography;
        let id = id.into();
        div()
            .id(id.clone())
            .debug_selector(move || id.to_string())
            .min_h(px(t.ui_text_size(28.0).max(28.0)))
            .px(px(6.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(5.0))
            .when(enabled, |this| {
                this.cursor_pointer()
                    .hover(|this| this.bg(c.dialog_secondary_button_hover))
            })
            .when(!enabled, |this| this.opacity(0.35))
            .text_size(px(t.ui_text_size(12.0)))
            .text_color(c.dialog_muted)
            .child(label)
            .on_click(cx.listener(move |this, event, window, cx| {
                if enabled {
                    on_click(this, event, window, cx);
                }
            }))
    }

    pub(crate) fn render_shortcut_row(
        &self,
        definition: ShortcutDefinition,
        keybindings: &BTreeMap<String, Vec<String>>,
        theme: &Theme,
        strings: &crate::i18n::I18nStrings,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let c = &theme.colors;
        let t = &theme.typography;
        let is_recording = self.recording_shortcut == Some(definition.command);
        let keys = resolved_shortcut_keys(keybindings, definition.command);
        let is_custom = keybindings.contains_key(definition.id);
        let label = Self::shortcut_command_label(definition.command, strings);
        let command = definition.command;

        let mut chips = div().flex().flex_wrap().justify_end().gap(px(5.0));
        if is_recording {
            chips = chips.child(Self::shortcut_chip(
                &strings.preferences_shortcut_recording,
                true,
                theme,
            ));
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
                chips = chips.child(div().text_color(c.dialog_muted).child("—"));
            }
            for key in displayed {
                let label = match Keystroke::parse(&key) {
                    Ok(keystroke) => keystroke
                        .to_string()
                        .replace("ctrl-", "Ctrl+")
                        .replace("alt-", "Alt+")
                        .replace("shift-", "Shift+"),
                    Err(error) => {
                        eprintln!("显示快捷键失败：{error}");
                        key
                    }
                };
                chips = chips.child(Self::shortcut_chip(&label, false, theme));
            }
        }

        div()
            .id(SharedString::from(format!(
                "preferences-shortcut-row-{}",
                definition.id
            )))
            .debug_selector(move || format!("preferences-shortcut-row-{}", definition.id))
            .w_full()
            .min_h(px(t.ui_text_size(42.0).max(42.0)))
            .px(px(8.0))
            .py(px(5.0))
            .flex()
            .items_center()
            .gap(px(12.0))
            .hover(|this| this.bg(c.dialog_secondary_button_hover))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .truncate()
                    .text_size(px(t.ui_text_size(13.0)))
                    .text_color(c.dialog_body)
                    .child(label),
            )
            .child(div().max_w(px(240.0)).min_w(px(0.0)).child(chips))
            .child(
                div()
                    .flex_shrink_0()
                    .flex()
                    .gap(px(3.0))
                    .child(Self::shortcut_action_button(
                        ("preferences-shortcut-record", definition.command as u32),
                        strings.preferences_shortcut_record.clone(),
                        true,
                        theme,
                        move |this, event, window, cx| {
                            this.begin_recording_shortcut(command, event, window, cx)
                        },
                        cx,
                    ))
                    .child(Self::shortcut_action_button(
                        ("preferences-shortcut-reset", definition.command as u32),
                        strings.preferences_shortcut_reset.clone(),
                        is_custom || is_recording,
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
        let keybindings = normalize_shortcut_config(&self.keybindings);

        let categories = [
            ShortcutCategory::File,
            ShortcutCategory::Edit,
            ShortcutCategory::Navigation,
            ShortcutCategory::Formatting,
            ShortcutCategory::Block,
            ShortcutCategory::Other,
        ];

        let mut page = div()
            .w_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap(px(20.0))
            .child(
                div()
                    .px(px(8.0))
                    .text_size(px(t.ui_text_size(12.0)))
                    .text_color(c.dialog_muted)
                    .child(strings.preferences_shortcuts_hint.clone()),
            );
        for category in categories {
            let rows = shortcut_definitions()
                .iter()
                .copied()
                .filter(|definition| definition.category == category)
                .map(|definition| {
                    self.render_shortcut_row(definition, &keybindings, theme, strings, cx)
                        .into_any_element()
                })
                .collect::<Vec<_>>();
            if rows.is_empty() {
                continue;
            }
            let count = rows.len();
            let mut separator = c.dialog_border;
            separator.a *= 0.45;
            let mut list = div().w_full().flex().flex_col();
            for (index, row) in rows.into_iter().enumerate() {
                if index > 0 {
                    list = list.child(div().h(px(1.0)).mx(px(8.0)).bg(separator));
                }
                list = list.child(row);
            }
            page = page.child(
                div()
                    .w_full()
                    .flex_shrink_0()
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(
                        div()
                            .px(px(8.0))
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .text_size(px(t.ui_text_size(12.0)))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(c.dialog_muted)
                            .child(Self::shortcut_category_label(category, strings))
                            .child(
                                div()
                                    .text_size(px(t.ui_text_size(11.0)))
                                    .opacity(0.65)
                                    .child(count.to_string()),
                            ),
                    )
                    .child(list),
            );
        }
        page.into_any_element()
    }

    pub(crate) fn render_window_page(
        &mut self,
        _theme: &Theme,
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
            |preferences, value| {
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
            |preferences, value| {
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
            |preferences, value| {
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
        )
    }

    pub(crate) fn render_status_bar_page(
        &self,
        _theme: &Theme,
        strings: &crate::i18n::I18nStrings,
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
        )
    }
}
