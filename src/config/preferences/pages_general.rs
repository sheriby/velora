use super::controls::PreferenceChoice;
use super::*;

impl PreferencesWindow {
    pub(crate) fn render_startup_page(
        &mut self,
        _theme: &Theme,
        strings: &crate::i18n::I18nStrings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let startup = self.choice_field(
            "preferences-startup-dropdown",
            strings.preferences_startup_option.clone(),
            self.startup_open.as_str(),
            |_| {
                vec![
                    PreferenceChoice::new(
                        "new_file",
                        strings.preferences_startup_new_file.clone(),
                        "preferences-startup-new-file",
                    ),
                    PreferenceChoice::new(
                        "last_opened_file",
                        strings.preferences_startup_last_opened_file.clone(),
                        "preferences-startup-last-opened-file",
                    ),
                ]
            },
            false,
            window,
            cx,
            |this, value, _, _| this.startup_open = StartupOpenPreference::from_str(value),
        );
        let sidebar_open = self.choice_field(
            "preferences-sidebar-open-dropdown",
            strings.preferences_sidebar_open.clone(),
            self.sidebar_open.as_str(),
            |_| {
                [
                    (
                        SidebarOpenPreference::FollowLast,
                        strings.preferences_sidebar_follow_last.clone(),
                    ),
                    (
                        SidebarOpenPreference::Always,
                        strings.preferences_sidebar_open_always.clone(),
                    ),
                    (
                        SidebarOpenPreference::Never,
                        strings.preferences_sidebar_open_never.clone(),
                    ),
                ]
                .into_iter()
                .map(|(value, label)| {
                    PreferenceChoice::new(
                        value.as_str(),
                        label,
                        format!("preferences-sidebar-open-{}", value.as_str()),
                    )
                })
                .collect()
            },
            false,
            window,
            cx,
            |this, value, _, _| this.sidebar_open = SidebarOpenPreference::from_str(value),
        );
        let sidebar_panel = self.choice_field(
            "preferences-sidebar-panel-dropdown",
            strings.preferences_sidebar_panel.clone(),
            self.sidebar_panel.as_str(),
            |_| {
                [
                    (
                        SidebarPanelPreference::FollowLast,
                        strings.preferences_sidebar_follow_last.clone(),
                    ),
                    (
                        SidebarPanelPreference::Files,
                        strings.workspace_tab_files.clone(),
                    ),
                    (
                        SidebarPanelPreference::Outline,
                        strings.workspace_tab_outline.clone(),
                    ),
                ]
                .into_iter()
                .map(|(value, label)| {
                    PreferenceChoice::new(
                        value.as_str(),
                        label,
                        format!("preferences-sidebar-panel-{}", value.as_str()),
                    )
                })
                .collect()
            },
            false,
            window,
            cx,
            |this, value, _, _| this.sidebar_panel = SidebarPanelPreference::from_str(value),
        );
        let tree_sort = self.choice_field(
            "preferences-tree-sort-dropdown",
            strings.preferences_file_tree_sort.clone(),
            self.tree_sort.as_str(),
            |_| {
                [
                    (TreeSortPreference::Name, strings.tree_sort_name.clone()),
                    (
                        TreeSortPreference::ModifiedTime,
                        strings.tree_sort_mtime.clone(),
                    ),
                    (TreeSortPreference::Type, strings.tree_sort_type.clone()),
                ]
                .into_iter()
                .enumerate()
                .map(|(index, (value, label))| {
                    PreferenceChoice::new(
                        value.as_str(),
                        label,
                        format!("preferences-tree-sort-{index}"),
                    )
                })
                .collect()
            },
            false,
            window,
            cx,
            |this, value, _, _| this.tree_sort = TreeSortPreference::from_str(value),
        );
        let folder_icons = self.choice_field(
            "preferences-folder-icon-style",
            strings.preferences_folder_icon_style.clone(),
            self.folder_icon_style.as_str(),
            |_| {
                [
                    (FolderIconStyle::Blue, strings.folder_icon_blue.clone()),
                    (
                        FolderIconStyle::Outline,
                        strings.folder_icon_outline.clone(),
                    ),
                ]
                .into_iter()
                .map(|(value, label)| {
                    PreferenceChoice::new(
                        value.as_str(),
                        label,
                        format!("preferences-folder-icon-{}", value.as_str()),
                    )
                })
                .collect()
            },
            false,
            window,
            cx,
            |this, value, _, _| this.folder_icon_style = FolderIconStyle::from_str(value),
        );
        let debounce = self.choice_field(
            "preferences-autosave-dropdown",
            strings.preferences_file_autosave_debounce.clone(),
            self.autosave_debounce_ms.to_string(),
            |preferences| {
                let mut values = vec![800u64, 2000, 5000];
                if !values.contains(&preferences.autosave_debounce_ms) {
                    values.push(preferences.autosave_debounce_ms);
                    values.sort_unstable();
                }
                values
                    .into_iter()
                    .map(|value| {
                        PreferenceChoice::new(
                            value.to_string(),
                            format!("{value} ms"),
                            format!("preferences-autosave-{value}"),
                        )
                    })
                    .collect()
            },
            false,
            window,
            cx,
            |this, value, _, _| {
                if let Ok(value) = value.parse() {
                    this.autosave_debounce_ms = value;
                }
            },
        );
        let external = self.choice_field(
            "preferences-external-change-dropdown",
            strings.preferences_file_external_change.clone(),
            self.external_change_policy.as_str(),
            |_| {
                [
                    (
                        ExternalChangePolicy::Auto,
                        strings.preferences_external_change_auto.clone(),
                    ),
                    (
                        ExternalChangePolicy::Manual,
                        strings.preferences_external_change_manual.clone(),
                    ),
                ]
                .into_iter()
                .map(|(value, label)| {
                    PreferenceChoice::new(
                        value.as_str(),
                        label,
                        format!("preferences-external-change-{}", value.as_str()),
                    )
                })
                .collect()
            },
            false,
            window,
            cx,
            |this, value, _, _| this.external_change_policy = ExternalChangePolicy::from_str(value),
        );
        let deletion = self.choice_field(
            "preferences-delete-policy-dropdown",
            strings.preferences_file_delete_policy.clone(),
            self.delete_policy.as_str(),
            |_| {
                [
                    (
                        DeletePolicy::Trash,
                        strings.preferences_delete_policy_trash.clone(),
                    ),
                    (
                        DeletePolicy::Permanent,
                        strings.preferences_delete_policy_permanent.clone(),
                    ),
                ]
                .into_iter()
                .map(|(value, label)| {
                    PreferenceChoice::new(
                        value.as_str(),
                        label,
                        format!("preferences-delete-policy-{}", value.as_str()),
                    )
                })
                .collect()
            },
            false,
            window,
            cx,
            |this, value, _, _| this.delete_policy = DeletePolicy::from_str(value),
        );
        let interval = self.number_field(
            "preferences-history-interval",
            strings.preferences_history_interval.clone(),
            self.file_history.interval_minutes as f64,
            0.0..=1440.0,
            window,
            cx,
            |this, value| this.file_history.interval_minutes = value as u64,
        );
        let count = self.number_field(
            "preferences-history-count",
            strings.preferences_history_count.clone(),
            self.file_history.max_entries as f64,
            1.0..=1000.0,
            window,
            cx,
            |this, value| this.file_history.max_entries = value as usize,
        );
        let days = self.number_field(
            "preferences-history-days",
            strings.preferences_history_days.clone(),
            self.file_history.retention_days as f64,
            1.0..=3650.0,
            window,
            cx,
            |this, value| this.file_history.retention_days = value as u64,
        );
        let autosave = Self::boolean_field(
            "preferences-autosave",
            strings.preferences_autosave.clone(),
            self.autosave,
            cx,
            |this, value| this.autosave = value,
        );
        let smart = Self::boolean_field(
            "preferences-smart-punctuation",
            strings.preferences_smart_punctuation.clone(),
            self.smart_punctuation,
            cx,
            |this, value| this.smart_punctuation = value,
        );
        let markers = Self::boolean_field(
            "preferences-hide-markdown-markers",
            strings.preferences_hide_markdown_markers.clone(),
            self.hide_markdown_markers,
            cx,
            |this, value| this.hide_markdown_markers = value,
        );
        let updates = Self::boolean_field(
            "preferences-update-startup",
            strings.preferences_updates_startup.clone(),
            self.check_updates_on_startup,
            cx,
            |this, value| this.check_updates_on_startup = value,
        );
        let beta = Self::boolean_field(
            "preferences-update-beta",
            strings.preferences_updates_beta.clone(),
            self.include_prereleases,
            cx,
            |this, value| this.include_prereleases = value,
        );
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap_6()
            .children([
                Self::preference_group(
                    "preferences-startup-group",
                    strings.preferences_group_startup.clone(),
                    vec![
                        Self::preference_row(strings.preferences_startup_option.clone(), startup),
                        Self::preference_row(
                            strings.preferences_sidebar_open.clone(),
                            sidebar_open,
                        ),
                        Self::preference_row(
                            strings.preferences_sidebar_panel.clone(),
                            sidebar_panel,
                        ),
                    ],
                    window,
                    PREFERENCE_CHOICE_WIDTH,
                ),
                Self::preference_group(
                    "preferences-workspace-group",
                    strings.preferences_group_workspace.clone(),
                    vec![
                        Self::preference_row(strings.preferences_file_tree_sort.clone(), tree_sort),
                        Self::preference_row(
                            strings.preferences_folder_icon_style.clone(),
                            folder_icons,
                        ),
                        Self::preference_row(
                            strings.preferences_file_external_change.clone(),
                            external,
                        ),
                        Self::preference_row(
                            strings.preferences_file_delete_policy.clone(),
                            deletion,
                        ),
                    ],
                    window,
                    PREFERENCE_CHOICE_WIDTH,
                ),
                Self::preference_group(
                    "preferences-saving-group",
                    strings.preferences_group_saving.clone(),
                    vec![
                        Self::preference_row(strings.preferences_autosave.clone(), autosave),
                        Self::preference_row(
                            strings.preferences_file_autosave_debounce.clone(),
                            debounce,
                        ),
                    ],
                    window,
                    PREFERENCE_CHOICE_WIDTH,
                ),
                Self::preference_group(
                    "preferences-history-group",
                    strings.preferences_group_history.clone(),
                    vec![
                        Self::preference_row(
                            strings.preferences_history_interval.clone(),
                            interval,
                        ),
                        Self::preference_row(strings.preferences_history_count.clone(), count),
                        Self::preference_row(strings.preferences_history_days.clone(), days),
                    ],
                    window,
                    PREFERENCE_NUMBER_WIDTH,
                ),
                Self::preference_group(
                    "preferences-editing-group",
                    strings.preferences_group_editing.clone(),
                    vec![
                        Self::preference_row(strings.preferences_smart_punctuation.clone(), smart),
                        Self::preference_row(
                            strings.preferences_hide_markdown_markers.clone(),
                            markers,
                        ),
                    ],
                    window,
                    rems(3.0),
                ),
                Self::preference_group(
                    "preferences-updates-group",
                    strings.preferences_group_updates.clone(),
                    vec![
                        Self::preference_row(strings.preferences_updates_startup.clone(), updates),
                        Self::preference_row(strings.preferences_updates_beta.clone(), beta),
                    ],
                    window,
                    rems(3.0),
                ),
            ])
            .into_any_element()
    }

