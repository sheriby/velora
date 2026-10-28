    use gpui::{AppContext, TestAppContext};

    
    use crate::components::{BlockKind, Editor};

    #[gpui::test]
    async fn core_repair_incremental_chunks_share_lookahead_without_rescanning_the_tail(cx: &mut TestAppContext) {
        let source = (0..3000).map(|index| format!("paragraph {index}\n\n")).collect::<String>();
        super::super::scan_probe::reset();
        let chunked = cx.new(|cx| Editor::from_markdown_with_chunk_budget(cx, source.clone(), None, 1));
        chunked.update(cx, |editor, cx| {
            while editor.document.pending_tail().is_some() {
                editor.materialize_next_pending_chunk(cx);
            }
        });
        let work = super::super::scan_probe::bytes();
        assert!(work <= source.len() * 6, "分批不能重复索引剩余全文：{work} / {}", source.len());
        let full = cx.new(|cx| Editor::from_markdown_with_chunk_budget(cx, source.clone(), None, usize::MAX));
        let signature = |editor: &Editor, cx: &gpui::App| editor.document.visible_blocks().iter().map(|visible| {
            let block = visible.entity.read(cx);
            (block.kind(), block.record.title.visible_text())
        }).collect::<Vec<_>>();
        let expected = full.read_with(cx, signature);
        chunked.read_with(cx, |editor, cx| {
            assert_eq!(editor.buffer.text(), source);
            assert_eq!(signature(editor, cx), expected);
        });
    }

    #[gpui::test]
    async fn core_repair_inline_code_paragraph_lookahead_scans_linear_input(cx: &mut TestAppContext) {
        let mut failures = Vec::new();
        for count in [100, 200, 400] {
            let source = format!("`begin\n{}end`", "body\n\n".repeat(count));
            super::super::scan_probe::reset();
            let editor = cx.new(|cx| Editor::from_markdown(cx, source.clone(), None));
            let work = super::super::scan_probe::bytes();
            editor.read_with(cx, |editor, cx| {
                assert_eq!(editor.buffer.text(), source);
                assert_eq!(editor.document.root_count(), 1);
                assert_eq!(editor.document.first_root().unwrap().read(cx).kind(), BlockKind::Paragraph);
            });
            if work > source.len() * 12 { failures.push((count, work, source.len())); }
        }
        assert!(failures.is_empty(), "段落前缀及后文扫描应随输入线性增长：{failures:?}");
    }


    /// 一项大列表（松散列表：一项里很多子段）不能对每个子块重扫剩余区间。
    ///
    /// 26ffd2c 把根块路径那份重复扫描合并掉了，列表项这条原样留着：项内每个子块各建一份
    /// 前瞻索引、各切一次「本项剩余区间」→ O(子块数 × 剩余行数)。闸门看**扫描量**（不是
    /// 墙钟）：同一份夹具翻倍时，每个字节的扫描量不许跟着涨。修复前实测 40.9 / 81.3 / 165.1
    /// （翻倍即翻倍），修复后 3.5 上下。
    #[gpui::test]
    async fn review_loose_list_item_children_share_the_lookahead(cx: &mut TestAppContext) {
        let mut failures = Vec::new();
        for count in [60usize, 120, 240] {
            let mut source = String::from("- 首项\n\n");
            for index in 0..count {
                source.push_str(&format!("  body {index}\n\n"));
            }
            super::super::scan_probe::reset();
            let editor = cx.new(|cx| Editor::from_markdown(cx, source.clone(), None));
            let work = super::super::scan_probe::bytes();
            editor.read_with(cx, |editor, _cx| {
                assert_eq!(editor.buffer.text(), source);
                assert_eq!(editor.document.root_count(), 1, "整篇是一项大列表");
                assert_eq!(
                    editor.document.visible_blocks().len(),
                    count + 1,
                    "项本身 + 每个子段"
                );
            });
            if work > source.len() * 12 {
                failures.push((count, work, source.len()));
            }
        }
        assert!(
            failures.is_empty(),
            "列表项内每个子块都在重扫剩余区间（数量随输入平方增长）：{failures:?}"
        );
    }

    #[gpui::test]
    async fn review_image_paragraph_can_be_setext_heading(cx: &mut TestAppContext) {
        // 图片行仍是段落，不能因原生图片运行时而排除合法 setext 标题。
        let source = "![图片](pic.png)\n---";
        let html = crate::export::html::render_html(source, &crate::theme::Theme::default_theme(), "标题");
        assert!(html.contains("<h2 "));
        let editor = cx.new(|cx| Editor::from_markdown(cx, source.into(), None));
        editor.read_with(cx, |editor, cx| {
            assert_eq!(editor.document.first_root().unwrap().read(cx).kind(), BlockKind::Heading { level: 2 });
        });
    }

    #[gpui::test]
    async fn review_setext_underline_does_not_promote_block_syntax(cx: &mut TestAppContext) {
        // 非段落行后的划线曾吞并前行，编辑后原块语法与分隔线一同丢失。
        for (caption, expected) in [
            ("# 标题", BlockKind::Heading { level: 1 }),
            ("- 项", BlockKind::BulletedListItem),
            ("> 引用", BlockKind::Quote),
            ("    code", BlockKind::CodeBlock { language: None }),
        ] {
            let editor = cx.new(|cx| Editor::from_markdown(cx, format!("{caption}\n---\n正文"), None));
            editor.read_with(cx, |editor, cx| {
                let blocks = editor.document.visible_blocks();
                assert_eq!(blocks[0].entity.read(cx).kind(), expected, "{caption}");
                assert!(blocks.iter().any(|block| block.entity.read(cx).kind() == BlockKind::Separator),
                    "{caption}: {:?}", blocks.iter().map(|block| block.entity.read(cx).kind()).collect::<Vec<_>>());
            });
        }
    }

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
            // 下划线照原文写回：ATX 是规范写法，但文件里写的是 setext，
            // 未编辑的标题不该换形状（旧断言烘焙了 `## Heading` 重写）。
            assert_eq!(
                editor.document.markdown_text(cx),
                "Heading\n-------\n\nfirst line\nsecond line"
            );
        });
    }

    #[gpui::test]
    async fn imports_atx_heading_with_ideographic_space_separator(cx: &mut TestAppContext) {
        // 输入法在 `##` 后打的是全角空格（U+3000）时同样是标题：判据只认
        // ASCII 空格的话，这种写法在阅读视图里退成「##　标题」原文段落。
        let editor = cx.new(|cx| {
            Editor::from_markdown(cx, "##\u{3000}标题\n\n正文".to_string(), None)
        });
        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(
                visible[0].entity.read(cx).kind(),
                BlockKind::Heading { level: 2 }
            );
            assert_eq!(visible[0].entity.read(cx).display_text(), "标题");
            assert_eq!(visible[1].entity.read(cx).kind(), BlockKind::Paragraph);
        });
    }

    #[gpui::test]
    async fn imports_atx_heading_with_tab_separator(cx: &mut TestAppContext) {
        // 制表符分隔（外部文件与粘贴里会有真 tab）：阅读视图同样按标题渲染。
        let editor = cx.new(|cx| {
            Editor::from_markdown(cx, "##\t标题\n\n正文".to_string(), None)
        });
        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(
                visible[0].entity.read(cx).kind(),
                BlockKind::Heading { level: 2 }
            );
            assert_eq!(visible[0].entity.read(cx).display_text(), "标题");
            assert_eq!(visible[1].entity.read(cx).kind(), BlockKind::Paragraph);
        });
    }

    #[gpui::test]
    async fn imports_indented_code_blocks_and_keeps_them_indented(cx: &mut TestAppContext) {
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
            // 文件里没有围栏行，序列化也就不补一对：那是「格式化文档」才许的规范化。
            assert_eq!(
                editor.document.markdown_text(cx),
                "    let x = 1;\n    println!(\"hi\");"
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
        // `topic_embedding_attention` 词中下划线按 CommonMark 不是强调定界符，
        // 原文必须原样保留。
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
            // 每一行的缩进按文件那一行记下的位数落笔（这里是六格），不发明围栏行。
            assert_eq!(
                editor.document.markdown_text(cx),
                "- item with code block\n      let x = 1;\n      let y = 2;"
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
            // `>>` 按文件写法逐字节还原（旧断言烘焙了 `> > ` 规范重拼）；第三层
            // 里层记号仍是规范形——每行宽度账只在 quote_depth == 1 的引用上消费，
            // 里层的绝对宽度没法还原成 `>>>` 这种无空格叠写。
            assert_eq!(
                editor.document.markdown_text(cx),
                "> level1\n>> level2\n>> > level3"
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


    #[gpui::test]
    async fn single_character_setext_underline_round_trips(cx: &mut TestAppContext) {
        // CommonMark 的 setext 下划线只要求至少一个字符：`Title\n=` 是一级标题、
        // `Title\n--` 是二级标题；写回时下划线照原文落笔，不能改写成 ATX。
        for (source, level) in [("Title\n=", 1u8), ("Title\n--", 2u8)] {
            let editor = cx.new(|cx| Editor::from_markdown(cx, source.into(), None));
            editor.update(cx, |editor, cx| {
                let visible = editor.document.visible_blocks();
                assert_eq!(visible.len(), 1, "{source}");
                assert_eq!(
                    visible[0].entity.read(cx).kind(),
                    BlockKind::Heading { level },
                    "{source}"
                );
                assert_eq!(visible[0].entity.read(cx).display_text(), "Title");
                assert_eq!(editor.document.markdown_text(cx), source, "{source}");
            });
        }
    }

    #[gpui::test]
    async fn multiline_setext_content_forms_one_heading(cx: &mut TestAppContext) {
        // CommonMark 的 setext 内容可以连续多行：整段连同下划线是一个标题。
        // 拆成「段落 + 下划线块」的话接缝还会凭空多补一个空行。
        for (source, level) in [
            ("a\nb\n---", 2u8),
            ("a\nb\n===", 1u8),
        ] {
            let editor = cx.new(|cx| Editor::from_markdown(cx, source.into(), None));
            editor.update(cx, |editor, cx| {
                let visible = editor.document.visible_blocks();
                assert_eq!(visible.len(), 1, "{source}");
                assert_eq!(
                    visible[0].entity.read(cx).kind(),
                    BlockKind::Heading { level },
                    "{source}"
                );
                assert_eq!(visible[0].entity.read(cx).display_text(), "a\nb", "{source}");
                assert_eq!(editor.document.markdown_text(cx), source, "{source}");
            });
        }
    }

    #[gpui::test]
    async fn indented_line_after_paragraph_stays_paragraph_text(cx: &mut TestAppContext) {
        // 缩进代码块不能打断段落（CommonMark）：紧邻的 4 空格行是续行，
        // 拆成代码块的话序列化时段落→代码接缝再补一个空行，原文被加行改写。
        let source = "some text\n    code-ish line";
        let editor = cx.new(|cx| Editor::from_markdown(cx, source.into(), None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 1, "紧邻的缩进行按段落续行吞入");
            assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(
                visible[0].entity.read(cx).display_text(),
                "some text\n    code-ish line"
            );
            assert_eq!(editor.document.markdown_text(cx), source);
        });
    }

    #[gpui::test]
    async fn reference_definition_url_on_the_next_line_stays_a_definition(cx: &mut TestAppContext) {
        // `[foo]:` 换行 `/url` 是合法的链接定义写法：URL 行并进定义 raw 块，
        // 不并的话它掉给根循环当正文段落，用户看到一个突兀的 `/url`。
        let source = "[foo]:\n/url";
        let editor = cx.new(|cx| Editor::from_markdown(cx, source.into(), None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 1, "定义与它的 URL 行是一块");
            assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::RawMarkdown);
            assert_eq!(editor.document.markdown_text(cx), source);
        });
    }

    #[gpui::test]
    async fn thematic_break_run_is_not_swallowed_by_frontmatter_detection(cx: &mut TestAppContext) {
        // 以 `---`（水平分隔线）开头的文档，后文再出现一个 `---` 时，
        // 中间的标题正文不能被整篇吞成 FrontMatter。
        let source = "---\n# Title\n\nbody\n\n---\nmore";
        let editor = cx.new(|cx| Editor::from_markdown(cx, source.into(), None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert!(
                !visible
                    .iter()
                    .any(|block| block.entity.read(cx).kind() == BlockKind::FrontMatter),
                "分隔线开头的普通文档不能被吞成 YAML 块"
            );
            assert!(visible
                .iter()
                .any(|block| block.entity.read(cx).kind() == BlockKind::Heading { level: 1 }));
            // 相邻根块之间的规范空行是序列化模型的既有口径（run-1 折进分隔符），
            // 这里只锁「内容不被吞」：标题与正文都还在，各占自己的块。
            let serialized = editor.document.markdown_text(cx);
            assert!(serialized.contains("# Title"));
            assert!(serialized.contains("body"));
            assert!(serialized.contains("more"));
        });
    }
