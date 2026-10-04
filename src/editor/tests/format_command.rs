//! 「格式化文档」是这台编辑器里**唯一**允许改写法的地方。
//!
//! 打开、打字、保存、撤销都不许规范化（Setext→ATX、`__`→`**`、`1)`→`1.`、表格列宽
//! 重排）——那是 buffer 当事实源换来的性质：用户没碰过的字节必须还是磁盘上那样。
//! 所以这里断言两件相反的事：**不点这条命令时写法原样**，**点了之后写法按模型规范化
//! 且能一步撤销回原字节**。

use super::common::*;

/// 四种「模型写法与文件写法不同」的形状挤在一份文档里。
const LOSSY_WRITING_STYLE: &str = concat!(
    "标题甲\n",
    "===\n",
    "\n",
    "__强调__ 里的下划线写法\n",
    "\n",
    "1) 括号序号甲\n",
    "1) 括号序号乙\n",
    "\n",
    "| 名称 | 数量 |\n",
    "| ---- | ---- |\n",
    "| 甲   | 1    |\n",
);

#[gpui::test]
async fn formatting_the_document_writes_the_models_canonical_style(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, LOSSY_WRITING_STYLE.into(), None));
    redraw(cx);

    // 打开本身不改写：这条是下面一切的前提。
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(editor.buffer.text(), LOSSY_WRITING_STYLE, "打开就把写法改了");
    });

    let serializations_before =
        editor.read_with(cx, |editor, _| editor.source_serializations.get());
    editor.update(cx, |editor, cx| editor.format_document(cx));
    redraw(cx);

    let (file, canonical, passes) = editor.read_with(cx, |editor, cx| {
        (
            editor.buffer.text(),
            editor.document.markdown_text(cx),
            editor.source_serializations.get() - serializations_before,
        )
    });
    assert_eq!(passes, 1, "格式化自己付的那一次整篇序列化没进计数器");
    assert_eq!(
        file,
        format!("{canonical}\n"),
        "格式化之后，缓冲区应当就是模型那份规范化文本（补回文件原来的末行换行）"
    );
    assert!(file.contains("# 标题甲"), "Setext 没转成 ATX：{file:?}");
    assert!(file.contains("**强调**"), "下划线强调没统一：{file:?}");
    assert!(file.contains("1. 括号序号甲"), "括号序号没统一：{file:?}");
    assert!(!file.contains("===\n"), "Setext 下划线还留在文件里：{file:?}");
    assert!(!file.contains("__强调__"), "旧写法没被换掉：{file:?}");
    assert!(!file.contains("1) 括号序号"), "旧写法没被换掉：{file:?}");
}

#[gpui::test]
async fn undo_after_formatting_restores_the_original_bytes(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, LOSSY_WRITING_STYLE.into(), None));
    redraw(cx);

    editor.update(cx, |editor, cx| editor.format_document(cx));
    redraw(cx);
    let formatted = editor.read_with(cx, |editor, _cx| editor.buffer.text());
    assert_ne!(formatted, LOSSY_WRITING_STYLE, "格式化没落笔，撤销无从谈起");

    editor.update(cx, |editor, cx| editor.undo_document(cx));
    redraw(cx);
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(
            editor.buffer.text(),
            LOSSY_WRITING_STYLE,
            "撤销没把原字节逐段放回：格式化必须是一步可撤销的编辑"
        );
    });
}

#[gpui::test]
async fn formatting_an_already_canonical_document_writes_nothing(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, LOSSY_WRITING_STYLE.into(), None));
    redraw(cx);

    editor.update(cx, |editor, cx| editor.format_document(cx));
    redraw(cx);
    let once = editor.read_with(cx, |editor, _cx| editor.buffer.text());
    let dirty_before = editor.read_with(cx, |editor, _cx| editor.document_dirty);
    let edits_before = editor.read_with(cx, |editor, _| editor.undo_history.len());

    editor.update(cx, |editor, cx| editor.format_document(cx));
    redraw(cx);
    let (twice, dirty, edits) = editor.read_with(cx, |editor, _cx| {
        (
            editor.buffer.text(),
            editor.document_dirty,
            editor.undo_history.len(),
        )
    });
    assert_eq!(once, twice, "再格式化一次又动了字节：规范化不该是累加的");
    assert_eq!(dirty, dirty_before, "没有变化的那次格式化不该把文档标脏");
    assert_eq!(
        edits, edits_before,
        "没有变化的那次格式化不该留下一条空撤销组"
    );
}

#[gpui::test]
async fn typing_never_formats_the_document(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, LOSSY_WRITING_STYLE.into(), None));
    redraw(cx);

    let serializations_before =
        editor.read_with(cx, |editor, _| editor.source_serializations.get());
    cx.simulate_input("X");
    redraw(cx);
    editor.read_with(cx, |editor, _| {
        assert_eq!(
            editor.source_serializations.get() - serializations_before,
            0,
            "打字把整篇序列化又请回来了"
        );
    });

    // 打一个字只该多出那一个字：四种旧写法仍然原样在文件里。
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(
            editor.buffer.text(),
            LOSSY_WRITING_STYLE.replacen("标题甲", "X标题甲", 1),
            "打字顺带规范化了用户没碰过的写法"
        );
    });
}

#[gpui::test]
async fn formatting_a_source_document_does_nothing(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, LOSSY_WRITING_STYLE.into(), None));
    redraw(cx);

    editor.update(cx, |editor, cx| editor.toggle_view_mode(cx));
    redraw(cx);
    editor.update(cx, |editor, cx| editor.format_document(cx));
    redraw(cx);

    editor.read_with(cx, |editor, _cx| {
        assert_eq!(
            editor.buffer.text(),
            LOSSY_WRITING_STYLE,
            "源码视图里没有「模型的写法」可言，格式化不该动任何字节"
        );
    });
}

/// 撤销一次格式化之后，模型里的「写法数据」必须跟着字节一起退回去。
///
/// 盯的是这一步会悄悄坏事的版本：撤销只把缓冲区字节放回去，而 `__强调__` 这类写法是
/// 记在块树里的（保真那批提交特意存进去的）。要是树里还留着格式化之后的默认记号，
/// 下一次按区间写回就会拿 `**` 覆盖用户刚撤销回来的 `__`——用户撤销成功了，打个字
/// 又变回去。
#[gpui::test]
async fn formatting_again_after_an_undo_still_normalizes(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, LOSSY_WRITING_STYLE.to_string(), None)
    });
    redraw(cx);

    editor.update(cx, |editor, cx| editor.format_document(cx));
    redraw(cx);
    let formatted = editor.read_with(cx, |editor, _cx| editor.buffer.text());

    editor.update(cx, |editor, cx| editor.undo_document(cx));
    redraw(cx);
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(editor.buffer.text(), LOSSY_WRITING_STYLE, "撤销没退回原字节");
    });

    editor.update(cx, |editor, cx| editor.format_document(cx));
    redraw(cx);
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(
            editor.buffer.text(),
            formatted,
            "撤销过一次之后再格式化拿不到同一份规范写法：模型里的写法数据没跟着撤销走"
        );
    });
}