    pub(crate) fn render_theme_page(
        &mut self,
        _theme: &Theme,
        strings: &crate::i18n::I18nStrings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = self.choice_field(
            "preferences-theme-dropdown",
            strings.preferences_local_theme.clone(),
            self.selected_theme_id.clone(),
            |this| {
                this.theme_options
                    .iter()
                    .enumerate()
                    .map(|(index, entry)| {
                        PreferenceChoice::new(
                            entry.id.clone(),
                            this.theme_display_name(entry, strings),
                            format!("preferences-theme-option-{index}"),
                        )
                    })
                    .collect()
            },
            false,
            window,
            cx,
            |this, value, _, _| this.selected_theme_id = value.to_owned(),
        );
        let chinese = cx.global::<I18nManager>().current_language_id() == "zh-CN";
        let width = self.choice_field(
            "preferences-writing-width",
            strings.preferences_writing_width.clone(),
            self.writing_width.as_str(),
            |_| {
                [
                    WritingWidthPreference::Theme,
                    WritingWidthPreference::Compact,
                    WritingWidthPreference::Standard,
                    WritingWidthPreference::Wide,
                ]
                .into_iter()
                .enumerate()
                .map(|(index, value)| {
                    let label = match (chinese, value) {
                        (true, WritingWidthPreference::Theme) => "跟随主题",
                        (true, WritingWidthPreference::Compact) => "紧凑 · 50%",
                        (true, WritingWidthPreference::Standard) => "标准 · 62%",
                        (true, WritingWidthPreference::Wide) => "宽敞 · 75%",
                        (false, WritingWidthPreference::Theme) => "Follow theme",
                        (false, WritingWidthPreference::Compact) => "Compact · 50%",
                        (false, WritingWidthPreference::Standard) => "Standard · 62%",
                        (false, WritingWidthPreference::Wide) => "Wide · 75%",
                    };
                    PreferenceChoice::new(
                        value.as_str(),
                        label,
                        format!("preferences-writing-width-option-{index}"),
                    )
                })
                .collect()
            },
            false,
            window,
            cx,
            |this, value, _, _| this.writing_width = WritingWidthPreference::from_str(value),
        );
        let appearance = Self::preference_group(
            "preferences-appearance-group",
            String::new(),
            vec![
                Self::preference_row(strings.preferences_local_theme.clone(), theme),
                Self::preference_row(strings.preferences_writing_width.clone(), width),
            ],
            window,
            PREFERENCE_CHOICE_WIDTH,
        );
        let font_controls_width = PREFERENCE_CHOICE_WIDTH + PREFERENCE_NUMBER_WIDTH + rems(0.5);
        let fonts_horizontal = Self::preference_group_is_horizontal(window, font_controls_width);
        let mut fonts = Vec::new();
        for (role, label, font_label, size_label) in [
            (
                FontRole::Ui,
                strings.preferences_group_interface.clone(),
                strings.preferences_ui_font.clone(),
                strings.preferences_ui_font_size.clone(),
            ),
            (
                FontRole::Body,
                strings.preferences_group_document.clone(),
                strings.preferences_body_font.clone(),
                strings.preferences_body_font_size.clone(),
            ),
            (
                FontRole::Code,
                strings.preferences_group_code.clone(),
                strings.preferences_code_font.clone(),
                strings.preferences_code_font_size.clone(),
            ),
        ] {
            let font = self
                .font_family_field(role, &font_label, strings, window, cx)
                .when(!fonts_horizontal, |picker| {
                    picker.w_0().flex_1().min_w(rems(6.0))
                });
            let size = self.font_size_field(role, &size_label, window, cx);
            fonts.push(Self::preference_row(
                label,
                div()
                    .w_full()
                    .min_w_0()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .justify_end()
                    .gap_2()
                    .child(font)
                    .child(size),
            ));
        }
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap_4()
            .child(appearance)
            .child(Self::preference_group(
                "preferences-fonts-group",
                strings.preferences_group_fonts.clone(),
                fonts,
                window,
                font_controls_width,
            ))
            .into_any_element()
    }

