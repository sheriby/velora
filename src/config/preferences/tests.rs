    use super::{
        AiPreferences, AppPreferences, DeletePolicy, EditorSettings, ExportThemePreference,
        ExternalChangePolicy, FontPreferences, ImagePasteBehavior, PreferencesNav,
        StartupOpenPreference, StatusBarPreferences, TreeSortPreference, WindowOpenPosition,
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
        assert_eq!(scaled.typography.h1_size, theme.typography.h1_size * 1.5);
        assert_eq!(scaled.typography.h6_size, theme.typography.h6_size * 1.5);

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
        assert_eq!(preferences.writing_width, WritingWidthPreference::Theme);
        assert_eq!(
            preferences.image_paste_behavior,
            ImagePasteBehavior::CopyToAssetsFolder
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn writing_width_presets_keep_theme_default_and_explicit_sizes() {
        assert_eq!(WritingWidthPreference::Theme.max_width(700.0), 700.0);
        assert_eq!(WritingWidthPreference::Compact.max_width(700.0), 640.0);
        assert_eq!(WritingWidthPreference::Standard.max_width(700.0), 760.0);
        assert_eq!(WritingWidthPreference::Wide.max_width(700.0), 900.0);
        assert_eq!(
            WritingWidthPreference::from_str("unknown"),
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
            startup_open: StartupOpenPreference::LastOpenedFile,
            default_language_id: "zh-CN".into(),
            default_theme_id: "velora-light".into(),
            export_theme: ExportThemePreference::Dark,
            show_table_headers: false,
            smart_punctuation: true,
            external_change_policy: ExternalChangePolicy::Manual,
            delete_policy: DeletePolicy::Permanent,
            image_paste_behavior: ImagePasteBehavior::CopyToAssetsFolder,
            fonts: FontPreferences {
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
            new_file_template: String::new(),
            remember_window_bounds: true,
            window_frame: None,
            window_open_position: WindowOpenPosition::Center,
            ai: crate::config::preferences::AiPreferences::default(),
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
        assert!(text.contains("code_font_size = 13"));
        assert!(text.contains("writing_width = \"wide\""));
        assert!(text.contains("workspace_sidebar_width = 320"));
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
            startup_open: StartupOpenPreference::NewFile,
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
            new_file_template: String::new(),
            remember_window_bounds: true,
            window_frame: None,
            window_open_position: WindowOpenPosition::default(),
            ai: crate::config::preferences::AiPreferences::default(),
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
            &dirs,
        )
        .expect("window preferences should save");
        assert_eq!(saved.tree_sort, TreeSortPreference::Name);
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
    fn ai_preferences_round_trip_through_config_file() {
        let root = std::env::temp_dir().join(format!(
            "velora-ai-prefs-{}",
            uuid::Uuid::new_v4()
        ));
        let dirs = VeloraConfigDirs::from_root(&root);
        let mut preferences = AppPreferences::default();
        preferences.ai = AiPreferences {
            provider_id: "deepseek".into(),
            api_base_url: "https://api.deepseek.com/v1".into(),
            api_key: "sk-test".into(),
            model: "deepseek-chat".into(),
            translate_target: "en".into(),
        };

        save_app_preferences_with_dirs(&preferences, &dirs)
            .expect("preferences should save to config.toml");
        let loaded = read_app_preferences_with_dirs(&dirs).expect("preferences should read back");
        assert_eq!(loaded.ai, preferences.ai);
        assert!(loaded.ai.is_configured());
        assert_eq!(loaded.ai.translate_target(), "en");

        let text =
            std::fs::read_to_string(dirs.app_config_file()).expect("config.toml should exist");
        assert!(text.contains("[ai]"));
        assert!(text.contains("provider_id = \"deepseek\""));
        assert!(text.contains("model = \"deepseek-chat\""));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn missing_ai_section_reads_as_unconfigured() {
        let root = std::env::temp_dir().join(format!(
            "velora-ai-prefs-legacy-{}",
            uuid::Uuid::new_v4()
        ));
        let dirs = VeloraConfigDirs::from_root(&root);
        std::fs::create_dir_all(root.clone()).expect("create root");
        std::fs::write(
            dirs.app_config_file(),
            "preferences_version = 3\n\n[editor]\nautosave = true\n",
        )
        .expect("write legacy config");

        let loaded = read_app_preferences_with_dirs(&dirs).expect("legacy config should read");
        assert_eq!(loaded.ai, AiPreferences::default());
        assert!(!loaded.ai.is_configured(), "缺 [ai] 段 = 未配置");
        // translate_target 为空时按「跟随界面」解释。
        assert_eq!(
            loaded.ai.translate_target(),
            super::AUTO_TRANSLATE_TARGET
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[gpui::test]
    async fn editor_settings_ai_getter_setter_round_trip(cx: &mut TestAppContext) {
        init_preferences_test_app(cx);
        cx.update(|cx| {
            let configured = AiPreferences {
                provider_id: "custom".into(),
                api_base_url: "http://127.0.0.1:11434/v1".into(),
                api_key: "key".into(),
                model: "llama3.1".into(),
                translate_target: super::AUTO_TRANSLATE_TARGET.into(),
            };
            EditorSettings::set_ai(cx, configured.clone());
            assert_eq!(EditorSettings::ai(cx), configured);
            assert!(EditorSettings::ai(cx).is_configured());
        });
    }
