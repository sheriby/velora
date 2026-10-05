mod tests {
    use crate::app_menu::build_menus_impl as build_menus;
    use crate::app_menu::{
        applescript_string_literal,
        recent_menu_entries,
    };
    use crate::components::{
        AddLanguageConfig, AddThemeConfig, CheckForUpdates, CloseWindow, CopyAsHtml, ExportHtml,
        ExportPdf, ExportPng, NewWindow, NoRecentFiles, OpenFile, OpenPreferences, OpenRecentFile,
        PrintDocument, QuitApplication,
        SaveDocument, SelectLanguage, SelectTheme, ShowAbout,
    };
    use crate::i18n::I18nManager;
    use crate::theme::ThemeManager;
    use gpui::MenuItem;
    use std::path::PathBuf;

    fn action_name(item: &MenuItem) -> &str {
        match item {
            MenuItem::Action { name, .. } => name.as_ref(),
            _ => panic!("expected action menu item"),
        }
    }

    fn submenu(item: &MenuItem) -> &gpui::Menu {
        match item {
            MenuItem::Submenu(menu) => menu,
            _ => panic!("expected submenu item"),
        }
    }

    #[test]
    fn applescript_string_literal_escapes_special_characters() {
        assert_eq!(
            applescript_string_literal(r#"/Applications/velora "Test".app/Contents/MacOS/velora"#),
            r#""/Applications/velora \"Test\".app/Contents/MacOS/velora""#
        );
        assert_eq!(
            applescript_string_literal(r#"/Applications/O'Brien\velora.app"#),
            r#""/Applications/O'Brien\\velora.app""#
        );
    }

    // On macOS the menu bar is: [velora app menu, File, Export, Language, Theme, View, Help]
    // On other platforms:       [File, Export, Language, Theme, View, Help]
    #[cfg(target_os = "macos")]
    const EXPORT_IDX: usize = 2;
    #[cfg(not(target_os = "macos"))]
    const EXPORT_IDX: usize = 1;

    #[cfg(target_os = "macos")]
    const LANGUAGE_IDX: usize = 3;
    #[cfg(not(target_os = "macos"))]
    const LANGUAGE_IDX: usize = 2;

    #[cfg(target_os = "macos")]
    const THEME_IDX: usize = 4;
    #[cfg(not(target_os = "macos"))]
    const THEME_IDX: usize = 3;

    #[cfg(target_os = "macos")]
    const VIEW_IDX: usize = 5;
    #[cfg(not(target_os = "macos"))]
    const VIEW_IDX: usize = 4;

    #[cfg(target_os = "macos")]
    const AI_IDX: usize = 6;
    #[cfg(not(target_os = "macos"))]
    const AI_IDX: usize = 5;

    #[cfg(target_os = "macos")]
    const HELP_IDX: usize = 7;
    #[cfg(not(target_os = "macos"))]
    const HELP_IDX: usize = 6;

    #[test]
    fn build_menus_uses_english_fallback_by_default() {
        let theme_manager = ThemeManager::default();
        let i18n_manager = I18nManager::default();
        let menus = build_menus(&theme_manager, &i18n_manager, &[]);

        let menu_names = menus
            .iter()
            .map(|menu| menu.name.to_string())
            .collect::<Vec<_>>();

        #[cfg(target_os = "macos")]
        assert_eq!(
            menu_names,
            vec![
                "Velora",
                "File",
                "Export",
                "Language",
                "Theme",
                "View",
                "AI",
                "Help"
            ]
        );
        #[cfg(not(target_os = "macos"))]
        assert_eq!(
            menu_names,
            vec![
                "File",
                "Export",
                "Language",
                "Theme",
                "View",
                "AI",
                "Help"
            ]
        );

        // New Window belongs with file operations on macOS and remains the
        // first File menu item on other platforms.
        #[cfg(target_os = "macos")]
        assert_eq!(action_name(&menus[1].items[0]), "New Window");
        #[cfg(not(target_os = "macos"))]
        assert_eq!(action_name(&menus[0].items[0]), "New Window");

        // Open Recent File submenu location differs by platform.
        #[cfg(target_os = "macos")]
        assert_eq!(
            submenu(&menus[1].items[3]).name.to_string(),
            "Open Recent"
        );
        #[cfg(not(target_os = "macos"))]
        assert_eq!(
            submenu(&menus[0].items[3]).name.to_string(),
            "Open Recent"
        );

        // Close Window is colocated with New Window in the File menu.
        #[cfg(target_os = "macos")]
        assert_eq!(action_name(&menus[1].items[1]), "Close Window");
        #[cfg(not(target_os = "macos"))]
        assert_eq!(action_name(&menus[0].items[1]), "Close Window");

        // Preferences location differs by platform.
        #[cfg(target_os = "macos")]
        assert_eq!(action_name(&menus[0].items[0]), "Preferences");
        #[cfg(not(target_os = "macos"))]
        assert_eq!(action_name(&menus[0].items[4]), "Preferences");

        assert_eq!(action_name(&menus[EXPORT_IDX].items[0]), "HTML");
        assert_eq!(action_name(&menus[EXPORT_IDX].items[1]), "PDF");
        assert_eq!(action_name(&menus[EXPORT_IDX].items[2]), "Image (PNG)");
        assert_eq!(action_name(&menus[EXPORT_IDX].items[3]), "Print…");
        assert_eq!(action_name(&menus[EXPORT_IDX].items[4]), "Copy as HTML");
        assert_eq!(action_name(&menus[LANGUAGE_IDX].items[0]), "简体中文");
        assert_eq!(
            action_name(&menus[LANGUAGE_IDX].items[1]),
            "\u{2713} English"
        );
        assert_eq!(action_name(&menus[VIEW_IDX].items[0]), "Toggle Sidebar");
        assert_eq!(action_name(&menus[VIEW_IDX].items[1]), "Toggle Full Screen");
        assert_eq!(action_name(&menus[VIEW_IDX].items[3]), "Toggle View Mode");
        assert_eq!(action_name(&menus[VIEW_IDX].items[4]), "Toggle Focus Mode");
        assert_eq!(
            action_name(&menus[VIEW_IDX].items[5]),
            "Toggle Typewriter Mode"
        );
        assert_eq!(
            action_name(&menus[VIEW_IDX].items[7]),
            "Command Palette…"
        );
        assert_eq!(action_name(&menus[VIEW_IDX].items[8]), "Find in Document…");
        assert_eq!(action_name(&menus[VIEW_IDX].items[9]), "Find Next");
        assert_eq!(action_name(&menus[VIEW_IDX].items[10]), "Find Previous");
    }

    #[test]
    fn open_file_dialog_prompt_is_generic() {
        // 用户报修：打开文件的对话框确定铵钮上写着「打开 Markdown 文件」。这个字符串
        // 原样进原生对话框的确定铵钮，必须短而通用；文件夹入口要有自己的文案。
        use crate::i18n::I18nStrings;
        for strings in [I18nStrings::zh_cn(), I18nStrings::en_us()] {
            assert!(!strings.open_markdown_files_prompt.contains("Markdown"));
            assert!(!strings.menu_open_folder.is_empty());
            assert!(!strings.open_folder_prompt.is_empty());
        }
        assert_eq!(I18nStrings::zh_cn().open_markdown_files_prompt, "打开文件");
        assert_eq!(I18nStrings::zh_cn().open_folder_prompt, "打开文件夹");
    }

    #[test]
    fn build_menus_uses_chinese_language_when_selected() {
        let theme_manager = ThemeManager::default();
        let i18n_manager = I18nManager::new_with_language_id("zh-CN");
        let menus = build_menus(&theme_manager, &i18n_manager, &[]);

        #[cfg(target_os = "macos")]
        assert_eq!(
            submenu(&menus[1].items[3]).name.to_string(),
            i18n_manager.strings().menu_open_recent_file.as_str()
        );
        #[cfg(not(target_os = "macos"))]
        assert_eq!(
            submenu(&menus[0].items[3]).name.to_string(),
            i18n_manager.strings().menu_open_recent_file.as_str()
        );

        let menu_names = menus
            .iter()
            .map(|menu| menu.name.to_string())
            .collect::<Vec<_>>();

        #[cfg(target_os = "macos")]
        assert_eq!(
            menu_names,
            vec![
                "Velora",
                "文件",
                "导出",
                "语言",
                "主题",
                "视图",
                "AI",
                "帮助"
            ]
        );
        #[cfg(not(target_os = "macos"))]
        assert_eq!(
            menu_names,
            vec!["文件", "导出", "语言", "主题", "视图", "AI", "帮助"]
        );

        #[cfg(target_os = "macos")]
        assert_eq!(action_name(&menus[1].items[0]), "新建窗口");
        #[cfg(not(target_os = "macos"))]
        assert_eq!(action_name(&menus[0].items[0]), "新建窗口");
        assert_eq!(action_name(&menus[EXPORT_IDX].items[0]), "HTML");
        assert_eq!(action_name(&menus[EXPORT_IDX].items[1]), "PDF");
        assert_eq!(action_name(&menus[EXPORT_IDX].items[2]), "图片（PNG 长图）");
        assert_eq!(action_name(&menus[EXPORT_IDX].items[3]), "打印…");
        assert_eq!(action_name(&menus[EXPORT_IDX].items[4]), "复制为 HTML");
        assert_eq!(
            action_name(&menus[LANGUAGE_IDX].items[0]),
            "\u{2713} 简体中文"
        );
        assert_eq!(action_name(&menus[LANGUAGE_IDX].items[1]), "English");
        assert_eq!(action_name(&menus[VIEW_IDX].items[0]), "切换侧边栏");
        assert_eq!(action_name(&menus[VIEW_IDX].items[1]), "切换全屏");
        assert_eq!(action_name(&menus[VIEW_IDX].items[3]), "切换视图模式");
        assert_eq!(action_name(&menus[VIEW_IDX].items[4]), "切换专注模式");
        assert_eq!(action_name(&menus[VIEW_IDX].items[5]), "切换打字机模式");
        assert_eq!(action_name(&menus[VIEW_IDX].items[7]), "命令面板…");
        assert_eq!(action_name(&menus[VIEW_IDX].items[8]), "查找当前文档…");
        assert_eq!(action_name(&menus[VIEW_IDX].items[9]), "查找下一个");
        assert_eq!(action_name(&menus[VIEW_IDX].items[10]), "查找上一个");
    }

    #[test]
    fn export_menu_items_dispatch_export_actions() {
        let theme_manager = ThemeManager::default();
        let i18n_manager = I18nManager::default();
        let menus = build_menus(&theme_manager, &i18n_manager, &[]);

        match &menus[EXPORT_IDX].items[0] {
            MenuItem::Action { action, .. } => {
                assert!(action.as_any().is::<ExportHtml>());
            }
            _ => panic!("expected export html action item"),
        }

        match &menus[EXPORT_IDX].items[1] {
            MenuItem::Action { action, .. } => {
                assert!(action.as_any().is::<ExportPdf>());
            }
            _ => panic!("expected export pdf action item"),
        }

        // roadmap F5：PNG 长图菜单项排在 PDF 之后。
        match &menus[EXPORT_IDX].items[2] {
            MenuItem::Action { action, .. } => {
                assert!(action.as_any().is::<ExportPng>());
            }
            _ => panic!("expected export png action item"),
        }

        // roadmap F4：打印菜单项存在且分发 PrintDocument，复制为 HTML 顺延到第 5 项。
        match &menus[EXPORT_IDX].items[3] {
            MenuItem::Action { action, .. } => {
                assert!(action.as_any().is::<PrintDocument>());
            }
            _ => panic!("expected print action item"),
        }
        match &menus[EXPORT_IDX].items[4] {
            MenuItem::Action { action, .. } => {
                assert!(action.as_any().is::<CopyAsHtml>());
            }
            _ => panic!("expected copy as html action item"),
        }
    }

    /// roadmap H5：菜单与命令面板同源于命令注册表——逐条对照视图菜单
    /// （文案 + 动作 + 分隔线位置），多一条少一条都会失败。
    #[test]
    fn view_menu_matches_the_command_registry() {
        let theme_manager = ThemeManager::default();
        let i18n_manager = I18nManager::default();
        let menus = build_menus(&theme_manager, &i18n_manager, &[]);
        let strings = i18n_manager.strings();
        let items = &menus[VIEW_IDX].items;

        let mut index = 0;
        for spec in crate::commands::commands_for(crate::commands::CommandMenu::View) {
            if spec.separator_before && index > 0 {
                assert!(
                    matches!(items[index], MenuItem::Separator),
                    "{} 前应有分隔线",
                    spec.id
                );
                index += 1;
            }
            match items.get(index) {
                Some(MenuItem::Action { name, action, .. }) => {
                    assert_eq!(name.as_ref(), spec.label(strings).as_str(), "{} 文案", spec.id);
                    assert!(
                        action.as_ref().partial_eq(spec.boxed_action().as_ref()),
                        "{} 动作类型不一致",
                        spec.id
                    );
                }
                _ => panic!("视图菜单缺少条目 {}", spec.id),
            }
            index += 1;
        }
        assert_eq!(index, items.len(), "视图菜单存在注册表之外的条目");
    }

    #[test]
    fn language_menu_items_dispatch_select_language_actions() {
        let theme_manager = ThemeManager::default();
        let i18n_manager = I18nManager::default();
        let menus = build_menus(&theme_manager, &i18n_manager, &[]);

        match &menus[LANGUAGE_IDX].items[0] {
            MenuItem::Action { action, .. } => {
                let action = action
                    .as_any()
                    .downcast_ref::<SelectLanguage>()
                    .expect("language item should dispatch SelectLanguage");
                assert_eq!(action.language_id, "zh-CN");
            }
            _ => panic!("expected language action item"),
        }
    }

    #[test]
    fn recent_files_submenu_uses_empty_state_when_history_is_empty() {
        let theme_manager = ThemeManager::default();
        let i18n_manager = I18nManager::default();
        let menus = build_menus(&theme_manager, &i18n_manager, &[]);

        // On macOS: File menu is index 1, Open Recent is item 3 within it.
        // On other platforms: File menu is index 0, Open Recent is item 3.
        #[cfg(target_os = "macos")]
        let recent_menu = submenu(&menus[1].items[3]);
        #[cfg(not(target_os = "macos"))]
        let recent_menu = submenu(&menus[0].items[3]);

        assert_eq!(recent_menu.name.to_string(), "Open Recent");
        assert_eq!(recent_menu.items.len(), 1);
        assert_eq!(action_name(&recent_menu.items[0]), "No Recent Files or Folders");
        match &recent_menu.items[0] {
            MenuItem::Action { action, .. } => {
                assert!(action.as_any().is::<NoRecentFiles>());
            }
            _ => panic!("expected empty recent-file action item"),
        }
    }

    #[test]
    fn recent_menu_entries_list_workspaces_only() {
        // 用户要求：「打开最近」只允许出现工作区（文件夹），不允许出现文件。
        let folders = vec![PathBuf::from("/work/alpha"), PathBuf::from("/work/beta")];
        assert_eq!(recent_menu_entries(&folders), folders);
        let duplicated = vec![PathBuf::from("/work/a"), PathBuf::from("/work/a")];
        assert_eq!(recent_menu_entries(&duplicated), vec![PathBuf::from("/work/a")]);
        let many: Vec<PathBuf> = (0..20).map(|index| PathBuf::from(format!("/w/{index}"))).collect();
        assert_eq!(recent_menu_entries(&many).len(), 15);
    }

    #[test]
    fn recent_menu_never_reads_the_file_history() {
        // 结构守卫：菜单与工作区两个入口都只吃 recent-folders；一旦有人把文件
        // 历史接回「打开最近」，这里就会读到文件历史的读取函数与旧的双表合并。
        let source = include_str!("../app_menu.rs");
        // 断言文本里不能出现被搜索的字面量，所以拼出来。
        let file_history_reader = concat!("read_recent_", "files");
        let legacy_merge = concat!("merged_", "recent_entries");
        assert!(
            !source.contains(file_history_reader),
            "app_menu.rs 不应再读文件历史（「打开最近」只列工作区）"
        );
        assert!(
            !source.contains(legacy_merge),
            "旧的「文件+文件夹交错合并」应整体移除"
        );
    }

    #[test]
    fn recent_files_submenu_dispatches_path_actions() {
        let theme_manager = ThemeManager::default();
        let i18n_manager = I18nManager::default();
        let recent_files = vec![
            PathBuf::from(r"C:\docs\one.md"),
            PathBuf::from(r"D:\notes\two.markdown"),
        ];
        let menus = build_menus(&theme_manager, &i18n_manager, &recent_files);

        #[cfg(target_os = "macos")]
        let recent_menu = submenu(&menus[1].items[3]);
        #[cfg(not(target_os = "macos"))]
        let recent_menu = submenu(&menus[0].items[3]);

        assert_eq!(recent_menu.items.len(), 2);
        assert_eq!(action_name(&recent_menu.items[0]), r"C:\docs\one.md");
        match &recent_menu.items[0] {
            MenuItem::Action { action, .. } => {
                let action = action
                    .as_any()
                    .downcast_ref::<OpenRecentFile>()
                    .expect("recent file should dispatch OpenRecentFile");
                assert_eq!(action.path, r"C:\docs\one.md");
            }
            _ => panic!("expected recent-file action item"),
        }
    }

    #[test]
    fn fallback_menu_routes_window_context_actions_without_app_defer() {
        assert!(crate::app_menu::is_window_context_menu_action(&NewWindow));
        assert!(crate::app_menu::is_window_context_menu_action(&OpenFile));
        assert!(crate::app_menu::is_window_context_menu_action(&OpenPreferences));
        assert!(crate::app_menu::is_window_context_menu_action(&OpenRecentFile {
            path: "notes.md".into(),
        }));
        assert!(crate::app_menu::is_window_context_menu_action(&NoRecentFiles));
        assert!(crate::app_menu::is_window_context_menu_action(&AddLanguageConfig));
        assert!(crate::app_menu::is_window_context_menu_action(&AddThemeConfig));
        assert!(crate::app_menu::is_window_context_menu_action(&SaveDocument));
        assert!(crate::app_menu::is_window_context_menu_action(&QuitApplication));
        assert!(crate::app_menu::is_window_context_menu_action(&CloseWindow));
        assert!(!crate::app_menu::is_window_context_menu_action(&SelectTheme {
            theme_id: "velora-dark".into(),
        }));
        assert!(!crate::app_menu::is_window_context_menu_action(&SelectLanguage {
            language_id: "en-US".into(),
        }));
    }

    #[test]
    fn config_import_items_are_bottom_menu_actions() {
        let theme_manager = ThemeManager::default();
        let i18n_manager = I18nManager::default();
        let menus = build_menus(&theme_manager, &i18n_manager, &[]);

        let language_items = &menus[LANGUAGE_IDX].items;
        assert!(matches!(
            language_items[language_items.len() - 2],
            MenuItem::Separator
        ));
        assert_eq!(
            action_name(&language_items[language_items.len() - 1]),
            "Add Language Config"
        );
        match &language_items[language_items.len() - 1] {
            MenuItem::Action { action, .. } => {
                assert!(action.as_any().is::<AddLanguageConfig>());
            }
            _ => panic!("expected add language config action item"),
        }

        let theme_items = &menus[THEME_IDX].items;
        assert_eq!(action_name(&theme_items[0]), "Follow System");
        assert_eq!(action_name(&theme_items[1]), "\u{2713} Dark");
        assert_eq!(action_name(&theme_items[2]), "Light");
        assert!(matches!(
            theme_items[theme_items.len() - 2],
            MenuItem::Separator
        ));
        assert_eq!(
            action_name(&theme_items[theme_items.len() - 1]),
            "Add Theme Config"
        );
        match &theme_items[1] {
            MenuItem::Action { action, .. } => {
                assert!(action.as_any().is::<SelectTheme>());
            }
            _ => panic!("expected select theme action item"),
        }
        match &theme_items[theme_items.len() - 1] {
            MenuItem::Action { action, .. } => {
                assert!(action.as_any().is::<AddThemeConfig>());
            }
            _ => panic!("expected add theme config action item"),
        }
    }

    #[test]
    fn theme_menu_marks_selected_builtin_light_theme() {
        let mut theme_manager = ThemeManager::default();
        assert!(theme_manager.set_theme_by_id("velora-light"));
        let i18n_manager = I18nManager::default();
        let menus = build_menus(&theme_manager, &i18n_manager, &[]);
        let theme_items = &menus[THEME_IDX].items;

        assert_eq!(action_name(&theme_items[0]), "Follow System");
        assert_eq!(action_name(&theme_items[1]), "Dark");
        assert_eq!(action_name(&theme_items[2]), "\u{2713} Light");
        match &theme_items[2] {
            MenuItem::Action { action, .. } => {
                let action = action
                    .as_any()
                    .downcast_ref::<SelectTheme>()
                    .expect("light theme item should dispatch SelectTheme");
                assert_eq!(action.theme_id, "velora-light");
            }
            _ => panic!("expected light theme action item"),
        }
    }

    #[test]
    fn help_menu_omits_upstream_update_action() {
        let theme_manager = ThemeManager::default();
        let i18n_manager = I18nManager::default();
        let menus = build_menus(&theme_manager, &i18n_manager, &[]);
        let help_items = &menus[HELP_IDX].items;

        assert!(help_items.iter().all(|item| match item {
            MenuItem::Action { action, .. } => !action.as_any().is::<CheckForUpdates>(),
            _ => true,
        }));
    }

    #[test]
    #[cfg(not(target_os = "macos"))]
    fn help_menu_contains_about_only() {
        let theme_manager = ThemeManager::default();
        let i18n_manager = I18nManager::default();
        let menus = build_menus(&theme_manager, &i18n_manager, &[]);
        let help_items = &menus[HELP_IDX].items;

        assert_eq!(help_items.len(), 1);
        match &help_items[0] {
            MenuItem::Action { action, .. } => {
                assert!(action.as_any().is::<ShowAbout>());
            }
            _ => panic!("expected about action item"),
        }
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn help_menu_contains_cli_and_about_on_macos() {
        let theme_manager = ThemeManager::default();
        let i18n_manager = I18nManager::default();
        let menus = build_menus(&theme_manager, &i18n_manager, &[]);
        let help_items = &menus[HELP_IDX].items;

        // 安装或卸载命令、分隔线、关于
        assert_eq!(help_items.len(), 3);
        assert!(matches!(help_items[1], MenuItem::Separator));
        match &help_items[2] {
            MenuItem::Action { action, .. } => {
                assert!(action.as_any().is::<ShowAbout>());
            }
            _ => panic!("expected about action item"),
        }
    }
}
