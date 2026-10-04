//! 字节保真回环：打开一个文件、不做任何编辑、保存 —— 文件必须**一个字节都不变**。
//!
//! 这条性质属于架构而不是实现细节：只有「缓冲区即原文」才能免费做到；靠块树
//! 重新序列化则必然改写紧凑相邻块、表格列宽填充、Setext 标题、括号序号列表、
//! CRLF 与末行换行。所以本套件验的是公共行为（磁盘字节），不碰任何内部表示，
//! 重构完成后这些用例应一字不改地全部通过。
//!
//! 走的是真实打开漏斗：读字节 → `encoding::read_document_string` 解码 →
//! `Editor::from_file_source`，与 `app_menu`／工作区标签打开文件完全同一条路。

use super::common::*;
use crate::editor::encoding;

/// 一个保真用例：夹具名 + 磁盘上应有的精确内容（UTF-8 写出）。
type Case = (&'static str, &'static str);

/// 打开 → 不编辑 → 保存。返回（保存后的字节, 打开后是否被标成脏）。
///
/// 这里**故意不 panic**：本套件的阶段 0 用途是一次跑出整张失败清单，
/// 任何断言失败都会让后面的用例不再执行。
fn open_then_save_without_edit(
    cx: &mut TestAppContext,
    name: &str,
    source: &str,
) -> (Vec<u8>, bool) {
    let source = source.as_bytes();
    let path = temp_markdown_path(name);
    fs::write(&path, source).expect("write fixture");
    let cleanup = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup);
    });

    let document = encoding::load_document(&path).expect("read fixture");
    let (editor, cx) = cx.add_window_view({
        let path = path.clone();
        move |_window, cx| Editor::from_loaded_document(cx, document, Some(path))
    });

    let dirty_on_open = editor.read_with(cx, |editor, _cx| editor.document_dirty);

    cx.simulate_keystrokes("ctrl-s");
    redraw(cx);

    (fs::read(&path).expect("read saved file"), dirty_on_open)
}

/// 把一次失败摘要成可读的几行，便于一次跑出整张失败清单。
fn describe_case(name: &str, expected: &[u8], actual: &[u8], dirty_on_open: bool) -> String {
    let rendered = |bytes: &[u8]| -> String {
        String::from_utf8_lossy(bytes)
            .replace('\r', "\\r")
            .replace('\n', "\\n")
            .replace('\t', "\\t")
    };
    let mut out = String::new();
    if dirty_on_open {
        out.push_str(&format!("  [{name}] 仅仅打开就把文档标成脏了\n"));
    }
    if actual != expected {
        out.push_str(&format!(
            "  [{name}] 保存改写了文件\n    期望 {} 字节: {}\n    保存后 {} 字节: {}\n",
            expected.len(),
            rendered(expected),
            actual.len(),
            rendered(actual),
        ));
    }
    out
}

/// 全部保真用例的总表。逐条对应方案 §1.2 的有损点清单。
const FIDELITY_CASES: &[Case] = &[
    ("无末行换行", "# 标题\n\n正文没有末行换行"),
    ("有末行换行", "# 标题\n\n正文有末行换行\n"),
    ("多个末行换行", "正文\n\n\n"),
    ("CRLF 行尾", "# 标题\r\n\r\n正文\r\n第二行\r\n"),
    ("CRLF 且无末行换行", "# 标题\r\n\r\n正文"),
    (
        "紧凑相邻块：围栏后紧跟分隔线",
        "```rust\nfn main() {}\n```\n---\n\n结尾\n",
    ),
    (
        "紧凑相邻块：段落直接连标题",
        "段落一\n## 紧跟的标题\n段落二\n### 再一个\n",
    ),
    (
        "表格列宽对齐填充",
        "| 名称   | 数量 | 说明                 |\n\
         |:-------|-----:|----------------------|\n\
         | 苹果   |    3 | 红富士               |\n\
         | 香蕉   |   12 | 进口                 |\n",
    ),
    ("表格紧凑分隔行", "|a|b|\n|---|---|\n|1|2|\n"),
    ("Setext 标题", "一级标题\n========\n\n二级标题\n--------\n\n正文\n"),
    ("括号序号列表", "1) 第一项\n2) 第二项\n3) 第三项\n"),
    ("无序列表标记混用", "- 短横\n* 星号\n+ 加号\n"),
    (
        "强调定界符混用",
        "双下划线 __粗__ 和双星号 **粗** 和单星号 *斜* 和单下划线 _斜_\n",
    ),
    ("引用前缀风格", "> 引用一\n>引用二\n>   引用三\n"),
    ("代码围栏信息与长度", "~~~~rust title=\"示例\"\nlet x = 1;\n~~~~\n"),
    (
        "缩进两格的代码围栏",
        "  ```rust\n  let a = 1;\n  ```\n\n结尾\n",
    ),
    (
        "列表项里缩进四格的代码围栏",
        "- 步骤\n    ```rust\n    let b = 2;\n    ```\n\n结尾\n",
    ),
    (
        "缩进代码块",
        "段落\n\n    四空格缩进的代码\n    第二行\n\n结尾\n",
    ),
    (
        "front matter 原样",
        "---\ntitle: 我的笔记\n# 这不是标题\ntags: [a, b]\n---\n\n正文\n",
    ),
    (
        "中文与 emoji 混排",
        "标题：🎉 会议记录（2026-10-02）\n\n- 参会：张三、李四\n- 结论：把缓冲区做成唯一事实源 ✅\n",
    ),
    ("硬换行：行尾两空格", "第一行  \n第二行\n\n下一段\n"),
    ("硬换行：行尾反斜杠", "第一行\\\n第二行\n"),
    ("转义字符原样", "字面星号 \\*不强调\\* 和字面下划线 \\_x\\_\n"),
    (
        "链接引用式与自动链接",
        "看 [文档][ref] 或者 <https://example.com>。\n\n[ref]: https://example.org/a \"标题\"\n",
    ),
    ("脚注定义", "有脚注[^1]。\n\n[^1]: 脚注内容\n    续行\n"),
    (
        "fenced div 与 HTML 块",
        "::: note\n提示内容\n:::\n\n<div class=\"x\">\n  <p>html</p>\n</div>\n",
    ),
    ("展示数学", "$$\n\\sum_{i=1}^n x_i = 1\n$$\n"),
    ("空文件", ""),
    ("只有空白", "   \n\n\t\n"),
];

#[gpui::test]
async fn opening_then_saving_without_edit_preserves_every_byte(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let mut failures: Vec<String> = Vec::new();
    for (name, source) in FIDELITY_CASES {
        // 每个用例单独开一个窗口，用例之间不共享编辑器状态。
        let (saved, dirty_on_open) = open_then_save_without_edit(cx, name, source);
        let report = describe_case(name, source.as_bytes(), &saved, dirty_on_open);
        if !report.is_empty() {
            failures.push(report);
        }
    }

    assert!(
        failures.is_empty(),
        "打开→不编辑→保存 改写了用户文件，{} / {} 个用例失败：\n{}",
        failures.len(),
        FIDELITY_CASES.len(),
        failures.join("\n")
    );
}

/// 同一条表再走一遍「编辑后保存」：在第一块里插一个 `X`，保存出去的字节必须
/// 只是「原文 + 那一个字符」，别的一个字节都不许多、也不许少。
///
/// 这条管的是写回与保存那两步会不会顺手重排别处：区间级写回只碰插入点那一段，
/// 剩下的字节（表格列宽填充、Setext、括号序号、CRLF、末行换行）从磁盘原样带回。
/// 插入点不假设在文件开头——块前缀（`# `、`- `、`> `）不属于可见文本，光标
/// 落在前缀之后是正常行为，所以断言的是「差异恰好是一个插入的 `X`」。
#[gpui::test]
async fn saving_after_an_edit_at_the_start_preserves_every_other_byte(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let mut failures: Vec<String> = Vec::new();
    for (name, source) in FIDELITY_CASES {
        let path = temp_markdown_path(name);
        fs::write(&path, source).expect("write fixture");
        let cleanup = path.clone();
        cx.on_quit(move || {
            let _ = fs::remove_file(&cleanup);
        });
        let document = encoding::load_document(&path).expect("read fixture");
        let (editor, cx) = cx.add_window_view({
            let path = path.clone();
            move |_window, cx| Editor::from_loaded_document(cx, document, Some(path))
        });
        redraw(cx);

        let first = editor.read_with(cx, |editor, _cx| {
            editor.document.visible_blocks().first().map(|visible| visible.entity.clone())
        });
        let Some(first) = first else {
            failures.push(format!("  [{name}] 打开后一个可见块都没有"));
            continue;
        };
        cx.update(|_window, cx| {
            first.update(cx, |block, _cx| block.selected_range = 0..0);
        });
        cx.simulate_input("X");
        redraw(cx);
        cx.simulate_keystrokes("ctrl-s");
        redraw(cx);

        let saved = fs::read(&path).expect("read saved file");
        if let Some(report) = describe_insertion_case(name, source.as_bytes(), &saved) {
            failures.push(report);
        }
    }

    assert!(
        failures.is_empty(),
        "打开→插一个字符→保存 改写了不该动的字节，{} / {} 个用例失败：\n{}",
        failures.len(),
        FIDELITY_CASES.len(),
        failures.join("\n")
    );
}

