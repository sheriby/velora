use super::*;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::Disableable as _;
use gpui_kit::assets::IconName;
impl Render for PreferencesWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.system_font_families.is_empty() {
            self.system_font_families = window.text_system().all_font_names();
        }
        if self.system_appearance_subscription.is_none() {
            self.system_appearance_subscription = Some(cx.observe_window_appearance(
                window,
                |_preferences, window, cx| {
                    let appearance = window.appearance();
                    cx.update_global::<ThemeManager, _>(|manager, _cx| {
                        manager.set_system_appearance(appearance)
                    });
                    cx.refresh_windows();
                },
            ));
        }
        let mut theme = cx.global::<ThemeManager>().current().clone();
        EditorSettings::apply_ui_typography(cx, &mut theme);
        let strings = cx.global::<I18nManager>().strings().clone();
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        self.controls.sync_locale(cx.global::<I18nManager>().current_language_id());
        self.apply_ai_preferences_focus(window, cx);
        self.refresh_ai_preferences_dirty(cx);
        let can_save = self.has_unsaved_changes() && !self.controls.has_invalid_number();
        let window_title =
            SharedString::from(format!("Velora - {}", strings.preferences_window_title));
        window.set_window_title(window_title.as_ref());
        let titlebar_height = custom_titlebar_height(window, d);

        let content = {
            let page_title = match self.nav {
                PreferencesNav::File => strings.preferences_nav_file.clone(),
                PreferencesNav::Theme => strings.preferences_nav_theme.clone(),
                PreferencesNav::Image => strings.preferences_nav_image.clone(),
                PreferencesNav::Ai => "AI".into(),
                PreferencesNav::Shortcuts => strings.preferences_nav_shortcuts.clone(),
                PreferencesNav::StatusBar => strings.preferences_nav_status_bar.clone(),
                PreferencesNav::Window => strings.preferences_nav_window.clone(),
            };

            let nav_items: [(&'static str, String, IconName, bool, fn(&mut Self, &ClickEvent, &mut Window, &mut Context<Self>)); 7] = [
                (
                    "preferences-nav-file",
                    strings.preferences_nav_file.clone(),
                    IconName::FileText,
                    self.nav == PreferencesNav::File,
                    Self::set_nav_file,
                ),
                (
                    "preferences-nav-theme",
                    strings.preferences_nav_theme.clone(),
                    IconName::Palette,
                    self.nav == PreferencesNav::Theme,
                    Self::set_nav_theme,
                ),
                (
                    "preferences-nav-image",
                    strings.preferences_nav_image.clone(),
                    IconName::Image,
                    self.nav == PreferencesNav::Image,
                    Self::set_nav_image,
                ),
                (
                    "preferences-nav-ai",
                    "AI".into(),
                    IconName::Sparkles,
                    self.nav == PreferencesNav::Ai,
                    Self::set_nav_ai,
                ),
                (
                    "preferences-nav-shortcuts",
                    strings.preferences_nav_shortcuts.clone(),
                    IconName::Keyboard,
                    self.nav == PreferencesNav::Shortcuts,
                    Self::set_nav_shortcuts,
                ),
                (
                    "preferences-nav-status-bar",
                    strings.preferences_nav_status_bar.clone(),
                    IconName::PanelBottom,
                    self.nav == PreferencesNav::StatusBar,
                    Self::set_nav_status_bar,
                ),
                (
                    "preferences-nav-window",
                    strings.preferences_nav_window.clone(),
                    IconName::AppWindow,
                    self.nav == PreferencesNav::Window,
                    Self::set_nav_window,
                ),
            ];
            let mut sidebar = div()
                .w(px(196.0))
                .h_full()
                .flex_shrink_0()
                .px(px(12.0))
                .py(px(16.0))
                .bg(c.dialog_secondary_button_bg)
                .flex()
                .flex_col()
                .gap(px(2.0))
                .border_r(px(d.dialog_border_width))
                .border_color(c.dialog_border);
            for (id, label, icon, selected, handler) in nav_items {
                sidebar = sidebar.child(self.nav_button(id, label, icon, selected, &theme, handler, cx));
            }

            // 页面内容：错误条 + 卡片；内容列 flex_shrink_0、高度按内容算，
            // 这样外层 overflow_y_scroll 才有东西可滚（旧版套了一层 flex_1，
            // 高度被压成视口高度，于是「文件」和「窗口」两页永远滚不动）。
            let mut page_column = div().w_full().flex_shrink_0().flex().flex_col().gap(px(14.0));
            if let Some(error) = self.save_error.clone() {
                page_column = page_column.child(self.error_banner(&theme, error));
            }
            if let Some(error) = self.shortcut_error.clone() {
                page_column = page_column.child(self.error_banner(&theme, error));
            }
            page_column = page_column.child(match self.nav {
                PreferencesNav::File => self.render_startup_page(&theme, &strings, window, cx),
                PreferencesNav::Theme => self.render_theme_page(&theme, &strings, window, cx),
                PreferencesNav::Image => self.render_image_page(&theme, &strings, window, cx),
                PreferencesNav::Ai => self.render_ai_page(&theme, &strings, window, cx),
                PreferencesNav::Shortcuts => self.render_shortcuts_page(&theme, &strings, cx),
                PreferencesNav::StatusBar => self.render_status_bar_page(&theme, &strings, cx),
                PreferencesNav::Window => self.render_window_page(&theme, &strings, window, cx),
            });

            let footer = div()
                .w_full()
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_end()
                .gap_2()
                .child(
                    Button::new("preferences-cancel")
                        .debug_selector(|| "preferences-cancel".into())
                        .outline()
                        .label(strings.preferences_cancel.clone())
                        .on_click(cx.listener(Self::cancel)),
                )
                .child(
                    Button::new("preferences-save")
                        .debug_selector(|| "preferences-save".into())
                        .primary()
                        .label(strings.preferences_save.clone())
                        .disabled(!can_save)
                        .on_click(cx.listener(Self::save)),
                );

            div()
                .size_full()
                .pt(px(titlebar_height))
                .flex()
                .key_context("Preferences")
                .track_focus(&self.focus_handle)
                .on_key_down(cx.listener(Self::capture_shortcut_key))
                .bg(c.editor_background)
                .text_color(c.dialog_body)
                .child(sidebar)
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .h_full()
                        .flex()
                        .justify_center()
                        .child(
                            div()
                                .w_full()
                                .max_w(px(660.0))
                                .h_full()
                                .px(px(24.0))
                                .py(px(18.0))
                                .flex()
                                .flex_col()
                                .gap(px(14.0))
                                .child(self.page_header(page_title, &theme))
                                .child(
                                    div()
                                        .id("preferences-page-scroll")
                                        .debug_selector(|| "preferences-page-scroll".to_string())
                                        .track_scroll(&self.page_scroll)
                                        .w_full()
                                        .flex_1()
                                        .min_h(px(0.0))
                                        .overflow_y_scroll()
                                        .on_scroll_wheel(cx.listener(|_this, _, window, cx| {
                                            Self::dismiss_preferences_popup(window, cx);
                                        }))
                                        .flex()
                                        .flex_col()
                                        .child(page_column),
                                )
                                .child(footer),
                        ),
                )
        };

        let root = div()
            .size_full()
            .font(font(EditorSettings::fonts(cx).ui_family))
            .text_size(px(t.dialog_body_size))
            .relative()
            .bg(c.editor_background)
            .child(content);

        if let Some(titlebar) = render_custom_titlebar(
            "preferences-titlebar",
            window_title,
            None,
            None,
            &theme,
            window,
            cx,
            Self::on_titlebar_close,
        ) {
            root.child(titlebar)
        } else {
            root
        }
    }
}

