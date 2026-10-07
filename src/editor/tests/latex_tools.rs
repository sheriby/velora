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

/// 数学块里补全开着时 Tab 该确认补全，而不是插一个制表符（Tab 平时是缩进
/// 绑定；补全的按键走 intercept_keystrokes，先于绑定解析，能抢下来）。
#[gpui::test]
async fn latex_completion_tab_confirms_in_math_block(cx: &mut TestAppContext) {
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

    let consumed = editor.update(cx, |editor, cx| {
        let tab = gpui::Keystroke::parse("tab").expect("tab keystroke");
        editor.latex_completion_key_down(&tab, cx)
    });
    assert!(consumed, "补全开着时 Tab 该被补全消费掉");
    editor.read_with(cx, |editor, cx| {
        assert!(!editor.latex_completion_is_open(), "确认后浮层该收起");
        assert_eq!(
            math.read(cx).display_text(),
            "$$\n\\alpha \n$$",
            "Tab 该插入模板而不是制表符"
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
    editor.update(cx, |editor, _cx| editor.focus_block(math.entity_id()));
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
/// 聚焦数学块后，右上角的 ƒx 标记必须真的上屏（走完整渲染路径）——
/// 点击与双击是公式编辑器的两个入口。
#[gpui::test]
async fn focused_math_block_shows_fx_button_on_screen(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "$$\nx^2\n$$\n".into(), None));
    redraw(cx);

    let math = editor
        .read_with(cx, |editor, cx| math_block_entity(editor, cx))
        .expect("夹具应有一个数学块");
    editor.update(cx, |editor, _cx| editor.focus_block(math.entity_id()));
    redraw(cx);

    assert!(
        cx.debug_bounds("math-fx-button").is_some(),
        "聚焦数学块应渲染出 ƒx 标记（debug_selector math-fx-button）"
    );
}

/// 弹窗草稿里打 `\` 弹命令补全，Enter 确认替换查询串（光标按模板落点）。
#[gpui::test]
async fn formula_editor_draft_completion_confirms(cx: &mut TestAppContext) {
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

    editor.update(cx, |editor, cx| {
        editor.replace_formula_draft(0..0, "\\fra", None, false, cx);
    });
    editor.read_with(cx, |editor, _cx| {
        let state = editor.formula_editor.as_ref().expect("弹窗开着");
        let completion = state.completion.as_ref().expect("\\fra 应弹补全");
        assert_eq!(completion.anchor, 0, "锚在反斜杠上");
        assert!(completion.results.iter().any(|entry| entry.name == "frac"));
    });

    editor.update(cx, |editor, cx| editor.confirm_formula_draft_completion(0, cx));
    editor.read_with(cx, |editor, _cx| {
        let state = editor.formula_editor.as_ref().expect("弹窗开着");
        assert_eq!(state.draft, "\\frac{}{}");
        assert_eq!(state.selected_range.start, "\\frac{".len());
        assert!(state.completion.is_none(), "确认后补全该收起");
    });
}

/// 草稿能删除：退格删光标前一字符（DeleteBack 动作绑定在块编辑器 context，
/// 焦点在弹窗时不派发——删除由弹窗自管）。
#[gpui::test]
async fn formula_editor_draft_backspace_deletes(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "$$\nabc\n$$\n".into(), None));
    redraw(cx);

    let math = editor
        .read_with(cx, |editor, cx| math_block_entity(editor, cx))
        .expect("夹具应有一个数学块");
    editor.update_in(cx, |editor, window, cx| {
        editor.open_formula_editor_for_block(math.entity_id(), window, cx);
    });
    editor.read_with(cx, |editor, _cx| {
        let state = editor.formula_editor.as_ref().expect("弹窗开着");
        assert_eq!(state.draft, "abc");
    });

    let backspace = gpui::KeyDownEvent {
        keystroke: gpui::Keystroke::parse("backspace").expect("backspace"),
        is_held: false,
    };
    editor.update_in(cx, |editor, window, cx| {
        editor.formula_editor_key_down(&backspace, window, cx);
    });
    editor.read_with(cx, |editor, _cx| {
        let state = editor.formula_editor.as_ref().expect("弹窗开着");
        assert_eq!(state.draft, "ab", "退格应删掉 c");
    });
}

/// 打开弹窗并把焦点交给草稿输入，画一帧让焦点与几何都落地。窗口要先激活：
/// 光标只在「草稿聚焦 + 窗口是 key」时画（gpui 在窗口 resign key 时不清 focus）。
fn open_formula_editor(
    editor: &gpui::Entity<Editor>,
    math: &gpui::Entity<Block>,
    cx: &mut gpui::VisualTestContext,
) {
    editor.update_in(cx, |editor, window, cx| {
        editor.open_formula_editor_for_block(math.entity_id(), window, cx);
    });
    activate_visual_window(cx);
    redraw(cx);
}

/// 现象：草稿里只打一个 `\` 不弹补全，得再敲一个字母再删掉才弹。
/// 根因：敲字走 GPUI 的 `EntityInputHandler` → overlay 输入路径，那条路径
/// 只同步预览、不刷新 `\` 补全会话（补全只在 `replace_formula_draft` 里刷）。
#[gpui::test]
async fn formula_editor_completion_opens_on_first_backslash(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "$$\n\n$$\n".into(), None));
    redraw(cx);

    let math = editor
        .read_with(cx, |editor, cx| math_block_entity(editor, cx))
        .expect("夹具应有一个数学块");
    open_formula_editor(&editor, &math, cx);

    // 真实敲字：字符由焦点元素的输入处理器写进草稿。
    editor.update_in(cx, |editor, window, cx| {
        editor.replace_text_in_range(None, "\\", window, cx);
    });
    editor.read_with(cx, |editor, _cx| {
        let state = editor.formula_editor.as_ref().expect("弹窗该开着");
        assert_eq!(state.draft, "\\", "敲进去的该是反斜杠");
        let completion = state
            .completion
            .as_ref()
            .expect("只打一个 \\ 也该弹命令补全");
        assert_eq!(completion.anchor, 0, "补全该锚在这个反斜杠上");
        assert_eq!(
            completion.results.first().map(|entry| entry.name),
            Some("frac")
        );
    });
}

