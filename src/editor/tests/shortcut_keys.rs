use super::common::*;

/// 段落族快捷键对齐 Typora（issue #15）：标题 ⌘1-6、正文 ⌘0、引用 ⌘⌥Q、
/// 有序 ⌘⌥O、无序 ⌘⌥U、代码块 ⌘⌥C、表格 ⌘⌥T、数学块 ⌘⌥B。
/// Windows 列（ctrl-shift-* / ctrl-t）同表生效，逐键各测一条。
#[gpui::test]
async fn cmd_one_and_cmd_zero_turn_the_block_into_a_heading_and_back(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, "hello world".to_string(), None));
    redraw(cx);
    redraw(cx);
    cx.update(|window, _cx| window.activate_window());

    cx.simulate_keystrokes("cmd-1");
    redraw(cx);
    editor.read_with(cx, |editor, cx| {
        let visible = editor.document.visible_blocks();
        assert_eq!(
            visible[0].entity.read(cx).kind(),
            crate::components::BlockKind::Heading { level: 1 },
            "⌘1 应把段落换成一级标题"
        );
    });

    cx.simulate_keystrokes("cmd-0");
    redraw(cx);
    editor.read_with(cx, |editor, cx| {
        let visible = editor.document.visible_blocks();
        assert_eq!(
            visible[0].entity.read(cx).kind(),
            crate::components::BlockKind::Paragraph,
            "⌘0 应把标题换回正文段落"
        );
    });
}

#[gpui::test]
async fn cmd_three_sets_a_level_three_heading(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, "hello world".to_string(), None));
    redraw(cx);
    redraw(cx);
    cx.update(|window, _cx| window.activate_window());

    cx.simulate_keystrokes("cmd-3");
    redraw(cx);
    editor.read_with(cx, |editor, cx| {
        let visible = editor.document.visible_blocks();
        assert_eq!(
            visible[0].entity.read(cx).kind(),
            crate::components::BlockKind::Heading { level: 3 },
            "⌘3 应换成三级标题，块内文本：{}",
            visible[0].entity.read(cx).display_text()
        );
    });
}

#[gpui::test]
async fn quote_shortcuts_on_both_platforms_toggle_the_quote(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, "hello world".to_string(), None));
    redraw(cx);
    redraw(cx);
    cx.update(|window, _cx| window.activate_window());

    cx.simulate_keystrokes("cmd-alt-q");
    redraw(cx);
    editor.read_with(cx, |editor, cx| {
        assert!(
            matches!(editor.document.visible_blocks()[0].entity.read(cx).kind(), crate::components::BlockKind::Quote),
            "⌘⌥Q 应把段落换成引用"
        );
    });

    // Windows 形式 Ctrl+Shift+Q 在引用上再按一次 = 退回正文（换种类可逆）。
    cx.simulate_keystrokes("ctrl-shift-q");
    redraw(cx);
    editor.read_with(cx, |editor, cx| {
        assert!(
            matches!(editor.document.visible_blocks()[0].entity.read(cx).kind(), crate::components::BlockKind::Paragraph),
            "Ctrl+Shift+Q 应把引用退回正文"
        );
    });
}

#[gpui::test]
async fn list_shortcuts_on_both_platforms_set_the_list_kinds(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, "hello world".to_string(), None));
    redraw(cx);
    redraw(cx);
    cx.update(|window, _cx| window.activate_window());

    cx.simulate_keystrokes("cmd-alt-u");
    redraw(cx);
    editor.read_with(cx, |editor, cx| {
        assert!(
            matches!(
                editor.document.visible_blocks()[0].entity.read(cx).kind(),
                crate::components::BlockKind::BulletedListItem
            ),
            "⌘⌥U 应换成无序列表项"
        );
    });

    cx.simulate_keystrokes("cmd-alt-o");
    redraw(cx);
    editor.read_with(cx, |editor, cx| {
        assert!(
            matches!(
                editor.document.visible_blocks()[0].entity.read(cx).kind(),
                crate::components::BlockKind::NumberedListItem
            ),
            "⌘⌥O 应从无序项换成有序列表项"
        );
    });

    // Windows 形式 Ctrl+Shift+] 回到无序项。
    cx.simulate_keystrokes("ctrl-shift-]");
    redraw(cx);
    editor.read_with(cx, |editor, cx| {
        assert!(
            matches!(
                editor.document.visible_blocks()[0].entity.read(cx).kind(),
                crate::components::BlockKind::BulletedListItem
            ),
            "Ctrl+Shift+] 应换回无序列表项"
        );
    });
}

