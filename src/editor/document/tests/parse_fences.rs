    use gpui::{AppContext, TestAppContext};

    use super::super::{
        collect_block_html_region, find_matching_closing_fence, is_closing_fence,
        is_reference_definition_start, parse_list_marker, parse_opening_fence,
        strip_indented_code_prefix, strip_one_quote_level,
    };
    use crate::components::{BlockKind, Editor};

    #[test]
    fn closing_fence_follows_the_commonmark_run_rule() {
        let opener = parse_opening_fence("````rust").expect("opening fence");

        assert!(is_closing_fence("````", &opener));
        // 长一点的闭合围栏照样收尾（以前这里 `!is_closing_fence("`````")` 把
        // CommonMark 合法的长闭合判成不合法，```` ```text ```` 用 ````` 收尾的块永不闭合）。
        assert!(is_closing_fence("`````", &opener));
        assert!(is_closing_fence("  ````   ", &opener));
        // 短一档、带信息串、换一种围栏符都只是内容，不是闭合。
        assert!(!is_closing_fence("```", &opener));
        assert!(!is_closing_fence("```` ``", &opener));
        assert!(!is_closing_fence("~~~~", &opener));
    }

    #[test]
    fn tilde_fence_closes_at_an_equal_or_longer_run() {
        let opener = parse_opening_fence("~~~js").expect("opening fence");

        assert!(is_closing_fence("~~~~", &opener));
        assert!(is_closing_fence("~~~", &opener));
        assert!(!is_closing_fence("~~", &opener));
        assert!(!is_closing_fence("~~~ tail", &opener));
    }

    #[test]
    fn fence_detection_rejects_indent_beyond_three_spaces() {
        assert!(parse_opening_fence("    ```rust").is_none());

        let opener = parse_opening_fence("```rust").expect("opening fence");
        assert!(!is_closing_fence("    ```", &opener));
    }

    #[test]
    fn unmatched_opening_fence_does_not_form_code_block() {
        let lines = vec![
            "```rust".to_string(),
            "fn main() {}".to_string(),
            "plain tail".to_string(),
        ];
        let opener = parse_opening_fence(&lines[0]).expect("opening fence");
        assert_eq!(find_matching_closing_fence(&lines, 0, &opener), None);
    }

    #[gpui::test]
    async fn yaml_frontmatter_is_preserved_as_an_opaque_block(cx: &mut TestAppContext) {
        let source = "---\ntitle: Notes\ntags: [writing]\n---\n\n# Heading\n\nBody.";
        let editor = cx.new(|cx| Editor::from_markdown(cx, source.into(), None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert!(visible.len() >= 2);
            assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::FrontMatter);
            assert_eq!(
                visible[0].entity.read(cx).display_text(),
                "---\ntitle: Notes\ntags: [writing]\n---"
            );
            assert!(visible
                .iter()
                .any(|block| block.entity.read(cx).kind() == BlockKind::Heading { level: 1 }));
            assert_eq!(editor.document.markdown_text(cx), source);
        });
    }

    #[gpui::test]
    async fn opening_thematic_break_is_not_frontmatter(cx: &mut TestAppContext) {
        let source = "---\n\ntext after";
        let editor = cx.new(|cx| Editor::from_markdown(cx, source.into(), None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert!(
                !visible.iter().any(|block| matches!(
                    block.entity.read(cx).kind(),
                    BlockKind::RawMarkdown | BlockKind::FrontMatter
                )),
                "a bare opening --- must not become frontmatter"
            );
        });
    }

    #[gpui::test]
    async fn balanced_unknown_fenced_div_stays_as_editable_raw_markdown(cx: &mut TestAppContext) {
        let source = "before\n\n::: warning\nUse caution.\n:::\n\n# after";
        let editor = cx.new(|cx| Editor::from_markdown(cx, source.into(), None));

        editor.update(cx, |editor, cx| {
            assert!(matches!(editor.view_mode, crate::editor::ViewMode::Rendered));
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 3);
            assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::Paragraph);
            let raw = visible[1].entity.clone();
            assert_eq!(raw.read(cx).kind(), BlockKind::RawMarkdown);
            assert_eq!(
                raw.read(cx).display_text(),
                "::: warning\nUse caution.\n:::"
            );
            assert_eq!(
                visible[2].entity.read(cx).kind(),
                BlockKind::Heading { level: 1 }
            );
            assert_eq!(editor.document.markdown_text(cx), source);

            let start = "::: warning\n".len();
            raw.update(cx, |block, block_cx| {
                block.prepare_undo_capture(
                    crate::components::UndoCaptureKind::NonCoalescible,
                    block_cx,
                );
                block.replace_text_in_visible_range(
                    start..start + "Use caution.".len(),
                    "Be careful.",
                    None,
                    false,
                    block_cx,
                );
            });
            assert_eq!(
                editor.document.markdown_text(cx),
                "before\n\n::: warning\nBe careful.\n:::\n\n# after"
            );
        });
    }

    #[gpui::test]
    async fn ambiguous_markdown_extensions_open_and_stay_in_source_mode(cx: &mut TestAppContext) {
        let unsupported = [
            "::: custom\ncontent",
            "!!! note\n  content",
            "??? note\n  content",
        ];

        for source in unsupported {
            let editor = cx.new(|cx| Editor::from_markdown(cx, source.into(), None));
            editor.update(cx, |editor, cx| {
                assert!(matches!(editor.view_mode, crate::editor::ViewMode::Source));
                assert!(editor.source_mode_fallback_required);
                assert_eq!(editor.document.raw_source_text(cx), source);

                editor.toggle_view_mode(cx);
                assert!(matches!(editor.view_mode, crate::editor::ViewMode::Source));
                assert!(editor.source_mode_fallback_required);
                assert_eq!(editor.document.raw_source_text(cx), source);
            });
        }

        let fenced = cx.new(|cx| {
            Editor::from_markdown(cx, "```markdown\n!!! note\n::: custom\n```".into(), None)
        });
        fenced.update(cx, |editor, _cx| {
            assert!(matches!(editor.view_mode, crate::editor::ViewMode::Rendered));
            assert!(!editor.source_mode_fallback_required);
        });
    }

    #[test]
    fn a_longer_run_is_a_valid_closing_fence() {
        // CommonMark: the first fence whose run is >= the opener and carries no
        // info string closes the block, so a ```` line closes a ``` opener right
        // there (the old exact-length rule skipped it and kept scanning).
        let lines = vec![
            "```rust".to_string(),
            "````".to_string(),
            "body".to_string(),
            "```".to_string(),
        ];
        let opener = parse_opening_fence(&lines[0]).expect("opening fence");
        assert_eq!(find_matching_closing_fence(&lines, 0, &opener), Some(1));
    }

    #[test]
    fn fence_closes_at_first_match_even_before_a_later_opener() {
        // The first closing fence ends the block; later fences belong to
        // whatever follows, not to this block (issue #58).
        let lines = vec![
            "```rust".to_string(),
            "```".to_string(),
            "body".to_string(),
            "```".to_string(),
            "```ts".to_string(),
        ];
        let opener = parse_opening_fence(&lines[0]).expect("opening fence");
        assert_eq!(find_matching_closing_fence(&lines, 0, &opener), Some(1));
    }

    #[test]
    fn empty_language_fence_closes_at_first_match() {
        // Adjacent empty-language blocks must stay separate rather than the
        // first absorbing the second's fences as body content (issue #58).
        let lines = vec![
            "```".to_string(),
            "first".to_string(),
            "```".to_string(),
            "```".to_string(),
            "second".to_string(),
            "```".to_string(),
        ];
        let opener = parse_opening_fence(&lines[0]).expect("opening fence");
        assert_eq!(find_matching_closing_fence(&lines, 0, &opener), Some(2));
    }

    #[test]
    fn info_tagged_fence_does_not_absorb_following_empty_blocks() {
        // An info-string opener must still close at its own fence instead of
        // swallowing later empty-language blocks (issue #58).
        let lines = vec![
            "```bash".to_string(),
            "git clone url".to_string(),
            "```".to_string(),
            "```".to_string(),
            "cargo build".to_string(),
            "```".to_string(),
        ];
        let opener = parse_opening_fence(&lines[0]).expect("opening fence");
        assert_eq!(find_matching_closing_fence(&lines, 0, &opener), Some(2));
    }

    #[test]
    fn an_inner_info_string_run_is_content_and_closes_at_the_next_bare_fence() {
        // CommonMark: inside a block a fence carrying an info string is not a
        // closing fence, it is code content; the block still closes at the next
        // bare fence. The old code abandoned the block at the inner opener.
        let lines = vec![
            "```rust".to_string(),
            "body".to_string(),
            "```ts".to_string(),
            "```".to_string(),
        ];
        let opener = parse_opening_fence(&lines[0]).expect("opening fence");
        assert_eq!(find_matching_closing_fence(&lines, 0, &opener), Some(3));
    }

    #[test]
    fn a_shorter_inner_fence_run_is_content_and_closes_at_the_equal_opener_run() {
        // `````python 块里那行 `` ``` inside code ``：三个反引号比开栏（四个）短，
        // 又不是裸围栏——它是代码内容，块要在四个反引号的裸闭合处才收尾。
        let lines = vec![
            "````python".to_string(),
            "``` inside code".to_string(),
            "value = 1".to_string(),
            "````".to_string(),
        ];
        let opener = parse_opening_fence(&lines[0]).expect("opening fence");
        assert_eq!(find_matching_closing_fence(&lines, 0, &opener), Some(3));
    }

    #[test]
    fn parses_indented_code_blocks() {
        assert_eq!(strip_indented_code_prefix("    code"), Some("code"));
        assert_eq!(strip_indented_code_prefix("\tcode"), Some("code"));
        assert_eq!(strip_indented_code_prefix("  code"), None);
    }

    #[test]
    fn parses_original_unordered_list_markers() {
        assert_eq!(
            parse_list_marker("- item").unwrap().kind,
            BlockKind::BulletedListItem
        );
        assert_eq!(
            parse_list_marker("* item").unwrap().kind,
            BlockKind::BulletedListItem
        );
        assert_eq!(
            parse_list_marker("+ item").unwrap().kind,
            BlockKind::BulletedListItem
        );
        assert_eq!(
            parse_list_marker("- [ ] item").unwrap().kind,
            BlockKind::TaskListItem { checked: false }
        );
        assert_eq!(
            parse_list_marker("* [x] item").unwrap().kind,
            BlockKind::TaskListItem { checked: true }
        );
        assert_eq!(
            parse_list_marker("+ [X] item").unwrap().kind,
            BlockKind::TaskListItem { checked: true }
        );
    }

    #[test]
    fn parses_commonmark_ordered_list_markers() {
        let dot = parse_list_marker("1. item").expect("dot marker");
        assert_eq!(dot.kind, BlockKind::NumberedListItem);
        assert_eq!(dot.text, "item");
        assert_eq!(dot.content_indent_columns, 3);
        assert_eq!(dot.numbered_start, Some(1));

        let start = parse_list_marker("5. item").expect("authored start marker");
        assert_eq!(start.numbered_start, Some(5));

        let paren = parse_list_marker("12) item").expect("paren marker");
        assert_eq!(paren.kind, BlockKind::NumberedListItem);
        assert_eq!(paren.text, "item");
        assert_eq!(paren.content_indent_columns, 4);
        assert_eq!(paren.numbered_start, Some(12));

        let tab = parse_list_marker("1)\titem").expect("tab separator");
        assert_eq!(tab.kind, BlockKind::NumberedListItem);
        assert_eq!(tab.text, "item");
        assert_eq!(tab.content_indent_columns, 4);

        assert!(parse_list_marker("1)item").is_none());
        assert!(parse_list_marker("1234567890) item").is_none());

        // 无序项没有起始号。
        assert_eq!(parse_list_marker("- item").unwrap().numbered_start, None);
    }

    #[test]
    fn strips_one_quote_level_per_line() {
        assert_eq!(strip_one_quote_level("> quote"), Some("quote".to_string()));
        assert_eq!(
            strip_one_quote_level("   > quote"),
            Some("quote".to_string())
        );
        assert_eq!(
            strip_one_quote_level(">> nested"),
            Some("> nested".to_string())
        );
    }

    #[test]
    fn recognizes_reference_definition_lines() {
        assert!(is_reference_definition_start("[id]: http://example.com"));
        assert!(is_reference_definition_start(
            "   [id]: <http://example.com/>"
        ));
        assert!(!is_reference_definition_start("[id] http://example.com"));
    }

    #[gpui::test]
    async fn long_closing_fence_renders_as_code_and_closes_the_block(cx: &mut TestAppContext) {
        // cases/09-code-long-close.md：```text 用 ```` 收尾。以前永不闭合，
        // 界面把围栏行与语言标记当正文显示；现在认出是代码块。
        let source = "```text\nhello\nworld\n````\n\n尾部段落";
        let editor = cx.new(|cx| Editor::from_markdown(cx, source.into(), None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            let code = visible[0].entity.read(cx);
            assert_eq!(
                code.kind(),
                BlockKind::CodeBlock {
                    language: Some("text".into())
                }
            );
            assert_eq!(code.display_text(), "hello\nworld");
            assert_eq!(
                visible[1].entity.read(cx).kind(),
                BlockKind::Paragraph
            );
        });
    }

    #[gpui::test]
    async fn tilde_fence_with_long_close_renders_as_code(cx: &mut TestAppContext) {
        let source = "~~~sh\necho hi\n~~~~\n\n尾部段落";
        let editor = cx.new(|cx| Editor::from_markdown(cx, source.into(), None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            let code = visible[0].entity.read(cx);
            assert_eq!(
                code.kind(),
                BlockKind::CodeBlock {
                    language: Some("sh".into())
                }
            );
            assert_eq!(code.display_text(), "echo hi");
            assert_eq!(
                visible[1].entity.read(cx).kind(),
                BlockKind::Paragraph
            );
        });
    }

    #[gpui::test]
    async fn inner_backtick_run_stays_code_content(cx: &mut TestAppContext) {
        // cases/09-code-inner-fence.md：```` 块里那行 ``` inside code 是代码内容。
        // 旧版把带信息串的内层行当成开栏，整块被放弃（认不出块尾）。
        let source = "````python\n``` inside code\nvalue = 1\n````\n\n尾部段落";
        let editor = cx.new(|cx| Editor::from_markdown(cx, source.into(), None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            let code = visible[0].entity.read(cx);
            assert_eq!(
                code.kind(),
                BlockKind::CodeBlock {
                    language: Some("python".into())
                }
            );
            assert_eq!(code.display_text(), "``` inside code\nvalue = 1");
            assert_eq!(
                visible[1].entity.read(cx).kind(),
                BlockKind::Paragraph
            );
            // 落笔会挑一档比正文里最长围栏还长的记号（正文含 ```，故选 `~~~`/`````），
            // 形状是等价的：再解析一次仍是同一个代码块。
            let round = editor.document.markdown_text(cx);
            let reparsed = cx.new(|cx| Editor::from_markdown(cx, round.clone(), None));
            reparsed.update(cx, |reparsed, cx| {
                let code = reparsed.document.visible_blocks()[0].entity.read(cx);
                assert_eq!(
                    code.kind(),
                    BlockKind::CodeBlock {
                        language: Some("python".into())
                    }
                );
                assert_eq!(code.display_text(), "``` inside code\nvalue = 1");
            });
        });
    }


    #[test]
    fn block_html_region_runs_until_blank_line() {
        let lines = vec![
            "<table>".to_string(),
            "<tr><td>x</td></tr>".to_string(),
            "</table>".to_string(),
            "".to_string(),
            "tail".to_string(),
        ];
        assert_eq!(collect_block_html_region(&lines, 0), 3);
    }