/// 打开→在首块**第二行**行首插一个字符→保存：其他字节一个不动，字还得插在光标那一处。
///
/// 上面那张表量的是首行行首，这一张量的是续行。续行的记号宽度按文件逐行量
/// （`measured_block_line_prefixes`）；回到按模型拼之后，`>引用二`（记号后没空格）少一位、
/// `>   引用三` 多一位，四空格嵌套列表的续段还会走整块重贴、把用户那四格缩进洗成两格。
/// 「差异恰好是一个插入字符」不够——插错位置也是差一个字符，所以再对一次插入点。
/// 只有一个可见行的形状跳过，但可量的形状少到 6 个以下就是夹具变了。
#[gpui::test]
async fn saving_after_an_edit_on_the_second_line_preserves_every_other_byte(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let mut failures: Vec<String> = Vec::new();
    let mut measured = 0usize;
    for (name, source) in FIDELITY_CASES {
        let path = temp_markdown_path(name);
        fs::write(&path, source).expect("write fixture");
        let cleanup = path.clone();
        cx.on_quit(move || {
            let _ = fs::remove_file(&cleanup);
        });
        let document = encoding::load_document(&path).expect("read fixture");
        let (editor, cx) = cx.add_window_view({
            let path = path.clone();
            move |_window, cx| Editor::from_loaded_document(cx, document, Some(path))
        });
        redraw(cx);

        let Some(first) = editor.read_with(cx, |editor, _cx| {
            editor.document.visible_blocks().first().map(|visible| visible.entity.clone())
        }) else {
            failures.push(format!("  [{name}] 打开后一个可见块都没有"));
            continue;
        };
        let Some(caret) = first.read_with(cx, |block, _cx| {
            block
                .record
                .title
                .visible_text()
                .find('\n')
                .map(|newline| newline + 1)
        }) else {
            continue;
        };
        let expected_at =
            editor.read_with(cx, |editor, cx| editor.caret_source_offset(first.entity_id(), caret, cx));
        measured += 1;
        cx.update(|_window, cx| {
            first.update(cx, |block, _cx| block.selected_range = caret..caret);
        });
        cx.simulate_input("X");
        redraw(cx);
        cx.simulate_keystrokes("ctrl-s");
        redraw(cx);

        let saved = fs::read(&path).expect("read saved file");
        if let Some(report) = describe_insertion_case(name, source.as_bytes(), &saved) {
            failures.push(report);
            continue;
        }
        let inserted_at = source
            .as_bytes()
            .iter()
            .zip(saved.iter())
            .take_while(|(before, after)| before == after)
            .count();
        if Some(inserted_at) != expected_at {
            failures.push(format!(
                "  [{name}] 字插在第 {inserted_at} 字节，光标说的是 {expected_at:?}：那一行的记号宽度与文件不符"
            ));
        }
    }

    assert!(
        measured >= 6,
        "首块有第二行的形状只剩 {measured} 个可量：夹具变了"
    );
    assert!(
        failures.is_empty(),
        "打开→在首块第二行插一个字符→保存 改写了不该动的字节，{} / {} 个用例失败：\n{}",
        failures.len(),
        measured,
        failures.join("\n")
    );
}

/// 保存结果必须等于「原文在某一处插入了一个 `X`」：长度多一、插入点之后逐字节
/// 相同。除此之外什么都不能变。返回 `Some(报告)` 表示这个用例不合格。
fn describe_insertion_case(name: &str, original: &[u8], saved: &[u8]) -> Option<String> {
    let report = || {
        format!(
            "  [{name}] 保存改写了文件：原文 {} 字节，保存后 {} 字节\n    原文: {}\n    保存后: {}",
            original.len(),
            saved.len(),
            escape_bytes(original),
            escape_bytes(saved),
        )
    };
    if saved.len() != original.len() + 1 {
        return Some(report());
    }
    match original
        .iter()
        .zip(saved.iter())
        .position(|(before, after)| before != after)
    {
        Some(index) => {
            if saved[index] == b'X' && saved[index + 1..] == original[index..] {
                None
            } else {
                Some(report())
            }
        }
        // 原文整个是保存结果的前缀：`X` 追加在了末尾，也算只插了一个字符。
        None if saved[original.len()] == b'X' => None,
        None => Some(report()),
    }
}

fn escape_bytes(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .replace('\r', "\\r")
        .replace('\n', "\\n")
        .replace('\t', "\\t")
}

/// 第三维：打开 → 在第一块开头按回车拆块。这一步只该改写**这一块自己的**字节。
///
/// 回车拆块以前是这样走的：块先把光标之后的文字从自己身上切掉（Changed），编辑器
/// 按区间把这次切掉写成一次删除，于是这一块在缓冲区里塌成零宽；紧接着的
/// RequestNewline 再想按区间写回就没有区间可用，只能退回整篇重新序列化——
/// `__粗__` 变 `**粗**`、`>引用二` 变 `> 引用二`、表格列宽重填、末行换行被丢掉，
/// 一次回车把全文的写法洗了一遍。
///
/// 这里断言两件事：本块区间以外的字节原样，以及整篇序列化次数没有增加。
/// 还会退回整篇重投影的形状——白名单，只许减不许增。
///
/// 现在是空的：最后一条「引用前缀风格」（在引用容器里按回车）改走了区间落笔 +
/// 区域重解析（`Editor::reproject_root_region`）。以前那一趟先把整棵树序列化进
/// 缓冲区再整篇重解析，未编辑块的写法被一起洗掉；现在它只动本块那一段，也只换
/// 本段重解析出来的那几根块。
const WHOLE_DOCUMENT_RESYNC_STILL_ALLOWED: &[&str] = &[];

#[gpui::test]
async fn splitting_the_first_block_only_touches_that_blocks_bytes(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let mut failures: Vec<String> = Vec::new();
    for (name, source) in FIDELITY_CASES {
        let path = temp_markdown_path(name);
        fs::write(&path, source).expect("write fixture");
        let cleanup = path.clone();
        cx.on_quit(move || {
            let _ = fs::remove_file(&cleanup);
        });
        let document = encoding::load_document(&path).expect("read fixture");
        let (editor, cx) = cx.add_window_view({
            let path = path.clone();
            move |_window, cx| Editor::from_loaded_document(cx, document, Some(path))
        });
        redraw(cx);

        let first = editor.read_with(cx, |editor, _cx| {
            editor.document.visible_blocks().first().map(|visible| visible.entity.clone())
        });
        let Some(first) = first else {
            failures.push(format!("  [{name}] 打开后一个可见块都没有"));
            continue;
        };
        // 缓冲区是 LF 空间，块区间也记在这套坐标里，所以这条对照走缓冲区而不是磁盘。
        let (buffer_before, own_span, serializations_before) = editor.read_with(cx, |editor, _cx| {
            let span = editor.document.source_span_of(first.entity_id()).or_else(|| {
                editor
                    .document
                    .root_ancestor_of(first.entity_id())
                    .and_then(|root| editor.document.source_span_of(root.entity_id()))
            });
            (
                editor.buffer.text(),
                span,
                editor.source_serializations.get(),
            )
        });

        cx.update(|_window, cx| {
            first.update(cx, |block, _cx| block.selected_range = 0..0);
        });
        cx.dispatch_action(Newline);
        redraw(cx);

        let buffer_after = editor.read_with(cx, |editor, _cx| editor.buffer.text());
        let serializations = editor.read_with(cx, |editor, _| editor.source_serializations.get());
        let mut problems: Vec<String> = Vec::new();
        if serializations > serializations_before
            && !WHOLE_DOCUMENT_RESYNC_STILL_ALLOWED.contains(name)
        {
            problems.push("触发了整篇重新序列化".to_string());
        }
        if let Some(span) = &own_span {
            if buffer_before[..span.start] != buffer_after[..span.start] {
                problems.push("本块之前的字节被改写".to_string());
            }
            if !buffer_after.ends_with(&buffer_before[span.end..]) {
                problems.push("本块之后的字节被改写".to_string());
            }
        }
        if !problems.is_empty() {
            failures.push(format!(
                "  [{name}] 拆块洗掉了不相干的块（{}）\n    拆块前: {}\n    拆块后: {}",
                problems.join("，"),
                escape_bytes(buffer_before.as_bytes()),
                escape_bytes(buffer_after.as_bytes()),
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "拆块改写了本块以外的字节，{} / {} 个用例失败：\n{}",
        failures.len(),
        FIDELITY_CASES.len(),
        failures.join("\n")
    );
}



/// 工作区标签打开（`open_workspace_file`）与文件窗口打开走的是不同漏斗，
/// 它也必须带上原始字节。
#[gpui::test]
async fn opening_from_a_workspace_tab_preserves_every_byte(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let root = temp_markdown_path("workspace-round-trip");
    fs::create_dir_all(&root).expect("create workspace root");
    let path = root.join("笔记.md");
    let original = "| 名称   | 数量 |\n|:-------|-----:|\n| 苹果   |    3 |\r\n\r\n末尾没有换行"
        .as_bytes()
        .to_vec();
    fs::write(&path, &original).expect("write fixture");
    let cleanup = root.clone();
    cx.on_quit(move || {
        let _ = fs::remove_dir_all(cleanup);
    });

    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(path.clone(), window, cx);
        });
    });
    redraw(cx);

    editor.read_with(cx, |editor, _cx| {
        assert!(!editor.document_dirty, "标签打开后不应是脏的");
    });
    cx.simulate_keystrokes("ctrl-s");
    redraw(cx);

    assert_eq!(fs::read(&path).expect("read saved"), original);
}