/// 现象：光标条比字高出一截（贴在行框顶上，而字在半行距里居中）。
/// 断言：光标条与它所在行的行框上下边对齐，换行后跟着下一行走。
#[gpui::test]
async fn formula_editor_caret_sits_in_its_line_box(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "$$\nF = ma \\frac{a}{b}\n$$\n".into(), None)
    });
    redraw(cx);

    let math = editor
        .read_with(cx, |editor, cx| math_block_entity(editor, cx))
        .expect("夹具应有一个数学块");
    open_formula_editor(&editor, &math, cx);

    editor.update(cx, |editor, cx| {
        let len = editor
            .formula_editor
            .as_ref()
            .map(|state| state.draft.len())
            .unwrap_or(0);
        editor.replace_formula_draft(len..len, " \\\\", None, false, cx);
    });
    redraw(cx);

    let vertical_drift = |cx: &mut gpui::VisualTestContext, line: &'static str| -> f32 {
        let caret = cx.debug_bounds("formula-editor-caret").expect("光标条该上屏");
        let row = cx.debug_bounds(line).expect("草稿行该上屏");
        let center = |bounds: gpui::Bounds<gpui::Pixels>| {
            (f32::from(bounds.top()) + f32::from(bounds.bottom())) / 2.0
        };
        center(caret) - center(row)
    };
    let drift = vertical_drift(&mut *cx, "formula-editor-line-0");
    assert!(
        drift.abs() <= 0.6,
        "光标条该与所在行行框上下对齐，实测偏 {drift}px"
    );

    // 换行后光标落到第二行：跟着第二行的行框，不是留在第一行的高度上。
    editor.update(cx, |editor, cx| {
        let end = editor
            .formula_editor
            .as_ref()
            .map(|state| state.draft.len())
            .unwrap_or(0);
        editor.replace_formula_draft(end..end, "\nF = ma", None, false, cx);
    });
    redraw(cx);
    let drift = vertical_drift(&mut *cx, "formula-editor-line-1");
    assert!(
        drift.abs() <= 0.6,
        "换行后光标条该与第二行行框对齐，实测偏 {drift}px"
    );
}

/// 现象：补全列表钉在面板左缘、横贯整个面板宽，离光标很远。
/// 断言：列表左缘贴着光标、在光标下方，宽度收窄且留在面板内。
#[gpui::test]
async fn formula_editor_completion_follows_the_caret(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "$$\n\n$$\n".into(), None));
    redraw(cx);

    let math = editor
        .read_with(cx, |editor, cx| math_block_entity(editor, cx))
        .expect("夹具应有一个数学块");
    open_formula_editor(&editor, &math, cx);

    let draft = "F = ma \\frac{a}{b} \\sqrt{x} \\int_{0}^{1} \\fr";
    editor.update(cx, |editor, cx| {
        editor.replace_formula_draft(0..0, draft, None, false, cx);
    });
    redraw(cx);

    let caret = cx.debug_bounds("formula-editor-caret").expect("光标条该上屏");
    let list = cx
        .debug_bounds("formula-editor-completion")
        .expect("补全列表该上屏");
    let panel = cx.debug_bounds("formula-editor-panel").expect("面板该上屏");
    let gap = f32::from(list.left()) - f32::from(caret.left());
    assert!(
        gap.abs() <= 8.0,
        "补全列表该贴着光标，实测左缘差 {gap}px"
    );
    assert!(
        f32::from(list.top()) >= f32::from(caret.bottom()) - 1.0,
        "补全列表该落在光标下方"
    );
    assert!(
        f32::from(list.size.width) <= 320.0,
        "补全列表不该铺满面板宽，实测 {}",
        f32::from(list.size.width)
    );
    assert!(
        f32::from(list.right()) <= f32::from(panel.right()) + 1.0,
        "补全列表该留在面板里"
    );
}

/// 焦点不在草稿上时，光标条不该还挂在屏上（以前无条件画，窗口失焦后尤其
/// 显眼）；聚焦时闪烁任务在跑，失焦就停。
#[gpui::test]
async fn formula_editor_caret_only_while_draft_is_focused(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "$$\nF = ma\n$$\n".into(), None)
    });
    redraw(cx);

    let math = editor
        .read_with(cx, |editor, cx| math_block_entity(editor, cx))
        .expect("夹具应有一个数学块");
    open_formula_editor(&editor, &math, cx);

    assert!(
        cx.debug_bounds("formula-editor-caret").is_some(),
        "聚焦草稿时光标条该上屏"
    );
    editor.read_with(cx, |editor, _cx| {
        let state = editor.formula_editor.as_ref().expect("弹窗开着");
        assert!(
            state.caret_blink_task.is_some(),
            "聚焦时光标闪烁任务该在跑"
        );
    });

    // 窗口失焦（截图里的状态：红绿灯是灰的，光标却还亮着）。
    editor.update_in(cx, |_editor, window, _cx| window.blur());
    redraw(cx);
    assert!(
        cx.debug_bounds("formula-editor-caret").is_none(),
        "失焦后光标条不该还画着"
    );
    editor.read_with(cx, |editor, _cx| {
        let state = editor.formula_editor.as_ref().expect("弹窗开着");
        assert!(
            state.caret_blink_task.is_none(),
            "失焦后闪烁任务该停掉（Task drop 即取消）"
        );
    });
}

