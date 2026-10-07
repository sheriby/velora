//! LaTeX 编辑三件套：等宽字体、`\` 命令补全、公式编辑器面板。

use super::common::*;
use crate::components::latex::LATEX_SYMBOLS;

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
        // 确认是一次不可合并的 undo 组：连续确认不能被合并成一步。
        assert_eq!(
            editor.undo_history.len(),
            1,
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
/// 双击数学块打开公式编辑器弹窗——走完整渲染路径（双击事件 → 块事件 →
/// pending → 下一帧弹窗上屏），debug_bounds 查面板选择器。
#[gpui::test]
async fn double_click_math_block_opens_formula_editor(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "$$\nx^2\n$$\n".into(), None));
    redraw(cx);

    let math = editor
        .read_with(cx, |editor, cx| math_block_entity(editor, cx))
        .expect("夹具应有一个数学块");
    editor.update(cx, |editor, cx| editor.focus_block(math.entity_id()));
    redraw(cx);

    // 双击：两次按下同一位点，第二次 click_count = 2（事件直接构造，
    // position 落在块内任意处）。
    let down = |count: usize| gpui::MouseDownEvent {
        position: gpui::point(gpui::px(40.0), gpui::px(40.0)),
        button: gpui::MouseButton::Left,
        modifiers: gpui::Modifiers::default(),
        click_count: count,
        first_mouse: count == 1,
    };
    let down_first = down(1);
    let down_second = down(2);
    editor.update_in(cx, |editor, window, cx| {
        math.update(cx, |block, block_cx| {
            block.on_mouse_down(&down_first, window, block_cx);
        });
        let _ = editor;
    });
    redraw(cx);
    editor.update_in(cx, |editor, window, cx| {
        math.update(cx, |block, block_cx| {
            block.on_mouse_down(&down_second, window, block_cx);
        });
        let _ = editor;
    });
    redraw(cx);

    editor.read_with(cx, |editor, _cx| {
        assert!(
            editor.formula_editor.is_some(),
            "双击数学块应打开公式编辑器弹窗"
        );
    });
    assert!(
        cx.debug_bounds("formula-editor-panel").is_some(),
        "弹窗应真实上屏（debug_selector formula-editor-panel）"
    );
}

/// 弹窗里编辑草稿：预览跟着渲染，「应用」把草稿写回数学块（一次 undo 组）。
#[gpui::test]
async fn formula_editor_draft_previews_and_applies(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "$$\n\n$$\n".into(), None));
    redraw(cx);

    let math = editor
        .read_with(cx, |editor, cx| math_block_entity(editor, cx))
        .expect("夹具应有一个数学块");
    editor.update_in(cx, |editor, window, cx| {
        editor.open_formula_editor_for_block(math.entity_id(), window, cx);
    });

    // 点符号面板的 frac：草稿出现模板，光标落进第一对花括号，预览渲染出来。
    let frac = &LATEX_SYMBOLS[0];
    assert_eq!(frac.name, "frac");
    editor.update(cx, |editor, cx| editor.insert_formula_symbol(frac, cx));
    editor.read_with(cx, |editor, _cx| {
        let state = editor.formula_editor.as_ref().expect("弹窗该开着");
        assert_eq!(state.draft, "\\frac{}{}");
        assert_eq!(state.selected_range.start, "\\frac{".len());
        assert!(
            state.preview_path.is_some(),
            "非空草稿应有渲染预览"
        );
    });

    // 「应用」：草稿写回块（多行形式），弹窗关闭，撤销栈多一组。
    let undo_before = editor.read_with(cx, |editor, _cx| editor.undo_history.len());
    editor.update_in(cx, |editor, window, cx| editor.apply_formula_editor(window, cx));
    editor.read_with(cx, |editor, cx| {
        assert!(editor.formula_editor.is_none(), "应用后弹窗该关闭");
        let text = math.read(cx).display_text();
        assert_eq!(
            text, "$$\n\\frac{}{}\n$$",
            "草稿应写回数学块，实际 {text:?}"
        );
        assert_eq!(
            editor.undo_history.len(),
            undo_before + 1,
            "应用应落一次独立 undo 组"
        );
    });
}

/// 取消（Esc 路径同一函数）：草稿丢弃，块文本不变。
#[gpui::test]
async fn formula_editor_cancel_keeps_block_unchanged(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "$$\nF = ma\n$$\n".into(), None));
    redraw(cx);

    let math = editor
        .read_with(cx, |editor, cx| math_block_entity(editor, cx))
        .expect("夹具应有一个数学块");
    editor.update_in(cx, |editor, window, cx| {
        editor.open_formula_editor_for_block(math.entity_id(), window, cx);
    });
    editor.read_with(cx, |editor, _cx| {
        let state = editor.formula_editor.as_ref().expect("弹窗开着");
        eprintln!("PROBE draft after open = {:?}", state.draft);
        eprintln!("PROBE block display = {:?}", math.read(_cx).display_text());
    });
    editor.update(cx, |editor, cx| {
        let len = editor
            .formula_editor
            .as_ref()
            .map(|state| state.draft.len())
            .unwrap_or(0);
        editor.replace_formula_draft(0..len, "E = mc^2", None, false, cx);
    });
    editor.read_with(cx, |editor, _cx| {
        let state = editor.formula_editor.as_ref().expect("弹窗开着");
        assert_eq!(state.draft, "E = mc^2");
    });

    editor.update(cx, |editor, cx| editor.close_formula_editor(cx));
    editor.read_with(cx, |editor, cx| {
        assert!(editor.formula_editor.is_none(), "取消后弹窗该关闭");
        let text = math.read(cx).display_text();
        assert_eq!(
            text, "$$\nF = ma\n$$",
            "取消不得改动块文本，实际 {text:?}"
        );
    });
}
