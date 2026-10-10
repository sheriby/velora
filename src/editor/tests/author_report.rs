//! 原始复现素材的整篇回归：覆盖阅读、复制为 HTML 与编辑保存，避免片段单测
//! 通过时遗漏真实文档的入口和上下文。素材统一放在 `fixtures/regressions/`。

use std::path::{Path, PathBuf};

use super::common::*;
use crate::components::BlockKind;
use crate::theme::Theme;

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join("regressions")
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

#[gpui::test]
async fn reported_documents_copy_html_through_the_actual_shortcut(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    for name in [
        "01-utf16",
        "02-table-code",
        "02-table-math",
        "03-formula-tail",
        "04-math-blank-line",
        "04-math-closing",
        "05-quote-math",
        "06-indented-code",
        "06-link-url",
        "07-numeric-math",
        "08-utf8-bom",
        "09-code-inner-fence",
        "09-code-long-close",
        "10-escape",
        "10-line-break",
        "11-table-br",
        "12-images",
        "13-setext-heading",
        "14-heading-link",
        "14-toc",
        "15-callout",
    ] {
        let path = fixture_dir().join(format!("{name}.md"));
        let document = crate::editor::encoding::load_document(&path).expect("读取复现素材");
        let original = document.text.replace("\r\n", "\n");
        let (editor, cx) =
            cx.add_window_view(move |_, cx| Editor::from_loaded_document(cx, document, Some(path)));
        redraw(cx);
        redraw(cx);
        cx.simulate_keystrokes("ctrl-shift-c");
        let html = cx
            .update(|_, cx| cx.read_from_clipboard().and_then(|item| item.text()))
            .expect("实际复制命令应写入 HTML");
        let body = html.split_once("<body").expect("应有 HTML 正文").1;
        let expected = cx.update(|_, cx| {
            crate::export::html::render_html_with_base_dir(
                &original,
                cx.global::<ThemeManager>().current(),
                name,
                Some(&fixture_dir()),
            )
        });
        assert_eq!(
            body,
            expected.split_once("<body").expect("应有 HTML 正文").1,
            "{name} 从实际复制入口输出的内容应与原始文档一致"
        );
        editor.read_with(cx, |editor, _| {
            assert_eq!(editor.buffer.text(), original, "复制不应修改 {name}");
        });
    }
}

#[gpui::test]
async fn item02_and_item11_table_reading_uses_the_preserved_cell_content(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    for name in ["02-table-code", "02-table-math", "11-table-br"] {
        let (editor, cx) =
            cx.add_window_view(|_, cx| Editor::from_markdown(cx, case_text(name), None));
        redraw(cx);
        redraw(cx);
        editor.read_with(cx, |editor, cx| {
            let tables = editor
                .document
                .visible_blocks()
                .iter()
                .filter_map(|visible| visible.entity.read(cx).table_runtime.clone())
                .collect::<Vec<_>>();
            assert!(!tables.is_empty(), "{name} 应有表格运行时");
            match name {
                "02-table-code" => {
                    let table = &tables[0];
                    for (row, column, expected) in [
                        (0, 0, r"a\\b"),
                        (0, 1, r"a\|b"),
                        (1, 0, r"C:\tmp"),
                        (1, 1, r"x\|y"),
                    ] {
                        let cell = table.rows[row][column].read(cx);
                        assert_eq!(cell.display_text(), expected);
                        assert!(cell.inline_spans().iter().all(|span| span.style.code));
                    }
                }
                "02-table-math" => {
                    for (table, column, body) in [
                        (0, 0, r"\begin{matrix}1&2\\3&4\end{matrix}"),
                        (1, 0, r"a\|b"),
                        (1, 1, r"a\Vert b"),
                    ] {
                        let cell = tables[table].rows[0][column].read(cx);
                        let math = cell
                            .inline_spans()
                            .iter()
                            .find_map(|span| span.math.as_ref())
                            .expect("表内公式应进入公式渲染分支");
                        assert_eq!(math.body, body, "表内公式反斜杠必须完整");
                        assert!(
                            crate::components::latex::render_inline_math_svg(
                                &math.body,
                                gpui::black(),
                                20.0,
                            )
                            .is_ok(),
                            "{body} 应生成 SVG，不能退回源码"
                        );
                    }
                }
                "11-table-br" => {
                    let cell = tables[0].rows[0][1].read(cx);
                    assert_eq!(cell.display_text(), "first\nsecond");
                    assert_eq!(
                        cell.last_layout.as_ref().expect("单元格应完成布局").len(),
                        2
                    );
                    assert!(
                        cell.inline_spans()
                            .iter()
                            .all(|span| { span.link.is_none() && !span.style.underline })
                    );
                }
                _ => unreachable!(),
            }
        });
    }
}

