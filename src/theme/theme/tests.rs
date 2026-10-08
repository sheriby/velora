mod tests {
    use crate::theme::{Theme, ThemeManager};
    use crate::config::VeloraConfigDirs;
    use gpui::{Hsla, Rgba, WindowAppearance, rgba};

    #[test]
    fn typography_builtin_headings_have_the_same_readable_hierarchy() {
        for theme in [
            Theme::default_theme(),
            Theme::light_theme(),
            Theme::paper_theme(),
            Theme::forest_theme(),
            Theme::midnight_theme(),
            Theme::ink_theme(),
        ] {
            let typography = &theme.typography;
            assert!(
                typography.h1_size >= typography.text_size * 2.0,
                "{} 的 H1 层级太弱",
                theme.name
            );
            assert!(
                typography.h2_size >= typography.text_size * 1.65,
                "{} 的 H2 层级太弱",
                theme.name
            );
            assert!(
                typography.h3_size >= typography.text_size * 1.35,
                "{} 的 H3 层级太弱",
                theme.name
            );
            assert!(typography.h1_size > typography.h2_size && typography.h2_size > typography.h3_size);
            assert_eq!(
                theme.dimensions.h1_border_width, 0.0,
                "标题用留白和字号区分，不画下划线"
            );
        }
    }

    #[test]
    fn current_line_highlight_stays_transparent_and_theme_tinted() {
        // 用户报修：深色默认主题的当前行高亮曾是 94% 不透明白，直接盖住正文。
        // 高亮必须低透明度，且内置色板主题取自 selection 淡色（跟主题走）。
        let themes = [
            ("default", Theme::default_theme()),
            ("light", Theme::light_theme()),
            ("paper", Theme::paper_theme()),
            ("forest", Theme::forest_theme()),
            ("midnight", Theme::midnight_theme()),
            ("ink", Theme::ink_theme()),
        ];
        for (name, theme) in themes {
            let alpha = theme.colors.current_line_bg.a;
            assert!(
                alpha <= 0.45,
                "主题 {name} 的当前行高亮 alpha={alpha:.2} 过高：接近不透明会盖住正文"
            );
        }

        let line = Rgba::from(Theme::forest_theme().colors.current_line_bg);
        assert!(
            line.g > line.r && line.g > line.b,
            "forest 当前行高亮应为 selection 淡绿系，实际 {line:?}"
        );
    }

    #[test]
    fn md_syntax_colors_fall_back_for_legacy_theme_json() {
        // 旧主题 JSON 没有 md_syntax_* 键：反序列化按 Dark+ 值兜底，不能
        // 让自定义主题用户拿到黑色或空白。
        let default_json = Theme::default_theme()
            .to_json()
            .expect("default theme should serialize");
        let parsed: serde_json::Value =
            serde_json::from_str(&default_json).expect("default theme json should parse");
        let mut object = parsed
            .as_object()
            .expect("theme should serialize to a json object")
            .clone();
        let colors = object
            .get_mut("colors")
            .and_then(|colors| colors.as_object_mut())
            .expect("theme should include colors");
        for key in [
            "md_syntax_heading1",
            "md_syntax_heading2",
            "md_syntax_heading3",
            "md_syntax_heading4",
            "md_syntax_heading5",
            "md_syntax_heading6",
            "md_syntax_strong",
            "md_syntax_marker",
            "md_syntax_emphasis_marker",
            "md_syntax_code",
            "md_syntax_link_text",
            "md_syntax_link_url",
            "md_syntax_label",
        ] {
            colors.remove(key);
        }
        let json = serde_json::to_string(&object).expect("theme json should serialize");

        let theme = Theme::from_json(&json).expect("theme without md_syntax_* should deserialize");
        assert_eq!(
            theme.colors.md_syntax_heading1,
            Hsla::from(rgba(0x569cd6ff)),
            "缺省的 md_syntax_heading1 按 Dark+ 标题蓝兜底"
        );
        assert_eq!(
            theme.colors.md_syntax_strong,
            Hsla::from(rgba(0xe5c07bff)),
            "缺省的 md_syntax_strong 按 Dark+ 金色兜底"
        );
        assert_eq!(theme.colors.md_syntax_label, Hsla::from(rgba(0xc586c0ff)));
    }

    #[test]
    fn theme_token_documentation_covers_every_token() {
        // roadmap H3：主题 token 全表文档化；结构体新增字段必须同步 docs/主题变量.md。
        let doc = include_str!("../../../docs/主题变量.md");
        let json = Theme::default_theme().to_json().expect("theme json");
        let value: serde_json::Value = serde_json::from_str(&json).expect("parse theme json");

        let mut documented = std::collections::BTreeSet::new();
        for line in doc.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("| `")
                && let Some((token, _)) = rest.split_once("`")
            {
                documented.insert(token.to_string());
            }
        }

        let mut missing = Vec::new();
        for section in ["colors", "dimensions", "typography", "placeholders"] {
            let Some(map) = value.get(section).and_then(|section| section.as_object()) else {
                continue;
            };
            for key in map.keys() {
                if !documented.contains(key) {
                    missing.push(format!("{section}.{key}"));
                }
            }
        }
        assert!(missing.is_empty(), "主题文档缺少 token：{missing:?}");

        let mut unknown = Vec::new();
        for token in &documented {
            let known = ["colors", "dimensions", "typography", "placeholders"]
                .iter()
                .any(|section| {
                    value
                        .get(section)
                        .and_then(|section| section.get(token))
                        .is_some()
                });
            if !known {
                unknown.push(token.clone());
            }
        }
        assert!(unknown.is_empty(), "主题文档包含不存在的 token：{unknown:?}");
    }

    #[test]
    fn system_theme_tracks_window_appearance_changes() {
        let mut manager = ThemeManager::default();
        manager.set_system_appearance(WindowAppearance::Dark);
        assert!(manager.set_theme_by_id("system"));
        assert_eq!(manager.current_theme_id(), "system");
        assert_eq!(
            manager.current().colors.editor_background,
            Theme::default_theme().colors.editor_background
        );

        manager.set_system_appearance(WindowAppearance::Light);
        assert_eq!(
            manager.current().colors.editor_background,
            Theme::light_theme().colors.editor_background
        );

        assert!(manager.set_theme_by_id("velora-dark"));
        manager.set_system_appearance(WindowAppearance::Dark);
        assert_eq!(
            manager.current().colors.editor_background,
            Theme::default_theme().colors.editor_background
        );
    }

    #[test]
    fn deserializes_legacy_block_focused_bg_key() {
        let default_json = Theme::default_theme()
            .to_json()
            .expect("default theme should serialize");
        let legacy_json = default_json.replace("source_mode_block_bg", "block_focused_bg");

        let theme = Theme::from_json(&legacy_json).expect("legacy theme should deserialize");
        assert!(theme.colors.source_mode_block_bg.a > 0.0);
    }

    #[test]
    fn old_theme_files_default_to_system_fonts() {
        let mut value = serde_json::to_value(Theme::default_theme()).unwrap();
        let typography = value["typography"].as_object_mut().unwrap();
        typography.remove("body_font_family");
        typography.remove("heading_font_family");
        value["dimensions"]
            .as_object_mut()
            .unwrap()
            .remove("writing_max_width");
        let theme: Theme = serde_json::from_value(value).unwrap();
        assert_eq!(theme.typography.body_font_family, ".SystemUIFont");
        assert_eq!(theme.typography.heading_font_family, ".SystemUIFont");
        assert_eq!(theme.dimensions.writing_max_width, 760.0);
    }

    #[test]
    fn border_h2_falls_back_when_omitted() {
        let default_json = Theme::default_theme()
            .to_json()
            .expect("default theme should serialize");
        let parsed: serde_json::Value =
            serde_json::from_str(&default_json).expect("default theme json should parse");
        let mut object = parsed
            .as_object()
            .expect("theme should serialize to a json object")
            .clone();
        object
            .get_mut("colors")
            .and_then(|colors| colors.as_object_mut())
            .expect("theme should include colors")
            .remove("border_h2");
        let json = serde_json::to_string(&object).expect("theme json should serialize");

        let theme = Theme::from_json(&json).expect("theme without border_h2 should deserialize");
        assert_eq!(theme.colors.border_h2, rgba(0xe0e0e0cc).into());
    }

    #[test]
    fn comment_background_falls_back_when_omitted() {
        let default_json = Theme::default_theme()
            .to_json()
            .expect("default theme should serialize");
        let parsed: serde_json::Value =
            serde_json::from_str(&default_json).expect("default theme json should parse");
        let mut object = parsed
            .as_object()
            .expect("theme should serialize to a json object")
            .clone();
        object
            .get_mut("colors")
            .and_then(|colors| colors.as_object_mut())
            .expect("theme should include colors")
            .remove("comment_bg");
        let json = serde_json::to_string(&object).expect("theme json should serialize");

        let theme = Theme::from_json(&json).expect("theme without comment_bg should deserialize");
        assert_eq!(theme.colors.comment_bg, rgba(0xfbbf2426).into());
    }

    #[test]
    fn default_theme_json_omits_dialog_badge_and_strings_tokens() {
        let default_json = Theme::default_theme()
            .to_json()
            .expect("default theme should serialize");
        let parsed: serde_json::Value =
            serde_json::from_str(&default_json).expect("default theme json should parse");

        assert!(parsed.get("strings").is_none());

        let colors = parsed
            .get("colors")
            .and_then(|colors| colors.as_object())
            .expect("theme should include colors");
        assert!(!colors.contains_key(&format!("dialog_{}", "badge_bg")));
        assert!(!colors.contains_key(&format!("dialog_{}", "badge_text")));

        let dimensions = parsed
            .get("dimensions")
            .and_then(|dimensions| dimensions.as_object())
            .expect("theme should include dimensions");
        assert!(!dimensions.contains_key(&format!("dialog_{}", "badge_padding_x")));
        assert!(!dimensions.contains_key(&format!("dialog_{}", "badge_padding_y")));
    }

    #[test]
    fn legacy_theme_json_with_strings_still_loads() {
        let default_json = Theme::default_theme()
            .to_json()
            .expect("default theme should serialize");
        let parsed: serde_json::Value =
            serde_json::from_str(&default_json).expect("default theme json should parse");
        let mut object = parsed
            .as_object()
            .expect("theme should serialize to a json object")
            .clone();
        object.insert(
            "strings".into(),
            serde_json::json!({
                "menu_file": "Legacy File",
                "menu_language": "Legacy Language"
            }),
        );
        let json = serde_json::to_string(&object).expect("theme json should serialize");

        Theme::from_json(&json).expect("legacy theme strings should be ignored safely");
    }

    #[test]
    fn callout_dimensions_fall_back_when_omitted() {
        let default_json = Theme::default_theme()
            .to_json()
            .expect("default theme should serialize");
        let parsed: serde_json::Value =
            serde_json::from_str(&default_json).expect("default theme json should parse");
        let mut object = parsed
            .as_object()
            .expect("theme should serialize to a json object")
            .clone();
        let dimensions = object
            .get_mut("dimensions")
            .and_then(|dimensions| dimensions.as_object_mut())
            .expect("theme should include dimensions");
        dimensions.remove("callout_padding_x");
        dimensions.remove("callout_padding_y");
        dimensions.remove("callout_body_gap");
        dimensions.remove("callout_radius");
        dimensions.remove("callout_border_width");
        dimensions.remove("callout_header_gap");
        dimensions.remove("callout_header_margin_bottom");
        let json = serde_json::to_string(&object).expect("theme json should serialize");

        let theme = Theme::from_json(&json).expect("theme without callout dimensions should load");
        assert_eq!(theme.dimensions.callout_padding_x, 14.0);
        assert_eq!(theme.dimensions.callout_padding_y, 10.0);
        assert_eq!(theme.dimensions.callout_body_gap, 8.0);
        assert_eq!(theme.dimensions.callout_radius, 10.0);
        assert_eq!(theme.dimensions.callout_border_width, 4.0);
        assert_eq!(theme.dimensions.callout_header_gap, 6.0);
        assert_eq!(theme.dimensions.callout_header_margin_bottom, 6.0);
    }

    #[test]
    fn footnote_tokens_fall_back_when_omitted() {
        let default_json = Theme::default_theme()
            .to_json()
            .expect("default theme should serialize");
        let parsed: serde_json::Value =
            serde_json::from_str(&default_json).expect("default theme json should parse");
        let mut object = parsed
            .as_object()
            .expect("theme should serialize to a json object")
            .clone();

        let colors = object
            .get_mut("colors")
            .and_then(|colors| colors.as_object_mut())
            .expect("theme should include colors");
        colors.remove("footnote_bg");
        colors.remove("footnote_border");
        colors.remove("footnote_badge_bg");
        colors.remove("footnote_badge_text");
        colors.remove("footnote_backref");

        let dimensions = object
            .get_mut("dimensions")
            .and_then(|dimensions| dimensions.as_object_mut())
            .expect("theme should include dimensions");
        dimensions.remove("footnote_padding_x");
        dimensions.remove("footnote_padding_y");
        dimensions.remove("footnote_radius");
        dimensions.remove("footnote_badge_padding_x");
        dimensions.remove("footnote_badge_padding_y");

        let json = serde_json::to_string(&object).expect("theme json should serialize");
        let theme = Theme::from_json(&json).expect("theme without footnote tokens should load");

        assert_eq!(theme.colors.footnote_bg, rgba(0x292929ff).into());
        assert_eq!(theme.colors.footnote_border, rgba(0x48464452).into());
        assert_eq!(theme.colors.footnote_badge_bg, rgba(0x3b3a3924).into());
        assert_eq!(theme.colors.footnote_badge_text, rgba(0xd6d6d6ff).into());
        assert_eq!(theme.colors.footnote_backref, rgba(0x75beffff).into());
        assert_eq!(theme.dimensions.footnote_padding_x, 10.0);
        assert_eq!(theme.dimensions.footnote_padding_y, 6.0);
        assert_eq!(theme.dimensions.footnote_radius, 6.0);
        assert_eq!(theme.dimensions.footnote_badge_padding_x, 4.0);
        assert_eq!(theme.dimensions.footnote_badge_padding_y, 1.0);
    }

    #[test]
    fn code_language_palette_tokens_fall_back_when_omitted() {
        let default_json = Theme::default_theme()
            .to_json()
            .expect("default theme should serialize");
        let parsed: serde_json::Value =
            serde_json::from_str(&default_json).expect("default theme json should parse");
        let mut object = parsed
            .as_object()
            .expect("theme should serialize to a json object")
            .clone();

        let colors = object
            .get_mut("colors")
            .and_then(|colors| colors.as_object_mut())
            .expect("theme should include colors");
        colors.remove("code_bg");
        colors.remove("code_language_input_bg");
        colors.remove("code_language_input_border");
        colors.remove("code_language_input_text");
        colors.remove("code_language_input_placeholder");

        let json = serde_json::to_string(&object).expect("theme json should serialize");
        let theme =
            Theme::from_json(&json).expect("theme without code language palette should load");

        assert_eq!(theme.colors.code_bg, rgba(0x252832ff).into());
        assert_eq!(theme.colors.code_language_input_bg, rgba(0x333333ff).into());
        assert_eq!(
            theme.colors.code_language_input_border,
            rgba(0x484644ff).into()
        );
        assert_eq!(
            theme.colors.code_language_input_text,
            rgba(0xf5f5f5ff).into()
        );
        assert_eq!(
            theme.colors.code_language_input_placeholder,
            rgba(0x9c9c9cff).into()
        );
    }

    #[test]
    fn important_callout_defaults_use_purple_palette() {
        let theme = Theme::default_theme();
        assert_eq!(theme.colors.callout_important_bg, rgba(0xa78bfa1f).into());
        assert_eq!(
            theme.colors.callout_important_border,
            rgba(0xa78bfaff).into()
        );
        assert_eq!(theme.dimensions.block_gap, 6.0);
        assert_eq!(theme.colors.footnote_bg, rgba(0x292929ff).into());
        assert_eq!(theme.dimensions.footnote_padding_x, 10.0);
        assert_eq!(theme.colors.code_bg, rgba(0x252832ff).into());
        assert_eq!(theme.colors.code_language_input_bg, rgba(0x333333ff).into());
        assert_eq!(
            theme.colors.code_language_input_border,
            rgba(0x484644ff).into()
        );
    }

    #[test]
    fn light_theme_uses_light_palette_without_changing_layout_tokens() {
        let dark = Theme::default_theme();
        let light = Theme::light_theme();

        assert_eq!(light.name, "Velora Light");
        assert_eq!(light.colors.editor_background, rgba(0xffffffff).into());
        assert_eq!(light.colors.text_default, rgba(0x252832ff).into());
        assert_eq!(light.colors.text_link, rgba(0x6558d3ff).into());
        assert_eq!(light.colors.code_bg, rgba(0xf2f3f6ff).into());
        assert_eq!(
            light.colors.code_language_input_border,
            rgba(0xd1d1d1ff).into()
        );
        assert_eq!(
            light.colors.table_cell_active_outline,
            rgba(0x6558d3ff).into()
        );
        assert_eq!(light.dimensions.block_gap, dark.dimensions.block_gap);
        assert_eq!(light.typography.text_size, dark.typography.text_size);
    }

    #[test]
    fn menu_dimension_tokens_fall_back_when_omitted() {
        let default_json = Theme::default_theme()
            .to_json()
            .expect("default theme should serialize");
        let parsed: serde_json::Value =
            serde_json::from_str(&default_json).expect("default theme json should parse");
        let mut object = parsed
            .as_object()
            .expect("theme should serialize to a json object")
            .clone();

        let dimensions = object
            .get_mut("dimensions")
            .and_then(|dimensions| dimensions.as_object_mut())
            .expect("theme should include dimensions");
        dimensions.remove("menu_bar_height");
        dimensions.remove("menu_item_height");
        dimensions.remove("context_menu_panel_width");
        dimensions.remove("table_insert_dialog_width");
        dimensions.remove("view_mode_toggle_min_width");
        dimensions.remove("view_mode_toggle_text_size");

        let json = serde_json::to_string(&object).expect("theme json should serialize");
        let theme = Theme::from_json(&json).expect("theme without menu tokens should load");

        assert_eq!(theme.dimensions.menu_bar_height, 32.0);
        assert_eq!(theme.dimensions.menu_item_height, 28.0);
        assert_eq!(theme.dimensions.context_menu_panel_width, 132.0);
        assert_eq!(theme.dimensions.table_insert_dialog_width, 380.0);
        assert_eq!(theme.dimensions.view_mode_toggle_min_width, 88.0);
        assert_eq!(theme.dimensions.view_mode_toggle_text_size, 11.0);
    }

    #[test]
    fn imports_partial_jsonc_theme_and_persists_normalized_json() {
        let root = std::env::temp_dir().join(format!("velora-theme-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("temp root should be created");
        let source = root.join("theme.jsonc");
        std::fs::write(
            &source,
            r#"{
                // Required metadata.
                "name": "Night Writer",
                "creator": "Ada",
                "description": "",
                "theme": {
                    "dimensions": {
                        "block_gap": 12.0,
                        "menu_text_size": null
                    },
                    "placeholders": {
                        "empty_editing": ""
                    }
                }
            }"#,
        )
        .expect("theme config should be written");

        let dirs = VeloraConfigDirs::from_root(&root);
        let mut manager = ThemeManager::default();
        let imported_id = manager
            .import_theme_config_with_dirs(&source, &dirs)
            .expect("theme config should import");

        assert_eq!(manager.current_theme_id(), imported_id);
        assert_eq!(manager.current().name, "Night Writer");
        assert_eq!(
            manager.current().colors.editor_background,
            Theme::default_theme().colors.editor_background
        );
        assert_eq!(manager.current().dimensions.block_gap, 12.0);
        assert_eq!(manager.current().dimensions.menu_text_size, 12.0);
        assert!(
            manager
                .available_themes()
                .iter()
                .any(|entry| { entry.id == imported_id && entry.name == "Night Writer - Ada" })
        );

        let normalized = std::fs::read_to_string(dirs.themes_dir().join("Night_Writer_Ada.json"))
            .expect("normalized theme config should exist");
        assert!(normalized.contains("\"name\": \"Night Writer\""));
        assert!(normalized.contains("\"creator\": \"Ada\""));
        assert!(normalized.contains("\"base_theme_id\": \"velora-dark\""));
        assert!(normalized.contains("\"block_gap\": 12.0"));
        assert!(!normalized.contains("menu_text_size"));
        assert!(!normalized.contains("empty_editing"));
        assert!(!normalized.contains("description"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn custom_theme_pack_can_inherit_light_base() {
        let value = serde_json::json!({
            "name": "Day Writer",
            "creator": "Ada",
            "base_theme_id": "velora-light",
            "theme": {
                "dimensions": {
                    "menu_panel_radius": 12.0
                },
                "colors": {
                    "text_link": null
                }
            }
        });

        let (entry, normalized) =
            crate::theme::custom_theme_from_value(value).expect("theme should import");
        let light = Theme::light_theme();

        assert_eq!(entry.base_theme_id, "velora-light");
        assert_eq!(
            entry.theme.colors.editor_background,
            light.colors.editor_background
        );
        assert_eq!(entry.theme.colors.text_default, light.colors.text_default);
        assert_eq!(entry.theme.colors.text_link, light.colors.text_link);
        assert_eq!(entry.theme.dimensions.menu_panel_radius, 12.0);
        assert_eq!(
            normalized
                .get("base_theme_id")
                .and_then(|value| value.as_str()),
            Some("velora-light")
        );
        assert!(
            normalized
                .pointer("/theme/colors")
                .and_then(|value| value.as_object())
                .map(|colors| !colors.contains_key("text_link"))
                .unwrap_or(true)
        );
    }

    #[test]
    fn invalid_custom_theme_base_falls_back_to_dark() {
        let value = serde_json::json!({
            "name": "Broken Base",
            "creator": "Ada",
            "base_theme_id": "missing",
            "theme": {
                "dimensions": {
                    "block_gap": 10.0
                }
            }
        });

        let (entry, normalized) =
            crate::theme::custom_theme_from_value(value).expect("invalid base should not fail import");

        assert_eq!(entry.base_theme_id, "velora-dark");
        assert_eq!(
            entry.theme.colors.editor_background,
            Theme::default_theme().colors.editor_background
        );
        assert_eq!(
            normalized
                .get("base_theme_id")
                .and_then(|value| value.as_str()),
            Some("velora-dark")
        );
    }

    #[test]
    fn importing_without_base_uses_current_builtin_theme_as_base() {
        let root =
            std::env::temp_dir().join(format!("velora-light-theme-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("temp root should be created");
        let source = root.join("theme.jsonc");
        std::fs::write(
            &source,
            r#"{
                "name": "Light Radius",
                "creator": "Ada",
                "theme": {
                    "dimensions": {
                        "menu_panel_radius": 14.0
                    }
                }
            }"#,
        )
        .expect("theme config should be written");

        let dirs = VeloraConfigDirs::from_root(&root);
        let mut manager = ThemeManager::default();
        assert!(manager.set_theme_by_id("velora-light"));
        let imported_id = manager
            .import_theme_config_with_dirs(&source, &dirs)
            .expect("theme config should import");

        assert_eq!(manager.current_theme_id(), imported_id);
        assert_eq!(
            manager.current().colors.editor_background,
            Theme::light_theme().colors.editor_background
        );
        assert_eq!(manager.current().dimensions.menu_panel_radius, 14.0);

        let normalized = std::fs::read_to_string(dirs.themes_dir().join("Light_Radius_Ada.json"))
            .expect("normalized theme config should exist");
        assert!(normalized.contains("\"base_theme_id\": \"velora-light\""));

        let mut reloaded = ThemeManager::default();
        reloaded
            .load_custom_themes_from_dirs(&dirs)
            .expect("saved theme should reload");
        assert!(reloaded.set_theme_by_id(&imported_id));
        assert_eq!(
            reloaded.current().colors.editor_background,
            Theme::light_theme().colors.editor_background
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn theme_manager_switches_builtin_themes() {
        let mut manager = ThemeManager::default();
        assert_eq!(manager.current_theme_id(), "velora-dark");
        assert_eq!(manager.current().name, "Velora Dark");
        assert_eq!(
            manager
                .available_themes()
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            vec![
                "System",
                "Velora Dark",
                "Velora Light",
                "Paper",
                "Forest",
                "Midnight",
                "Ink",
            ]
        );

        assert!(manager.set_theme_by_id("velora-light"));
        assert_eq!(manager.current_theme_id(), "velora-light");
        assert_eq!(manager.current().name, "Velora Light");
        assert_eq!(
            manager.current().colors.editor_background,
            rgba(0xffffffff).into()
        );

        assert!(manager.set_theme_by_id("velora-dark"));
        assert_eq!(manager.current_theme_id(), "velora-dark");
        assert_eq!(manager.current().name, "Velora Dark");
        for (id, name) in [
            ("paper", "Paper"),
            ("forest", "Forest"),
            ("midnight", "Midnight"),
            ("ink", "Ink"),
        ] {
            assert!(manager.set_theme_by_id(id));
            assert_eq!(manager.current_theme_id(), id);
            assert_eq!(manager.current().name, name);
        }
        assert!(!manager.set_theme_by_id("missing"));
    }

    #[test]
    fn builtin_writing_styles_have_distinct_surfaces_and_rhythm() {
        let themes = [
            Theme::light_theme(),
            Theme::paper_theme(),
            Theme::forest_theme(),
            Theme::default_theme(),
            Theme::midnight_theme(),
            Theme::ink_theme(),
        ];
        for (index, theme) in themes.iter().enumerate() {
            assert!(theme.typography.text_line_height >= 1.6);
            assert!(theme.dimensions.block_gap >= 6.0);
            assert_ne!(theme.colors.text_default, theme.colors.editor_background);
            for previous in &themes[..index] {
                assert_ne!(
                    theme.colors.editor_background,
                    previous.colors.editor_background
                );
            }
        }
        assert_eq!(Theme::paper_theme().dimensions.writing_max_width, 700.0);
        assert_eq!(Theme::ink_theme().dimensions.writing_max_width, 720.0);
    }

    #[test]
    fn custom_theme_can_inherit_the_paper_style() {
        let value = serde_json::json!({
            "name": "Paper Variant",
            "creator": "Test",
            "base_theme_id": "paper",
            "theme": { "typography": { "h1_size": 35.0 } }
        });
        let (entry, _) = crate::theme::custom_theme_from_value(value).unwrap();
        assert_eq!(entry.base_theme_id, "paper");
        assert_eq!(
            entry.theme.colors.editor_background,
            Theme::paper_theme().colors.editor_background
        );
        assert_eq!(entry.theme.typography.h1_size, 35.0);
    }
}
