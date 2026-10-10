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

        assert_eq!(body.matches("<svg").count(), 2, "actual: {html}");
        assert!(body.contains(">a "), "actual: {html}");
        assert!(body.contains(" b"), "actual: {html}");
        assert!(body.contains("class=\"vlt-math\""), "actual: {html}");
        assert!(!body.contains("$$"), "actual: {html}");
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
}