/// 草稿输入区要能用鼠标点光标、拖出选区（以前按下只吞事件，光标永远停在
/// 末尾，也没有任何选区渲染）。
#[gpui::test]
async fn formula_editor_mouse_click_and_drag_select(cx: &mut TestAppContext) {
    use crate::editor::formula_editor::{INPUT_CONTENT_INSET_X, INPUT_FONT_SIZE};

    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "$$\n\n$$\n".into(), None));
    redraw(cx);

    let math = editor
        .read_with(cx, |editor, cx| math_block_entity(editor, cx))
        .expect("夹具应有一个数学块");
    open_formula_editor(&editor, &math, cx);
    let draft = "F = ma \\frac{a}{b}";
    editor.update(cx, |editor, cx| {
        editor.replace_formula_draft(0..0, draft, None, false, cx);
    });
    redraw(cx);

    // 「F = ma」这 6 个字节的宽度用同一套字体量出来，点击点落在它右边一点。
    let prefix_width = editor.update_in(cx, |_editor, window, _cx| {
        let fonts = crate::config::EditorSettings::fonts(_cx).code_family;
        let run = gpui::TextRun {
            len: 6,
            font: gpui::font(fonts.clone()),
            color: gpui::black(),
            background_color: None,
            underline: None,
            strikethrough: None,
            font_size: None,
        };
        window
            .text_system()
            .shape_line("F = ma".into(), gpui::px(INPUT_FONT_SIZE), &[run], None)
            .width
    });
    let input = cx
        .debug_bounds("formula-editor-input")
        .expect("草稿输入区该上屏");
    let click = gpui::point(
        input.left() + gpui::px(INPUT_CONTENT_INSET_X) + prefix_width + gpui::px(1.0),
        input.top() + gpui::px(12.0),
    );
    cx.simulate_mouse_down(click, gpui::MouseButton::Left, gpui::Modifiers::none());
    cx.simulate_mouse_up(click, gpui::MouseButton::Left, gpui::Modifiers::none());
    redraw(cx);
    editor.read_with(cx, |editor, _cx| {
        let state = editor.formula_editor.as_ref().expect("弹窗开着");
        assert_eq!(
            state.selected_range,
            6..6,
            "点在「F = ma」之后该把光标放在偏移 6"
        );
        assert!(
            !state.selecting_with_mouse,
            "抬手之后拖动状态该结束"
        );
    });

    // 从 6 拖到行尾之后：选区 6..草稿长度，并且真的画出选区色块。
    let tail_end = gpui::point(input.right() - gpui::px(2.0), input.top() + gpui::px(12.0));
    cx.simulate_mouse_down(click, gpui::MouseButton::Left, gpui::Modifiers::none());
    cx.simulate_mouse_move(tail_end, gpui::MouseButton::Left, gpui::Modifiers::none());
    redraw(cx);
    editor.read_with(cx, |editor, _cx| {
        let state = editor.formula_editor.as_ref().expect("弹窗开着");
        assert_eq!(
            state.selected_range,
            6..draft.len(),
            "拖动该把选区从锚点拉到指针处"
        );
    });
    assert!(
        cx.debug_bounds("formula-editor-caret").is_none(),
        "有选区时光标条该收起（选区本身按 run 底色画在文字上）"
    );
    cx.simulate_mouse_up(tail_end, gpui::MouseButton::Left, gpui::Modifiers::none());
    editor.read_with(cx, |editor, _cx| {
        let state = editor.formula_editor.as_ref().expect("弹窗开着");
        assert!(!state.selecting_with_mouse, "抬手后不再跟随指针");
        assert_eq!(state.selected_range, 6..draft.len(), "抬手不该改变选区");
    });

    // 选区里的退格删掉整段（删除路径本来就吃选区，这里是守住新选区接得上）。
    editor.update_in(cx, |editor, window, cx| {
        let backspace = gpui::KeyDownEvent {
            keystroke: gpui::Keystroke::parse("backspace").expect("backspace"),
            is_held: false,
        };
        editor.formula_editor_key_down(&backspace, window, cx);
    });
    editor.read_with(cx, |editor, _cx| {
        let state = editor.formula_editor.as_ref().expect("弹窗开着");
        assert_eq!(state.draft, "F = ma", "退格该删掉整段选区");
    });
}