/// 拖拽/按路径替换文档：缓冲区必须整个换掉，不能留着上一个文档的内容，
/// 否则保存会写出别的文件的字节。
#[gpui::test]
async fn replacing_the_document_by_path_swaps_the_buffer_and_keeps_its_bytes(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);

    let path = temp_markdown_path("replace-by-path");
    let original = "# 换进来的文档\n\n末行换行也保留\n".as_bytes().to_vec();
    fs::write(&path, &original).expect("write fixture");
    let cleanup = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup);
    });

    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "旧文档的内容，绝不能再出现在保存结果里".to_string(), None)
    });
    editor.update(cx, |editor, cx| {
        editor
            .replace_document_from_path(&path, cx)
            .expect("open by path");
    });
    redraw(cx);

    editor.read_with(cx, |editor, _cx| {
        assert!(
            !editor.buffer.text().contains("旧文档"),
            "缓冲区还留着上一个文档的内容"
        );
        assert!(!editor.document_dirty, "按路径替换后不应是脏的");
    });
    cx.simulate_keystrokes("ctrl-s");
    redraw(cx);

    assert_eq!(fs::read(&path).expect("read saved"), original);
}

/// 段首打字不许重新解释这一段里已有的字面转义。
///
/// `\*不强调\*` 显示成字面星号，靠的是源码里那两个反斜杠；树里只剩一个 `*`，和没配对
/// 的定界符长得一模一样。转义位置因此是必须存下来的数据（`InlineTextTree::escaped_offsets`）：
/// 少了它，一次打字就把写法读成语法，可见文本变短、渲染变粗、字节也被顺手改写。
#[gpui::test]
async fn typing_next_to_literal_escapes_keeps_them_literal(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    const SOURCE: &str = "字面星号 \\*不强调\\* 和字面下划线 \\_x\\_\n";
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, SOURCE.to_string(), None));
    redraw(cx);
    let first = editor.read_with(cx, |editor, _cx| {
        editor.document.visible_blocks().first().map(|visible| visible.entity.clone())
    }).expect("应有第一个块");
    cx.update(|_window, cx| {
        first.update(cx, |block, _cx| block.selected_range = 0..0);
    });
    cx.simulate_input("X");
    redraw(cx);

    let (buffer_text, visible_text) = editor.read_with(cx, |editor, cx| {
        let block = editor
            .document
            .visible_blocks()
            .first()
            .map(|visible| visible.entity.clone())
            .expect("应有第一个块");
        (
            editor.buffer.text(),
            block.read(cx).record.title.visible_text(),
        )
    });
    assert_eq!(
        buffer_text, "X字面星号 \\*不强调\\* 和字面下划线 \\_x\\_\n",
        "插入处以外的字面转义被重新解释了"
    );
    assert_eq!(
        visible_text, "X字面星号 *不强调* 和字面下划线 _x_",
        "字面星号被读成了强调定界符，可见文本短了一截"
    );
}

/// 转义写法存下来之后，同一段里现敲的 markdown 语法照常生效：两者不打架。
#[gpui::test]
async fn typing_markdown_in_an_escaped_paragraph_makes_bold_and_keeps_the_escapes(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);

    const SOURCE: &str = "字面星号 \\*不强调\\*\n";
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, SOURCE.to_string(), None));
    redraw(cx);
    let first = editor.read_with(cx, |editor, _cx| {
        editor.document.visible_blocks().first().map(|visible| visible.entity.clone())
    }).expect("应有第一个块");
    cx.update(|_window, cx| {
        first.update(cx, |block, _cx| block.selected_range = 0..0);
    });
    for ch in "**粗**".chars() {
        cx.simulate_input(&ch.to_string());
    }
    redraw(cx);

    let (buffer_text, visible_text) = editor.read_with(cx, |editor, cx| {
        let block = editor
            .document
            .visible_blocks()
            .first()
            .map(|visible| visible.entity.clone())
            .expect("应有第一个块");
        (
            editor.buffer.text(),
            block.read(cx).record.title.visible_text(),
        )
    });
    assert_eq!(
        buffer_text, "**粗**字面星号 \\*不强调\\*\n",
        "现写的定界符没落到源码里，或者旧的转义被改写了"
    );
    assert_eq!(
        visible_text, "粗字面星号 *不强调*",
        "逐字符敲的 ** 没成粗体，或字面星号被读成了定界符"
    );
}

/// 编码维度的回环用例：磁盘字节不是 UTF-8 时也必须原样带回去。
///
/// `FileShape` 记的是「编码 + 行尾」，缓冲区里始终是解码后的规范文本，保存时
/// 按同一个形状重编码——所以 GB18030 的字节、UTF-8 的 BOM 都不该在打开保存这一
/// 趟里变成 UTF-8 正文。
fn raw_fidelity_cases() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        // 「中文笔记」的 GB18030 字节（Windows 中文环境常见）。
        (
            "GB18030 正文",
            encoding_rs::GB18030
                .encode("中文笔记\r\n第二行\r\n")
                .0
                .to_vec(),
        ),
        // UTF-8 BOM：BOM 本身是合法 UTF-8 字符，跟着文本一起回来。
        ("UTF-8 BOM", "\u{feff}# 标题\n\n正文\n".as_bytes().to_vec()),
        (
            "UTF-8 BOM 且 CRLF",
            "\u{feff}# 标题\r\n\r\n正文\r\n".as_bytes().to_vec(),
        ),
        // 混合行尾（有 lone CR）：按 LF 形状处理，不许把 LF 升格成 CRLF。
        (
            "混合行尾",
            "第一行\r\n第二行\n第三行\r\n".as_bytes().to_vec(),
        ),
    ]
}

#[gpui::test]
async fn opening_then_saving_a_non_utf8_file_preserves_every_byte(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let cases = raw_fidelity_cases();
    let mut failures: Vec<String> = Vec::new();
    for (name, bytes) in &cases {
        let path = temp_markdown_path(name);
        fs::write(&path, bytes).expect("write fixture");
        let cleanup = path.clone();
        cx.on_quit(move || {
            let _ = fs::remove_file(&cleanup);
        });
        let document = encoding::load_document(&path).expect("read fixture");
        let (editor, cx) = cx.add_window_view({
            let path = path.clone();
            move |_window, cx| Editor::from_loaded_document(cx, document, Some(path))
        });
        redraw(cx);
        let dirty_on_open = editor.read_with(cx, |editor, _cx| editor.document_dirty);

        cx.simulate_keystrokes("ctrl-s");
        redraw(cx);

        let saved = fs::read(&path).expect("read saved file");
        let report = describe_case(name, bytes, &saved, dirty_on_open);
        if !report.is_empty() {
            failures.push(report);
        }
    }

    assert!(
        failures.is_empty(),
        "非 UTF-8 文件打开→保存改写了字节，{} / {} 个用例失败：\n{}",
        failures.len(),
        cases.len(),
        failures.join("\n")
    );
}

/// 编辑之后也要按原编码写回去：只改一个字符不许顺手把整篇转成 UTF-8。
#[gpui::test]
async fn saving_an_edited_non_utf8_file_keeps_its_encoding(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let text = "中文笔记\n\n第二行\n";
    let original = encoding_rs::GB18030.encode(text).0.into_owned();
    let path = temp_markdown_path("gb18030-edit");
    fs::write(&path, &original).expect("write fixture");
    let cleanup = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup);
    });

    let document = encoding::load_document(&path).expect("read fixture");
    let (editor, cx) = cx.add_window_view({
        let path = path.clone();
        move |_window, cx| Editor::from_loaded_document(cx, document, Some(path))
    });
    redraw(cx);
    let first = editor
        .read_with(cx, |editor, _cx| {
            editor
                .document
                .visible_blocks()
                .first()
                .map(|visible| visible.entity.clone())
        })
        .expect("应有第一个块");
    cx.update(|_window, cx| {
        first.update(cx, |block, _cx| block.selected_range = 0..0);
    });
    cx.simulate_input("X");
    redraw(cx);
    cx.simulate_keystrokes("ctrl-s");
    redraw(cx);

    let saved = fs::read(&path).expect("read saved file");
    let edited = format!("X{text}");
    let expected = encoding_rs::GB18030.encode(&edited).0.to_vec();
    assert_eq!(saved, expected, "编辑之后保存出去的不是 GB18030 的字节");
}