pub(crate) fn open_preferences_window_with_state(
    cx: &mut App,
    preferences: AppPreferences,
    theme_options: Vec<ThemeCatalogEntry>,
    title: String,
) -> crate::ui::ContentWindowHandle<PreferencesWindow> {
    open_preferences_window_with_size(
        cx,
        preferences,
        theme_options,
        title,
        size(px(880.0), px(620.0)),
    )
}

/// 同 [`open_preferences_window_with_state`]，但窗口尺寸由调用方定（单测用它开一个
/// 矮窗口，验证内容超出视口时真的能滚——测试平台不支持 resize）。
pub(crate) fn open_preferences_window_with_size(
    cx: &mut App,
    preferences: AppPreferences,
    theme_options: Vec<ThemeCatalogEntry>,
    title: String,
    window_size: Size<Pixels>,
) -> crate::ui::ContentWindowHandle<PreferencesWindow> {
    let bounds = Bounds::centered(None, window_size, cx);
    let window_title = SharedString::from(format!("Velora - {title}"));
    let handle = crate::ui::open_window(
            velora_window_options(window_title, bounds),
            cx,
            move |_window, cx| {
                cx.new(move |cx| PreferencesWindow::new(preferences, theme_options, cx))
            },
        )
        .expect("preferences window should open");

    handle
        .update(cx, |preferences, window, cx| {
            window.activate_window();
            preferences.focus_handle.focus(window, cx);
        })
        .expect("newly opened preferences window should be updateable");

    handle
}

pub(crate) fn open_preferences_window(cx: &mut App) -> crate::ui::ContentWindowHandle<PreferencesWindow> {
    let preferences = match read_app_preferences() {
        Ok(preferences) => preferences,
        Err(err) => {
            eprintln!("failed to read app preferences: {err}");
            AppPreferences::default()
        }
    };
    let theme_options = cx.global::<ThemeManager>().available_themes().to_vec();
    let title = cx
        .global::<I18nManager>()
        .strings()
        .preferences_window_title
        .clone();
    open_preferences_window_with_state(cx, preferences, theme_options, title)
}
