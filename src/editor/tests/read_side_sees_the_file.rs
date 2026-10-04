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

/// 记号与它后面的空格都属于记号，块内偏移说的要是文件里的那段字节。
///
/// 读侧为了知道「记号占几字节」按模型重新拼了一遍（标题拼 `# `、序号项拼 `1. `、任务框拼
/// `- [x] `）。文件里的写法与拼出来的不一样，块内每一个偏移就整体漂：Setext 标题在文件里
/// 根本没有 `# `，内容位置晚 2 字节（中文还会切进字符中间）。后果是搜索命中的选区与高亮、
/// 行列号、粘贴插入点说的都不是那几个字节。记号占几位是解析时就知道的事，只有解析时记得住
/// （`Setext 的记号是 0 位`），这里才不必猜。
#[gpui::test]
async fn block_offsets_land_on_the_bytes_the_file_actually_has(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    // 每行写法：内容在文件里从第几个字节开始，是拼出来的还是量出来的。
    const SHAPES: [(&str, &str); 7] = [
        ("setext 标题", "标题甲\n-----\n"),
        ("没有空格的引用记号", ">段庚\n"),
        ("两个空格的 ATX 标题", "#  标题乙\n"),
        ("三个空格的圆括号序号", "1)   项丙\n"),
        ("两个空格的子弹", "-  项丁\n"),
        ("引用块里的一行", "> 段戊\n"),
        ("大写叉的任务框", "- [X] 项己\n"),
    ];

    let mut failures = Vec::new();
    for (name, source_text) in SHAPES {
        let (editor, cx) =
            cx.add_window_view(|_window, cx| Editor::from_markdown(cx, source_text.to_string(), None));
        redraw(cx);

        let (content, offset_at_content_end, source) = editor.read_with(cx, |editor, cx| {
            let block = editor
                .document
                .visible_blocks()
                .first()
                .expect("夹具应有一个可见块")
                .entity
                .clone();
            let content = block.read_with(cx, |block, _cx| block.record.title.visible_text());
            let id = block.entity_id();
            let source = editor.buffer.text();
            let offset_at_content_end = editor.caret_source_offset(id, content.len(), cx);
            (content, offset_at_content_end, source)
        });

        let Some(at) = source.find(content.as_str()) else {
            failures.push(format!("  [{name}] {content:?} 不在文件里：夹具变了"));
            continue;
        };
        let expected = at + content.len();
        match offset_at_content_end {
            Some(offset) if offset == expected => {}
            other => failures.push(format!(
                "  [{name}] 内容 {content:?} 的末尾报 {other:?}，文件里在 {expected}"
            )),
        }
    }

    assert!(
        failures.is_empty(),
        "块内偏移按模型拼的记号算，落不到文件真实的字节上：\n{}",
        failures.join("\n")
    );
}

/// 敲下划线把段落提成标题之后，块内偏移也要落在文件真实的字节上。
///
/// 这一步写回会把 Setext 改写成 ATX（文件里就是 `# 标题甲`），所以读侧按 ATX 拼记号
/// 是对的；这里钉的是「块换了 kind 之后偏移没有跟着漂」——提成标题时没人给块记新
/// 的记号宽度，宽度记错就整体漂两字节（中文还会切进字符中间）。
#[gpui::test]
async fn a_typed_setext_heading_reports_the_bytes_of_its_text_line(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "标题甲\n\n=====".to_string(), None));
    redraw(cx);

    let underline = editor.read_with(cx, |editor, _cx| {
        editor.document.visible_blocks()[1].entity.clone()
    });
    cx.update(|window, cx| {
        underline.update(cx, |block, block_cx| {
            block.move_to(block.visible_len(), block_cx);
            block.on_newline(&Newline, window, block_cx);
        });
    });
    redraw(cx);

    let (content, offset_at_content_end, source, kind) = editor.read_with(cx, |editor, cx| {
        let visible = editor.document.visible_blocks();
        let heading = visible.first().expect("成标题后第一个块就是标题").entity.clone();
        let content = heading.read_with(cx, |block, _cx| block.record.title.visible_text());
        let kind = heading.read_with(cx, |block, _cx| block.kind());
        let id = heading.entity_id();
        let source = editor.buffer.text();
        let length = content.len();
        (content, editor.caret_source_offset(id, length, cx), source, kind)
    });
    assert_eq!(kind, BlockKind::Heading { level: 1 }, "这一步该把段落提成一级标题");
    let at = source.find(&content).expect("标题文字应在文件里");
    assert_eq!(
        offset_at_content_end,
        Some(at + content.len()),
        "手打 Setext 标题的内容末尾报 {offset_at_content_end:?}，文件里在 {}",
        at + content.len()
    );
}

