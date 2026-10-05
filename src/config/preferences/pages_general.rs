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
            let new_file_label = strings.preferences_startup_new_file.clone();
            let last_file_label = strings.preferences_startup_last_opened_file.clone();
            dropdown = dropdown
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
        }
        let tree_sort_selected = match self.tree_sort {
            TreeSortPreference::Name => strings.tree_sort_name.clone(),
            TreeSortPreference::ModifiedTime => strings.tree_sort_mtime.clone(),
            TreeSortPreference::Type => strings.tree_sort_type.clone(),
        };
        let mut tree_sort_dropdown = div()
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
            let options = [
                (TreeSortPreference::Name, strings.tree_sort_name.clone()),
                (TreeSortPreference::ModifiedTime, strings.tree_sort_mtime.clone()),
                (TreeSortPreference::Type, strings.tree_sort_type.clone()),
            ];
            for (index, (sort, label)) in options.into_iter().enumerate() {
                let is_selected = self.tree_sort == sort;
                let sort_value = sort;
                tree_sort_dropdown = tree_sort_dropdown.child(
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
        }

        let debounce_selected = format!("{} ms", self.autosave_debounce_ms);
        let mut debounce_dropdown = div()
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
            for ms in [800u64, 2000u64, 5000u64] {
                let is_selected = self.autosave_debounce_ms == ms;
                debounce_dropdown = debounce_dropdown.child(
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
        }

        let external_change_selected = match self.external_change_policy {
            ExternalChangePolicy::Auto => strings.preferences_external_change_auto.clone(),
            ExternalChangePolicy::Manual => strings.preferences_external_change_manual.clone(),
        };
        let mut external_change_dropdown = div()
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
                external_change_dropdown = external_change_dropdown.child(Self::dropdown_item(
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
        }

        let delete_policy_selected = match self.delete_policy {
            DeletePolicy::Trash => strings.preferences_delete_policy_trash.clone(),
            DeletePolicy::Permanent => strings.preferences_delete_policy_permanent.clone(),
        };
        let mut delete_policy_dropdown = div()
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
                delete_policy_dropdown = delete_policy_dropdown.child(Self::dropdown_item(
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

        self.settings_card(
            theme,
            vec![
                self.settings_row(theme, strings.preferences_startup_option.clone(), dropdown),
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
            let mut list = div()
                .id("preferences-theme-dropdown-list")
                .flex()
                .flex_col()
                .gap(px(4.0))
                .max_h(px(240.0))
                .overflow_y_scroll();

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
            dropdown = dropdown.child(list);
        }
        let chinese = cx.global::<I18nManager>().current_language_id() == "zh-CN";
        let width_label = |width| match (chinese, width) {
            (true, WritingWidthPreference::Theme) => "跟随主题",
            (true, WritingWidthPreference::Compact) => "紧凑 · 640 px",
            (true, WritingWidthPreference::Standard) => "标准 · 760 px",
            (true, WritingWidthPreference::Wide) => "宽敞 · 900 px",
            (false, WritingWidthPreference::Theme) => "Follow Theme",
            (false, WritingWidthPreference::Compact) => "Compact · 640 px",
            (false, WritingWidthPreference::Standard) => "Standard · 760 px",
            (false, WritingWidthPreference::Wide) => "Wide · 900 px",
        };
        let mut writing_width = div()
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
            for (index, width) in [
                WritingWidthPreference::Theme,
                WritingWidthPreference::Compact,
                WritingWidthPreference::Standard,
                WritingWidthPreference::Wide,
            ]
            .into_iter()
            .enumerate()
            {
                writing_width = writing_width.child(Self::dropdown_item(
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
        }
        let markdown_font_options = [
            "theme",
            ".SystemUIFont",
            "PingFang SC",
            "Noto Sans CJK SC",
            "Georgia",
        ];
        let code_font_options = if cfg!(target_os = "windows") {
            ["Consolas", "Courier New", "Cascadia Code"]
        } else {
            ["Menlo", "Monaco", "SF Mono"]
        };
        let markdown_font_label = match self.fonts.markdown_family.as_str() {
            "theme" => "跟随主题".to_string(),
            ".SystemUIFont" => "系统字体".to_string(),
            _ => self.fonts.markdown_family.clone(),
        };
        let mut markdown_font = div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(Self::dropdown_button(
                "preferences-markdown-font",
                markdown_font_label,
                theme,
                Self::toggle_markdown_font_dropdown,
                cx,
            ));
        if self.markdown_font_dropdown_open {
            for (index, family) in markdown_font_options.into_iter().enumerate() {
                markdown_font = markdown_font.child(Self::dropdown_item(
                    ("preferences-markdown-font-option", index),
                    match family {
                        "theme" => "跟随主题".into(),
                        ".SystemUIFont" => "系统字体".into(),
                        _ => family.into(),
                    },
                    self.fonts.markdown_family == family,
                    theme,
                    move |this, _, _, cx| {
                        this.fonts.markdown_family = family.into();
                        this.markdown_font_dropdown_open = false;
                        cx.notify();
                    },
                    cx,
                ));
            }
        }
        let mut code_font = div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(Self::dropdown_button(
                "preferences-code-font",
                self.fonts.code_family.clone(),
                theme,
                Self::toggle_code_font_dropdown,
                cx,
            ));
        if self.code_font_dropdown_open {
            for (index, family) in code_font_options.into_iter().enumerate() {
                code_font = code_font.child(Self::dropdown_item(
                    ("preferences-code-font-option", index),
                    family.into(),
                    self.fonts.code_family == family,
                    theme,
                    move |this, _, _, cx| {
                        this.fonts.code_family = family.into();
                        this.code_font_dropdown_open = false;
                        cx.notify();
                    },
                    cx,
                ));
            }
        }
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
            self.settings_row(theme, "Markdown 字体".to_string(), markdown_font),
            self.font_size_row("Markdown 字号", self.fonts.markdown_size, true, theme, cx),
            self.settings_row(theme, "代码等宽字体".to_string(), code_font),
            self.font_size_row("代码字号", self.fonts.code_size, false, theme, cx),
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

    pub(crate) fn font_size_row(
        &self,
        label: &str,
        size: u16,
        markdown: bool,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = &theme.colors;
        let button = |id: &'static str, sign: &'static str, delta: i16, cx: &mut Context<Self>| {
            div()
                .id(id)
                .w(px(36.0))
                .h(px(32.0))
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
                    let current = if markdown {
                        &mut this.fonts.markdown_size
                    } else {
                        &mut this.fonts.code_size
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
                    if markdown {
                        "markdown-font-smaller"
                    } else {
                        "code-font-smaller"
                    },
                    "−",
                    -1,
                    cx,
                ))
                .child(div().w(px(52.0)).text_center().child(format!("{size} px")))
                .child(button(
                    if markdown {
                        "markdown-font-larger"
                    } else {
                        "code-font-larger"
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
            for (index, behavior) in options.into_iter().enumerate() {
                let selected = behavior == self.image_paste_behavior;
                let label = Self::image_paste_behavior_label(behavior, strings);
                dropdown = dropdown.child(Self::dropdown_item(
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
