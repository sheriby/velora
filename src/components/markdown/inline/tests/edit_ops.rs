    use super::super::{
        InlineFragment, InlineInsertionAttributes, InlineLinkHit, InlineMathDelimiter,
        InlineScript, InlineStyle, InlineTextTree, LinkReferenceDefinitions, StyleFlag,
    };
    use crate::components::HtmlCssColor;

    #[test]
    fn review_editing_link_keeps_sibling_backslash_escapes() {
        // 替换链接片段曾清空整块转义账，导致相邻字面星号重读后变为强调。
        let mut tree = InlineTextTree::from_markdown("\\*前\\* [链](x) \\*后\\*");
        let index = tree.fragments.iter().position(|fragment| fragment.link.is_some()).unwrap();
        let mut replacement = tree.fragments[index].clone();
        replacement.text = "更长的链接".into();
        tree.replace_fragment_range(index..index + 1, vec![replacement]);
        assert_eq!(tree.serialize_markdown(), "\\*前\\* [更长的链接](x) \\*后\\*");
    }


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

    /// 评审维度一 4：`~~`/`==` 的开合此前只有空白判据（`delimiter_can_*` 的
    /// `_ => true` 分支），不判标点侧翼——`2~~(3+4)~~5` 在 UI 里被整段划掉，
    /// 而 GFM/pulldown 按字面处理，同一份文档界面与导出两个答案。
    /// 收敛为与 `*` 家族同一套星号侧翼规则（词内允许、标点侧不成翼）。
    #[test]
    fn tilde_and_equal_delimiters_require_punctuation_flanking() {
        for markdown in ["2~~(3+4)~~5", "2==(3+4)==5", "a~~[b]~~c", "a==[b]==c"] {
            let tree = InlineTextTree::from_markdown(markdown);
            assert_eq!(
                tree.visible_text(),
                markdown,
                "{markdown}：前接字母数字且后接标点的不成翼，应保持字面"
            );
            assert_eq!(tree.serialize_markdown(), markdown);
            assert!(
                tree.render_cache()
                    .spans()
                    .iter()
                    .all(|span| !span.style.strikethrough && !span.style.highlight),
                "{markdown}：不得产出删除线或高亮样式"
            );
        }
    }

    /// 星号规则允许词内配对（与下划线规则不同），收紧侧翼时不许收紧过头：
    /// `a~~b~~c`、`a==b==c` 仍是删除线/高亮。
    #[test]
    fn intraword_strikethrough_and_highlight_still_parse() {
        let tree = InlineTextTree::from_markdown("a~~b~~c");
        assert!(tree.render_cache().style_at(1).strikethrough);
        assert_eq!(tree.serialize_markdown(), "a~~b~~c");

        let tree = InlineTextTree::from_markdown("a==b==c");
        assert!(tree.render_cache().style_at(1).highlight);
        assert_eq!(tree.serialize_markdown(), "a==b==c");
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
    fn insertion_at_outer_end_of_terminal_bold_is_plain_text() {
        // 边界插入「返回普通文本」是函数注释里写明的口径，code 族照做了，
        // 粗斜体却延续片段样式——在强调句尾补一个句号会得到粗体句号，还得手动
        // 切回。非默认样式在边界一律不延续。
        let tree = InlineTextTree::from_markdown("**123**");
        let result = tree.replace_visible_range(
            tree.visible_len()..tree.visible_len(),
            "456",
            tree.attributes_for_insertion_at(tree.visible_len()),
        );
        assert_eq!(result.tree.serialize_markdown(), "**123**456");
    }

    #[test]
    fn insertion_at_outer_start_of_terminal_bold_is_plain_text() {
        let tree = InlineTextTree::from_markdown("**123**");
        let result = tree.replace_visible_range(0..0, "0", tree.attributes_for_insertion_at(0));
        assert_eq!(result.tree.serialize_markdown(), "0**123**");
    }

    #[test]
    fn insertion_at_outer_end_of_terminal_underline_is_plain_text() {
        // 下划线/上下标此前也落在「延续」分支，与 code 族不对称。
        let tree = InlineTextTree::from_markdown("<u>123</u>");
        let result = tree.replace_visible_range(
            tree.visible_len()..tree.visible_len(),
            "456",
            tree.attributes_for_insertion_at(tree.visible_len()),
        );
        assert_eq!(result.tree.serialize_markdown(), "<u>123</u>456");
    }

    #[test]
    fn footnote_branch_does_not_eat_image_syntax_with_caret_alt() {
        // `[` 的处理先脚注后链接，脚注分支不像链接分支那样检查前驱 `!`——
        // `![^1](img.png)` 被拆成字面 `!` + 脚注引用 + 字面括号，图片写法在
        // 界面上碎成感叹号、上标和括号文本。`!` 前驱时脚注放弃识别，原文交给
        // 图片路径。
        let source = "![^1](img.png)";
        let tree = InlineTextTree::from_markdown(source);
        assert_eq!(tree.visible_text(), source);
        assert!(
            tree.fragments.iter().all(|fragment| fragment.footnote.is_none()),
            "图片 alt 不该被读成脚注引用"
        );
        assert_eq!(tree.serialize_markdown(), source);
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

        assert!(!serialized.contains("*****34"));
        assert_eq!(reparsed.visible_text(), "1234");
        assert_eq!(reparsed.render_cache().spans(), tree.render_cache().spans());
    }

    #[test]
    fn empty_link_labels_remain_visible_and_round_trip() {
        // 空标题没有文本片段承载链接属性，归一化曾连同链接外壳一起吞掉。
        for markdown in [
            "[]()",
            "[]() tail",
            "before []() after",
            "末尾🙂[]()",
            "[](/文档.md)",
        ] {
            let tree = InlineTextTree::from_markdown(markdown);
            assert_eq!(tree.visible_text(), markdown, "空链接应保留可编辑原文");
            assert_eq!(tree.serialize_markdown(), markdown, "空链接往返不应丢字节");
        }
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
        // `\(...\)` 是无歧义的 TeX 定界符：`value \(42\).` 不许被 `$42$` 的货币启发
        // 误判成钱数（cases/07-numeric-math.md）。货币启发只许作用在
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
        // CommonMark 转义的是**全部 ASCII 标点**，不只 `\ * _ ~ [ ] ` ^ 与三个标签——
        // `\#` 该显示裸 `#`，不该多一个反斜杠（cases/10-escape.md）。
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
        // `Slash one\` + 换行是 CommonMark 硬换行（cases/10-line-break.md）：
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
    fn link_title_quotes_do_not_end_the_target_early() {
        let markdown = "[a](x \"ti)tle\")";
        let tree = InlineTextTree::from_markdown(markdown);
        assert_eq!(tree.visible_text(), "a");
        assert_eq!(
            tree.render_cache().link_hit_at(0),
            Some(&InlineLinkHit {
                prompt_target: "x".to_string(),
                open_target: "x".to_string(),
            })
        );
        assert_eq!(tree.serialize_markdown(), markdown);
    }

    #[test]
    fn empty_angle_target_is_a_link_but_invalid_angle_target_is_not() {
        let empty = InlineTextTree::from_markdown("[a](<>)");
        assert_eq!(
            empty.render_cache().link_hit_at(0),
            Some(&InlineLinkHit {
                prompt_target: String::new(),
                open_target: String::new(),
            })
        );

        let invalid = InlineTextTree::from_markdown("[a](<a<b>)");
        assert_eq!(
            invalid.render_cache().link_hit_at(0),
            None,
            "未转义的尖括号目标不是合法链接"
        );
    }

    #[test]
    fn parses_single_quoted_and_parenthesized_link_titles() {
        for markdown in ["[a](x 'title')", "[a](x (title))", "[a](x (a (b)))"] {
            let tree = InlineTextTree::from_markdown(markdown);
            assert_eq!(tree.visible_text(), "a", "{markdown}");
            assert_eq!(
                tree.render_cache().link_hit_at(0),
                Some(&InlineLinkHit {
                    prompt_target: "x".to_string(),
                    open_target: "x".to_string(),
                }),
                "{markdown}"
            );
        }
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
            "[collapsed reference][]",
            "collapsed 写法要原样写回"
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
            "[shortcut reference]",
            "shortcut 写法要原样写回"
        );
    }

    /// 三种引用写法各自原样写回：full / collapsed / shortcut 不再统一改写成
    /// full（未编辑保存要逐字节还原）。
    #[test]
    fn reference_link_forms_round_trip_without_rewriting() {
        let definitions = crate::components::markdown::link::parse_link_reference_definitions(
            "[alpha]: https://example.com/a\n[beta]: https://example.com/b\n[gamma]: https://example.com/c",
        );
        for source in ["[alpha][alpha]", "[beta][]", "[gamma]"] {
            let tree =
                InlineTextTree::from_markdown_with_link_references(source, &definitions);
            assert_eq!(
                tree.serialize_markdown(),
                source,
                "引用写法 {source} 应原样写回"
            );
        }
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
        // 词内下划线必须按字面保留：`**topic_embedding_attention 有轨迹无产物**` 曾渲染成
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

    /// CJK 语境里的强调：中文句子标点后面不跟空格，按 CommonMark 的侧翼判定收尾标记
    /// 永远关不上，于是 `**注意：**重点` 整段按字面画。按 markdown-cjk-friendly 的口径放
    /// 宽：CJK 标点不算标点，CJK 字可以顶替规则里要求的「空白或标点」。
    ///
    /// 只放宽 `*` 家族（含 `~~`/`==`）；`_` 仍走原判据，词内下划线必须继续按字面保留。
    #[test]
    fn cjk_punctuation_next_to_a_marker_still_forms_emphasis() {
        fn marked(sample: &str) -> (bool, String) {
            let tree = InlineTextTree::from_markdown(sample);
            let visible = tree
                .fragments
                .iter()
                .map(|fragment| {
                    let mut marks = String::new();
                    if fragment.style.bold {
                        marks.push('B');
                    }
                    if fragment.style.italic {
                        marks.push('I');
                    }
                    if fragment.style.strikethrough {
                        marks.push('S');
                    }
                    if fragment.style.highlight {
                        marks.push('H');
                    }
                    if marks.is_empty() {
                        marks.push('_');
                    }
                    format!("[{marks}]{}", fragment.text)
                })
                .collect::<Vec<_>>()
                .join("");
            let styled = tree.fragments.iter().any(|fragment| {
                fragment.style.bold
                    || fragment.style.italic
                    || fragment.style.strikethrough
                    || fragment.style.highlight
            });
            (styled, visible)
        }

        // 收尾标记紧跟在全角标点后面：中文写作的常态。
        for sample in [
            "测试**加粗：**测试",
            "测试*斜体：*测试",
            "测试~~删除：~~测试",
            "测试==高亮：==测试",
            "**注意：**这是重点",
            "（**强调**）中文",
            "中文「**引用**」句",
            "**这些星号会被识别为强调标记，而不是原样显示。**都是因为这个句子。",
            "**このアスタリスクは強調記号として認識されます。**この文のおかげで。",
            "他说**这段话，很重要，**你记一下",
            "断句。**下一句是重点。**再往后",
        ] {
            let (styled, visible) = marked(sample);
            assert!(styled, "{sample:?} 该成形，实际按字面：{visible}");
        }

        // 收尾标记后面跟 CJK 字、前面是 ASCII 标点：CommonMark 里也关不上。
        let (styled, _) = marked("**他强调(parenthesis)**가 있는 바람에");
        assert!(styled, "括号后接 CJK 字该成形");

        // 已经成形的写法不许被这次放宽弄坏。
        for sample in [
            "测试**加粗**测试",
            "测试 **加粗：** 测试",
            "测试**加粗**：测试",
            "**粗体** 中文",
        ] {
            let (styled, visible) = marked(sample);
            assert!(styled, "{sample:?} 本来成形，现在按字面：{visible}");
        }

        // 词内下划线仍按字面保留，外面那对 `**` 照旧成形。
        let (styled, visible) = marked("**topic_embedding_attention 有轨迹无产物**");
        assert!(styled, "粗体该成形");
        assert!(visible.contains("_attention"), "词内下划线按字面保留：{visible}");

        // 收紧过的地方保持：ASCII 标点两侧仍不成翼。
        for sample in ["2~~(3+4)~~5", "*a.*b", "a**b:**c"] {
            let (styled, visible) = marked(sample);
            assert!(!styled, "{sample:?} 该按字面，实际成形：{visible}");
            assert_eq!(visible, format!("[_]{sample}"), "{sample:?} 字面保留");
        }
        assert_eq!(
            marked("topic_embedding_attention").1,
            "[_]topic_embedding_attention",
            "词内下划线按字面保留"
        );
    }


    /// 同一条契约的删除线那一半：解析侧早把 `~~` 的侧翼放宽到 CJK 口径，序列化侧
    /// 却从来没有 `<del>` 候选——中文标点旁的删除线界面划掉、导出按字面（pulldown 的
    /// `delim_run_can_close` 对 `run_len > 1` 在 `prev = '：'` 时不认）。星号那条退路
    /// （HTML 栈 + 64 分 vs 侧翼不合格的 1000 分）对删除线无处可退。
    #[test]
    fn core_repair_manual_cjk_strikethrough_serializes_for_the_actual_html_exporter() {
        let mut failures = Vec::new();
        for (source, range, expected) in [
            ("测试删除：测试", 6..15, "<del>删除：</del>"),
            ("a struck z", 2..8, "<del>struck</del>"),
        ] {
            let mut tree = InlineTextTree::plain(source);
            tree.toggle_strikethrough(range);
            let markdown = tree.serialize_markdown();
            let html = crate::export::html::render_html(&markdown, &crate::theme::Theme::default_theme(), "格式兼容");
            let body = html.split_once("<body>").expect("导出正文").1;
            if !body.contains(expected) { failures.push((source, markdown.clone(), expected)); }
            let parsed = InlineTextTree::from_markdown(&markdown);
            assert_eq!(parsed.visible_text(), source);
            assert!(
                parsed.fragments.iter().any(|fragment| fragment.style.strikethrough),
                "写回去的那份还得读成删除线：{markdown:?}"
            );
        }
        assert!(failures.is_empty(), "手动删除线不能在导出时变成字面记号：{failures:?}");
    }

    /// 已知分叉：导出（pulldown-cmark）没有 CJK 开关，同一份文件在导出 HTML/PDF 里
    /// 仍是字面。这条用例把分叉摆在明面上，免得以后有人照导出「修」编辑器。
    #[test]
    fn the_export_still_shows_cjk_emphasis_literally() {
        let sample = "测试**加粗：**测试";
        assert!(
            InlineTextTree::from_markdown(sample)
                .fragments
                .iter()
                .any(|fragment| fragment.style.bold),
            "编辑器该成形"
        );
        assert!(
            !pulldown_cmark::Parser::new(sample).any(|event| matches!(
                event,
                pulldown_cmark::Event::Start(pulldown_cmark::Tag::Strong)
            )),
            "导出仍是字面"
        );
    }

    #[test]
    fn core_repair_manual_cjk_emphasis_serializes_for_the_actual_html_exporter() {
        let mut failures = Vec::new();
        for (source, range, bold, italic, expected) in [
            ("测试加粗：测试", 6..15, true, false, "<strong>加粗：</strong>"),
            ("测试斜体：测试", 6..15, false, true, "<em>斜体：</em>"),
            ("测试加粗：测试", 6..15, true, true, "<strong><em>加粗：</em></strong>"),
            ("a bold z", 2..6, true, false, "<strong>bold</strong>"),
        ] {
            let mut tree = InlineTextTree::plain(source);
            if bold { tree.toggle_bold(range.clone()); }
            if italic { tree.toggle_italic(range); }
            let markdown = tree.serialize_markdown();
            let html = crate::export::html::render_html(&markdown, &crate::theme::Theme::default_theme(), "格式兼容");
            let body = html.split_once("<body>").expect("导出正文").1;
            if !body.contains(expected) { failures.push((source, markdown.clone(), expected)); }
            let parsed = InlineTextTree::from_markdown(&markdown);
            assert_eq!(parsed.visible_text(), source);
            assert!(parsed.fragments.iter().any(|fragment| (!bold || fragment.style.bold) && (!italic || fragment.style.italic)));
        }
        assert!(failures.is_empty(), "手动格式不能在导出时变成字面记号：{failures:?}");
    }

#[test]
fn deeply_nested_inline_html_falls_back_to_literal_text() {
    let depth = 100;
    let markdown = format!("{}x{}", "<u>".repeat(depth), "</u>".repeat(depth));
    let tree = InlineTextTree::from_markdown(&markdown);
    assert!(
        tree.visible_text().contains("<u>"),
        "超过递归上限的容器标签没有按字面输出"
    );
}

#[test]
#[ignore = "慢用例（>1s）：本地默认跳过，CI 跑"]
fn extremely_deep_inline_html_does_not_overflow_the_stack() {
    let depth = 12_000;
    let markdown = format!("{}x{}", "<u>".repeat(depth), "</u>".repeat(depth));
    let tree = InlineTextTree::from_markdown(&markdown);
    assert!(tree.visible_text().contains('x'));
    assert!(tree.visible_text().contains("<u>"));
}

/// `<br>` 的写法在模型里只记「这里有一个换行」，序列化统一写回 `<br>`
/// （md 模型文档「语义相同、写法可能不同」一节）。这条把规范化固定在
/// 文档声明的一致口径上，避免有人半路改成保真或改成别的字面量。
#[test]
fn br_variants_are_normalized_to_the_documented_form() {
    for source in ["a<br>b", "a<br/>b", "a<br />b", "a<BR>b"] {
        let tree = InlineTextTree::from_markdown(source);
        assert_eq!(tree.visible_text(), "a\nb", "{source} 应解析成换行");
        assert_eq!(tree.serialize_markdown(), "a<br>b", "{source} 的写法");
    }
}

/// HTML 行内写法在模型里塌成同一套样式属性，序列化重建为等价 markdown 记号
/// 或 `<u>`（模型文档「语义相同、写法可能不同」一节）。`<sup>` 没有前置字母
/// 数字时按 stacks 的侧翼规则保留 HTML 写法。这条把既有规范化固定进文档
/// 口径，避免被当成静默改写反复报修，也防止有人半路改成保真。
#[test]
fn html_inline_forms_are_normalized_to_the_documented_equivalents() {
    for (source, expected) in [
        ("<strong>x</strong>", "**x**"),
        ("<b>x</b>", "**x**"),
        ("<em>y</em>", "*y*"),
        ("<i>y</i>", "*y*"),
        ("<del>x</del>", "~~x~~"),
        ("<sup>x</sup>", "<sup>x</sup>"),
        ("<ins>x</ins>", "<u>x</u>"),
    ] {
        let tree = InlineTextTree::from_markdown(source);
        assert_eq!(
            tree.serialize_markdown(),
            expected,
            "{source} 的规范化口径"
        );
    }
}