#[gpui::test]
async fn code_fence_shortcuts_on_both_platforms_convert_to_a_fence(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, "hello world".to_string(), None));
    redraw(cx);
    redraw(cx);
    cx.update(|window, _cx| window.activate_window());

    cx.simulate_keystrokes("cmd-alt-c");
    redraw(cx);
    editor.read_with(cx, |editor, cx| {
        assert!(
            editor.document.visible_blocks()[0]
                .entity
                .read(cx)
                .kind()
                .is_code_block(),
            "⌘⌥C 应把段落换成代码块"
        );
    });

    // Windows 形式 Ctrl+Shift+K 在代码块上再按一次 = 退回正文。
    cx.simulate_keystrokes("ctrl-shift-k");
    redraw(cx);
    editor.read_with(cx, |editor, cx| {
        assert!(
            matches!(editor.document.visible_blocks()[0].entity.read(cx).kind(), crate::components::BlockKind::Paragraph),
            "Ctrl+Shift+K 应把代码块退回正文"
        );
    });
}

#[gpui::test]
async fn cmd_alt_b_inserts_a_math_block_after_the_caret_block(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, "hello world".to_string(), None));
    redraw(cx);
    redraw(cx);
    cx.update(|window, _cx| window.activate_window());

    cx.simulate_keystrokes("cmd-alt-b");
    redraw(cx);
    redraw(cx);

    editor.read_with(cx, |editor, cx| {
        let visible = editor.document.visible_blocks();
        assert_eq!(visible.len(), 3, "⌘⌥B 应插入数学块，且数学块是结构块，尾部自动补一个段落");
        assert_eq!(
            visible[1].entity.read(cx).kind(),
            crate::components::BlockKind::MathBlock,
            "插入的应是数学块"
        );
    });
}

#[gpui::test]
async fn cmd_alt_t_opens_the_table_insert_dialog(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, "hello world".to_string(), None));
    redraw(cx);
    redraw(cx);
    cx.update(|window, _cx| window.activate_window());

    cx.simulate_keystrokes("cmd-alt-t");
    redraw(cx);
    redraw(cx);

    editor.read_with(cx, |editor, _cx| {
        assert!(
            editor.table_insert_dialog.is_some(),
            "⌘⌥T 应打开表格插入对话框"
        );
    });
}

/// 临时探针：键位表里有没有 ⌘3；直接派发 Heading3 动作能不能换种类。
#[gpui::test]
async fn probe_heading3_keymap_and_dispatch(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, "hello world".to_string(), None));
    redraw(cx);
    redraw(cx);
    cx.update(|window, _cx| window.activate_window());

    cx.dispatch_action(crate::components::Heading3);
    redraw(cx);
    editor.read_with(cx, |editor, cx| {
        println!(
            "probe: after direct dispatch kind = {:?}",
            editor.document.visible_blocks()[0].entity.read(cx).kind()
        );
    });
}


/// `==xx==` 应解析为标记文本并以荧光底色渲染（issue：用户报修没渲染）。
/// 变体覆盖中日韩相邻（flanking 规则最可能吞掉 == 的场景）。
#[gpui::test]
async fn double_equal_signs_produce_highlight_spans(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    for (label, source) in [
        ("拉丁夹注", "前缀 ==标记文本== 后缀"),
        ("行首独立", "==标记文本=="),
        ("中文相邻", "一些文字==标记文本==一些文字"),
        ("括号内", "（==标记文本==）"),
        ("列表项", "- 列表里 ==标记文本== 继续写"),
        ("纯ASCII", "some ==xx== text"),
    ] {
        let (editor, cx) =
            cx.add_window_view(|_, cx| Editor::from_markdown(cx, source.to_string(), None));
        redraw(cx);
        redraw(cx);
        editor.read_with(cx, |editor, cx| {
            let visible = editor.document.visible_blocks();
            let block = &visible[0].entity;
            let spans = block.read(cx).inline_spans();
            let highlighted: Vec<_> = spans
                .iter()
                .filter(|span| span.style.highlight)
                .collect();
            assert!(
                !highlighted.is_empty(),
                "{label}：==…== 应产生高亮 span，实际 spans：{spans:?}"
            );
        });
    }
}
