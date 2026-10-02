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

    let text = encoding::read_document_string(&path).expect("decode fixture");
    let (editor, cx) = cx.add_window_view({
        let path = path.clone();
        move |_window, cx| Editor::from_file_source(cx, text, Some(path))
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
