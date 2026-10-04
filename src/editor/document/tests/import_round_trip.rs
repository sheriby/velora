    use gpui::{AppContext, TestAppContext};

    
    use crate::components::{BlockKind, Editor};

    #[gpui::test]
    async fn imports_setext_headings_and_grouped_paragraphs(cx: &mut TestAppContext) {
        let editor = cx.new(|cx| {
            Editor::from_markdown(
                cx,
                "Heading\n-------\n\nfirst line\nsecond line".to_string(),
                None,
            )
        });

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 2);
            assert_eq!(
                visible[0].entity.read(cx).kind(),
                BlockKind::Heading { level: 2 }
            );
            assert_eq!(visible[0].entity.read(cx).display_text(), "Heading");
            assert_eq!(visible[1].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(
                visible[1].entity.read(cx).display_text(),
                "first line\nsecond line"
            );
            assert_eq!(
                editor.document.markdown_text(cx),
                "## Heading\n\nfirst line\nsecond line"
            );
        });
    }

    #[gpui::test]
    async fn imports_indented_code_blocks_and_serializes_fenced(cx: &mut TestAppContext) {
        let editor = cx.new(|cx| {
            Editor::from_markdown(
                cx,
                "    let x = 1;\n    println!(\"hi\");".to_string(),
                None,
            )
        });

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 1);
            assert!(visible[0].entity.read(cx).kind().is_code_block());
            assert_eq!(
                visible[0].entity.read(cx).display_text(),
                "let x = 1;\nprintln!(\"hi\");"
            );
            assert_eq!(
                editor.document.markdown_text(cx),
                "```\nlet x = 1;\nprintln!(\"hi\");\n```"
            );
        });
    }

    #[gpui::test]
    async fn imports_consecutive_code_blocks_without_merging(cx: &mut TestAppContext) {
        // An info-tagged block followed by language-less blocks: each must
        // parse as its own code block rather than being merged (issue #58).
        let source = "```bash\ngit clone url\n```\n\n```\ncargo build\n```\n\n```\nmake\n```";
        let editor = cx.new(|cx| Editor::from_markdown(cx, source.to_string(), None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            let code_blocks: Vec<_> = visible
                .iter()
                .filter(|block| block.entity.read(cx).kind().is_code_block())
                .collect();
            assert_eq!(code_blocks.len(), 3);
            assert_eq!(
                code_blocks[0].entity.read(cx).display_text(),
                "git clone url"
            );
            assert_eq!(code_blocks[1].entity.read(cx).display_text(), "cargo build");
            assert_eq!(code_blocks[2].entity.read(cx).display_text(), "make");
        });
    }

    #[gpui::test]
    async fn preserves_hard_break_spaces_in_paragraph_round_trip(cx: &mut TestAppContext) {
        let editor = cx.new(|cx| Editor::from_markdown(cx, "alpha  \nbeta".to_string(), None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 1);
            assert_eq!(visible[0].entity.read(cx).display_text(), "alpha  \nbeta");
            assert_eq!(editor.document.markdown_text(cx), "alpha  \nbeta");

            editor.toggle_view_mode(cx);
            editor.toggle_view_mode(cx);

            let visible = editor.document.visible_blocks();
            assert_eq!(visible[0].entity.read(cx).display_text(), "alpha  \nbeta");
            assert_eq!(editor.document.markdown_text(cx), "alpha  \nbeta");
        });
    }

    #[gpui::test]
    async fn preserves_tibetan_spaces_in_paragraph_round_trip(cx: &mut TestAppContext) {
        let tibetan = "༄༅།།དཔལ་ལྡན་རྩ་བའི་བླ་མ་རིན་པོ་ཆེ།། བདག་གི་སྤྱི་བོར་པདྨའི་གདན་བཞུགས་ནས།། ";
        let editor = cx.new(|cx| Editor::from_markdown(cx, tibetan.to_string(), None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 1);
            assert_eq!(visible[0].entity.read(cx).display_text(), tibetan);
            assert!(visible[0].entity.read(cx).display_text().contains("།། བདག"));
            assert!(visible[0].entity.read(cx).display_text().ends_with(' '));
            assert_eq!(editor.document.markdown_text(cx), tibetan);

            editor.toggle_view_mode(cx);
            editor.toggle_view_mode(cx);

            let visible = editor.document.visible_blocks();
            assert_eq!(visible[0].entity.read(cx).display_text(), tibetan);
            assert_eq!(editor.document.markdown_text(cx), tibetan);
        });
    }

    #[gpui::test]
    async fn preserves_chinese_spaces_in_paragraph_round_trip(cx: &mut TestAppContext) {
        let chinese = "中文 文本 ";
        let editor = cx.new(|cx| Editor::from_markdown(cx, chinese.to_string(), None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 1);
            assert_eq!(visible[0].entity.read(cx).display_text(), chinese);
            assert_eq!(editor.document.markdown_text(cx), chinese);
        });
    }

    #[gpui::test]
    async fn preserves_intraword_underscores_in_paragraph_round_trip(cx: &mut TestAppContext) {
        // 用户报修：含下划线标识符的段落存盘后被改写。`topic_embedding_attention`
        // 词中下划线按 CommonMark 不是强调定界符，原文必须原样保留。
        let source = "数据来源：`work_0826A3_fa/评估汇总.md`（**topic_embedding_attention 有轨迹无产物**，按步骤 2.7 用 `trace_extract_chain.py` 提取）。";
        let editor = cx.new(|cx| Editor::from_markdown(cx, source.to_string(), None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 1);
            assert!(
                visible[0]
                    .entity
                    .read(cx)
                    .display_text()
                    .contains("topic_embedding_attention 有轨迹无产物")
            );
            assert_eq!(editor.document.markdown_text(cx), source);
            assert_eq!(editor.document.markdown_text(cx), source);

            editor.toggle_view_mode(cx);
            editor.toggle_view_mode(cx);
            assert_eq!(editor.document.markdown_text(cx), source);
        });
    }

    #[gpui::test]
    async fn preserves_hard_break_spaces_in_simple_quote(cx: &mut TestAppContext) {
        let editor = cx.new(|cx| Editor::from_markdown(cx, "> alpha  \n> beta".to_string(), None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 1);
            assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::Quote);
            assert_eq!(visible[0].entity.read(cx).display_text(), "alpha  \nbeta");
            assert_eq!(editor.document.markdown_text(cx), "> alpha  \n> beta");
        });
    }

    #[gpui::test]
    async fn preserves_hard_break_spaces_in_list_item_continuation(cx: &mut TestAppContext) {
        let editor = cx.new(|cx| Editor::from_markdown(cx, "- alpha  \n  beta".to_string(), None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 1);
            assert_eq!(
                visible[0].entity.read(cx).kind(),
                BlockKind::BulletedListItem
            );
            assert_eq!(visible[0].entity.read(cx).display_text(), "alpha  \nbeta");
            assert_eq!(editor.document.markdown_text(cx), "- alpha  \n  beta");
        });
    }

    #[gpui::test]
    async fn imports_nested_list_children_as_native_blocks(cx: &mut TestAppContext) {
        let editor = cx.new(|cx| {
            Editor::from_markdown(
                cx,
                "- parent\n  - nested bullet\n  - [x] nested task".to_string(),
                None,
            )
        });

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 3);
            assert_eq!(
                visible[0].entity.read(cx).kind(),
                BlockKind::BulletedListItem
            );
            assert_eq!(visible[0].entity.read(cx).display_text(), "parent");
            assert_eq!(
                visible[1].entity.read(cx).kind(),
                BlockKind::BulletedListItem
            );
            assert_eq!(visible[1].entity.read(cx).display_text(), "nested bullet");
            assert_eq!(
                visible[2].entity.read(cx).kind(),
                BlockKind::TaskListItem { checked: true }
            );
            assert_eq!(visible[2].entity.read(cx).display_text(), "nested task");
        });
    }

    #[gpui::test]
    async fn imports_indented_code_block_as_native_list_child(cx: &mut TestAppContext) {
        let editor = cx.new(|cx| {
            Editor::from_markdown(
                cx,
                "- item with code block\n\n      let x = 1;\n      let y = 2;".to_string(),
                None,
            )
        });

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 2);
            assert_eq!(
                visible[0].entity.read(cx).kind(),
                BlockKind::BulletedListItem
            );
            assert_eq!(
                visible[1].entity.read(cx).kind(),
                BlockKind::CodeBlock { language: None }
            );
            assert_eq!(
                visible[1].entity.read(cx).display_text(),
                "let x = 1;\nlet y = 2;"
            );
            assert_eq!(
                editor.document.markdown_text(cx),
                "- item with code block\n  ```\n  let x = 1;\n  let y = 2;\n  ```"
            );

            editor.toggle_view_mode(cx);
            editor.toggle_view_mode(cx);

            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 2);
            assert_eq!(
                visible[1].entity.read(cx).kind(),
                BlockKind::CodeBlock { language: None }
            );
            assert_eq!(
                visible[1].entity.read(cx).display_text(),
                "let x = 1;\nlet y = 2;"
            );
        });
    }

    #[gpui::test]
    async fn imports_fenced_code_block_as_native_list_child(cx: &mut TestAppContext) {
        let editor = cx.new(|cx| {
            Editor::from_markdown(
                cx,
                "- item with fenced code\n  ```rust\n  fn main() {}\n  ```".to_string(),
                None,
            )
        });

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 2);
            assert_eq!(
                visible[0].entity.read(cx).kind(),
                BlockKind::BulletedListItem
            );
            assert_eq!(
                visible[1].entity.read(cx).kind(),
                BlockKind::CodeBlock {
                    language: Some("rust".into())
                }
            );
            assert_eq!(visible[1].entity.read(cx).display_text(), "fn main() {}");
        });
    }

    #[gpui::test]
    async fn imports_simple_quote_as_native_list_child(cx: &mut TestAppContext) {
        let editor = cx.new(|cx| {
            Editor::from_markdown(
                cx,
                "1. item with nested quote\n\n   > quoted text\n   >\n   > quoted paragraph two"
                    .to_string(),
                None,
            )
        });

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 2);
            assert_eq!(
                visible[0].entity.read(cx).kind(),
                BlockKind::NumberedListItem
            );
            assert_eq!(
                visible[0].entity.read(cx).display_text(),
                "item with nested quote"
            );
            assert_eq!(visible[1].entity.read(cx).kind(), BlockKind::Quote);
            assert_eq!(
                visible[1].entity.read(cx).display_text(),
                "quoted text\n\nquoted paragraph two"
            );
            assert_eq!(
                editor.document.markdown_text(cx),
                "1. item with nested quote\n  > quoted text\n  > \n  > quoted paragraph two"
            );

            editor.toggle_view_mode(cx);
            editor.toggle_view_mode(cx);

            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 2);
            assert_eq!(
                visible[0].entity.read(cx).kind(),
                BlockKind::NumberedListItem
            );
            assert_eq!(visible[1].entity.read(cx).kind(), BlockKind::Quote);
            assert_eq!(
                visible[1].entity.read(cx).display_text(),
                "quoted text\n\nquoted paragraph two"
            );
        });
    }

    #[gpui::test]
    async fn separated_numbered_list_runs_restart_at_one_after_blank_line(cx: &mut TestAppContext) {
        let editor = cx
            .new(|cx| Editor::from_markdown(cx, "1. aa\n2. bb\n3. cc\n\n1. dd".to_string(), None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 5);
            assert_eq!(
                visible[0].entity.read(cx).kind(),
                BlockKind::NumberedListItem
            );
            assert_eq!(visible[0].entity.read(cx).list_ordinal, Some(1));
            assert_eq!(visible[1].entity.read(cx).list_ordinal, Some(2));
            assert_eq!(visible[2].entity.read(cx).list_ordinal, Some(3));
            assert_eq!(visible[3].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(visible[3].entity.read(cx).display_text(), "");
            assert_eq!(
                visible[4].entity.read(cx).kind(),
                BlockKind::NumberedListItem
            );
            assert_eq!(visible[4].entity.read(cx).display_text(), "dd");
            assert_eq!(visible[4].entity.read(cx).list_ordinal, Some(1));
            assert_eq!(
                editor.document.markdown_text(cx),
                "1. aa\n2. bb\n3. cc\n\n1. dd"
            );
        });
    }

    #[gpui::test]
    async fn imports_parenthesized_ordered_lists_and_keeps_the_paren_markers(
        cx: &mut TestAppContext,
    ) {
        let editor = cx.new(|cx| Editor::from_markdown(cx, "1) one\n2) two".to_string(), None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 2);
            assert_eq!(
                visible[0].entity.read(cx).kind(),
                BlockKind::NumberedListItem
            );
            assert_eq!(
                visible[1].entity.read(cx).kind(),
                BlockKind::NumberedListItem
            );
            assert_eq!(visible[0].entity.read(cx).display_text(), "one");
            assert_eq!(visible[1].entity.read(cx).display_text(), "two");
            assert_eq!(visible[0].entity.read(cx).list_ordinal, Some(1));
            assert_eq!(visible[1].entity.read(cx).list_ordinal, Some(2));
            // 记号的写法是原文的一部分：`1)` 序列化回去还是 `1)`，不再规范成 `1.`。
            assert_eq!(editor.document.markdown_text(cx), "1) one\n2) two");
        });
    }

    #[gpui::test]
    async fn imports_nested_parenthesized_ordered_list_children(cx: &mut TestAppContext) {
        let editor =
            cx.new(|cx| Editor::from_markdown(cx, "1) parent\n   1) child".to_string(), None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 2);
            assert_eq!(
                visible[0].entity.read(cx).kind(),
                BlockKind::NumberedListItem
            );
            assert_eq!(
                visible[1].entity.read(cx).kind(),
                BlockKind::NumberedListItem
            );
            assert_eq!(visible[0].entity.read(cx).display_text(), "parent");
            assert_eq!(visible[1].entity.read(cx).display_text(), "child");
            assert_eq!(visible[1].entity.read(cx).render_depth, 1);
            assert_eq!(editor.document.markdown_text(cx), "1) parent\n  1) child");
        });
    }

    #[gpui::test]
    async fn imports_nested_quotes_as_native_blocks(cx: &mut TestAppContext) {
        let editor = cx.new(|cx| {
            Editor::from_markdown(cx, "> level1\n>> level2\n>>> level3".to_string(), None)
        });

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 3);
            assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::Quote);
            assert_eq!(visible[0].entity.read(cx).display_text(), "level1");
            assert_eq!(visible[0].entity.read(cx).quote_depth, 1);
            assert_eq!(visible[1].entity.read(cx).kind(), BlockKind::Quote);
            assert_eq!(visible[1].entity.read(cx).display_text(), "level2");
            assert_eq!(visible[1].entity.read(cx).quote_depth, 2);
            assert_eq!(visible[2].entity.read(cx).kind(), BlockKind::Quote);
            assert_eq!(visible[2].entity.read(cx).display_text(), "level3");
            assert_eq!(visible[2].entity.read(cx).quote_depth, 3);
            assert_eq!(
                editor.document.markdown_text(cx),
                "> level1\n> > level2\n> > > level3"
            );
        });
    }

    #[gpui::test]
    async fn literal_blank_line_splits_quote_groups(cx: &mut TestAppContext) {
        let editor =
            cx.new(|cx| Editor::from_markdown(cx, "> first\n\n> second".to_string(), None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 2);
            assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::Quote);
            assert_eq!(visible[0].entity.read(cx).display_text(), "first");
            assert_eq!(visible[0].entity.read(cx).quote_depth, 1);
            assert_eq!(visible[1].entity.read(cx).kind(), BlockKind::Quote);
            assert_eq!(visible[1].entity.read(cx).display_text(), "second");
            assert_eq!(visible[1].entity.read(cx).quote_depth, 1);
            assert_eq!(editor.document.markdown_text(cx), "> first\n\n> second");
        });
    }

    #[gpui::test]
    async fn quoted_blank_line_stays_inside_same_quote_group(cx: &mut TestAppContext) {
        let editor =
            cx.new(|cx| Editor::from_markdown(cx, "> first\n>\n> second".to_string(), None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 1);
            assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::Quote);
            assert_eq!(visible[0].entity.read(cx).display_text(), "first\n\nsecond");
            // 记号的写法是文件里的事实：`>`（记号后不带空格）序列化回来还是 `>`，
            // 改成 `> ` 那种规范化只许住在「格式化文档」里。
            assert_eq!(editor.document.markdown_text(cx), "> first\n>\n> second");
        });
    }