#[gpui::test]
async fn item07_numeric_math_has_a_renderable_formula_span(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, case_text("07-numeric-math"), None));
    redraw(cx);
    redraw(cx);
    editor.read_with(cx, |editor, cx| {
        let math = editor
            .document
            .visible_blocks()
            .iter()
            .find_map(|visible| {
                visible
                    .entity
                    .read(cx)
                    .inline_spans()
                    .iter()
                    .find_map(|span| span.math.clone())
            })
            .expect("明确的数字公式必须有公式跨度");
        assert_eq!(math.body, "42");
        assert!(
            crate::components::latex::render_inline_math_svg(&math.body, gpui::black(), 20.0,)
                .is_ok()
        );
    });
}

#[gpui::test]
async fn item12_all_image_spellings_load_the_actual_local_files(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let path = fixture_dir().join("12-images.md");
    let document = crate::editor::encoding::load_document(&path).expect("读取图片用例");
    let (editor, cx) =
        cx.add_window_view(move |_, cx| Editor::from_loaded_document(cx, document, Some(path)));
    redraw(cx);
    redraw(cx);
    editor.read_with(cx, |editor, cx| {
        let sources = editor
            .document
            .visible_blocks()
            .iter()
            .filter_map(|visible| {
                visible
                    .entity
                    .read(cx)
                    .image_runtime()
                    .map(|runtime| runtime.resolved_source.clone())
            })
            .collect::<Vec<_>>();
        assert_eq!(sources.len(), 3, "三种写法都应安装图片运行时");
        for (source, name) in sources
            .iter()
            .zip(["orange.png", "blue card.png", "blue card.png"])
        {
            let expected = fixture_dir().join("assets").join(name);
            assert_eq!(source, &ImageResolvedSource::Local(expected.clone()));
            assert!(expected.is_file(), "图片路径必须落到真实文件");
        }
    });
    assert!(cx.debug_bounds("image-content").is_some(), "应挂载图片元素");
    assert!(
        cx.debug_bounds("image-placeholder").is_none(),
        "不能落入图片加载失败占位"
    );
}

#[gpui::test]
async fn item01_and_item08_original_encoded_files_survive_an_edit_and_save(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    for (name, use_crlf) in [
        ("01-utf16", false),
        ("08-utf8-bom", false),
        ("01-utf16", true),
        ("08-utf8-bom", true),
    ] {
        let original = fs::read(fixture_dir().join(format!("{name}.md"))).expect("读取原始字节");
        let (mut shape, text) = crate::editor::buffer::FileShape::detect_and_decode(&original);
        let line_ending: &[u8] = if use_crlf { b"\r\n" } else { b"\n" };
        shape.line_ending = crate::editor::buffer::FileShape::detect(line_ending).line_ending;
        let original = shape.encode(&text);
        let path = temp_markdown_path(name);
        fs::write(&path, &original).expect("写入素材副本");
        let document = crate::editor::encoding::load_document(&path).expect("解码原始素材");
        let disk_source = document.text.clone();
        let cleanup = path.clone();
        cx.on_quit(move || fs::remove_file(&cleanup).expect("清理素材副本"));
        let (editor, cx) = cx.add_window_view({
            let path = path.clone();
            move |_, cx| Editor::from_loaded_document(cx, document, Some(path))
        });
        redraw(cx);
        redraw(cx);
        editor.update(cx, |editor, cx| {
            let last = editor
                .document
                .visible_blocks()
                .iter()
                .rev()
                .find(|visible| {
                    let block = visible.entity.read(cx);
                    block.kind() == BlockKind::Paragraph && !block.display_text().is_empty()
                })
                .expect("应有正文末段")
                .entity
                .clone();
            let end = last.read(cx).visible_len();
            editor.focus_block(last.entity_id());
            last.update(cx, |block, cx| block.move_to(end, cx));
            cx.notify();
        });
        redraw(cx);
        let (caret, source) = editor.read_with(cx, |editor, cx| {
            let active = editor
                .document
                .visible_blocks()
                .iter()
                .find(|visible| Some(visible.entity.entity_id()) == editor.active_entity_id)
                .expect("应有焦点块");
            let block = active.entity.read(cx);
            let caret = editor
                .caret_source_offset(active.entity.entity_id(), block.selected_range.end, cx)
                .expect("光标应有源码偏移");
            (caret, editor.buffer.text())
        });
        // 光标属于 LF 缓冲区；磁盘字节仍保留 CRLF，两套长度不能直接比较。
        assert_eq!(
            caret,
            source.trim_end_matches('\n').len(),
            "应在正文末尾输入：{name}, CRLF={use_crlf}"
        );
        let disk_prefix = disk_source.trim_end_matches(['\r', '\n']);
        let (byte_offset, inserted) = if name == "01-utf16" {
            (2 + disk_prefix.encode_utf16().count() * 2, vec![b'Z', 0])
        } else {
            (3 + disk_prefix.len(), vec![b'Z'])
        };
        let mut expected = original.clone();
        expected.splice(byte_offset..byte_offset, inserted);
        cx.simulate_input("Z");
        redraw(cx);
        cx.simulate_keystrokes("ctrl-s");
        redraw(cx);
        assert_eq!(
            fs::read(&path).expect("读取保存结果"),
            expected,
            "{name} 插入 Z 后不能改掉 BOM、编码或其他字节"
        );
    }
}