/// 双击选词：落在「frac」这串字母里，选中的就是这 4 个字母。
#[gpui::test]
async fn formula_editor_double_click_selects_word(cx: &mut TestAppContext) {
    use crate::editor::formula_editor::{INPUT_CONTENT_INSET_X, INPUT_FONT_SIZE};

    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "$$\n\n$$\n".into(), None));
    redraw(cx);

    let math = editor
        .read_with(cx, |editor, cx| math_block_entity(editor, cx))
        .expect("夹具应有一个数学块");
    open_formula_editor(&editor, &math, cx);
    editor.update(cx, |editor, cx| {
        editor.replace_formula_draft(0..0, "F = ma \\frac{a}{b}", None, false, cx);
    });
    redraw(cx);

    let input = cx
        .debug_bounds("formula-editor-input")
        .expect("草稿输入区该上屏");
    let word_x = editor.update_in(cx, |_editor, window, _cx| {
        let fonts = crate::config::EditorSettings::fonts(_cx).code_family;
        let run = gpui::TextRun {
            len: 9,
            font: gpui::font(fonts.clone()),
            color: gpui::black(),
            background_color: None,
            underline: None,
            strikethrough: None,
            font_size: None,
        };
        window
            .text_system()
            .shape_line("F = ma \\f".into(), gpui::px(INPUT_FONT_SIZE), &[run], None)
            .width
    });
    let word_point = gpui::point(
        input.left() + gpui::px(INPUT_CONTENT_INSET_X) + word_x,
        input.top() + gpui::px(12.0),
    );
    cx.simulate_event(gpui::MouseDownEvent {
        position: word_point,
        button: gpui::MouseButton::Left,
        modifiers: gpui::Modifiers::none(),
        click_count: 2,
        first_mouse: false,
    });
    cx.simulate_mouse_up(word_point, gpui::MouseButton::Left, gpui::Modifiers::none());
    editor.read_with(cx, |editor, _cx| {
        let state = editor.formula_editor.as_ref().expect("弹窗开着");
        assert_eq!(
            &state.draft[state.selected_range.clone()],
            "frac",
            "双击该选中指针所在的那个词"
        );
    });
}

/// 长行软换行后，光标与指针命中都要按「视觉行」算。以前按一硬行一行算：
/// 单行长公式的末尾光标被放到整行未换行的宽度上（跑到框外右侧看不见），
/// 点第二行也会算成第一行里的某个偏移。
#[gpui::test]
async fn formula_editor_caret_and_hit_test_follow_soft_wrap(cx: &mut TestAppContext) {
    use crate::editor::formula_editor::{
        INPUT_CONTENT_INSET_X, INPUT_LINE_HEIGHT, INPUT_PADDING_Y,
    };

    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "$$\n\n$$\n".into(), None));
    redraw(cx);

    let math = editor
        .read_with(cx, |editor, cx| math_block_entity(editor, cx))
        .expect("夹具应有一个数学块");
    open_formula_editor(&editor, &math, cx);
    // 一句会撑满两三个视觉行的长公式（无换行符，纯软换行）。
    let draft = "F = ma \\frac{a}{b} \\int_{0}^{1} ma \\frac{a}{b} \\prod_{a}^{b} ".repeat(4);
    editor.update(cx, |editor, cx| {
        editor.replace_formula_draft(0..0, &draft, None, false, cx);
    });
    redraw(cx);

    let input = cx
        .debug_bounds("formula-editor-input")
        .expect("草稿输入区该上屏");
    // 末尾光标：必须落在框内，且在第二视觉行以下。
    let caret = cx.debug_bounds("formula-editor-caret").expect("光标条该上屏");
    assert!(
        f32::from(caret.right()) <= f32::from(input.right()) + 1.0,
        "软换行后末尾光标不该跑到输入框右侧外面，实测 x={:?}",
        f32::from(caret.left())
    );
    assert!(
        f32::from(caret.bottom()) <= f32::from(input.bottom()) + 1.0,
        "软换行后末尾光标不该高出输入框下沿"
    );
    assert!(
        f32::from(caret.top()) > f32::from(input.top()) + INPUT_LINE_HEIGHT,
        "末尾在后面的视觉行上，光标 y 该低于第一行"
    );

    // 逐视觉行点左缘：偏移要随行号单调变大（同一硬行的软换行也能分开）。
    let mut offsets = Vec::new();
    for row in 0..3usize {
        let point = gpui::point(
            input.left() + gpui::px(INPUT_CONTENT_INSET_X) + gpui::px(1.0),
            input.top() + gpui::px(1.0 + INPUT_PADDING_Y + (row as f32) * INPUT_LINE_HEIGHT)
                + gpui::px(4.0),
        );
        cx.simulate_mouse_down(point, gpui::MouseButton::Left, gpui::Modifiers::none());
        cx.simulate_mouse_up(point, gpui::MouseButton::Left, gpui::Modifiers::none());
        redraw(cx);
        offsets.push(editor.read_with(cx, |editor, _cx| {
            editor
                .formula_editor
                .as_ref()
                .expect("弹窗开着")
                .selected_range
                .start
        }));
    }
    assert!(
        offsets[0] < 3,
        "点第一视觉行左缘该落在行首附近，实测 {offsets:?}"
    );
    assert!(
        offsets[1] > offsets[0] + 20,
        "点第二视觉行左缘该落到后半段，实测 {offsets:?}"
    );
    assert!(
        offsets[2] > offsets[1] + 20,
        "点第三视觉行左缘该再往后，实测 {offsets:?}"
    );
    assert!(offsets[2] < draft.len(), "第三行不该指到草稿末尾之后");
}

