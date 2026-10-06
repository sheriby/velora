//! LaTeX 编辑三件套：等宽字体、`\` 命令补全、公式编辑器面板。

use super::common::*;

fn block_with_text(
    editor: &Editor,
    cx: &gpui::App,
    needle: &str,
) -> Option<gpui::Entity<crate::components::Block>> {
    editor
        .document
        .visible_blocks()
        .iter()
        .find(|visible| visible.entity.read(cx).display_text().contains(needle))
        .map(|visible| visible.entity.clone())
}

fn math_block_entity(
    editor: &Editor,
    cx: &gpui::App,
) -> Option<gpui::Entity<crate::components::Block>> {
    editor
        .document
        .visible_blocks()
        .iter()
        .find(|visible| {
            matches!(
                visible.entity.read(cx).kind(),
                crate::components::BlockKind::MathBlock
            )
        })
        .map(|visible| visible.entity.clone())
}

/// 数学块里打 `\al` 弹补全，Enter 确认 `\alpha ` 并落在尾随空格之后。
#[gpui::test]
async fn latex_completion_confirms_template_in_math_block(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "$$\n\\al\n$$\n".into(), None));
    redraw(cx);

    let math = editor
        .read_with(cx, |editor, cx| math_block_entity(editor, cx))
        .expect("夹具应有一个数学块");
    let cursor = math.read_with(cx, |block, _cx| {
        block.display_text().find("\\al").expect("\\al 在块里") + "\\al".len()
    });
    math.update(cx, |block, block_cx| block.move_to(cursor, block_cx));
    editor.update(cx, |editor, cx| {
        editor.update_latex_completion_for_block(&math, cx);
    });

    let (open, first, anchor) = editor.read_with(cx, |editor, _cx| {
        match editor.latex_completion.as_ref() {
            Some(state) => (
                true,
                state.results.first().map(|entry| entry.name),
                state.anchor,
            ),
            None => (false, None, 0),
        }
    });
    assert!(open, "数学块里的 \\al 应弹出补全");
    assert_eq!(first, Some("alpha"));
    assert_eq!(anchor, 3, "锚点应在反斜杠上");

    // Enter 确认：查询串换成模板，光标落到插入文本的落点上。
    editor.update(cx, |editor, cx| {
        let keystroke = gpui::Keystroke::parse("enter").expect("enter keystroke");
        assert!(editor.latex_completion_key_down(&keystroke, cx));
    });
    editor.read_with(cx, |editor, _cx| {
        assert!(!editor.latex_completion_is_open(), "确认后浮层该收起");
        let text = math.read(_cx).display_text();
        assert_eq!(
            text, "$$\n\\alpha \n$$",
            "确认该把 \\al 换成 \\alpha 加尾随空格，实际 {text:?}"
        );
    });
}

/// 普通段落里的反斜杠不弹补全；进了行内 `$...$` 才弹。
#[gpui::test]
async fn latex_completion_only_opens_in_math_context(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "普通 \\al 文本\n\n价格 $x\\al$\n".into(), None)
    });
    redraw(cx);

    let plain = editor.read_with(cx, |editor, cx| {
        block_with_text(editor, cx, "普通")
            .expect("夹具应有普通文本块")
    });
    let plain_cursor = plain.read_with(cx, |block, _cx| {
        block.display_text().find("\\al").expect("\\al 在块里") + "\\al".len()
    });
    plain.update(cx, |block, block_cx| block.move_to(plain_cursor, block_cx));
    editor.update(cx, |editor, cx| {
        editor.update_latex_completion_for_block(&plain, cx);
    });
    editor.read_with(cx, |editor, _cx| {
        assert!(
            editor.latex_completion.is_none(),
            "普通文本里的 \\al 不该弹补全"
        );
    });

    // 行内公式那一段：反斜杠在 $...$ 里才弹。
    let inline = editor.read_with(cx, |editor, cx| {
        block_with_text(editor, cx, "$x\\al").expect("夹具应有行内公式块")
    });
    let inline_cursor = inline.read_with(cx, |block, _cx| {
        block.display_text().find("\\al").expect("\\al 在块里") + "\\al".len()
    });
    inline.update(cx, |block, block_cx| block.move_to(inline_cursor, block_cx));
    editor.update(cx, |editor, cx| {
        editor.update_latex_completion_for_block(&inline, cx);
    });
    editor.read_with(cx, |editor, _cx| {
        assert!(
            editor.latex_completion.is_some(),
            "行内公式里的 \\al 该弹补全"
        );
    });
}