    fn font_family_field(
        &mut self,
        role: FontRole,
        label: &str,
        strings: &crate::i18n::I18nStrings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let (id, option_id, value) = match role {
            FontRole::Ui => (
                "preferences-ui-font",
                "preferences-ui-font-option",
                self.fonts.ui_family.clone(),
            ),
            FontRole::Body => (
                "preferences-markdown-font",
                "preferences-markdown-font-option",
                self.fonts.markdown_family.clone(),
            ),
            FontRole::Code => (
                "preferences-code-font",
                "preferences-code-font-option",
                self.fonts.code_family.clone(),
            ),
        };
        self.choice_field(
            id,
            label.to_owned(),
            value,
            |this| {
                let mut families = this.system_font_families.clone();
                if !families.iter().any(|family| family == ".SystemUIFont") {
                    families.push(".SystemUIFont".into());
                }
                if role == FontRole::Body {
                    families.insert(0, "theme".into());
                }
                families
                    .into_iter()
                    .enumerate()
                    .map(|(index, value)| {
                        let label = match value.as_str() {
                            ".SystemUIFont" => strings.preferences_system_font.clone(),
                            "theme" => strings.preferences_theme_font.clone(),
                            _ => value.clone(),
                        };
                        PreferenceChoice::new(value, label, format!("{option_id}-{index}"))
                    })
                    .collect()
            },
            true,
            window,
            cx,
            move |this, value, _, _| match role {
                FontRole::Ui => this.fonts.ui_family = value.to_owned(),
                FontRole::Body => this.fonts.markdown_family = value.to_owned(),
                FontRole::Code => this.fonts.code_family = value.to_owned(),
            },
        )
    }