/// 窗口不是 key（macOS 红绿灯变灰）时光标该消失。gpui 在 resign key 时不清
/// `window.focus`，所以只看 `is_focused` 判不出失焦。
#[gpui::test]
async fn formula_editor_caret_hides_when_window_is_not_key(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "$$\nx^2\n$$\n".into(), None));
    redraw(cx);

    let math = editor
        .read_with(cx, |editor, cx| math_block_entity(editor, cx))
        .expect("夹具应有一个数学块");
    open_formula_editor(&editor, &math, cx);
    assert!(
        cx.debug_bounds("formula-editor-caret").is_some(),
        "窗口激活且草稿聚焦时光标该在"
    );

    // 另开一个窗口并激活它：原窗口不再是 key 窗口。
    let (_other, mut other_cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "# 另一个窗口\n".into(), None)
    });
    activate_visual_window(&mut other_cx);
    redraw(cx);

    assert!(
        editor.read_with(cx, |editor, _cx| editor.formula_editor.is_some()),
        "切窗口不该顺手关掉弹窗"
    );
    assert!(
        cx.debug_bounds("formula-editor-caret").is_none(),
        "窗口不是 key 窗口时光标条不该还画着"
    );
}
/// 按一个键并返回之后的选区两端。
fn press_draft_key(
    editor: &gpui::Entity<Editor>,
    cx: &mut gpui::VisualTestContext,
    keys: &str,
) -> (usize, usize) {
    editor.update_in(cx, |editor, window, cx| {
        let keystroke = gpui::Keystroke::parse(keys).expect(keys);
        let event = gpui::KeyDownEvent {
            keystroke,
            is_held: false,
        };
        editor.formula_editor_key_down(&event, window, cx);
    });
    editor.read_with(cx, |editor, _cx| {
        let state = editor.formula_editor.as_ref().expect("弹窗开着");
        (state.selected_range.start, state.selected_range.end)
    })
}

/// 上下 / Home / End / ⌘←→ 在软换行的长行上按视觉行走：以前上下键与
/// Home/End 都没接，长公式里光标只能在原地左右挪。
#[gpui::test]
async fn formula_editor_arrow_keys_navigate_visual_rows(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "$$\n\n$$\n".into(), None));
    redraw(cx);

    let math = editor
        .read_with(cx, |editor, cx| math_block_entity(editor, cx))
        .expect("夹具应有一个数学块");
    open_formula_editor(&editor, &math, cx);
    let draft = "F = ma \\frac{a}{b} \\int_{0}^{1} ma \\frac{a}{b} \\prod_{a}^{b} ".repeat(4);
    editor.update(cx, |editor, cx| {
        editor.replace_formula_draft(0..0, &draft, None, false, cx);
    });
    redraw(cx);
    redraw(cx);

    let (head, _) = press_draft_key(&editor, cx, "cmd-left");
    assert_eq!(head, 0, "⌘← 该回到草稿开头");

    let (end_row0, _) = press_draft_key(&editor, cx, "end");
    assert!(
        end_row0 > 10 && end_row0 < draft.len(),
        "End 该落在第一视觉行行尾，而不是整条硬行末尾，实测 {end_row0}/{}",
        draft.len()
    );
    let (home_row0, _) = press_draft_key(&editor, cx, "home");
    assert_eq!(home_row0, 0, "Home 在该行行首");

    let (down1, _) = press_draft_key(&editor, cx, "down");
    assert!(
        down1 > 10,
        "从行首按 Down 该落到第二视觉行，实测 {down1}"
    );
    let (down2, _) = press_draft_key(&editor, cx, "down");
    assert!(down2 > down1, "再按 Down 该继续往下，实测 {down1} → {down2}");
    let (up1, _) = press_draft_key(&editor, cx, "up");
    assert!(
        (up1 as isize - down1 as isize).abs() <= 1,
        "Up 该回到上一行的同一列，实测 {up1} vs {down1}"
    );

    let (sel_start, sel_end) = press_draft_key(&editor, cx, "shift-down");
    assert_eq!(sel_start, up1, "shift 扩展不该动锚点");
    assert!(sel_end > sel_start, "shift+Down 该把选区拉到下一行");

    let (_, tail) = press_draft_key(&editor, cx, "cmd-right");
    assert_eq!(tail, draft.len(), "⌘→ 该跳到草稿末尾");
}

/// 草稿长到超出输入框高度时，光标要自动滚进可见区（否则打字看不见落点）。
#[gpui::test]
async fn formula_editor_scrolls_caret_into_view(cx: &mut TestAppContext) {
    use crate::editor::formula_editor::INPUT_LINE_HEIGHT;

    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "$$\n\n$$\n".into(), None));
    redraw(cx);

    let math = editor
        .read_with(cx, |editor, cx| math_block_entity(editor, cx))
        .expect("夹具应有一个数学块");
    open_formula_editor(&editor, &math, cx);
    // 20 遍 ≈ 十来个视觉行，远超输入框的 132px。
    let draft = "F = ma \\frac{a}{b} \\int_{0}^{1} \\prod_{a}^{b} ".repeat(20);
    editor.update(cx, |editor, cx| {
        editor.replace_formula_draft(0..0, &draft, None, false, cx);
    });
    redraw(cx);
    redraw(cx);

    let input = cx
        .debug_bounds("formula-editor-input")
        .expect("草稿输入区该上屏");
    let caret = cx.debug_bounds("formula-editor-caret").expect("光标该上屏");
    assert!(
        f32::from(caret.bottom()) <= f32::from(input.bottom()) + 1.0,
        "末尾光标该被滚进可见区，实测 caret.bottom={:?} input.bottom={:?}",
        f32::from(caret.bottom()),
        f32::from(input.bottom())
    );
    assert!(
        f32::from(caret.top()) >= f32::from(input.top()) - 1.0,
        "光标不该滚到可见区上沿之外"
    );

    // 回到开头：可见区要跟着滚回顶部，光标重新出现在第一行。
    press_draft_key(&editor, cx, "cmd-left");
    redraw(cx);
    let caret = cx.debug_bounds("formula-editor-caret").expect("光标该上屏");
    assert!(
        f32::from(caret.top()) >= f32::from(input.top()) - 1.0
            && f32::from(caret.bottom())
                <= f32::from(input.top()) + INPUT_LINE_HEIGHT + 12.0,
        "回到开头时光标该重新出现在可见区顶部，实测 caret.top={:?} input.top={:?}",
        f32::from(caret.top()),
        f32::from(input.top())
    );
}

