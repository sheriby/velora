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

    /// 应用内跳转算出的锚点，与导出写进 HTML 的那个 `id`，必须是同一个字符串。
    ///
    /// 两处现在共用 `export::html::heading_slug` 这一份。这条断言防的是「有人再抄
    /// 第二份」：抄了不会立刻错，错在以后只改一边 —— 表现是界面里 Ctrl+点跳得动、
    /// 导出的 HTML 里同一条链接点不动（或反过来），两边都「看着自洽」而没人报警。
    /// 所以断的是**导出真发出来的 id** 拿到应用内来跳转找不找得到那一行，
    /// 而不是让两边各自跟同一个函数比——那种断言同义反复，永远绿。
    #[test]
    fn the_anchor_the_app_jumps_to_is_the_id_the_export_writes() {
        let source = "# 部署步骤 Guide 🎉\n\nbody\n\n## Setup, fast!\n\nmore\n";
        let html = crate::export::html::render_html(
            source,
            &crate::theme::Theme::default_theme(),
            "锚点",
        );
        let exported_ids = html
            .lines()
            .filter(|line| line.contains("<h1 ") || line.contains("<h2 "))
            .map(|line| {
                let after = line.split("id=\"").nth(1).expect("导出的标题带 id");
                after[..after.find('"').expect("id 有右引号")].to_string()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            exported_ids.len(),
            2,
            "导出应当发两个标题 id：{html}"
        );

        let heading_lines = source
            .lines()
            .filter(|line| line.starts_with('#'))
            .collect::<Vec<_>>();
        for id in &exported_ids {
            let index = heading_line_for_anchor(source, id)
                .unwrap_or_else(|| panic!("导出的 id {id:?} 在应用内跳不过去"));
            assert!(
                heading_lines.contains(&source.lines().nth(index).expect("行号越界")),
                "id {id:?} 跳到了非标题行 {index}"
            );
        }
        // 反向也一样：应用内认得的锚点，导出确实用了同一个串。
        for line in &heading_lines {
            let text = line.trim_start_matches('#').trim();
            let slug = crate::export::html::heading_slug(text).expect("标题作得出锚点");
            assert!(
                exported_ids.contains(&slug),
                "应用内算出的 {slug:?} 没被导出使用：{exported_ids:?}"
            );
        }
    }
}
