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

/// 公式编辑器面板：从公式块上打开并绑定该块，插入跟随块内**当前光标**，
/// 再点一次按钮收起。
#[gpui::test]
async fn formula_panel_binds_block_and_follows_caret(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "$$\n\n$$\n".into(), None));
    redraw(cx);

    let math = editor
        .read_with(cx, |editor, cx| math_block_entity(editor, cx))
        .expect("夹具应有一个数学块");
    editor.update(cx, |editor, cx| {
        editor.focus_block(math.entity_id());
        editor.toggle_formula_panel_for_block(math.entity_id(), cx);
    });

    editor.read_with(cx, |editor, _cx| {
        let state = editor.formula_panel.as_ref().expect("面板该打开");
        assert_eq!(state.target, math.entity_id(), "面板该绑定这个公式块");
    });

    // 光标在块首：点 frac 模板落进第一对花括号。
    let frac = &LATEX_SYMBOLS[0];
    assert_eq!(frac.name, "frac");
    math.update(cx, |block, block_cx| block.move_to(3, block_cx));
    editor.update(cx, |editor, cx| editor.insert_latex_symbol(frac, cx));
    editor.read_with(cx, |_editor, cx| {
        let text = math.read(cx).display_text();
        assert_eq!(
            text, "$$\n\\frac{}{}\n$$",
            "第一格该写进光标处，实际 {text:?}"
        );
    });

    // 光标挪到别处再点一格：插入跟随**当前**光标，不是冻结的旧插入点。
    math.update(cx, |block, block_cx| {
        let end = block.visible_len().saturating_sub(3); // 闭 $$ 之前
        block.move_to(end, block_cx);
    });
    let alpha = LATEX_SYMBOLS
        .iter()
        .find(|entry| entry.name == "alpha")
        .expect("符号表该有 alpha");
    editor.update(cx, |editor, cx| editor.insert_latex_symbol(alpha, cx));
    editor.read_with(cx, |editor, cx| {
        let text = math.read(cx).display_text();
        assert_eq!(
            text, "$$\n\\frac{}{}\\alpha \n$$",
            "第二格该跟着新光标走，实际 {text:?}"
        );
        assert!(
            editor.formula_panel.is_some(),
            "插入后面板保持打开便于连续输入"
        );
    });

    // 再点一次按钮：同块的面板收起。
    editor.update(cx, |editor, cx| {
        editor.toggle_formula_panel_for_block(math.entity_id(), cx);
    });
    editor.read_with(cx, |editor, _cx| {
        assert!(editor.formula_panel.is_none(), "同块再开一次该是收起");
    });
}
/// 面板绑定的块被删掉后，下一次点击自动收面板而不是 panic。
#[gpui::test]
async fn formula_panel_closes_when_target_block_is_gone(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "$$\nx\n$$\n".into(), None));
    redraw(cx);

    let math = editor
        .read_with(cx, |editor, cx| math_block_entity(editor, cx))
        .expect("夹具应有一个数学块");
    editor.update(cx, |editor, cx| {
        editor.toggle_formula_panel_for_block(math.entity_id(), cx);
    });
    editor.read_with(cx, |editor, _cx| {
        assert!(editor.formula_panel.is_some(), "面板该先打开");
    });

    // 模拟目标块从树里消失：把会话里的目标指到一个不在树上的实体 id。
    editor.update(cx, |editor, _cx| {
        if let Some(state) = editor.formula_panel.as_mut() {
            state.target = gpui::EntityId::from(u64::MAX);
        }
    });
    editor.update(cx, |editor, cx| {
        editor.insert_latex_symbol(&LATEX_SYMBOLS[0], cx);
    });
    editor.read_with(cx, |editor, _cx| {
        assert!(
            editor.formula_panel.is_none(),
            "目标块不存在时点击该收面板"
        );
    });
}
/// 「ƒx 符号」按钮发出的 BlockEvent::RequestFormulaPanel 走编辑器事件臂
/// 打开/关闭面板——按钮与面板之间的链路不能断。
#[gpui::test]
async fn fx_button_event_toggles_formula_panel(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "$$\nx\n$$\n".into(), None));
    redraw(cx);

    let math = editor
        .read_with(cx, |editor, cx| math_block_entity(editor, cx))
        .expect("夹具应有一个数学块");
    let event = crate::components::BlockEvent::RequestFormulaPanel;
    editor.update(cx, |editor, cx| {
        editor.on_block_event(math.clone(), &event, cx);
    });
    editor.read_with(cx, |editor, _cx| {
        assert!(
            editor.formula_panel.is_some(),
            "事件应经编辑器事件臂打开面板"
        );
    });

    editor.update(cx, |editor, cx| {
        editor.on_block_event(math.clone(), &event, cx);
    });
    editor.read_with(cx, |editor, _cx| {
        assert!(editor.formula_panel.is_none(), "再发一次该收起");
    });
}
