    use super::{
        NoRecentFiles, RenderedRowSpacingInfo, callout_row_top_gap, editor_text_font,
        focus_mode_row_opacity, hamburger_menu_item_panel_origin, hamburger_menu_panel_top,
        hamburger_menu_row_top, import_menu_split_index, in_window_menu_bar_height_for_target_os,
        menu_bar_button_width, menu_items_visual_height_with_gaps, menu_panel_left,
        menu_panel_width_for_labels, owned_menu_item_labels, rendered_row_top_gap,
        scrollable_import_menu_scroll_height, submenu_bridge_geometry,
        supports_hamburger_menu_for_target_os, supports_in_window_menu_for_target_os,
        supports_menu_bar_row_for_target_os, tibetan_font_fallbacks_for_target_os,
        typewriter_target_scroll_offset,
    };
    use crate::components::{AddLanguageConfig, AddThemeConfig};
    use crate::theme::Theme;
    use gpui::{OwnedMenu, OwnedMenuItem};
    use uuid::Uuid;

    fn disabled_menu_action(name: &str) -> OwnedMenuItem {
        OwnedMenuItem::Action {
            name: name.into(),
            action: Box::new(NoRecentFiles),
            os_action: None,
        }
    }

    fn add_theme_menu_action() -> OwnedMenuItem {
        OwnedMenuItem::Action {
            name: "Add Theme Config".into(),
            action: Box::new(AddThemeConfig),
            os_action: None,
        }
    }

    fn add_language_menu_action() -> OwnedMenuItem {
        OwnedMenuItem::Action {
            name: "Add Language Config".into(),
            action: Box::new(AddLanguageConfig),
            os_action: None,
        }
    }

    #[test]
    fn contiguous_quote_rows_collapse_inter_row_gap() {
        let group = Uuid::new_v4();
        let gap = rendered_row_top_gap(
            Some(RenderedRowSpacingInfo {
                quote_group_anchor: Some(group),
                ..RenderedRowSpacingInfo::default()
            }),
            RenderedRowSpacingInfo {
                quote_group_anchor: Some(group),
                ..RenderedRowSpacingInfo::default()
            },
            4.0,
            true,
        );
        assert_eq!(gap, 0.0);
    }

    #[test]
    fn focus_mode_fades_only_rows_outside_the_focused_group() {
        assert_eq!(focus_mode_row_opacity(true, Some(3), 2, 5), 1.0);
        assert_eq!(focus_mode_row_opacity(true, Some(3), 0, 2), 0.38);
        assert_eq!(focus_mode_row_opacity(false, Some(3), 0, 2), 1.0);
        assert_eq!(focus_mode_row_opacity(true, None, 0, 2), 1.0);
    }

    #[test]
    fn typewriter_scroll_centers_caret_within_document_limits() {
        assert_eq!(
            typewriter_target_scroll_offset(-120.0, 400.0, 600.0, 500.0),
            -320.0
        );
        assert_eq!(
            typewriter_target_scroll_offset(0.0, 400.0, 200.0, 500.0),
            0.0
        );
        assert_eq!(
            typewriter_target_scroll_offset(-200.0, 400.0, 700.0, 300.0),
            -300.0
        );
    }

    #[test]
    fn editor_text_font_keeps_system_ui_as_primary_family() {
        assert_eq!(
            editor_text_font(".SystemUIFont").family.to_string(),
            ".SystemUIFont"
        );
    }

    #[test]
    fn tibetan_font_fallbacks_prioritize_platform_defaults() {
        assert_eq!(
            tibetan_font_fallbacks_for_target_os("windows")
                .first()
                .map(String::as_str),
            Some("Microsoft Himalaya")
        );
        assert_eq!(
            tibetan_font_fallbacks_for_target_os("macos")
                .first()
                .map(String::as_str),
            Some("Kailasa")
        );
        assert_eq!(
            tibetan_font_fallbacks_for_target_os("linux")
                .first()
                .map(String::as_str),
            Some("Noto Serif Tibetan")
        );
        assert_eq!(
            tibetan_font_fallbacks_for_target_os("unknown")
                .first()
                .map(String::as_str),
            Some("Noto Serif Tibetan")
        );
    }

    #[test]
    fn nested_quote_separator_row_keeps_outer_group_gap_collapsed() {
        let group = Uuid::new_v4();
        let gap = rendered_row_top_gap(
            Some(RenderedRowSpacingInfo {
                quote_group_anchor: Some(group),
                ..RenderedRowSpacingInfo::default()
            }),
            RenderedRowSpacingInfo {
                quote_group_anchor: Some(group),
                ..RenderedRowSpacingInfo::default()
            },
            4.0,
            true,
        );
        assert_eq!(gap, 0.0);
    }

    #[test]
    fn distinct_quote_groups_keep_default_gap() {
        let gap = rendered_row_top_gap(
            Some(RenderedRowSpacingInfo {
                quote_group_anchor: Some(Uuid::new_v4()),
                ..RenderedRowSpacingInfo::default()
            }),
            RenderedRowSpacingInfo {
                quote_group_anchor: Some(Uuid::new_v4()),
                ..RenderedRowSpacingInfo::default()
            },
            4.0,
            true,
        );
        assert_eq!(gap, 4.0);
    }

    #[test]
    fn non_quote_rows_keep_default_gap() {
        let gap = rendered_row_top_gap(
            Some(RenderedRowSpacingInfo {
                quote_group_anchor: None,
                ..RenderedRowSpacingInfo::default()
            }),
            RenderedRowSpacingInfo {
                quote_group_anchor: Some(Uuid::new_v4()),
                ..RenderedRowSpacingInfo::default()
            },
            4.0,
            true,
        );
        assert_eq!(gap, 4.0);
    }

    #[test]
    fn rendered_headings_and_lists_have_distinct_vertical_rhythm() {
        let paragraph = RenderedRowSpacingInfo::default();
        let heading = RenderedRowSpacingInfo {
            heading_level: Some(1),
            ..paragraph
        };
        let list_item = RenderedRowSpacingInfo {
            is_list_item: true,
            ..paragraph
        };
        assert_eq!(
            rendered_row_top_gap(Some(paragraph), heading, 8.0, true),
            19.2
        );
        assert_eq!(
            rendered_row_top_gap(Some(heading), paragraph, 8.0, true),
            6.0
        );
        assert_eq!(
            rendered_row_top_gap(Some(list_item), list_item, 8.0, true),
            4.0
        );
        assert_eq!(
            rendered_row_top_gap(Some(paragraph), heading, 8.0, false),
            8.0
        );
    }

    #[test]
    fn callout_inner_spacing_uses_header_and_body_tokens() {
        let theme = Theme::default_theme();
        let dimensions = &theme.dimensions;

        let header_gap = callout_row_top_gap(
            Some(RenderedRowSpacingInfo {
                is_callout_header: true,
                ..RenderedRowSpacingInfo::default()
            }),
            RenderedRowSpacingInfo::default(),
            dimensions,
        );
        let body_gap = callout_row_top_gap(
            Some(RenderedRowSpacingInfo {
                is_callout_header: false,
                ..RenderedRowSpacingInfo::default()
            }),
            RenderedRowSpacingInfo::default(),
            dimensions,
        );

        assert_eq!(header_gap, dimensions.callout_header_margin_bottom);
        assert_eq!(body_gap, dimensions.callout_body_gap);
    }

    #[test]
    fn nested_quote_rows_inside_callout_collapse_body_gap() {
        let theme = Theme::default_theme();
        let dimensions = &theme.dimensions;
        let group = Uuid::new_v4();

        let gap = callout_row_top_gap(
            Some(RenderedRowSpacingInfo {
                is_callout_header: false,
                visible_quote_group_anchor: Some(group),
                ..RenderedRowSpacingInfo::default()
            }),
            RenderedRowSpacingInfo {
                visible_quote_group_anchor: Some(group),
                ..RenderedRowSpacingInfo::default()
            },
            dimensions,
        );

        assert_eq!(gap, 0.0);
    }

    #[test]
    fn menu_button_width_expands_for_long_ascii_labels() {
        let theme = Theme::default_theme();
        let dimensions = &theme.dimensions;

        assert_eq!(
            menu_bar_button_width("文件", dimensions),
            dimensions.menu_bar_button_width
        );
        assert!(menu_bar_button_width("Language", dimensions) > dimensions.menu_bar_button_width);
    }

    #[test]
    fn in_window_menu_is_enabled_for_every_target_except_macos() {
        for target_os in [
            "windows",
            "linux",
            "freebsd",
            "openbsd",
            "netbsd",
            "dragonfly",
            "solaris",
            "illumos",
            "android",
            "unknown",
        ] {
            assert!(
                supports_in_window_menu_for_target_os(target_os),
                "{target_os} should use the in-window fallback menu"
            );
        }
        assert!(!supports_in_window_menu_for_target_os("macos"));
    }

    #[test]
    fn windows_moves_the_menu_row_into_the_titlebar() {
        // 菜单栏那一行只在 Linux/FreeBSD 这类没有系统菜单栏的桌面保留；
        // Windows 改成标题栏里的汉堡按钮。
        assert!(supports_menu_bar_row_for_target_os("linux"));
        assert!(!supports_menu_bar_row_for_target_os("windows"));
        assert!(!supports_menu_bar_row_for_target_os("macos"));

        assert!(supports_hamburger_menu_for_target_os("windows"));
        assert!(!supports_hamburger_menu_for_target_os("linux"));
        assert!(!supports_hamburger_menu_for_target_os("macos"));
    }

    #[test]
    fn hamburger_item_panel_aligns_with_the_hovered_row() {
        let dimensions = Theme::default_theme().dimensions;
        let labels = vec!["File".to_string(), "Export".to_string()];
        let titlebar_height = 34.0;

        let origin = hamburger_menu_item_panel_origin(1, titlebar_height, &labels, &dimensions);        let list_width = menu_panel_width_for_labels(&labels, &dimensions);
        assert_eq!(
            origin.panel_left,
            dimensions.menu_bar_padding_x + list_width + dimensions.menu_panel_gap
        );

        // 条目面板第一行的 y 必须跟列表里被划过的第 1 行重合。
        let first_item_row_top =
            origin.panel_top + dimensions.menu_panel_top + dimensions.menu_panel_padding;
        assert_eq!(
            first_item_row_top,
            hamburger_menu_row_top(1, titlebar_height, &dimensions)
        );
        // 行越往下越高，且第二行比第一行低一行的高度。
        let row_delta = hamburger_menu_row_top(1, titlebar_height, &dimensions)
            - hamburger_menu_row_top(0, titlebar_height, &dimensions);
        assert_eq!(row_delta, dimensions.menu_item_height + dimensions.menu_panel_gap);
    }

    #[test]
    fn hamburger_list_hangs_right_below_the_button() {
        let dimensions = Theme::default_theme().dimensions;
        let titlebar_height = 36.0;
        let panel_top = hamburger_menu_panel_top(titlebar_height, &dimensions);
        let button_bottom = (titlebar_height + dimensions.menu_bar_button_height) / 2.0;

        // 缝就是 menu_bar_gap（默认 2px），不是几十像素。
        assert_eq!(panel_top, button_bottom + dimensions.menu_bar_gap);
        assert_eq!(panel_top, 32.0);

        // 与标题栏高度无关：标题栏再高，缝也不会跟着长。
        let tall_panel_top = hamburger_menu_panel_top(titlebar_height + 24.0, &dimensions);
        assert_eq!(
            tall_panel_top - panel_top,
            12.0,
            "标题栏每高 24px，面板只下移 12px（缝不变）"
        );

        // 旧写法（标题栏下沿 + menu_panel_top）会空出 30px：确认已经不再用它。
        assert!(dimensions.menu_panel_top > 10.0);
        assert!(panel_top < titlebar_height + dimensions.menu_panel_top);
    }

    #[test]
    fn in_window_menu_height_depends_on_platform_and_menu_presence() {
        let theme = Theme::default_theme();
        let dimensions = &theme.dimensions;

        assert_eq!(
            in_window_menu_bar_height_for_target_os("linux", true, dimensions),
            dimensions.menu_bar_height
        );
        // Windows 不再单占一行菜单栏（改成标题栏里的汉堡按钮）。
        assert_eq!(
            in_window_menu_bar_height_for_target_os("windows", true, dimensions),
            0.0
        );
        assert_eq!(
            in_window_menu_bar_height_for_target_os("linux", false, dimensions),
            0.0
        );
        assert_eq!(
            in_window_menu_bar_height_for_target_os("macos", true, dimensions),
            0.0
        );
    }

    #[test]
    fn menu_panel_left_uses_accumulated_dynamic_button_widths() {
        let theme = Theme::default_theme();
        let dimensions = &theme.dimensions;
        let labels = vec![
            "File".to_string(),
            "Language".to_string(),
            "Theme".to_string(),
            "Help".to_string(),
        ];

        let left = menu_panel_left(2, &labels, dimensions);
        let expected = dimensions.menu_bar_padding_x
            + menu_bar_button_width("File", dimensions)
            + dimensions.menu_bar_gap
            + menu_bar_button_width("Language", dimensions)
            + dimensions.menu_bar_gap;
        let old_fixed_left = dimensions.menu_bar_padding_x
            + 2.0 * (dimensions.menu_bar_button_width + dimensions.menu_bar_gap);

        assert_eq!(left, expected);
        assert!(left > old_fixed_left);
    }

    #[test]
    fn menu_panel_width_expands_for_long_recent_paths() {
        let theme = Theme::default_theme();
        let dimensions = &theme.dimensions;
        let short_labels = vec!["Save".to_string()];
        let long_labels = vec![r"C:\Users\someone\Documents\Very Long Folder\notes.md".to_string()];

        assert_eq!(
            menu_panel_width_for_labels(&short_labels, dimensions),
            dimensions.menu_panel_width
        );
        assert!(
            menu_panel_width_for_labels(&long_labels, dimensions) > dimensions.menu_panel_width
        );
    }

    #[test]
    fn import_menu_split_detects_theme_and_language_import_tails() {
        let theme_items = vec![
            disabled_menu_action("velora"),
            OwnedMenuItem::Separator,
            add_theme_menu_action(),
        ];
        let language_items = vec![
            disabled_menu_action("English"),
            OwnedMenuItem::Separator,
            add_language_menu_action(),
        ];
        let regular_items = vec![
            disabled_menu_action("Open"),
            OwnedMenuItem::Separator,
            disabled_menu_action("Save"),
        ];
        let malformed_import_items = vec![disabled_menu_action("velora"), add_theme_menu_action()];

        assert_eq!(import_menu_split_index(&theme_items), Some(1));
        assert_eq!(import_menu_split_index(&language_items), Some(1));
        assert_eq!(import_menu_split_index(&regular_items), None);
        assert_eq!(import_menu_split_index(&malformed_import_items), None);
    }

    #[test]
    fn scrollable_import_menu_height_caps_visible_items_and_clamps_to_viewport() {
        let theme = Theme::default_theme();
        let dimensions = &theme.dimensions;
        let scroll_items = (0..20)
            .map(|index| disabled_menu_action(&format!("Custom Theme {index}")))
            .collect::<Vec<_>>();
        let footer_items = vec![OwnedMenuItem::Separator, add_theme_menu_action()];
        let expected_large_height =
            menu_items_visual_height_with_gaps(&scroll_items[..12], dimensions);
        let full_scroll_content_height =
            menu_items_visual_height_with_gaps(&scroll_items, dimensions);
        let footer_height = menu_items_visual_height_with_gaps(&footer_items, dimensions);

        let large_height = scrollable_import_menu_scroll_height(
            &scroll_items,
            &footer_items,
            2000.0,
            0.0,
            dimensions,
        );
        let small_height = scrollable_import_menu_scroll_height(
            &scroll_items,
            &footer_items,
            180.0,
            0.0,
            dimensions,
        );

        assert!((large_height - expected_large_height).abs() < f32::EPSILON);
        assert!(full_scroll_content_height > large_height);
        assert!(large_height < expected_large_height + footer_height);
        assert!(small_height < large_height);
        assert!(small_height >= dimensions.menu_item_height);
    }

    #[test]
    fn submenu_bridge_spans_parent_child_menu_gap() {
        let theme = Theme::default_theme();
        let dimensions = &theme.dimensions;
        let labels = vec!["File".to_string()];
        let items = vec![
            OwnedMenuItem::Separator,
            OwnedMenuItem::Submenu(OwnedMenu {
                name: "Recent".into(),
                items: vec![OwnedMenuItem::Action {
                    name: r"C:\Users\someone\Documents\notes.md".into(),
                    action: Box::new(NoRecentFiles),
                    os_action: None,
                }],
            }),
        ];
        let submenu_labels = match &items[1] {
            OwnedMenuItem::Submenu(submenu) => owned_menu_item_labels(&submenu.items),
            _ => Vec::new(),
        };

        let bridge = submenu_bridge_geometry(
            menu_panel_left(0, &labels, dimensions),
            &items,
            1,
            &submenu_labels,
            dimensions,
        )
            .expect("submenu bridge geometry should be available");
        let submenu_width = menu_panel_width_for_labels(&submenu_labels, dimensions);

        assert_eq!(
            bridge.left,
            dimensions.menu_bar_padding_x + dimensions.menu_panel_width
        );
        assert_eq!(bridge.width, dimensions.menu_panel_gap + submenu_width);
        assert!(bridge.height > dimensions.menu_item_height);
        let item_top = dimensions.menu_panel_top
            + dimensions.menu_panel_padding
            + dimensions.menu_separator_height
            + dimensions.menu_separator_margin_y * 2.0
            + dimensions.menu_panel_gap;
        assert!(bridge.top < item_top);
        assert!(bridge.top >= dimensions.menu_panel_top);
    }

    #[test]
    fn submenu_bridge_uses_dynamic_main_menu_width() {
        let theme = Theme::default_theme();
        let dimensions = &theme.dimensions;
        let labels = vec!["File".to_string()];
        let items = vec![OwnedMenuItem::Submenu(OwnedMenu {
            name: "Open Recently Used Markdown File".into(),
            items: vec![OwnedMenuItem::Action {
                name: r"C:\Users\someone\Documents\Very Long Folder\notes.md".into(),
                action: Box::new(NoRecentFiles),
                os_action: None,
            }],
        })];
        let submenu_labels = match &items[0] {
            OwnedMenuItem::Submenu(submenu) => owned_menu_item_labels(&submenu.items),
            _ => Vec::new(),
        };

        let bridge = submenu_bridge_geometry(
            menu_panel_left(0, &labels, dimensions),
            &items,
            0,
            &submenu_labels,
            dimensions,
        )
            .expect("submenu bridge geometry should be available");

        assert!(bridge.left > dimensions.menu_bar_padding_x + dimensions.menu_panel_width);
        assert!(bridge.width > dimensions.menu_panel_gap + dimensions.menu_panel_width);
    }
