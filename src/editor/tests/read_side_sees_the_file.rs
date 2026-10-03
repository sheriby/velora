//! 读取侧（搜索、大纲、状态栏）说的是文件坐标。
//!
//! 这一组的对照物都是「块树重新序列化出来的那份文本」与「文件里的那份文本」
//! 不一致的写法：Setext 标题在文件里占两行、序列化后只剩一行；表格列宽在文件里
//! 填过空格、序列化后重新排。行号、字节区间、字数只要按重新序列化算，用户看到
//! 的就和磁盘上的文件差一行。

use super::common::*;
use crate::editor::status_bar::count_words;

/// Setext 标题 + 填充过的表格：重新序列化会少一行、改字节数。
const LOSSY_READ_DOC: &str = concat!(
    "标题\n",
    "=====\n",
    "\n",
    "| 名称 | 数量 |\n",
    "| ---- | ---- |\n",
    "| 甲   | 目标 |\n",
    "\n",
    "正文 one two\n",
);

#[gpui::test]
async fn outline_lines_are_the_lines_in_the_file(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, LOSSY_READ_DOC.to_string(), None)
    });
    redraw(cx);
    editor.update(cx, |editor, cx| editor.sync_workspace_outline(cx));

    let (entries, buffer_text) = editor.read_with(cx, |editor, _cx| {
        (
            editor
                .workspace
                .toc_entries
                .iter()
                .map(|entry| (entry.title.clone(), entry.line))
                .collect::<Vec<_>>(),
            editor.buffer.text(),
        )
    });
    let title = &entries[0].0;
    assert_eq!(title, "标题");
    let entry_line = entries[0].1;
    let file_line = buffer_text
        .split('\n')
        .position(|line| line == "标题")
        .expect("标题在文件里");
    assert_eq!(
        entry_line, file_line,
        "大纲记的行号应是文件里的第 {} 行，实际 {entry_line}",
        file_line
    );

    // 按大纲行号跳回去，必须落在这个标题块上。
    editor.update(cx, |editor, cx| {
        editor.jump_to_source_line(entry_line, cx);
    });
    redraw(cx);
    editor.read_with(cx, |editor, cx| {
        let active = editor
            .active_entity_id
            .and_then(|id| editor.document.block_entity_by_id(id))
            .expect("跳转后应有活动块");
        assert_eq!(
            active.read(cx).display_text(),
            "标题",
            "按文件行号跳回去落在了别的块上"
        );
    });
}

/// 状态栏读的就是缓冲区内容，而重新序列化的文本是另一份。
///
/// 字数对规范化不敏感（标记、空行、列宽都不是词），所以这条守的不是数字本身，
/// 而是「状态栏与文件同源」这个前提：读取侧一旦回到序列化文本，行列号、字数、
/// 搜索高亮会一起漂。
#[gpui::test]
async fn the_status_bar_counts_the_file_text(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, LOSSY_READ_DOC.to_string(), None)
    });
    redraw(cx);

    let (buffer_text, serialized, shown) = editor.update(cx, |editor, cx| {
        (
            editor.buffer.text(),
            editor.document.markdown_text(cx),
            editor.cached_total_word_count(cx),
        )
    });
    assert_eq!(shown, count_words(&buffer_text), "字数不是按文件内容算的");
    assert_ne!(
        buffer_text, serialized,
        "夹具得是「重新序列化会改写形状」的写法，否则这条断言测不出东西"
    );
}

/// 状态栏报的「行 : 列」是缓冲区里的位置：行按文件里的行，列按字素数。
///
/// 守的是「读取侧与文件同源」这个前提，顺带钉住中文的列不是按字节算的。
#[gpui::test]
async fn the_status_bar_reports_the_cursor_position_of_the_buffer(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "第一行\n\n第三行 with 中文\n".to_string(), None)
    });
    redraw(cx);
    editor.update(cx, |editor, cx| editor.toggle_view_mode(cx));
    redraw(cx);

    let position = editor.update(cx, |editor, cx| {
        let root = editor.document.root_blocks()[0].clone();
        let offset = editor
            .buffer
            .text()
            .find(" with")
            .expect("夹具里应有这一段");
        root.update(cx, |block, _cx| block.selected_range = offset..offset);
        editor.compute_source_cursor_position(cx)
    });
    assert_eq!(
        position,
        (3, 4),
        "行列号不是按缓冲区里的位置算的：{position:?}"
    );
}
