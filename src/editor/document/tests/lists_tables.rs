    use gpui::{AppContext, TestAppContext};

    
    use crate::components::{BlockKind, Editor};

    #[gpui::test]
    async fn imports_task_lists_and_keeps_their_bullet_markers(cx: &mut TestAppContext) {
        let editor = cx.new(|cx| {
            Editor::from_markdown(
                cx,
                "- [ ] todo\n* [x] done\n+ [X] shipped".to_string(),
                None,
            )
        });

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 3);
            assert_eq!(
                visible[0].entity.read(cx).kind(),
                BlockKind::TaskListItem { checked: false }
            );
            assert_eq!(visible[0].entity.read(cx).display_text(), "todo");
            assert_eq!(
                visible[1].entity.read(cx).kind(),
                BlockKind::TaskListItem { checked: true }
            );
            assert_eq!(
                editor.document.markdown_text(cx),
                "- [ ] todo\n* [x] done\n+ [x] shipped"
            );
        });
    }

    #[gpui::test]
    async fn parses_root_level_pipe_table_as_native_table(cx: &mut TestAppContext) {
        let markdown = "| A | B |\n| --- | --- |\n| 1 | 2 |".to_string();
        let editor = cx.new(|cx| Editor::from_markdown(cx, markdown.clone(), None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 1);
            assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::Table);
            let table = visible[0]
                .entity
                .read(cx)
                .record
                .table
                .as_ref()
                .expect("native table data");
            assert_eq!(table.header.len(), 2);
            assert_eq!(table.rows.len(), 1);
            assert_eq!(table.rows[0].len(), 2);
            assert_eq!(editor.document.markdown_text(cx), markdown);
        });
    }

    #[gpui::test]
    async fn broken_root_level_table_degrades_to_plain_text_lines(cx: &mut TestAppContext) {
        let markdown = "| A | B |\n| nope | --- |\n| 1 | 2 |".to_string();
        let editor = cx.new(|cx| Editor::from_markdown(cx, markdown.clone(), None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 3);
            assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(visible[1].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(visible[2].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(visible[0].entity.read(cx).display_text(), "| A | B |");
            assert_eq!(visible[1].entity.read(cx).display_text(), "| nope | --- |");
            assert_eq!(visible[2].entity.read(cx).display_text(), "| 1 | 2 |");
            assert_eq!(
                editor.document.markdown_text(cx),
                "| A | B |\n\n| nope | --- |\n\n| 1 | 2 |"
            );
        });
    }

    #[gpui::test]
    async fn imports_display_math_block_with_inline_fence_delimiters(cx: &mut TestAppContext) {
        // Typora 常见写法：`$$` 后面直接跟 `\begin{aligned}`，末行 `\end{aligned}$$`。
        let markdown = concat!(
            "$$\\begin{aligned}\n",
            "&=dy\\cdot y-c_A\\,dy\\cdot y_A\\\\[2pt]\n",
            "&=\\boxed{\\ dy\\cdot\\big(y-c_A y_A\\big)\\ \\neq 0\\ }\n",
            "\\end{aligned}$$\n",
            "\n",
            "下面是正文段落。\n",
        )
        .to_string();
        let editor = cx.new(|cx| Editor::from_markdown(cx, markdown.clone(), None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 2);
            assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::MathBlock);
            let math = visible[0].entity.read(cx).display_text();
            assert!(math.starts_with("$$\\begin{aligned}"), "公式块首行保留原文：{math:?}");
            assert!(math.ends_with("\\end{aligned}$$"), "末行的 $$ 不能丢：{math:?}");
            assert!(math.contains("\\boxed"), "公式正文应完整：{math:?}");
            assert_eq!(visible[1].entity.read(cx).kind(), BlockKind::Paragraph);
            // 公式块后面的段落不能被吞进公式区域。
            assert_eq!(visible[1].entity.read(cx).display_text(), "下面是正文段落。");
            assert_eq!(
                editor.document.markdown_text(cx).matches("\\boxed").count(),
                1,
                "公式只能出现一次"
            );
        });
    }

    #[gpui::test]
    async fn imports_four_space_indented_display_math(cx: &mut TestAppContext) {
        // 缩进 4 格曾会被当成缩进代码块，公式原样显示成源码。
        let markdown = concat!(
            "    $$\n",
            "    \\int_0^1 x\\,dx\n",
            "    $$\n",
        );
        let editor = cx.new(|cx| Editor::from_markdown(cx, markdown.to_string(), None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 1);
            assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::MathBlock);
            assert!(editor.document.markdown_text(cx).contains("\\int_0^1 x\\,dx"));
        });
    }

    #[gpui::test]
    async fn imports_display_math_on_list_item_line(cx: &mut TestAppContext) {
        // 公式直接写在项标记后面：`- $$ … $$`。
        let markdown = concat!(
            "- $$\\begin{aligned}\n",
            "  x &= y\n",
            "  \\end{aligned}$$\n",
        )
        .to_string();
        let editor = cx.new(|cx| Editor::from_markdown(cx, markdown.clone(), None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            let item = visible[0].entity.read(cx);
            assert_eq!(item.kind(), BlockKind::BulletedListItem);
            assert_eq!(item.children.len(), 1, "公式应该作为列表项的子块");
            let math = item.children[0].read(cx);
            assert_eq!(math.kind(), BlockKind::MathBlock);
            assert_eq!(
                math.display_text().lines().next(),
                Some("$$\\begin{aligned}"),
                "列表项里的公式应独立成块（去掉项缩进）"
            );
            // 项自己的文本不能再留着 `$$`（否则会渲染成源码）。
            assert_eq!(item.display_text(), "");
            assert_eq!(
                editor.document.markdown_text(cx).matches("\\begin{aligned}").count(),
                1,
                "公式只能出现一次"
            );
        });
    }

    #[gpui::test]
    async fn imports_display_math_block_as_native_math_block(cx: &mut TestAppContext) {
        let markdown = "$$\n\\int_0^1 x^2 dx\n$$".to_string();
        let editor = cx.new(|cx| Editor::from_markdown(cx, markdown.clone(), None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 1);
            assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::MathBlock);
            assert_eq!(visible[0].entity.read(cx).display_text(), markdown);
            assert_eq!(editor.document.markdown_text(cx), markdown);
        });
    }

    #[gpui::test]
    async fn imports_single_line_display_math_between_paragraphs(cx: &mut TestAppContext) {
        let markdown = "before\n$$x^2$$\nafter".to_string();
        let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 3);
            assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(visible[1].entity.read(cx).kind(), BlockKind::MathBlock);
            assert_eq!(visible[1].entity.read(cx).display_text(), "$$x^2$$");
            assert_eq!(visible[2].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(
                editor.document.markdown_text(cx),
                "before\n\n$$x^2$$\n\nafter"
            );
        });
    }

    #[gpui::test]
    async fn unclosed_display_math_stays_raw(cx: &mut TestAppContext) {
        let markdown = "$$\n\\int_0^1 x^2 dx".to_string();
        let editor = cx.new(|cx| Editor::from_markdown(cx, markdown.clone(), None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 1);
            assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::RawMarkdown);
            assert_eq!(editor.document.markdown_text(cx), markdown);
        });
    }

    #[gpui::test]
    async fn imports_mermaid_fence_as_native_mermaid_block(cx: &mut TestAppContext) {
        let markdown = "before\n```mermaid\nflowchart LR\nA --> B\n```\nafter".to_string();
        let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 3);
            assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(visible[1].entity.read(cx).kind(), BlockKind::MermaidBlock);
            assert_eq!(
                visible[1].entity.read(cx).display_text(),
                "```mermaid\nflowchart LR\nA --> B\n```"
            );
            assert_eq!(visible[2].entity.read(cx).kind(), BlockKind::Paragraph);
            assert_eq!(
                editor.document.markdown_text(cx),
                "before\n\n```mermaid\nflowchart LR\nA --> B\n```\n\nafter"
            );
        });
    }

    #[gpui::test]
    async fn imports_tilde_mmd_fence_as_native_mermaid_block(cx: &mut TestAppContext) {
        let markdown = "~~~MMD\nflowchart LR\nA --> B\n~~~".to_string();
        let editor = cx.new(|cx| Editor::from_markdown(cx, markdown.clone(), None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 1);
            assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::MermaidBlock);
            assert_eq!(editor.document.markdown_text(cx), markdown);
        });
    }

    #[gpui::test]
    async fn regular_fenced_code_is_not_mermaid(cx: &mut TestAppContext) {
        let markdown = "```rust\nfn main() {}\n```".to_string();
        let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 1);
            assert!(matches!(
                visible[0].entity.read(cx).kind(),
                BlockKind::CodeBlock { .. }
            ));
        });
    }

    #[gpui::test]
    async fn imports_details_html_block_with_blank_lines_as_native_html_block(
        cx: &mut TestAppContext,
    ) {
        let markdown =
            "<details>\n<summary>Title</summary>\n\nHidden content with `code`.\n\n</details>"
                .to_string();
        let editor = cx.new(|cx| Editor::from_markdown(cx, markdown.clone(), None));

        editor.update(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 1);
            assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::HtmlBlock);
            assert_eq!(visible[0].entity.read(cx).display_text(), markdown);
            assert!(
                visible[0]
                    .entity
                    .read(cx)
                    .record
                    .html
                    .as_ref()
                    .is_some_and(|html| html.is_semantic())
            );
            assert_eq!(editor.document.markdown_text(cx), markdown);
        });
    }

    #[gpui::test]
    async fn centered_div_keeps_the_following_markdown_table(cx: &mut TestAppContext) {
        let markdown = "<div align=\"center\">\n\n| 日期 | 版本 |\n| :---: | :---: |\n| 2026-08-04 | 1.0 |\n\n</div>".to_string();
        let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

        editor.update(cx, |editor, cx| {
            let roots = editor.document.root_blocks();
            assert_eq!(roots.len(), 3);
            assert_eq!(roots[0].read(cx).kind(), BlockKind::HtmlBlock);
            assert_eq!(roots[1].read(cx).kind(), BlockKind::Table);
            // The stray closing tag keeps its text but draws no row.
            let stray = roots[2].read(cx);
            assert_eq!(stray.kind(), BlockKind::HtmlBlock);
            assert!(stray.renders_nothing());

            let text = editor.document.markdown_text(cx);
            assert!(text.contains("| 日期 | 版本 |"), "actual: {text}");
            assert!(text.contains("</div>"), "actual: {text}");
        });
    }

