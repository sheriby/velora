#[cfg(test)]
mod tests {
    use super::super::{I18nLanguagePack, I18nManager, I18nStrings, language_id_for_locale_preferences};
    use crate::config::VeloraConfigDirs;
    use crate::theme::ThemeManager;

    #[test]
    fn built_in_chinese_strings_are_utf8() {
        let strings = I18nStrings::zh_cn();
        assert_eq!(strings.menu_file, "文件");
        assert_eq!(strings.menu_export, "导出");
        assert_eq!(strings.menu_language, "语言");
        assert_eq!(strings.save_failed_title, "保存失败");
        assert_eq!(strings.export_failed_title, "导出失败");
        assert_eq!(strings.view_mode_switch_to_source, "切换到源码");
        assert!(strings.source_mode_fallback_message.contains("保留原文"));
        assert_eq!(strings.context_menu_insert, "插入");
        assert_eq!(strings.table_insert_title, "插入表格");
        assert_eq!(strings.image_loading_without_alt, "正在加载图片...");
        assert_eq!(
            strings.help_check_updates_message,
            "正在检查 Velora 的最新版本..."
        );
        assert_eq!(strings.update_open_release, "前往下载");
        assert_eq!(strings.help_about_github_label, "项目仓库");
        assert_eq!(
            strings.help_about_star_message,
            "第三方来源与许可信息见项目文档。"
        );
    }

    #[test]
    fn manager_switches_builtin_languages() {
        let mut manager = I18nManager::default();
        assert_eq!(manager.current_language_id(), "en-US");
        assert_eq!(manager.strings().menu_file, "File");
        assert_eq!(manager.strings().menu_export, "Export");

        assert!(manager.set_language_by_id("zh-CN"));
        assert_eq!(manager.current_language_id(), "zh-CN");
        assert_eq!(manager.strings().menu_file, "文件");
        assert_eq!(manager.strings().menu_export, "导出");
        assert!(!manager.set_language_by_id("zh-CN"));
        assert!(!manager.set_language_by_id("missing"));
    }

    #[test]
    fn language_catalog_contains_chinese_and_english() {
        let manager = I18nManager::default();
        let ids = manager
            .available_languages()
            .iter()
            .map(|entry| (entry.id.as_str(), entry.name.as_str()))
            .collect::<Vec<_>>();
        assert_eq!(ids, vec![("zh-CN", "简体中文"), ("en-US", "English")]);
    }

    #[test]
    fn manager_can_be_constructed_with_known_language() {
        let manager = I18nManager::new_with_language_id("zh-CN");
        assert_eq!(manager.current_language_id(), "zh-CN");
        assert_eq!(manager.strings().menu_file, "文件");

        let fallback = I18nManager::new_with_language_id("missing");
        assert_eq!(fallback.current_language_id(), "en-US");
        assert_eq!(fallback.strings().menu_file, "File");
    }

    #[test]
    fn theme_switch_does_not_modify_selected_language() {
        let mut theme_manager = ThemeManager::default();
        let mut i18n_manager = I18nManager::new_with_language_id("zh-CN");

        assert!(theme_manager.set_theme_by_id("velora-dark"));
        assert!(!i18n_manager.set_language_by_id("missing"));

        assert_eq!(theme_manager.current_theme_id(), "velora-dark");
        assert_eq!(i18n_manager.current_language_id(), "zh-CN");
        assert_eq!(i18n_manager.strings().menu_file, "文件");
    }

    #[test]
    fn locale_preferences_map_to_builtin_languages() {
        assert_eq!(language_id_for_locale_preferences(["zh-CN"]), "zh-CN");
        assert_eq!(language_id_for_locale_preferences(["zh-HK"]), "zh-CN");
        assert_eq!(language_id_for_locale_preferences(["zh-Hant-TW"]), "zh-CN");
        assert_eq!(language_id_for_locale_preferences(["zh_SG.UTF-8"]), "zh-CN");
        assert_eq!(language_id_for_locale_preferences(["en-US"]), "en-US");
        assert_eq!(language_id_for_locale_preferences(["en_GB.UTF-8"]), "en-US");
        assert_eq!(
            language_id_for_locale_preferences(["fr-FR", "zh-CN"]),
            "zh-CN"
        );
        assert_eq!(
            language_id_for_locale_preferences(Vec::<&str>::new()),
            "en-US"
        );
        assert_eq!(language_id_for_locale_preferences(["fr-FR"]), "en-US");
        assert_eq!(language_id_for_locale_preferences(["!!!"]), "en-US");
    }

