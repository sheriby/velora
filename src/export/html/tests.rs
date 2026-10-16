mod tests {
    use super::super::{
        IMAGE_MIME_BY_EXTENSION, contains_tibetan_text, css_color, local_image_data_uri,
        prepare_print_html, render_chromium_pdf_html_with_base_dir, render_html,
        render_html_with_base_dir,
    };
    use crate::config::preferences::{read_app_preferences_with_dirs, save_app_preferences_with_dirs};
    use crate::config::{ExportThemePreference, VeloraConfigDirs};
    use crate::export::{resolve_export_theme, resolve_export_theme_choice};
    use crate::theme::Theme;
    use std::fs;
    use uuid::Uuid;

    #[test]
    fn core_repair_inline_html_styles_keep_their_contents_and_nested_markdown() {
        let mut failures = Vec::new();
        for (source, expected) in [
            ("测试<strong>加粗：</strong>测试", "<strong>加粗：</strong>"),
            ("text <strong><em>nested</em></strong> end", "<strong><em>nested</em></strong>"),
            ("text <em>italic **bold** [link](https://example.com)</em> end", "<em>italic <strong>bold</strong> <a href=\"https://example.com\">link</a></em>"),
            ("# text <strong>heading</strong>", "<strong>heading</strong>"),
            ("| text <strong>cell</strong> |\n| --- |\n| value |", "<strong>cell</strong>"),
        ] {
            let html = render_html(source, &Theme::default_theme(), "行内格式");
            if !body_only(&html).contains(expected) { failures.push((source, body_only(&html).to_string())); }
        }
        let html = render_html("text <strong>$x$</strong> end", &Theme::default_theme(), "公式");
        let body = body_only(&html);
        if !body.contains("<strong><span class=\"vlt-inline-math\"") || !body.contains("</span></strong>") {
            failures.push(("公式", body.to_string()));
        }
        assert!(failures.is_empty(), "行内标签不能在内容之前自动闭合：{failures:?}");
    }

    #[test]
    fn core_repair_inline_html_sanitization_blocks_dangerous_attributes_and_scope_leaks() {
        for source in [
            "text <strong onclick=\"alert(1)\">danger</strong> end",
            "text <a href=\"java&#x73;cript:alert(1)\">danger</a> end",
            "# <img src=\"javascript:alert(1)\">",
        ] {
            let html = render_html(source, &Theme::default_theme(), "净化");
            let body = body_only(&html);
            assert!(!body.contains("<strong onclick="), "{body}");
            assert!(!body.contains("href=\"javascript:"), "{body}");
            assert!(!body.contains("src=\"javascript:"), "{body}");
        }
        let html = render_html("text <strong>open\n\nnext", &Theme::default_theme(), "范围");
        assert!(body_only(&html).contains("<p>next</p>"));
        let html = render_html("text <strong><em>nested</strong> end\n\nnext", &Theme::default_theme(), "错配");
        assert!(body_only(&html).contains("<p>next</p>"));
    }


    /// 没配上对的行内标签在导出里也必须按字面。
    ///
    /// 阅读视图（`inline/parse.rs:parse_inline_html_container` → `locate_matching_inline_html_close`）
    /// 配不上对就整段按字面（`html_like_tags_are_not_autolinks_when_unsafe_or_unclosed`），
    /// 而 6f7253a 把导出改成浏览器语义——块尾按栈自动补闭标签。同一句语法于是两面
    /// 分叉：界面显示字面 `text <strong>open`，导出把后面的字染成粗体。契约取阅读视图
    /// 那一侧：导出是「所见」的另一种载体，不该凭空补出用户没写的作用域。

    /// 缩进四格的 `$$…$$` 与顶格的一样是公式——与阅读视图同一判据。
    ///
    /// 块模型认「任意缩进的 `$$` 块」是公式（Typora 粘过来的文档常这样写），pulldown 先
    /// 把它读成缩进代码块：不接住的话，同一段 `$$` 界面是公式、导出是
    /// `<pre><code>$$…$$</code></pre>`（评审 E1）。
    #[test]
    fn review_indented_display_math_exports_as_math_like_the_reading_view() {
        for source in ["$$x^2$$", "  $$x^2$$", "    $$x^2$$", "\t$$x^2$$"] {
            let html = render_html(source, &Theme::default_theme(), "公式");
            assert!(
                body_only(&html).contains("class=\"vlt-math\""),
                "{source:?} 该按公式导出：{}",
                body_only(&html)
            );
        }
        // 围栏代码块里的 `$$` 仍是代码（用户明说是代码）。
        let html = render_html("```text\n$$x$$\n```", &Theme::default_theme(), "公式");
        assert!(
            body_only(&html).contains("language-text"),
            "{}",
            body_only(&html)
        );
    }

    #[test]
    fn review_indented_display_math_keeps_text_after_the_closing_fence() {
        // cases/03-formula-tail.md 的缩进形态：旧实现取「第一个公式」视图而不是分段表，
        // 闭合 `$$` 之后的正文与同块第二条公式在导出里凭空消失。
        let html = render_html("    $$x^2$$ LOST_SENTINEL", &Theme::default_theme(), "公式");
        let body = body_only(&html);
        assert!(body.contains("class=\"vlt-math\""), "{body}");
        assert!(
            body.contains("LOST_SENTINEL"),
            "缩进公式的尾文被吞掉了：{body}"
        );
        assert!(!body.contains("$$"), "定界符不该留在导出里：{body}");

        let html = render_html("    $$a^2$$ $$b^2$$", &Theme::default_theme(), "公式");
        let body = body_only(&html);
        assert_eq!(
            body.matches("class=\"vlt-math\"").count(),
            2,
            "一块里的第二条公式不该消失：{body}"
        );
    }

    #[test]
    fn review_unclosed_inline_html_exports_literally_like_the_reading_view() {
        let html = render_html("text <strong>open\n\nnext", &Theme::default_theme(), "范围");
        let body = body_only(&html);
        assert!(
            !body.contains("<strong>open"),
            "没配对的 <strong> 又被当成作用域了：{body}"
        );
        assert!(body.contains("&lt;strong&gt;open"), "该按字面显示：{body}");
        assert!(body.contains("<p>next</p>"), "下一段不受影响：{body}");

        // 配对的那一半照旧：样式与内容都在，下一段也不受影响。
        let html = render_html("text <strong>bold</strong> end", &Theme::default_theme(), "范围");
        assert!(body_only(&html).contains("<strong>bold</strong>"));

        // 错配：`<strong>` 配到了闭标签、里面的 `<em>` 没有 → 只有 `em` 退字面。
        let html = render_html("text <strong><em>nested</strong> end\n\nnext", &Theme::default_theme(), "范围");
        let body = body_only(&html);
        assert!(body.contains("<strong>&lt;em&gt;nested</strong>"), "{body}");
        assert!(body.contains("<p>next</p>"), "{body}");

        // 没有配对的**闭合**标签：界面原样显示那几个字符，导出却把它整段丢掉。
        for (source, escaped) in [
            ("text </em> tail", "&lt;/em&gt;"),
            ("a </strong> b", "&lt;/strong&gt;"),
            ("没有开标签 </code> 收尾", "&lt;/code&gt;"),
        ] {
            let html = render_html(source, &Theme::default_theme(), "闭标签");
            let body = body_only(&html);
            assert!(
                body.contains(escaped),
                "{source:?} 里没配对的闭合标签该原样输出：{body}"
            );
        }
    }

    #[test]
    fn review_export_recognizes_math_next_to_escaped_dollar() {
        for source in ["$x\\$$", "\\$$x$"] {
            let html = render_html(source, &Theme::default_theme(), "转义");
            assert!(body_only(&html).contains("class=\"vlt-inline-math\""), "{source}");
        }
    }

    #[test]
    fn review_export_downgrades_unsafe_url_schemes() {
        // Markdown 链接曾绕过 HTML 属性的协议清洗，分享物携带可执行 href。
        for source in [
            "[危险](javascript:alert(1))", "[危险](JaVaScRiPt:alert(1))",
            "[危险][r]\n\n[r]: javascript:alert(1)", "<javascript:alert(1)>",
            "[危险](data:text/html,evil)", "[危险](vbscript:msgbox(1))",
            "[危险](file:///tmp/secret.md)", "[危险](java&#x73;cript:alert(1))",
            "![危险](javascript:alert(1))", "![危险](data:image/svg+xml,evil)",
        ] {
            let html = render_html(source, &Theme::default_theme(), "安全");
            let body = body_only(&html);
            assert!(!body.contains("<a href="), "{source}: {body}");
            assert!(!body.contains("<img "), "{source}: {body}");
        }
        for destination in ["https://example.com/?q=javascript:example", "mailto:a@example.com", "tel:123", "ftp://example.com/a", "relative.md#标题", "#标题"] {
            let html = render_html(&format!("[合法]({destination})"), &Theme::default_theme(), "安全");
            assert!(body_only(&html).contains("<a href="));
        }
    }

    #[test]
    fn review_export_does_not_embed_images_outside_document_directory() {
        // 绝对路径、编码后的上级路径及符号链接曾把目录外文件烘进分享物。
        let root = std::env::temp_dir().join(format!("velora-image-boundary-{}", Uuid::new_v4()));
        let notes = root.join("notes"); fs::create_dir_all(&notes).unwrap();
        let secret = root.join("secret.png"); fs::write(&secret, b"PRIVATE_BYTE_PAYLOAD").unwrap();
        let sources = vec![secret.to_string_lossy().into_owned(), "%2e%2e%2fsecret.png".into()];
        #[cfg(unix)]
        let sources = {
            std::os::unix::fs::symlink(&secret, notes.join("escape.png")).unwrap();
            let mut sources = sources;
            sources.push("escape.png".into());
            sources
        };
        for source in sources {
            assert!(local_image_data_uri(&source, Some(&notes)).is_none(), "{source}");
        }
        fs::write(notes.join("allowed.png"), b"ALLOWED").unwrap();
        assert!(local_image_data_uri("allowed.png", Some(&notes)).is_some());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn review_export_refuses_oversized_local_image_payload() {
        // 内联前没有字节上限，稀疏大文件同样会被完整读入并编码。
        let root = std::env::temp_dir().join(format!("velora-image-limit-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("large.png");
        fs::File::create(&path).unwrap().set_len(50 * 1024 * 1024 + 1).unwrap();
        assert!(local_image_data_uri("large.png", Some(&root)).is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn review_double_dollars_inside_prose_stay_literal() {
        // 段落中的双美元符不打开块公式；导出曾独立接受该定界符。
        let source = "其中 $$x>0$$ 成立";
        let html = render_html(source, &Theme::default_theme(), "公式");
        assert!(!body_only(&html).contains("<svg"));
        assert!(body_only(&html).contains("$$x&gt;0$$"));
        let tree = crate::components::InlineTextTree::from_markdown(source);
        assert_eq!(tree.visible_text(), source);
        assert!(tree.render_cache().spans().iter().all(|span| span.math.is_none()));
    }

    #[test]
    fn review_export_display_math_matches_application_svg_size() {
        // 导出曾直接使用正文字号，漏掉阅读视图使用的公式比例。
        let theme = Theme::default_theme();
        let expected = crate::components::render_latex_to_svg(
            "x^2", theme.colors.text_default,
            crate::components::display_math_font_size(theme.typography.text_size),
            crate::components::latex::MathLayout::Display,
        ).unwrap();
        let html = render_html("$$x^2$$", &theme, "字号");
        assert!(body_only(&html).contains(expected.split('>').next().unwrap()));
    }

    #[test]
    fn review_highlight_whitespace_delimiters_stay_literal() {
        // 导出曾接受空白旁的闭合符，与阅读视图的高亮判据不同。
        for source in ["== x ==", "==x ==", "== x=="] {
            let html = render_html(source, &Theme::default_theme(), "高亮");
            assert!(!html.split("<body>").nth(1).unwrap().contains("<mark>"));
        }
    }

    #[test]
    fn export_theme_dark_preference_renders_dark_background_token() {
        // roadmap F3：config.toml 里 `[export] theme = "dark"` 时，导出 HTML
        // 使用内置深色主题的背景色 token，即使当前应用主题是浅色。
        let root = std::env::temp_dir().join(format!("velora-export-theme-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).expect("create temp config dir");
        let dirs = VeloraConfigDirs::from_root(&root);
        fs::write(
            dirs.app_config_file(),
            "[export]\ntheme = \"dark\"\n",
        )
        .expect("write config.toml");

        let preferences = read_app_preferences_with_dirs(&dirs).expect("read preferences");
        assert_eq!(preferences.export_theme, ExportThemePreference::Dark);

        let theme = resolve_export_theme_choice(&Theme::light_theme(), preferences.export_theme);
        let html = render_html("# Title\n\ntext", &theme, "Doc");

        assert!(html.contains("color-scheme: dark;"));
        assert!(html.contains(&format!(
            "--vlt-bg: {};",
            css_color(Theme::default_theme().colors.editor_background)
        )));
        assert!(!html.contains(&format!(
            "--vlt-bg: {};",
            css_color(Theme::light_theme().colors.editor_background)
        )));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn export_theme_light_preference_renders_light_background_token() {
        let theme = resolve_export_theme_choice(&Theme::default_theme(), ExportThemePreference::Light);
        let html = render_html("# Title\n\ntext", &theme, "Doc");

        assert!(html.contains("color-scheme: light;"));
        assert!(html.contains(&format!(
            "--vlt-bg: {};",
            css_color(Theme::light_theme().colors.editor_background)
        )));
    }

    #[test]
    fn export_theme_default_keeps_current_theme_output() {
        // 默认 current 行为与现状一致：沿用调用方传入的当前主题。
        assert_eq!(
            ExportThemePreference::default(),
            ExportThemePreference::Current
        );
        let current = Theme::light_theme();
        let resolved = resolve_export_theme_choice(&current, ExportThemePreference::Current);

        assert_eq!(
            render_html("# Title\n\ntext", &resolved, "Doc"),
            render_html("# Title\n\ntext", &current, "Doc")
        );
        assert_eq!(resolved.colors.editor_background, current.colors.editor_background);
    }

    #[test]
    fn resolve_export_theme_reads_configured_preference() {
        // 无配置文件时回退为「当前主题」。
        let current = Theme::light_theme();
        assert_eq!(
            resolve_export_theme(&current).colors.editor_background,
            current.colors.editor_background
        );
    }

    #[test]
    fn export_theme_preference_round_trips_through_config_file() {
        let root = std::env::temp_dir().join(format!("velora-export-theme-{}", Uuid::new_v4()));
        let dirs = VeloraConfigDirs::from_root(&root);
        let preferences = crate::config::preferences::AppPreferences {
            export_theme: ExportThemePreference::Light,
            ..Default::default()
        };

        save_app_preferences_with_dirs(&preferences, &dirs).expect("save preferences");
        let text = fs::read_to_string(dirs.app_config_file()).expect("read config.toml");
        assert!(text.contains("[export]"));
        assert!(text.contains("theme = \"light\""));
        assert_eq!(
            read_app_preferences_with_dirs(&dirs).expect("read preferences"),
            preferences
        );

        fs::write(dirs.app_config_file(), "[export]\ntheme = \"neon\"\n")
            .expect("write invalid theme");
        assert_eq!(
            read_app_preferences_with_dirs(&dirs)
                .expect("read preferences")
                .export_theme,
            ExportThemePreference::Current
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn renders_complete_html_document_with_theme_css() {
        let html = render_html("# Title\n\ntext", &Theme::default_theme(), "Doc");

        assert!(html.starts_with("<!doctype html>"));
        assert!(html.contains("<html lang=\"en\">"));
        assert!(html.contains("<title>Doc</title>"));
        assert!(html.contains("<style>"));
        assert!(html.contains("--vlt-bg:"));
        assert!(html.contains("<main class=\"vlt-document\">"));
        assert!(html.contains("<h1 id=\"title\">Title</h1>"), "actual: {html}");
        assert!(html.contains("<p>text</p>"));
    }
    #[test]
    fn detects_tibetan_text_for_document_language() {
        assert!(contains_tibetan_text("\u{0f56}\u{0f7c}\u{0f51}"));
        assert!(!contains_tibetan_text("Chinese text"));
    }

    #[test]
    fn exports_tibetan_with_language_and_font_fallbacks() {
        let markdown = concat!(
            "\u{0f56}\u{0f7c}\u{0f51}\u{0f0b}\u{0f61}\u{0f72}\u{0f42}",
            " ",
            "\u{0f56}\u{0f7c}\u{0f51}\u{0f0b}\u{0f61}\u{0f72}\u{0f42} "
        );
        let html = render_html(markdown, &Theme::default_theme(), "Doc");

        assert!(html.contains("<html lang=\"bo\">"));
        assert!(html.contains("\u{0f56}\u{0f7c}\u{0f51}"));
        assert!(html.contains("\u{0f61}\u{0f72}\u{0f42}"));
        assert!(html.contains("\"Noto Serif Tibetan\""));
        assert!(html.contains("\"Microsoft Himalaya\""));
    }
    #[test]
    fn emits_pdf_compatible_theme_css() {
        let theme = Theme::default_theme();
        let html = render_html("# Title\n\ntext", &theme, "Doc");

        assert!(!html.contains("hsla("));
        assert!(html.contains("color-scheme: dark;"));
        assert!(html.contains(&format!(
            "--vlt-bg: {};",
            css_color(theme.colors.editor_background)
        )));
        assert!(html.contains("html { background-color: var(--vlt-bg); color: var(--vlt-text); }"));
        assert!(html.contains("background-color: var(--vlt-code-bg);"));
        assert!(html.contains("border: 1px solid;\n  border-color: var(--vlt-border);"));
        assert!(html.contains(
            "blockquote.markdown-alert-note { background-color: var(--vlt-callout-note-bg); border-color: var(--vlt-callout-note-border); }"
        ));
        assert!(!html.contains("background: var("));
        assert!(!html.contains("border-left-color:"));
    }

    #[test]
    fn light_theme_exports_light_color_scheme() {
        let theme = Theme::light_theme();
        let html = render_html("# Title\n\ntext", &theme, "Doc");

        assert!(html.contains("color-scheme: light;"));
        assert!(html.contains(&format!(
            "--vlt-bg: {};",
            css_color(theme.colors.editor_background)
        )));
        assert!(!html.contains("color-scheme: dark;"));
    }

    #[test]
    fn chromium_pdf_light_theme_clears_print_container_frames() {
        let html = render_chromium_pdf_html_with_base_dir(
            "# Title\n\ntext",
            &Theme::light_theme(),
            "Doc",
            None,
        );

        assert!(html.contains("color-scheme: light;"));
        assert!(html.contains("background-color: var(--vlt-bg);"));
        assert!(html.contains("border: 0;"));
        assert!(html.contains("outline: 0;"));
        assert!(html.contains("box-shadow: none;"));
        assert!(!html.contains("color-scheme: dark;"));
    }

    #[test]
    fn enables_extended_markdown_features() {
        let markdown = "> [!NOTE]\n> body\n\n| A | B |\n| - | - |\n| 1 | 2 |\n\n- [x] done\n\n~~old~~\n\nhello[^a]\n\n[^a]: footnote";
        let html = render_html(markdown, &Theme::default_theme(), "Doc");

        assert!(html.contains("markdown-alert-note"));
        assert!(html.contains("<table>"));
        assert!(html.contains("checked"));
        assert!(html.contains("<del>old</del>"));
        assert!(html.contains("footnote"));
    }

    #[test]
    fn renders_html_comment_blocks_as_visible_escaped_text() {
        let markdown = "<!--\n<strong>not html</strong>\n-->";
        let html = render_html(markdown, &Theme::default_theme(), "Doc");

        assert!(html.contains("class=\"vlt-comment\""), "actual: {html}");
        assert!(html.contains("&lt;!--"), "actual: {html}");
        assert!(
            html.contains("&lt;strong&gt;not html&lt;/strong&gt;"),
            "actual: {html}"
        );
        assert!(!html.contains("<!--\n<strong>not html</strong>\n-->"));
    }

    #[test]
    fn does_not_rewrite_comment_markers_inside_fenced_code() {
        let markdown = "```\n<!--\nnot a comment block\n-->\n```";
        let html = render_html(markdown, &Theme::default_theme(), "Doc");

        assert!(!html.contains("class=\"vlt-comment\""));
        assert!(html.contains("&lt;!--"));
        assert!(html.contains("not a comment block"));
    }

    #[test]
    fn escapes_risky_raw_html_blocks_for_export() {
        let html = render_html("<script>alert(1)</script>", &Theme::default_theme(), "Doc");

        assert!(html.contains("class=\"vlt-raw-html\""));
        assert!(html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
        assert!(!html.contains("<script>alert(1)</script>"));
    }

    #[test]
    fn escapes_risky_child_inside_safe_html_for_export() {
        let html = render_html(
            "<div>safe<script>alert(1)</script>tail</div>",
            &Theme::default_theme(),
            "Doc",
        );

        assert!(html.contains("<div>safe"), "actual: {html}");
        assert!(
            html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"),
            "actual: {html}"
        );
        assert!(html.contains("tail</div>"), "actual: {html}");
        assert!(
            !html.contains("<script>alert(1)</script>"),
            "actual: {html}"
        );
    }

    #[test]
    fn sanitizes_safe_html_style_attributes_for_export() {
        let html = render_html(
            "<span style=\"color:blue; background-image:url(javascript:bad); background-color:#ff0; font-size:120%\">x</span>",
            &Theme::default_theme(),
            "Doc",
        );

        assert!(
            html.contains(
                "style=\"color: rgba(0,0,255,1.000); background-color: rgba(255,255,0,1.000); font-size: 120%;\""
            ),
            "actual: {html}"
        );
        assert!(!html.contains("background-image"), "actual: {html}");
    }

    #[test]
    fn escapes_title_and_markdown_body_html() {
        let html = render_html("# A & B", &Theme::default_theme(), "A & <B>");

        assert!(html.contains("<title>A &amp; &lt;B&gt;</title>"));
        assert!(
            html.contains("<h1 id=\"a--b\">A &amp; B</h1>"),
            "actual: {html}"
        );
    }

    #[test]
    fn exports_display_math_as_svg() {
        let html = render_html("$$\n\\frac{1}{2}\n$$", &Theme::default_theme(), "Doc");

        assert!(html.contains("class=\"vlt-math\""));
        assert!(html.contains("<svg"));
        assert!(!html.contains("$$\n\\frac{1}{2}\n$$"));
    }

    #[test]
    fn exports_mermaid_block_as_svg() {
        let html = render_html(
            "```mermaid\nflowchart LR\nA --> B\n```",
            &Theme::default_theme(),
            "Doc",
        );

        assert!(html.contains("class=\"vlt-mermaid\""));
        assert!(html.contains("<img alt=\"Mermaid diagram\""));
        assert!(html.contains("data:image/svg+xml;base64,"));
        assert!(!html.contains("```mermaid\nflowchart LR\nA --&gt; B\n```"));
    }

    #[test]
    fn exports_local_image_as_data_uri_when_base_dir_is_available() {
        let root = std::env::temp_dir().join(format!("velora-html-export-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).expect("create temp export dir");
        fs::write(
            root.join("diagram.svg"),
            "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 1 1\"></svg>",
        )
        .expect("write local image");

        let html = render_html_with_base_dir(
            "![diagram](diagram.svg)",
            &Theme::default_theme(),
            "Doc",
            Some(&root),
        );
        let _ = fs::remove_dir_all(&root);

        assert!(html.contains("data:image/svg+xml;base64,"));
        assert!(!html.contains("src=\"diagram.svg\""));
    }

    #[test]
    fn exports_standalone_html_image_with_sanitized_zoom() {
        let root = std::env::temp_dir().join(format!("velora-html-export-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).expect("create temp export dir");
        fs::write(root.join("diagram.png"), [137, 80, 78, 71]).expect("write local image");

        let html = render_html_with_base_dir(
            "<img src=\"diagram.png\" alt=\"diagram\" style=\"color:red; zoom:80%; width:10px\" />",
            &Theme::default_theme(),
            "Doc",
            Some(&root),
        );
        let _ = fs::remove_dir_all(&root);

        assert!(html.contains("<img src=\"data:image/png;base64,"));
        assert!(html.contains("alt=\"diagram\""));
        assert!(html.contains("style=\"zoom: 80%;\""));
        assert!(!html.contains("color:red"));
        assert!(!html.contains("width:10px"));
    }

    #[test]
    fn export_keeps_missing_local_image_path() {
        let root = std::env::temp_dir().join(format!("velora-html-export-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).expect("create temp export dir");

        let html = render_html_with_base_dir(
            "![diagram](missing.png)",
            &Theme::default_theme(),
            "Doc",
            Some(&root),
        );
        let _ = fs::remove_dir_all(&root);

        assert!(html.contains("src=\"missing.png\""));
        assert!(!html.contains("data:image/png;base64,"));
    }

    #[test]
    fn exports_inline_math_as_svg() {
        let html = render_html("before $x^2$ after", &Theme::default_theme(), "Doc");

        assert!(html.contains("class=\"vlt-inline-math\""));
        assert!(html.contains("<svg"));
        assert!(!html.contains("$x^2$"));
        assert!(html.contains("before"));
        assert!(html.contains("after"));
    }

    #[test]
    fn export_inline_math_ignores_code_and_escaped_delimiters() {
        let html = render_html("`$x$` and \\$y$", &Theme::default_theme(), "Doc");

        assert!(!html.contains("class=\"vlt-inline-math\""));
        assert!(html.contains("$x$"));
        assert!(html.contains("$y$"));
    }

    #[test]
    fn exports_superscript_and_subscript_as_html_tags() {
        let html = render_html("x^2^ and H~2~O", &Theme::default_theme(), "Doc");

        assert!(html.contains("x<sup>2</sup>"));
        assert!(html.contains("H<sub>2</sub>O"));
    }

    /// `==x==` 的导出：只把两端换成标签，中间留给 pulldown-cmark，所以套在高亮里的
    /// 强调仍然按语法渲染。
    #[test]
    fn exports_highlight_as_mark_tag_and_keeps_nested_emphasis() {
        let html = render_html(
            "==marked== and **==both==**",
            &Theme::default_theme(),
            "Doc",
        );

        assert!(html.contains("<mark>marked</mark>"), "{html}");
        assert!(
            html.contains("<strong><mark>both</mark></strong>"),
            "{html}"
        );
    }

    #[test]
    fn export_highlight_rewrite_ignores_code_escaped_and_unpaired_markers() {
        let html = render_html(
            "`==x==` \\==x\\== and 1 == 2",
            &Theme::default_theme(),
            "Doc",
        );

        assert!(html.contains("<code>==x==</code>"), "{html}");
        // 只在正文里比，样式表里那句注释也写着这两个等号。
        let body = html.split("<body>").nth(1).unwrap_or_default();
        assert!(!body.contains("<mark>"), "转义与没配对的 == 不该被当成高亮：{body}");
    }

    #[test]
    fn export_script_rewrite_ignores_code_escaped_and_strikethrough() {
        let html = render_html(
            "`x^2^ H~2~O` \\^2^ \\~2~ ~~old~~",
            &Theme::default_theme(),
            "Doc",
        );

        assert!(!html.contains("<sup>2</sup>"));
        assert!(!html.contains("<sub>2</sub>"));
        assert!(html.contains("<code>x^2^ H~2~O</code>"));
        assert!(html.contains("^2^"));
        assert!(html.contains("~2~"));
        assert!(html.contains("<del>old</del>"));
    }

    #[test]
    fn exports_frontmatter_as_key_value_table() {
        let source = concat!(
            "---\n",
            "name: triton-ascend-coder\n",
            "temperature: 0.1\n",
            "skills:\n",
            "  - op-task-extractor\n",
            "  - kernel-designer\n",
            "---\n",
            "\n",
            "# Body\n"
        );
        let html = render_html(source, &Theme::default_theme(), "Doc");

        assert!(html.contains("vlt-front-matter"));
        assert!(html.contains("<th>name</th><td>triton-ascend-coder</td>"));
        assert!(html.contains("<th>temperature</th><td>0.1</td>"));
        assert!(html.contains("<li>op-task-extractor</li>"));
        assert!(html.contains("<li>kernel-designer</li>"));
        // 围栏不能漏进 pulldown，否则会被拆成分隔线；正文照常渲染。
        assert!(!html.contains("<hr"));
        assert!(html.contains("<h1 id=\"body\">Body</h1>"), "actual: {html}");
    }

    #[test]
    fn invalid_display_math_exports_escaped_raw_markdown() {
        let html = render_html("$$\n\\frac{a}\n$$", &Theme::default_theme(), "Doc");

        assert!(html.contains("class=\"vlt-math-error\""));
        assert!(html.contains("$$\n\\frac{a}\n$$"));
        assert!(!html.contains("class=\"vlt-math\"><svg"));
    }

    #[test]
    fn invalid_mermaid_exports_escaped_raw_markdown() {
        let html = render_html(
            "```mermaid\nnot a real mermaid diagram ::::\n```",
            &Theme::default_theme(),
            "Doc",
        );

        assert!(html.contains("class=\"vlt-mermaid-error\""));
        assert!(html.contains("not a real mermaid diagram ::::"));
        assert!(!html.contains("data:image/svg+xml;base64,"));
    }

    #[test]
    fn single_file_export_embeds_local_images_as_data_uris() {
        // roadmap F1：本地图片内嵌为 data URI，导出的单文件 HTML 离线可看，
        // 且不再引用本地相对路径。
        let root = std::env::temp_dir().join(format!("velora-export-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("create root");
        let image_path = root.join("banner.png");
        // 最小 PNG 头（8 字节签名 + IHDR 长度），仅用于 base64 内嵌断言。
        std::fs::write(&image_path, [0x89, b'P', b'N', b'G']).expect("write image");

        let markdown = "![banner](./banner.png)";
        let html = render_html_with_base_dir(
            markdown,
            &Theme::default_theme(),
            "export",
            Some(&root),
        );

        assert!(
            html.contains("data:image/png;base64,"),
            "local image should be embedded as a data URI"
        );
        assert!(
            !html.contains("./banner.png"),
            "local relative path should not remain in single-file export"
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn prepare_print_html_injects_page_and_print_overrides() {
        // roadmap F4：打印用 HTML 在导出 HTML 基础上注入 @page 与打印覆盖规则。
        let html = prepare_print_html(
            "<!doctype html>\n<html>\n<head>\n<style>\n.vlt-document { width: min(100% - 48px, 920px); }\n</style>\n</head>\n<body><main class=\"vlt-document\"><h1>Title</h1></main></body>\n</html>\n",
        );

        assert!(html.contains("@page"));
        assert!(html.contains("size: A4"));
        assert!(html.contains("margin: 15mm"));
        assert!(html.contains("@media print"));
        assert!(html.contains("print-color-adjust: exact;"));
        assert!(html.contains("break-inside: avoid;"));
        assert!(html.contains(".vlt-document"));
        assert!(html.contains("<h1>Title</h1>"));
        // 注入位置在样式表内部，且原有屏幕样式保持在前。
        assert!(html.find("@page").expect("page rule") < html.find("</style>").expect("style end"));
        assert!(
            html.find(".vlt-document { width: min(100% - 48px, 920px); }")
                .expect("screen layout")
                < html.find("@page").expect("page rule")
        );
    }

    #[test]
    fn prepare_print_html_without_style_returns_input_unchanged() {
        let html = prepare_print_html("<p>no stylesheet</p>");

        assert_eq!(html, "<p>no stylesheet</p>");
    }

    #[test]
    fn print_html_matches_chromium_pdf_print_rules() {
        // 打印路径与 PDF 导出路径共用同一段打印规则，避免两条路径版式漂移。
        let markdown = "# Title\n\nbody";
        let print_html = prepare_print_html(&render_html(markdown, &Theme::default_theme(), "Doc"));
        let pdf_html = render_chromium_pdf_html_with_base_dir(
            markdown,
            &Theme::default_theme(),
            "Doc",
            None,
        );

        for rule in [
            "@page",
            "size: A4",
            "margin: 15mm",
            "print-color-adjust: exact;",
            "break-inside: avoid;",
        ] {
            assert!(print_html.contains(rule), "print html missing '{rule}'");
            assert!(pdf_html.contains(rule), "pdf html missing '{rule}'");
        }
    }

    // ---- 报修回归：导出必须走 pulldown 事件流后处理，不再预重写原始文本 ----
    // 夹具取自测试者复现包 cases/，断言只看用户可见的输出字节。

    fn body_only(html: &str) -> &str {
        html.split("<body>").nth(1).unwrap_or(html)
    }

    #[test]
    fn exports_display_math_and_keeps_trailing_text() {
        // cases/03-formula-tail.md：`$$x^2$$ LOST_SENTINEL` 曾被整行吞掉尾部文本。
        let html = render_html("$$x^2$$ LOST_SENTINEL", &Theme::default_theme(), "Doc");
        let body = body_only(&html);

        assert!(body.contains("class=\"vlt-math\""), "actual: {html}");
        assert!(body.contains("<svg"), "actual: {html}");
        assert!(body.contains("LOST_SENTINEL"), "actual: {html}");
        assert!(!body.contains("$$"), "actual: {html}");
    }

    #[test]
    fn exports_every_display_math_in_one_paragraph() {
        // `$$x^2$$ $$y^2$$` 曾只保留第一条公式。
        let html = render_html("$$x^2$$ $$y^2$$", &Theme::default_theme(), "Doc");
        let body = body_only(&html);

        assert_eq!(
            body.matches("class=\"vlt-math\"").count(),
            2,
            "两条块级公式都要渲染：{html}"
        );
        assert!(body.contains("<svg"), "actual: {html}");
        assert!(!body.contains("$$"), "actual: {html}");
    }

    #[test]
    fn exports_display_math_with_blank_line() {
        // cases/04-math-blank-line.md：块内空行曾把公式劈成两段字面 `$$`。
        let markdown = "# E05_blank_line_in_display\n\n$$\nx^2\n\n+y^2\n$$\nBLANK_TAIL_SENTINEL";
        let html = render_html(markdown, &Theme::default_theme(), "Doc");
        let body = body_only(&html);

        assert!(body.contains("class=\"vlt-math\""), "actual: {html}");
        assert!(body.contains("<svg"), "actual: {html}");
        assert!(
            body.contains("BLANK_TAIL_SENTINEL"),
            "闭合 `$$` 之后的文字不许丢：{html}"
        );
        assert!(!body.contains("$$"), "actual: {html}");
    }

    #[test]
    fn exports_display_math_closing_on_content_line() {
        // cases/04-math-closing.md：闭合 `$$` 贴在内容行尾（Typora 粘贴写法）。
        let markdown = "$$\\begin{aligned}\nx &= y\n\\end{aligned}$$\nAfter formula.";
        let html = render_html(markdown, &Theme::default_theme(), "Doc");
        let body = body_only(&html);

        assert!(body.contains("class=\"vlt-math\""), "actual: {html}");
        assert!(body.contains("<svg"), "actual: {html}");
        assert!(body.contains("After formula."), "actual: {html}");
        assert!(!body.contains("$$"), "actual: {html}");
        assert!(!body.contains("\\begin{aligned}"), "actual: {html}");
    }

    /// 导出面找闭合 `$$` 必须借应用侧那一处判据（`latex::split_display_math_closing_line`
    /// 按反斜杠游程判奇偶），不许在这里 `find("$$")` 再猜一次：`\$` 是字面美元，不构成
    /// 闭合。`8c7f8ec` 把唯一正本升级成认 `\$` 之后，导出这份裸搜索就成了第二份会漂的
    /// 判据（评审报告维度一 2）。
    ///
    /// 断的是「整段被当成**一个**显示公式块交给公式管线」，不断「渲得出来」——这种写法
    /// 的公式体里必然留着一个 `$$`，ratex 本就读不了，两面都只能落到错误卡；漂开时导出
    /// 会把这行当普通段落原样吐出去（界面是一张公式、分享物是源码字面），那才是要钉的。
    #[test]
    fn exports_display_math_with_backslash_escaped_dollars_inside() {
        let markdown = r"$$a\$$ 和 b$$";
        let html = render_html(markdown, &Theme::default_theme(), "Doc");
        let body = body_only(&html);

        assert!(
            body.contains("vlt-math-error"),
            "整段该被认成一个显示公式块（与 `parse_display_math_segments` 同一判据）；\
             导出却把它当普通段落原样吐出：actual: {body}"
        );
        assert!(
            body.contains(r"$$a\$$ 和 b$$"),
            "公式体要延伸到**真正的**闭合符，整段原文一起进错误卡，不能截在转义美元上；\
             actual: {body}"
        );
    }

    /// `\\( \\)` 那一支同理：开界要认反斜杠游程。`a\\(b\\)` 里 `\\` 是转义出来的字面
    /// 反斜杠、括号是普通字符（内存模型走 `parse_until` 的转义分支就是这个结果），
    /// 导出原先不查，把第二个 `\` 当开界符造出一体为 `b\` 的公式。
    #[test]
    fn keeps_escaped_paren_math_literal_in_export() {
        let markdown = r"路径 a\\(b\\) 结尾";
        let html = render_html(markdown, &Theme::default_theme(), "Doc");
        let body = body_only(&html);

        assert!(
            !body.contains("class=\"vlt-math\""),
            "转义反斜杠之后的 `\\(` 不是公式定界符，不该产出公式；actual: {html}"
        );
        assert!(
            body.contains(r"a\(b\)"),
            "正文该是 `\\\\` 解出来的字面反斜杠加括号（`a\\(b\\)`），不是公式；actual: {body}"
        );
    }

    #[test]
    fn exports_display_math_inside_blockquote() {
        // cases/05-quote-math.md：引用块里的公式曾原样导出 `$$` 源码。
        let markdown = "> $$\n> x^2\n> $$";
        let html = render_html(markdown, &Theme::default_theme(), "Doc");
        let body = body_only(&html);

        assert!(body.contains("<blockquote>"), "actual: {html}");
        assert!(body.contains("class=\"vlt-math\""), "actual: {html}");
        assert!(body.contains("<svg"), "actual: {html}");
        assert!(!body.contains("$$"), "actual: {html}");
    }

    #[test]
    fn exports_inline_math_inside_blockquote_and_list() {
        let markdown = "> quote $x^2$ tail\n\n- item $y^2$ tail";
        let html = render_html(markdown, &Theme::default_theme(), "Doc");
        let body = body_only(&html);

        assert!(body.contains("<blockquote>"), "actual: {html}");
        assert!(body.contains("<li>"), "actual: {html}");
        assert_eq!(body.matches("<svg").count(), 2, "actual: {html}");
        assert!(!body.contains("$x^2$"), "actual: {html}");
        assert!(!body.contains("$y^2$"), "actual: {html}");
    }

    #[test]
    fn exports_display_math_in_nested_blockquote_list() {
        // 「同类新写法不用改代码」验收：嵌套引用+列表里的块级公式。
        let markdown = "> - $$\n>   x^2\n>   $$";
        let html = render_html(markdown, &Theme::default_theme(), "Doc");
        let body = body_only(&html);

        assert!(body.contains("<blockquote>"), "actual: {html}");
        assert!(body.contains("<li>"), "actual: {html}");
        assert!(body.contains("class=\"vlt-math\""), "actual: {html}");
        assert!(!body.contains("$$"), "actual: {html}");
    }

    #[test]
    fn keeps_dollar_signs_in_indented_code() {
        // cases/06-indented-code.md：SVG 曾漏进 <code> 里。
        let markdown = "# E11_indented_code\n\n    print(\"$x^2$\")";
        let html = render_html(markdown, &Theme::default_theme(), "Doc");
        let body = body_only(&html);

        assert!(body.contains("<code>print(\"$x^2$\")</code>"), "actual: {html}");
        assert!(!body.contains("<svg"), "actual: {html}");
        assert!(!body.contains("vlt-inline-math"), "actual: {html}");
    }

    #[test]
    fn keeps_dollar_signs_in_fenced_code() {
        let markdown = "```python\nprint(\"$x^2$\")\n```";
        let html = render_html(markdown, &Theme::default_theme(), "Doc");
        let body = body_only(&html);

        assert!(body.contains("print(\"$x^2$\")"), "actual: {html}");
        assert!(!body.contains("<svg"), "actual: {html}");
    }

    #[test]
    fn keeps_math_like_dollars_in_link_destination() {
        // cases/06-link-url.md：链接 URL 曾被改写导致 `<a href>` 整个消失。
        let markdown = "[doc](https://example.com/$x$)";
        let html = render_html(markdown, &Theme::default_theme(), "Doc");
        let body = body_only(&html);

        assert!(
            body.contains("<a href=\"https://example.com/$x$\">doc</a>"),
            "actual: {html}"
        );
        assert!(!body.contains("<svg"), "actual: {html}");
    }

    #[test]
    fn keeps_setext_headings_as_headings() {
        // cases/13-setext-heading.md：`=` 下划线曾被高亮改写劈成 <mark>。
        let markdown = "Setext heading\n==============\n\nLower heading\n-------------";
        let html = render_html(markdown, &Theme::default_theme(), "Doc");
        let body = body_only(&html);

        assert!(body.contains(">Setext heading</h1>"), "actual: {html}");
        assert!(body.contains(">Lower heading</h2>"), "actual: {html}");
        assert!(!body.contains("<mark>"), "actual: {html}");
        assert!(!body.contains("====="), "actual: {html}");
    }

    #[test]
    fn keeps_horizontal_rule_after_setext_dash() {
        let markdown = "Before\n\n---\n\nAfter";
        let html = render_html(markdown, &Theme::default_theme(), "Doc");
        let body = body_only(&html);

        assert!(body.contains("<hr"), "actual: {html}");
        assert!(!body.contains("<mark>"), "actual: {html}");
    }

    #[test]
    fn exports_heading_ids_and_resolves_internal_anchor() {
        // cases/14-heading-link.md：导出 HTML 里标题没有 id，锚点链接全部失效。
        let markdown = "[Jump to section](#target-section)\n\n## Target Section\n\nTarget body.";
        let html = render_html(markdown, &Theme::default_theme(), "Doc");
        let body = body_only(&html);

        assert!(
            body.contains("<a href=\"#target-section\">Jump to section</a>"),
            "actual: {html}"
        );
        assert!(body.contains("id=\"target-section\""), "actual: {html}");
        assert!(body.contains("<h2 id=\"target-section\">Target Section</h2>"), "actual: {html}");
    }

    #[test]
    fn deduplicates_heading_slugs() {
        let markdown = "# Section\n\ntext\n\n# Section\n\nmore\n\n# Section";
        let html = render_html(markdown, &Theme::default_theme(), "Doc");

        assert!(html.contains("id=\"section\""), "actual: {html}");
        assert!(html.contains("id=\"section-1\""), "actual: {html}");
        assert!(html.contains("id=\"section-2\""), "actual: {html}");
    }

    #[test]
    fn slugifies_cjk_and_emoji_headings() {
        let html = render_html("# 中文标题\n\n# 🚀 Launch", &Theme::default_theme(), "Doc");

        assert!(html.contains("id=\"中文标题\""), "CJK 标题不能塌成空 id：{html}");
        assert!(html.contains("id=\"-launch\""), "actual: {html}");
    }

    #[test]
    fn exports_ideographic_space_heading_separator_as_heading() {
        // `##` 后的分隔符是全角空格（U+3000，中文输入法常见）时导出同样是标题：
        // 导出侧是严格 CommonMark（只认 ASCII 空格与制表符），不归一的话应用里
        // 是标题、分享件里却是「##　标题」原文。
        let markdown = "##\u{3000}中文标题\n\n正文。";
        let html = render_html(markdown, &Theme::default_theme(), "Doc");
        let body = body_only(&html);

        assert!(
            body.contains("<h2 id=\"中文标题\">中文标题</h2>"),
            "全角分隔符标题应导出为标题且带锚点：{body}"
        );
        assert!(!body.contains("##"), "标记不得残留：{body}");
    }

    #[test]
    fn exports_tab_separated_heading_as_heading() {
        // 制表符分隔：pulldown-cmark 原生认开放分隔符的 tab，不需要字节归一；
        // 这里钉住与应用侧同判。闭合井号前是 tab 时这版 pulldown **不剥**
        // （实测内容 `收尾\t##`），应用侧同判保留——两侧一致优先于按 spec 剥。
        let markdown = "##\t中文标题\n\n## 收尾\t##\n\n## 剥掉 ##\n\n正文。";
        let html = render_html(markdown, &Theme::default_theme(), "Doc");
        let body = body_only(&html);

        assert!(
            body.contains("<h2 id=\"中文标题\">中文标题</h2>"),
            "tab 分隔符标题应导出为标题：{body}"
        );
        assert!(
            body.contains(">收尾\t##</h2>"),
            "tab 前的闭合井号与 comrak 同判（保留）：{body}"
        );
        assert!(
            body.contains(">剥掉</h2>"),
            "ASCII 空格前的闭合井号照旧剥掉：{body}"
        );
    }

    #[test]
    fn ideographic_space_normalization_only_touches_heading_markers() {
        // 归一化只动标题记号里的三个字节：代码块是字面内容，正文里的 U+3000
        // 是正文字符，都必须原样进导出件。
        let markdown = "```md\n##\u{3000}例子\n```\n\n前言：\u{3000}说明。";
        let html = render_html(markdown, &Theme::default_theme(), "Doc");
        let body = body_only(&html);

        assert!(body.contains("##\u{3000}例子"), "代码块内容不得被改：{body}");
        assert!(body.contains("前言：\u{3000}说明。"), "正文字符不得被改：{body}");
    }

    #[test]
    fn expands_toc_into_linked_heading_list() {
        // cases/14-toc.md：`[TOC]` 曾被原样导出成字面文本。
        let markdown = "# D23_toc\n\n[TOC]\n\n## First section\n\nFirst body.\n\n## Second section\n\nSecond body.";
        let html = render_html(markdown, &Theme::default_theme(), "Doc");
        let body = body_only(&html);

        assert!(!body.contains("[TOC]"), "actual: {html}");
        assert!(
            body.contains("<a href=\"#first-section\">First section</a>"),
            "actual: {html}"
        );
        assert!(
            body.contains("<a href=\"#second-section\">Second section</a>"),
            "actual: {html}"
        );
    }

    #[test]
    fn exports_callout_with_custom_title_as_alert() {
        // cases/15-callout.md：带自定义标题的 callout 曾退化成普通引用块。
        let markdown = "> [!NOTE] Named note\n> Note body with **bold**.";
        let html = render_html(markdown, &Theme::default_theme(), "Doc");
        let body = body_only(&html);

        assert!(body.contains("markdown-alert-note"), "actual: {html}");
        assert!(body.contains("Named note"), "actual: {html}");
        assert!(
            body.contains("<p>Note body with <strong>bold</strong>.</p>"),
            "actual: {html}"
        );
        assert!(body.contains("<strong>bold</strong>"), "actual: {html}");
        assert!(!body.contains("[!NOTE]"), "actual: {html}");
    }

    #[test]
    fn exports_all_alert_types_with_custom_titles() {
        let markdown = concat!(
            "> [!NOTE] a title\n> body\n\n",
            "> [!TIP] b title\n> body\n\n",
            "> [!IMPORTANT] c title\n> body\n\n",
            "> [!WARNING] d title\n> body\n\n",
            "> [!CAUTION] e title\n> body",
        );
        let html = render_html(markdown, &Theme::default_theme(), "Doc");
        let body = body_only(&html);

        for class in [
            "markdown-alert-note",
            "markdown-alert-tip",
            "markdown-alert-important",
            "markdown-alert-warning",
            "markdown-alert-caution",
        ] {
            assert!(body.contains(class), "missing '{class}': {html}");
        }
        for marker in ["[!NOTE]", "[!TIP]", "[!IMPORTANT]", "[!WARNING]", "[!CAUTION]"] {
            assert!(!body.contains(marker), "leaked '{marker}': {html}");
        }
    }

    #[test]
    fn exports_bare_alert_still_uses_alert_markup() {
        let markdown = "> [!WARNING]\n> Warning body.";
        let html = render_html(markdown, &Theme::default_theme(), "Doc");
        let body = body_only(&html);

        assert!(body.contains("markdown-alert-warning"), "actual: {html}");
        assert!(!body.contains("[!WARNING]"), "actual: {html}");
    }

    #[test]
    fn inlines_local_images_with_percent_space_and_angle_paths() {
        // cases/12-images.md：同一个文件的几种写法都要内联。`%20` 写法曾经留成相对
        // 路径（单文件 HTML 少一张图），尖括号写法曾经在阅读视图里 404——两边现在
        // 共用 `resolve_image_source` 一个解析口径。
        let root = std::env::temp_dir().join(format!("velora-html-export-{}", Uuid::new_v4()));
        let assets = root.join("assets");
        fs::create_dir_all(&assets).expect("create assets dir");
        let png_bytes = [0x89u8, b'P', b'N', b'G', 1, 2, 3, 4];
        for name in [
            "orange.png",
            "blue card.png",
            "100% done.png",
            "封面.png",
        ] {
            fs::write(assets.join(name), png_bytes)
                .unwrap_or_else(|error| panic!("write {name}: {error}"));
        }

        let blue_url = url::Url::from_file_path(assets.join("blue card.png"))
            .expect("temp image path should form file URL")
            .to_string();
        // 未转义的空格不是合法的 Markdown 目标，尖括号写法里才允许直接写空格。
        let blue_url_with_space = blue_url.replace("%20", " ");
        let markdown = format!(
            concat!(
                "![orange](assets/orange.png)\n\n",
                "![blue](assets/blue%20card.png)\n\n",
                "![blue angle](<assets/blue card.png>)\n\n",
                "![blue angle encoded](<assets/blue%20card.png>)\n\n",
                "![blue titled](<assets/blue card.png> \"Blue\")\n\n",
                "![literal percent](assets/100%25%20done.png)\n\n",
                "![cjk](assets/封面.png)\n\n",
                "![cjk encoded](assets/%E5%B0%81%E9%9D%A2.png)\n\n",
                "![file url]({blue_url})\n\n",
                "![file url angle](<{blue_url_with_space}>)\n\n",
                "![remote](https://example.com/blue%20card.png)\n",
            ),
            blue_url = blue_url,
            blue_url_with_space = blue_url_with_space,
        );
        let html =
            render_html_with_base_dir(&markdown, &Theme::default_theme(), "Doc", Some(&root));
        fs::remove_dir_all(&root).expect("clean up export fixture dir");

        assert_eq!(
            html.matches("data:image/png;base64,").count(),
            10,
            "十种写法都要内联，实际导出：{html}"
        );
        assert!(!html.contains("src=\"assets"), "actual: {html}");
        assert!(!html.contains("src=\"file:"), "actual: {html}");
        // 远程写法不能被当本地文件读：src 必须原样留着。
        assert!(
            html.contains("src=\"https://example.com/blue%20card.png\""),
            "actual: {html}"
        );
    }

    #[test]
    fn inlines_bare_space_destination_the_reading_view_also_accepts() {
        // 阅读视图的解析器比 CommonMark 宽松，`![x](assets/blue card.png)` 这种没包
        // 尖括号的写法在正文里能显示。导出的解析函数必须给出同一个文件，否则以后
        // 谁在事件流上接这种写法就会静默漏图（报告 12 的根因是两处口径不一致）。
        let root = std::env::temp_dir().join(format!("velora-html-export-{}", Uuid::new_v4()));
        let assets = root.join("assets");
        fs::create_dir_all(&assets).expect("create assets dir");
        fs::write(assets.join("blue card.png"), [0x89u8, b'P', b'N', b'G', 1, 2, 3, 4])
            .expect("write blue card.png");

        for spelling in [
            "assets/blue card.png",
            "assets/blue%20card.png",
            "<assets/blue card.png>",
            "<assets/blue%20card.png>",
        ] {
            let data_uri = local_image_data_uri(spelling, Some(&root));
            assert!(
                data_uri.is_some(),
                "{spelling:?} 应内联成本地图片，实际 {data_uri:?}"
            );
        }

        // 远程与不存在的目标不碰文件系统。
        assert!(local_image_data_uri("https://example.com/blue%20card.png", Some(&root)).is_none());
        assert!(local_image_data_uri("assets/missing.png", Some(&root)).is_none());

        fs::remove_dir_all(&root).expect("clean up export fixture dir");
    }

    #[test]
    fn inlines_every_format_the_app_accepts_as_a_local_image() {
        // 应用能显示的本地图片就是 gpui `Img` 自己报的那份清单；导出内联的格式比它窄，
        // 正文里显示得好好的图就会在共享的单文件 HTML 里留成相对路径（报告 12 同一类：
        // TIFF、AVIF、ICO 都撞到过）。所以这条用例逐条走那份清单，而不是走一张手抄表。
        let root = std::env::temp_dir().join(format!("velora-html-export-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).expect("create temp export dir");
        for extension in gpui::Img::extensions() {
            let name = format!("shot.{extension}");
            fs::write(root.join(&name), b"placeholder bytes").expect("write fixture image");
            match local_image_data_uri(&name, Some(&root)) {
                Some(data_uri) => assert!(
                    data_uri.starts_with("data:image/") && data_uri.contains(";base64,"),
                    "{name} 的 data URI 要带 image MIME，实际 {data_uri}"
                ),
                None => panic!("{name} 是应用能显示的本地图片，导出必须内联成 data URI"),
            }
        }

        // 应用画不出的扩展名不内联，保持原始写法。
        fs::write(root.join("notes.txt"), b"placeholder bytes").expect("write fixture text");
        assert!(
            local_image_data_uri("notes.txt", Some(&root)).is_none(),
            "非图片扩展名不该被内联"
        );

        fs::remove_dir_all(&root).expect("clean up export fixture dir");
    }

    #[test]
    fn image_mime_table_covers_exactly_the_formats_the_app_accepts() {
        // 导出侧只剩一张 MIME 表（gpui 不提供 MIME，data URI 又必须带一个），它的键集
        // 逐条钉在 `gpui::Img::extensions()` 上：任何一边加了扩展名而另一边没跟上，这里
        // 就红，不用等下游报修。MIME 值统一要 `image/` 前缀，否则浏览器不认这张 data URI。
        let app_extensions = gpui::Img::extensions();
        for extension in app_extensions {
            assert!(
                IMAGE_MIME_BY_EXTENSION
                    .iter()
                    .any(|(candidate, _)| candidate == extension),
                "导出 MIME 表缺应用支持的扩展名 {extension:?}"
            );
        }
        for (extension, mime) in IMAGE_MIME_BY_EXTENSION {
            assert!(
                app_extensions.contains(&extension),
                "导出 MIME 表里的 {extension:?} 不是应用支持的图片格式"
            );
            assert!(
                mime.starts_with("image/"),
                "{extension} 的 MIME {mime} 不是 image 类型"
            );
        }
    }

    #[test]
    fn exports_inline_math_adjacent_to_cjk_and_emoji() {
        let html = render_html("公式 $x^2$ 结束", &Theme::default_theme(), "Doc");
        let body = body_only(&html);
        assert!(body.contains("<svg"), "actual: {html}");
        assert!(body.contains("公式"), "actual: {html}");
        assert!(body.contains("结束"), "actual: {html}");
        assert!(!body.contains("$x^2$"), "actual: {html}");

        let html = render_html("🔥 $y^2$ ✨", &Theme::default_theme(), "Doc");
        let body = body_only(&html);
        assert!(body.contains("<svg"), "actual: {html}");
        assert!(body.contains("🔥"), "actual: {html}");
        assert!(body.contains("✨"), "actual: {html}");
        assert!(!body.contains("$y^2$"), "actual: {html}");
    }

    #[test]
    fn exports_display_math_at_block_middle_and_end() {
        let html = render_html(
            "a $$x^2$$ b\n\n$$y^2$$",
            &Theme::default_theme(),
            "Doc",
        );
        let body = body_only(&html);

        assert_eq!(body.matches("<svg").count(), 1, "actual: {html}");
        assert!(body.contains(">a "), "actual: {html}");
        assert!(body.contains(" b"), "actual: {html}");
        assert!(body.contains("class=\"vlt-math\""), "actual: {html}");
        assert!(body.contains("$$x^2$$"), "actual: {html}");
    }

    /// 段首公式闭合后、同段再来一对 `$$`：UI 的分段表（single_line_segments）
    /// 把两段都按块级公式排版；导出侧第二个公式此前掉进行内包装
    /// （`vlt-inline-math` 的 CSS 把 svg 压到 1.8em），同一份文档两个公式
    /// 两种尺寸，带分式的块级公式上下标被压扁。
    #[test]
    fn second_display_formula_mid_paragraph_uses_block_wrapper() {
        let html = render_html(
            "$$x^2$$ 及 $$\\frac{a}{b}$$",
            &Theme::default_theme(),
            "Doc",
        );
        let body = body_only(&html);

        assert_eq!(body.matches("<svg").count(), 2, "actual: {html}");
        assert_eq!(
            body.matches("class=\"vlt-math\"").count(),
            2,
            "两个块级公式都应使用 vlt-math 包装：{html}"
        );
        assert!(!body.contains("vlt-inline-math"), "actual: {html}");
    }

    #[test]
    fn exports_empty_display_math_block_as_math_or_error() {
        let html = render_html("$$\n\n$$", &Theme::default_theme(), "Doc");

        // 空公式块要么渲染成公式要么给错误块，不允许裸 `$$` 泄漏。
        assert!(
            html.contains("class=\"vlt-math\"") || html.contains("vlt-math-error"),
            "actual: {html}"
        );
    }

    #[test]
    fn keeps_unpaired_double_dollar_as_literal() {
        let html = render_html("cost $$ only", &Theme::default_theme(), "Doc");
        let body = body_only(&html);

        assert!(body.contains("$$"), "actual: {html}");
        assert!(!body.contains("<svg"), "actual: {html}");
    }

    #[test]
    fn exports_paren_math_with_numeric_body() {
        // cases/07-numeric-math.md：`\\(42\\)` 是数学，不是货币。
        let html = render_html("value \\(42\\).", &Theme::default_theme(), "Doc");
        let body = body_only(&html);

        assert!(body.contains("<svg"), "actual: {html}");
        assert!(body.contains("value"), "actual: {html}");
        assert!(!body.contains("\\(42\\)"), "actual: {html}");
    }

    #[test]
    fn exports_paren_math_whose_body_looks_like_money() {
        // 报告 7 的导出侧断言：`\(...\)` 是无歧义 TeX 定界符，钱/公式判据一次都不许过问
        // 它。下面这些体内全是数字、`0.50` 与 `1,000` 这串在 `$…$` 写法里会被当钱读。
        // UI 侧同一断言在 `src/components/markdown/inline/tests/edit_ops.rs`。
        for raw in [r"42", r"0.50", r"1,000", r"12_345"] {
            let markdown = format!("value \\({raw}\\) here");
            let html = render_html(&markdown, &Theme::default_theme(), "Doc");
            let body = body_only(&html);

            assert!(body.contains("<svg"), "{markdown:?} 应导出为公式：{html}");
            assert!(
                !body.contains(&format!("\\({raw}\\)")),
                "{markdown:?} 不该被当钱数原样写出：{html}"
            );
        }
    }

    #[test]
    fn app_and_export_make_the_same_money_versus_math_call() {
        // 「这串 `$…$` 是钱还是公式」现在只有一处判据
        // （`src/components/markdown/inline/parse.rs:looks_like_currency_between`）：阅读视图
        // 与导出各自只喂它自己那份上下文（token 序列 / 重建原文加字节偏移）。
        // 这条用例把两个界面逐条对齐——谁再抄一份近似实现、把同一串判成不同的东西，这里就红。
        for markdown in [
            "cost $42$",
            "$42 dollars",
            "total $0.50$ here",
            "price $1,000$ today",
            "$12$",
            "买 3$x^2$ 个",
            "看了$x^2$一眼",
            "value $2x$ here",
            "公式 $x^2$ 结束",
            "$a$ 与 $b$ 两个变量",
            "$x$ = 5",
            "10$ and $20",
            "| Value |\n| --- |\n| $x^2$ |",
            "| Money |\n| --- |\n| $42$ |",
        ] {
            let app_is_math = app_reads_inline_math(markdown);
            let export_is_math = export_reads_inline_math(markdown);
            assert_eq!(
                app_is_math, export_is_math,
                "{markdown:?} 在阅读视图里是公式 {app_is_math}，在导出里是公式 {export_is_math}"
            );
        }
    }

    /// 阅读视图：这串写法里有没有内联公式片段。
    fn app_reads_inline_math(markdown: &str) -> bool {
        crate::components::markdown::inline::InlineTextTree::from_markdown(markdown)
            .render_cache()
            .spans()
            .iter()
            .any(|span| span.math.is_some())
    }

    /// 导出：正文里有没有公式 svg（只看 `<body>` 之后，主题样式表里也有 `svg` 字样）。
    fn export_reads_inline_math(markdown: &str) -> bool {
        body_only(&render_html(markdown, &Theme::default_theme(), "Doc")).contains("<svg")
    }

    #[test]
    fn exports_math_inside_table_cells() {
        // cases/02-table-math.md：表格是内联上下文，没有段落可缓冲，
        // 事件流设计必须在单元格内继续处理行内公式。
        let markdown = "| Value |\n| --- |\n| $x^2$ |";
        let html = render_html(markdown, &Theme::default_theme(), "Doc");
        let body = body_only(&html);

        assert!(body.contains("<svg"), "actual: {html}");
        assert!(!body.contains("$x^2$"), "actual: {html}");
    }

    #[test]
    fn exports_display_math_in_table_cell_without_losing_cell_tags() {
        let markdown = "| Value |\n| --- |\n| $$x^2$$ |";
        let html = render_html(markdown, &Theme::default_theme(), "Doc");
        let body = body_only(&html);

        assert!(body.contains("<td>"), "actual: {html}");
        assert!(body.contains("</td>"), "actual: {html}");
        assert!(body.contains("<svg"), "actual: {html}");
        assert!(!body.contains("$$"), "actual: {html}");
    }

    #[test]
    fn export_does_not_write_absolute_local_image_paths_into_the_shareable_html() {
        // 内联不成的绝对路径图不能原样写进 <img src>：收件人的机器上这张图
        // 本来就打不开，留在分享物里只泄露发送者的目录结构与用户名。能相对化
        // （落在文档目录内）写相对；读不到或越界给不含目录信息的中性占位。
        // 断言只看 <body> 之后：整份文件的 <head> 里内嵌着主题样式表。
        let root = std::env::temp_dir().join(format!("velora-image-privacy-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();

        // 1) 目录外的绝对路径（读不到）：正文写法。
        let outside = "/Users/velora-owner/Desktop/private/leak.png";
        let html = render_html_with_base_dir(
            &format!("![外部]({outside})"),
            &Theme::default_theme(),
            "隐私",
            Some(&root),
        );
        let body = body_only(&html);
        assert!(!body.contains("/Users/"), "绝对路径不得写进分享物：{body}");
        assert!(
            body.contains("data:image/png;base64,"),
            "读不到的绝对路径图应给中性占位：{body}"
        );

        // 2) 目录外的绝对路径：原生 HTML <img> 写法同一条口径。
        let html = render_html_with_base_dir(
            &format!("<img src=\"{outside}\" alt=\"外部\">"),
            &Theme::default_theme(),
            "隐私",
            Some(&root),
        );
        let body = body_only(&html);
        assert!(!body.contains("/Users/"), "原生 HTML 图同样不得泄绝对路径：{body}");

        // 3) 跨平台写法的绝对路径：`C:\…` 在 macOS 上也必须认出是绝对路径
        // （反过来 Windows 上 `/Users/…` 也一样）。判据只看当前平台的
        // `Path::is_absolute` 就会判成相对、原样写进分享物。
        let windows_outside = "C:\\Users\\someone\\Desktop\\private\\leak.png";
        let html = render_html_with_base_dir(
            &format!("![外部]({windows_outside})"),
            &Theme::default_theme(),
            "隐私",
            Some(&root),
        );
        let body = body_only(&html);
        assert!(
            !body.contains("someone") && !body.contains("Users"),
            "跨平台写法的绝对路径不得写进分享物：{body}"
        );
        assert!(
            body.contains("data:image/png;base64,"),
            "跨平台写法的绝对路径图应给中性占位：{body}"
        );
        let html = render_html_with_base_dir(
            &format!("<img src=\"{windows_outside}\" alt=\"外部\">"),
            &Theme::default_theme(),
            "隐私",
            Some(&root),
        );
        let body = body_only(&html);
        // 盘符 src 的 <img> 走不到图片替代分支：`is_safe_export_url` 把 `C:` 读成
        // 未知 scheme，分类阶段（预览与导出同一道门）就落成原文文本块，导出按转义
        // 源码展示。与预览保持一致，因此不占位；也没有 live src 可泄漏。
        assert!(
            body.contains("vlt-raw-html") && body.contains("C:\\Users\\someone"),
            "跨平台绝对路径的原生 HTML 图应保持原文文本块的既有分类：{body}"
        );

        // 4) 目录内但读不到（文件不存在）：相对化成文档目录内的写法。
        let missing = root.join("missing.png");
        let html = render_html_with_base_dir(
            &format!("![缺失]({})", missing.display()),
            &Theme::default_theme(),
            "隐私",
            Some(&root),
        );
        let body = body_only(&html);
        assert!(
            body.contains("src=\"missing.png\""),
            "目录内的绝对路径应相对化：{body}"
        );
        assert!(!body.contains(root.to_string_lossy().as_ref()), "路径本身也不得出现：{body}");

        fs::remove_dir_all(root).unwrap();
    }

    /// 未闭合 `$$` 向后并段不再每段全量重建 recon（评审报告维度四建议 14）：
    /// 旧循环每并一段就对已并区间整体 build_recon 一遍，k 段是 O(k²) 字节
    /// 扫描。新形状：定位闭合时每段只扫自己那段，最终 emit 只重建一次。
    #[test]
    fn unclosed_display_math_merges_with_one_recon_build() {
        let mut markdown = String::from("$$ 公式开头\n\n");
        for index in 0..20 {
            markdown.push_str(&format!("第{index}段正文，足够普通。\n\n"));
        }
        markdown.push_str("公式结尾 $$\n\n收尾段。\n");

        let before = super::super::probe::recon_builds();
        let html = render_html(&markdown, &Theme::default_theme(), "并段");
        let builds = super::super::probe::recon_builds() - before;
        // 修复前实测 25 次（每并一段重建一遍）；修复后只剩各段落常规
        // span 检测的固有重建（实测 3 次），不随并段数增长。
        assert!(
            builds <= 4,
            "并段定位不许全量重建 recon，重建数必须与并段数无关（实际 {builds} 次）"
        );

        let body = body_only(&html);
        // 被并的段落是公式体的一部分（多段块级公式），闭合 `$$` 之后的内容
        // 才按正文输出——「不吞内容」盯的是闭合点之后的正文。
        assert!(
            body.contains("vlt-math") || body.contains("vlt-display-math"),
            "公式本体要在：{body}"
        );
        assert!(
            !body.contains("公式开头") && !body.contains("公式结尾"),
            "公式定界符内的内容归公式，不按正文重复输出：{body}"
        );
        assert!(body.contains("收尾段"), "闭合 $$ 之后的正文不许被吞：{body}");
    }

    /// 段首未闭合 `$$` 与链接目标里的 `$$` 同时出现时，整段不许从导出件里消失。
    ///
    /// 定位闭合点的增量计数会扫过**被 recon 丢弃的 gap**（链接/图片目标的字节），
    /// 而 `emit_merged_display` 仍按 recon 判闭合：两边口径一旦不一致，前者说
    /// 「凑够两个 `$$`」、后者在 recon 里找不到闭合点，于是什么都不发，调用方还把
    /// 这一段当成已消费跳过——整段内容从 HTML / PDF / PNG / 复制为 HTML 里消失。
    /// 旧实现两次判据同源（都 `find("$$")` on recon），这个分支到不了。
    #[test]
    fn a_dollar_pair_inside_a_link_target_never_swallows_the_paragraph() {
        let markdown = "$$ 开头 [链接](b$$c) 结尾\n";
        let html = render_html(markdown, &Theme::default_theme(), "吞段");
        let body = body_only(&html);
        assert!(
            body.contains("开头") && body.contains("结尾"),
            "段首未闭合 $$ 与链接目标里的 $$ 撞上时整段被吞了：{body}"
        );
    }
}
