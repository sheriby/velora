use super::*;

impl PreferencesWindow {
    pub(crate) fn render_startup_page(
        &self,
        theme: &Theme,
        strings: &crate::i18n::I18nStrings,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = match self.startup_open {
            StartupOpenPreference::NewFile => strings.preferences_startup_new_file.clone(),
            StartupOpenPreference::LastOpenedFile => {
                strings.preferences_startup_last_opened_file.clone()
            }
        };
        let mut dropdown = div()
            .relative()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(Self::dropdown_button(
                "preferences-startup-dropdown",
                selected,
                theme,
                Self::toggle_startup_dropdown,
                cx,
            ));
        if self.startup_dropdown_open {
            let mut list = div().w_full().flex().flex_col().gap(px(4.0));
            let new_file_label = strings.preferences_startup_new_file.clone();
            let last_file_label = strings.preferences_startup_last_opened_file.clone();
            list = list
                .child(Self::dropdown_item(
                    "preferences-startup-new-file",
                    new_file_label,
                    self.startup_open == StartupOpenPreference::NewFile,
                    theme,
                    |this, _, _, cx| {
                        this.startup_open = StartupOpenPreference::NewFile;
                        this.startup_dropdown_open = false;
                        cx.notify();
                    },
                    cx,
                ))
                .child(Self::dropdown_item(
                    "preferences-startup-last-opened-file",
                    last_file_label,
                    self.startup_open == StartupOpenPreference::LastOpenedFile,
                    theme,
                    |this, _, _, cx| {
                        this.startup_open = StartupOpenPreference::LastOpenedFile;
                        this.startup_dropdown_open = false;
                        cx.notify();
                    },
                    cx,
                ));
            dropdown = dropdown.child(self.dropdown_menu("preferences-startup-dropdown", list, theme));
        }
        let sidebar_open_selected = match self.sidebar_open {
            SidebarOpenPreference::FollowLast => strings.preferences_sidebar_follow_last.clone(),
            SidebarOpenPreference::Always => strings.preferences_sidebar_open_always.clone(),
            SidebarOpenPreference::Never => strings.preferences_sidebar_open_never.clone(),
        };
        let mut sidebar_open_dropdown = div()
            .relative()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(Self::dropdown_button(
                "preferences-sidebar-open-dropdown",
                sidebar_open_selected,
                theme,
                Self::toggle_sidebar_open_dropdown,
                cx,
            ));
        if self.sidebar_open_dropdown_open {
            let mut list = div().w_full().flex().flex_col().gap(px(4.0));
            let options = [
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
            ];
            for (value, label) in options {
                list = list.child(Self::dropdown_item(
                    gpui::SharedString::from(format!(
                        "preferences-sidebar-open-{}",
                        value.as_str()
                    )),
                    label,
                    self.sidebar_open == value,
                    theme,
                    move |this, _, _, cx| {
                        this.sidebar_open = value;
                        this.sidebar_open_dropdown_open = false;
                        cx.notify();
                    },
                    cx,
                ));
            }
            sidebar_open_dropdown = sidebar_open_dropdown.child(self.dropdown_menu("preferences-sidebar-open-dropdown", list, theme));
        }

        let sidebar_panel_selected = match self.sidebar_panel {
            SidebarPanelPreference::FollowLast => strings.preferences_sidebar_follow_last.clone(),
            SidebarPanelPreference::Files => strings.workspace_tab_files.clone(),
            SidebarPanelPreference::Outline => strings.workspace_tab_outline.clone(),
        };
        let mut sidebar_panel_dropdown = div()
            .relative()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(Self::dropdown_button(
                "preferences-sidebar-panel-dropdown",
                sidebar_panel_selected,
                theme,
                Self::toggle_sidebar_panel_dropdown,
                cx,
            ));
        if self.sidebar_panel_dropdown_open {
            let mut list = div().w_full().flex().flex_col().gap(px(4.0));
            let options = [
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
            ];
            for (value, label) in options {
                list = list.child(Self::dropdown_item(
                    gpui::SharedString::from(format!(
                        "preferences-sidebar-panel-{}",
                        value.as_str()
                    )),
                    label,
                    self.sidebar_panel == value,
                    theme,
                    move |this, _, _, cx| {
                        this.sidebar_panel = value;
                        this.sidebar_panel_dropdown_open = false;
                        cx.notify();
                    },
                    cx,
                ));
            }
            sidebar_panel_dropdown = sidebar_panel_dropdown.child(self.dropdown_menu("preferences-sidebar-panel-dropdown", list, theme));
        }

        let tree_sort_selected = match self.tree_sort {
            TreeSortPreference::Name => strings.tree_sort_name.clone(),
            TreeSortPreference::ModifiedTime => strings.tree_sort_mtime.clone(),
            TreeSortPreference::Type => strings.tree_sort_type.clone(),
        };
        let mut tree_sort_dropdown = div()
            .relative()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(Self::dropdown_button(
                "preferences-tree-sort-dropdown",
                tree_sort_selected,
                theme,
                Self::toggle_tree_sort_dropdown,
                cx,
            ));
        if self.tree_sort_dropdown_open {
            let mut list = div().w_full().flex().flex_col().gap(px(4.0));
            let options = [
                (TreeSortPreference::Name, strings.tree_sort_name.clone()),
                (TreeSortPreference::ModifiedTime, strings.tree_sort_mtime.clone()),
                (TreeSortPreference::Type, strings.tree_sort_type.clone()),
            ];
            for (index, (sort, label)) in options.into_iter().enumerate() {
                let is_selected = self.tree_sort == sort;
                let sort_value = sort;
                list = list.child(
                    Self::dropdown_item(
                        gpui::SharedString::from(format!("preferences-tree-sort-{index}")),
                        label,
                        is_selected,
                        theme,
                        move |this, _, _, cx| {
                            this.tree_sort = sort_value;
                            this.tree_sort_dropdown_open = false;
                            cx.notify();
                        },
                        cx,
                    ),
                );
            }
            tree_sort_dropdown = tree_sort_dropdown.child(self.dropdown_menu("preferences-tree-sort-dropdown", list, theme));
        }

        let debounce_selected = format!("{} ms", self.autosave_debounce_ms);
        let mut debounce_dropdown = div()
            .relative()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(Self::dropdown_button(
                "preferences-autosave-dropdown",
                debounce_selected.clone().into(),
                theme,
                Self::toggle_autosave_dropdown,
                cx,
            ));
        if self.autosave_dropdown_open {
            let mut list = div().w_full().flex().flex_col().gap(px(4.0));
            for ms in [800u64, 2000u64, 5000u64] {
                let is_selected = self.autosave_debounce_ms == ms;
                list = list.child(
                    Self::dropdown_item(
                        gpui::SharedString::from(format!("preferences-autosave-{ms}")),
                        format!("{ms} ms"),
                        is_selected,
                        theme,
                        move |this, _, _, cx| {
                            this.autosave_debounce_ms = ms;
                            this.autosave_dropdown_open = false;
                            cx.notify();
                        },
                        cx,
                    ),
                );
            }
            debounce_dropdown = debounce_dropdown.child(self.dropdown_menu("preferences-autosave-dropdown", list, theme));
        }

        let external_change_selected = match self.external_change_policy {
            ExternalChangePolicy::Auto => strings.preferences_external_change_auto.clone(),
            ExternalChangePolicy::Manual => strings.preferences_external_change_manual.clone(),
        };
        let mut external_change_dropdown = div()
            .relative()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(Self::dropdown_button(
                "preferences-external-change-dropdown",
                external_change_selected,
                theme,
                Self::toggle_external_change_dropdown,
                cx,
            ));
        if self.external_change_dropdown_open {
            let mut list = div().w_full().flex().flex_col().gap(px(4.0));
            for (policy, label) in [
                (
                    ExternalChangePolicy::Auto,
                    strings.preferences_external_change_auto.clone(),
                ),
                (
                    ExternalChangePolicy::Manual,
                    strings.preferences_external_change_manual.clone(),
                ),
            ] {
                let is_selected = self.external_change_policy == policy;
                list = list.child(Self::dropdown_item(
                    gpui::SharedString::from(format!(
                        "preferences-external-change-{}",
                        policy.as_str()
                    )),
                    label,
                    is_selected,
                    theme,
                    move |this, _, _, cx| {
                        this.external_change_policy = policy;
                        this.external_change_dropdown_open = false;
                        cx.notify();
                    },
                    cx,
                ));
            }
            external_change_dropdown = external_change_dropdown.child(self.dropdown_menu("preferences-external-change-dropdown", list, theme));
        }

        let delete_policy_selected = match self.delete_policy {
            DeletePolicy::Trash => strings.preferences_delete_policy_trash.clone(),
            DeletePolicy::Permanent => strings.preferences_delete_policy_permanent.clone(),
        };
        let mut delete_policy_dropdown = div()
            .relative()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(Self::dropdown_button(
                "preferences-delete-policy-dropdown",
                delete_policy_selected,
                theme,
                Self::toggle_delete_policy_dropdown,
                cx,
            ));
        if self.delete_policy_dropdown_open {
            let mut list = div().w_full().flex().flex_col().gap(px(4.0));
            for (policy, label) in [
                (
                    DeletePolicy::Trash,
                    strings.preferences_delete_policy_trash.clone(),
                ),
                (
                    DeletePolicy::Permanent,
                    strings.preferences_delete_policy_permanent.clone(),
                ),
            ] {
                let is_selected = self.delete_policy == policy;
                list = list.child(Self::dropdown_item(
                    gpui::SharedString::from(format!(
                        "preferences-delete-policy-{}",
                        policy.as_str()
                    )),
                    label,
                    is_selected,
                    theme,
                    move |this, _, _, cx| {
                        this.delete_policy = policy;
                        this.delete_policy_dropdown_open = false;
                        cx.notify();
                    },
                    cx,
                ));
            }
            delete_policy_dropdown = delete_policy_dropdown.child(self.dropdown_menu("preferences-delete-policy-dropdown", list, theme));
        }

        let smart_punctuation_toggle =
            crate::components::switch::Switch::new("preferences-smart-punctuation")
                .checked(self.smart_punctuation)
                .on_click(cx.listener(|this, _event, _window, cx| {
                    this.smart_punctuation = !this.smart_punctuation;
                    cx.notify();
                }));

        let autosave_toggle = crate::components::switch::Switch::new("preferences-autosave")
            .checked(self.autosave)
            .on_click(cx.listener(|this, _event, _window, cx| {
                this.autosave = !this.autosave;
                cx.notify();
            }));

        let update_startup = crate::components::switch::Switch::new("preferences-update-startup")
            .checked(self.check_updates_on_startup)
            .on_click(cx.listener(|this, _, _, cx| { this.check_updates_on_startup = !this.check_updates_on_startup; cx.notify(); }));
        let update_beta = crate::components::switch::Switch::new("preferences-update-beta")
            .checked(self.include_prereleases)
            .on_click(cx.listener(|this, _, _, cx| { this.include_prereleases = !this.include_prereleases; cx.notify(); }));
        self.settings_card(
            theme,
            vec![
                self.settings_row(theme, strings.preferences_startup_option.clone(), dropdown),
                self.settings_row(
                    theme,
                    strings.preferences_sidebar_open.clone(),
                    sidebar_open_dropdown,
                ),
                self.settings_row(
                    theme,
                    strings.preferences_sidebar_panel.clone(),
                    sidebar_panel_dropdown,
                ),
                self.settings_row(theme, strings.preferences_updates_startup.clone(), update_startup),
                self.settings_row(theme, strings.preferences_updates_beta.clone(), update_beta),
                self.settings_row(
                    theme,
                    strings.preferences_file_tree_sort.clone(),
                    tree_sort_dropdown,
                ),
                self.settings_row(theme, strings.preferences_autosave.clone(), autosave_toggle),
                self.settings_row(
                    theme,
                    strings.preferences_file_autosave_debounce.clone(),
                    debounce_dropdown,
                ),
                self.settings_row(
                    theme,
                    strings.preferences_smart_punctuation.clone(),
                    smart_punctuation_toggle,
                ),
                self.settings_row(
                    theme,
                    strings.preferences_file_external_change.clone(),
                    external_change_dropdown,
                ),
                self.settings_row(
                    theme,
                    strings.preferences_file_delete_policy.clone(),
                    delete_policy_dropdown,
                ),
            ],
        )
    }

