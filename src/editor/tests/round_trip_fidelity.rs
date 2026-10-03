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
        // 「转义字符原样」由 `typing_next_to_literal_escapes_still_rewrites_the_block`
        // 单独盯着：那一条洗掉字节的原因不在写回层，是块自己的标题从可见文本重建时
        // 把字面 `*` 当成了强调定界符。
        if *name == "转义字符原样" {
            continue;
        }
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
        let (buffer_before, own_span, serializations_before) = editor.read_with(cx, |editor, cx| {
            let span = first.read(cx).record.source_span.clone().or_else(|| {
                editor
                    .document
                    .root_ancestor_of(first.entity_id())
                    .and_then(|root| root.read(cx).record.source_span.clone())
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

/// 已知缺陷（钉住现状，不是认可）：段首打字会把整段的字面转义洗掉。
///
/// 根因不在写回层：块在光标处插入文字时是从**可见文本**重建标题的，于是原本
/// 显示成字面星号的 `\*不强调\*` 被当成强调定界符（渲染也跟着变粗），可见长度
/// 缩短，编辑器只能退回整块重新序列化。缓冲区这边按区间写回已经能保住这些字节
/// ——修好上面那一步之后，这条应该并进 `saving_after_an_edit_at_the_start_...`。
#[gpui::test]
async fn typing_next_to_literal_escapes_still_rewrites_the_block(cx: &mut TestAppContext) {
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

    let buffer_text = editor.read_with(cx, |editor, _cx| editor.buffer.text());
    assert_eq!(
        buffer_text, "X字面星号 *不强调* 和字面下划线 _x_\n",
        "字面转义的处理变了：这条测试该并进逐字节保真那张表"
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