/// 编辑引用块不吃掉末行换行，也不许把 CRLF 降成 LF。
///
/// 序列化把每根块当「一行」，行尾那个换行不在它的产物里：缓冲区原本以换行结尾却
/// 不补回来，重投影一次就把文件的末行换行删掉，CRLF 文件连带少一个 `\r`。这条用
/// 一个 CRLF 引用夹具盯着落笔与保存这两步——以前走的是整篇重投影，现在走的是按
/// 区间落笔，两个形状都不该漂。
#[gpui::test]
async fn editing_a_quote_keeps_the_final_newline_and_line_endings(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    const FIXTURE: &str = "> 引用一\n>引用二\n>   引用三\n";

    let path = temp_markdown_path("resync-final-newline");
    fs::write(&path, FIXTURE.replace('\n', "\r\n")).expect("write fixture");
    let cleanup = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup);
    });

    let document = encoding::load_document(&path).expect("read fixture");
    let open_path = path.clone();
    let (editor, cx) = cx.add_window_view(move |_window, cx| {
        Editor::from_loaded_document(cx, document, Some(open_path))
    });
    redraw(cx);

    let first = editor.read_with(cx, |editor, _cx| {
        editor
            .document
            .visible_blocks()
            .first()
            .map(|visible| visible.entity.clone())
            .expect("夹具应有第一个可见块")
    });
    cx.update(|_window, cx| {
        first.update(cx, |block, _cx| block.selected_range = 0..0);
    });
    cx.dispatch_action(Newline);
    redraw(cx);
    cx.simulate_keystrokes("ctrl-s");
    redraw(cx);

    let saved = fs::read(&path).expect("read saved file");
    let text = String::from_utf8_lossy(&saved).to_string();
    // 块首回车在引用里插出一个空引用行。这一步现在按区间落笔，不再被整篇重投影
    // 连同用户的编辑一起洗掉（以前回车等于没按）。
    assert!(
        saved.starts_with(b"> \r\n"),
        "块首回车没在引用里插出空行：{text:?}"
    );
    assert!(
        saved.ends_with(b"\r\n"),
        "落笔吃掉了末行换行（或把 CRLF 降成了 LF）：{text:?}"
    );
    assert!(
        text.contains(">   引用三\r\n"),
        "没改过的那一行被改写了：{text:?}"
    );
    // 形状本身也不能变：每个 LF 都得有 CR 在前面，不许混进裸 LF。
    let total_lf = saved.iter().filter(|byte| **byte == b'\n').count();
    let crlf_pairs = saved.windows(2).filter(|pair| *pair == b"\r\n").count();
    assert_eq!(
        crlf_pairs, total_lf,
        "行尾形状被混用了：CRLF {crlf_pairs} 处，LF 共 {total_lf} 处"
    );
}


/// 代码文档改一行，保存出去只能多这几个字节。
///
/// 源码/代码文档的保存目前还取块树序列化（`serialized_document_text`），而那份投影
/// 是「整篇源码按行块重新拼一遍」——末行换行的有无、行尾形状都靠拼接规则碰对。这条
/// 守的是结果而不是实现：以后哪一步把保存换成缓冲区或改坏拼接，只要多写或少写一个
/// 用户没打过的字节，这里就红。
#[gpui::test]
async fn editing_a_code_document_without_a_final_newline_keeps_it_absent(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    let path = std::env::temp_dir().join(format!("velora-code-no-eol-{}.py", std::process::id()));
    fs::write(&path, "print(1)\nprint(2)").expect("write fixture");
    let cleanup = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup);
    });
    let document = encoding::load_document(&path).expect("read fixture");
    let open_path = path.clone();
    let (editor, cx) = cx.add_window_view(move |_window, cx| {
        Editor::from_loaded_document(cx, document, Some(open_path))
    });
    redraw(cx);

    editor.update(cx, |editor, cx| {
        let line = editor
            .document
            .visible_blocks()
            .first()
            .expect("代码文档应有可见块")
            .entity
            .clone();
        line.update(cx, |line, cx| {
            line.prepare_undo_capture(crate::components::UndoCaptureKind::CoalescibleText, cx);
            line.replace_text_in_visible_range(0..0, "# ", None, false, cx);
        });
    });
    redraw(cx);
    cx.simulate_keystrokes("ctrl-s");
    redraw(cx);

    let saved = fs::read(&path).expect("read saved");
    assert_eq!(
        String::from_utf8_lossy(&saved).as_ref(),
        "# print(1)\nprint(2)",
        "代码文档保存动到了用户没改的字节：{:?}",
        String::from_utf8_lossy(&saved)
    );
}

/// CRLF 的代码文档：改一行、保存，字节按文件原来的形状回来，且不留下「未保存」。
///
/// 保存取文本时要同时盯两件事：落盘字节（行尾形状由 `FileShape` 重新编码）与
/// 保存后写在版本号里的那份文本——两者对不上，下一次校验磁盘就会把自己刚写的
/// 文件当成外部改动。
#[gpui::test]
async fn editing_a_crlf_code_document_saves_crlf_bytes_and_clears_dirty(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    let path = std::env::temp_dir().join(format!("velora-code-crlf-{}.py", std::process::id()));
    fs::write(&path, b"print(1)\r\nprint(2)\r\n").expect("write fixture");
    let cleanup = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup);
    });
    let document = encoding::load_document(&path).expect("read fixture");
    let open_path = path.clone();
    let (editor, cx) = cx.add_window_view(move |_window, cx| {
        Editor::from_loaded_document(cx, document, Some(open_path))
    });
    redraw(cx);

    editor.update(cx, |editor, cx| {
        let line = editor
            .document
            .visible_blocks()
            .first()
            .expect("代码文档应有可见块")
            .entity
            .clone();
        line.update(cx, |line, cx| {
            line.prepare_undo_capture(crate::components::UndoCaptureKind::CoalescibleText, cx);
            line.replace_text_in_visible_range(0..0, "# ", None, false, cx);
        });
    });
    redraw(cx);
    cx.simulate_keystrokes("ctrl-s");
    redraw(cx);

    let saved = fs::read(&path).expect("read saved");
    assert_eq!(
        saved.as_slice(),
        b"# print(1)\r\nprint(2)\r\n",
        "CRLF 代码文档保存的行尾形状或字节不对：{:?}",
        String::from_utf8_lossy(&saved)
    );
    editor.read_with(cx, |editor, _cx| {
        assert!(
            !editor.document_dirty,
            "保存之后还标着未保存：版本号与落盘文本对不上"
        );
    });
}

/// 在多行引用里打字，同块没改过的那几行的前缀写法必须原样留着。
///
/// 引用容器是一个多行根块：`> 引用一`、`>引用二`、`>   引用三` 三种写法都合法，
/// 前缀的空格数是用户写的字节。以前「结构一变就把整块按模型重拼」会把没改过的那几行
/// 一并规范成 `> `，这里钉住按区间落笔的结果。
#[gpui::test]
async fn typing_inside_a_quote_line_keeps_the_sibling_line_prefixes(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    const FIXTURE: &str = "> 引用一\n>引用二\n>   引用三\n";
    let path = temp_markdown_path("quote-sibling-prefixes");
    fs::write(&path, FIXTURE.replace('\n', "\r\n")).expect("write fixture");
    let cleanup = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup);
    });

    let document = encoding::load_document(&path).expect("read fixture");
    let open_path = path.clone();
    let (editor, cx) = cx.add_window_view(move |_window, cx| {
        Editor::from_loaded_document(cx, document, Some(open_path))
    });
    redraw(cx);

    let first = editor.read_with(cx, |editor, _cx| {
        editor
            .document
            .visible_blocks()
            .first()
            .map(|visible| visible.entity.clone())
            .expect("夹具应有第一个可见块")
    });
    cx.update(|_window, cx| {
        first.update(cx, |block, _cx| block.selected_range = 9..9);
    });
    editor.update(cx, |editor, _cx| editor.focus_block(first.entity_id()));
    redraw(cx);
    cx.simulate_input("甲");
    redraw(cx);
    cx.simulate_keystrokes("ctrl-s");
    redraw(cx);

    let saved = fs::read_to_string(&path).expect("read saved file");
    assert_eq!(
        saved,
        "> 引用一甲\r\n>引用二\r\n>   引用三\r\n",
        "打字把同块没改过的那几行改写了：{saved:?}"
    );
}

/// 选区跨过引用里的换行再打字，也一样只能改落点那一段。
#[gpui::test]
async fn replacing_a_selection_across_a_quote_line_break_keeps_the_sibling_prefixes(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);

    const FIXTURE: &str = "> 引用一\n>引用二\n>   引用三\n";
    let path = temp_markdown_path("quote-cross-line-replace");
    fs::write(&path, FIXTURE).expect("write fixture");
    let cleanup = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup);
    });

    let document = encoding::load_document(&path).expect("read fixture");
    let open_path = path.clone();
    let (editor, cx) = cx.add_window_view(move |_window, cx| {
        Editor::from_loaded_document(cx, document, Some(open_path))
    });
    redraw(cx);

    let first = editor.read_with(cx, |editor, _cx| {
        editor
            .document
            .visible_blocks()
            .first()
            .map(|visible| visible.entity.clone())
            .expect("夹具应有第一个可见块")
    });
    cx.update(|_window, cx| {
        // 「引用一」之后到「引用」之后：跨过那条换行。
        first.update(cx, |block, _cx| block.selected_range = 9..16);
    });
    editor.update(cx, |editor, _cx| editor.focus_block(first.entity_id()));
    redraw(cx);
    cx.simulate_input("甲");
    redraw(cx);
    cx.simulate_keystrokes("ctrl-s");
    redraw(cx);

    let saved = fs::read_to_string(&path).expect("read saved file");
    assert_eq!(
        saved,
        "> 引用一甲二\n>   引用三\n",
        "跨引用行的选区替换把没改过的那行改写了：{saved:?}"
    );
}