/// 导入时量出来的记号宽度，不能在块被编辑之后变成假数据。
///
/// Setext 标题导入时记的是「内容从第 0 个字节开始」。用户在块里打一个字，写回若把这一块
/// 按 ATX 重新落进文件，文件里就有了 `# `，而块还记着 0——此后每个块内偏移整体漂两字节。
/// 这里钉两件事：下划线形状原样留在文件里（区间写回只动光标那几个字节），偏移也还落在
/// 那些真字节上。
#[gpui::test]
async fn typing_in_a_setext_heading_keeps_the_underline_and_the_offsets(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "标题甲\n-----\n".to_string(), None)
    });
    redraw(cx);

    let heading = editor.read_with(cx, |editor, _cx| {
        editor.document.visible_blocks()[0].entity.clone()
    });
    cx.update(|_window, cx| {
        heading.update(cx, |block, _cx| block.selected_range = 0..0);
    });
    cx.simulate_input("X");
    redraw(cx);

    let (content, offset_at_content_end, source) = editor.read_with(cx, |editor, cx| {
        let id = heading.entity_id();
        let content = heading.read_with(cx, |block, _cx| block.record.title.visible_text());
        let length = content.len();
        (content, editor.caret_source_offset(id, length, cx), editor.buffer.text())
    });
    assert_eq!(
        source, "X标题甲\n-----\n",
        "在 Setext 标题里打一个字不该把下划线洗成 ATX"
    );
    assert_eq!(
        offset_at_content_end,
        Some(source.find(&content).expect("标题文字应在文件里") + content.len()),
        "记号宽度在编辑后漂了：内容 {content:?} 的末尾报 {offset_at_content_end:?}"
    );
}

/// 挂在容器里的写法也要把块内偏移落在文件的真字节上。
///
/// 上一张表量的是根块那一行；这一张量的是**嵌套**：列表套列表、引用里的列表、标注里
/// 的正文、`*`/`+` 子弹、五位序号、制表符分隔、行尾还挂着闭合 `#` 的标题。读侧一旦
/// 按模型拼记号，越深的层级漂得越多。
///
/// 这张表按「整段可见文本应是文件里连续的一段」比对，所以放不下**多行块**（脚注定义
/// 的续行、引用容器的正文——它们的可见文本在文件里跨行，行与行之间还夹着各自的记号）。
/// 缩进过的代码围栏在这张表里量的是内容行末尾落得准不准（围栏行自己的缩进由
/// `typing_inside_an_indented_code_fence_lands_on_those_bytes` 钉）。
#[gpui::test]
async fn nested_shapes_put_block_offsets_on_the_real_bytes(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    // (名字, 文件内容, 目标块的可见文本含这段, 问的是块内第几个字节；`usize::MAX` = 末尾)
    const SHAPES: [(&str, &str, &str, usize); 11] = [
        ("列表套列表", "- 项甲\n  - 子乙\n", "子乙", usize::MAX),
        ("制表符分隔的子弹", "-\t项丙\n", "项丙", usize::MAX),
        ("星号子弹", "* 项丁\n", "项丁", usize::MAX),
        ("加号子弹", "+ 项戊\n", "项戊", usize::MAX),
        ("五位开头的序号", "5. 项己\n", "项己", usize::MAX),
        ("引用里的列表", "> - 项庚\n", "项庚", usize::MAX),
        ("引用里两个空格", ">  段辛\n", "段辛", usize::MAX),
        ("标注里的正文", "> [!NOTE] 提示壬\n> 正文癸\n", "正文癸", usize::MAX),
        // 代码块的可见文本含内容行的缩进，问块内第 4 个字节（`let` 的 `t`）；
        // 「块内最后一个位置」对围栏说的是整块（含闭合行）末尾，那是另一笔账。
        ("缩进两格的代码围栏", "  ```rust\n  let 丑 = 1;\n  ```\n", "let 丑 = 1;", 4),
        ("行尾闭合记号的标题", "# 标题寅 #\n", "标题寅", usize::MAX),
        ("标题前三个空格", "   # 标题卯\n", "标题卯", usize::MAX),
    ];

    let mut failures = Vec::new();
    for (name, source_text, want_content, want_offset) in SHAPES {
        let (editor, cx) = cx.add_window_view(|_window, cx| {
            Editor::from_markdown(cx, source_text.to_string(), None)
        });
        redraw(cx);

        let (content, offset_at_content_end, source, found) = editor.read_with(cx, |editor, cx| {
            let mut blocks = editor
                .document
                .visible_blocks()
                .iter()
                .map(|visible| visible.entity.clone())
                .collect::<Vec<_>>();
            for visible in editor.document.visible_blocks() {
                blocks.extend(visible.entity.read_with(cx, |block, _| block.children.clone()));
            }
            let Some(target) = blocks.iter().find(|block| {
                block
                    .read_with(cx, |block, _| block.record.title.visible_text())
                    .contains(want_content)
            }) else {
                return (String::new(), None, editor.buffer.text(), false);
            };
            let content = target.read_with(cx, |block, _| block.record.title.visible_text());
            let id = target.entity_id();
            let length = if want_offset == usize::MAX {
                content.len()
            } else {
                want_offset
            };
            (
                content,
                editor.caret_source_offset(id, length, cx),
                editor.buffer.text(),
                true,
            )
        });

        if !found {
            failures.push(format!("  [{name}] 块树里找不到含 {want_content:?} 的块"));
            continue;
        }
        let Some(at) = source.find(content.as_str()) else {
            failures.push(format!("  [{name}] {content:?} 不在文件里：夹具变了"));
            continue;
        };
        let expected = if want_offset == usize::MAX {
            at + content.len()
        } else {
            at + want_offset
        };
        if offset_at_content_end != Some(expected) {
            failures.push(format!(
                "  [{name}] 内容 {content:?} 的末尾报 {offset_at_content_end:?}，文件里在 {expected}"
            ));
        }
    }

    assert!(failures.is_empty(), "嵌套层里的块内偏移落不到文件真实的字节上：\n{}", failures.join("\n"));
}