    pub(crate) fn render_theme_page(
        &self,
        theme: &Theme,
        strings: &crate::i18n::I18nStrings,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut dropdown = div()
            .relative()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(Self::dropdown_button(
                "preferences-theme-dropdown",
                self.selected_theme_name(strings),
                theme,
                Self::toggle_theme_dropdown,
                cx,
            ));
        if self.theme_dropdown_open {
            let mut list = div().flex().flex_col().gap(px(4.0));

            for (index, entry) in self.theme_options.clone().into_iter().enumerate() {
                let selected = entry.id == self.selected_theme_id;
                let name = self.theme_display_name(&entry, strings);
                let preview = cx
                    .global::<ThemeManager>()
                    .preview_colors(&entry.id)
                    .unwrap_or((
                        theme.colors.editor_background,
                        theme.colors.text_default,
                        theme.colors.text_link,
                    ));
                list = list.child(Self::theme_dropdown_item(
                    index,
                    name,
                    selected,
                    preview,
                    theme,
                    move |this, _, _, cx| {
                        this.selected_theme_id = entry.id.clone();
                        this.theme_dropdown_open = false;
                        cx.notify();
                    },
                    cx,
                ));
            }
            dropdown = dropdown.child(self.dropdown_menu("preferences-theme-dropdown", list, theme));
        }
        let chinese = cx.global::<I18nManager>().current_language_id() == "zh-CN";
        let width_label = |width| match (chinese, width) {
            (true, WritingWidthPreference::Theme) => "跟随主题",
            (true, WritingWidthPreference::Compact) => "紧凑 · 50%",
            (true, WritingWidthPreference::Standard) => "标准 · 62%",
            (true, WritingWidthPreference::Wide) => "宽敞 · 75%",
            (false, WritingWidthPreference::Theme) => "Follow Theme",
            (false, WritingWidthPreference::Compact) => "Compact · 50%",
            (false, WritingWidthPreference::Standard) => "Standard · 62%",
            (false, WritingWidthPreference::Wide) => "Wide · 75%",
        };
        let mut writing_width = div()
            .relative()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(Self::dropdown_button(
                "preferences-writing-width",
                width_label(self.writing_width).into(),
                theme,
                Self::toggle_writing_width_dropdown,
                cx,
            ));
        if self.writing_width_dropdown_open {
            let mut list = div().w_full().flex().flex_col().gap(px(4.0));
            for (index, width) in [
                WritingWidthPreference::Theme,
                WritingWidthPreference::Compact,
                WritingWidthPreference::Standard,
                WritingWidthPreference::Wide,
            ]
            .into_iter()
            .enumerate()
            {
                list = list.child(Self::dropdown_item(
                    ("preferences-writing-width-option", index),
                    width_label(width).into(),
                    self.writing_width == width,
                    theme,
                    move |this, _, _, cx| {
                        this.writing_width = width;
                        this.writing_width_dropdown_open = false;
                        cx.notify();
                    },
                    cx,
                ));
            }
            writing_width = writing_width.child(self.dropdown_menu("preferences-writing-width", list, theme));
        }
        let ui_font = self.font_family_row(FontRole::Ui, theme, strings, cx);
        let body_font = self.font_family_row(FontRole::Body, theme, strings, cx);
        let code_font = self.font_family_row(FontRole::Code, theme, strings, cx);
        // 两张卡片：主题一张，字体与排版一张。页面滚动交给外层容器，
        // 这里不再自己开一个 max_h + overflow 的滚动区（嵌套滚动会互抢滚轮）。
        let theme_rows = vec![self.settings_row(
            theme,
            strings.preferences_local_theme.clone(),
            dropdown,
        )];
        let typography_rows = vec![
            self.settings_row(
                theme,
                if chinese {
                    "写作列宽".to_string()
                } else {
                    "Writing Width".to_string()
                },
                writing_width,
            ),
            self.settings_row(theme, strings.preferences_ui_font.clone(), ui_font),
            self.font_size_row(
                &strings.preferences_ui_font_size, self.fonts.ui_size, FontRole::Ui, theme, cx,
            ),
            self.settings_row(theme, strings.preferences_body_font.clone(), body_font),
            self.font_size_row(
                &strings.preferences_body_font_size, self.fonts.markdown_size, FontRole::Body, theme, cx,
            ),
            self.settings_row(theme, strings.preferences_code_font.clone(), code_font),
            self.font_size_row(
                &strings.preferences_code_font_size, self.fonts.code_size, FontRole::Code, theme, cx,
            ),
        ];
        div()
            .w_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap(px(14.0))
            .child(self.settings_card(theme, theme_rows))
            .child(self.settings_card(theme, typography_rows))
            .into_any_element()
    }

