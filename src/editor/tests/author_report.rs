//! 作者反馈语料的整篇验收：`fixtures/author-report-2026-10/` 是报修方
//! 随报告提供的原始用例（cases/），这里按报告编号逐条锁住「阅读效果 + HTML 输出」
//! 两端。报告末尾要求的验收方式就是「用对应示例复查阅读效果和 HTML 输出」，
//! 片段级单测各自守着实现细节，但没人把作者的原文整篇跑一遍。
//!
//! 每条断言都对着报修里那句现象写：报修说什么，这里就断什么。

use std::path::{Path, PathBuf};

use super::common::*;
use crate::components::BlockKind;
use crate::theme::Theme;

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join("author-report-2026-10")
}

/// 读作者的原文。行尾统一成 LF：报修里一半用例是 CRLF 文件，
/// 形状（编码/BOM/行尾）由 `FileShape` 那一层负责，这里只关心内容。
fn case_text(name: &str) -> String {
    let raw = std::fs::read(fixture_dir().join(format!("{name}.md")))
        .unwrap_or_else(|err| panic!("读夹具 {name}.md 失败：{err}"));
    crate::editor::encoding::decode_document_bytes(raw).replace("\r\n", "\n")
}

/// 作者那份文件走真实导出后的**正文**（图片按用例目录解析，才能验内嵌）。
///
/// 只取 `<body>` 之后：导出把整张主题 CSS 内嵌在 `<head>` 里，样式表和它的注释
/// 里就有 `.vlt-inline-math`、`[TOC]` 这类字面量——拿整份文件做「不该出现」的
/// 断言，红点会落在样式表上，看着像缺陷其实是断言读错了地方。
fn case_body(name: &str) -> String {
    let dir = fixture_dir();
    let html = crate::export::html::render_html_with_base_dir(
        &case_text(name),
        &Theme::default_theme(),
        "报修语料",
        Some(&dir),
    );
    html.split_once("<body")
        .map(|(_, rest)| rest.to_string())
        .unwrap_or(html)
}

/// 报修第 1 条：UTF-16 文件必须解码成人看得懂的字，而不是替换符。
#[test]
fn item01_utf16_case_decodes_to_the_users_text() {
    let document = crate::editor::encoding::load_document(&fixture_dir().join("01-utf16.md"))
        .expect("UTF-16 用例应当能打开");
    assert!(
        !document.text.contains('\u{fffd}'),
        "正文里仍有替换符，说明没真解码：{:?}",
        &document.text[..document.text.len().min(60)]
    );
    assert!(document.text.contains("UTF16"), "actual: {}", document.text);
}

/// 报修第 2 条：表格里的反斜杠是内容，切分单元格时不许先吃掉。
#[gpui::test]
fn item02_table_backslashes_survive_import_and_serialize(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let source = case_text("02-table-code");
    let editor = cx.new(|cx| Editor::from_markdown(cx, source.clone(), None));

    editor.update(cx, |editor, cx| {
        let markdown = editor.document.markdown_text(cx);
        assert!(
            markdown.contains("a\\\\b"),
            "代码里的双反斜杠被折成一条：{markdown}"
        );
        assert!(markdown.contains("C:\\tmp"), "Windows 路径被改掉：{markdown}");
        assert!(
            markdown.contains("a\\|b"),
            "转义竖线丢了，一行会被拆成两列：{markdown}"
        );
        assert_eq!(
            markdown.matches('\\').count(),
            source.matches('\\').count(),
            "往返之后反斜杠的总数变了：{markdown}"
        );
    });
}

/// 报修第 3 条：闭合 `$$` 之后的文字在界面和 HTML 里都不许消失。
#[gpui::test]
fn item03_text_after_display_math_survives_both_surfaces(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let editor = cx.new(|cx| Editor::from_markdown(cx, case_text("03-formula-tail"), None));
    let math = editor.read_with(cx, |editor, cx| {
        editor
            .document
            .visible_blocks()
            .into_iter()
            .map(|visible| visible.entity.read(cx).display_text().to_string())
            .collect::<Vec<_>>()
            .join("\n")
    });
    assert!(math.contains("LOST_SENTINEL"), "界面上这段字没有落点：{math}");

    let html = case_body("03-formula-tail");
    assert!(html.contains("LOST_SENTINEL"), "导出的 HTML 丢了尾文：{html}");
    assert!(html.contains("<svg"), "公式没渲染成图形：{html}");
}

/// 报修第 4 条：块内有空行、闭合符贴在末行，两种写法都要导出成公式。
#[test]
fn item04_math_shapes_the_ui_accepts_also_export() {
    for name in ["04-math-blank-line", "04-math-closing"] {
        let html = case_body(name);
        assert!(html.contains("vlt-math"), "{name} 没导出成公式：{html}");
        assert!(!html.contains("$$"), "{name} 仍带着 $$ 原文：{html}");
    }
}