/// 挂在引用块里的表格没有自己的源码区间，它的格子也要映射到缓冲区里的真实字节。
///
/// 以前按「列宽 = 内容长 + 3」推算，用户填过宽度的列上会漂（实测命中选中的是
/// `" | 苹"` 而不是 `苹果`）。现在改成在**所在根块**的原文行里逐格量。
#[gpui::test]
async fn a_table_inside_a_quote_maps_its_cells_to_the_real_bytes(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    const FIXTURE: &str = "> 前话\n>\n> | 名称   | 数量 |\n> |:-------|-----:|\n> | 苹果   |    3 |\n";
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, FIXTURE.to_string(), None));
    redraw(cx);

    let mapped = editor.read_with(cx, |editor, cx| {
        let source = editor.buffer.text();
        ["名称", "数量", "苹果", "3"]
            .into_iter()
            .map(|text| {
                let binding = editor
                    .table_cells
                    .values()
                    .find(|binding| binding.cell.read(cx).display_text() == text)
                    .cloned();
                let Some(binding) = binding else {
                    return format!("{text}: 没有这一格的绑定");
                };
                match editor.source_mapping_for_entity(binding.cell.entity_id(), cx) {
                    Some(mapping) => {
                        let range = mapping.full_source_range;
                        format!("{text}: {:?}", &source[..range.end][range.start..])
                    }
                    None => format!("{text}: 没有映射"),
                }
            })
            .collect::<Vec<_>>()
    });
    assert_eq!(
        mapped,
        vec!["名称: \"名称\"", "数量: \"数量\"", "苹果: \"苹果\"", "3: \"3\""],
        "容器里的表格的格子映射没有落在它自己的原文字节上：{mapped:?}"
    );
}

/// 空格子也要说得出自己在文件里的哪一段字节。
///
/// 读侧量格子位置的办法是「在原文那一行里搜这一格序列化出来的文字」：空格子序列化
/// 出空串，于是这一格**没有映射**——光标停在空格里时算不出源码偏移，粘贴、跳转、
/// 状态栏的行列号都只能退回默认位置。格子位置该按结构量（第几行第几列，从原文的
/// 管道符之间夹出来），与写回用的是同一把尺。
#[gpui::test]
async fn an_empty_table_cell_still_knows_which_bytes_it_is(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    const FIXTURE: &str = "| 名称   | 数量 |\n|:-------|-----:|\n| 苹果   |      |\n";
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, FIXTURE.to_string(), None));
    redraw(cx);

    let (ranges, caret, column_bounds) = editor.read_with(cx, |editor, cx| {
        let source = editor.buffer.text();
        // 数据行 `| 苹果   |      |` 里第二格那一列的范围：前一个管道符之后到行尾。
        let row_start = source.find("| 苹果").expect("夹具里应有这一行");
        let row_line = source[row_start..].split('\n').next().expect("行应有内容");
        let second_pipe = row_start
            + row_line
                .match_indices('|')
                .nth(1)
                .expect("这一行该有两个管道符")
                .0;
        let column_bounds = (second_pipe, row_start + row_line.len());

        let ranges = editor
            .table_cells
            .values()
            .map(|binding| {
                let position = binding
                    .cell
                    .read_with(cx, |block, _cx| block.table_cell_position())
                    .expect("格子绑定应有位置");
                (
                    (position.row, position.column),
                    editor
                        .source_mapping_for_entity(binding.cell.entity_id(), cx)
                        .map(|mapping| mapping.full_source_range),
                )
            })
            .collect::<Vec<_>>();
        let empty_cell = editor
            .table_cells
            .values()
            .find(|binding| {
                binding
                    .cell
                    .read_with(cx, |block, _cx| block.table_cell_position())
                    == Some(crate::components::TableCellPosition { row: 1, column: 1 })
            })
            .expect("夹具里该有那个空格子");
        let caret = editor.caret_source_offset(empty_cell.cell.entity_id(), 0, cx);
        (ranges, caret, column_bounds)
    });

    let missing = ranges
        .iter()
        .filter(|(_, range)| range.is_none())
        .map(|(position, _)| format!("第 {} 行第 {} 列", position.0 + 1, position.1 + 1))
        .collect::<Vec<_>>();
    assert!(
        missing.is_empty(),
        "这些格子说不出自己在文件里的区间：{missing:?}（空格子序列化出空串，按文字搜就搜不到）"
    );
    let (second_pipe, row_end) = column_bounds;
    let empty_range = ranges
        .iter()
        .find(|(position, _)| *position == (1, 1))
        .and_then(|(_, range)| range.clone())
        .expect("上一条断言已保证有映射");
    assert!(
        empty_range.is_empty() && empty_range.start > second_pipe && empty_range.start <= row_end,
        "空格子的映射区间没落在它自己那一列里：{empty_range:?}（列在 {second_pipe}..{row_end}）"
    );
    assert_eq!(
        caret,
        Some(empty_range.start),
        "光标在空格子里时算不出缓冲区偏移（粘贴、跳转、行列号都靠这个偏移）"
    );
}

/// 容器里的表格（挂在引用块下）按结构量格子时，容器记号不算第 0 列。
///
/// 引用块里的行写着 `> | 甲 | 乙 |`：`table_cell_source_range` 那条尺是从第一个管道符
/// 起算的，量之前得先把 `>` 与缩进让开，否则整表往右错一格，空格子的区间还会落在记号上。
#[gpui::test]
async fn a_quote_table_maps_columns_after_its_container_marker(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    const FIXTURE: &str = "> | 名称   | 数量 |\n> |:-------|-----:|\n> | 苹果   |      |\n";
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, FIXTURE.to_string(), None));
    redraw(cx);

    let (cells, row_start, second_pipe, source) = editor.read_with(cx, |editor, cx| {
        let source = editor.buffer.text();
        let row_start = source.find("> | 苹果").expect("夹具里应有这一行");
        let row_line = source[row_start..].split('\n').next().expect("行应有内容");
        let second_pipe = row_start
            + row_line
                .match_indices('|')
                .nth(1)
                .expect("这一行该有两个管道符")
                .0;

        let mut cells = editor
            .table_cells
            .values()
            .map(|binding| {
                let position = binding
                    .cell
                    .read_with(cx, |block, _cx| block.table_cell_position())
                    .expect("格子绑定应有位置");
                (
                    (position.row, position.column),
                    binding
                        .cell
                        .read_with(cx, |block, _cx| block.display_text().to_string()),
                    editor
                        .source_mapping_for_entity(binding.cell.entity_id(), cx)
                        .map(|mapping| mapping.full_source_range),
                )
            })
            .collect::<Vec<_>>();
        cells.sort_by_key(|(position, _, _)| *position);
        (cells, row_start, second_pipe, source)
    });

    // 空格子的原文区间是零宽，位置在它自己那一列里（第二个管道符之后）。
    let expected: [((usize, usize), &str, &str); 4] = [
        ((0, 0), "名称", "名称"),
        ((0, 1), "数量", "数量"),
        ((1, 0), "苹果", "苹果"),
        ((1, 1), "", ""),
    ];
    assert_eq!(cells.len(), expected.len(), "夹具里的格子数变了：对照要跟着改");
    for ((position, text, range), ((want_row, want_column), want_text, want_slice)) in
        cells.iter().zip(expected.iter())
    {
        assert_eq!(*position, (*want_row, *want_column));
        assert_eq!(text, want_text);
        let label = format!("第 {} 行第 {} 列（{text:?}）", want_row + 1, want_column + 1);
        let range = range.clone().unwrap_or_else(|| panic!("{label} 没有映射"));
        assert_eq!(
            &source[range.clone()],
            *want_slice,
            "{label} 量到的原文不是它自己那格的字节（容器记号 `> ` 被当成了第 0 列？{range:?}）"
        );
        if *position == (1, 1) {
            assert!(
                range.start > second_pipe && range.start <= row_start + 21,
                "{label} 没落在它自己那一列里：{range:?}（该在 {second_pipe}..{} 之间）",
                row_start + 21
            );
        }
    }
}

/// 表格的格子按「哪一张表、第几行、第几列」报出它在文件里的区间与光标偏移。
///
/// 表与表的顺序按块树里的顺序（与映射无关），所以两张表头一模一样的表也不会混在一起。
fn nested_table_cells_by_table(
    editor: &gpui::Entity<Editor>,
    cx: &mut gpui::VisualTestContext,
) -> Vec<Vec<((usize, usize), Option<std::ops::Range<usize>>, Option<usize>)>> {
    editor.read_with(cx, |editor, cx| {
        let tables = editor
            .document
            .visible_blocks()
            .into_iter()
            .filter(|visible| visible.entity.read(cx).kind() == BlockKind::Table)
            .map(|visible| visible.entity.entity_id())
            .collect::<Vec<_>>();
        let mut per_table = vec![Vec::new(); tables.len()];
        for binding in editor.table_cells.values() {
            let Some(index) = tables
                .iter()
                .position(|id| *id == binding.table_block.entity_id())
            else {
                continue;
            };
            let position = binding
                .cell
                .read_with(cx, |block, _cx| block.table_cell_position())
                .expect("格子绑定应有位置");
            let range = editor
                .source_mapping_for_entity(binding.cell.entity_id(), cx)
                .map(|mapping| mapping.full_source_range);
            let caret = editor.caret_source_offset(binding.cell.entity_id(), 0, cx);
            per_table[index].push(((position.row, position.column), range, caret));
        }
        for cells in &mut per_table {
            cells.sort_by_key(|(position, _, _)| *position);
        }
        per_table
    })
}