    fn font_family_row(
        &self,
        role: FontRole,
        theme: &Theme,
        strings: &crate::i18n::I18nStrings,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (id, option_id, selected, open, toggle): (
            _,
            _,
            _,
            _,
            fn(&mut Self, &ClickEvent, &mut Window, &mut Context<Self>),
        ) = match role {
            FontRole::Ui => (
                "preferences-ui-font",
                "preferences-ui-font-option",
                &self.fonts.ui_family,
                self.ui_font_dropdown_open,
                Self::toggle_ui_font_dropdown,
            ),
            FontRole::Body => (
                "preferences-markdown-font",
                "preferences-markdown-font-option",
                &self.fonts.markdown_family,
                self.markdown_font_dropdown_open,
                Self::toggle_markdown_font_dropdown,
            ),
            FontRole::Code => (
                "preferences-code-font",
                "preferences-code-font-option",
                &self.fonts.code_family,
                self.code_font_dropdown_open,
                Self::toggle_code_font_dropdown,
            ),
        };
        let family_label = |family: &str| match family {
            "theme" => strings.preferences_theme_font.clone(),
            ".SystemUIFont" => strings.preferences_system_font.clone(),
            _ => family.to_string(),
        };
        let mut dropdown = div()
            .relative()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(Self::dropdown_button(
                id,
                family_label(selected),
                theme,
                toggle,
                cx,
            ));
        if open {
            let mut families = self.system_font_families.clone();
            if role == FontRole::Body {
                families.insert(0, "theme".into());
            }
            let mut list = div().flex().flex_col();
            for (index, family) in families.into_iter().enumerate() {
                list = list.child(Self::dropdown_item(
                    (option_id, index),
                    family_label(&family),
                    selected == &family,
                    theme,
                    move |this, _, _, cx| {
                        match role {
                            FontRole::Ui => {
                                this.fonts.ui_family = family.clone();
                                this.ui_font_dropdown_open = false;
                            }
                            FontRole::Body => {
                                this.fonts.markdown_family = family.clone();
                                this.markdown_font_dropdown_open = false;
                            }
                            FontRole::Code => {
                                this.fonts.code_family = family.clone();
                                this.code_font_dropdown_open = false;
                            }
                        }
                        cx.notify();
                    },
                    cx,
                ));
            }
            dropdown = dropdown.child(self.dropdown_menu(id, list, theme));
        }
        dropdown.into_any_element()
    }