/// 报修第 5 条：引用里的公式也要渲染，且保住引用层级。
#[test]
fn item05_math_inside_blockquote_exports_as_math() {
    let html = case_body("05-quote-math");
    assert!(html.contains("<blockquote"), "引用层级没了：{html}");
    assert!(html.contains("vlt-math"), "引用里的公式退回源码：{html}");
    assert!(!html.contains("$$"), "还留着 $$ 原文：{html}");
}

/// 报修第 6 条：代码与链接地址里的 `$` 不参与公式替换。
#[test]
fn item06_dollars_in_code_and_link_destination_stay_literal() {
    let code = case_body("06-indented-code");
    assert!(code.contains("$x^2$"), "代码里的字面量被改写：{code}");
    assert!(
        !code.contains("vlt-inline-math"),
        "公式画进了代码里：{code}"
    );
    assert!(!code.contains("<svg"), "代码里漏进了 SVG：{code}");

    let link = case_body("06-link-url");
    assert!(
        link.contains("<a href=\"https://example.com/$x$\">"),
        "链接地址被动过，或者链接根本没生成：{link}"
    );
    assert!(
        !link.contains("vlt-inline-math"),
        "地址里的 `$` 当成了公式：{link}"
    );
}

/// 报修第 7 条：`\\(42\\)` 是明确的公式写法，不该按金额判据跳过。
#[gpui::test]
fn item07_numeric_paren_math_renders_in_the_ui(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let editor = cx.new(|cx| Editor::from_markdown(cx, case_text("07-numeric-math"), None));

    editor.update(cx, |editor, cx| {
        let paragraph = editor
            .document
            .visible_blocks()
            .into_iter()
            .map(|visible| {
                let block = visible.entity.read(cx);
                (block.kind(), block.display_text().to_string())
            })
            .find(|(kind, _)| *kind == BlockKind::Paragraph)
            .map(|(_, body)| body)
            .unwrap_or_default();
        assert!(paragraph.contains('4'), "正文里找不到那个数：{paragraph}");
        assert!(
            !paragraph.contains("(42)"),
            "定界符原文还留在屏幕上：{paragraph}"
        );
    });
}

/// 报修第 8 条：带 BOM 的文件，首行 `# Title` 仍是标题。
#[gpui::test]
fn item08_first_line_after_a_bom_is_still_a_heading(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let editor = cx.new(|cx| Editor::from_markdown(cx, case_text("08-utf8-bom"), None));

    editor.update(cx, |editor, cx| {
        let first = editor.document.visible_blocks()[0].entity.read(cx);
        assert_eq!(
            first.kind(),
            BlockKind::Heading { level: 1 },
            "首行不是标题"
        );
        assert!(!first.display_text().contains('#'), "画面上还留着 #");
    });
}

/// 报修第 9 条：两种合法的围栏写法都要显示成代码块，围栏行不许露出来。
#[gpui::test]
fn item09_valid_fences_render_as_code_blocks(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    for (name, sentinel) in [
        ("09-code-long-close", "CODE_SENTINEL"),
        ("09-code-inner-fence", "``` inside code"),
    ] {
        let editor = cx.new(|cx| Editor::from_markdown(cx, case_text(name), None));
        let rendered = editor.read_with(cx, |editor, cx| {
            editor
                .document
                .visible_blocks()
                .into_iter()
                .map(|visible| {
                    let block = visible.entity.read(cx);
                    (block.kind(), block.display_text().to_string())
                })
                .collect::<Vec<_>>()
        });
        let code = rendered
            .iter()
            .find(|(kind, _)| matches!(kind, BlockKind::CodeBlock { .. }))
            .unwrap_or_else(|| panic!("{name} 没有代码块：{rendered:?}"));
        assert!(
            code.1.contains(sentinel),
            "{name} 的代码内容缺了：{}",
            code.1
        );
        assert!(
            !rendered
                .iter()
                .any(|(kind, text)| *kind == BlockKind::Paragraph && text.contains(sentinel)),
            "{name} 的代码块掉回成了正文"
        );
    }
}

