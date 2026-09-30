    use super::super::{
        ShortcutCommand, normalize_shortcut_config, resolved_shortcut_keys, shortcut_conflict_for,
    };
    use std::collections::BTreeMap;

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
    fn toggle_sidebar_has_default_shortcuts() {
        assert_eq!(
            resolved_shortcut_keys(&BTreeMap::new(), ShortcutCommand::ToggleSidebar),
            vec!["ctrl-w".to_string()]
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
            assert_eq!(
                resolved_shortcut_keys(&BTreeMap::new(), ShortcutCommand::CloseWindow),
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
