mod tests {
    use super::super::{contains_tibetan_text, css_color, prepare_print_html, render_chromium_pdf_html_with_base_dir,
        render_html, render_html_with_base_dir};
    use crate::config::preferences::{read_app_preferences_with_dirs, save_app_preferences_with_dirs};
    use crate::config::{ExportThemePreference, VeloraConfigDirs};
    use crate::export::{resolve_export_theme, resolve_export_theme_choice};
    use crate::theme::Theme;
    use std::fs;
    use uuid::Uuid;

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
        assert!(html.contains("<h1>Title</h1>"));
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
        assert!(html.contains("<h1>A &amp; B</h1>"));
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
        assert!(html.contains("<h1>Body</h1>"));
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
}
