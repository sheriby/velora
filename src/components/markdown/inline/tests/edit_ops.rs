    use super::super::{
        InlineFragment, InlineInsertionAttributes, InlineLinkHit, InlineMathDelimiter,
        InlineScript, InlineStyle, InlineTextTree, LinkReferenceDefinitions, StyleFlag,
    };
    use crate::components::HtmlCssColor;


    #[test]
    fn toggle_style_operates_on_selected_slice_only() {
        let mut tree = InlineTextTree::plain("123");
        assert!(tree.toggle_bold(1..3));
        assert_eq!(tree.serialize_markdown(), "1**23**");

        assert!(tree.toggle_bold(2..3));
        assert_eq!(tree.serialize_markdown(), "1**2**3");
    }

    #[test]
    fn replaces_visible_range_and_normalizes_manual_markdown_input() {
        let tree = InlineTextTree::plain(String::new());
        let result =
            tree.replace_visible_range(0..0, "**bold**", InlineInsertionAttributes::default());

        assert_eq!(result.tree.visible_text(), "bold");
        assert_eq!(result.map_offset(8), 4);
        assert_eq!(result.tree.serialize_markdown(), "**bold**");
    }

    #[test]
    fn renders_nested_marks_without_storing_markers_in_text() {
        let tree = InlineTextTree::from_markdown("**<u>*TEST*</u>**");
        let cache = tree.render_cache();

        assert_eq!(cache.visible_text(), "TEST");
        assert_eq!(
            cache.style_at(0),
            InlineStyle {
                bold: true,
                italic: true,
                underline: true,
                strikethrough: false,
                highlight: false,
                code: false,
                script: InlineScript::Normal,
                emphasis_marker: Some('*'),
                line_break: false,
            }
        );
    }

    #[test]
    fn replace_visible_range_raw_preserves_markers_as_literal_text() {
        let tree = InlineTextTree::plain("alpha");
        let result = tree.replace_visible_range_raw(
            5..5,
            "**`<u>x</u>`**",
            InlineInsertionAttributes::default(),
        );

        assert_eq!(result.tree.visible_text(), "alpha**`<u>x</u>`**");
        // raw 编辑路径（代码块/raw 块）的文本就是源码本身：序列化原样写回，
        // 不加转义——这些块重读时不走行内 markdown 解析，记号保持字面。
        assert_eq!(result.tree.serialize_markdown(), "alpha**`<u>x</u>`**");
    }

    #[test]
    fn unwrap_code_fragments_keeps_text_and_removes_code_style() {
        let mut tree = InlineTextTree::from_markdown("before `code` after");
        tree.unwrap_styles_on_fragments(&[(1, StyleFlag::Code)]);

        assert_eq!(tree.visible_text(), "before code after");
        let cache = tree.render_cache();
        assert!(!cache.style_at(7).code);
        assert_eq!(tree.serialize_markdown(), "before code after");
    }

    #[test]
    fn parses_and_serializes_strikethrough() {
        let tree = InlineTextTree::from_markdown("~~text~~");
        let cache = tree.render_cache();

        assert_eq!(tree.visible_text(), "text");
        assert!(cache.style_at(0).strikethrough);
        assert_eq!(tree.serialize_markdown(), "~~text~~");
    }

    #[test]
    fn parses_and_serializes_superscript() {
        let tree = InlineTextTree::from_markdown("x^2^");
        let cache = tree.render_cache();

        assert_eq!(tree.visible_text(), "x2");
        assert_eq!(cache.style_at(1).script, InlineScript::Superscript);
        assert_eq!(tree.serialize_markdown(), "x^2^");
    }

    #[test]
    fn parses_and_serializes_subscript_without_conflicting_with_strikethrough() {
        let tree = InlineTextTree::from_markdown("H~2~O and ~~old~~");
        let cache = tree.render_cache();

        assert_eq!(tree.visible_text(), "H2O and old");
        assert_eq!(cache.style_at(1).script, InlineScript::Subscript);
        assert!(cache.style_at("H2O and ".len()).strikethrough);
        assert_eq!(tree.serialize_markdown(), "H~2~O and ~~old~~");
    }

    #[test]
    fn script_markers_require_ascii_context_and_ascii_body() {
        for markdown in ["\\^2^", "\\~2~", "汉^2^", "H~二~O", "`x^2^ H~2~O`"] {
            let tree = InlineTextTree::from_markdown(markdown);
            assert!(
                tree.render_cache()
                    .spans()
                    .iter()
                    .all(|span| span.style.script == InlineScript::Normal),
                "{markdown} should not produce script spans"
            );
        }
    }

    #[test]
    fn inline_html_sup_and_sub_map_to_script_style() {
        let tree = InlineTextTree::from_markdown("x<sup>2</sup> and H<sub>2</sub>O");
        let cache = tree.render_cache();

        assert_eq!(tree.visible_text(), "x2 and H2O");
        assert_eq!(cache.style_at(1).script, InlineScript::Superscript);
        assert_eq!(
            cache.style_at("x2 and H".len()).script,
            InlineScript::Subscript
        );
        assert_eq!(tree.serialize_markdown(), "x^2^ and H~2~O");

        let standalone = InlineTextTree::from_markdown("<sup>2</sup>");
        assert_eq!(standalone.serialize_markdown(), "<sup>2</sup>");
    }

    #[test]
    fn unmatched_strikethrough_markers_stay_literal() {
        let tree = InlineTextTree::from_markdown("~~text");
        assert_eq!(tree.visible_text(), "~~text");
        // 序列化保真：未配对的删除线记号原样写回（重读还是字面）。
        assert_eq!(tree.serialize_markdown(), "~~text");
    }

    #[test]
    fn parses_and_serializes_highlight() {
        let tree = InlineTextTree::from_markdown("==text==");
        let cache = tree.render_cache();

        assert_eq!(tree.visible_text(), "text");
        assert!(cache.style_at(0).highlight);
        assert_eq!(tree.serialize_markdown(), "==text==");
    }

    /// 高亮能与强调套在一起；同一句里后面那个没配对的 `==` 必须还是字面记号
    /// （`1 == 2` 这种写法在数学与代码说明里很常见，是本仓库引入新语法的代价边界）。
    /// 套叠的先后按样式栈序写回（强调在外、高亮在内），与 `~~` 已有的口径一致。
    #[test]
    fn highlight_nests_with_emphasis_and_leaves_an_unpaired_marker_literal() {
        let tree = InlineTextTree::from_markdown("==**bold**== and 1 == 2");
        let cache = tree.render_cache();

        assert_eq!(tree.visible_text(), "bold and 1 == 2");
        assert!(cache.style_at(0).highlight);
        assert!(cache.style_at(0).bold);
        assert!(!cache.style_at("bold".len()).highlight);
        assert_eq!(tree.serialize_markdown(), "**==bold==** and 1 == 2");

        // 再读一次样式不变：写回换了先后，但没把套叠读丢。
        let again = InlineTextTree::from_markdown(&tree.serialize_markdown());
        assert!(again.render_cache().style_at(0).highlight);
        assert!(again.render_cache().style_at(0).bold);
    }

    #[test]
    fn toggle_highlight_operates_on_selected_slice_only() {
        let mut tree = InlineTextTree::plain("1234");
        assert!(tree.toggle_highlight(1..3));
        assert_eq!(tree.serialize_markdown(), "1==23==4");
        assert!(tree.toggle_highlight(1..3));
        assert_eq!(tree.serialize_markdown(), "1234");
    }

    #[test]
    fn toggle_strikethrough_operates_on_selected_slice_only() {
        let mut tree = InlineTextTree::plain("1234");
        assert!(tree.toggle_strikethrough(1..4));
        assert!(tree.toggle_strikethrough(2..4));

        let serialized = tree.serialize_markdown();
        let reparsed = InlineTextTree::from_markdown(&serialized);

        assert_eq!(serialized, "1~~2~~34");
        assert_eq!(tree, reparsed);
    }

    #[test]
    fn insertion_at_outer_end_of_terminal_strikethrough_is_plain_text() {
        let tree = InlineTextTree::from_markdown("~~123~~");
        let result = tree.replace_visible_range(
            tree.visible_len()..tree.visible_len(),
            "456",
            tree.attributes_for_insertion_at(tree.visible_len()),
        );
        assert_eq!(result.tree.serialize_markdown(), "~~123~~456");
    }

    #[test]
    fn insertion_at_outer_start_of_terminal_strikethrough_is_plain_text() {
        let tree = InlineTextTree::from_markdown("~~123~~");
        let result = tree.replace_visible_range(0..0, "0", tree.attributes_for_insertion_at(0));
        assert_eq!(result.tree.serialize_markdown(), "0~~123~~");
    }

    #[test]
    fn serializes_partial_underline_removal_without_ambiguous_star_runs() {
        let mut tree = InlineTextTree::plain("1234");
        assert!(tree.toggle_bold(1..4));
        assert!(tree.toggle_underline(1..4));
        assert!(tree.toggle_italic(1..4));
        assert!(tree.toggle_underline(2..4));

        let serialized = tree.serialize_markdown();
        let reparsed = InlineTextTree::from_markdown(&serialized);

        assert_eq!(serialized, "1**<u>*2*</u>*34***");
        assert!(!serialized.contains("*****34"));
        assert_eq!(reparsed.visible_text(), "1234");
        assert_eq!(reparsed.render_cache().spans(), tree.render_cache().spans());
    }

    #[test]
    fn parses_inline_links_autolinks_and_preserves_other_unsupported_inline_syntax() {
        let markdown =
            "[link](http://example.com) ![alt](/img.png) <http://example.com/> <span>x</span>";
        let tree = InlineTextTree::from_markdown(markdown);

        assert_eq!(
            tree.visible_text(),
            "link ![alt](/img.png) http://example.com/ <span>x</span>"
        );
        assert_eq!(tree.render_cache().link_at(0), Some("http://example.com"));
        assert_eq!(
            tree.render_cache().link_at("link ![alt](/img.png) ".len()),
            Some("http://example.com/")
        );
        assert_eq!(tree.serialize_markdown(), markdown);
    }

    #[test]
    fn parses_dollar_inline_math_as_source_preserving_fragment() {
        let markdown = "before $x^2$ after";
        let tree = InlineTextTree::from_markdown(markdown);
        let cache = tree.render_cache();
        let math_start = "before ".len();
        let math = cache
            .inline_math_at(math_start)
            .expect("inline math span should be recorded");

        assert_eq!(tree.visible_text(), markdown);
        assert_eq!(math.source, "$x^2$");
        assert_eq!(math.body, "x^2");
        assert_eq!(math.delimiter, InlineMathDelimiter::Dollar);
        assert_eq!(tree.serialize_markdown(), markdown);
    }

    #[test]
    fn parses_paren_inline_math_as_source_preserving_fragment() {
        let markdown = "before \\(\\frac{1}{2}\\) after";
        let tree = InlineTextTree::from_markdown(markdown);
        let cache = tree.render_cache();
        let math_start = "before ".len();
        let math = cache
            .inline_math_at(math_start)
            .expect("inline math span should be recorded");

        assert_eq!(tree.visible_text(), markdown);
        assert_eq!(math.source, "\\(\\frac{1}{2}\\)");
        assert_eq!(math.body, "\\frac{1}{2}");
        assert_eq!(math.delimiter, InlineMathDelimiter::Paren);
        assert_eq!(tree.serialize_markdown(), markdown);
    }

    #[test]
    fn paren_math_never_falls_to_the_currency_heuristic() {
        // 用户报修（cases/07-numeric-math.md）：`value \(42\).` 被 `$42$` 的货币启发
        // 误判成钱数、原样显示。`\(...\)` 是无歧义的 TeX 定界符，货币启发只许作用在
        // `$…$` 这种本身就与钱写法冲突的定界符上（规则见 looks_like_obvious_currency）。
        for (markdown, start) in [
            (r"value \(42\).", "value ".len()),
            (r"total \(0.5\) us", "total ".len()),
            (r"\(42\)", 0),
            (r"a \(42\) b \(43\)", 2),
        ] {
            let tree = InlineTextTree::from_markdown(markdown);
            let cache = tree.render_cache();
            let math = cache
                .inline_math_at(start)
                .unwrap_or_else(|| panic!("{markdown:?} 应渲染为公式"));
            assert_eq!(math.delimiter, InlineMathDelimiter::Paren);
            assert_eq!(tree.visible_text(), markdown);
            assert_eq!(tree.serialize_markdown(), markdown);
        }

        // `$…$` 侧的货币启发原样保留：体内全是数字、或紧贴数字，都按钱读。
        for plain in ["cost $42$", "$42 dollars", "total $0.50$"] {
            let tree = InlineTextTree::from_markdown(plain);
            assert!(
                tree.render_cache()
                    .spans()
                    .iter()
                    .all(|span| span.math.is_none()),
                "{plain:?} 是货币写法，不该渲染为公式"
            );
        }
    }

    #[test]
    fn rejects_conservative_inline_math_cases() {
        for markdown in ["\\$x$", "$ x $", "$", "$x\ny$", "cost $12$"] {
            let tree = InlineTextTree::from_markdown(markdown);
            assert!(
                tree.render_cache()
                    .spans()
                    .iter()
                    .all(|span| span.math.is_none()),
                "{markdown:?} should stay plain text"
            );
        }
    }

    #[test]
    fn inline_math_does_not_parse_inside_code_spans() {
        let tree = InlineTextTree::from_markdown("`$x$` and $y$");
        let cache = tree.render_cache();

        assert!(cache.style_at(0).code);
        assert!(cache.inline_math_at(0).is_none());
        assert!(cache.inline_math_at("$x$ and ".len()).is_some());
        assert_eq!(tree.serialize_markdown(), "`$x$` and $y$");
    }

    #[test]
    fn escapes_all_ascii_punctuation_not_just_the_old_whitelist() {
        // 用户报修（cases/10-escape.md）：转义白名单只认 `\ * _ ~ [ ] ` ^ 与三个标签，
        // CommonMark 转义的是**全部 ASCII 标点**——`\#` 该显示裸 `#`，不该多一个反斜杠。
        let markdown = "\\*literal stars\\* \\# literal hash \\\\backslash and \\<u>literal tag\\</u>";
        let tree = InlineTextTree::from_markdown(markdown);
        assert_eq!(
            tree.visible_text(),
            "*literal stars* # literal hash \\backslash and <u>literal tag</u>"
        );
        // 逐字节还原用户写法（cases/10-escape.md 的原文行）。
        assert_eq!(tree.serialize_markdown(), markdown);
    }

    #[test]
    fn every_commonmark_punctuation_escape_loses_its_backslash() {
        // 共享规则的覆盖面：ASCII 标点表全集（CommonMark 的转义集 = is_ascii_punctuation）。
        for punctuation_char in [
            '!', '"', '#', '$', '%', '&', '\'', '(', ')', '*', '+', ',', '-', '.', '/',
            ':', ';', '<', '=', '>', '?', '@', '[', '\\', ']', '^', '_', '`', '{', '|',
            '}', '~',
        ] {
            let markdown = format!("\\{punctuation_char}x");
            let tree = InlineTextTree::from_markdown(&markdown);
            assert_eq!(
                tree.visible_text(),
                format!("{punctuation_char}x"),
                "{markdown:?} 的转义没有生效"
            );
            assert_eq!(tree.serialize_markdown(), markdown, "{markdown:?} 写法被改写");
        }
    }

    #[test]
    fn backslash_end_of_line_is_a_hard_break_without_the_backslash() {
        // 用户报修（cases/10-line-break.md）：`Slash one\` + 换行是 CommonMark 硬换行，
        // 可见文本不该留着反斜杠；序列化必须把用户的反斜杠写法逐字节写回。
        let markdown = "Slash one\\\nSlash two";
        let tree = InlineTextTree::from_markdown(markdown);
        assert_eq!(tree.visible_text(), "Slash one\nSlash two");
        assert_eq!(tree.serialize_markdown(), markdown);

        // CRLF 行尾同理：换行整体保留，反斜杠消失。
        let crlf = "Slash one\\\r\nSlash two";
        let tree = InlineTextTree::from_markdown(crlf);
        assert_eq!(tree.visible_text(), "Slash one\r\nSlash two");
        assert_eq!(tree.serialize_markdown(), crlf);
    }

    #[test]
    fn two_trailing_spaces_before_newline_round_trip() {
        // cases/10-line-break.md 的兄弟规则：行尾两个空格 + 换行是硬换行，
        // 换行符本身就是可见断点；空格与写法都要原样保留。
        let markdown = "Hard one  \nHard two";
        let tree = InlineTextTree::from_markdown(markdown);
        assert_eq!(tree.visible_text(), markdown);
        assert_eq!(tree.serialize_markdown(), markdown);
    }

    #[test]
    fn parses_inline_link_title_without_polluting_open_target() {
        let markdown = "[ABC](https://abc.com \"https://abc.com\")";
        let tree = InlineTextTree::from_markdown(markdown);

        assert_eq!(tree.visible_text(), "ABC");
        assert_eq!(
            tree.render_cache().link_hit_at(0),
            Some(&InlineLinkHit {
                prompt_target: "https://abc.com".to_string(),
                open_target: "https://abc.com".to_string(),
            })
        );
        assert_eq!(tree.serialize_markdown(), markdown);
    }

    #[test]
    fn parses_span_style_as_inline_html_not_link() {
        let markdown = "留意<span style='color:blue;'>磁盘预留空间、系统环境变量</span>等问题";
        let tree = InlineTextTree::from_markdown(markdown);
        let cache = tree.render_cache();
        let span_start = "留意".len();

        assert_eq!(tree.visible_text(), "留意磁盘预留空间、系统环境变量等问题");
        assert_eq!(cache.link_at(span_start), None);
        assert!(matches!(
            cache.html_style_at(span_start).and_then(|style| style.color),
            Some(HtmlCssColor::Rgba(color))
                if color.red == 0 && color.green == 0 && color.blue == 255
        ));
        assert_eq!(cache.html_style_at(0), None);
        assert_eq!(
            tree.serialize_markdown(),
            "留意<span style=\"color: rgba(0,0,255,1.000);\">磁盘预留空间、系统环境变量</span>等问题"
        );
    }

    #[test]
    fn inline_span_style_allows_nested_markdown_code() {
        let markdown = "<span style='color:blue;'>英伟达驱动`CUDA+cuDNN`</span>";
        let tree = InlineTextTree::from_markdown(markdown);
        let cache = tree.render_cache();
        let code_start = "英伟达驱动".len();

        assert_eq!(tree.visible_text(), "英伟达驱动CUDA+cuDNN");
        assert!(cache.style_at(code_start).code);
        assert!(matches!(
            cache.html_style_at(code_start).and_then(|style| style.color),
            Some(HtmlCssColor::Rgba(color))
                if color.red == 0 && color.green == 0 && color.blue == 255
        ));

        let reparsed = InlineTextTree::from_markdown(&tree.serialize_markdown());
        assert_eq!(reparsed.visible_text(), tree.visible_text());
        assert_eq!(reparsed.render_cache().spans(), tree.render_cache().spans());
    }

    #[test]
    fn html_like_tags_are_not_autolinks_when_unsafe_or_unclosed() {
        let unclosed = InlineTextTree::from_markdown("<span style='color:blue;'>x");
        assert_eq!(unclosed.visible_text(), "<span style='color:blue;'>x");
        assert_eq!(unclosed.render_cache().link_at(0), None);

        let script = InlineTextTree::from_markdown("<script>alert(1)</script>");
        assert_eq!(script.visible_text(), "<script>alert(1)</script>");
        assert_eq!(script.render_cache().link_at(0), None);
    }

    #[test]
    fn parses_reference_style_links_with_definitions_and_preserves_syntax() {
        let markdown = "[reference link][ref-link]";
        let definitions =
            crate::components::markdown::link::parse_link_reference_definitions("[ref-link]: https://example.com");
        let tree = InlineTextTree::from_markdown_with_link_references(markdown, &definitions);

        assert_eq!(tree.visible_text(), "reference link");
        assert_eq!(tree.render_cache().link_at(0), Some("https://example.com"));
        assert_eq!(tree.serialize_markdown(), markdown);
    }

    #[test]
    fn parses_reference_style_links_with_generic_normalized_labels() {
        let markdown = "[reference link][Ref   Links]";
        let definitions = crate::components::markdown::link::parse_link_reference_definitions(
            "[ref links]: https://example.com",
        );
        let tree = InlineTextTree::from_markdown_with_link_references(markdown, &definitions);

        assert_eq!(tree.visible_text(), "reference link");
        assert_eq!(tree.render_cache().link_at(0), Some("https://example.com"));
        assert_eq!(
            tree.render_cache().link_hit_at(0),
            Some(&InlineLinkHit {
                prompt_target: "Ref   Links".to_string(),
                open_target: "https://example.com".to_string(),
            })
        );
        assert_eq!(tree.serialize_markdown(), markdown);
    }

    #[test]
    fn parses_collapsed_reference_style_links_with_definitions() {
        let markdown = "[collapsed reference][]";
        let definitions = crate::components::markdown::link::parse_link_reference_definitions(
            "[collapsed reference]: https://example.org",
        );
        let tree = InlineTextTree::from_markdown_with_link_references(markdown, &definitions);

        assert_eq!(tree.visible_text(), "collapsed reference");
        assert_eq!(tree.render_cache().link_at(0), Some("https://example.org"));
        assert_eq!(
            tree.serialize_markdown(),
            "[collapsed reference][collapsed reference]"
        );
    }

    #[test]
    fn parses_shortcut_reference_style_links_with_definitions() {
        let markdown = "[shortcut reference]";
        let definitions = crate::components::markdown::link::parse_link_reference_definitions(
            "[shortcut reference]: https://example.net",
        );
        let tree = InlineTextTree::from_markdown_with_link_references(markdown, &definitions);

        assert_eq!(tree.visible_text(), "shortcut reference");
        assert_eq!(tree.render_cache().link_at(0), Some("https://example.net"));
        assert_eq!(
            tree.serialize_markdown(),
            "[shortcut reference][shortcut reference]"
        );
    }

    #[test]
    fn resolves_reference_link_examples_from_test_markdown() {
        let markdown = include_str!("../../../../../fixtures/markdown-baseline.md");
        let definitions = crate::components::markdown::link::parse_link_reference_definitions(markdown);
        let tree = InlineTextTree::from_markdown_with_link_references(
            "[reference link][ref-link] [collapsed reference][] [shortcut reference]",
            &definitions,
        );

        assert_eq!(
            tree.visible_text(),
            "reference link collapsed reference shortcut reference"
        );
        assert_eq!(tree.render_cache().link_at(0), Some("https://example.com"));
        assert_eq!(
            tree.render_cache().link_at("reference link ".len()),
            Some("https://example.org")
        );
        assert_eq!(
            tree.render_cache()
                .link_at("reference link collapsed reference ".len()),
            Some("https://example.net")
        );
    }

    #[test]
    fn unresolved_reference_style_links_remain_literal_text() {
        let markdown = "[reference link][missing]";
        let tree = InlineTextTree::from_markdown_with_link_references(
            markdown,
            &LinkReferenceDefinitions::default(),
        );

        assert_eq!(tree.visible_text(), markdown);
        assert_eq!(tree.render_cache().link_at(0), None);
        assert_eq!(tree.serialize_markdown(), markdown);
    }

    #[test]
    fn typing_at_the_caret_keeps_the_escapes_the_source_already_had() {
        // `\*` 在源码里是字面星号；重解析时再把它当定界符，可见文本就短一截、
        // 序列化也回不去原来的字节了。
        let markdown = "字面星号 \\*不强调\\* 和字面下划线 \\_x\\_";
        let tree = InlineTextTree::from_markdown(markdown);
        let result = tree.replace_visible_range_with_link_references(
            0..0,
            "X",
            InlineInsertionAttributes::default(),
            &LinkReferenceDefinitions::default(),
        );

        assert_eq!(
            result.tree.visible_text(),
            "X字面星号 *不强调* 和字面下划线 _x_",
            "字面星号被读成了强调定界符"
        );
        assert_eq!(
            result.tree.serialize_markdown(),
            format!("X{markdown}"),
            "插入处以外的写法被改写了"
        );
    }

    #[test]
    fn unresolved_shortcut_reference_links_remain_literal_text() {
        let markdown = "[shortcut reference]";
        let tree = InlineTextTree::from_markdown_with_link_references(
            markdown,
            &LinkReferenceDefinitions::default(),
        );

        assert_eq!(tree.visible_text(), markdown);
        assert_eq!(tree.render_cache().link_at(0), None);
        assert_eq!(tree.serialize_markdown(), markdown);
    }

    #[test]
    fn shortcut_reference_detection_does_not_consume_images_as_links() {
        let definitions = crate::components::markdown::link::parse_link_reference_definitions(
            "[alt]: https://example.com/not-an-image-link",
        );
        let tree = InlineTextTree::from_markdown_with_link_references("![alt]", &definitions);

        assert_eq!(tree.visible_text(), "![alt]");
        assert_eq!(tree.render_cache().link_at(0), None);
        assert_eq!(tree.serialize_markdown(), "![alt]");
    }

    #[test]
    fn shortcut_reference_detection_does_not_rewrite_reference_images() {
        let definitions = crate::components::markdown::link::parse_link_reference_definitions(
            "[img]: https://example.com/image.png",
        );
        let tree =
            InlineTextTree::from_markdown_with_link_references("![cover][img]", &definitions);

        assert_eq!(tree.visible_text(), "![cover][img]");
        assert_eq!(tree.render_cache().link_at(0), None);
        assert_eq!(tree.serialize_markdown(), "![cover][img]");
    }

    #[test]
    fn parses_mailto_autolinks_and_preserves_syntax() {
        let markdown = "<mailto:test@example.com>";
        let tree = InlineTextTree::from_markdown(markdown);

        assert_eq!(tree.visible_text(), "mailto:test@example.com");
        assert_eq!(
            tree.render_cache().link_at(0),
            Some("mailto:test@example.com")
        );
        assert_eq!(tree.serialize_markdown(), markdown);
    }

    #[test]
    fn parses_any_standalone_autolink_and_preserves_syntax() {
        let markdown = "<ref2>";
        let tree = InlineTextTree::from_markdown(markdown);

        assert_eq!(tree.visible_text(), "ref2");
        assert_eq!(tree.render_cache().link_at(0), Some("ref2"));
        assert_eq!(
            tree.render_cache().link_hit_at(0),
            Some(&InlineLinkHit {
                prompt_target: "ref2".to_string(),
                open_target: "ref2".to_string(),
            })
        );
        assert_eq!(tree.serialize_markdown(), markdown);
    }

    #[test]
    fn parses_nested_inline_marks_inside_link_label() {
        let tree = InlineTextTree::from_markdown("[**go** now](https://example.com)");
        let cache = tree.render_cache();

        assert_eq!(tree.visible_text(), "go now");
        assert_eq!(cache.link_at(0), Some("https://example.com"));
        assert!(cache.style_at(0).bold);
        assert_eq!(
            tree.serialize_markdown(),
            "[**go** now](https://example.com)"
        );
    }

    #[test]
    fn serializes_partial_bold_removal_without_ambiguous_star_runs() {
        let mut tree = InlineTextTree::plain("1234");
        assert!(tree.toggle_bold(1..4));
        assert!(tree.toggle_italic(1..4));
        assert!(tree.toggle_bold(2..4));

        let serialized = tree.serialize_markdown();
        let reparsed = InlineTextTree::from_markdown(&serialized);

        assert_eq!(serialized, "1***2***<em>34</em>");
        assert_eq!(reparsed.visible_text(), "1234");
        assert_eq!(reparsed.render_cache().spans(), tree.render_cache().spans());
    }

    // --- inline code tests ---

    #[test]
    fn parses_backtick_as_code_style() {
        let tree = InlineTextTree::from_markdown("a `code` b");
        let cache = tree.render_cache();

        assert_eq!(cache.visible_text(), "a code b");
        // "code" at offset 2 should have code style
        let style = cache.style_at(2);
        assert!(style.code, "expected code=true at offset 2");
        assert!(!style.bold);
    }

    #[test]
    fn backtick_content_preserves_markers_as_literal() {
        // Inside a code span, ** and * are literal, not parsed as bold/italic.
        let tree = InlineTextTree::from_markdown("`**not bold**`");
        let cache = tree.render_cache();

        assert_eq!(cache.visible_text(), "**not bold**");
        let style = cache.style_at(0);
        assert!(style.code);
        assert!(!style.bold);
        assert!(!style.italic);
    }

    #[test]
    fn unclosed_backtick_is_literal() {
        let tree = InlineTextTree::from_markdown("a `b");
        assert_eq!(tree.visible_text(), "a `b");
        // 序列化保真：未闭合的反引号原样写回（重读还是字面，写法不许被洗）。
        assert_eq!(tree.serialize_markdown(), "a `b");
    }

    #[test]
    fn toggle_code_on_selection() {
        let mut tree = InlineTextTree::plain("hello world");
        assert!(tree.toggle_code(0..5)); // "hello"
        assert_eq!(tree.serialize_markdown(), "`hello` world");
    }

    #[test]
    fn toggle_code_twice_removes_code() {
        let mut tree = InlineTextTree::plain("hello world");
        assert!(tree.toggle_code(0..5));
        assert!(tree.toggle_code(0..5)); // toggle back
        assert_eq!(tree.serialize_markdown(), "hello world");
    }

    #[test]
    fn code_round_trips_through_serialization() {
        let tree = InlineTextTree::from_markdown("a `code` b");
        let serialized = tree.serialize_markdown();
        let reparsed = InlineTextTree::from_markdown(&serialized);

        assert_eq!(serialized, "a `code` b");
        assert_eq!(reparsed.visible_text(), "a code b");
        assert_eq!(reparsed.render_cache().spans(), tree.render_cache().spans());
    }

    #[test]
    fn code_inside_bold_text() {
        // `**bold `code` more**` — bold wraps around a code span.
        let tree = InlineTextTree::from_markdown("**bold `code` more**");
        let serialized = tree.serialize_markdown();
        let reparsed = InlineTextTree::from_markdown(&serialized);

        assert_eq!(tree.visible_text(), "bold code more");
        assert_eq!(reparsed.visible_text(), tree.visible_text());
        assert_eq!(reparsed.render_cache().spans(), tree.render_cache().spans());
    }

    #[test]
    fn consecutive_backticks_treated_as_literal() {
        // Per CommonMark: a backtick run that has no matching closing run
        // is treated as literal text.
        let tree = InlineTextTree::from_markdown("``");
        // Two backticks with no closing -> literal (run_len=2, no matching close).
        assert_eq!(tree.visible_text(), "``");
        assert!(!tree.render_cache().style_at(0).code);
    }

    #[test]
    fn variable_length_backtick_run() {
        // `` `` `x` ``` `` (run_len=1 with 'x', matching close of run_len=1)
        let tree = InlineTextTree::from_markdown("`x`");
        assert_eq!(tree.visible_text(), "x");
        assert!(tree.render_cache().style_at(0).code);

        // ``` `` `` `` `` (run_len=2, content "a", run_len=2 close)
        let tree2 = InlineTextTree::from_markdown("``a``");
        assert_eq!(tree2.visible_text(), "a");
        assert!(tree2.render_cache().style_at(0).code);
    }

    #[test]
    fn code_span_content_normalization() {
        // Leading/trailing single space is stripped.
        let tree = InlineTextTree::from_markdown("` hello `");
        assert_eq!(tree.visible_text(), "hello");
        assert!(tree.render_cache().style_at(0).code);

        // All-space content is preserved (no stripping per spec).
        let tree2 = InlineTextTree::from_markdown("`   `");
        assert_eq!(tree2.visible_text(), "   ");
    }

    #[test]
    fn code_span_newline_is_preserved_as_hard_line() {
        let tree = InlineTextTree::from_markdown("`a\nb`");
        assert_eq!(tree.visible_text(), "a\nb");

        let cache = tree.render_cache();
        assert_eq!(cache.spans().len(), 1);
        assert_eq!(cache.spans()[0].range, 0..3);
        assert!(cache.spans()[0].style.code);
        assert_eq!(tree.serialize_markdown(), "`a\nb`");
    }

    #[test]
    fn code_span_blank_line_stays_inside_single_code_span() {
        let tree = InlineTextTree::from_markdown("`line 1\n\nline 2`");
        assert_eq!(tree.visible_text(), "line 1\n\nline 2");

        let cache = tree.render_cache();
        assert_eq!(cache.spans().len(), 1);
        assert_eq!(cache.spans()[0].range, 0.."line 1\n\nline 2".len());
        assert!(cache.spans()[0].style.code);
        assert_eq!(tree.serialize_markdown(), "`line 1\n\nline 2`");
    }

    #[test]
    fn code_span_content_keeps_inline_markers_literal() {
        let tree = InlineTextTree::from_markdown("`*[x] [link](x) \\\\`");

        assert_eq!(tree.visible_text(), "*[x] [link](x) \\\\");
        let cache = tree.render_cache();
        assert_eq!(cache.spans().len(), 1);
        assert!(cache.spans()[0].style.code);
        assert!(cache.spans()[0].link.is_none());
        assert!(!cache.spans()[0].style.bold);
        assert!(!cache.spans()[0].style.italic);
    }

    #[test]
    fn parses_literal_backtick_runs_with_unambiguous_delimiters() {
        let markdown = "`` ` `` and ``` `` ``` and ```` ``` ````";
        let tree = InlineTextTree::from_markdown(markdown);
        let cache = tree.render_cache();
        let code_ranges = cache
            .spans()
            .iter()
            .filter(|span| span.style.code)
            .map(|span| span.range.clone())
            .collect::<Vec<_>>();

        assert_eq!(tree.visible_text(), "` and `` and ```");
        assert_eq!(code_ranges, vec![0..1, 6..8, 13..16]);
        assert!(!cache.style_at("` ".len()).code);
        assert!(!cache.style_at("` and `` ".len()).code);

        let serialized = tree.serialize_markdown();
        let reparsed = InlineTextTree::from_markdown(&serialized);
        assert_eq!(reparsed.visible_text(), tree.visible_text());
        assert_eq!(reparsed.render_cache().spans(), cache.spans());
    }

    #[test]
    fn serializes_code_spans_with_safe_backtick_delimiters_and_padding() {
        for text in [" leading", "trailing ", "`tick", "tick`", "`", "``", "   "] {
            let tree = InlineTextTree::from_fragments(vec![InlineFragment {
                text: text.to_string(),
                style: InlineStyle {
                    code: true,
                    ..InlineStyle::default()
                },
                html_style: None,
                link: None,
                footnote: None,
                math: None,
            }]);
            let serialized = tree.serialize_markdown();
            let reparsed = InlineTextTree::from_markdown(&serialized);

            assert_eq!(
                reparsed.visible_text(),
                text,
                "serialized as {serialized:?}"
            );
            assert_eq!(reparsed.render_cache().spans(), tree.render_cache().spans());
        }
    }

    #[test]
    fn source_to_rendered_round_trip_preserves_code_span() {
        // Simulate Source -> Rendered: raw markdown -> from_markdown parses it.
        let raw = "`123`";
        let tree = InlineTextTree::from_markdown(raw);
        assert_eq!(tree.visible_text(), "123");
        assert!(tree.render_cache().style_at(0).code);

        // Serialize back: must produce valid markdown.
        let serialized = tree.serialize_markdown();
        assert_eq!(serialized, "`123`");

        // Re-parse: must produce same result.
        let reparsed = InlineTextTree::from_markdown(&serialized);
        assert_eq!(reparsed.visible_text(), "123");
        assert!(reparsed.render_cache().style_at(0).code);
    }

    #[test]
    fn raw_text_with_backticks_not_double_escaped() {
        // Simulate the Source block's display_text() path.
        let raw = "`123`";
        // display_text() returns raw text as-is; from_markdown re-parses.
        let parsed = InlineTextTree::from_markdown(raw);
        assert_eq!(parsed.visible_text(), "123");

        // A second round-trip should NOT escape or double the backticks.
        let serialized = parsed.serialize_markdown();
        assert_eq!(serialized, "`123`");
        let reparsed = InlineTextTree::from_markdown(&serialized);
        assert_eq!(reparsed.visible_text(), "123");
    }

    #[test]
    fn escaped_backtick_in_code() {
        let tree = InlineTextTree::from_markdown("\\`not code\\`");
        assert_eq!(tree.visible_text(), "`not code`");
        // Escaped backticks are literal, not code delimiters.
        let cache = tree.render_cache();
        assert!(!cache.style_at(0).code);
        assert_eq!(tree.serialize_markdown(), "\\`not code\\`");
    }

    #[test]
    fn intraword_underscores_stay_literal() {
        // 用户报修：`**topic_embedding_attention 有轨迹无产物**` 渲染成
        // `**topic*embedding*attention 有轨迹无产物**`，存盘后原文被改写。
        for source in [
            "**topic_embedding_attention 有轨迹无产物**",
            "**topic_embedding_attention**",
            "topic_embedding_attention",
            "a_b_c",
            "**a_b**",
            "中文_强调_中文",
            "foo__bar__baz",
            "snake_case_name",
        ] {
            let tree = InlineTextTree::from_markdown(source);
            assert_eq!(
                tree.serialize_markdown(),
                source,
                "词中下划线必须原样保留（不解析成强调、不补反斜杠），输入 {source:?}"
            );
            assert_eq!(tree.visible_text(), source.replace("**", ""), "可见文本");
        }
    }

    #[test]
    fn unclosed_underscore_span_keeps_text_uncopied() {
        // 正文扫描拒绝所有闭合候选时，不能既留下正文又重扫整段（内容会翻倍）。
        for source in ["_a_b", "_x_1", "_private_var", "_foo_bar baz"] {
            let tree = InlineTextTree::from_markdown(source);
            assert_eq!(tree.visible_text(), source, "可见文本不得重复，输入 {source:?}");
        }
        // 没关闭的串仍是字面量；末尾能找到合法闭合的才是斜体（与 CommonMark 一致）。
        assert_eq!(InlineTextTree::from_markdown("_a_b_").visible_text(), "a_b");
    }

    #[test]
    fn underscore_emphasis_outside_words_still_parses() {
        // 放宽词中规则不能误伤真正的下划线强调。
        assert_eq!(
            InlineTextTree::from_markdown("_italic_").serialize_markdown(),
            "_italic_"
        );
        assert_eq!(
            InlineTextTree::from_markdown("__bold__").serialize_markdown(),
            "__bold__"
        );
        assert_eq!(
            InlineTextTree::from_markdown("(_强调_)").serialize_markdown(),
            "(_强调_)"
        );
        assert_eq!(
            InlineTextTree::from_markdown("`_a_b_`").serialize_markdown(),
            "`_a_b_`"
        );
    }
    #[test]
    fn visible_text_normalization_keeps_backslashes_literal() {
        let references = LinkReferenceDefinitions::default();
        let backslashes = |count: usize| "\\".repeat(count);
        let visible = |text: &str| {
            InlineTextTree::plain(text)
                .normalize_visible_text_with_link_references(&references)
                .tree
        };

        // 可见文本模式：用户按下的反斜杠就是字符本身
        assert_eq!(
            visible(&format!("a{}b", backslashes(2))).visible_text(),
            format!("a{}b", backslashes(2))
        );
        assert_eq!(
            visible(&format!("{}*b", backslashes(1))).visible_text(),
            format!("{}*b", backslashes(1))
        );
        // 源文本模式（读文件）：转义语义不变
        assert_eq!(
            InlineTextTree::from_markdown(&format!("a{}b", backslashes(2))).visible_text(),
            format!("a{}b", backslashes(1))
        );
        assert_eq!(InlineTextTree::from_markdown("\\*b").visible_text(), "*b");
        // 写回源文件：只有会被重读吃掉的反斜杠才转义（`\\` 后面的第一个 `\`
        // 要保护，第二个后面跟普通字符就原样）。整体重读必须还原出两个反斜杠。
        let written = visible(&format!("a{}b", backslashes(2))).serialize_markdown();
        assert_eq!(written, format!("a{}b", backslashes(3)));
        assert_eq!(
            InlineTextTree::from_markdown(&written).visible_text(),
            format!("a{}b", backslashes(2)),
            "写回再重读，用户敲的反斜杠一个不能少"
        );
    }