/// 草稿要能撤销 / 重做：连打一串字算一步，粘贴这类整段编辑各算一步。
#[gpui::test]
async fn formula_editor_draft_undo_redo(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "$$\n\n$$\n".into(), None));
    redraw(cx);

    let math = editor
        .read_with(cx, |editor, cx| math_block_entity(editor, cx))
        .expect("夹具应有一个数学块");
    open_formula_editor(&editor, &math, cx);

    // 真实敲字：一个字一个字进草稿。
    for character in ["a", "b", "c"] {
        editor.update_in(cx, |editor, window, cx| {
            editor.replace_text_in_range(None, character, window, cx);
        });
    }
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(editor.formula_editor.as_ref().expect("弹窗开着").draft, "abc");
    });

    let (_, _) = press_draft_key(&editor, cx, "cmd-z");
    editor.read_with(cx, |editor, _cx| {
        let state = editor.formula_editor.as_ref().expect("弹窗开着");
        assert_eq!(state.draft, "", "连打三个字该合成一步撤销，实测 {:?}", state.draft);
    });

    press_draft_key(&editor, cx, "cmd-shift-z");
    editor.read_with(cx, |editor, _cx| {
        let state = editor.formula_editor.as_ref().expect("弹窗开着");
        assert_eq!(state.draft, "abc", "⌘⇧Z 该把打字重做回来");
    });

    // 粘贴是独立一步：撤销只退掉粘贴，不连带把之前的打字也退掉。
    cx.update(|_window, cx| {
        cx.write_to_clipboard(gpui::ClipboardItem::new_string("\\alpha".into()))
    });
    press_draft_key(&editor, cx, "cmd-v");
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(editor.formula_editor.as_ref().expect("弹窗开着").draft, "abc\\alpha");
    });
    press_draft_key(&editor, cx, "cmd-z");
    editor.read_with(cx, |editor, _cx| {
        let state = editor.formula_editor.as_ref().expect("弹窗开着");
        assert_eq!(state.draft, "abc", "撤销该只退掉粘贴那一步，实测 {:?}", state.draft);
    });
}

/// 指针离开输入区之后，选区不该继续跟着鼠标走：抬手发生在框外时拖动状态没清，
/// 之后鼠标只是划过就会把选区改掉。
#[gpui::test]
async fn formula_editor_drag_stops_outside_the_field(cx: &mut TestAppContext) {
    use crate::editor::formula_editor::INPUT_LINE_HEIGHT;

    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "$$\n\n$$\n".into(), None));
    redraw(cx);

    let math = editor
        .read_with(cx, |editor, cx| math_block_entity(editor, cx))
        .expect("夹具应有一个数学块");
    open_formula_editor(&editor, &math, cx);
    editor.update(cx, |editor, cx| {
        editor.replace_formula_draft(0..0, "F = ma \\frac{a}{b}", None, false, cx);
    });
    redraw(cx);

    let input = cx
        .debug_bounds("formula-editor-input")
        .expect("草稿输入区该上屏");
    let start = gpui::point(input.left() + gpui::px(20.0), input.top() + gpui::px(12.0));
    let inside = gpui::point(
        input.left() + gpui::px(120.0),
        input.top() + gpui::px(1.0 + INPUT_LINE_HEIGHT),
    );
    // 抬手点在输入区外（下面的页签/符号区）。
    let outside = gpui::point(input.left() + gpui::px(120.0), input.bottom() + gpui::px(40.0));

    cx.simulate_mouse_down(start, gpui::MouseButton::Left, gpui::Modifiers::none());
    cx.simulate_mouse_move(inside, gpui::MouseButton::Left, gpui::Modifiers::none());
    cx.simulate_mouse_up(outside, gpui::MouseButton::Left, gpui::Modifiers::none());
    redraw(cx);
    editor.read_with(cx, |editor, _cx| {
        let state = editor.formula_editor.as_ref().expect("弹窗开着");
        assert!(
            !state.selecting_with_mouse,
            "在输入区外抬手就该结束拖动状态"
        );
    });
    let selection_after_up = editor.read_with(cx, |editor, _cx| {
        editor
            .formula_editor
            .as_ref()
            .expect("弹窗开着")
            .selected_range
            .clone()
    });

    // 没有按下任何键的划过：不该动选区。
    cx.simulate_mouse_move(start, None::<gpui::MouseButton>, gpui::Modifiers::none());
    cx.simulate_mouse_move(inside, None::<gpui::MouseButton>, gpui::Modifiers::none());
    redraw(cx);
    editor.read_with(cx, |editor, _cx| {
        let state = editor.formula_editor.as_ref().expect("弹窗开着");
        assert_eq!(
            state.selected_range, selection_after_up,
            "松手后鼠标划过不该改选区"
        );
    });
}

