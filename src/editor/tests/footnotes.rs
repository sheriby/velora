use super::common::*;

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

