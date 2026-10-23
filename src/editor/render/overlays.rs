use super::*;
use gpui::prelude::FluentBuilder as _;
use gpui_kit::base::Link;
use gpui_kit::component::{
    ActiveTheme as _, Sizable as _, Icon,
    button::{Button, ButtonVariants as _},
    scroll::ScrollableElement as _,
    dialog::DialogDescription,
    spinner::Spinner,
};

impl Editor {
    pub(crate) fn render_welcome_page(
        &self,
        theme: &Theme,
        strings: &I18nStrings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let scroll = window
            .use_keyed_state("welcome-scroll", cx, |_, _| ScrollHandle::new())
            .read(cx)
            .clone();
        let recent = welcome_recent_entries();
        let mut column = div()
            .w_full()
            .max_w(rems(30.0))
            .my_auto()
            .flex_none()
            .flex()
            .flex_col()
            .items_center()
            .gap_6()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_3()
                    .child(
                        img("icon/velora.png")
                            .size(px(88.0))
                            .object_fit(ObjectFit::Contain),
                    )
                    .child(
                        div()
                            .text_2xl()
                            .font_weight(FontWeight::BOLD)
                            .child("Velora"),
                    ),
            )
            .child(
                crate::ui::scroll_focus_region("welcome-actions".into(), &scroll, window, cx)
                    .flex()
                    .flex_wrap()
                    .justify_center()
                    .gap_2()
                    .child(
                        Button::new("welcome-new-document")
                            .debug_selector(|| "welcome-new-document".into())
                            .primary()
                            .label(strings.welcome_new_document.clone())
                            .icon(Icon::default().path("icon/workspace/new-file.svg"))
                            .on_click(cx.listener(Self::on_welcome_new_document)),
                    )
                    .child(
                        Button::new("welcome-open-folder")
                            .debug_selector(|| "welcome-open-folder".into())
                            .outline()
                            .label(strings.open_folder_prompt.clone())
                            .icon(Icon::default().path("icon/workspace/open-folder.svg"))
                            .on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(crate::components::OpenFolder), cx)
                            }),
                    ),
            );
        if !recent.is_empty() {
            let home_directory = directories::UserDirs::new().map(|dirs| dirs.home_dir().to_path_buf());
            let mut list = div().w_full().min_w_0().flex().flex_col().gap_1().child(
                div()
                    .px_3()
                    .mb_1()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(strings.welcome_recent.clone()),
            );
            for (index, path) in recent.iter().enumerate() {
                let label = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.to_string_lossy().into_owned());
                let parent = path.parent().unwrap_or(path);
                let parent_label = match home_directory
                    .as_ref()
                    .and_then(|home| parent.strip_prefix(home).ok())
                {
                    Some(relative) => std::path::Path::new("~")
                        .join(relative)
                        .to_string_lossy()
                        .into_owned(),
                    None => parent.to_string_lossy().into_owned(),
                };
                let entry_path = path.clone();
                let remove_path = path.clone();
                let identity = path.to_string_lossy();
                list = list.child(
                    crate::ui::scroll_focus_region(
                        SharedString::from(format!("welcome-recent-entry:{identity}")).into(),
                        &scroll,
                        window,
                        cx,
                    )
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(
                        Button::new(SharedString::from(format!(
                            "welcome-recent-open:{identity}"
                        )))
                        .ghost()
                        .flex_1()
                        .min_w_0()
                        .h_auto()
                        .py_2()
                        .justify_start()
                        .accessibility_label(format!("{label} · {parent_label}"))
                        .tooltip(path.to_string_lossy().into_owned())
                        .child(Self::render_workspace_folder_icon(theme, false, cx))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .text_left()
                                .child(div().truncate().child(label.clone()))
                                .child(
                                    div()
                                        .debug_selector(move || format!("welcome-recent-path-{index}"))
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .truncate()
                                        .child(parent_label),
                                ),
                        )
                        .on_click(cx.listener(move |editor, _, window, cx| {
                            editor.open_recent_entry(&entry_path, window, cx);
                        })),
                    )
                    .child(
                        Button::new(SharedString::from(format!(
                            "welcome-recent-remove:{identity}"
                        )))
                        .debug_selector(move || format!("welcome-recent-remove-{index}"))
                        .small()
                        .ghost()
                        .icon(gpui_kit::assets::IconName::Close)
                        .accessibility_label(format!("{} · {label}", strings.welcome_remove_recent))
                        .tooltip(strings.welcome_remove_recent.clone())
                        .on_click(cx.listener(move |editor, _, _, cx| {
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
                        })),
                    ),
                );
            }
            column = column.child(list);
        }
        let mut shortcuts = div()
            .flex()
            .flex_wrap()
            .justify_center()
            .gap_4()
            .text_xs()
            .text_color(cx.theme().muted_foreground);
        for (action, label) in [
            (
                Box::new(crate::components::NewDocument) as Box<dyn Action>,
                strings.welcome_new_document.clone(),
            ),
            (
                Box::new(crate::components::OpenFile) as Box<dyn Action>,
                strings.open_markdown_files_prompt.clone(),
            ),
        ] {
            if let Some(binding) =
                gpui_kit::component::kbd::Kbd::binding_for_action(action.as_ref(), None, window)
            {
                shortcuts = shortcuts.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(binding)
                        .child(label),
                );
            }
        }
        column = column.child(shortcuts);
        div()
            .id("welcome-page")
            .debug_selector(|| "welcome-page".into())
            .size_full()
            .relative()
            .overflow_y_scroll()
            .track_scroll(&scroll)
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                div()
                    .w_full()
                    .min_h_full()
                    .p_6()
                    .flex()
                    .flex_col()
                    .items_center()
                    .child(column),
            )
            .vertical_scrollbar(&scroll)
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