/// 符号网格不该靠滚轮才看得全：最大的一类（希腊字母 37 个）最后一格要完整
/// 落在网格里，而且整个面板（含「应用到公式」按钮）要在窗口内。
#[gpui::test]
async fn formula_editor_symbol_grid_shows_whole_category(cx: &mut TestAppContext) {
    use crate::components::latex::LatexCategory;

    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "$$\nx^2\n$$\n".into(), None));
    redraw(cx);

    let math = editor
        .read_with(cx, |editor, cx| math_block_entity(editor, cx))
        .expect("夹具应有一个数学块");
    open_formula_editor(&editor, &math, cx);
    editor.update(cx, |editor, cx| {
        if let Some(state) = editor.formula_editor.as_mut() {
            state.category = LatexCategory::Greek;
        }
        cx.notify();
    });
    redraw(cx);

    let grid_rows = LATEX_SYMBOLS
        .iter()
        .filter(|symbol| symbol.category == LatexCategory::Greek)
        .count();
    assert!(grid_rows > 24, "夹具前提：希腊字母是该面板最大的一类");

    let grid = cx
        .debug_bounds("formula-editor-grid")
        .expect("符号网格该上屏");
    let last = cx.debug_bounds("formula-cell-last").expect("最后一格该上屏");
    assert!(
        f32::from(last.bottom()) <= f32::from(grid.bottom()) + 1.0,
        "最后一格被网格裁掉了，说明还得滚轮才看得全：last.bottom={} grid.bottom={}",
        f32::from(last.bottom()),
        f32::from(grid.bottom())
    );
    assert!(
        f32::from(last.top()) >= f32::from(grid.top()) - 1.0,
        "最后一格不该在网格上沿之外"
    );

    let viewport_height = f32::from(cx.update(|window, _cx| window.viewport_size().height));
    let apply = cx
        .debug_bounds("formula-editor-apply")
        .expect("应用到公式按钮该上屏");
    assert!(
        f32::from(apply.bottom()) <= viewport_height + 1.0,
        "面板不该高过窗口，按钮该一眼可见：viewport={viewport_height} apply.bottom={}",
        f32::from(apply.bottom())
    );
}

/// 草稿要能全选 / 复制 / 剪切 / 粘贴（以前只有光标，剪贴板四个键全没接）。
#[gpui::test]
async fn formula_editor_draft_select_all_copy_cut_paste(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "$$\n\n$$\n".into(), None));
    redraw(cx);

    let math = editor
        .read_with(cx, |editor, cx| math_block_entity(editor, cx))
        .expect("夹具应有一个数学块");
    open_formula_editor(&editor, &math, cx);
    editor.update(cx, |editor, cx| {
        editor.replace_formula_draft(0..0, "E = mc^2", None, false, cx);
    });

    let press = |editor: &gpui::Entity<Editor>, cx: &mut gpui::VisualTestContext, keys: &str| {
        editor.update_in(cx, |editor, window, cx| {
            let keystroke = gpui::Keystroke::parse(keys).expect(keys);
            let event = gpui::KeyDownEvent {
                keystroke,
                is_held: false,
            };
            editor.formula_editor_key_down(&event, window, cx);
        });
    };

    press(&editor, cx, "cmd-a");
    editor.read_with(cx, |editor, _cx| {
        let state = editor.formula_editor.as_ref().expect("弹窗开着");
        assert_eq!(state.selected_range, 0.."E = mc^2".len(), "⌘A 该全选草稿");
    });

    press(&editor, cx, "cmd-c");
    let clipboard = cx.update(|_window, cx| cx.read_from_clipboard().and_then(|item| item.text()));
    assert_eq!(
        clipboard.as_deref(),
        Some("E = mc^2"),
        "⌘C 该把选区写进剪贴板"
    );
    editor.read_with(cx, |editor, _cx| {
        let state = editor.formula_editor.as_ref().expect("弹窗开着");
        assert_eq!(state.draft, "E = mc^2", "复制不该改动草稿");
    });

    press(&editor, cx, "cmd-x");
    editor.read_with(cx, |editor, _cx| {
        let state = editor.formula_editor.as_ref().expect("弹窗开着");
        assert_eq!(state.draft, "", "⌘X 该删掉选区");
    });
    let clipboard = cx.update(|_window, cx| cx.read_from_clipboard().and_then(|item| item.text()));
    assert_eq!(clipboard.as_deref(), Some("E = mc^2"), "剪切后剪贴板该留着内容");

    cx.update(|_window, cx| {
        cx.write_to_clipboard(gpui::ClipboardItem::new_string("\\frac{a}{b}".into()))
    });
    press(&editor, cx, "cmd-v");
    editor.read_with(cx, |editor, _cx| {
        let state = editor.formula_editor.as_ref().expect("弹窗开着");
        assert_eq!(state.draft, "\\frac{a}{b}", "⌘V 该把剪贴板插进光标处");
        let len = state.draft.len();
        assert_eq!(state.selected_range, len..len, "粘贴后光标该落在插入文本之后");
        assert!(state.completion.is_none(), "光标停在闭合括号之后不该弹补全");
    });

    // 粘贴以 `\命令` 前缀收尾的内容：补全会话该跟着起来（与敲字同口径）。
    cx.update(|_window, cx| {
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(" \\fr".into()))
    });
    press(&editor, cx, "cmd-v");
    editor.read_with(cx, |editor, _cx| {
        let state = editor.formula_editor.as_ref().expect("弹窗开着");
        assert_eq!(state.draft, "\\frac{a}{b} \\fr");
        let completion = state
            .completion
            .as_ref()
            .expect("粘贴进来的 \\fr 也该弹补全");
        assert!(completion.results.iter().any(|entry| entry.name == "frac"));
    });
}
/// 现象：聚焦数学块的 ƒx 入口浮在卡片右上角外面（贴着块外壳的边）。
/// 断言：入口落在公式卡片内、与首行 `$$` 同一水平带。
#[gpui::test]
async fn focused_math_block_fx_button_sits_inside_the_card(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "$$\nx^2\n$$\n".into(), None));
    redraw(cx);

    let math = editor
        .read_with(cx, |editor, cx| math_block_entity(editor, cx))
        .expect("夹具应有一个数学块");
    editor.update(cx, |editor, _cx| editor.focus_block(math.entity_id()));
    redraw(cx);
    redraw(cx);

    let fx = cx.debug_bounds("math-fx-button").expect("ƒx 入口该上屏");
    let card = cx.debug_bounds("math-block-card").expect("公式卡片该上屏");
    assert!(
        f32::from(fx.right()) <= f32::from(card.right()) + 1.0,
        "ƒx 不该探出卡片右缘，实测超出 {}px",
        f32::from(fx.right()) - f32::from(card.right())
    );
    assert!(
        f32::from(fx.top()) >= f32::from(card.top()) - 1.0,
        "ƒx 不该压在卡片上缘之外，实测超出 {}px",
        f32::from(card.top()) - f32::from(fx.top())
    );
    assert!(
        f32::from(fx.bottom()) - f32::from(card.top()) <= 30.0,
        "ƒx 该与首行 `$$` 同一水平带"
    );
}