    #[test]
    fn imports_jsonc_language_pack_and_persists_normalized_json() {
        let root = std::env::temp_dir().join(format!("velora-i18n-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("temp root should be created");
        let source = root.join("language.jsonc");
        std::fs::write(
            &source,
            r#"{
                // Required metadata.
                "id": "ja-JP",
                "name": "日本語",
                "author": "",
                "strings": {
                    "menu_file": "ファイル",
                    "menu_export": ""
                }
            }"#,
        )
        .expect("language config should be written");

        let dirs = VeloraConfigDirs::from_root(&root);
        let mut manager = I18nManager::default();
        let imported_id = manager
            .import_language_config_with_dirs(&source, &dirs)
            .expect("language config should import");

        assert_eq!(imported_id, "ja-JP");
        assert_eq!(manager.current_language_id(), "ja-JP");
        assert_eq!(manager.strings().menu_file, "ファイル");
        assert_eq!(manager.strings().menu_export, "Export");
        assert!(
            manager
                .available_languages()
                .iter()
                .any(|entry| entry.id == "ja-JP" && entry.name == "日本語")
        );

        let normalized = std::fs::read_to_string(dirs.languages_dir().join("ja-JP.json"))
            .expect("normalized language config should exist");
        assert!(normalized.contains("\"menu_file\": \"ファイル\""));
        assert!(!normalized.contains("menu_export"));
        assert!(!normalized.contains("author"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn loads_language_pack_dropped_into_user_directory() {
        // roadmap H4：用户目录中直接放置的语言包在启动/初始化时被加载并生效。
        let root = std::env::temp_dir().join(format!("velora-i18n-{}", uuid::Uuid::new_v4()));
        let dirs = VeloraConfigDirs::from_root(&root);
        std::fs::create_dir_all(dirs.languages_dir()).expect("languages dir should exist");
        std::fs::write(
            dirs.languages_dir().join("ko-KR.json"),
            r#"{
                "id": "ko-KR",
                "name": "한국어",
                "strings": { "menu_file": "파일" }
            }"#,
        )
        .expect("language pack should be written");

        let mut manager = I18nManager::default();
        manager
            .load_custom_languages_from_dirs(&dirs)
            .expect("user language dir should load");
        assert!(
            manager
                .available_languages()
                .iter()
                .any(|entry| entry.id == "ko-KR" && entry.name == "한국어")
        );
        assert!(manager.set_language_by_id("ko-KR"));
        assert_eq!(manager.strings().menu_file, "파일");
        // 未覆盖的字符串回退到英文默认值。
        assert_eq!(manager.strings().menu_export, "Export");

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn custom_language_cannot_override_builtin_language_id() {
        let root = std::env::temp_dir().join(format!("velora-i18n-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("temp root should be created");
        let source = root.join("language.json");
        std::fs::write(
            &source,
            r#"{
                "id": "en-US",
                "name": "Override",
                "strings": { "menu_file": "Override" }
            }"#,
        )
        .expect("language config should be written");

        let dirs = VeloraConfigDirs::from_root(&root);
        let mut manager = I18nManager::default();
        let err = manager
            .import_language_config_with_dirs(&source, &dirs)
            .expect_err("built-in language ids should be rejected");
        assert!(err.to_string().contains("built-in language"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn language_pack_json_falls_back_for_missing_strings() {
        let pack = I18nLanguagePack::from_json(
            r#"{
                "id": "zh-CN",
                "name": "简体中文",
                "strings": {
                    "menu_file": "文件菜单",
                    "unsaved_changes_hint": "legacy hint",
                    "drop_replace_hint": "legacy hint",
                    "unknown_field": "ignored"
                }
            }"#,
        )
        .expect("language pack should load");

        assert_eq!(pack.id, "zh-CN");
        assert_eq!(pack.name, "简体中文");
        assert_eq!(pack.strings.menu_file, "文件菜单");
        assert_eq!(pack.strings.menu_export, "导出");
        assert_eq!(pack.strings.info_dialog_ok, "确定");
        assert_eq!(pack.strings.update_open_release, "前往下载");
        assert_eq!(pack.strings.help_about_github_label, "项目仓库");
        assert_eq!(
            pack.strings.help_about_star_message,
            "第三方来源与许可信息见项目文档。"
        );
    }

    #[test]
    fn unknown_language_pack_falls_back_to_english_strings() {
        let pack = I18nLanguagePack::from_json(
            r#"{
                "id": "fr-FR",
                "strings": {
                    "menu_file": "Fichier"
                }
            }"#,
        )
        .expect("language pack should load");

        assert_eq!(pack.id, "fr-FR");
        assert_eq!(pack.name, "fr-FR");
        assert_eq!(pack.strings.menu_file, "Fichier");
        assert_eq!(pack.strings.menu_export, "Export");
        assert_eq!(pack.strings.info_dialog_ok, "OK");
        assert_eq!(pack.strings.update_open_release, "Open Releases");
        assert_eq!(pack.strings.menu_open_recent_file, "Open Recent");
        assert_eq!(
            pack.strings.menu_no_recent_files,
            "No Recent Files or Folders"
        );
        assert_eq!(
            pack.strings.recent_file_missing_title,
            "Recent File Missing"
        );
        assert_eq!(pack.strings.help_about_github_label, "Project repository");
        assert_eq!(
            pack.strings.help_about_star_message,
            "Third-party sources and licenses are documented in the project."
        );
    }
}