#[gpui::test]
async fn item03_applying_formula_edits_preserves_tail_text_and_neighboring_formulas(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    for source in [
        fs::read_to_string(fixture_dir().join("03-formula-tail.md")).expect("读取原始用例"),
        "# 相邻公式\n\n$$x^2$$ $$y^2$$ 中文尾文🌿\n".to_string(),
        "# 多行公式\n\n$$\nx^2\n$$ 中文尾文🌿\n".to_string(),
        "# 多行相邻公式\n\n$$\nx^2\n$$ $$y^2$$ 中文尾文🌿\n".to_string(),
    ] {
        for edited in [false, true] {
            let path = temp_markdown_path("formula-tail-apply");
            fs::write(&path, &source).expect("写入素材副本");
            let document = crate::editor::encoding::load_document(&path).expect("读取副本");
            let cleanup = path.clone();
            cx.on_quit(move || fs::remove_file(&cleanup).expect("清理素材副本"));
            let (editor, cx) = cx.add_window_view({
                let path = path.clone();
                move |_, cx| Editor::from_loaded_document(cx, document, Some(path))
            });
            redraw(cx);
            let math = editor.read_with(cx, |editor, cx| {
                editor
                    .document
                    .visible_blocks()
                    .iter()
                    .find(|visible| visible.entity.read(cx).kind() == BlockKind::MathBlock)
                    .expect("应有数学块")
                    .entity
                    .clone()
            });
            let undo_before = editor.read_with(cx, |editor, _| editor.undo_history.len());
            editor.update_in(cx, |editor, window, cx| {
                editor.open_formula_editor_for_block(math.entity_id(), window, cx);
                assert_eq!(
                    editor.formula_editor.as_ref().expect("应打开弹窗").draft,
                    "x^2"
                );
            });
            redraw(cx);
            editor.update_in(cx, |editor, window, cx| {
                if edited {
                    editor.replace_formula_draft(0..3, "z^2", None, false, cx);
                }
                editor.apply_formula_editor(window, cx);
            });
            redraw(cx);
            let expected = if edited {
                source.replacen("x^2", "z^2", 1)
            } else {
                source.clone()
            };
            editor.read_with(cx, |editor, _| {
                assert_eq!(
                    editor.buffer.text(),
                    expected.replace("\r\n", "\n"),
                    "公式弹窗只能修改目标公式，不能丢掉尾文和其他公式；edited={edited}"
                );
                assert_eq!(editor.undo_history.len(), undo_before + usize::from(edited));
            });
            cx.simulate_keystrokes("ctrl-shift-c");
            let html = cx
                .update(|_, cx| cx.read_from_clipboard().and_then(|item| item.text()))
                .expect("复制应得到 HTML");
            let body = html.split_once("<body").expect("应有正文").1;
            let tail = if source.contains("LOST_SENTINEL") {
                "LOST_SENTINEL"
            } else {
                "中文尾文🌿"
            };
            assert!(body.contains(tail), "编辑后的 HTML 不能丢尾文");
            assert_eq!(
                body.matches("<svg").count(),
                if source.contains("y^2") { 2 } else { 1 }
            );
            if edited {
                cx.simulate_keystrokes("ctrl-z");
                redraw(cx);
                editor.read_with(cx, |editor, _| {
                    assert_eq!(editor.buffer.text(), source.replace("\r\n", "\n"))
                });
                cx.simulate_keystrokes("ctrl-y");
                redraw(cx);
                editor.read_with(cx, |editor, _| {
                    assert_eq!(editor.buffer.text(), expected.replace("\r\n", "\n"))
                });
            }
            cx.simulate_keystrokes("ctrl-s");
            redraw(cx);
            assert_eq!(fs::read_to_string(&path).expect("读取保存结果"), expected);
        }
    }
}

