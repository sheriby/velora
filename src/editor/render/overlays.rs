use super::*;
use gpui::prelude::FluentBuilder as _;
use gpui_kit::base::Link;
use gpui_kit::component::{
    ActiveTheme as _, Sizable as _,
    dialog::DialogDescription,
    spinner::Spinner,
};

impl Editor {
    /// Full-area welcome page for windows opened without a document: brand
    /// mark, quick actions, and recent entries.
    pub(crate) fn render_welcome_page(
        &self,
        theme: &Theme,
        strings: &I18nStrings,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;

        let recent = welcome_recent_entries();
        let primary_button = div()
            .id("welcome-new-document")
            .debug_selector(|| "welcome-new-document".into())
            .h(px(d.dialog_button_height))
            .px(px(d.dialog_button_padding_x + 8.0))
            .flex()
            .items_center()
            .justify_center()
            .gap(px(8.0))
            .rounded(px((d.dialog_radius - 4.0).max(0.0)))
            .bg(c.dialog_primary_button_bg)
            .hover(|this| this.bg(c.dialog_primary_button_hover))
            .active(|this| this.opacity(0.92))
            .cursor_pointer()
            .text_size(px(t.dialog_button_size))
            .font_weight(t.dialog_button_weight.to_font_weight())
            .text_color(c.dialog_primary_button_text)
            .child(
                svg()
                    .path("icon/workspace/new-file.svg")
                    .size(px(18.0))
                    .text_color(c.dialog_primary_button_text),
            )
            .child(strings.welcome_new_document.clone())
            .on_click(cx.listener(Self::on_welcome_new_document));
        let open_button = div()
            .id("welcome-open-file")
            .debug_selector(|| "welcome-open-file".into())
            .h(px(d.dialog_button_height))
            .px(px(d.dialog_button_padding_x + 8.0))
            .flex()
            .items_center()
            .justify_center()
            .gap(px(8.0))
            .rounded(px((d.dialog_radius - 4.0).max(0.0)))
            .border(px(d.dialog_border_width))
            .border_color(c.dialog_border)
            .bg(c.dialog_secondary_button_bg)
            .hover(|this| this.bg(c.dialog_secondary_button_hover))
            .active(|this| this.opacity(0.92))
            .cursor_pointer()
            .text_size(px(t.dialog_button_size))
            .font_weight(t.dialog_button_weight.to_font_weight())
            .text_color(c.dialog_secondary_button_text)
            .child(
                svg()
                    .path("icon/workspace/activity-files.svg")
                    .size(px(18.0))
                    .text_color(c.dialog_secondary_button_text),
            )
            .child(strings.open_markdown_files_prompt.clone())
            .on_click(|_, window, cx| {
                window.dispatch_action(Box::new(crate::components::OpenFile), cx)
            });

        let mut column = div()
            .w(px(360.0))
            .max_w(relative(1.0))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(14.0))
            .child(
                img("icon/velora.png")
                    .size(px(88.0))
                    .object_fit(ObjectFit::Contain),
            )
            .child(
                div()
                    .text_size(px(theme.typography.ui_text_size(24.0)))
                    .font_weight(FontWeight::BOLD)
                    .text_color(c.text_default)
                    .child("Velora"),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .mt(px(6.0))
                    .child(primary_button)
                    .child(open_button),
            );