/// 一格的映射落在缓冲区第几行（缺映射报 `None`）。
fn nested_table_cell_lines(
    cells: &[((usize, usize), Option<std::ops::Range<usize>>, Option<usize>)],
    source: &str,
) -> Vec<Option<usize>> {
    cells
        .iter()
        .map(|(_, range, _)| {
            range
                .as_ref()
                .map(|range| source[..range.start].matches('\n').count())
        })
        .collect()
}

/// 容器里的表格**表头写着转义管道符**时，每一格仍要说得出它在文件里的哪几个字节。
///
/// 这张表在哪里、有多大，读侧原来是靠「拿表头序列化出来的文字回原文里搜同一行」找的。
/// 搜的口径与量格子的口径不是同一把尺：`a\|b` 里的 `\|` 是格子里的内容，旧口径按裸 `|`
/// 切列，切出 3 段对不上模型的 2 格，于是**整张表一个映射都没有**——光标停在格子里算不出
/// 缓冲区偏移，粘贴、跳转、状态栏的行列号只能退回默认位置。
#[gpui::test]
async fn a_quote_table_with_an_escaped_pipe_in_its_header_still_maps_every_cell(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);

    const FIXTURE: &str = "> | a\\|b | 数量 |\n> | --- | --- |\n> | 1 | 2 |\n";
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, FIXTURE.to_string(), None));
    redraw(cx);

    let tables = nested_table_cells_by_table(&editor, cx);
    let source = editor.read_with(cx, |editor, _cx| editor.buffer.text());
    assert_eq!(tables.len(), 1, "夹具里该有一张表");
    let cells = &tables[0];
    assert_eq!(cells.len(), 4, "夹具的格子数变了：对照要跟着改");

    // 期望的是**文件里的那几位字节**：转义的那一格连反斜杠一起算，不是模型序列化出来的文字。
    let expected: [((usize, usize), &str); 4] = [
        ((0, 0), "a\\|b"),
        ((0, 1), "数量"),
        ((1, 0), "1"),
        ((1, 1), "2"),
    ];
    for index in 0..expected.len() {
        let (position, want_slice) = &expected[index];
        let (cell, range, caret) = &cells[index];
        let label = format!("第 {} 行第 {} 列", cell.0 + 1, cell.1 + 1);
        assert_eq!(cell, position, "{label} 的位置对不上");
        let range = range.clone().unwrap_or_else(|| panic!("{label} 没有映射"));
        assert_eq!(
            &source[range.clone()],
            *want_slice,
            "{label} 量到的原文不是它自己那格的字节"
        );
        assert_eq!(
            caret,
            &Some(range.start),
            "{label} 光标在格子开头却算不出缓冲区偏移"
        );
    }
}

/// 同一个引用块里有两张表头一模一样的表：每一张的格子落在**自己**那几行里。
///
/// 旧读侧在整根块的原文里搜表头，搜到两处再靠「离走树算出的起点最近」猜哪张是本表；
/// 猜错就是两张表的格子互换字节。现在起点是走树带下来的事实，核对只看结构
/// （格数、定界行、行数），不再有两处可选。
#[gpui::test]
async fn two_quote_tables_with_the_same_header_map_to_their_own_rows(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    const FIXTURE: &str = "> | 甲 | 乙 |\n> | --- | --- |\n> | 1 | 2 |\n>\n\
        > | 甲 | 乙 |\n> | --- | --- |\n> | 3 | 4 |\n";
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, FIXTURE.to_string(), None));
    redraw(cx);

    let tables = nested_table_cells_by_table(&editor, cx);
    let source = editor.read_with(cx, |editor, _cx| editor.buffer.text());
    assert_eq!(tables.len(), 2, "夹具里该有两张表");

    let missing = tables
        .iter()
        .flatten()
        .filter(|(_, range, _)| range.is_none())
        .count();
    assert_eq!(missing, 0, "有格子说不出自己在文件里的区间：{tables:?}");

    // 第一张表占文件的第 0、2 行，第二张占第 4、6 行（表头行 + 数据行）。
    assert_eq!(
        nested_table_cell_lines(&tables[0], &source),
        vec![Some(0), Some(0), Some(2), Some(2)],
        "第一张表的格子没落在它自己那两行里"
    );
    assert_eq!(
        nested_table_cell_lines(&tables[1], &source),
        vec![Some(4), Some(4), Some(6), Some(6)],
        "第二张表的格子被算到别处去了"
    );
    assert_eq!(&source[tables[0][2].1.clone().expect("有映射")], "1");
    assert_eq!(&source[tables[1][2].1.clone().expect("有映射")], "3");
}

/// 表前面还挂着别的子块（列表、代码围栏）时，表的落点要跟着走树算出的偏移走。
///
/// 这张表的位置是「前面每一块在文件里占了多少字节」累加出来的，累加口径一旦按模型拼
/// （列表两个空格、围栏补 phantom 行），表就往旁边漂，格子量到邻居的字节。
#[gpui::test]
async fn a_quote_table_below_a_list_and_a_fence_maps_to_the_rows_it_is_on(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);

    const FIXTURE: &str = "> - 项甲\n>\n> ```\n> code\n> ```\n>\n\
        > | 甲 | 乙 |\n> | --- | --- |\n> | 1 | 2 |\n";
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, FIXTURE.to_string(), None));
    redraw(cx);

    let tables = nested_table_cells_by_table(&editor, cx);
    let source = editor.read_with(cx, |editor, _cx| editor.buffer.text());
    assert_eq!(tables.len(), 1, "夹具里该有一张表");
    let lines = nested_table_cell_lines(&tables[0], &source);
    assert_eq!(
        lines,
        vec![Some(6), Some(6), Some(8), Some(8)],
        "表前面的列表与围栏把落点带偏了：{lines:?}\n缓冲区是 {source:?}"
    );
    let wanted = ["甲", "乙", "1", "2"];
    for (index, (position, range, _)) in tables[0].iter().enumerate() {
        let range = range.clone().unwrap_or_else(|| panic!("{:?} 没有映射", position));
        assert_eq!(
            &source[range],
            wanted[index],
            "第 {} 行第 {} 列量到的原文不是它自己那格的字节",
            position.0 + 1,
            position.1 + 1
        );
    }
}

/// 在表**上面**打字之后，容器里那张表的格子还指着它那几行。
///
/// 走树算出的起点是唯一的落点来源，所以同块里前面那一段一变色（引用行重写、字节平移），
/// 累加就得跟着准。这里在段落里打一个字，再逐格核对区间仍在表自己的行里、字节仍是那一格。
#[gpui::test]
async fn typing_above_a_nested_table_keeps_its_cells_on_their_own_rows(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    const FIXTURE: &str = "> 前言\n>\n> | 甲 | 乙 |\n> | --- | --- |\n> | 1 | 2 |\n";
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, FIXTURE.to_string(), None));
    redraw(cx);

    let intro = editor.read_with(cx, |editor, cx| {
        editor
            .document
            .visible_blocks()
            .into_iter()
            .find(|visible| visible.entity.read(cx).display_text() == "前言")
            .map(|visible| visible.entity.clone())
            .expect("夹具里该有那段话")
    });
    cx.update(|_window, cx| {
        editor.update(cx, |editor, _cx| editor.focus_block(intro.entity_id()));
        intro.update(cx, |block, block_cx| block.move_to(0, block_cx));
    });
    redraw(cx);
    cx.simulate_input("写");
    redraw(cx);

    let tables = nested_table_cells_by_table(&editor, cx);
    let source = editor.read_with(cx, |editor, _cx| editor.buffer.text());
    assert_eq!(
        source, "> 写前言\n>\n> | 甲 | 乙 |\n> | --- | --- |\n> | 1 | 2 |\n",
        "打字这一步把别的字节也改了：{source:?}"
    );
    assert_eq!(tables.len(), 1, "夹具里该有一张表");
    let lines = nested_table_cell_lines(&tables[0], &source);
    assert_eq!(
        lines,
        vec![Some(2), Some(2), Some(4), Some(4)],
        "上面那一段变长之后，表的格子没跟着平移到自己那两行：{lines:?}"
    );
    let wanted = ["甲", "乙", "1", "2"];
    for (index, (position, range, _)) in tables[0].iter().enumerate() {
        let range = range.clone().unwrap_or_else(|| panic!("{:?} 没有映射", position));
        assert_eq!(
            &source[range],
            wanted[index],
            "第 {} 行第 {} 列量到的原文不是它自己那格的字节",
            position.0 + 1,
            position.1 + 1
        );
    }
}
/// 拆一个 `1)` 的列表项，两个半截都还得写 `1)`/`2)`，不许变成 `1.`。
///
/// 用户在原文里用的是圆括号，编辑器却把项当成「只有序号、没有写法」的东西：显示按
/// 规范补点号，序列化也按规范补点号，于是回车一分为二的那一刻，用户自己写的记号
/// 被换掉了（用户报修「为啥 1) 还会变成 1. 啊！！」）。
#[gpui::test]
async fn splitting_a_paren_numbered_item_keeps_the_paren_marker(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "1) alpha one\n2) beta two\n".to_string(), None)
    });
    redraw(cx);
    editor.update(cx, |editor, cx| {
        let item = editor.document.visible_blocks()[0].entity.clone();
        item.update(cx, |block, _cx| block.selected_range = 4..4);
        editor.pending_focus = Some(item.entity_id());
    });
    redraw(cx);
    cx.dispatch_action(Newline);
    redraw(cx);

    let buffer_text = editor.read_with(cx, |editor, _cx| editor.buffer.text());
    assert_eq!(
        buffer_text, "1) alph\n2) a one\n2) beta two\n",
        "拆项把用户写的圆括号记号换成了点号"
    );
}