#[gpui::test]
async fn formula_dialog_restores_document_keyboard_focus_after_apply_or_cancel(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    for apply in [true, false] {
        let (editor, cx) = cx
            .add_window_view(|_, cx| Editor::from_markdown(cx, case_text("03-formula-tail"), None));
        redraw(cx);
        let (original_focus, math) = editor.update_in(cx, |editor, window, cx| {
            let original_focus = editor.focused_edit_target_entity_id(window, cx);
            let math = editor
                .document
                .visible_blocks()
                .iter()
                .find(|visible| visible.entity.read(cx).kind() == BlockKind::MathBlock)
                .expect("应有公式")
                .entity
                .entity_id();
            (original_focus, math)
        });
        assert!(original_focus.is_some(), "正文初始应有键盘焦点");
        editor.update_in(cx, |editor, window, cx| {
            editor.open_formula_editor_for_block(math, window, cx);
        });
        redraw(cx);
        cx.simulate_keystrokes(if apply { "ctrl-enter" } else { "escape" });
        redraw(cx);
        editor.update_in(cx, |editor, window, cx| {
            assert!(editor.formula_editor.is_none(), "应关闭弹窗");
            assert_eq!(
                editor.focused_edit_target_entity_id(window, cx),
                original_focus,
                "应用或取消公式弹窗后，正文快捷键必须继续收到事件；apply={apply}"
            );
        });
        cx.simulate_keystrokes("ctrl-shift-c");
        let copied = cx.update(|_, cx| cx.read_from_clipboard().and_then(|item| item.text()));
        assert!(copied.is_some_and(|html| html.contains("LOST_SENTINEL")));
    }
}

#[test]
fn item03_multiline_closing_line_keeps_the_next_formula_separate() {
    let segments =
        crate::components::latex::parse_display_math_segments("$$\nx^2\n$$ $$y^2$$ 中文尾文🌿")
            .expect("应识别多行公式");
    let formulas = segments
        .iter()
        .filter_map(|segment| {
            if let crate::components::latex::DisplayMathSegment::Formula { body, .. } = segment {
                Some(body.as_str())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(
        formulas,
        vec!["x^2", "y^2"],
        "第一处关闭符之后不是首公式的正文"
    );
}

#[gpui::test]
fn item03_multiline_tail_does_not_absorb_the_following_paragraph(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let source = "# 标题\n\n$$\nx^2\n$$ 中文尾文🌿\n\n后续段落\n";
    let editor = cx.new(|cx| Editor::from_markdown(cx, source.into(), None));
    editor.read_with(cx, |editor, cx| {
        let kinds = editor
            .document
            .visible_blocks()
            .iter()
            .map(|visible| {
                let block = visible.entity.read(cx);
                (block.kind(), block.display_text().to_string())
            })
            .collect::<Vec<_>>();
        assert!(
            kinds.iter().any(|(kind, text)| {
                *kind == BlockKind::MathBlock && text.ends_with("$$ 中文尾文🌿")
            }),
            "关闭行有尾文仍必须结束公式区域：{kinds:?}"
        );
        assert!(
            kinds
                .iter()
                .any(|(kind, text)| { *kind == BlockKind::Paragraph && text == "后续段落" }),
            "后续段落不能被吞入公式区域：{kinds:?}"
        );
    });
}