        if !recent.is_empty() {
            let home_directory = directories::UserDirs::new().map(|dirs| dirs.home_dir().to_path_buf());
            let mut list = div()
                .w_full()
                .flex()
                .flex_col()
                .gap(px(2.0))
                .mt(px(10.0))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .text_size(px(theme.typography.ui_text_size(11.0)))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(c.dialog_muted)
                        .child(
                            svg()
                                .path("icon/workspace/history.svg")
                                .size(px(14.0))
                                .text_color(c.dialog_muted),
                        )
                        .child(strings.welcome_recent.clone()),
                );
            for (index, path) in recent.iter().enumerate() {
                let label = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.to_string_lossy().into_owned());
                let entry_path = path.clone();
                let remove_path = path.clone();
                let tooltip_path = path.to_string_lossy().into_owned();
                let parent_path = path.parent().unwrap_or(path);
                let parent_label = match home_directory
                    .as_ref()
                    .and_then(|home| parent_path.strip_prefix(home).ok())
                {
                    Some(relative_path) => std::path::Path::new("~")
                        .join(relative_path)
                        .to_string_lossy()
                        .into_owned(),
                    None => parent_path.to_string_lossy().into_owned(),
                };
                let remove_label = strings.welcome_remove_recent.clone();
                list = list.child(
                    div()
                        .id(("welcome-recent-entry", index))
                        .w_full()
                        .px(px(8.0))
                        .h(px(32.0))
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .rounded(px(5.0))
                        .cursor_pointer()
                        .hover(|this| this.bg(c.dialog_secondary_button_hover))
                        .text_size(px(theme.typography.ui_text_size(12.0)))
                        .text_color(c.dialog_body)
                        .child(
                            svg()
                                .path("icon/workspace/open-folder.svg")
                                .size(px(14.0))
                                .text_color(c.dialog_muted),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.0))
                                .flex()
                                .items_center()
                                .gap(px(8.0))
                                .child(
                                    div()
                                        .max_w(relative(0.55))
                                        .flex_shrink_0()
                                        .truncate()
                                        .child(label),
                                )
                                .child(
                                    div()
                                        .debug_selector(move || {
                                            format!("welcome-recent-path-{index}")
                                        })
                                        .flex_1()
                                        .min_w(px(0.0))
                                        .truncate()
                                        .text_size(px(theme.typography.ui_text_size(10.5)))
                                        .text_color(c.dialog_muted)
                                        .child(parent_label),
                                )
                                .id(("welcome-recent-details", index))
                                .tooltip(move |_, cx| {
                                    cx.new(|_| crate::editor::workspace::WorkspaceTooltip {
                                        label: tooltip_path.clone(),
                                    })
                                    .into()
                                }),
                        )
                        .child(
                            div()
                                .id(("welcome-recent-remove", index))
                                .debug_selector(move || format!("welcome-recent-remove-{index}"))
                                .size(px(24.0))
                                .flex_shrink_0()
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(4.0))
                                .hover(|this| this.bg(c.dialog_secondary_button_hover))
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .on_click(cx.listener(move |editor, _, _, cx| {
                                    cx.stop_propagation();
                                    match crate::config::remove_recent_folder(&remove_path) {
                                        Ok(_) => {
                                            crate::app_menu::install_menus(cx);
                                            cx.refresh_windows();
                                        }
                                        Err(error) => {
                                            let title = cx
                                                .global::<I18nManager>()
                                                .strings()
                                                .welcome_remove_recent
                                                .clone();
                                            editor.show_message_modal(title, error.to_string(), cx);
                                        }
                                    }
                                }))
                                .tooltip(move |_, cx| {
                                    cx.new(|_| crate::editor::workspace::WorkspaceTooltip {
                                        label: remove_label.clone(),
                                    })
                                    .into()
                                })
                                .child(
                                    svg()
                                        .path("icon/workspace/tab-close.svg")
                                        .size(px(12.0))
                                        .text_color(c.dialog_muted),
                                ),
                        )
                        .on_click(cx.listener(move |editor, _event, window, cx| {
                            editor.open_recent_entry(&entry_path, window, cx);
                        })),
                );
            }
            column = column.child(list);
        }

        div()
            .id("welcome-page")
            .w_full()
            .h_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .bg(c.editor_background)
            .child(column)
            .child(
                div()
                    .mt(px(18.0))
                    .text_size(px(theme.typography.ui_text_size(11.0)))
                    .text_color(c.dialog_muted)
                    .child(strings.welcome_shortcut_hint.clone()),
            )
            .into_any_element()
    }

    pub(crate) fn on_welcome_new_document(
        &mut self,
        _: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.new_workspace_document(window, cx);
    }

    pub(crate) fn info_dialog_title<'a>(&self, strings: &'a I18nStrings, kind: InfoDialogKind) -> &'a str {
        match kind {
            InfoDialogKind::CheckForUpdates => &strings.help_check_updates_title,
            InfoDialogKind::About => &strings.help_about_title,
        }
    }

    pub(crate) fn about_dialog_body_lines(strings: &I18nStrings) -> Vec<String> {
        vec![
            format!("Velora {}", env!("CARGO_PKG_VERSION")),
            strings.help_about_message.clone(),
            format!("{}: {}", strings.help_about_github_label, ABOUT_GITHUB_URL),
            strings.help_about_star_message.clone(),
        ]
    }

    pub(crate) fn info_dialog_body(&self, strings: &I18nStrings, kind: InfoDialogKind) -> String {
        match kind {
            InfoDialogKind::CheckForUpdates => strings.help_check_updates_message.clone(),
            InfoDialogKind::About => Self::about_dialog_body_lines(strings).join("\n"),
        }
    }

    pub(crate) fn render_info_dialog_body(
        &self,
        strings: &I18nStrings,
        kind: InfoDialogKind,
        cx: &App,
    ) -> AnyElement {
        let content = div().w_full().min_w_0().flex().flex_col().gap_3();
        match kind {
            InfoDialogKind::CheckForUpdates => content
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .when(self.update_check_in_progress, |row| {
                            row.child(
                                div()
                                    .debug_selector(|| "info-update-spinner".into())
                                    .child(Spinner::new().small()),
                            )
                        })
                        .child(
                            DialogDescription::new().child(self.info_dialog_body(strings, kind)),
                        ),
                )
                .into_any_element(),
            InfoDialogKind::About => content
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::BOLD)
                        .child(format!("Velora {}", env!("CARGO_PKG_VERSION"))),
                )
                .child(DialogDescription::new().child(strings.help_about_message.clone()))
                // 组件层 Link 未接键盘语义，Base Link 保留焦点和导航行为。
                .child(
                    Link::new("about-github-link")
                        .href(ABOUT_GITHUB_URL)
                        .accessibility_label(strings.help_about_github_label.clone())
                        .open_with(|_, _, _, cx| open_about_github_url(cx))
                        .text_color(cx.theme().link)
                        .underline()
                        .debug_selector(|| "about-github-link".into())
                        .on_action(|_: &gpui_kit::base::actions::Confirm, _, cx| {
                            open_about_github_url(cx)
                        })
                        .child(strings.help_about_github_label.clone()),
                )
                .child(DialogDescription::new().child(strings.help_about_star_message.clone()))
                .into_any_element(),
        }
    }
}
