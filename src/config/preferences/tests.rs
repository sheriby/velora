    use super::{
        AppPreferences, DeletePolicy, EditorSettings, ExportThemePreference,
        ExternalChangePolicy, FontPreferences, ImagePasteBehavior, PreferencesNav,
        SidebarOpenPreference, SidebarPanelPreference, StartupOpenPreference,
        StatusBarPreferences, TreeSortPreference, WindowOpenPosition,
        WritingWidthPreference,
        load_or_create_app_preferences_with_dirs_and_locales, open_preferences_window_with_size,
        open_preferences_window_with_state,
        read_app_preferences_with_dirs, save_app_preferences_with_dirs,
        save_preferences_from_window_with_dirs,
    };
    use crate::config::VeloraConfigDirs;
    use crate::i18n::I18nManager;
    use crate::theme::{ThemeCatalogEntry, ThemeManager};
    use gpui::TestAppContext;
    use gpui::px;
    use std::collections::BTreeMap;

    #[gpui::test]
    async fn scaled_typography_applies_font_size_and_ui_zoom(cx: &mut TestAppContext) {
        // 用户报修：界面缩放设置了没反应。缩放与字号必须是同一个派生，
        // 正文/代码/标题一起缩放；文档块以前只套字号，漏了缩放因子。
        init_preferences_test_app(cx);
        cx.update_global::<EditorSettings, _>(|settings, _cx| {
            settings.fonts.markdown_size = 20;
            settings.fonts.code_size = 12;
            settings.zoom_percent = 150;
        });
        let theme = cx.read_global::<ThemeManager, _>(|manager, _cx| {
            manager.current_arc().as_ref().clone()
        });

        let mut scaled = theme.clone();
        cx.update(|cx| EditorSettings::apply_scaled_typography(cx, &mut scaled));
        assert_eq!(scaled.typography.text_size, 30.0, "正文 20px × 150%");
        assert_eq!(scaled.typography.code_size, 18.0, "代码 12px × 150%");
        assert_eq!(scaled.typography.h1_size, theme.typography.h1_size * 1.875);
        assert_eq!(scaled.typography.h6_size, theme.typography.h6_size * 1.875);

        let (text_size, code_size) = cx.update(|cx| EditorSettings::scaled_font_sizes(cx));
        assert_eq!((text_size, code_size), (30.0, 18.0));

        // 100% 时只套字号，标题保持主题原值。
        let mut plain = theme.clone();
        cx.update_global::<EditorSettings, _>(|settings, _cx| {
            settings.zoom_percent = 100;
            settings.fonts.markdown_size = 16;
            settings.fonts.code_size = 14;
        });
        cx.update(|cx| EditorSettings::apply_scaled_typography(cx, &mut plain));
        assert_eq!(plain.typography.text_size, 16.0);
        assert_eq!(plain.typography.code_size, 14.0);
        assert_eq!(plain.typography.h1_size, theme.typography.h1_size);
    }

    #[gpui::test]
    async fn ui_font_size_does_not_change_body_or_code_sizes(cx: &mut TestAppContext) {
        init_preferences_test_app(cx);
        let theme = cx.read_global::<ThemeManager, _>(|manager, _cx| manager.current().clone());
        cx.update_global::<EditorSettings, _>(|settings, _cx| {
            settings.fonts = FontPreferences {
                ui_size: 28,
                markdown_size: 20,
                code_size: 12,
                ..FontPreferences::default()
            };
        });
        let mut scaled = theme.clone();
        cx.update(|cx| {
            EditorSettings::apply_ui_typography(cx, &mut scaled);
            EditorSettings::apply_scaled_typography(cx, &mut scaled);
        });
        assert_eq!(
            scaled.typography.dialog_body_size,
            theme.typography.dialog_body_size * 2.0
        );
        assert_eq!(
            scaled.dimensions.status_bar_text_size,
            theme.dimensions.status_bar_text_size * 2.0
        );
        assert_eq!(scaled.typography.text_size, 20.0);
        assert_eq!(scaled.typography.code_size, 12.0);
        assert_eq!(scaled.typography.h1_size, theme.typography.h1_size * 1.25);
    }

    #[gpui::test]
    async fn system_font_dropdowns_select_each_category_and_apply_saved_settings(
        cx: &mut TestAppContext,
    ) {
        init_preferences_test_app(cx);
        let handle = cx.update(|cx| {
            open_preferences_window_with_size(
                cx,
                AppPreferences::default(),
                default_theme_options(),
                "Preferences".into(),
                gpui::size(px(880.0), px(1100.0)),
            )
        });
        let mut preferences_cx = gpui::VisualTestContext::from_window(handle.into(), cx);
        handle
            .update(cx, |preferences, window, cx| {
                assert_eq!(
                    preferences.system_font_families,
                    window.text_system().all_font_names()
                );
                preferences.system_font_families = vec!["报告专用字体".into()];
                preferences.nav = PreferencesNav::Theme;
                cx.notify();
            })
            .expect("偏好窗口应可更新");
        preferences_cx.update(|window, cx| window.draw(cx).clear());
        preferences_cx.run_until_parked();
        for (button, option, larger) in [
            (
                "preferences-ui-font",
                "preferences-ui-font-option-0",
                "ui-font-larger",
            ),
            (
                "preferences-markdown-font",
                "preferences-markdown-font-option-1",
                "markdown-font-larger",
            ),
            (
                "preferences-code-font",
                "preferences-code-font-option-0",
                "code-font-larger",
            ),
        ] {
            let bounds = preferences_cx.debug_bounds(button).expect("字体按钮应可见");
            preferences_cx.simulate_click(bounds.center(), gpui::Modifiers::none());
            preferences_cx.update(|window, cx| window.draw(cx).clear());
            preferences_cx.run_until_parked();
            let bounds = preferences_cx
                .debug_bounds(option)
                .expect("系统提供的字体应出现在列表中");
            preferences_cx.simulate_click(bounds.center(), gpui::Modifiers::none());
            preferences_cx.update(|window, cx| window.draw(cx).clear());
            preferences_cx.run_until_parked();
            let bounds = preferences_cx.debug_bounds(larger).expect("字号按钮应可见");
            preferences_cx.simulate_click(bounds.center(), gpui::Modifiers::none());
            preferences_cx.update(|window, cx| window.draw(cx).clear());
            preferences_cx.run_until_parked();
        }
        let saved_fonts = handle
            .update(cx, |preferences, window, cx| {
                assert!(preferences.has_unsaved_changes());
                let fonts = preferences.fonts.clone();
                assert_eq!(fonts.ui_family, "报告专用字体");
                assert_eq!(fonts.markdown_family, "报告专用字体");
                assert_eq!(fonts.code_family, "报告专用字体");
                assert_eq!(
                    (fonts.ui_size, fonts.markdown_size, fonts.code_size),
                    (15, 17, 15)
                );
                preferences.apply_saved_preferences(
                    AppPreferences {
                        fonts: fonts.clone(),
                        ..AppPreferences::default()
                    },
                    window,
                    cx,
                );
                assert!(!preferences.has_unsaved_changes());
                fonts
            })
            .expect("偏好窗口应可更新");
        assert_eq!(cx.update(|cx| EditorSettings::fonts(cx)), saved_fonts);
    }

    /// 下拉列表被当成设置行的普通子元素，展开会挤走后面的字号控件。
    #[gpui::test]
    async fn dropdown_menus_do_not_change_settings_page_layout(cx: &mut TestAppContext) {
        init_preferences_test_app(cx);
        let handle = cx.update(|cx| {
            open_preferences_window_with_state(
                cx,
                AppPreferences::default(),
                default_theme_options(),
                "偏好设置".into(),
            )
        });
        let mut preferences_cx = gpui::VisualTestContext::from_window(handle.into(), cx);
        handle
            .update(cx, |preferences, _window, cx| {
                preferences.nav = PreferencesNav::Theme;
                preferences.system_font_families = (0..120)
                    .map(|index| format!("BIZ UDGothic {index:03}"))
                    .collect();
                cx.notify();
            })
            .expect("偏好窗口应可更新");
        for button in [
            "preferences-theme-dropdown",
            "preferences-writing-width",
            "preferences-ui-font",
            "preferences-markdown-font",
            "preferences-code-font",
        ] {
            handle
                .update(cx, |preferences, _window, cx| {
                    preferences.theme_dropdown_open = false;
                    preferences.writing_width_dropdown_open = false;
                    preferences.ui_font_dropdown_open = false;
                    preferences.markdown_font_dropdown_open = false;
                    preferences.code_font_dropdown_open = false;
                    cx.notify();
                })
                .expect("偏好窗口应可更新");
            preferences_cx.update(|window, cx| window.draw(cx).clear());
            preferences_cx.run_until_parked();
            let before = preferences_cx
                .debug_bounds("code-font-larger")
                .expect("代码字号控件应绘制");
            handle
                .update(cx, |preferences, window, cx| match button {
                    "preferences-theme-dropdown" => {
                        preferences.toggle_theme_dropdown(&gpui::ClickEvent::default(), window, cx)
                    }
                    "preferences-writing-width" => preferences.toggle_writing_width_dropdown(
                        &gpui::ClickEvent::default(),
                        window,
                        cx,
                    ),
                    "preferences-ui-font" => preferences.toggle_ui_font_dropdown(
                        &gpui::ClickEvent::default(),
                        window,
                        cx,
                    ),
                    "preferences-markdown-font" => preferences.toggle_markdown_font_dropdown(
                        &gpui::ClickEvent::default(),
                        window,
                        cx,
                    ),
                    _ => preferences.toggle_code_font_dropdown(
                        &gpui::ClickEvent::default(),
                        window,
                        cx,
                    ),
                })
                .expect("下拉应能展开");
            preferences_cx.update(|window, cx| window.draw(cx).clear());
            preferences_cx.run_until_parked();
            assert_eq!(
                preferences_cx.debug_bounds("code-font-larger"),
                Some(before),
                "展开 {button} 不应撑高设置行或挤走后续控件"
            );
        }
    }

    /// 嵌套滚动时，字体列表未接管滚轮，外层设置页面也跟着移动。
    #[gpui::test]
    async fn dropdown_scroll_does_not_move_the_settings_page(cx: &mut TestAppContext) {
        init_preferences_test_app(cx);
        let handle = cx.update(|cx| {
            open_preferences_window_with_size(
                cx,
                AppPreferences::default(),
                default_theme_options(),
                "偏好设置".into(),
                gpui::size(px(880.0), px(560.0)),
            )
        });
        let mut preferences_cx = gpui::VisualTestContext::from_window(handle.into(), cx);
        handle
            .update(cx, |preferences, _window, cx| {
                preferences.nav = PreferencesNav::Theme;
                preferences.system_font_families = (0..120)
                    .map(|index| format!("BIZ UDGothic {index:03}"))
                    .collect();
                preferences.ui_font_dropdown_open = true;
                cx.notify();
            })
            .expect("偏好窗口应可更新");
        preferences_cx.update(|window, cx| window.draw(cx).clear());
        preferences_cx.run_until_parked();
        let first = preferences_cx
            .debug_bounds("preferences-ui-font-option-0")
            .expect("字体列表应绘制");
        let before = handle
            .update(cx, |preferences, _window, _cx| {
                preferences.page_scroll.offset()
            })
            .expect("页面应有滚动句柄");
        let position = first.center();
        preferences_cx.simulate_mouse_move(position, None, gpui::Modifiers::none());
        preferences_cx.simulate_event(gpui::ScrollWheelEvent {
            position,
            delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.0), px(-70.0))),
            modifiers: gpui::Modifiers::none(),
            touch_phase: gpui::TouchPhase::default(),
        });
        preferences_cx.update(|window, cx| window.draw(cx).clear());
        preferences_cx.run_until_parked();
        let after = handle
            .update(cx, |preferences, _window, _cx| {
                preferences.page_scroll.offset()
            })
            .expect("页面应有滚动句柄");
        assert_eq!(after, before, "字体菜单里的滚轮不应带动设置页面");
        assert!(
            preferences_cx
                .debug_bounds("preferences-ui-font-option-0")
                .expect("字体列表仍应绘制")
                .top()
                < first.top(),
            "字体列表本身应随滚轮滚动"
        );
        for delta in [-100_000.0, -70.0, 100_000.0, 70.0] {
            preferences_cx.simulate_event(gpui::ScrollWheelEvent {
                position,
                delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.0), px(delta))),
                modifiers: gpui::Modifiers::none(),
                touch_phase: gpui::TouchPhase::default(),
            });
            preferences_cx.update(|window, cx| window.draw(cx).clear());
            preferences_cx.run_until_parked();
            let offset = handle
                .update(cx, |preferences, _window, _cx| {
                    preferences.page_scroll.offset()
                })
                .expect("页面应有滚动句柄");
            assert_eq!(offset, before, "菜单滚到顶部或底部后也不应把滚轮传给页面");
        }
        let page = preferences_cx
            .debug_bounds("preferences-page-scroll")
            .expect("页面应可滚动");
        preferences_cx.simulate_mouse_move(page.center(), None, gpui::Modifiers::none());
        preferences_cx.simulate_event(gpui::ScrollWheelEvent {
            position: page.center(),
            delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.0), px(-70.0))),
            modifiers: gpui::Modifiers::none(),
            touch_phase: gpui::TouchPhase::default(),
        });
        preferences_cx.update(|window, cx| window.draw(cx).clear());
        preferences_cx.run_until_parked();
        handle
            .update(cx, |preferences, _window, _cx| {
                assert!(
                    !preferences.ui_font_dropdown_open,
                    "菜单外滚动页面时应收起菜单"
                );
                assert_ne!(
                    preferences.page_scroll.offset(),
                    before,
                    "菜单外滚轮仍应滚动设置页面"
                );
            })
            .expect("偏好窗口应可更新");
    }

    /// 页面末尾的菜单要浮出滚动区并回到窗口内，点选不能落到被覆盖的控件上。
    #[gpui::test]
    async fn dropdown_menu_at_the_bottom_remains_visible_and_selectable(cx: &mut TestAppContext) {
        init_preferences_test_app(cx);
        let handle = cx.update(|cx| {
            open_preferences_window_with_state(
                cx,
                AppPreferences::default(),
                default_theme_options(),
                "偏好设置".into(),
            )
        });
        let mut preferences_cx = gpui::VisualTestContext::from_window(handle.into(), cx);
        handle
            .update(cx, |preferences, window, cx| {
                preferences.nav = PreferencesNav::Theme;
                preferences.system_font_families = (0..120)
                    .map(|index| format!("BIZ UDGothic {index:03}"))
                    .collect();
                preferences.toggle_code_font_dropdown(&gpui::ClickEvent::default(), window, cx);
            })
            .expect("偏好窗口应可更新");
        preferences_cx.update(|window, cx| window.draw(cx).clear());
        preferences_cx.run_until_parked();
        let menu = preferences_cx
            .debug_bounds("preferences-code-font-list")
            .expect("代码字体菜单应绘制");
        let height = preferences_cx.update(|window, _cx| window.viewport_size().height);
        assert!(
            menu.top() >= px(0.0) && menu.bottom() <= height,
            "菜单应完全落在窗口内"
        );
        assert!(menu.size.height <= px(240.0), "长列表应有高度上限");
        let first = preferences_cx
            .debug_bounds("preferences-code-font-option-0")
            .expect("首个字体应绘制");
        assert!(menu.contains(&first.center()), "首个选项应在菜单中可点击");
        preferences_cx.simulate_click(first.center(), gpui::Modifiers::none());
        preferences_cx.update(|window, cx| window.draw(cx).clear());
        preferences_cx.run_until_parked();
        handle
            .update(cx, |preferences, _window, _cx| {
                assert_eq!(preferences.fonts.code_family, "BIZ UDGothic 000");
                assert_eq!(
                    preferences.fonts.ui_family, ".SystemUIFont",
                    "点击浮层不应点到其下的 UI 字体控件"
                );
                assert!(!preferences.code_font_dropdown_open);
            })
            .expect("偏好窗口应可更新");
    }

    #[test]
    fn shortcut_page_prefers_platform_keys_without_changing_bindings() {
        let keys = vec!["cmd-s".to_string(), "ctrl-s".to_string()];
        assert_eq!(
            super::PreferencesWindow::preferred_shortcut(&keys, true),
            keys.first()
        );
        assert_eq!(
            super::PreferencesWindow::preferred_shortcut(&keys, false),
            keys.get(1)
        );
        let neutral = vec!["enter".to_string()];
        assert_eq!(
            super::PreferencesWindow::preferred_shortcut(&neutral, true),
            neutral.first()
        );
        assert_eq!(
            super::PreferencesWindow::preferred_shortcut(&[], false),
            None
        );
        let fullscreen = vec!["ctrl-cmd-f".to_string(), "f11".to_string()];
        assert_eq!(
            super::PreferencesWindow::preferred_shortcut(&fullscreen, false),
            fullscreen.get(1)
        );
        let word_motion = vec!["ctrl-left".to_string(), "alt-left".to_string()];
        assert_eq!(
            super::PreferencesWindow::preferred_shortcut(&word_motion, true),
            word_motion.get(1)
        );
        assert_eq!(keys, vec!["cmd-s", "ctrl-s"]);
    }

    #[gpui::test]
    async fn shortcut_page_edit_cancel_and_reset_remain_usable(cx: &mut TestAppContext) {
        init_preferences_test_app(cx);
        let handle = cx.update(|cx| {
            open_preferences_window_with_state(
                cx,
                AppPreferences::default(),
                default_theme_options(),
                "偏好设置".into(),
            )
        });
        let mut preferences_cx = gpui::VisualTestContext::from_window(handle.into(), cx);
        handle
            .update(cx, |preferences, _window, cx| {
                preferences.nav = PreferencesNav::Shortcuts;
                cx.notify();
            })
            .expect("偏好窗口应可更新");
        preferences_cx.update(|window, cx| window.draw(cx).clear());
        preferences_cx.run_until_parked();
        let command = crate::components::ShortcutCommand::SaveDocument;
        let edit_id =
            Box::leak(format!("preferences-shortcut-record-{}", command as u32).into_boxed_str());
        let reset_id =
            Box::leak(format!("preferences-shortcut-reset-{}", command as u32).into_boxed_str());
        let edit = preferences_cx
            .debug_bounds(edit_id)
            .expect("修改按钮应可见");
        preferences_cx.simulate_click(edit.center(), gpui::Modifiers::none());
        preferences_cx.simulate_keystrokes("escape");
        handle
            .update(cx, |preferences, _window, _cx| {
                assert!(preferences.recording_shortcut.is_none());
                assert!(preferences.keybindings.is_empty());
            })
            .expect("偏好窗口应可更新");
        preferences_cx.update(|window, cx| window.draw(cx).clear());
        preferences_cx.run_until_parked();
        preferences_cx.simulate_click(edit.center(), gpui::Modifiers::none());
        preferences_cx.simulate_keystrokes("ctrl-alt-s");
        preferences_cx.update(|window, cx| window.draw(cx).clear());
        preferences_cx.run_until_parked();
        handle
            .update(cx, |preferences, _window, _cx| {
                assert_eq!(
                    preferences.keybindings.get("save_document"),
                    Some(&vec!["ctrl-alt-s".into()])
                );
                assert!(preferences.has_unsaved_changes());
            })
            .expect("快捷键应已修改");
        let reset = preferences_cx
            .debug_bounds(reset_id)
            .expect("重置按钮应可见");
        preferences_cx.simulate_click(reset.center(), gpui::Modifiers::none());
        handle
            .update(cx, |preferences, _window, _cx| {
                assert!(preferences.keybindings.is_empty());
                assert!(!preferences.has_unsaved_changes());
            })
            .expect("重置后应恢复默认绑定");
    }

    fn init_preferences_test_app(cx: &mut TestAppContext) {
        cx.update(|cx| {
            I18nManager::init_with_language_id(cx, "en-US");
            ThemeManager::init_with_theme_id(cx, "velora-dark");
            crate::components::init(cx);
            EditorSettings::init(cx, true);
        });
    }

    fn default_theme_options() -> Vec<ThemeCatalogEntry> {
        vec![
            ThemeCatalogEntry {
                id: "system".into(),
                name: "System".into(),
            },
            ThemeCatalogEntry {
                id: "velora-dark".into(),
                name: "Velora".into(),
            },
            ThemeCatalogEntry {
                id: "velora-light".into(),
                name: "Velora Light".into(),
            },
        ]
    }

    #[test]
    fn missing_preferences_file_returns_defaults() {
        let root = std::env::temp_dir().join(format!(
            "velora-preferences-missing-{}",
            uuid::Uuid::new_v4()
        ));
        let dirs = VeloraConfigDirs::from_root(&root);
        let preferences =
            read_app_preferences_with_dirs(&dirs).expect("missing preferences should load");
        assert_eq!(preferences, AppPreferences::default());
        assert_eq!(preferences.default_theme_id, "forest");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn migrates_legacy_default_theme_and_image_paste_behavior_once() {
        let root = std::env::temp_dir().join(format!(
            "velora-preferences-migration-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).expect("temp root should exist");
        let dirs = VeloraConfigDirs::from_root(&root);
        std::fs::write(
            dirs.app_config_file(),
            r#"
                [theme]
                default_theme_id = "old-unknown-theme"

                [editor]
                image_paste_behavior = "none"
            "#,
        )
        .expect("legacy preferences should be written");

        let preferences =
            read_app_preferences_with_dirs(&dirs).expect("legacy preferences should migrate");
        assert_eq!(preferences.default_theme_id, "forest");
        assert_eq!(
            preferences.image_paste_behavior,
            ImagePasteBehavior::CopyToAssetsFolder
        );
        let migrated_text =
            std::fs::read_to_string(dirs.app_config_file()).expect("config should be migrated");
        assert!(migrated_text.contains("preferences_version = 3"));
        assert!(migrated_text.contains("default_theme_id = \"forest\""));
        assert!(migrated_text.contains("image_paste_behavior = \"copy_to_assets_folder\""));

        let current_preferences = migrated_text
            .replace(
                "default_theme_id = \"forest\"",
                "default_theme_id = \"velora-dark\"",
            )
            .replace(
                "image_paste_behavior = \"copy_to_assets_folder\"",
                "image_paste_behavior = \"none\"",
            );
        std::fs::write(dirs.app_config_file(), current_preferences)
            .expect("current explicit preferences should be written");
        let preferences = read_app_preferences_with_dirs(&dirs)
            .expect("versioned preferences should load without migration");
        assert_eq!(preferences.default_theme_id, "velora-dark");
        assert_eq!(preferences.image_paste_behavior, ImagePasteBehavior::None);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn version_two_default_system_theme_migrates_to_forest() {
        let root = std::env::temp_dir().join(format!(
            "velora-preferences-theme-default-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).expect("temp root should exist");
        let dirs = VeloraConfigDirs::from_root(&root);
        std::fs::write(
            dirs.app_config_file(),
            r#"
                preferences_version = 2

                [theme]
                default_theme_id = "system"

                [editor]
                markdown_font_family = "PingFang SC"
            "#,
        )
        .expect("v2 preferences should be written");

        let preferences = read_app_preferences_with_dirs(&dirs).expect("v2 preferences should load");
        // 「system」是旧默认值，跟着新默认主题走；顺手确认其它设置没被这次迁移碰掉。
        assert_eq!(preferences.default_theme_id, "forest");
        assert_eq!(preferences.fonts.markdown_family, "PingFang SC");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn explicitly_chosen_theme_is_not_overwritten_by_default_theme() {
        let root = std::env::temp_dir().join(format!(
            "velora-preferences-theme-explicit-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).expect("temp root should exist");
        let dirs = VeloraConfigDirs::from_root(&root);
        std::fs::write(
            dirs.app_config_file(),
            r#"
                preferences_version = 2

                [theme]
                default_theme_id = "velora-light"
            "#,
        )
        .expect("v2 preferences should be written");

        let preferences = read_app_preferences_with_dirs(&dirs).expect("v2 preferences should load");
        assert_eq!(preferences.default_theme_id, "velora-light");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn version_one_preferences_follow_theme_font_without_resetting_choices() {
        let value: toml::Value = toml::from_str(
            r#"
                preferences_version = 1

                [theme]
                default_theme_id = "velora-dark"

                [editor]
                image_paste_behavior = "none"
                markdown_font_family = ".SystemUIFont"
            "#,
        )
        .unwrap();
        let (preferences, migrated) = super::load_preferences_from_toml_value(&value, "en-US");
        assert!(migrated);
        assert_eq!(preferences.default_theme_id, "velora-dark");
        assert_eq!(preferences.image_paste_behavior, ImagePasteBehavior::None);
        assert_eq!(preferences.fonts.markdown_family, "theme");
    }

    #[test]
    fn partial_or_invalid_preferences_fall_back_by_field() {
        let root = std::env::temp_dir().join(format!(
            "velora-preferences-partial-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).expect("temp root should exist");
        let dirs = VeloraConfigDirs::from_root(&root);
        std::fs::write(
            dirs.app_config_file(),
            r#"
                [startup]
                open = "not-valid"

                [theme]
                default_theme_id = "velora-light"
            "#,
        )
        .expect("preferences should be written");

        let preferences =
            read_app_preferences_with_dirs(&dirs).expect("partial preferences should load");
        assert_eq!(preferences.startup_open, StartupOpenPreference::NewFile);
        assert_eq!(preferences.default_language_id, "en-US");
        assert_eq!(preferences.default_theme_id, "velora-light");
        assert_eq!(preferences.export_theme, ExportThemePreference::Current);
        assert!(!preferences.smart_punctuation);
        assert!(preferences.autosave);
        assert_eq!(preferences.external_change_policy, ExternalChangePolicy::Auto);
        assert_eq!(preferences.delete_policy, DeletePolicy::Trash);
        assert_eq!(preferences.writing_width, WritingWidthPreference::Standard);
        assert_eq!(
            preferences.image_paste_behavior,
            ImagePasteBehavior::CopyToAssetsFolder
        );
        let _ = std::fs::remove_dir_all(root);
    }

    /// 写作宽度：窄窗与定值时代一致（640/760/900 是下限），宽窗按窗口比例长。
    /// 3840 宽的 4K 屏上 760px 不到两成，正文列得跟着窗口走。
    #[test]
    fn writing_width_follows_the_window_and_keeps_narrow_windows_as_before() {
        let theme_centered = 1108.0;
        let theme_cap = 700.0;
        let narrow = 1200.0 - 48.0;
        let four_k = 3840.0 - 48.0;

        // 窄窗：比例算出来比下限小，取下限——与三档定值一模一样。
        assert_eq!(
            WritingWidthPreference::Compact.column_width(narrow, theme_centered, theme_cap),
            640.0
        );
        assert_eq!(
            WritingWidthPreference::Standard.column_width(narrow, theme_centered, theme_cap),
            760.0
        );
        assert_eq!(
            WritingWidthPreference::Wide.column_width(narrow, theme_centered, theme_cap),
            900.0
        );

        // 4K：按比例，不再是 640/760/900 那一小截。
        for (width, ratio) in [
            (WritingWidthPreference::Compact, 0.50),
            (WritingWidthPreference::Standard, 0.62),
            (WritingWidthPreference::Wide, 0.75),
        ] {
            let column = width.column_width(four_k, theme_centered, theme_cap);
            assert!(
                (column - four_k * ratio).abs() < 0.01,
                "4K 上 {width:?} 该是可用宽度的 {ratio}，实得 {column}"
            );
        }
        let standard = WritingWidthPreference::Standard.column_width(four_k, theme_centered, theme_cap);
        assert!(
            standard > 2000.0,
            "4K 上默认档的正文列只有 {standard}px，还是一小截"
        );

        // 「跟随主题」不管窗口多宽都是主题写的那条上限。
        assert_eq!(
            WritingWidthPreference::Theme.column_width(four_k, 2199.0, theme_cap),
            700.0
        );

        // 极窄窗口：下限比可用宽度还大时按可用宽度走。
        assert_eq!(
            WritingWidthPreference::Wide.column_width(400.0, 300.0, theme_cap),
            400.0
        );

        // 默认档跟着窗口走，不是「跟随主题」。
        assert_eq!(WritingWidthPreference::default(), WritingWidthPreference::Standard);
        assert_eq!(
            WritingWidthPreference::from_str("unknown"),
            WritingWidthPreference::Standard
        );
        assert_eq!(
            WritingWidthPreference::from_str("theme"),
            WritingWidthPreference::Theme
        );
    }

    #[test]
    fn invalid_image_paste_behavior_falls_back_to_none() {
        let root = std::env::temp_dir().join(format!(
            "velora-preferences-image-invalid-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).expect("temp root should exist");
        let dirs = VeloraConfigDirs::from_root(&root);
        std::fs::write(
            dirs.app_config_file(),
            r#"
                [editor]
                image_paste_behavior = "somewhere-dangerous"
            "#,
        )
        .expect("preferences should be written");

        let preferences = read_app_preferences_with_dirs(&dirs).expect("preferences should load");
        assert_eq!(preferences.image_paste_behavior, ImagePasteBehavior::None);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn damaged_preferences_file_returns_defaults() {
        let root = std::env::temp_dir().join(format!(
            "velora-preferences-damaged-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).expect("temp root should exist");
        let dirs = VeloraConfigDirs::from_root(&root);
        std::fs::write(dirs.app_config_file(), "not = [valid")
            .expect("preferences should be written");

        let preferences =
            read_app_preferences_with_dirs(&dirs).expect("damaged preferences should load");
        assert_eq!(preferences, AppPreferences::default());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn saves_and_reads_preferences() {
        let root = std::env::temp_dir().join(format!(
            "velora-preferences-save-{}",
            uuid::Uuid::new_v4()
        ));
        let dirs = VeloraConfigDirs::from_root(&root);
        let preferences = AppPreferences {
            updates: super::UpdatePreferences::default(),            startup_open: StartupOpenPreference::LastOpenedFile,
            default_language_id: "zh-CN".into(),
            default_theme_id: "velora-light".into(),
            export_theme: ExportThemePreference::Dark,
            show_table_headers: false,
            smart_punctuation: true,
            external_change_policy: ExternalChangePolicy::Manual,
            delete_policy: DeletePolicy::Permanent,
            image_paste_behavior: ImagePasteBehavior::CopyToAssetsFolder,
            fonts: FontPreferences {
                ui_family: "Arial".into(),
                ui_size: 18,
                markdown_family: "PingFang SC".into(),
                markdown_size: 18,
                code_family: "Menlo".into(),
                code_size: 13,
            },
            writing_width: WritingWidthPreference::Wide,
            workspace_sidebar_width: 320,
            keybindings: BTreeMap::new(),
            status_bar: StatusBarPreferences::default(),
            autosave_debounce_ms: 800,
            autosave: true,
            tree_sort: TreeSortPreference::default(),
            sidebar_open: SidebarOpenPreference::Always,
            sidebar_panel: SidebarPanelPreference::Outline,
            new_file_template: String::new(),
            remember_window_bounds: true,
            window_frame: None,
            window_open_position: WindowOpenPosition::Center,
            zoom_percent: 100,
            default_window_width: 1080,
            default_window_height: 720,
        };

        save_app_preferences_with_dirs(&preferences, &dirs)
            .expect("preferences should save to config.toml");
        let loaded = read_app_preferences_with_dirs(&dirs).expect("preferences should read back");
        assert_eq!(loaded, preferences);
        assert!(loaded.smart_punctuation);
        assert_eq!(loaded.external_change_policy, ExternalChangePolicy::Manual);
        assert_eq!(loaded.delete_policy, DeletePolicy::Permanent);
        let text =
            std::fs::read_to_string(dirs.app_config_file()).expect("config.toml should exist");
        assert!(text.contains("external_change_policy = \"manual\""));
        assert!(text.contains("delete_policy = \"permanent\""));

        let text =
            std::fs::read_to_string(dirs.app_config_file()).expect("config.toml should exist");
        assert!(text.contains("remember_bounds = true"));
        assert!(text.contains("open_position = \"center\""));
        assert!(text.contains("open = \"last_opened_file\""));
        assert!(text.contains("default_language_id = \"zh-CN\""));
        assert!(text.contains("default_theme_id = \"velora-light\""));
        assert!(text.contains("show_table_headers = false"));
        assert!(text.contains("markdown_font_family = \"PingFang SC\""));
        assert!(text.contains("ui_font_family = \"Arial\""));
        assert!(text.contains("ui_font_size = 18"));
        assert!(text.contains("code_font_size = 13"));
        assert!(text.contains("writing_width = \"wide\""));
        assert!(text.contains("workspace_sidebar_width = 320"));
        assert!(text.contains("sidebar_open = \"always\""));
        assert!(text.contains("sidebar_panel = \"outline\""));
        assert!(text.contains("image_paste_behavior = \"copy_to_assets_folder\""));
        // roadmap F3：[export] theme 随其他偏好一起持久化。
        assert!(text.contains("[export]"));
        assert!(text.contains("theme = \"dark\""));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn missing_preferences_file_is_created_with_detected_language() {
        let root = std::env::temp_dir().join(format!(
            "velora-preferences-create-{}",
            uuid::Uuid::new_v4()
        ));
        let dirs = VeloraConfigDirs::from_root(&root);
        let preferences = load_or_create_app_preferences_with_dirs_and_locales(&dirs, ["zh-HK"])
            .expect("preferences should be created");
        assert_eq!(preferences.default_language_id, "zh-CN");
        assert!(dirs.app_config_file().exists());
        let text =
            std::fs::read_to_string(dirs.app_config_file()).expect("config.toml should exist");
        assert!(text.contains("remember_bounds = true"));
        assert!(text.contains("[language]"));
        assert!(text.contains("default_language_id = \"zh-CN\""));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn legacy_preferences_are_normalized_with_language() {
        let root = std::env::temp_dir().join(format!(
            "velora-preferences-legacy-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).expect("temp root should exist");
        let dirs = VeloraConfigDirs::from_root(&root);
        std::fs::write(
            dirs.app_config_file(),
            r#"
                [startup]
                open = "last_opened_file"

                [theme]
                default_theme_id = "velora-light"
            "#,
        )
        .expect("legacy preferences should be written");

        let preferences = load_or_create_app_preferences_with_dirs_and_locales(&dirs, ["en-GB"])
            .expect("legacy preferences should normalize");
        assert_eq!(
            preferences.startup_open,
            StartupOpenPreference::LastOpenedFile
        );
        assert_eq!(preferences.default_language_id, "en-US");
        assert_eq!(preferences.default_theme_id, "velora-light");
        // 老配置没有 open_position 键时按「记住上次位置」处理。
        assert_eq!(preferences.window_open_position, WindowOpenPosition::Remember);
        let text =
            std::fs::read_to_string(dirs.app_config_file()).expect("config.toml should exist");
        assert!(text.contains("remember_bounds = true"));
        assert!(text.contains("open_position = \"remember\""));
        assert!(text.contains("[language]"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn saving_preferences_window_preserves_language() {
        let root = std::env::temp_dir().join(format!(
            "velora-preferences-window-{}",
            uuid::Uuid::new_v4()
        ));
        let dirs = VeloraConfigDirs::from_root(&root);
        let preferences = AppPreferences {
            updates: super::UpdatePreferences::default(),            startup_open: StartupOpenPreference::NewFile,
            smart_punctuation: false,
            external_change_policy: ExternalChangePolicy::Auto,
            delete_policy: DeletePolicy::Trash,
            default_language_id: "zh-CN".into(),
            default_theme_id: "velora-dark".into(),
            export_theme: ExportThemePreference::Dark,
            show_table_headers: true,
            image_paste_behavior: ImagePasteBehavior::None,
            fonts: FontPreferences::default(),
            writing_width: WritingWidthPreference::Theme,
            workspace_sidebar_width: 258,
            keybindings: BTreeMap::new(),
            status_bar: StatusBarPreferences::default(),
            autosave_debounce_ms: 800,
            autosave: true,
            tree_sort: TreeSortPreference::default(),
            sidebar_open: SidebarOpenPreference::default(),
            sidebar_panel: SidebarPanelPreference::default(),
            new_file_template: String::new(),
            remember_window_bounds: true,
            window_frame: None,
            window_open_position: WindowOpenPosition::default(),
            zoom_percent: 100,
            default_window_width: 1080,
            default_window_height: 720,
        };
        save_app_preferences_with_dirs(&preferences, &dirs)
            .expect("preferences should save to config.toml");

        let saved = save_preferences_from_window_with_dirs(
            StartupOpenPreference::LastOpenedFile,
            "velora-light",
            ImagePasteBehavior::CopyToNamedAssetsFolder,
            &FontPreferences::default(),
            WritingWidthPreference::Compact,
            BTreeMap::from([("save_document".to_string(), vec!["ctrl-alt-s".to_string()])]),
            &StatusBarPreferences::default(),
            TreeSortPreference::Name,
            SidebarOpenPreference::Never,
            SidebarPanelPreference::Files,
            800,
            true,
            WindowOpenPosition::Center,
            true,
            // 两个相邻的 bool 各给一个非默认方向，位置写反就会被下面两行抓住。
            false,
            110,
            1280,
            800,
            ExternalChangePolicy::Manual,
            DeletePolicy::Permanent,
            true,
            false,
            &dirs,
        )
        .expect("window preferences should save");
        assert_eq!(saved.tree_sort, TreeSortPreference::Name);
        assert_eq!(saved.sidebar_open, SidebarOpenPreference::Never);
        assert_eq!(saved.sidebar_panel, SidebarPanelPreference::Files);
        assert_eq!(saved.autosave_debounce_ms, 800);
        assert!(!saved.autosave);
        assert!(saved.smart_punctuation);
        assert!(saved.remember_window_bounds);
        assert_eq!(saved.zoom_percent, 110);
        assert_eq!(saved.default_window_width, 1280);
        assert_eq!(saved.default_window_height, 800);
        assert_eq!(saved.external_change_policy, ExternalChangePolicy::Manual);
        assert_eq!(saved.delete_policy, DeletePolicy::Permanent);
        assert_eq!(saved.default_language_id, "zh-CN");
        assert_eq!(saved.startup_open, StartupOpenPreference::LastOpenedFile);
        assert_eq!(saved.default_theme_id, "velora-light");
        assert_eq!(saved.writing_width, WritingWidthPreference::Compact);
        assert_eq!(
            saved.image_paste_behavior,
            ImagePasteBehavior::CopyToNamedAssetsFolder
        );
        assert_eq!(
            saved.keybindings.get("save_document"),
            Some(&vec!["ctrl-alt-s".to_string()])
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[gpui::test]
    async fn preferences_pages_render_inside_a_scroll_container(cx: &mut TestAppContext) {
        // 用户报修（Windows）：偏好设置「文件」页内容超出窗口高度时无法滚动。
        // 页面内容必须挂在 overflow_y_scroll 容器里。
        init_preferences_test_app(cx);
        let handle = cx.update(|cx| {
            open_preferences_window_with_state(
                cx,
                AppPreferences::default(),
                default_theme_options(),
                "Preferences".into(),
            )
        });
        cx.run_until_parked();

        let mut preferences_cx = gpui::VisualTestContext::from_window(handle.into(), cx);

        handle
            .update(cx, |preferences, _window, cx| {
                preferences.nav = PreferencesNav::File;
                cx.notify();
            })
            .expect("preferences window should update");
        preferences_cx.run_until_parked();
        assert!(
            preferences_cx.debug_bounds("preferences-page-scroll").is_some(),
            "偏好设置「文件」页应挂在可滚动容器里"
        );

        handle
            .update(cx, |preferences, _window, cx| {
                preferences.nav = PreferencesNav::Shortcuts;
                cx.notify();
            })
            .expect("preferences window should update");
        preferences_cx.run_until_parked();
        assert!(
            preferences_cx.debug_bounds("preferences-page-scroll").is_some(),
            "快捷键页也应挂在滚动容器里"
        );
    }

    #[gpui::test]
    async fn preferences_pages_really_scroll_when_content_overflows(cx: &mut TestAppContext) {
        // 用户报修（两次）：偏好设置「文件」「窗口」两页内容超出窗口高度时滚不动。
        // 旧的测试只验了「挂了一个 overflow 容器」——容器在但高度被上一层的 flex_1
        // 压成视口高度，于是根本滚不动。这里用滚动句柄验 max_offset。
        init_preferences_test_app(cx);
        // 窗口开矮一点：内容必然超出视口（测试平台不支持 resize，只能一开始就开小）。
        let handle = cx.update(|cx| {
            open_preferences_window_with_size(
                cx,
                AppPreferences::default(),
                default_theme_options(),
                "Preferences".into(),
                gpui::size(px(880.0), px(320.0)),
            )
        });
        cx.run_until_parked();

        let mut preferences_cx = gpui::VisualTestContext::from_window(handle.into(), cx);
        preferences_cx.run_until_parked();

        for nav in [PreferencesNav::File, PreferencesNav::Window] {
            handle
                .update(&mut preferences_cx, |preferences, _window, cx| {
                    preferences.nav = nav;
                    cx.notify();
                })
                .expect("preferences window should update");
            preferences_cx.run_until_parked();

            let max_offset = handle
                .update(&mut preferences_cx, |preferences, _window, _cx| {
                    preferences.page_scroll.max_offset()
                })
                .expect("preferences window should update");
            assert!(
                max_offset.height > px(0.0),
                "「{nav:?}」页内容超出窗口时必须能滚，实测 max_offset {max_offset:?}"
            );

            // 侧边栏在最左边（旧版把标签堆在 30% 宽的栏里且右对齐）。
            let nav_bounds = preferences_cx
                .debug_bounds("preferences-nav-file")
                .expect("侧边栏第一项应渲染");
            assert!(
                nav_bounds.origin.x < px(200.0),
                "侧边栏应贴在窗口左侧，实测 x = {:?}",
                nav_bounds.origin.x
            );
        }
    }

    #[gpui::test]
    async fn window_page_exposes_zoom_and_default_size_controls(cx: &mut TestAppContext) {
        // roadmap H1 批次二：偏好设置「窗口」分组页（缩放 + 默认窗口尺寸）；
        // 用户报修补齐：打开位置下拉与「记住窗口位置与大小」开关也放在这一页。
        init_preferences_test_app(cx);
        let handle = cx.update(|cx| {
            open_preferences_window_with_state(
                cx,
                AppPreferences::default(),
                default_theme_options(),
                "Preferences".into(),
            )
        });
        cx.run_until_parked();

        handle
            .update(cx, |preferences, _window, cx| {
                assert_eq!(preferences.zoom_percent, 100);
                assert_eq!(preferences.default_window_width, 1080);
                assert_eq!(preferences.default_window_height, 720);
                assert_eq!(
                    preferences.window_open_position,
                    WindowOpenPosition::Remember
                );

                // 切到窗口页并展开三个下拉（渲染路径由窗口自身的绘制触发）。
                preferences.nav = PreferencesNav::Window;
                preferences.zoom_dropdown_open = true;
                preferences.window_size_dropdown_open = true;
                preferences.window_open_position_dropdown_open = true;
                cx.notify();

                // 改动进入未保存状态并可通过保存路径持久化。
                preferences.zoom_percent = 125;
                preferences.default_window_width = 1280;
                preferences.default_window_height = 800;
                preferences.window_open_position = WindowOpenPosition::Center;
                assert!(preferences.has_unsaved_changes());
            })
            .expect("preferences window should update");
    }

    #[gpui::test]
    async fn clicking_a_zoom_dropdown_item_updates_the_selection(cx: &mut TestAppContext) {
        // 用户报修：界面缩放设置了没反应。这条守住入口本身——点「125%」必须
        // 真的写进窗口状态并进入待保存（渲染侧的修正在下面那条测试里）。
        init_preferences_test_app(cx);
        let handle = cx.update(|cx| {
            open_preferences_window_with_state(
                cx,
                AppPreferences::default(),
                default_theme_options(),
                "Preferences".into(),
            )
        });
        let mut preferences_cx = gpui::VisualTestContext::from_window(handle.into(), cx);
        preferences_cx.run_until_parked();

        handle
            .update(&mut preferences_cx, |preferences, _window, cx| {
                preferences.nav = PreferencesNav::Window;
                preferences.zoom_dropdown_open = true;
                cx.notify();
            })
            .expect("preferences window should update");
        preferences_cx.run_until_parked();

        let item = preferences_cx
            .debug_bounds("preferences-zoom-125")
            .expect("「125%」下拉项应渲染");
        preferences_cx.simulate_click(item.center(), gpui::Modifiers::none());
        preferences_cx.run_until_parked();

        handle
            .update(&mut preferences_cx, |preferences, _window, _cx| {
                assert_eq!(preferences.zoom_percent, 125, "点选后应写入 125%");
                assert!(!preferences.zoom_dropdown_open, "点选后下拉应收起");
                assert!(preferences.has_unsaved_changes(), "应进入待保存状态");
            })
            .expect("preferences window should update");
    }

    #[gpui::test]
    async fn preferences_window_activates_and_focuses_on_open(cx: &mut TestAppContext) {
        init_preferences_test_app(cx);

        let handle = cx.update(|cx| {
            open_preferences_window_with_state(
                cx,
                AppPreferences::default(),
                default_theme_options(),
                "Preferences".into(),
            )
        });
        cx.run_until_parked();

        let active_window = cx.update(|cx| cx.active_window().expect("window should be active"));
        assert_eq!(active_window.window_id(), handle.window_id());
        assert!(
            handle
                .update(cx, |preferences, window, _cx| preferences
                    .focus_handle
                    .is_focused(window))
                .expect("preferences window should be updateable")
        );
        assert!(
            !handle
                .update(cx, |preferences, _window, _cx| preferences
                    .has_unsaved_changes())
                .expect("preferences window should be updateable")
        );
    }

    #[gpui::test]
    async fn preferences_dirty_state_tracks_draft_changes(cx: &mut TestAppContext) {
        init_preferences_test_app(cx);

        let handle = cx.update(|cx| {
            open_preferences_window_with_state(
                cx,
                AppPreferences::default(),
                default_theme_options(),
                "Preferences".into(),
            )
        });
        cx.run_until_parked();

        handle
            .update(cx, |preferences, _window, _cx| {
                assert!(!preferences.has_unsaved_changes());
                preferences.startup_open = StartupOpenPreference::LastOpenedFile;
                assert!(preferences.has_unsaved_changes());
                preferences.startup_open = StartupOpenPreference::NewFile;
                assert!(!preferences.has_unsaved_changes());

                preferences.image_paste_behavior = ImagePasteBehavior::CopyToDocumentFolder;
                assert!(preferences.has_unsaved_changes());
                preferences.image_paste_behavior = ImagePasteBehavior::CopyToAssetsFolder;
                assert!(!preferences.has_unsaved_changes());

                preferences
                    .keybindings
                    .insert("save_document".into(), vec!["ctrl-alt-s".into()]);
                assert!(preferences.has_unsaved_changes());
            })
            .expect("preferences window should be updateable");
    }

    #[gpui::test]
    async fn applying_saved_preferences_keeps_window_open_and_focused(cx: &mut TestAppContext) {
        init_preferences_test_app(cx);

        let handle = cx.update(|cx| {
            open_preferences_window_with_state(
                cx,
                AppPreferences::default(),
                default_theme_options(),
                "Preferences".into(),
            )
        });
        cx.run_until_parked();

        handle
            .update(cx, |preferences, window, cx| {
                preferences.startup_open = StartupOpenPreference::LastOpenedFile;
                assert!(preferences.has_unsaved_changes());
                let saved = AppPreferences {
                    startup_open: StartupOpenPreference::LastOpenedFile,
                    ..AppPreferences::default()
                };
                preferences.apply_saved_preferences(saved, window, cx);
            })
            .expect("preferences window should be updateable");
        cx.run_until_parked();

        assert_eq!(cx.update(|cx| cx.windows().len()), 1);
        let active_window = cx.update(|cx| cx.active_window().expect("window should be active"));
        assert_eq!(active_window.window_id(), handle.window_id());
        assert!(
            handle
                .update(cx, |preferences, window, _cx| preferences
                    .focus_handle
                    .is_focused(window))
                .expect("preferences window should remain updateable")
        );
        assert!(
            !handle
                .update(cx, |preferences, _window, _cx| preferences
                    .has_unsaved_changes())
                .expect("preferences window should remain updateable")
        );
    }

#[test]
fn update_preferences_default_to_startup_checks_and_preserve_beta_and_skip_settings() {
    let defaults = toml::Value::try_from(super::PreferencesFile::from(&AppPreferences::default())).expect("默认配置");
    assert_eq!(defaults.get("updates").and_then(|updates| updates.get("check_on_startup")).and_then(toml::Value::as_bool), Some(true), "默认应启动检查更新");
    let value: toml::Value = toml::from_str("[updates]\ncheck_on_startup = false\ninclude_prereleases = true\nignored_version = '0.2.5'").expect("更新设置");
    let (preferences, _) = super::load_preferences_from_toml_value(&value, "en-US");
    let saved = toml::Value::try_from(super::PreferencesFile::from(&preferences)).expect("持久化");
    assert_eq!(saved.get("updates").and_then(|updates| updates.get("include_prereleases")).and_then(toml::Value::as_bool), Some(true));
    assert_eq!(saved.get("updates").and_then(|updates| updates.get("ignored_version")).and_then(toml::Value::as_str), Some("0.2.5"));
}