fn draft_line<'a>(
    lines: &'a [crate::editor::formula_editor::FormulaDraftLine],
    index: usize,
) -> &'a crate::editor::formula_editor::FormulaDraftLine {
    lines.get(index).unwrap_or_else(|| panic!("草稿该有第 {index} 行"))
}

fn run_covering(
    line: &crate::editor::formula_editor::FormulaDraftLine,
    offset: usize,
) -> &gpui::TextRun {
    let mut start = 0usize;
    for run in &line.runs {
        if offset >= start && offset < start + run.len {
            return run;
        }
        start += run.len;
    }
    panic!("没有 run 覆盖偏移 {offset}（行 {:?}）", line.text);
}

fn run_color_for(line: &crate::editor::formula_editor::FormulaDraftLine, needle: &str) -> gpui::Hsla {
    let start = line
        .text
        .find(needle)
        .unwrap_or_else(|| panic!("{needle:?} 不在这一行 {:?}", line.text));
    run_covering(line, start).color
}

fn run_background_for(
    line: &crate::editor::formula_editor::FormulaDraftLine,
    offset: usize,
) -> Option<gpui::Hsla> {
    run_covering(line, offset).background_color
}

/// 现象：弹窗草稿整片一个颜色，公式块编辑态里 `\命令`、括号、数字都有语法色。
/// 断言：草稿按行切成铺满整行的 run，`\命令` 取关键字色、括号取标点色、
/// 数字取数字色、`%` 到行尾取注释色，正文仍是默认色；多字节安全。
#[test]
fn formula_draft_lines_carry_latex_syntax_colors() {
    use crate::components::markdown::code_highlight::{
        CodeHighlightClass, code_highlight_color,
    };
    use gpui::{Font, FontStyle, FontWeight, TextRun};

    let theme = Theme::default_theme();
    let colors = &theme.colors;
    let font = Font {
        family: "Monaco".into(),
        features: gpui::FontFeatures::default(),
        fallbacks: None,
        weight: FontWeight::NORMAL,
        style: FontStyle::Normal,
    };
    let draft = "F = ma \\frac{a}{b} \\sqrt[3]{x} % 注释 αβ\n\\alpha + \\beta_{1}";
    let lines =
        crate::editor::formula_editor::formula_draft_lines(draft, colors, font.clone(), 0..0);

    assert_eq!(lines.len(), 2, "换行该切成两行");
    for line in &lines {
        let total: usize = line.runs.iter().map(|run: &TextRun| run.len).sum();
        assert_eq!(total, line.text.len(), "run 必须铺满整行，实测 {:?}", line.text);
    }

    let keyword = code_highlight_color(colors, CodeHighlightClass::Keyword);
    let punctuation = code_highlight_color(colors, CodeHighlightClass::Punctuation);
    let number = code_highlight_color(colors, CodeHighlightClass::Number);
    let comment = code_highlight_color(colors, CodeHighlightClass::Comment);

    let first = draft_line(&lines, 0);
    assert_eq!(run_color_for(first, "F"), colors.text_default, "正文保持默认色");
    assert_eq!(run_color_for(first, "\\frac"), keyword, "\\命令 该是关键字色");
    assert_eq!(run_color_for(first, "{a}"), punctuation, "花括号该是标点色");
    assert_eq!(run_color_for(first, "3"), number, "数字该是数字色");
    assert_eq!(run_color_for(first, "%"), comment, "注释该是注释色");
    assert_eq!(run_color_for(first, "注释 αβ"), comment, "注释吃到行尾且多字节安全");

    let second = draft_line(&lines, 1);
    assert_eq!(run_color_for(second, "\\alpha"), keyword);
    assert_eq!(run_color_for(second, "_"), code_highlight_color(colors, CodeHighlightClass::Operator));
    assert_eq!(run_color_for(second, "1"), number);
    assert!(!first.text.contains('\n') && !second.text.contains('\n'), "行内不该有换行符");

    // 选区落在 run 的底色上（软换行时 GPUI 按视觉行断笔，手铺色块对不上）。
    let selected = crate::editor::formula_editor::formula_draft_lines(
        draft,
        colors,
        font.clone(),
        7..12, // "\frac" 那五个字节
    );
    let selected_first = draft_line(&selected, 0);
    assert_eq!(
        run_background_for(selected_first, 8),
        Some(colors.selection),
        "选区里的 \\命令 该带选中底色"
    );
    assert_eq!(
        run_background_for(selected_first, 2),
        None,
        "选区外的正文不该被染色"
    );
    let total: usize = selected_first.runs.iter().map(|run| run.len).sum();
    assert_eq!(total, selected_first.text.len(), "切开选区后 run 仍要铺满整行");
}