    fn font_size_row(
        &self,
        label: &str,
        size: u16,
        role: FontRole,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = &theme.colors;
        let button = |id: &'static str, sign: &'static str, delta: i16, cx: &mut Context<Self>| {
            div()
                .id(id)
                .debug_selector(move || id.to_string())
                .w(px(36.0))
                .min_h(px((theme.typography.dialog_body_size * 1.5 + 8.0).max(32.0)))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(6.0))
                .border_1()
                .border_color(c.dialog_border)
                .bg(c.dialog_secondary_button_bg)
                .hover(|this| this.bg(c.dialog_secondary_button_hover))
                .cursor_pointer()
                .child(sign)
                .on_click(cx.listener(move |this, _, _, cx| {
                    let current = match role {
                        FontRole::Ui => &mut this.fonts.ui_size,
                        FontRole::Body => &mut this.fonts.markdown_size,
                        FontRole::Code => &mut this.fonts.code_size,
                    };
                    *current = ((*current as i16 + delta).clamp(10, 36)) as u16;
                    cx.notify();
                }))
        };
        self.settings_row(
            theme,
            label.to_string(),
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(button(
                    match role {
                        FontRole::Ui => "ui-font-smaller",
                        FontRole::Body => "markdown-font-smaller",
                        FontRole::Code => "code-font-smaller",
                    },
                    "−",
                    -1,
                    cx,
                ))
                .child(div().w(px(52.0)).text_center().child(format!("{size} px")))
                .child(button(
                    match role {
                        FontRole::Ui => "ui-font-larger",
                        FontRole::Body => "markdown-font-larger",
                        FontRole::Code => "code-font-larger",
                    },
                    "+",
                    1,
                    cx,
                )),
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
        &self,
        theme: &Theme,
        strings: &crate::i18n::I18nStrings,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let options = [
            ImagePasteBehavior::None,
            ImagePasteBehavior::CopyToDocumentFolder,
            ImagePasteBehavior::CopyToAssetsFolder,
            ImagePasteBehavior::CopyToNamedAssetsFolder,
        ];
        let mut dropdown = div()
            .relative()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(Self::dropdown_button(
                "preferences-image-dropdown",
                Self::image_paste_behavior_label(self.image_paste_behavior, strings),
                theme,
                Self::toggle_image_dropdown,
                cx,
            ));
        if self.image_dropdown_open {
            let mut list = div().w_full().flex().flex_col().gap(px(4.0));
            for (index, behavior) in options.into_iter().enumerate() {
                let selected = behavior == self.image_paste_behavior;
                let label = Self::image_paste_behavior_label(behavior, strings);
                list = list.child(Self::dropdown_item(
                    ("preferences-image-option", index),
                    label,
                    selected,
                    theme,
                    move |this, _, _, cx| {
                        this.image_paste_behavior = behavior;
                        this.image_dropdown_open = false;
                        cx.notify();
                    },
                    cx,
                ));
            }
            dropdown = dropdown.child(self.dropdown_menu("preferences-image-dropdown", list, theme));
        }
        self.settings_card(
            theme,
            vec![self.settings_row(
                theme,
                strings.preferences_image_insert_behavior.clone(),
                dropdown,
            )],
        )
    }
}