/// 块自己的字节被结构写回重新写过之后，那份记号宽度账要么按刚落笔的形状重记，要么
/// 作废——就是不许留着旧的。
///
/// 写回会改文件里这一行的形状：Setext 提成 ATX 就在行首多了 `# `，而块里若还留着
/// 「内容从第 0 个字节开始」那份旧账，每个块内偏移都整体漂两位（实测报内容末尾在第 9 位，
/// 文件里在 11 位，中文还会切进字符中间）。落笔的那一行是序列化拼出来的，拼出几位就记
/// 几位（`written_line_ledger`），认不出形状的那一族才作废。
#[gpui::test]
async fn a_rewritten_block_refreshes_its_parse_time_prefix_record(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "标题甲\n\n=====".to_string(), None)
    });
    redraw(cx);

    // 段落是解析期记过账的：一行都没剥，宽度 0。
    editor.read_with(cx, |editor, cx| {
        assert_eq!(
            editor.document.root_blocks()[0].read(cx).record.source_line_prefixes,
            vec![0],
            "段落没记号，解析期该记下「内容从第 0 个字节开始」"
        );
    });

    let underline = editor.read_with(cx, |editor, _cx| {
        editor.document.visible_blocks()[1].entity.clone()
    });
    cx.update(|window, cx| {
        underline.update(cx, |block, block_cx| {
            block.move_to(block.visible_len(), block_cx);
            block.on_newline(&Newline, window, block_cx);
        });
    });
    redraw(cx);

    // 那一段字节被重新写过（`标题甲` + `=====` 变成 `# 标题甲`）：账要跟着新形状走。
    editor.read_with(cx, |editor, cx| {
        let heading = editor.document.root_blocks()[0].clone();
        assert_eq!(
            heading.read(cx).kind(),
            BlockKind::Heading { level: 1 },
            "这一步该把段落提成一级标题"
        );
        assert_eq!(
            heading.read(cx).record.source_line_prefixes,
            vec![2],
            "提成 ATX 之后账里还是旧宽度（或者留着旧的）：{:?}",
            heading.read(cx).record.source_line_prefixes
        );
        let content = heading.read(cx).display_text().to_string();
        let at_content_end =
            editor.caret_source_offset(heading.entity_id(), content.len(), cx);
        assert_eq!(
            at_content_end,
            Some(11),
            "块内偏移要落在文件真实的字节上：`# 标题甲` 的内容末尾在 11"
        );
    });
}
