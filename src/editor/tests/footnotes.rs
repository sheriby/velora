use super::common::*;

#[gpui::test]
async fn review_footnote_references_inside_table_cells_resolve(cx: &mut TestAppContext) {
    // 表格格子不在可见块树内，脚注注册表曾漏扫它们。
    let source = "| [^a] |\n| --- |\n| [^a] |\n\n[^a]: 注释";
    let editor = cx.new(|cx| Editor::from_markdown(cx, source.into(), None));
    editor.read_with(cx, |editor, cx| {
        let runtime = editor.document.first_root().unwrap().read(cx).table_runtime.as_ref().unwrap().clone();
        for cell in [&runtime.header[0], &runtime.rows[0][0]] {
            let block = cell.read(cx);
            assert_eq!(block.inline_footnote_hit_at(0).map(|hit| hit.ordinal), Some(1));
            assert_eq!(block.display_text(), "¹");
        }
    });
}

#[gpui::test]
async fn root_level_footnotes_number_by_first_reference_and_render_in_place(
    cx: &mut TestAppContext,
) {
    let markdown = [
        "Here is a footnote reference.[^1]",
        "",
        "Here is another footnote reference.[^longnote]",
        "",
        "A footnote can appear after multiple paragraphs, lists, and code blocks.",
        "",
        "[^1]: Footnote text.",
        "",
        "[^longnote]: Footnote text with **bold**, `code`, and a nested list:",
        "    - item 1",
        "    - item 2",
        "    ",
        "    Second paragraph in the footnote.",
    ]
    .join("\n");
    let canonical_markdown = [
        "Here is a footnote reference.[^1]",
        "",
        "Here is another footnote reference.[^longnote]",
        "",
        "A footnote can appear after multiple paragraphs, lists, and code blocks.",
        "",
        "[^1]: Footnote text.",
        "",
        "[^longnote]: Footnote text with **bold**, `code`, and a nested list:",
        "",
        "    - item 1",
        "    - item 2",
        "",
        "    Second paragraph in the footnote.",
    ]
    .join("\n");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown.clone(), None));

    editor.read_with(cx, |editor, cx| {
        let visible = editor.document.visible_blocks();

        let first_ref = visible
            .iter()
            .find(|visible| {
                visible
                    .entity
                    .read(cx)
                    .display_text()
                    .contains("Here is a footnote reference.")
            })
            .expect("first footnote reference")
            .entity
            .clone();
        assert_eq!(
            first_ref.read(cx).display_text(),
            format!("Here is a footnote reference.{}", superscript_ordinal(1))
        );

        let second_ref = visible
            .iter()
            .find(|visible| {
                visible
                    .entity
                    .read(cx)
                    .display_text()
                    .contains("Here is another footnote reference.")
            })
            .expect("second footnote reference")
            .entity
            .clone();
        assert_eq!(
            second_ref.read(cx).display_text(),
            format!(
                "Here is another footnote reference.{}",
                superscript_ordinal(2)
            )
        );

        let footnote_defs = visible
            .iter()
            .filter_map(|visible| {
                let block = visible.entity.read(cx);
                (block.kind() == BlockKind::FootnoteDefinition).then_some(visible.entity.clone())
            })
            .collect::<Vec<_>>();
        assert_eq!(footnote_defs.len(), 2);
        assert_eq!(footnote_defs[0].read(cx).display_text(), "1");
        assert_eq!(
            footnote_defs[0].read(cx).footnote_definition_ordinal(),
            Some(1)
        );
        assert_eq!(footnote_defs[1].read(cx).display_text(), "longnote");
        assert_eq!(
            footnote_defs[1].read(cx).footnote_definition_ordinal(),
            Some(2)
        );

        assert_eq!(editor.document.markdown_text(cx), canonical_markdown);
    });
}

