    use super::super::{
        ShortcutCommand, normalize_shortcut_config, resolved_shortcut_keys, shortcut_conflict_for,
        shortcut_definitions,
    };
    use std::collections::{BTreeMap, BTreeSet};

    #[test]
    fn custom_shortcut_replaces_command_defaults() {
        let mut config = BTreeMap::new();
        config.insert("save_document".to_string(), vec!["ctrl-alt-s".to_string()]);

        assert_eq!(
            resolved_shortcut_keys(&config, ShortcutCommand::SaveDocument),
            vec!["ctrl-alt-s".to_string()]
        );
    }

    #[test]
    fn print_document_has_default_shortcuts_outside_command_palette_key() {
        // 打印注册为可自定义命令；⌘P 留给快速打开，默认键为 ⌥⌘P / ⌃⌥P。
        assert_eq!(
            resolved_shortcut_keys(&BTreeMap::new(), ShortcutCommand::PrintDocument),
            vec!["alt-cmd-p".to_string(), "ctrl-alt-p".to_string()]
        );
        assert!(
            shortcut_conflict_for(
                ShortcutCommand::PrintDocument,
                &["alt-cmd-p".to_string(), "ctrl-alt-p".to_string()],
                &BTreeMap::new()
            )
            .is_none()
        );
    }

    #[test]
    fn toggle_view_mode_has_default_shortcuts() {
        assert_eq!(
            resolved_shortcut_keys(&BTreeMap::new(), ShortcutCommand::ToggleViewMode),
            vec!["ctrl-tab".to_string(), "cmd-tab".to_string()]
        );
    }

    #[test]
    fn toggle_sidebar_ships_without_a_default_shortcut() {
        // `ctrl-w` 在这个位置是个坑（Unix/Emacs 删前一个词、浏览器关标签），
        // 误按就收起侧边栏（用户报修「经常错误触发」）。默认不再绑任何键；
        // 空选区下的 cmd/ctrl-b 走加粗那条捕获路径，见
        // `Editor::on_bold_capture`。
        assert!(
            resolved_shortcut_keys(&BTreeMap::new(), ShortcutCommand::ToggleSidebar).is_empty()
        );
    }

    #[test]
    fn toggle_fullscreen_has_default_shortcuts() {
        assert_eq!(
            resolved_shortcut_keys(&BTreeMap::new(), ShortcutCommand::ToggleFullscreen),
            vec!["ctrl-cmd-f".to_string(), "f11".to_string()]
        );
    }

    #[test]
    fn select_all_has_default_shortcuts() {
        assert_eq!(
            resolved_shortcut_keys(&BTreeMap::new(), ShortcutCommand::SelectAll),
            vec!["cmd-a".to_string(), "ctrl-a".to_string()]
        );
        assert!(
            shortcut_conflict_for(
                ShortcutCommand::SelectAll,
                &["cmd-a".to_string(), "ctrl-a".to_string()],
                &BTreeMap::new()
            )
            .is_none()
        );
    }

    #[test]
    fn select_all_shortcut_can_be_customized() {
        let mut config = BTreeMap::new();
        config.insert("select_all".to_string(), vec!["ctrl-shift-a".to_string()]);

        assert_eq!(
            resolved_shortcut_keys(&config, ShortcutCommand::SelectAll),
            vec!["ctrl-shift-a".to_string()]
        );
    }

    /// 「选择全文」是明面上的一条命令，不吃 ⌘A 那条 750ms 循环（报修「全文选择不直观」）。
    /// 键位取 ⌘⇧A / Ctrl+Shift+A，与「全选」同一档位、邻近一颗。
    #[test]
    fn select_document_has_default_shortcuts() {
        assert_eq!(
            resolved_shortcut_keys(&BTreeMap::new(), ShortcutCommand::SelectDocument),
            vec!["cmd-shift-a".to_string(), "ctrl-shift-a".to_string()]
        );
        assert!(
            shortcut_conflict_for(
                ShortcutCommand::SelectDocument,
                &["cmd-shift-a".to_string(), "ctrl-shift-a".to_string()],
                &BTreeMap::new()
            )
            .is_none(),
            "「选择全文」的默认键位不能与键位表里任何一条撞车"
        );
    }

    /// 整张默认键位表逐颗查重：同一档位里一颗键只能归一条命令，撞了就是「按下去执行
    /// 了别的」那类 bug。新增一条绑定先跑这条，把全部默认键位过一遍，而不是只查自己那颗。
    #[test]
    fn every_default_shortcut_key_is_bound_only_once() {
        let mut bound: BTreeSet<(Option<&'static str>, &'static str)> = BTreeSet::new();
        for definition in shortcut_definitions() {
            for key in definition.default_keys {
                assert!(
                    bound.insert((definition.context, *key)),
                    "{key} 在档位 {:?} 下被 `{}` 重复绑定",
                    definition.context,
                    definition.id
                );
            }
        }
    }

    #[test]
    fn legacy_split_select_all_shortcut_config_maps_to_unified_command() {
        let mut config = BTreeMap::new();
        config.insert(
            "select_all_source_text".to_string(),
            vec!["ctrl-shift-a".to_string()],
        );

        assert_eq!(
            resolved_shortcut_keys(&config, ShortcutCommand::SelectAll),
            vec!["ctrl-shift-a".to_string()]
        );

        let normalized = normalize_shortcut_config(&config);
        assert_eq!(
            normalized.get("select_all"),
            Some(&vec!["ctrl-shift-a".to_string()])
        );
        assert!(!normalized.contains_key("select_all_source_text"));
        assert!(!normalized.contains_key("select_focused_block_text_rendered"));

        config.clear();
        config.insert(
            "select_focused_block_text_rendered".to_string(),
            vec!["ctrl-alt-shift-a".to_string()],
        );

        assert_eq!(
            resolved_shortcut_keys(&config, ShortcutCommand::SelectAll),
            vec!["ctrl-alt-shift-a".to_string()]
        );

        let normalized = normalize_shortcut_config(&config);
        assert_eq!(
            normalized.get("select_all"),
            Some(&vec!["ctrl-alt-shift-a".to_string()])
        );
        assert!(!normalized.contains_key("select_all_source_text"));
        assert!(!normalized.contains_key("select_focused_block_text_rendered"));
    }

    #[test]
    fn close_and_quit_defaults_are_platform_specific() {
        #[cfg(target_os = "macos")]
        {
            // cmd-w 归「关闭标签页」；关窗退到 cmd-shift-w（与 VS Code 一致）。
            assert_eq!(
                resolved_shortcut_keys(&BTreeMap::new(), ShortcutCommand::CloseWindow),
                vec!["cmd-shift-w".to_string()]
            );
            assert_eq!(
                resolved_shortcut_keys(&BTreeMap::new(), ShortcutCommand::CloseTab),
                vec!["cmd-w".to_string()]
            );
            assert_eq!(
                resolved_shortcut_keys(&BTreeMap::new(), ShortcutCommand::QuitApplication),
                vec!["cmd-q".to_string()]
            );
        }

        #[cfg(not(target_os = "macos"))]
        {
            assert_eq!(
                resolved_shortcut_keys(&BTreeMap::new(), ShortcutCommand::CloseWindow),
                vec!["ctrl-q".to_string()]
            );
            // 浏览器肌肉记忆：Ctrl+W 关当前标签页，不是关窗口。
            assert_eq!(
                resolved_shortcut_keys(&BTreeMap::new(), ShortcutCommand::CloseTab),
                vec!["ctrl-w".to_string()]
            );
            assert!(
                resolved_shortcut_keys(&BTreeMap::new(), ShortcutCommand::QuitApplication)
                    .is_empty()
            );
        }
    }

    #[test]
    fn word_and_block_shortcuts_have_ctrl_and_alt_defaults() {
        assert_eq!(
            resolved_shortcut_keys(&BTreeMap::new(), ShortcutCommand::WordMoveLeft),
            vec!["ctrl-left".to_string(), "alt-left".to_string()]
        );
        assert_eq!(
            resolved_shortcut_keys(&BTreeMap::new(), ShortcutCommand::WordDeleteBack),
            vec!["ctrl-backspace".to_string(), "alt-backspace".to_string()]
        );
        assert_eq!(
            resolved_shortcut_keys(&BTreeMap::new(), ShortcutCommand::BlockUp),
            vec!["ctrl-up".to_string(), "alt-up".to_string()]
        );
        assert_eq!(
            resolved_shortcut_keys(&BTreeMap::new(), ShortcutCommand::WordSelectRight),
            vec![
                "ctrl-shift-right".to_string(),
                "alt-shift-right".to_string()
            ]
        );
    }

    #[test]
    fn page_navigation_shortcuts_have_defaults() {
        assert_eq!(
            resolved_shortcut_keys(&BTreeMap::new(), ShortcutCommand::PageUp),
            vec!["pageup".to_string()]
        );
        assert_eq!(
            resolved_shortcut_keys(&BTreeMap::new(), ShortcutCommand::PageDown),
            vec!["pagedown".to_string()]
        );
        assert_eq!(
            resolved_shortcut_keys(&BTreeMap::new(), ShortcutCommand::JumpToTop),
            vec!["ctrl-home".to_string(), "cmd-up".to_string()]
        );
        assert_eq!(
            resolved_shortcut_keys(&BTreeMap::new(), ShortcutCommand::JumpToBottom),
            vec!["ctrl-end".to_string(), "cmd-down".to_string()]
        );
    }

    #[test]
    fn invalid_or_empty_shortcuts_fall_back_to_defaults() {
        let mut config = BTreeMap::new();
        config.insert("save_document".to_string(), vec!["".to_string()]);
        config.insert("open_file".to_string(), vec!["a".to_string()]);

        let normalized = normalize_shortcut_config(&config);
        assert!(!normalized.contains_key("save_document"));
        assert!(!normalized.contains_key("open_file"));
    }

    #[test]
    fn conflicting_custom_shortcut_falls_back_to_default() {
        let mut config = BTreeMap::new();
        config.insert("copy".to_string(), vec!["ctrl-x".to_string()]);

        let normalized = normalize_shortcut_config(&config);
        assert!(!normalized.contains_key("copy"));
        assert_eq!(
            resolved_shortcut_keys(&config, ShortcutCommand::Copy),
            vec!["cmd-c".to_string(), "ctrl-c".to_string()]
        );
    }

    #[test]
    fn detects_shortcut_conflicts_for_preferences_drafts() {
        let conflict = shortcut_conflict_for(
            ShortcutCommand::Copy,
            &["ctrl-x".to_string()],
            &BTreeMap::new(),
        )
        .expect("copy should conflict with cut");

        assert_eq!(conflict.id, "cut");
    }

    #[test]
    fn effective_shortcut_table_follows_installed_bindings() {
        let mut config = BTreeMap::new();
        config.insert("bold_selection".to_string(), vec!["cmd-alt-b".to_string()]);
        // `ctrl-x` 与「剪切」撞车，这条自定义会被退回默认键——显示表要跟着实际绑上的那颗。
        config.insert("copy".to_string(), vec!["ctrl-x".to_string()]);

        let shortcuts = super::EffectiveShortcuts::build(&config);
        assert_eq!(
            shortcuts.key(ShortcutCommand::BoldSelection),
            Some(if cfg!(target_os = "macos") { "alt-cmd-b" } else if cfg!(target_os = "windows") { "alt-win-b" } else { "alt-super-b" }),
            "改过绑定的命令，显示的那颗键要改成用户定的（写进表里的是规范化后的键序）"
        );
        assert_eq!(
            shortcuts.key(ShortcutCommand::Copy),
            Some("cmd-c"),
            "撞车而被退回默认键的命令，显示表不能留着那颗绑不上的键"
        );
        assert_eq!(
            shortcuts.key(ShortcutCommand::ItalicSelection),
            Some("cmd-i"),
            "没改过的命令仍是默认键"
        );
    }
