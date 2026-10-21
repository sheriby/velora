mod tests {
    use crate::app_menu::build_menus_impl as build_menus;
    use crate::app_menu::{
        applescript_string_literal,
        recent_menu_entries,
    };
    use crate::components::{
        AddLanguageConfig, AddThemeConfig, CheckForUpdates, CloseWindow, NewDocument, NoRecentFiles,
        OpenFile, OpenPreferences, OpenRecentFile, QuitApplication, SaveDocument, SelectLanguage,
        SelectTheme, ShowAbout,
    };
    use crate::commands::{CommandMenu, CommandSpec, commands, commands_for};
    use crate::i18n::I18nManager;
    use crate::theme::ThemeManager;
    use gpui::{Menu, MenuItem};
    use std::any::TypeId;
    use std::path::PathBuf;

    /// 菜单树代数守卫：install_menus 一次、代数进一——渲染侧的窗内菜单缓存
    /// 靠它失效（谁把 bump 丢了，Windows 上菜单就永远停在旧树）。
    #[gpui::test]
    async fn installing_menus_bumps_the_tree_generation(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
        });
        let before = crate::app_menu::build_menus::menu_tree_generation();
        cx.update(|cx| crate::app_menu::build_menus::install_menus(cx));
        let after = crate::app_menu::build_menus::menu_tree_generation();
        assert!(after > before, "install_menus 必须推进菜单树代数（{before} → {after}）");
    }

    #[test]
    fn ai_opt_in_native_menu_excludes_ai_actions_when_disabled() {
        fn has_ai(items: &[MenuItem]) -> bool {
            items.iter().any(|item| match item {
                MenuItem::Action { action, .. } => action.as_any().is::<crate::components::OpenAiAssistant>(),
                MenuItem::Submenu(menu) => has_ai(&menu.items),
                _ => false,
            })
        }
        let theme_manager = ThemeManager::default();
        let i18n_manager = I18nManager::default();
        let disabled = build_menus(&theme_manager, &i18n_manager, &[], false);
        assert!(!disabled.iter().any(|menu| has_ai(&menu.items)));
        let enabled = build_menus(&theme_manager, &i18n_manager, &[], true);
        assert!(enabled.iter().any(|menu| has_ai(&menu.items)));
    }

    #[test]
    fn windows_manifest_is_embedded_only_by_velora_in_builds_and_tests() {
        let manifest: toml::Value = toml::from_str(include_str!("../../Cargo.toml")).expect("应用 Cargo 配置");
        let core: toml::Value = toml::from_str(include_str!("../../vendor/gpui/crates/gpui/Cargo.toml")).expect("GPUI Cargo 配置");
        assert!(include_str!("../../resources/windows/velora.rc").contains("1 RT_MANIFEST"));
        assert!(include_str!("../../resources/windows/velora.manifest.xml").contains("Microsoft.Windows.Common-Controls"));
        for section in ["dependencies", "dev-dependencies"] {
            assert!(manifest[section].get("gpui").is_none(), "{section} 不应再引入另一份旧 GPUI");
            assert!(manifest[section].get("gpui-kit").is_some(), "{section} 应共用 Kit 入口");
        }
        let defaults = core["features"]["default"].as_array().expect("GPUI 默认特性");
        assert!(defaults.iter().any(|feature| feature.as_str() == Some("windows-manifest-provided-by-host")),
            "Kit 会合并开启 GPUI 默认特性，核心必须保留宿主 manifest 的优先权");
        assert!(include_str!("../../vendor/gpui/crates/gpui/build.rs").contains("not(feature = \"windows-manifest-provided-by-host\")"),
            "核心构建脚本应跳过宿主已提供的 manifest");
    }

    // macOS 门禁也要发现 Windows 补丁被漏带；这组源码守卫不代替原生系统验收。
    #[test]
    fn vendored_windows_ime_diagnostics_cover_committed_and_composing_text() {
        let source = include_str!("../../vendor/gpui/crates/gpui_windows/src/events.rs");
        for event in [
            "handled_by_app=",
            "WM_CHAR {input:?}",
            "WM_IME_COMPOSITION lparam=",
            "GCS_COMPSTR",
            "GCS_RESULTSTR",
            "无输入处理器",
        ] {
            assert!(
                source.lines().any(|line| line.contains(event)
                    && (line.contains('"') || line.contains("ime_debug_log"))),
                "Windows IME 诊断缺少事件：{event}"
            );
        }
    }

    #[test]
    fn kit_components_use_upstream_text_runs_without_local_overrides() {
        let manifest: toml::Value = toml::from_str(include_str!("../../Cargo.toml")).expect("应用依赖");
        for package in ["gpui-base", "gpui-component"] {
            assert!(manifest["patch"]["crates-io"].get(package).is_none(), "组件层应直接使用上游发布包：{package}");
        }
        let core = include_str!("../../vendor/gpui/crates/gpui/src/text_system.rs");
        let text_run = core.split_once("pub struct TextRun {").expect("TextRun").1.split('}').next().expect("字段");
        assert!(!text_run.contains("font_size"), "字号扩展不能改变普通 TextRun 的构造契约");
    }

    #[test]
    fn gpui_submodule_is_declared_and_every_build_workflow_fetches_it() {
        let modules = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/.gitmodules"))
            .unwrap_or_default();
        assert!(modules.contains("[submodule \"gpui\"]"), "GPUI 源码应由独立子模块记录提交");
        assert!(modules.contains("https://github.com/sheriby/velora-gpui.git"), "公开子模块应支持无 SSH 密钥检出");
        let manifest: toml::Value = toml::from_str(include_str!("../../Cargo.toml")).expect("应用依赖");
        for package in ["gpui-pre", "gpui-pre-apple", "gpui-pre-macos", "gpui-pre-windows", "gpui-pre-linux", "gpui-pre-wgpu"] {
            assert!(manifest["patch"]["crates-io"][package]["path"].as_str().is_some_and(|path| path.starts_with("vendor/gpui/crates/")), "{package} 应来自同一子模块");
        }
        for (name, workflow) in [
            ("ci", include_str!("../../.github/workflows/ci.yml")),
            ("release", include_str!("../../.github/workflows/release.yml")),
        ] {
            assert_eq!(workflow.matches("uses: actions/checkout@").count(), workflow.matches("submodules: recursive").count(), "{name} 的全部检出步骤必须获取子模块");
        }
    }

    #[test]
    fn vendored_windows_file_dialogs_keep_folder_fallback_and_persisted_identity() {
        let source = include_str!("../../vendor/gpui/crates/gpui_windows/src/platform.rs");
        assert!(
            source.contains("fn shell_folder_item("),
            "起始目录解析失败应回退默认目录，不能中断打开或保存"
        );
        assert!(
            source.contains("0x7d1c2f5e_9a44_4b3c_8f61_5e0a9b6c4d27"),
            "保留既有 client GUID，不重置用户的选择器状态"
        );
        assert_eq!(
            source
                .matches("SetClientGuid(&VELORA_FILE_DIALOG_CLIENT_GUID)")
                .count(),
            2,
            "打开与保存共用原来的持久化标识"
        );
        let dialogs = source
            .split_once("fn file_open_dialog(")
            .expect("打开对话框")
            .1;
        assert!(
            dialogs.contains("and_then(shell_folder_item)"),
            "打开对话框应允许起始目录解析失败"
        );
        assert!(
            dialogs.contains("shell_folder_item(&directory)"),
            "保存对话框应允许起始目录解析失败"
        );
        assert!(
            !dialogs.contains("SetFolder(&folder)?"),
            "设置目录失败也应继续显示对话框"
        );
    }

    #[test]
    fn vendored_linux_backends_keep_the_existing_core_field_compatibility() {
        let manifest: toml::Value =
            toml::from_str(include_str!("../../Cargo.toml")).expect("应用 Cargo 配置");
        for package in ["gpui-pre-linux", "gpui-pre-wgpu"] {
            assert!(
                manifest["patch"]["crates-io"].get(package).is_some(),
                "新增核心字段时不能遗漏平台构造器：{package}"
            );
        }
        let portal = include_str!("../../vendor/gpui/crates/gpui_linux/src/linux/platform.rs");
        let open = portal
            .split_once("fn prompt_for_paths(")
            .expect("打开入口")
            .1
            .split("fn prompt_for_new_path(")
            .next()
            .expect("打开实现");
        assert!(
            open.contains("options.directory") && open.contains(".current_folder("),
            "Linux 打开选择器保留原有起始目录能力"
        );
        let clipboard = include_str!("../../vendor/gpui/crates/gpui_linux/src/linux/wayland/clipboard.rs");
        assert!(!clipboard.contains("html: None"), "HTML 载荷留在应用层，不再要求平台补字段");
        let text = include_str!("../../vendor/gpui/crates/gpui_wgpu/src/cosmic_text_system.rs");
        assert!(
            text.contains("font_size: None"),
            "CosmicText 仍按整行字号排版，补齐结果字段"
        );
        assert!(
            text.contains("font_size: run.font_size"),
            "裁切 FontRun 时不能丢掉核心新增字段"
        );
    }
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

    /// 「打开最近」子菜单所在的序号：文件菜单里紧跟「打开文件夹」。
    fn recent_submenu_slot(file_menu: &Menu) -> usize {
        command_slot(file_menu, "open_folder") + 1
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
        let menus = build_menus(&theme_manager, &i18n_manager, &[], true);

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
        let file_menu = command_menu(&menus, "new_document");
        assert_eq!(file_menu.name.to_string(), "File");

        // 新建文档领文件菜单，关闭标签页与关闭窗口紧跟其后。
        assert_eq!(command_slot(file_menu, "new_document"), 0);
        assert_eq!(command_slot(file_menu, "close_tab"), 1);
        assert_eq!(command_slot(file_menu, "close_window"), 2);
        assert_eq!(command_label(file_menu, "new_document"), "New Document");
        assert_eq!(command_label(file_menu, "close_tab"), "Close Tab");
        assert_eq!(command_label(file_menu, "close_window"), "Close Window");

        // 三个打开入口连续排列。
        assert_eq!(recent_submenu(&menus).name.to_string(), "Open Recent");

        // macOS 的偏好设置保留在应用菜单；Windows 由汉堡一级入口承接。
        #[cfg(target_os = "macos")]
        let preferences_menu = command_menu(&menus, "preferences");
        #[cfg(target_os = "macos")]
        assert_eq!(command_label(preferences_menu, "preferences"), "Preferences");
        #[cfg(target_os = "macos")]
        assert_eq!(preferences_menu.name.to_string(), "Velora");
        #[cfg(target_os = "macos")]
        assert_eq!(command_slot(preferences_menu, "preferences"), 0);

        let export_menu = command_menu(&menus, "export_html");
        assert_eq!(command_slot(export_menu, "export_html"), 0);
        assert_eq!(command_label(export_menu, "export_html"), "HTML");
        assert_eq!(command_label(export_menu, "export_pdf"), "PDF");
        assert_eq!(command_label(export_menu, "export_png"), "Image (PNG)");
        assert_eq!(command_label(export_menu, "print"), "Print…");

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
        // 打开文件的对话框确定铵钮上曾写着「打开 Markdown 文件」。这个字符串
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
        let menus = build_menus(&theme_manager, &i18n_manager, &[], true);

        // 菜单项一律按命令 id 找：平台差异只体现在「命令落在哪个菜单」，
        // 不再写成 menus[1] / menus[0] 这种下标算术（下标会随注册表增删漂移）。
        let file_menu = command_menu(&menus, "new_document");
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

        assert_eq!(command_slot(file_menu, "new_document"), 0);
        assert_eq!(command_slot(file_menu, "close_tab"), 1);
        assert_eq!(command_slot(file_menu, "close_window"), 2);
        assert_eq!(command_label(file_menu, "new_document"), "新建文档");
        assert_eq!(command_label(file_menu, "close_tab"), "关闭标签页");
        assert_eq!(command_label(file_menu, "close_window"), "关闭窗口");

        let export_menu = command_menu(&menus, "export_html");
        assert_eq!(command_slot(export_menu, "export_html"), 0);
        assert_eq!(command_label(export_menu, "export_html"), "HTML");
        assert_eq!(command_label(export_menu, "export_pdf"), "PDF");
        assert_eq!(command_label(export_menu, "export_png"), "图片（PNG 长图）");
        assert_eq!(command_label(export_menu, "print"), "打印…");

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
        let menus = build_menus(&theme_manager, &i18n_manager, &[], true);

        // 导出菜单就是注册表 Export 分组那四条，顺序与注册表一致（roadmap F4/F5：
        // 打印排在 PNG 之后）。逐条对照文案与动作，多一条少一条都会失败。
        let export_menu = command_menu(&menus, "export_html");
        let expected = [
            ("export_html", "HTML"),
            ("export_pdf", "PDF"),
            ("export_png", "Image (PNG)"),
            ("print", "Print…"),
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
        let menus = build_menus(&theme_manager, &i18n_manager, &[], true);
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
        let menus = build_menus(&theme_manager, &i18n_manager, &[], true);

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
        let menus = build_menus(&theme_manager, &i18n_manager, &[], true);

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
        let menus = build_menus(&theme_manager, &i18n_manager, &recent_files, true);

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
        assert!(crate::app_menu::is_window_context_menu_action(&NewDocument));
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
        let menus = build_menus(&theme_manager, &i18n_manager, &[], true);

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
        let menus = build_menus(&theme_manager, &i18n_manager, &[], true);
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
    #[cfg(target_os = "macos")]
    fn application_menu_includes_a_manual_release_update_check() {
        let theme_manager = ThemeManager::default();
        let i18n_manager = I18nManager::default();
        let menus = build_menus(&theme_manager, &i18n_manager, &[], true);
        let items = &command_menu(&menus, "preferences").items;
        assert!(items.iter().any(|item| match item {
            MenuItem::Action { action, .. } => action.as_any().is::<CheckForUpdates>(),
            _ => false,
        }), "设置入口附近应提供手动检查更新");
    }

    #[test]
    fn help_menu_contains_about_only() {
        let theme_manager = ThemeManager::default();
        let i18n_manager = I18nManager::default();
        let menus = build_menus(&theme_manager, &i18n_manager, &[], true);
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
    fn open_file_folder_and_recent_are_adjacent_in_each_platform_file_menu() {
        let strings = crate::i18n::I18nStrings::en_us();
        for has_app_menu in [true, false] {
            let menu = Menu {
            disabled: false,
                name: strings.menu_file.clone().into(),
                items: crate::app_menu::command_menus::file_menu_items(&strings, Vec::new(), has_app_menu),
            };
            let file_index = command_slot(&menu, "open_file");
            assert_eq!(command_slot(&menu, "open_folder"), file_index + 1, "打开文件和打开文件夹之间不能插入其他条目");
            assert!(matches!(menu.items.get(file_index + 2), Some(MenuItem::Submenu(recent)) if recent.name.as_ref() == strings.menu_open_recent_file));
        }
    }

    #[test]
    fn windows_file_menu_excludes_preferences_and_update_check() {
        let strings = crate::i18n::I18nStrings::en_us();
        let items = crate::app_menu::command_menus::file_menu_items(&strings, Vec::new(), false);
        assert!(!items.iter().any(|item| matches!(item, MenuItem::Action { action, .. } if action.as_any().is::<OpenPreferences>() || action.as_any().is::<CheckForUpdates>())), "偏好设置和检查更新应直接出现在 Windows 一级菜单");
    }

    #[test]
    fn ai_sidebar_menu_entry_is_in_view_and_respects_opt_in() {
        let command=crate::commands::commands().iter().find(|command|command.id=="ai_sidebar")
            .expect("视图菜单缺少 AI 侧边栏命令");
        assert_eq!(command.menu,CommandMenu::View);
        assert_eq!(command.label(&crate::i18n::I18nStrings::zh_cn()),"AI 侧边栏");
        assert_eq!(command.label(&crate::i18n::I18nStrings::en_us()),"AI Sidebar");
        let action=command.boxed_action();
        for enabled in [false,true] {
            let menus=build_menus(&ThemeManager::default(),&I18nManager::default(),&[],enabled);
            let view=menus.iter().find(|menu|menu.name.as_ref()=="View").unwrap();
            assert_eq!(view.items.iter().any(|item|matches!(item,MenuItem::Action{action: candidate,..} if candidate.as_any().type_id()==action.as_any().type_id())),enabled);
        }
    }

#[test]
fn kit_ui_icons_are_available_without_loading_the_full_catalog() {
    use gpui::AssetSource;
    let extra_icons = [
        "icons/image.svg",
        "icons/keyboard.svg",
        "icons/sparkles.svg",
        "icons/app-window.svg",
        "icons/trash.svg",
        "icons/list-tree.svg",
        "icons/case-sensitive.svg",
        "icons/whole-word.svg",
        "icons/regex.svg",
        "icons/replace.svg",
        "icons/replace-all.svg",
    ];
    for path in extra_icons {
        assert!(matches!(crate::VeloraAssets.load(path), Ok(Some(_))), "界面图标应可加载：{path}");
    }
    let mut expected = gpui_kit::assets::Assets.list("icons").unwrap();
    expected.extend(extra_icons.into_iter().map(Into::into));
    expected.sort();
    expected.dedup();
    assert_eq!(crate::VeloraAssets.list("icons").unwrap(), expected,
        "只嵌入实际使用的补充图标，并去除默认包中的重复项");
}
}