/// 拆一个 `+` 的无序项，两个半截都还得写 `+`，不许被规范成 `-`。
#[gpui::test]
async fn splitting_a_plus_bulleted_item_keeps_the_plus_marker(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "+ alpha one\n+ beta two\n".to_string(), None)
    });
    redraw(cx);
    editor.update(cx, |editor, cx| {
        let item = editor.document.visible_blocks()[0].entity.clone();
        item.update(cx, |block, _cx| block.selected_range = 4..4);
        editor.pending_focus = Some(item.entity_id());
    });
    redraw(cx);
    cx.dispatch_action(Newline);
    redraw(cx);

    let buffer_text = editor.read_with(cx, |editor, _cx| editor.buffer.text());
    assert_eq!(
        buffer_text, "+ alph\n+ a one\n+ beta two\n",
        "拆项把用户写的加号记号换成了减号"
    );
}

/// 在带下划线强调的段落里打字（这一步会走整块落笔），同块没改过的下划线写法要原样留着。
///
/// 整块落笔拿的是块的序列化结果。序列化过去一律把强调写成星号，于是打一个字就把用户
/// 的 `__下划线__` 改成 `**下划线**`——现在写法记在样式里，落笔出去还是下划线。
#[gpui::test]
async fn typing_inside_an_underscore_emphasis_paragraph_keeps_the_underscores(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "强调 __下划线__ 尾巴\n".to_string(), None)
    });
    redraw(cx);
    editor.update(cx, |editor, cx| {
        let block = editor.document.visible_blocks()[0].entity.clone();
        let end = block.read(cx).record.title.visible_len();
        block.update(cx, |block, _cx| block.selected_range = end..end);
        editor.pending_focus = Some(block.entity_id());
    });
    redraw(cx);

    cx.simulate_input("*");
    redraw(cx);

    let buffer_text = editor.read_with(cx, |editor, _cx| editor.buffer.text());
    assert!(
        buffer_text.contains("__下划线__"),
        "整块落笔把下划线强调洗成了星号：{buffer_text:?}"
    );
}

/// 编码与 BOM 这一维的保真用例（阶段 3 的「编码/EOL 全矩阵」）。
///
/// 上面那张表都走 UTF-8；这一张走真实字节：中文 Windows 常见的 GB18030、
/// UTF-8 BOM、以及两者与 CRLF 的组合。形状记在 `FileShape` 里，保存按同一形状
/// 重新编码，所以这些用例验的还是同一件事：没改过的字节必须原样回去。
fn encoding_matrix_cases() -> Vec<(&'static str, Vec<u8>)> {
    let gb = |text: &str| encoding_rs::GB18030.encode(text).0.into_owned();
    let utf8_bom = |text: &str| {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(text.as_bytes());
        bytes
    };
    vec![
        ("UTF-8 BOM", utf8_bom("# 标题\n\n正文\n")),
        ("UTF-8 BOM 与 CRLF 表格填充", utf8_bom(
            "# 标题\r\n\r\n| 名称   | 数量 |\r\n|:-------|-----:|\r\n| 苹果   |    3 |\r\n",
        )),
        ("GB18030 简体中文", gb("# 会议记录\n\n中文正文与 English\n")),
        ("GB18030 与 CRLF", gb("# 会议记录\r\n\r\n中文正文\r\n第二行\r\n")),
        ("GB18030 无末行换行", gb("# 会议记录\n\n中文正文没有末行换行")),
        ("GB18030 与括号序号列表", gb("1) 第一项\n2) 第二项\n")),
    ]
}

/// 打开 → 不编辑 → 保存：编码、BOM、行尾一个字节都不许多、也不许少。
#[gpui::test]
async fn opening_then_saving_preserves_encoding_and_bom_byte_for_byte(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);

    let cases = encoding_matrix_cases();
    let total = cases.len();
    let mut failures: Vec<String> = Vec::new();
    for (name, source) in cases {
        let path = temp_markdown_path(name);
        fs::write(&path, &source).expect("write fixture");
        let cleanup = path.clone();
        cx.on_quit(move || {
            let _ = fs::remove_file(&cleanup);
        });
        let document = encoding::load_document(&path).expect("read fixture");
        let (editor, cx) = cx.add_window_view({
            let path = path.clone();
            move |_window, cx| Editor::from_loaded_document(cx, document, Some(path))
        });
        let dirty_on_open = editor.read_with(cx, |editor, _cx| editor.document_dirty);
        cx.simulate_keystrokes("ctrl-s");
        redraw(cx);

        let saved = fs::read(&path).expect("read saved file");
        let report = describe_case(name, &source, &saved, dirty_on_open);
        if !report.is_empty() {
            failures.push(report);
        }
    }

    assert!(
        failures.is_empty(),
        "打开→不编辑→保存 改写了编码或 BOM，{} / {} 个用例失败：\n{}",
        failures.len(),
        total,
        failures.join("\n")
    );
}

/// 编辑后保存：插进去的那个 ASCII 字符以外，编码字节（含 GB18030 的双字节/四字节
/// 序列）必须逐字节留在原位。
#[gpui::test]
async fn editing_then_saving_keeps_the_encoding_and_every_other_byte(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);

    let mut failures: Vec<String> = Vec::new();
    for (name, source) in encoding_matrix_cases() {
        let path = temp_markdown_path(name);
        fs::write(&path, &source).expect("write fixture");
        let cleanup = path.clone();
        cx.on_quit(move || {
            let _ = fs::remove_file(&cleanup);
        });
        let document = encoding::load_document(&path).expect("read fixture");
        let (editor, cx) = cx.add_window_view({
            let path = path.clone();
            move |_window, cx| Editor::from_loaded_document(cx, document, Some(path))
        });
        redraw(cx);
        let Some(first) = editor.read_with(cx, |editor, _cx| {
            editor.document.visible_blocks().first().map(|visible| visible.entity.clone())
        }) else {
            failures.push(format!("  [{name}] 打开后一个可见块都没有"));
            continue;
        };
        cx.update(|_window, cx| {
            first.update(cx, |block, _cx| block.selected_range = 0..0);
        });
        cx.simulate_input("X");
        redraw(cx);
        cx.simulate_keystrokes("ctrl-s");
        redraw(cx);

        let saved = fs::read(&path).expect("read saved file");
        if let Some(report) = describe_insertion_case(name, &source, &saved) {
            failures.push(report);
        }
    }

    assert!(
        failures.is_empty(),
        "打开→插一个字符→保存 改动了编码里不该动的字节，{} 个用例失败：\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// 一个文件里同时混着 CRLF 和 LF 时的行为，钉在两段上：
///
/// - 没编辑过：保存走原字节回写（`TextBuffer::pristine`），混排一个字节都不动。
/// - 编辑过：必须重新编码，而 `FileShape` 记的是**整个文件**的行尾形状，混排没有
///   形状可记，于是按 LF 落地（见 `detect_line_ending`：宁可少还原一处，也不把
///   LF 行升格成 CRLF）。这不是待修的保真缺陷；混排文件要统一行尾，走显式的格式化命令。
#[gpui::test]
async fn mixed_line_endings_survive_a_save_and_normalize_to_lf_only_after_an_edit(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);

    let source = "第一行\r\n第二行\n第三行\r\n";
    let path = temp_markdown_path("混排行尾");
    fs::write(&path, source).expect("write fixture");
    let cleanup = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup);
    });
    let document = encoding::load_document(&path).expect("read fixture");
    let (editor, cx) = cx.add_window_view({
        let path = path.clone();
        move |_window, cx| Editor::from_loaded_document(cx, document, Some(path))
    });
    redraw(cx);

    cx.simulate_keystrokes("ctrl-s");
    redraw(cx);
    assert_eq!(
        fs::read(&path).expect("read saved file"),
        source.as_bytes(),
        "没编辑过的混排文件，保存不该动任何字节"
    );

    let first = editor.read_with(cx, |editor, _cx| {
        editor
            .document
            .visible_blocks()
            .first()
            .map(|visible| visible.entity.clone())
    });
    let Some(first) = first else {
        panic!("打开后一个可见块都没有");
    };
    cx.update(|_window, cx| {
        first.update(cx, |block, _cx| block.selected_range = 0..0);
    });
    cx.simulate_input("X");
    redraw(cx);
    cx.simulate_keystrokes("ctrl-s");
    redraw(cx);

    assert_eq!(
        fs::read_to_string(&path).expect("read saved file"),
        "X第一行\n第二行\n第三行\n",
        "编辑后的混排文件应整体按 LF 写回，且除插入处外文本不变"
    );
}