    fn font_size_field(
        &mut self,
        role: FontRole,
        label: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (id, value) = match role {
            FontRole::Ui => ("preferences-ui-font-size", self.fonts.ui_size),
            FontRole::Body => ("preferences-markdown-font-size", self.fonts.markdown_size),
            FontRole::Code => ("preferences-code-font-size", self.fonts.code_size),
        };
        self.number_field(
            id,
            label.to_owned(),
            f64::from(value),
            10.0..=36.0,
            window,
            cx,
            move |this, value| match role {
                FontRole::Ui => this.fonts.ui_size = value as u16,
                FontRole::Body => this.fonts.markdown_size = value as u16,
                FontRole::Code => this.fonts.code_size = value as u16,
            },
        )
    }

    pub(crate) fn image_paste_behavior_label(
        behavior: ImagePasteBehavior,
        strings: &crate::i18n::I18nStrings,
    ) -> String {
        match behavior {
            ImagePasteBehavior::None => strings.preferences_image_paste_none.clone(),
            ImagePasteBehavior::CopyToDocumentFolder => strings
                .preferences_image_paste_copy_to_document_folder
                .clone(),
            ImagePasteBehavior::CopyToAssetsFolder => strings
                .preferences_image_paste_copy_to_assets_folder
                .clone(),
            ImagePasteBehavior::CopyToNamedAssetsFolder => strings
                .preferences_image_paste_copy_to_named_assets_folder
                .clone(),
        }
    }

    pub(crate) fn render_image_page(
        &mut self,
        _theme: &Theme,
        strings: &crate::i18n::I18nStrings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let image = self.choice_field(
            "preferences-image-dropdown",
            strings.preferences_image_insert_behavior.clone(),
            self.image_paste_behavior.as_str(),
            |_| {
                [
                    ImagePasteBehavior::None,
                    ImagePasteBehavior::CopyToDocumentFolder,
                    ImagePasteBehavior::CopyToAssetsFolder,
                    ImagePasteBehavior::CopyToNamedAssetsFolder,
                ]
                .into_iter()
                .enumerate()
                .map(|(index, value)| {
                    PreferenceChoice::new(
                        value.as_str(),
                        Self::image_paste_behavior_label(value, strings),
                        format!("preferences-image-option-{index}"),
                    )
                })
                .collect()
            },
            false,
            window,
            cx,
            |this, value, _, _| this.image_paste_behavior = ImagePasteBehavior::from_str(value),
        );
        Self::preference_group(
            "preferences-images-group",
            strings.preferences_image_group.clone(),
            vec![Self::preference_row(
                strings.preferences_image_insert_behavior.clone(),
                image,
            )],
            window,
            PREFERENCE_CHOICE_WIDTH,
        )
    }
}
