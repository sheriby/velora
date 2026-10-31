    use super::super::InlineTextTree;
    use crate::components::markdown::inline::delimiters::CLOSE_SCAN_STEPS;

    /// 一段未闭合的强调记号：闭合预扫不能对每个 opener 都把剩余 token
    /// 重扫一遍，否则长段落变成 O(n²)、每敲一键都要付。
    #[test]
    fn unclosed_emphasis_scans_stay_linear_in_paragraph_length() {
        let source = ".*a".repeat(400);
        CLOSE_SCAN_STEPS.with(|counter| counter.set(0));
        let tree = InlineTextTree::from_markdown(&source);
        assert_eq!(tree.visible_text(), source, "未闭合记号应原样保留");
        let steps = CLOSE_SCAN_STEPS.with(|counter| counter.get());
        assert!(
            steps <= source.len() as u64 * 4,
            "未闭合定界符的预扫步数应随段落线性增长：{steps} 步 / {} 字节",
            source.len()
        );
    }

    #[test]
    fn review_escaped_dollar_neighbor_keeps_inline_math_delimiter() {
        // 转义美元符不是定界符游程的一部分，不能阻止相邻的行内起点或闭合。
        for source in ["$x\\$$", "\\$$x$"] {
            let tree = InlineTextTree::from_markdown(source);
            assert!(tree.render_cache().spans().iter().any(|span| span.math.is_some()), "{source}");
            assert_eq!(tree.serialize_markdown(), source);
        }
    }

    #[test]
    fn review_double_dollars_do_not_open_inline_math_at_the_second_dollar() {
        // 双美元符的第二位不能另作行内起点，否则会把闭合符的一位吃进公式体。
        let tree = InlineTextTree::from_markdown("其中 $$x>0$$ 成立");
        assert!(tree.render_cache().spans().iter().all(|span| span.math.is_none()));
    }

    #[test]
    fn review_emphasis_punctuation_flanking_stays_literal() {
        // 星号曾只检查空白，在算式中把标点旁的正文误判为强调。
        for source in ["2*(3+4)*5", "a*(b)*c", "~~ x~~", "== x ==", "==x =="] {
            assert_eq!(InlineTextTree::from_markdown(source).visible_text(), source);
        }
    }

    #[test]
    fn review_link_destinations_survive_serialization() {
        // 解析剥掉尖括号和转义后，序列化必须重建合法的目标写法。
        for source in ["[a](<报告 2026.pdf>)", "[a](<x(y)>)", "[a](x\\)y)"] {
            let tree = InlineTextTree::from_markdown(source);
            let serialized = tree.serialize_markdown();
            let reparsed = InlineTextTree::from_markdown(&serialized);
            assert_eq!(reparsed.render_cache().link_at(0), tree.render_cache().link_at(0));
            assert!(pulldown_cmark::Parser::new(&serialized).any(|event| matches!(
                event, pulldown_cmark::Event::Start(pulldown_cmark::Tag::Link { .. })
            )), "目标应当仍是合法 Markdown 链接：{serialized}");
        }
    }

    #[test]
    fn review_code_span_backslash_before_closer_round_trips() {
        // 代码跨度内反斜杠不转义闭合反引号；预判曾跳过闭合符。
        for markdown in ["`C:\\`", "`中文\\`", "前 `x\\` 后"] {
            let tree = InlineTextTree::from_markdown(markdown);
            assert!(tree.render_cache().spans().iter().any(|span| span.style.code));
            assert_eq!(tree.serialize_markdown(), markdown);
        }
    }
    


    #[test]
    fn escaped_backslash_before_a_link_round_trips() {
        // markdown 空间编辑把键入的反斜杠翻倍拼进源码（`\\[a](...)`），重解析
        // 出一个带转义账的反斜杠；序列化必须原样写回两个字符，否则重读时
        // `\[` 被当转义吃掉，用户的反斜杠凭空消失。
        let markdown = "\\\\[a](https://example.com) tail";
        let tree = InlineTextTree::from_markdown(markdown);
        assert_eq!(tree.visible_text(), "\\a tail");
        assert_eq!(tree.serialize_markdown(), markdown);
    }

    #[test]
    fn serialize_round_trips_the_original_bytes_for_literal_markers() {
        // 回归（报修 2026-10-05：搜索跳转高亮错位一个字符）：裸的 `~`/`*`/
        // 反引号不许被洗成 `\~`/`\*`/`\``——多出的反斜杠让块 markdown 与缓冲区
        // 字节错位，命中换算整体漂移。逐条断言：parse∘serialize == 恒等。
        for text in [
            "单 bin crate（~95k 行（含测试）",
            "2 * 3 与 4_5",
            "波浪号 ~ 和上标 ^ 记号",
            "未闭合的 `code",
            "Windows 路径 C:\\Users\\doc.md",
        ] {
            let tree = InlineTextTree::from_markdown(text);
            assert_eq!(tree.serialize_markdown(), text, "写法被改写：{text:?}");
        }
        // 源码本就带转义的仍然原样保留。
        for text in ["\\*不强调\\*", "a\\\\b", "\\</u>标签"] {
            let tree = InlineTextTree::from_markdown(text);
            assert_eq!(tree.serialize_markdown(), text, "转义写法被改写：{text:?}");
        }
    }

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
            "**a[^1]b** and **a$x$b**",
            "_前$x^2$后_[^2]尾",
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

    /// 引用式链接的三种写法在两条序列化实现里都要原样写回（full/collapsed/
    /// shortcut），且彼此逐字节一致。
    #[test]
    fn reference_link_forms_match_between_plain_and_offset_map_serializers() {
        let definitions = crate::components::markdown::link::parse_link_reference_definitions(
            "[a]: https://x\n[b]: https://y\n[c]: https://z",
        );
        for source in ["[a][a]", "[b][]", "[c]"] {
            let tree = InlineTextTree::from_markdown_with_link_references(source, &definitions);
            assert_eq!(tree.serialize_markdown(), source, "{source} 应原样写回");
            assert_eq!(
                tree.serialize_markdown(),
                tree.markdown_offset_map().markdown,
                "{source} 两条序列化实现不一致"
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
        // 代码段内部不解析任何 Markdown：这里的 `![` 只是字面文本。
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
        // 序列化保真：空格旁的孤立记号是语法候选，写回不许洗成转义——
        // 多出的反斜杠会把「块 markdown ↔ 缓冲区字节」的对齐整体推歪。
        assert_eq!(tree.serialize_markdown(), "* a * _ b _");
    }

    #[test]
    fn preserves_unclosed_markers_as_literal_text() {
        let tree = InlineTextTree::from_markdown("1**234");

        assert_eq!(tree.visible_text(), "1**234");
        assert_eq!(tree.serialize_markdown(), "1**234");
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
        // 序列化保真：不完整的强调开头原样写回。
        assert_eq!(leading.serialize_markdown(), "**word");

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

    #[test]
    fn core_repair_source_mapping_preserves_visible_and_link_target_offsets() {
        use super::super::{InlineSourceOffsetMap, LinkReferenceDefinitions};
        for source in [
            "a <strong>b</strong> Z",
            "a ` b ` Z",
            "a ` b\r\nc ` Z",
            "a <br />Z",
            "甲 <strong>乙</strong> Z",
            "a [<strong>b</strong>](https://example.com \"title\") Z",
        ] {
            let mapping = InlineSourceOffsetMap::parse(source, &LinkReferenceDefinitions::default());
            let source_offset = source.rfind('Z').unwrap();
            let markdown_offset = mapping.markdown.rfind('Z').unwrap();
            assert_eq!(mapping.markdown_to_source[markdown_offset], source_offset, "{source}");
            assert_eq!(mapping.source_to_markdown[source_offset], markdown_offset, "{source}");
            assert!(mapping.markdown_to_source.windows(2).all(|pair| pair[0] <= pair[1]), "{source}");
            assert!(mapping.source_to_markdown.windows(2).all(|pair| pair[0] <= pair[1]), "{source}");
            for (offset, mapped) in mapping.markdown_to_source.iter().enumerate() {
                if mapping.markdown.is_char_boundary(offset) {
                    assert!(source.is_char_boundary(*mapped), "{source}: {offset} -> {mapped}");
                }
            }
            if let Some(offset) = mapping.markdown.find("example") {
                assert_eq!(mapping.markdown_to_source[offset], source.find("example").unwrap());
            }
        }
    }

    #[test]
    fn core_repair_code_normalization_keeps_following_escapes_at_their_characters() {
        let tree = InlineTextTree::from_markdown("` b ` \\*Z");
        assert_eq!(tree.serialize_markdown(), "`b` \\*Z");
    }

    #[test]
    fn link_label_inside_underscore_emphasis_keeps_its_marker() {
        // 链接标签不经 emit_token 输出，而是重解析后由
        // apply_extra_style_to_fragments 并入外层样式；那份合并判据唯独漏了
        // emphasis_marker——外层 `__` 的写法到标签片段处变成 None，序列化按
        // 规范写 `*`，`__[a](u)__` 一次保存就被改写成 `[**a**](u)`。
        //
        // 修复后写法记号保住（`[__a__](u)`），但记号**位置**（强调包在链接外
        // 还是写在标签内）在树模型里不可区分：`__[a](u)__` 与 `[__a__](u)`
        // 解析出同一棵树，序列化按「写在标签内」的既有约定输出。要逐字节
        // 还原两种写法得给强调记来源维度（与 full/collapsed 同类），
        // 这里先钉住：写法记号不丢、往返两轮后稳定。
        let first = InlineTextTree::from_markdown("__[a](u)__").serialize_markdown();
        assert_eq!(first, "[__a__](u)", "下划线写法被洗成星号");
        let second = InlineTextTree::from_markdown(&first).serialize_markdown();
        assert_eq!(second, first, "往返不收敛");

        let first = InlineTextTree::from_markdown("*[a](u)*").serialize_markdown();
        assert_eq!(first, "[*a*](u)", "星号斜体写法被洗掉");
        let second = InlineTextTree::from_markdown(&first).serialize_markdown();
        assert_eq!(second, first);
    }

    #[test]
    fn link_title_escapes_are_involutive() {
        // title 侧解析只做字面截取、序列化却写时转义，含 `\`/`"` 的标题
        // 每保存一次多一层反斜杠，永不收敛。解析反转义之后「解析反转义 ∘
        // 序列化转义」必须是对合；无歧义的写法还要逐字节还原。
        for source in [
            "[a](u \"ti\\\"tle\")",
            "[a](u \"尾部\\\\\")",
            "[a](u \"C:\\Users\")",
        ] {
            let first = InlineTextTree::from_markdown(source).serialize_markdown();
            assert_eq!(first, source, "title 写法被改写：{source:?}");
            let second = InlineTextTree::from_markdown(&first).serialize_markdown();
            assert_eq!(second, first, "title 转义不收敛：{first:?}");
        }
        // 源码里写成 `\\` 的反斜杠解析后与裸 `\` 停在同一个存储值上（转义信息
        // 在解析时丢失），第一次保存会收敛成最短写法；此后必须稳定不再增长。
        let growing = "[a](u \"C:\\\\path\")";
        let first = InlineTextTree::from_markdown(growing).serialize_markdown();
        let second = InlineTextTree::from_markdown(&first).serialize_markdown();
        let third = InlineTextTree::from_markdown(&second).serialize_markdown();
        assert_eq!(second, first, "title 仍在增长：{first:?} -> {second:?}");
        assert_eq!(third, second);
    }

    #[test]
    fn nested_link_does_not_lose_the_inner_target() {
        // label 扫描允许再含 `[...]`，内层链接解析出来后又被外层无条件
        // 覆盖——`[a[b](c) d](e)` 不编辑保存就变成 `[ab d](e)`，内层目标 `c`
        // 净丢失。与 CommonMark 对齐（链接不可嵌套、最内层优先）：标签里已有
        // 成链片段时放弃外层识别，`[` 按字面走，未编辑内容逐字节还原。
        let source = "[a[b](c) d](e)";
        let tree = InlineTextTree::from_markdown(source);
        assert_eq!(tree.serialize_markdown(), source, "嵌套链接被改写且目标丢失");
        assert_eq!(tree.visible_text(), "[ab d](e)");
        // 可见文本里前两个字节是字面 `[`、`a`，内层链接的 `b` 在字节 2。
        assert_eq!(
            tree.render_cache()
                .link_hit_at(2)
                .map(|hit| hit.open_target.clone()),
            Some("c".to_string()),
            "内层链接应当保留自己的目标"
        );
    }

    #[test]
    fn adjacent_links_with_the_same_target_stay_separate() {
        // 链接是纯值类型按字段比较，两个同目标链接恒等——解析合并与
        // 序列化 run 分组都把它们并成一个，`[a](u)[b](u)` 保存成 `[ab](u)`，
        // 两个可独立编辑/点击的链接塌成一个。每次链接语法要有自己的身份，
        // 等值比较才分得开。
        for source in ["[a](u)[b](u)", "[a](u)[b](u)[c](u)", "<http://a><http://a>"] {
            let tree = InlineTextTree::from_markdown(source);
            assert_eq!(tree.serialize_markdown(), source, "相邻链接被合并：{source:?}");
            let second =
                InlineTextTree::from_markdown(&tree.serialize_markdown()).serialize_markdown();
            assert_eq!(second, source, "往返不收敛：{source:?}");
        }
    }

    #[test]
    fn footnote_and_math_inside_emphasis_do_not_cut_the_run() {
        // run 分组把脚注/公式片段绝对排除，一个强调 span 被拆成两段、
        // 脚注/公式夹在中间的裸文本——`**a[^1]b**` 保存成 `**a**[^1]**b**`，
        // 未编辑字节被改写。markdown 本身允许这种写法，序列化应原样嵌回当前栈内。
        for source in ["**a[^1]b**", "**a$x$b**", "_前$x^2$后_"] {
            let first = InlineTextTree::from_markdown(source).serialize_markdown();
            assert_eq!(first, source, "强调 run 被脚注/公式切断：{source:?}");
            let second = InlineTextTree::from_markdown(&first).serialize_markdown();
            assert_eq!(second, first, "往返不收敛：{source:?}");
        }
    }

    #[test]
    fn literal_text_offset_map_has_no_holes_inside_multibyte_chars() {
        // visible→markdown 映射只填字符起始字节，多字节字符内部字节槽留在
        // 初始化的 0 上——把非边界偏移喂进反查的调用方会把光标/高亮打到段首。
        // 整表应当有值：单调不减、除首字符外不回 0、落点都在字符边界上。
        let tree = InlineTextTree::from_markdown("a中文**粗**体");
        let map = tree.markdown_offset_map();
        let markdown = map.markdown().to_string();
        let mut previous = 0usize;
        for offset in 0..=tree.visible_len() {
            let mapped = map.visible_to_markdown_offset(offset);
            assert!(mapped >= previous, "映射在 {offset} 处回退：{mapped} < {previous}");
            assert!(offset == 0 || mapped > 0, "映射在 {offset} 处是空洞（0）");
            assert!(
                markdown.is_char_boundary(mapped),
                "映射 {offset} -> {mapped} 不在 markdown 字符边界"
            );
            previous = mapped;
        }
    }

    #[test]
    fn footnote_offset_map_stays_on_char_boundaries_for_multibyte_ids() {
        // 脚注 run 的比例插值没做字符边界收敛，`[^注意]` 解析成上标后
        // 插值落点砸进「注」字内部——后续对 markdown 切片的代码在 release 下
        // 直接 panic。插值结果必须统一收敛到目标串的字符边界。
        let mut tree = InlineTextTree::from_markdown("前[^注意]后");
        tree.apply_footnote_reference_state(|_| Some((1, 0)));
        let map = tree.markdown_offset_map();
        let markdown = map.markdown().to_string();
        let visible = tree.visible_text();
        for offset in 0..=tree.visible_len() {
            let mapped = map.visible_to_markdown_offset(offset);
            assert!(
                markdown.is_char_boundary(mapped),
                "可见 {offset} -> {mapped} 落在 markdown 多字节字符内部"
            );
        }
        for offset in 0..=markdown.len() {
            let mapped = map.markdown_to_visible_offset(offset);
            assert!(
                visible.is_char_boundary(mapped),
                "markdown {offset} -> {mapped} 落在上标内部"
            );
        }
    }

    #[test]
    fn link_label_keeps_escaped_markers_through_round_trip() {
        // 链接标签在归一化里被重解析成一棵新树：内层解析记下的转义账本
        // （哪些字符是「源码用反斜杠换来的字面记号」）必须跟着片段一起并回
        // 外层输出树，否则序列化不再补反斜杠，`\*` 在盘上洗成 `*`，
        // 重新打开后裸星号配对成强调，语义被改写。
        let source = "[a \\*b c\\*](x)";
        let tree = InlineTextTree::from_markdown(source);

        assert_eq!(tree.visible_text(), "a *b c*");
        assert_eq!(tree.serialize_markdown(), source);
    }

    #[test]
    fn reference_style_link_label_keeps_escaped_markers_through_round_trip() {
        let source = "[a \\*b][ref]\n\n[ref]: /x";
        let tree = InlineTextTree::from_markdown_with_link_references(
            source,
            &crate::components::markdown::inline::LinkReferenceDefinitions::default(),
        );

        assert_eq!(tree.serialize_markdown(), source);
    }

    /// CommonMark 的「rule of 3」：既能开又能闭的定界串，两串长度之和为 3 的
    /// 倍数时禁止配对。`**a*b**` 整段是粗体（单星号按字面），`*a**b*` 整段是
    /// 斜体（双星号按字面）；此前分别读成「字面 + 斜体 + 字面」与两段斜体，
    /// 可见文本与导出都对不上。
    #[test]
    fn rule_of_three_keeps_mixed_strong_and_emphasis_markers_matching_commonmark() {
        let bold = InlineTextTree::from_markdown("**a*b**");
        assert_eq!(bold.visible_text(), "a*b");
        assert!(
            bold.render_cache()
                .spans()
                .iter()
                .any(|span| span.style.bold && span.range == (0.."a*b".len())),
            "整段应是粗体：{:?}",
            bold.render_cache().spans()
        );
        assert_eq!(bold.serialize_markdown(), "**a*b**");

        let italic = InlineTextTree::from_markdown("*a**b*");
        assert_eq!(italic.visible_text(), "a**b");
        assert!(
            italic
                .render_cache()
                .spans()
                .iter()
                .any(|span| span.style.italic && span.range == (0.."a**b".len())),
            "整段应是斜体：{:?}",
            italic.render_cache().spans()
        );
        assert_eq!(italic.serialize_markdown(), "*a**b*");
    }

    /// 可见↔markdown 偏移映射按树缓存：同一棵树反复查询只建一次，克隆出的
    /// 新树各自建一份。光标移动、选区拖拽查的都是同一棵持久树。
    #[test]
    fn offset_map_is_built_once_per_tree_and_shared_between_queries() {
        let tree = InlineTextTree::from_markdown("前 **粗** 与 [链接](https://a.test) 后");
        crate::components::markdown::inline::reset_offset_map_builds();
        let first = tree.markdown_offset_map();
        let second = tree.markdown_offset_map();
        assert!(
            std::sync::Arc::ptr_eq(&first, &second),
            "同一棵树应复用同一份映射"
        );
        assert_eq!(
            crate::components::markdown::inline::offset_map_builds(),
            1,
            "同一棵树只该建一次表"
        );
        let cloned = tree.clone();
        let _ = cloned.markdown_offset_map();
        assert_eq!(
            crate::components::markdown::inline::offset_map_builds(),
            2,
            "克隆出的树各自建一份"
        );
    }