/// 投影不变式的粗筛：**打一个字只该动光标那一个字**。
///
/// `record.title == parse(buffer[span])` 这条不变式破了的时候，症状都是这个样子——
/// 插入点以外的可见文本变了（写法被读成语法：`\*` 成强调、`_x_` 成下划线），或者别的
/// 块的可见文本被顺手重新解释。字节层面的表盯着磁盘，这条盯着渲染：两边都过才算「打字
/// 没有重新解释用户没碰的那段」。
#[gpui::test]
async fn typing_one_char_only_changes_the_text_at_the_caret(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let mut failures: Vec<String> = Vec::new();
    for (name, source) in FIDELITY_CASES {
        let path = temp_markdown_path(name);
        fs::write(&path, source).expect("write fixture");
        let cleanup = path.clone();
        cx.on_quit(move || {
            let _ = fs::remove_file(&cleanup);
        });
        let document = encoding::load_document(&path).expect("read fixture");
        let (editor, cx) = cx.add_window_view({
            let path = path.clone();
            move |_window, cx| Editor::from_loaded_document(cx, document, Some(path))
        });
        redraw(cx);

        let visible_before = editor.read_with(cx, |editor, cx| {
            editor
                .document
                .visible_blocks()
                .iter()
                .map(|visible| block_text_snapshot(visible.entity.read(cx)))
                .collect::<Vec<_>>()
        });
        let Some(first) = editor.read_with(cx, |editor, _cx| {
            editor.document.visible_blocks().first().map(|visible| visible.entity.clone())
        }) else {
            continue;
        };
        cx.update(|_window, cx| {
            first.update(cx, |block, _cx| block.selected_range = 0..0);
        });
        cx.simulate_input("X");
        redraw(cx);

        let visible_after = editor.read_with(cx, |editor, cx| {
            editor
                .document
                .visible_blocks()
                .iter()
                .map(|visible| block_text_snapshot(visible.entity.read(cx)))
                .collect::<Vec<_>>()
        });
        if visible_after.len() != visible_before.len() {
            failures.push(format!(
                "  [{name}] 打一个字把块列表换了：{} 块 → {} 块",
                visible_before.len(),
                visible_after.len()
            ));
            continue;
        }
        for (index, (before, after)) in visible_before.iter().zip(&visible_after).enumerate() {
            let expected = if index == 0 {
                format!("X{before}")
            } else {
                before.clone()
            };
            if *after != expected {
                failures.push(format!(
                    "  [{name}] 第 {} 块的可见文本被重新解释了：{before:?} → {after:?}（应为 {expected:?}）",
                    index + 1
                ));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "打字不该重新解释光标以外的写法，{} 个形状失败：\n{}",
        failures.len(),
        failures.join("\n")
    );
}


/// 一块「用户看得见的文字」的快照。表格的文字不在标题树里，一格一格拼起来才是它的可见文本。
fn block_text_snapshot(block: &crate::editor::Block) -> String {
    let Some(table) = block.record.table.as_ref() else {
        return block.record.title.visible_text();
    };
    let cells = |rows: &[Vec<crate::components::InlineTextTree>]| -> String {
        rows.iter()
            .map(|row| {
                row.iter()
                    .map(|cell| cell.visible_text())
                    .collect::<Vec<_>>()
                    .join("\u{1}")
            })
            .collect::<Vec<_>>()
            .join("\u{2}")
    };
    format!("{}\u{2}{}", cells(&[table.header.clone()]), cells(&table.rows))
}

/// 脚注引用旁边打字，序号要还贴在原地。
///
/// 编辑后的重解析按源码形状存片段（`[^1]`，序号留空），序号回填不能只等注册表换人——
/// 在段首打一个字根本不换注册表，于是渲染与可见长度都会差出一截。字节这边一直是好的
/// （缓冲区仍是 `[^1]`），坏的是看得见的形状，所以断言两条：可见文本仍是上标，字节仍只有 X。
#[gpui::test]
async fn typing_next_to_a_footnote_reference_keeps_the_ordinal_label(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    const SOURCE: &str = "有脚注[^1]。\n\n[^1]: 脚注内容\n";
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, SOURCE.to_string(), None));
    redraw(cx);
    let first = editor.read_with(cx, |editor, _cx| {
        editor.document.visible_blocks().first().map(|visible| visible.entity.clone())
    }).expect("应有第一个块");
    assert_eq!(
        first.read_with(cx, |block, _cx| block.record.title.visible_text()),
        "有脚注\u{b9}。",
        "脚注引用现在的可见形状变了：这条测试的对照要跟着改"
    );

    cx.update(|_window, cx| {
        first.update(cx, |block, _cx| block.selected_range = 0..0);
    });
    cx.simulate_input("X");
    redraw(cx);

    let (after, cursor) = first.read_with(cx, |block, _cx| {
        (block.record.title.visible_text(), block.cursor_offset())
    });
    assert_eq!(
        after, "X有脚注\u{b9}。",
        "打字把脚注引用打回了源码形状，序号丢了"
    );
    assert_eq!(cursor, 1, "光标没停在刚打的那个字之后");
    assert_eq!(
        editor.read_with(cx, |editor, _cx| editor.buffer.text()),
        "X有脚注[^1]。\n\n[^1]: 脚注内容\n",
        "字节层面被改写了：那已经不是这条测试的范围"
    );
}

/// 删掉脚注引用前面的那个字：序号也要还在。
///
/// 序号回填不是只有打字会用到——删除同样走「按可见文本重解析」这条路，片段会退回
/// 源码形状（`[^1]`），差别只在光标位置不动。
#[gpui::test]
async fn deleting_a_char_next_to_a_footnote_reference_keeps_the_ordinal_label(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);

    const SOURCE: &str = "有脚注[^1]。\n\n[^1]: 脚注内容\n";
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, SOURCE.to_string(), None));
    redraw(cx);
    let first = editor.read_with(cx, |editor, _cx| {
        editor.document.visible_blocks().first().map(|visible| visible.entity.clone())
    }).expect("应有第一个块");
    // 「注」与序号之间：删掉的是「注」，脚注引用本身不动。
    cx.update(|_window, cx| {
        first.update(cx, |block, _cx| block.selected_range = 9..9);
    });
    cx.simulate_keystrokes("backspace");
    redraw(cx);

    let clean = first.read_with(cx, |block, _cx| block.record.title.visible_text());
    assert_eq!(
        clean, "有脚\u{b9}。",
        "删掉脚注引用旁边的字，序号该还贴在原地"
    );
    assert_eq!(
        editor.read_with(cx, |editor, _cx| editor.buffer.text()),
        "有脚[^1]。\n\n[^1]: 脚注内容\n",
        "字节层面被改写了：那已经不是这条测试的范围"
    );
}

/// 两个脚注引用之间打字：两个序号都得还在，而且各归各的号。
///
/// 回填是「片段按顺序对上注册表里这一块的 occurrences」，一次编辑里只要有一个对不上，
/// 后面的就整排退回源码形状。光标压着脚注原子时还多一重：显示空间会把光标碰到的原子
/// 摊成源码形状（`[^2]`），跟代码片段一个规矩，所以光标位置按干净空间核对。
#[gpui::test]
async fn typing_between_two_footnote_references_keeps_both_ordinals(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    const SOURCE: &str = "甲[^1]乙[^2]丙。\n\n[^1]: 一\n\n[^2]: 二\n";
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, SOURCE.to_string(), None));
    redraw(cx);
    let first = editor.read_with(cx, |editor, _cx| {
        editor.document.visible_blocks().first().map(|visible| visible.entity.clone())
    }).expect("应有第一个块");
    let caret = first.read_with(cx, |block, _cx| {
        block
            .display_text()
            .find('\u{b2}')
            .expect("第二个脚注引用的上标应在显示文本里")
    });

    cx.update(|_window, cx| {
        first.update(cx, |block, _cx| block.selected_range = caret..caret);
    });
    cx.simulate_input("X");
    redraw(cx);

    let (clean, cursor_clean) = first.read_with(cx, |block, _cx| {
        let cursor = block.current_to_clean_range(block.cursor_offset()..block.cursor_offset());
        (block.record.title.visible_text(), cursor.start)
    });
    assert_eq!(
        clean,
        "甲\u{b9}乙X\u{b2}丙。",
        "两个脚注引用之间打字，序号该原样留着"
    );
    assert_eq!(
        cursor_clean, 9,
        "光标没停在刚打的那个字之后：clean={clean:?} cursor_clean={cursor_clean}"
    );
    assert_eq!(
        editor.read_with(cx, |editor, _cx| editor.buffer.text()),
        "甲[^1]乙X[^2]丙。\n\n[^1]: 一\n\n[^2]: 二\n",
        "字节层面被改写了：那已经不是这条测试的范围"
    );
}
