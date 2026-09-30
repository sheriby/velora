mod tests {
    use super::super::{LinkTarget, classify_link_target, heading_line_for_anchor, resolve_local_link_path};
    use crate::editor::Editor;
    use gpui::TestAppContext;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn link_targets_are_classified_without_asking_the_user() {
        assert_eq!(
            classify_link_target("https://example.com/a?b=1"),
            LinkTarget::External("https://example.com/a?b=1".to_string())
        );
        assert_eq!(
            classify_link_target("mailto:someone@example.com"),
            LinkTarget::External("mailto:someone@example.com".to_string())
        );
        assert_eq!(
            classify_link_target("#设计与来源"),
            LinkTarget::Anchor("设计与来源".to_string())
        );
        assert_eq!(
            classify_link_target("docs/plans/2026-09-24-design.md"),
            LinkTarget::Local {
                path: "docs/plans/2026-09-24-design.md".to_string(),
                anchor: None,
            }
        );
        // 本地路径可以带锚点，百分号转义要还原。
        assert_eq!(
            classify_link_target("./My%20Notes.md#%E6%A0%87%E9%A2%98"),
            LinkTarget::Local {
                path: "./My Notes.md".to_string(),
                anchor: Some("标题".to_string()),
            }
        );
        assert_eq!(
            classify_link_target("/abs/path/other.md"),
            LinkTarget::Local {
                path: "/abs/path/other.md".to_string(),
                anchor: None,
            }
        );
    }

    #[test]
    fn relative_links_resolve_against_the_current_document() {
        let document = PathBuf::from("/work/notes/index.md");
        assert_eq!(
            resolve_local_link_path(Some(&document), "docs/plans/x.md"),
            PathBuf::from("/work/notes/docs/plans/x.md")
        );
        assert_eq!(
            resolve_local_link_path(Some(&document), "/abs/x.md"),
            PathBuf::from("/abs/x.md")
        );
    }

    #[test]
    fn anchors_match_headings_loosely() {
        let source = "# 设计与来源\n\n- 正文\n\n## Math style (extension)\n";
        assert_eq!(heading_line_for_anchor(source, "设计与来源"), Some(0));
        assert_eq!(
            heading_line_for_anchor(source, "math-style-extension"),
            Some(4)
        );
        assert_eq!(heading_line_for_anchor(source, "不存在的标题"), None);
    }

    #[gpui::test]
    async fn external_links_go_to_the_default_browser(cx: &mut TestAppContext) {
        init_app(cx);
        let (editor, cx) = cx.add_window_view(|_, cx| {
            Editor::from_markdown(cx, "看 [官网](https://example.com) 吧\n".into(), None)
        });
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.open_link_target("https://example.com".to_string(), window, cx);
            });
        });
        assert_eq!(
            cx.opened_url().as_deref(),
            Some("https://example.com"),
            "网页链接应直接交给默认浏览器"
        );
    }

    #[gpui::test]
    async fn local_document_links_open_inside_the_app(cx: &mut TestAppContext) {
        init_app(cx);
        let root = std::env::temp_dir().join(format!("velora-link-open-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("docs")).unwrap();
        let current = root.join("index.md");
        let target = root.join("docs").join("target.md");
        fs::write(&current, "# 首页\n\nsee [target](docs/target.md)\n").unwrap();
        fs::write(&target, "# 目标文档\n").unwrap();
        cx.on_quit({
            let root = root.clone();
            move || {
                let _ = fs::remove_dir_all(root);
            }
        });

        let (editor, cx) = cx.add_window_view(|_, cx| {
            Editor::from_markdown(cx, format!("# 首页\n\nsee [target](docs/target.md)\n"), Some(current))
        });
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.open_link_target("docs/target.md".to_string(), window, cx);
            });
        });
        cx.run_until_parked();

        editor.read_with(cx, |editor, _| {
            assert_eq!(
                editor.file_path.as_deref(),
                Some(target.as_path()),
                "本地文档链接应在应用内打开"
            );
            assert!(editor.unsupported_preview_path.is_none());
        });
        assert!(
            cx.opened_url().is_none(),
            "本地文档不该交给浏览器，实测 {:?}",
            cx.opened_url()
        );
    }

    #[gpui::test]
    async fn missing_local_link_changes_nothing(cx: &mut TestAppContext) {
        init_app(cx);
        let root = std::env::temp_dir().join(format!("velora-link-miss-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let current = root.join("index.md");
        fs::write(&current, "# 首页\n").unwrap();
        cx.on_quit({
            let root = root.clone();
            move || {
                let _ = fs::remove_dir_all(root);
            }
        });

        let (editor, cx) =
            cx.add_window_view(|_, cx| Editor::from_markdown(cx, "# 首页\n".into(), Some(current)));
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.open_link_target("gone.md".to_string(), window, cx);
            });
        });
        editor.read_with(cx, |editor, _| {
            assert_eq!(
                editor.file_path.as_deref(),
                Some(root.join("index.md").as_path()),
                "点开到不存在的目标不该改变当前文档"
            );
            assert!(editor.unsupported_preview_path.is_none());
        });
        assert!(cx.opened_url().is_none(), "缺失的本地路径不该丢给浏览器");
    }

    fn init_app(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
        });
    }
}