/// 报修第 10 条：标点转义与行尾反斜杠换行，屏幕上都不许多出反斜杠。
#[gpui::test]
fn item10_escapes_and_hard_breaks_render_like_the_export(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let escape = cx.new(|cx| Editor::from_markdown(cx, case_text("10-escape"), None));
    let text = escape.read_with(cx, |editor, cx| {
        editor
            .document
            .visible_blocks()
            .into_iter()
            .map(|visible| visible.entity.read(cx).display_text().to_string())
            .collect::<Vec<_>>()
            .join(" ")
    });
    assert!(text.contains("# literal hash"), "`\\#` 仍显示反斜杠：{text}");
    assert!(!text.contains("\\#"), "多余的 \\# 漏在屏幕上：{text}");

    let breaks = cx.new(|cx| Editor::from_markdown(cx, case_text("10-line-break"), None));
    let text = breaks.read_with(cx, |editor, cx| {
        editor
            .document
            .visible_blocks()
            .into_iter()
            .map(|visible| visible.entity.read(cx).display_text().to_string())
            .collect::<Vec<_>>()
            .join("\n")
    });
    let lines = text.lines().collect::<Vec<_>>();
    // 报修的是「行末反斜杠没有隐藏」。两空格那一条本来就工作：尾部空白在屏幕上
    // 看不见，所以断的是「各自成一行」，不是「行尾一个字符不多」。
    assert!(
        lines.iter().any(|line| line.trim_end() == "Hard one")
            && lines.iter().any(|line| line.trim_end() == "Hard two"),
        "两空格硬换行没分成两行：{text}"
    );
    assert!(
        lines.contains(&"Slash one") && lines.contains(&"Slash two"),
        "行尾反斜杠既没断行也没隐藏：{text}"
    );
    assert!(!text.contains("Slash one\\"), "行尾的反斜杠还留着：{text}");
}

/// 报修第 11 条：单元格里的 `<br>` 是换行，不是带下划线的 “br”。
#[gpui::test]
fn item11_table_br_is_a_break_and_stays_a_break(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let source = case_text("11-table-br");
    let editor = cx.new(|cx| Editor::from_markdown(cx, source.clone(), None));

    editor.update(cx, |editor, cx| {
        assert!(
            editor
                .document
                .visible_blocks()
                .into_iter()
                .any(|visible| visible.entity.read(cx).kind() == BlockKind::Table),
            "这张表没解析成表"
        );
        let markdown = editor.document.markdown_text(cx);
        assert!(
            markdown.contains("first<br>second"),
            "写法被改写：{markdown}"
        );
    });

    // 断行本身（格子里没有字面 `br`、也没有链接）由
    // `components::markdown::table::tests::table_cell_br_...` 在格子那一层守着。
    let html = case_body("11-table-br");
    assert!(html.contains("<br>"), "导出侧的 <br> 也丢了：{html}");
    assert!(!html.contains(">br<"), "标签名又被当成文字输出：{html}");
}

/// 报修第 12 条：空格、`%20`、尖括号三种写法都要真内嵌进单文件 HTML。
#[test]
fn item12_every_local_image_spelling_gets_inlined() {
    let html = case_body("12-images");
    let inlined = html.matches("data:image/").count();
    assert_eq!(
        inlined, 3,
        "三张本地图片应当都内嵌，实际 {inlined} 张：{html}"
    );
    assert!(
        !html.contains("assets/blue") && !html.contains("assets/orange"),
        "还有图片留在相对路径上，单独发这个 HTML 就缺图：{html}"
    );
}

/// 报修第 13 条：Setext 一/二级标题不许被段落内的高亮语法吃掉。
#[test]
fn item13_setext_headings_export_as_headings() {
    let html = case_body("13-setext-heading");
    assert!(
        html.contains(">Setext heading</h1>") && html.contains("id=\"setext-heading\""),
        "一级标题没生成：{html}"
    );
    assert!(html.contains(">Lower heading</h2>"), "二级标题没生成：{html}");
    assert!(
        !html.contains("<mark>"),
        "等号行被当成了高亮标记：{html}"
    );
    assert!(
        html.contains("<hr"),
        "正文里那条分隔线应当还在：{html}"
    );
}

/// 报修第 14 条：标题要有 id，目录要真能跳。
#[test]
fn item14_heading_ids_and_toc_work_in_the_exported_file() {
    let anchors = case_body("14-heading-link");
    assert!(
        anchors.contains("href=\"#target-section\"") && anchors.contains("id=\"target-section\""),
        "链接还在但目标没有 id，跳不动：{anchors}"
    );

    let toc = case_body("14-toc");
    assert!(!toc.contains("[TOC]"), "目录在 HTML 里还是这几个字：{toc}");
    assert!(
        toc.contains("#first-section"),
        "目录没指向正文标题：{toc}"
    );
}

/// 报修第 15 条：带自定义标题的提示框要保住类型与标题。
#[test]
fn item15_named_callout_keeps_its_type_and_title() {
    let html = case_body("15-callout");
    assert!(
        html.contains("markdown-alert-note"),
        "提示框退成了普通引用：{html}"
    );
    assert!(html.contains("Named note"), "自定义标题没了：{html}");
    assert!(
        !html.contains("[!NOTE]"),
        "把 `[!NOTE]` 原文露了出来：{html}"
    );
    assert!(
        html.contains("markdown-alert-warning"),
        "无标题的 WARNING 也要保住：{html}"
    );
}
