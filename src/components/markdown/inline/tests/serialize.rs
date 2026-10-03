    use super::super::InlineTextTree;
    


    #[test]
    fn serialize_markdown_matches_offset_map() {
        // 无映射快路径必须与映射版本逐字节一致：保存/撤销/导出走前者，
        // 偏移映射走后者，两者一旦漂移，用户内容就会被改写。
        let corpus = [
            "plain text",
            "**bold** and *italic*",
            "_underline_ and __also__",
            "topic_embedding_attention stays",
            "code `x ` and ``y``",
            "<strong>x</strong> <em>y</em> <u>z</u>",
            "[label](https://example.com)",
            "![alt](pic.png)",
            "math $x^2$ inline",
            "super^script^ and ~~strike~~",
            "back\\slash and \\*star",
            "footnote[^1] and more",
            "中文与 English mixed **粗** 文本",
            "`![alt](p.png){width=50%}`",
            "a<u>b</u>c<strong>d</strong>e",
            "**bold `code` inside** and _nested <em>html</em>_",
            "`  padded code  `",
            "link with [**bold** label](https://a.b/c?d=e#f)",
        ];
        for text in corpus {
            let tree = InlineTextTree::from_markdown(text);
            assert_eq!(
                tree.serialize_markdown(),
                tree.markdown_offset_map().markdown,
                "无映射序列化与映射版本不一致：{text:?}"
            );
        }
    }

    #[test]
    fn inline_code_does_not_force_the_mixed_visual_path() {
        // 行内代码曾为了缩小字号而走「混合分段」渲染路径（逐词一个元素），
        // 该路径只在失焦时使用，聚焦后换成可编辑文本，于是点击时字号跳变。
        // 现在行内代码不再触发该路径：显示与编辑由同一套文本元素渲染。
        assert!(
            !InlineTextTree::from_markdown("alpha `code` beta").has_mixed_inline_visuals(),
            "行内代码不应再触发混合分段路径"
        );
        // 数学、上下标、行内图片仍然需要混合路径（它们有真正的非文本视觉）。
        assert!(InlineTextTree::from_markdown("alpha $x$ beta").has_mixed_inline_visuals());
        assert!(InlineTextTree::from_markdown("alpha^sup^ beta").has_mixed_inline_visuals());
        assert!(InlineTextTree::from_markdown("alpha ![a](b.png) beta").has_mixed_inline_visuals());
        // 代码段内部不解析任何 Markdown：这里的 `![` 只是字面文本（用户报修：
        // 表格单元格里被反引号包住的图片语法渲染成了「无法加载图片」占位框）。
        assert!(
            !InlineTextTree::from_markdown("源码 `![alt](path){width=NN%}`，100%")
                .has_mixed_inline_visuals(),
            "行内代码里的图片语法是字面文本"
        );
        let tree = InlineTextTree::from_markdown("前 `![alt](p.png) [a](b) $c$ ^d^` 后");
        assert_eq!(
            tree.visible_text(),
            "前 ![alt](p.png) [a](b) $c$ ^d^ 后",
            "代码段内容必须原样保留，不被当作图片/链接/数学/上下标解析"
        );
        assert_eq!(tree.serialize_markdown(), "前 `![alt](p.png) [a](b) $c$ ^d^` 后");
    }

    #[test]
    fn parses_supported_styles_and_serializes_canonically() {
        let tree = InlineTextTree::from_markdown("1**23**4*56*7<u>89</u>0***ab***<u>*cd*</u>");
        let serialized = tree.serialize_markdown();
        let reparsed = InlineTextTree::from_markdown(&serialized);

        assert_eq!(tree.visible_text(), "1234567890abcd");
        assert_eq!(reparsed.visible_text(), tree.visible_text());
        assert_eq!(reparsed.render_cache().spans(), tree.render_cache().spans());
    }

    #[test]
    fn plain_text_fast_serialization_matches_offset_mapping() {
        for text in [
            "",
            "中文 English 和 emoji ✨",
            "plain [brackets] and ![image](x.png)",
            "literal * _ ~ ^ ` \\",
            "<strong>literal tag</strong>",
        ] {
            let tree = InlineTextTree::plain(text);
            assert_eq!(
                tree.serialize_markdown(),
                tree.markdown_offset_map().markdown()
            );
        }
    }

    #[test]
    fn parses_underscore_emphasis_and_keeps_the_underscore_delimiters() {
        let tree = InlineTextTree::from_markdown("_a_ __b__");

        assert_eq!(tree.visible_text(), "a b");
        // 写法是原文的一部分：下划线强调序列化回去还是下划线，不再规范成星号。
        assert_eq!(tree.serialize_markdown(), "_a_ __b__");
    }

    #[test]
    fn emphasis_delimiters_surrounded_by_spaces_stay_literal() {
        let tree = InlineTextTree::from_markdown("* a * _ b _");

        assert_eq!(tree.visible_text(), "* a * _ b _");
        assert_eq!(tree.serialize_markdown(), "\\* a \\* \\_ b \\_");
    }

    #[test]
    fn preserves_unclosed_markers_as_literal_text() {
        let tree = InlineTextTree::from_markdown("1**234");

        assert_eq!(tree.visible_text(), "1**234");
        assert_eq!(tree.serialize_markdown(), "1\\*\\*234");
    }

    #[test]
    fn empty_emphasis_spans_stay_literal() {
        // `**`, `* *`, or `**word` must not be swallowed as an empty emphasis
        // span; the markers stay literal until a non-empty body is closed.
        for input in ["*", "**", "***", "****", "~~~~", "__"] {
            let tree = InlineTextTree::from_markdown(input);
            assert_eq!(tree.visible_text(), input, "input {input:?} lost markers");
        }

        let leading = InlineTextTree::from_markdown("**word");
        assert_eq!(leading.visible_text(), "**word");
        assert_eq!(leading.serialize_markdown(), "\\*\\*word");

        let trailing = InlineTextTree::from_markdown("**word*");
        assert_eq!(trailing.visible_text(), "**word*");
    }

    #[test]
    fn non_empty_emphasis_still_parses_after_empty_guard() {
        let bold = InlineTextTree::from_markdown("**word**");
        assert_eq!(bold.visible_text(), "word");
        assert_eq!(bold.serialize_markdown(), "**word**");

        let italic = InlineTextTree::from_markdown("*a*");
        assert_eq!(italic.visible_text(), "a");
        assert_eq!(italic.serialize_markdown(), "*a*");

        let single_char_bold = InlineTextTree::from_markdown("**a**");
        assert_eq!(single_char_bold.visible_text(), "a");
        assert_eq!(single_char_bold.serialize_markdown(), "**a**");

        let bold_italic = InlineTextTree::from_markdown("***x***");
        assert_eq!(bold_italic.visible_text(), "x");
        let spans = bold_italic.render_cache();
        assert!(
            spans
                .spans()
                .iter()
                .all(|span| span.style.bold && span.style.italic)
        );
    }

    #[test]
    fn unclosed_multichar_opener_stays_fully_literal() {
        // While typing `**bold**`, the intermediate `**bold*` must stay literal;
        // otherwise the second `*` opens an italic span and the bold is lost.
        let partial = InlineTextTree::from_markdown("**bold*");
        assert_eq!(partial.visible_text(), "**bold*");
        assert!(
            partial
                .render_cache()
                .spans()
                .iter()
                .all(|span| !span.style.italic && !span.style.bold),
            "`**bold*` must be plain literal, not italic"
        );

        // The completed marker still resolves to bold (not italic).
        let complete = InlineTextTree::from_markdown("**bold**");
        assert_eq!(complete.visible_text(), "bold");
        assert!(
            complete
                .render_cache()
                .spans()
                .iter()
                .all(|span| span.style.bold && !span.style.italic),
            "`**bold**` must be bold, not italic"
        );

        // A genuine single-`*` italic opener is unaffected by the multi-char rule.
        let italic = InlineTextTree::from_markdown("*word*");
        assert_eq!(italic.visible_text(), "word");
        assert!(
            italic
                .render_cache()
                .spans()
                .iter()
                .all(|span| span.style.italic && !span.style.bold),
            "`*word*` must stay italic"
        );

        // Other unclosed multi-char openers stay literal as a unit too.
        for input in ["__bold_", "~~strike~"] {
            let tree = InlineTextTree::from_markdown(input);
            assert_eq!(tree.visible_text(), input, "input {input:?} lost markers");
            assert!(
                tree.render_cache()
                    .spans()
                    .iter()
                    .all(|span| !span.style.italic
                        && !span.style.bold
                        && !span.style.strikethrough),
                "input {input:?} should be plain literal"
            );
        }
    }

    #[test]
    fn empty_code_span_is_unaffected_by_emphasis_guard() {
        // The empty-emphasis guard must not touch code spans. `*` inside a code
        // span stays literal and the span round-trips.
        let tree = InlineTextTree::from_markdown("`*`");
        assert_eq!(tree.visible_text(), "*");
        assert_eq!(tree.serialize_markdown(), "`*`");
    }

    #[test]
    fn preserves_escaped_marker_sequences_as_literal_text() {
        let tree = InlineTextTree::from_markdown("\\*\\*\\<u>text\\</u>\\\\");

        assert_eq!(tree.visible_text(), "**<u>text</u>\\");
        assert_eq!(tree.serialize_markdown(), "\\*\\*\\<u>text\\</u>\\\\");
    }

    #[test]
    fn preserves_tibetan_spaces_through_inline_round_trip() {
        let markdown = "༄༅།།དཔལ་ལྡན་རྩ་བའི་བླ་མ་རིན་པོ་ཆེ།། བདག་གི་སྤྱི་བོར་པདྨའི་གདན་བཞུགས་ནས།། ";
        let tree = InlineTextTree::from_markdown(markdown);
        let serialized = tree.serialize_markdown();

        assert_eq!(tree.visible_text(), markdown);
        assert!(tree.visible_text().contains("།། བདག"));
        assert!(tree.visible_text().ends_with(' '));
        assert_eq!(serialized, markdown);
        assert_eq!(
            InlineTextTree::from_markdown(&serialized).visible_text(),
            markdown
        );
    }

    #[test]
    fn preserves_chinese_spaces_through_inline_round_trip() {
        let markdown = "中文 文本 ";
        let tree = InlineTextTree::from_markdown(markdown);

        assert_eq!(tree.visible_text(), markdown);
        assert_eq!(tree.serialize_markdown(), markdown);
    }

    #[test]
    fn emphasis_keeps_the_delimiter_it_was_written_with() {
        // 定界符的写法是原文的一部分。解析认得 `_`，序列化却一律写回 `*`，于是任何一次
        // 整块落笔都会把用户的下划线强调改成星号（`__粗__` 变 `**粗**`）。
        for text in [
            "_italic_ and __bold__",
            "强调 __下划线__ 尾巴",
            "*star* and _under_",
            "***both*** and ___both___",
            "混合 _一_ 与 **二** 与 __三__",
        ] {
            assert_eq!(
                InlineTextTree::from_markdown(text).serialize_markdown(),
                text,
                "下划线写法的强调被改成了星号"
            );
        }
    }