#[gpui::test]
async fn callout_footnotes_number_and_render_in_place(cx: &mut TestAppContext) {
    let markdown = [
        "> [!WARNING]",
        "> Callout footnote reference.[^final]",
        "> ",
        "> [^final]: Nested footnote text.",
        "> Tail paragraph.",
    ]
    .join("\n");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown.clone(), None));

    editor.read_with(cx, |editor, cx| {
        let visible = editor.document.visible_blocks();

        let reference_block = visible
            .iter()
            .find(|visible| {
                visible
                    .entity
                    .read(cx)
                    .display_text()
                    .contains("Callout footnote reference.")
            })
            .expect("callout footnote reference")
            .entity
            .clone();
        assert_eq!(
            reference_block.read(cx).display_text(),
            format!("Callout footnote reference.{}", superscript_ordinal(1))
        );

        let definition = visible
            .iter()
            .find(|visible| visible.entity.read(cx).kind() == BlockKind::FootnoteDefinition)
            .expect("callout footnote definition")
            .entity
            .clone();
        assert_eq!(definition.read(cx).display_text(), "final");
        assert_eq!(definition.read(cx).quote_depth, 1);
        assert_eq!(definition.read(cx).footnote_definition_ordinal(), Some(1));
        assert_eq!(editor.document.markdown_text(cx), markdown);
    });
}

#[gpui::test]
async fn root_reference_binds_to_nested_quote_footnote_definition(cx: &mut TestAppContext) {
    let markdown = "Root reference.[^note]\n\n> [^note]: Nested quote footnote".to_string();
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown.clone(), None));

    editor.read_with(cx, |editor, cx| {
        let visible = editor.document.visible_blocks();

        let root_reference = visible
            .iter()
            .find(|visible| visible.entity.read(cx).quote_depth == 0)
            .expect("root reference block")
            .entity
            .clone();
        assert_eq!(
            root_reference.read(cx).display_text(),
            format!("Root reference.{}", superscript_ordinal(1))
        );

        let definition = visible
            .iter()
            .find(|visible| visible.entity.read(cx).kind() == BlockKind::FootnoteDefinition)
            .expect("nested quote footnote definition")
            .entity
            .clone();
        assert_eq!(definition.read(cx).display_text(), "note");
        assert_eq!(definition.read(cx).quote_depth, 1);
        assert_eq!(definition.read(cx).footnote_definition_ordinal(), Some(1));
        assert_eq!(editor.document.markdown_text(cx), markdown);
    });
}

#[gpui::test]
async fn unresolved_footnote_reference_stays_literal_and_unlinked(cx: &mut TestAppContext) {
    let markdown = "Missing footnote[^missing].".to_string();
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown.clone(), None));

    editor.read_with(cx, |editor, cx| {
        let block = editor
            .document
            .first_root()
            .expect("root paragraph")
            .clone();
        assert_eq!(block.read(cx).display_text(), markdown);
        assert!(
            block
                .read(cx)
                .inline_footnote_hit_at("Missing footnote".len())
                .is_none()
        );
        assert!(editor.footnote_registry.binding("missing").is_none());
        assert_eq!(editor.document.markdown_text(cx), markdown);
    });
}


/// 定义键取自标题的可见文本、引用键取自解析器的脚注 id：两边都要走同一次
/// 反转义。转义 id 与普通 id 交叉引用时都必须绑定成功，不能静默降级。
#[gpui::test]
async fn escaped_footnote_ids_bind_between_definition_and_reference(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    // 两侧写法一致（都带转义）时按同一把尺绑定；转义与非转义是两个字面 id，
    // 与解析器口径一致，不做隐式归一。
    for source in ["引用[^a\\*b]\n\n[^a\\*b]: 注"] {
        let editor = cx.new(|cx| Editor::from_markdown(cx, source.to_string(), None));
        editor.read_with(cx, |editor, cx| {
            let reference = editor
                .document
                .visible_blocks()
                .iter()
                .find(|visible| visible.entity.read(cx).display_text().contains("引用"))
                .expect("引用块")
                .entity
                .clone();
            assert!(
                reference
                    .read(cx)
                    .display_text()
                    .chars()
                    .any(|ch| ('¹'..='⁹').contains(&ch)),
                "{source:?} 的引用没有解析成脚注：{:?}",
                reference.read(cx).display_text()
            );
            let definition = editor
                .document
                .visible_blocks()
                .iter()
                .find(|visible| visible.entity.read(cx).kind() == BlockKind::FootnoteDefinition)
                .expect("定义块")
                .entity
                .clone();
            assert!(
                definition.read(cx).footnote_definition_ordinal().is_some(),
                "{source:?} 的定义没有绑上序号"
            );
        });
    }
}
