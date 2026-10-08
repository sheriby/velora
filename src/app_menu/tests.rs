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
    use crate::commands::{CommandMenu, CommandSpec, commands, commands_for};
    use crate::i18n::I18nManager;
    use crate::theme::ThemeManager;
    use gpui::{Menu, MenuItem};
    use std::any::TypeId;
    use std::path::PathBuf;

    fn action_name(item: &MenuItem) -> &str {
        match item {
            MenuItem::Action { name, .. } => name.as_ref(),
            _ => panic!("expected action menu item"),
        }
    }

    fn submenu(item: &MenuItem) -> &Menu {
        match item {
            MenuItem::Submenu(menu) => menu,
            _ => panic!("expected submenu item"),
        }
    }

    /// 注册表里的命令。
    fn command_spec(command_id: &str) -> &'static CommandSpec {
        commands()
            .iter()
            .find(|spec| spec.id == command_id)
            .unwrap_or_else(|| panic!("命令注册表里没有 {command_id}"))
    }

    /// 菜单条目的身份就是它的动作类型：按 id 找条目、判断命令落在哪个菜单都靠它。
    /// 注册表是「id → 动作」的唯一出处，`registry_action_types_are_unique` 守着
    /// 一条命令一个动作类型。
    fn action_type_of(command_id: &str) -> TypeId {
        command_spec(command_id).boxed_action().as_any().type_id()
    }

    fn item_action_type(item: &MenuItem) -> Option<TypeId> {
        match item {
            MenuItem::Action { action, .. } => Some(action.as_any().type_id()),
            _ => None,
        }
    }

    /// 命令在菜单里的序号。写死下标会在注册表增删条目时悄悄错位（中文菜单用例
    /// 漏了平台分支，CI 红过一次），所以一律按 id 找。
    fn command_slot(menu: &Menu, command_id: &str) -> usize {
        let wanted = action_type_of(command_id);
        menu.items
            .iter()
            .position(|item| item_action_type(item) == Some(wanted))
            .unwrap_or_else(|| panic!("菜单「{}」里没有命令 {command_id}", menu.name.to_string()))
    }

    /// 命令在菜单里的文案。
    fn command_label(menu: &Menu, command_id: &str) -> String {
        action_name(&menu.items[command_slot(menu, command_id)]).to_string()
    }

    /// 命令所在的菜单。平台与语言都影响菜单名和菜单数量，所以按内容认，不按下标。
    fn command_menu<'a>(menus: &'a [Menu], command_id: &str) -> &'a Menu {
        let wanted = action_type_of(command_id);
        menus
            .iter()
            .find(|menu| {
                menu.items
                    .iter()
                    .any(|item| item_action_type(item) == Some(wanted))
            })
            .unwrap_or_else(|| panic!("没有菜单包含命令 {command_id}"))
    }

    /// 按名字取菜单：语言与主题菜单不是注册表命令生成的，只能认名字。
    fn named_menu<'a>(menus: &'a [Menu], name: &str) -> &'a Menu {
        menus
            .iter()
            .find(|menu| menu.name.as_ref() == name)
            .unwrap_or_else(|| panic!("没有名为「{name}」的菜单"))
    }

    /// 「打开最近」子菜单所在的序号：文件菜单里紧跟「打开文件」。
    fn recent_submenu_slot(file_menu: &Menu) -> usize {
        command_slot(file_menu, "open_file") + 1
    }

    /// 「打开最近」子菜单本身。两个平台同位置，不再分平台写下标。
    fn recent_submenu(menus: &[Menu]) -> &Menu {
        let file_menu = command_menu(menus, "open_file");
        submenu(&file_menu.items[recent_submenu_slot(file_menu)])
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
                "Help"
            ]
        );

        // 菜单项一律按命令 id 找：平台差异只体现在「命令落在哪个菜单」，
        // 不再写成 menus[1] / menus[0] 这种下标算术（下标会随注册表增删漂移）。
        let file_menu = command_menu(&menus, "new_window");
        assert_eq!(file_menu.name.to_string(), "File");

        // New Window 领文件菜单，关闭标签页与关闭窗口紧跟其后。
        assert_eq!(command_slot(file_menu, "new_window"), 0);
        assert_eq!(command_slot(file_menu, "close_tab"), 1);
        assert_eq!(command_slot(file_menu, "close_window"), 2);
        assert_eq!(command_label(file_menu, "new_window"), "New Window");
        assert_eq!(command_label(file_menu, "close_tab"), "Close Tab");
        assert_eq!(command_label(file_menu, "close_window"), "Close Window");

        // 「打开最近」紧跟在「打开文件」之后。
        assert_eq!(recent_submenu(&menus).name.to_string(), "Open Recent");

        // 偏好设置：macOS 在应用菜单首项，其他平台跟在「打开最近」之后。
        let preferences_menu = command_menu(&menus, "preferences");
        assert_eq!(command_label(preferences_menu, "preferences"), "Preferences");
        #[cfg(target_os = "macos")]
        assert_eq!(preferences_menu.name.to_string(), "Velora");
        #[cfg(not(target_os = "macos"))]
        assert_eq!(preferences_menu.name.to_string(), "File");
        #[cfg(target_os = "macos")]
        assert_eq!(command_slot(preferences_menu, "preferences"), 0);
        #[cfg(not(target_os = "macos"))]
        assert_eq!(
            command_slot(preferences_menu, "preferences"),
            recent_submenu_slot(file_menu) + 1
        );

        let export_menu = command_menu(&menus, "export_html");
        assert_eq!(command_slot(export_menu, "export_html"), 0);
        assert_eq!(command_label(export_menu, "export_html"), "HTML");
        assert_eq!(command_label(export_menu, "export_pdf"), "PDF");
        assert_eq!(command_label(export_menu, "export_png"), "Image (PNG)");
        assert_eq!(command_label(export_menu, "print"), "Print…");
        assert_eq!(command_label(export_menu, "copy_as_html"), "Copy as HTML");

        let language_menu = named_menu(&menus, "Language");
        assert_eq!(action_name(&language_menu.items[0]), "简体中文");
        assert_eq!(action_name(&language_menu.items[1]), "\u{2713} English");

        let view_menu = command_menu(&menus, "toggle_sidebar");
        assert_eq!(view_menu.name.to_string(), "View");
        assert_eq!(command_label(view_menu, "toggle_sidebar"), "Toggle Sidebar");
        assert_eq!(
            command_label(view_menu, "toggle_fullscreen"),
            "Toggle Full Screen"
        );
        assert_eq!(
            command_label(view_menu, "toggle_view_mode"),
            "Toggle View Mode"
        );
        assert_eq!(
            command_label(view_menu, "toggle_focus_mode"),
            "Toggle Focus Mode"
        );
        assert_eq!(
            command_label(view_menu, "toggle_typewriter_mode"),
            "Toggle Typewriter Mode"
        );
        assert_eq!(
            command_label(view_menu, "open_command_palette"),
            "Command Palette…"
        );
        assert_eq!(
            command_label(view_menu, "find_in_document"),
            "Find in Document…"
        );
        assert_eq!(command_label(view_menu, "find_next"), "Find Next");
        assert_eq!(command_label(view_menu, "find_previous"), "Find Previous");
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

        // 菜单项一律按命令 id 找：平台差异只体现在「命令落在哪个菜单」，
        // 不再写成 menus[1] / menus[0] 这种下标算术（下标会随注册表增删漂移）。
        let file_menu = command_menu(&menus, "new_window");
        assert_eq!(file_menu.name.to_string(), "文件");

        assert_eq!(
            recent_submenu(&menus).name.to_string(),
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
                "帮助"
            ]
        );
        #[cfg(not(target_os = "macos"))]
        assert_eq!(
            menu_names,
            vec!["文件", "导出", "语言", "主题", "视图", "帮助"]
        );

        assert_eq!(command_slot(file_menu, "new_window"), 0);
        assert_eq!(command_slot(file_menu, "close_tab"), 1);
        assert_eq!(command_slot(file_menu, "close_window"), 2);
        assert_eq!(command_label(file_menu, "new_window"), "新建窗口");
        assert_eq!(command_label(file_menu, "close_tab"), "关闭标签页");
        assert_eq!(command_label(file_menu, "close_window"), "关闭窗口");

        let export_menu = command_menu(&menus, "export_html");
        assert_eq!(command_slot(export_menu, "export_html"), 0);
        assert_eq!(command_label(export_menu, "export_html"), "HTML");
        assert_eq!(command_label(export_menu, "export_pdf"), "PDF");
        assert_eq!(command_label(export_menu, "export_png"), "图片（PNG 长图）");
        assert_eq!(command_label(export_menu, "print"), "打印…");
        assert_eq!(command_label(export_menu, "copy_as_html"), "复制为 HTML");

        let language_menu = named_menu(&menus, "语言");
        assert_eq!(action_name(&language_menu.items[0]), "\u{2713} 简体中文");
        assert_eq!(action_name(&language_menu.items[1]), "English");

        let view_menu = command_menu(&menus, "toggle_sidebar");
        assert_eq!(view_menu.name.to_string(), "视图");
        assert_eq!(command_label(view_menu, "toggle_sidebar"), "切换侧边栏");
        assert_eq!(command_label(view_menu, "toggle_fullscreen"), "切换全屏");
        assert_eq!(command_label(view_menu, "toggle_view_mode"), "切换视图模式");
        assert_eq!(command_label(view_menu, "toggle_focus_mode"), "切换专注模式");
        assert_eq!(
            command_label(view_menu, "toggle_typewriter_mode"),
            "切换打字机模式"
        );
        assert_eq!(
            command_label(view_menu, "open_command_palette"),
            "命令面板…"
        );
        assert_eq!(
            command_label(view_menu, "find_in_document"),
            "查找当前文档…"
        );
        assert_eq!(command_label(view_menu, "find_next"), "查找下一个");
        assert_eq!(command_label(view_menu, "find_previous"), "查找上一个");
    }

    #[test]
    fn export_menu_items_dispatch_export_actions() {
        let theme_manager = ThemeManager::default();
        let i18n_manager = I18nManager::default();
        let menus = build_menus(&theme_manager, &i18n_manager, &[]);

        // 导出菜单就是注册表 Export 分组那五条，顺序与注册表一致（roadmap F4/F5：
        // 打印排在 PNG 之后，复制为 HTML 顺延到最后）。逐条对照文案与动作，
        // 多一条少一条都会失败。
        let export_menu = command_menu(&menus, "export_html");
        let expected = [
            ("export_html", "HTML"),
            ("export_pdf", "PDF"),
            ("export_png", "Image (PNG)"),
            ("print", "Print…"),
            ("copy_as_html", "Copy as HTML"),
        ];
        assert_eq!(export_menu.items.len(), expected.len());
        for (slot, (command_id, label)) in expected.into_iter().enumerate() {
            match export_menu.items.get(slot) {
                Some(MenuItem::Action { name, action, .. }) => {
                    assert_eq!(name.as_ref(), label, "{command_id} 的文案");
                    let registered = command_spec(command_id).boxed_action();
                    assert!(
                        action.as_ref().partial_eq(registered.as_ref()),
                        "{command_id} 的动作类型不一致"
                    );
                }
                _ => panic!("导出菜单第 {slot} 项应是 {command_id}"),
            }
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
        let items = &command_menu(&menus, "toggle_sidebar").items;

        let mut index = 0;
        for spec in commands_for(CommandMenu::View) {
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

        match &named_menu(&menus, "Language").items[0] {
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

        let recent_menu = recent_submenu(&menus);

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

        let recent_menu = recent_submenu(&menus);

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

        let language_items = &named_menu(&menus, "Language").items;
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

        let theme_items = &named_menu(&menus, "Theme").items;
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
        let theme_items = &named_menu(&menus, "Theme").items;

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
        let help_items = &command_menu(&menus, "show_about").items;

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
        let help_items = &command_menu(&menus, "show_about").items;

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
        let help_items = &command_menu(&menus, "show_about").items;

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
